use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::parity::Mux;

pub struct Shared {
    bytes: Vec<u8>,
    parser: vt100::Parser,
    closed: bool,
}

type Monitor = Arc<(Mutex<Shared>, Condvar)>;

const DEVICE_ATTRIBUTES_QUERY: &[u8] = b"\x1b[c";
const DEVICE_ATTRIBUTES_REPLY: &[u8] = b"\x1b[?1;2c";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedModes {
    pub alternate_screen: bool,
    pub bracketed_paste: bool,
    pub mouse_mode: String,
    pub mouse_encoding: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Termios {
    pub input: u32,
    pub output: u32,
    pub control: u32,
    pub local: u32,
    pub characters: Vec<u8>,
}

pub struct RawClient {
    master: File,
    child: Child,
    monitor: Monitor,
    reader: Option<JoinHandle<()>>,
    label: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Mark(usize);

fn lock(monitor: &Monitor) -> MutexGuard<'_, Shared> {
    monitor.0.lock().expect("raw pty state lock")
}

fn open_pty(rows: u16, cols: u16) -> (OwnedFd, OwnedFd) {
    let mut size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let mut master = -1;
    let mut slave = -1;
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    assert_eq!(result, 0, "openpty: {}", std::io::Error::last_os_error());
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    for descriptor in [&master, &slave] {
        let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
        let result =
            unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) };
        assert_eq!(result, 0, "set close-on-exec: {}", std::io::Error::last_os_error());
    }
    (master, slave)
}

impl RawClient {
    pub fn spawn(mux: &Mux, name: &str, arguments: &[&str], rows: u16, cols: u16) -> Self {
        let (master, slave) = open_pty(rows, cols);
        let mut command = Command::new(mux.binary());
        command
            .current_dir(mux.root())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", mux.root().join("home"))
            .env("TMPDIR", mux.root())
            .env("TMUX_TMPDIR", mux.root())
            .env("TERM", mux.term())
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .arg("-S")
            .arg(mux.socket());
        if let Some(config) = mux.config() {
            command.arg("-f").arg(config);
        }
        command.args(arguments);
        let stdin = slave.try_clone().expect("duplicate slave for stdin");
        let stdout = slave.try_clone().expect("duplicate slave for stdout");
        command
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("spawn {:?}: {error}", mux.binary()));
        drop(command);

