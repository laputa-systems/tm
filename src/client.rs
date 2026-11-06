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
    ClientMessage, PROTOCOL_VERSION, ServerMessage, encode_request, read_server_message,
    read_version, write_client_message, write_version,
};
use crate::terminal::{self, RawTerminal};

pub(crate) fn run(invocation: Invocation) -> Result<(), String> {
    let socket = invocation.socket;
    let request = invocation.request;
    let attach = invocation.attach;
    let caller_pane = caller_pane(&socket);
    let starts_server = matches!(request, crate::protocol::Request::NewSession { .. });
    if attach && starts_server && !stdin_is_terminal() {
        return Err(NOT_A_TERMINAL.to_owned());
    }
    if let crate::protocol::Request::Attach { target, .. } = &request
        && !stdin_is_terminal()
    {
        find_session(&socket, target.as_deref().unwrap_or_default(), caller_pane)?;
        return Err(NOT_A_TERMINAL.to_owned());
    }
    if let crate::protocol::Request::NewSession {
        name: Some(name),
        detached: true,
        attach_existing: true,
        ..
    } = &request
        && find_session(&socket, &format!("={name}"), caller_pane).is_ok()
    {
        return attach_loop(&socket, Some(format!("={name}")), caller_pane);
    }
    let frame = encode_request(&request, caller_pane).map_err(|error| error.to_string())?;
    let mut stream = connect_or_start(&socket, starts_server)?;
    send_request(&mut stream, &socket, &frame)?;
    let response = read_server_message(&mut stream).map_err(|error| error.to_string())?;
    let (ok, body) = match response {
        ServerMessage::Response { ok, body } => (ok, body),
        ServerMessage::Output { status, bytes } => {
            let mut stdout = io::stdout();
            stdout
                .write_all(&bytes)
                .and_then(|()| stdout.flush())
                .map_err(|error| error.to_string())?;
            if status != 0 {
                std::process::exit(i32::from(status));
            }
            return Ok(());
        }
        _ => return Err("server returned an invalid command response".to_owned()),
    };
    if !ok {
        return Err(body);
    }
    let prints_body = match &request {
        crate::protocol::Request::NewSession { format, .. } => format.is_some(),
        crate::protocol::Request::DisplayMessage { print, .. } => *print,
        crate::protocol::Request::CapturePane { print, .. } => *print,
        _ => !body.is_empty(),
    };
    if !attach && prints_body {
        match &request {
            crate::protocol::Request::ShowBuffer { .. }
            | crate::protocol::Request::SaveBuffer { .. }
            | crate::protocol::Request::ShowOptions { .. }
            | crate::protocol::Request::ShowEnvironment { .. }
            | crate::protocol::Request::ListWindows { .. } => print!("{body}"),
            _ => println!("{body}"),
        }
    }
    if !attach {
        return Ok(());
    }

    if let crate::protocol::Request::Attach { size, .. } = request {
        return attach_stream(stream, size);
    }

    let target = match &request {
        crate::protocol::Request::NewSession { .. } => Some(body),
        crate::protocol::Request::NewWindow { target, .. } => target.clone(),
        crate::protocol::Request::Attach { target, .. } => target.clone(),
        _ => None,
    };
    attach_loop(&socket, target, caller_pane)
}

fn caller_pane(socket: &std::path::Path) -> Option<u64> {
    let own_socket = std::env::var_os(crate::server::SOCKET_ENV)?;
    if std::path::Path::new(&own_socket) != socket {
        return None;
    }
    std::env::var(crate::server::PANE_ENV)
        .ok()?
        .strip_prefix('%')?
        .parse()
        .ok()
}

const NOT_A_TERMINAL: &str = "open terminal failed: not a terminal";

fn stdin_is_terminal() -> bool {
    // SAFETY: isatty only inspects file descriptor 0.
    unsafe { libc::isatty(0) == 1 }
}

fn find_session(
    socket: &std::path::Path,
    target: &str,
    caller_pane: Option<u64>,
) -> Result<(), String> {
    let mut stream = connect_or_start(socket, false)?;
    let frame = encode_request(
        &crate::protocol::Request::HasSession {
            target: target.to_owned(),
        },
        caller_pane,
    )
    .map_err(|error| error.to_string())?;
    send_request(&mut stream, socket, &frame)?;
    match read_server_message(&mut stream).map_err(|error| error.to_string())? {
        ServerMessage::Response { ok: true, .. } => Ok(()),
        ServerMessage::Response { body, .. } => Err(body),
        _ => Err("server returned an invalid command response".to_owned()),
    }
}

