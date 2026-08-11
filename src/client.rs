use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::command::Invocation;
use crate::model::Size;
use crate::protocol::{
    read_server_message, write_client_message, write_request, ClientMessage, ServerMessage,
};
use crate::terminal::{self, RawTerminal};

pub(crate) fn run(invocation: Invocation) -> Result<(), String> {
    let socket = invocation.socket;
    let request = invocation.request;
    let attach = invocation.attach;
    let starts_server = matches!(request, crate::protocol::Request::NewSession { .. });
    let mut stream = connect_or_start(&socket, starts_server)?;
    write_request(&mut stream, &request).map_err(|error| error.to_string())?;
    let response = read_server_message(&mut stream).map_err(|error| error.to_string())?;
    let ServerMessage::Response { ok, body } = response else {
        return Err("server returned an invalid command response".to_owned());
    };
    if !ok {
        return Err(body);
    }
    if !attach
        && (!body.is_empty() || matches!(request, crate::protocol::Request::DisplayMessage { .. }))
    {
        println!("{body}");
    }
    if !attach {
        return Ok(());
    }

    let target = match &request {
        crate::protocol::Request::NewSession { .. } => Some(body),
        crate::protocol::Request::NewWindow { target, .. } => target.clone(),
        crate::protocol::Request::Attach { target, .. } => target.clone(),
        _ => None,
    };
    attach_loop(&socket, target)
}

fn connect_or_start(socket: &std::path::Path, start: bool) -> Result<UnixStream, String> {
    if let Ok(stream) = UnixStream::connect(socket) {
        return Ok(stream);
    }
    if !start {
        return Err(format!("no tm server at {}", socket.display()));
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut daemon = Command::new(executable);
    daemon
        .arg("--daemon")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // The daemon must outlive the short-lived client process and its shell
    // process group. It only speaks over the Unix socket, so unlike an
    // attached terminal client it is safe to create a new session here.
    unsafe {
        daemon.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    daemon
        .spawn()
        .map_err(|error| format!("start tm daemon: {error}"))?;
    for _ in 0..100 {
        if let Ok(stream) = UnixStream::connect(socket) {
            return Ok(stream);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(format!("tm daemon did not start at {}", socket.display()))
}

fn attach_loop(socket: &std::path::Path, target: Option<String>) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket).map_err(|error| error.to_string())?;
    let size = terminal::size();
    write_request(
        &mut stream,
        &crate::protocol::Request::Attach { target, size },
    )
    .map_err(|error| error.to_string())?;
    let response = read_server_message(&mut stream).map_err(|error| error.to_string())?;
    let ServerMessage::Response { ok, body } = response else {
        return Err("server returned an invalid attach response".to_owned());
    };
    if !ok {
        return Err(body);
    }

    let mut raw =
        RawTerminal::enter().map_err(|error| format!("enter raw terminal mode: {error}"))?;
    let alive = Arc::new(AtomicBool::new(true));
    let writer = Arc::new(Mutex::new(
        stream.try_clone().map_err(|error| error.to_string())?,
    ));
    let input_alive = Arc::clone(&alive);
    let input_writer = Arc::clone(&writer);
    thread::Builder::new()
        .name("tm-input".to_owned())
        .spawn(move || forward_input(input_writer, input_alive))
        .map_err(|error| error.to_string())?;
    let resize_alive = Arc::clone(&alive);
    let resize_writer = Arc::clone(&writer);
    thread::Builder::new()
        .name("tm-resize".to_owned())
        .spawn(move || forward_resize(resize_writer, resize_alive, size))
        .map_err(|error| error.to_string())?;

    let mut stdout = io::stdout();
    while alive.load(Ordering::Acquire) {
        match read_server_message(&mut stream) {
            Ok(ServerMessage::Render(bytes)) => {
                stdout
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                stdout.flush().map_err(|error| error.to_string())?;
            }
            Ok(ServerMessage::Closed) => break,
            Ok(ServerMessage::Response { .. }) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    alive.store(false, Ordering::Release);
    let _ = raw.restore_for_drop();
    stdout
        .write_all(b"\x1b[?1000l\x1b[?1002l\x1b[?1006l\x1b[?1004l\x1b[>0u\x1b[?25h\x1b[0m\n")
        .map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())?;
    Ok(())
}

fn forward_input(stream: Arc<Mutex<UnixStream>>, alive: Arc<AtomicBool>) {
    let mut stdin = io::stdin();
    let mut buffer = [0_u8; 4096];
    while alive.load(Ordering::Acquire) {
        let length = match stdin.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(length) => length,
        };
        let Ok(mut stream) = stream.lock() else {
            break;
        };
        if write_client_message(
            &mut *stream,
            &ClientMessage::Input(buffer[..length].to_vec()),
        )
        .is_err()
        {
            break;
        }
    }
    alive.store(false, Ordering::Release);
}

fn forward_resize(stream: Arc<Mutex<UnixStream>>, alive: Arc<AtomicBool>, mut last_size: Size) {
    while alive.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(100));
        let size = terminal::size();
        if size == last_size {
            continue;
        }
        last_size = size;
        let Ok(mut stream) = stream.lock() else {
            break;
        };
        if write_client_message(&mut *stream, &ClientMessage::Resize(size)).is_err() {
            break;
        }
    }
}

trait RestoreRawTerminal {
    fn restore_for_drop(&mut self) -> io::Result<()>;
}

impl RestoreRawTerminal for RawTerminal {
    fn restore_for_drop(&mut self) -> io::Result<()> {
        self.restore()
    }
}