        let master = File::from(master);
        let monitor: Monitor = Arc::new((
            Mutex::new(Shared {
                bytes: Vec::new(),
                parser: vt100::Parser::new(rows, cols, 0),
                closed: false,
            }),
            Condvar::new(),
        ));
        let mut source = master.try_clone().expect("duplicate master for reader");
        let reader_monitor = Arc::clone(&monitor);
        let mut replies = master.try_clone().expect("duplicate master for replies");
        let reader = std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            let mut answered = 0;
            loop {
                let count = match source.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                let mut shared = lock(&reader_monitor);
                shared.bytes.extend_from_slice(&buffer[..count]);
                shared.parser.process(&buffer[..count]);
                let queries = self::count(&shared.bytes, DEVICE_ATTRIBUTES_QUERY);
                while answered < queries {
                    answered += 1;
                    let _ = replies.write_all(DEVICE_ATTRIBUTES_REPLY);
                }
                reader_monitor.1.notify_all();
            }
            lock(&reader_monitor).closed = true;
            reader_monitor.1.notify_all();
        });
        Self {
            master,
            child,
            monitor,
            reader: Some(reader),
            label: name.to_owned(),
        }
    }

    pub fn mark(&self) -> Mark {
        Mark(lock(&self.monitor).bytes.len())
    }

    pub fn bytes_since(&self, mark: Mark) -> Vec<u8> {
        lock(&self.monitor).bytes[mark.0..].to_vec()
    }

    pub fn count_since(&self, mark: Mark, needle: &[u8]) -> usize {
        count(&lock(&self.monitor).bytes[mark.0..], needle)
    }

    pub fn count(&self, needle: &[u8]) -> usize {
        self.count_since(Mark(0), needle)
    }

    pub fn screen_text(&self) -> String {
        lock(&self.monitor).parser.screen().contents()
    }

    pub fn modes(&self) -> ObservedModes {
        let shared = lock(&self.monitor);
        let screen = shared.parser.screen();
        ObservedModes {
            alternate_screen: screen.alternate_screen(),
            bracketed_paste: screen.bracketed_paste(),
            mouse_mode: format!("{:?}", screen.mouse_protocol_mode()),
            mouse_encoding: format!("{:?}", screen.mouse_protocol_encoding()),
        }
    }

    pub fn screen_rows(&self) -> Vec<String> {
        let shared = lock(&self.monitor);
        let screen = shared.parser.screen();
        let (_, columns) = screen.size();
        screen.rows(0, columns).collect()
    }

    pub fn screen_cells(&self) -> Vec<(String, vt100::Color, vt100::Color, bool)> {
        let shared = lock(&self.monitor);
        let screen = shared.parser.screen();
        let (rows, columns) = screen.size();
        (0..rows)
            .flat_map(|row| (0..columns).map(move |column| (row, column)))
            .map(|(row, column)| {
                let cell = screen.cell(row, column).expect("cell inside the screen");
                (cell.contents().to_owned(), cell.fgcolor(), cell.bgcolor(), cell.inverse())
            })
            .collect()
    }

    pub fn styled_row(&self, row: u16) -> String {
        let shared = lock(&self.monitor);
        let screen = shared.parser.screen();
        let (_, columns) = screen.size();
        let color = |color: vt100::Color| match color {
            vt100::Color::Default => "default".to_owned(),
            vt100::Color::Idx(index) => format!("c{index}"),
            vt100::Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        };
        let mut runs: Vec<(String, String)> = Vec::new();
        for column in 0..columns {
            let cell = screen.cell(row, column).expect("cell inside the screen");
            let flags: String = [
                (cell.bold(), 'b'),
                (cell.dim(), 'd'),
                (cell.italic(), 'i'),
                (cell.underline(), 'u'),
                (cell.inverse(), 'r'),
            ]
            .iter()
            .filter(|(set, _)| *set)
            .map(|(_, flag)| *flag)
            .collect();
            let style = format!("{}/{}/{flags}", color(cell.fgcolor()), color(cell.bgcolor()));
            let text = if cell.contents().is_empty() { " " } else { cell.contents() };
            match runs.last_mut() {
                Some((last, run)) if *last == style => run.push_str(text),
                _ => runs.push((style, text.to_owned())),
            }
        }
        runs.iter().map(|(style, text)| format!("[{style}]{text:?}")).collect::<Vec<_>>().join(" ")
    }

    pub fn is_closed(&self) -> bool {
        lock(&self.monitor).closed
    }

    pub fn resize(&self, rows: u16, columns: u16) {
        let size = libc::winsize {
            ws_row: rows,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        lock(&self.monitor).parser.screen_mut().set_size(rows, columns);
        let result = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
        assert_eq!(result, 0, "set window size: {}", std::io::Error::last_os_error());
        self.signal(libc::SIGWINCH);
    }

    pub fn termios(&self) -> Termios {
        let mut attributes: libc::termios = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::tcgetattr(self.master.as_raw_fd(), &mut attributes) };
        assert_eq!(result, 0, "tcgetattr: {}", std::io::Error::last_os_error());
        Termios {
            input: attributes.c_iflag as u32,
            output: attributes.c_oflag as u32,
            control: attributes.c_cflag as u32,
            local: attributes.c_lflag as u32,
            characters: attributes.c_cc.to_vec(),
        }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.master
            .write_all(bytes)
            .unwrap_or_else(|error| panic!("{}: write client input: {error}", self.label));
    }

    pub fn signal(&self, signal: i32) {
        let result = unsafe { libc::kill(self.child.id() as i32, signal) };
        assert_eq!(result, 0, "signal client: {}", std::io::Error::last_os_error());
    }

    pub fn wait_until<T>(
        &self,
        what: &str,
        timeout: Duration,
        mut probe: impl FnMut(&Shared) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + timeout;
        let mut shared = lock(&self.monitor);
        loop {
            if let Some(value) = probe(&shared) {
                return value;
            }
            let now = Instant::now();
            if shared.closed || now >= deadline {
                let reason = if shared.closed { "client closed" } else { "timed out" };
                panic!(
                    "{}: {reason} waiting for {what}\nscreen:\n{}\nbytes: {:?}",
                    self.label,
                    shared.parser.screen().contents(),
                    String::from_utf8_lossy(&shared.bytes)
                );
            }
            shared = self
                .monitor
                .1
                .wait_timeout(shared, deadline - now)
                .expect("raw pty state lock")
                .0;
        }
    }

    pub fn wait_for_bytes(&self, from: Mark, needle: &[u8], timeout: Duration) -> Mark {
        self.wait_until(&format!("bytes {needle:?}"), timeout, |shared| {
            find(&shared.bytes[from.0..], needle).map(|at| Mark(from.0 + at + needle.len()))
        })
    }

    pub fn wait_for_stream<T>(
        &self,
        from: Mark,
        what: &str,
        timeout: Duration,
        mut probe: impl FnMut(&[u8]) -> Option<T>,
    ) -> T {
        self.wait_until(what, timeout, |shared| probe(&shared.bytes[from.0..]))
    }

    pub fn wait_for_text(&self, text: &str, timeout: Duration) {
        self.wait_until(&format!("screen text {text:?}"), timeout, |shared| {
            shared.parser.screen().contents().contains(text).then_some(())
        });
    }

    pub fn wait_for_screen(
        &self,
        what: &str,
        timeout: Duration,
        mut predicate: impl FnMut(&vt100::Screen) -> bool,
    ) {
        self.wait_until(what, timeout, |shared| {
            predicate(shared.parser.screen()).then_some(())
        });
    }

    pub fn detach(&mut self) -> Option<i32> {
        self.write(b"\x01d");
        self.finish(Duration::from_secs(5))
    }

    pub fn finish(&mut self, timeout: Duration) -> Option<i32> {
        self.wait_until("client output closed", timeout, |shared| shared.closed.then_some(()));
        let status = self.child.wait().expect("reap client");
        status.code().or_else(|| status.signal().map(|signal| -signal))
    }
}

impl Drop for RawClient {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|window| *window == needle).count()
}

pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

pub fn osc52_sequences(stream: &[u8]) -> Vec<Vec<u8>> {
    let opener = b"\x1b]52;";
    let mut sequences = Vec::new();
    let mut offset = 0;
    while let Some(at) = find(&stream[offset..], opener) {
        let start = offset + at;
        let body = &stream[start + opener.len()..];
        let Some(end) = body.iter().position(|byte| *byte == 0x07 || *byte == 0x1b) else {
            break;
        };
        let terminator = if body[end] == 0x1b { 2 } else { 1 };
        let stop = (start + opener.len() + end + terminator).min(stream.len());
        sequences.push(stream[start..stop].to_vec());
        offset = stop;
    }
    sequences
}