fn protocol_mismatch(socket: &std::path::Path) -> String {
    let socket = socket.display();
    format!("tm server at {socket} runs a different protocol; run tm -S {socket} kill-server")
}

fn send_request(
    stream: &mut UnixStream,
    socket: &std::path::Path,
    frame: &[u8],
) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(4 + frame.len());
    write_version(&mut bytes).map_err(|error| error.to_string())?;
    bytes.extend_from_slice(frame);
    let written = stream.write_all(&bytes);
    match read_version(stream) {
        Ok(PROTOCOL_VERSION) => {}
        Ok(_) => return Err(protocol_mismatch(socket)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::BrokenPipe
            ) =>
        {
            return Err(protocol_mismatch(socket));
        }
        Err(error) => return Err(error.to_string()),
    }
    written.map_err(|error| error.to_string())
}

fn connect_failure(socket: &std::path::Path, error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::ConnectionRefused {
        return format!("no server running on {}", socket.display());
    }
    let text = error.to_string();
    let reason = text
        .rsplit_once(" (os error ")
        .map_or(text.as_str(), |(reason, _)| reason);
    format!("error connecting to {} ({reason})", socket.display())
}

fn connect_or_start(socket: &std::path::Path, start: bool) -> Result<UnixStream, String> {
    crate::server::require_own_socket(socket)?;
    let refusal = match UnixStream::connect(socket) {
        Ok(stream) => return Ok(stream),
        Err(error) => error,
    };
    if !start {
        return Err(connect_failure(socket, &refusal));
    }
    crate::server::ensure_socket_parent(socket).map_err(|error| error.to_string())?;
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
    let mut message = format!("tm daemon did not start at {}", socket.display());
    let log_path = {
        let mut value = socket.as_os_str().to_owned();
        value.push(".log");
        std::path::PathBuf::from(value)
    };
    if let Ok(log) = std::fs::read_to_string(&log_path) {
        let tail = log.lines().rev().take(8).collect::<Vec<_>>();
        if !tail.is_empty() {
            message.push('\n');
            message.push_str(&tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
    }
    Err(message)
}

fn attach_loop(
    socket: &std::path::Path,
    target: Option<String>,
    caller_pane: Option<u64>,
) -> Result<(), String> {
    if !stdin_is_terminal() {
        return Err(NOT_A_TERMINAL.to_owned());
    }
    crate::server::require_own_socket(socket)?;
    let mut stream = UnixStream::connect(socket).map_err(|error| error.to_string())?;
    let size = terminal::size();
    let term = std::env::var("TERM").unwrap_or_default();
    let frame = encode_request(
        &crate::protocol::Request::Attach { target, size, term },
        caller_pane,
    )
    .map_err(|error| error.to_string())?;
    send_request(&mut stream, socket, &frame)?;
    let response = read_server_message(&mut stream).map_err(|error| error.to_string())?;
    let ServerMessage::Response { ok, body } = response else {
        return Err("server returned an invalid attach response".to_owned());
    };
    if !ok {
        return Err(body);
    }

    attach_stream(stream, size)
}

fn restore_bytes() -> Vec<u8> {
    let term = std::env::var("TERM").unwrap_or_default();
    let features = crate::config::COMPILED_OPTIONS
        .iter()
        .find(|(key, _)| *key == "terminal-features")
        .map(|(_, value)| *value)
        .unwrap_or("");
    let capabilities = terminal::Capabilities::for_term(&term, features);
    terminal::restore_sequence(&capabilities)
}

fn attach_signals() -> libc::sigset_t {
    let mut set = unsafe { std::mem::zeroed::<libc::sigset_t>() };
    unsafe {
        libc::sigemptyset(&mut set);
        for signal in [
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTSTP,
            libc::SIGCONT,
        ] {
            libc::sigaddset(&mut set, signal);
        }
    }
    set
}

fn block_signals(set: &libc::sigset_t) -> Result<(), String> {
    // SAFETY: set points to the initialized signal mask for this process.
    let result = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, set, std::ptr::null_mut()) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result).to_string())
    }
}

fn start_signal_thread(
    set: libc::sigset_t,
    original: rustix::termios::Termios,
    raw: rustix::termios::Termios,
    restore: Vec<u8>,
    setup: Vec<u8>,
    writer: Arc<Mutex<UnixStream>>,
    size: Size,
) {
    let _ = thread::Builder::new()
        .name("tm-signals".to_owned())
        .spawn(move || {
            loop {
                let mut signal = 0;
                // SAFETY: set is a valid signal set for sigwait.
                if unsafe { libc::sigwait(&set, &mut signal) } != 0 {
                    continue;
                }
                if signal == libc::SIGTSTP {
                    let mut stdout = io::stdout();
                    let _ = stdout.write_all(&restore);
                    let _ = stdout.flush();
                    let _ = terminal::apply_termios(&original);
                    unsafe {
                        libc::raise(libc::SIGSTOP);
                    }
                    let _ = terminal::apply_termios(&raw);
                    let mut stdout = io::stdout();
                    let _ = stdout.write_all(&setup);
                    let _ = stdout.flush();
                    if let Ok(mut stream) = writer.lock() {
                        let _ = crate::protocol::write_client_message(
                            &mut *stream,
                            &crate::protocol::ClientMessage::Resize(size),
                        );
                    }
                    continue;
                }
                if signal == libc::SIGCONT {
                    continue;
                }
                let mut stdout = io::stdout();
                let _ = stdout.write_all(&restore);
                let _ = stdout.flush();
                let _ = terminal::apply_termios(&original);
                let reason: &[u8] = match signal {
                    libc::SIGTERM => b"[terminated]\n",
                    libc::SIGHUP => b"[lost tty]\n",
                    _ => b"",
                };
                let mut stdout = io::stdout();
                let _ = stdout.write_all(reason);
                let _ = stdout.flush();
                unsafe {
                    libc::_exit(if reason.is_empty() { 128 + signal } else { 1 });
                }
            }
        });
}

fn attach_stream(mut stream: UnixStream, size: Size) -> Result<(), String> {
    let signals = attach_signals();
    block_signals(&signals)?;
    let mut raw = RawTerminal::enter().map_err(|error| {
        if error.raw_os_error() == Some(libc::ENOTTY) {
            NOT_A_TERMINAL.to_owned()
        } else {
            format!("enter raw terminal mode: {error}")
        }
    })?;
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
    start_signal_thread(
        signals,
        raw.original_termios(),
        terminal::make_raw(&raw.original_termios()),
        restore_bytes(),
        {
            let term = std::env::var("TERM").unwrap_or_default();
            let features = crate::config::COMPILED_OPTIONS
                .iter()
                .find(|(key, _)| *key == "terminal-features")
                .map(|(_, value)| *value)
                .unwrap_or("");
            terminal::setup_sequence(&terminal::Capabilities::for_term(&term, features))
        },
        Arc::clone(&writer),
        size,
    );
    let resize_alive = Arc::clone(&alive);
    let resize_writer = Arc::clone(&writer);
    thread::Builder::new()
        .name("tm-resize".to_owned())
        .spawn(move || forward_resize(resize_writer, resize_alive, size))
        .map_err(|error| error.to_string())?;

    let mut stdout = io::stdout();
    let mut restored = false;
    while alive.load(Ordering::Acquire) {
        match read_server_message(&mut stream) {
            Ok(ServerMessage::Render(bytes)) => {
                stdout
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                stdout.flush().map_err(|error| error.to_string())?;
            }
            Ok(ServerMessage::Restore(bytes)) => {
                restored = true;
                stdout
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                stdout.flush().map_err(|error| error.to_string())?;
            }
            Ok(ServerMessage::Closed) => break,
            Ok(ServerMessage::Response { .. } | ServerMessage::Output { .. }) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    alive.store(false, Ordering::Release);
    let _ = raw.restore_for_drop();
    if !restored {
        let term = std::env::var("TERM").unwrap_or_default();
        let features = crate::config::COMPILED_OPTIONS
        .iter()
        .find(|(key, _)| *key == "terminal-features")
        .map(|(_, value)| *value)
        .unwrap_or("");
        let capabilities = terminal::Capabilities::for_term(&term, features);
        let restore = terminal::restore_sequence(&capabilities);
        stdout
            .write_all(&restore)
            .map_err(|error| error.to_string())?;
        stdout.flush().map_err(|error| error.to_string())?;
    }
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
