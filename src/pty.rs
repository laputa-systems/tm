use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, RawFd};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use crate::model::Size;
use rustix::termios::{Winsize, tcsetwinsize};

const INPUT_QUEUE_LIMIT: usize = 16 * 1024 * 1024;
const REPLY_QUEUE_LIMIT: usize = 64 * 1024;

#[derive(Clone)]
struct InputQueue {
    sender: mpsc::Sender<Vec<u8>>,
    queued: Arc<AtomicUsize>,
}

impl InputQueue {
    fn push(&self, bytes: &[u8], limit: usize) -> bool {
        let previous = self.queued.fetch_add(bytes.len(), Ordering::SeqCst);
        if previous.saturating_add(bytes.len()) > limit
            || self.sender.send(bytes.to_vec()).is_err()
        {
            self.queued.fetch_sub(bytes.len(), Ordering::SeqCst);
            return false;
        }
        true
    }
}

/// One Unix PTY master and the process group it owns.
pub(crate) struct Pty {
    master: Arc<Mutex<File>>,
    input: Option<InputQueue>,
    pid: libc::pid_t,
}

impl Clone for Pty {
    fn clone(&self) -> Self {
        Self {
            master: Arc::clone(&self.master),
            input: self.input.clone(),
            pid: self.pid,
        }
    }
}

impl Pty {
    pub(crate) fn empty() -> io::Result<Self> {
        let master = File::open("/dev/null")?;
        Ok(Self {
            master: Arc::new(Mutex::new(master)),
            input: None,
            pid: 0,
        })
    }

    pub(crate) fn spawn(
        command: &[String],
        cwd: Option<&Path>,
        size: Size,
        terminal: Option<&str>,
        environment: &HashMap<String, String>,
    ) -> io::Result<Self> {
        let (program, arguments) = command_line(command)?;
        let program_c = CString::new(program.as_bytes()).map_err(invalid_command)?;
        let argument_c = arguments
            .iter()
            .map(|argument| CString::new(argument.as_bytes()).map_err(invalid_command))
            .collect::<io::Result<Vec<_>>>()?;
        // The pane environment is converted before the fork so the child only
        // touches already-built data before exec.
        let environment_c = environment
            .iter()
            .map(|(name, value)| {
                let name = CString::new(name.as_bytes()).map_err(invalid_command)?;
                let value = CString::new(value.as_bytes()).map_err(invalid_command)?;
                Ok((name, value))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let mut argv = Vec::with_capacity(argument_c.len() + 2);
        argv.push(program_c.as_ptr());
        argv.extend(argument_c.iter().map(|argument| argument.as_ptr()));
        argv.push(std::ptr::null());

        let cwd_c = cwd
            .map(|path| CString::new(path.to_string_lossy().as_bytes()).map_err(invalid_command))
            .transpose()?;
        #[cfg_attr(target_os = "linux", allow(unused_mut))]
        let mut winsize = libc::winsize {
            ws_row: size.rows().max(1),
            ws_col: size.cols().max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let mut master: RawFd = -1;
        let mut slave: RawFd = -1;
        // openpty takes a const winsize pointer on Linux but a mutable one
        // on macOS, so the pointer mutability follows the target.
        #[cfg(target_os = "linux")]
        let winp = &raw const winsize;
        #[cfg(not(target_os = "linux"))]
        let winp = &raw mut winsize;

        // SAFETY: openpty initializes both descriptors when it succeeds. The
        // optional name, termios, and pixel geometry are intentionally absent.
        let result = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                winp,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        set_cloexec(master)?;
        set_cloexec(slave)?;

        // SAFETY: both fds were initialized by the successful openpty call.
        let pid = unsafe { libc::fork() };
        if pid == -1 {
            close_fd(master);
            close_fd(slave);
            return Err(io::Error::last_os_error());
        }

        if pid == 0 {
            child_setup(
                slave,
                master,
                &argv,
                cwd_c.as_ref(),
                terminal,
                &environment_c,
            );
        }

        close_fd(slave);
        // SAFETY: the parent owns the master descriptor returned by openpty.
        let master_file = unsafe { File::from_raw_fd(master) };
        let writer_file = master_file.try_clone()?;
        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        let queued = Arc::new(AtomicUsize::new(0));
        let drained = Arc::clone(&queued);
        std::thread::Builder::new()
            .name(format!("tm-pane-write-{pid}"))
            .spawn(move || {
                let mut writer_file = writer_file;
                for bytes in receiver {
                    let length = bytes.len();
                    if writer_file.write_all(&bytes).is_err() {
                        break;
                    }
                    drained.fetch_sub(length, Ordering::SeqCst);
                }
            })
            ?;
        Ok(Self {
            master: Arc::new(Mutex::new(master_file)),
            input: Some(InputQueue { sender, queued }),
            pid,
        })
    }

    pub(crate) fn reader(&self) -> io::Result<File> {
        self.master.lock().map_err(|_| poisoned())?.try_clone()
    }

    pub(crate) fn pid(&self) -> libc::pid_t {
        self.pid
    }

    pub(crate) fn current_dir(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let foreground = self.foreground_pgrp().unwrap_or(self.pid);
        process_cwd(foreground).or_else(|| process_cwd(self.pid))
    }

    fn foreground_pgrp(&self) -> Option<libc::pid_t> {
        let master = self.master.lock().ok()?;
        // SAFETY: the master descriptor is a live terminal endpoint.
        let pgrp = unsafe { libc::tcgetpgrp(master.as_raw_fd()) };
        (pgrp > 0).then_some(pgrp)
    }

    pub(crate) fn reap(pid: libc::pid_t) {
        let mut status = 0;
        // SAFETY: pid is the child process created for this PTY. The blocking
        // wait runs only after its master reader has observed EOF or EIO.
        unsafe {
            while libc::waitpid(pid, &mut status, 0) == -1
                && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
            {}
        }
    }

    pub(crate) fn write(&self, bytes: &[u8]) -> io::Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        let input = self.input.as_ref().ok_or_else(poisoned)?;
        if input.push(bytes, INPUT_QUEUE_LIMIT) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "pane input queue is full",
            ))
        }
    }

    pub(crate) fn write_query_reply(&self, bytes: &[u8]) {
        if let Some(input) = self.input.as_ref().filter(|_| !self.is_empty()) {
            input.push(bytes, REPLY_QUEUE_LIMIT);
        }
    }

    pub(crate) fn resize(&self, size: Size) -> io::Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        let winsize = Winsize {
            ws_row: size.rows().max(1),
            ws_col: size.cols().max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let master = self.master.lock().map_err(|_| poisoned())?;
        tcsetwinsize(master.as_fd(), winsize).map_err(io::Error::from)?;
        // SAFETY: the negative pid addresses the PTY child's process group.
        unsafe { libc::kill(-self.pid, libc::SIGWINCH) };
        Ok(())
    }

    pub(crate) fn kill(&self) {
        if self.is_empty() {
            return;
        }
        // SAFETY: `pid` is the child this process forked and has not reaped, so
        // the id still names that process group.
        unsafe {
            libc::kill(-self.pid, libc::SIGTERM);
            libc::kill(-self.pid, libc::SIGHUP);
        }
    }

    fn is_last_handle(&self) -> bool {
        Arc::strong_count(&self.master) == 1
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pid == 0
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        if self.is_last_handle() {
            self.kill();
        }
    }
}

pub(crate) fn default_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| Path::new(shell).is_file())
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

fn command_line(command: &[String]) -> io::Result<(String, Vec<String>)> {
    if let [line] = command {
        return Ok((default_shell(), vec!["-c".to_owned(), line.clone()]));
    }
    if let Some(program) = command.first() {
        return Ok((program.clone(), command[1..].to_vec()));
    }
    let shell = default_shell();
    let arguments = match Path::new(&shell).file_name().and_then(|name| name.to_str()) {
        Some("sh" | "bash" | "dash" | "ksh" | "zsh") => {
            vec!["-l".to_owned(), "-i".to_owned()]
        }
        _ => Vec::new(),
    };
    Ok((shell, arguments))
}

fn child_setup(
    slave: RawFd,
    master: RawFd,
    argv: &[*const libc::c_char],
    cwd: Option<&CString>,
    terminal: Option<&str>,
    environment: &[(CString, CString)],
) -> ! {
    // SAFETY: this branch is the child immediately after fork. It performs
    // only the Unix session/descriptor operations needed before exec.
    unsafe {
        let mut empty: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut empty);
        libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        libc::close(master);
        libc::setsid();
        // TIOCSCTTY already has the ioctl request type on Linux but needs a
        // conversion on other targets.
        #[cfg(target_os = "linux")]
        libc::ioctl(slave, libc::TIOCSCTTY, 0);
        #[cfg(not(target_os = "linux"))]
        libc::ioctl(slave, libc::TIOCSCTTY.into(), 0);
        libc::dup2(slave, libc::STDIN_FILENO);
        libc::dup2(slave, libc::STDOUT_FILENO);
        libc::dup2(slave, libc::STDERR_FILENO);
        if slave > libc::STDERR_FILENO {
            libc::close(slave);
        }
        if let Some(cwd) = cwd {
            libc::chdir(cwd.as_ptr());
        }
        let term = CString::new("TERM").expect("static variable name has no NUL");
        let terminal = terminal
            .filter(|value| !value.is_empty())
            .and_then(|value| CString::new(value).ok())
            .unwrap_or_else(|| CString::new("screen-256color").expect("static value is valid"));
        #[cfg(target_os = "linux")]
        libc::clearenv();
        #[cfg(target_os = "macos")]
        {
            // macOS does not provide clearenv(). Snapshot names and unset them so
            // later setenv calls rebuild a valid environment.
            unsafe extern "C" {
                safe fn _NSGetEnviron() -> *mut *mut *mut libc::c_char;
            }
            let mut names = Vec::new();
            let mut env = *_NSGetEnviron();
            while !env.is_null() && !(*env).is_null() {
                let entry = *env;
                let mut len = 0usize;
                while *entry.add(len) != 0 {
                    if *entry.add(len) == b'=' as libc::c_char {
                        names.push(std::slice::from_raw_parts(entry as *const u8, len).to_vec());
                        break;
                    }
                    len += 1;
                }
                env = env.add(1);
            }
            for mut name in names {
                name.push(0);
                libc::unsetenv(name.as_ptr() as *const libc::c_char);
            }
        }
        for (name, value) in environment {
            libc::setenv(name.as_ptr(), value.as_ptr(), 1);
        }
        libc::setenv(term.as_ptr(), terminal.as_ptr(), 1);
        libc::execvp(argv[0], argv.as_ptr());
        libc::_exit(127);
    }
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl only inspects the live descriptor owned by this call.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn close_fd(fd: RawFd) {
    // SAFETY: fd is an owned descriptor that must be closed on this path.
    unsafe { libc::close(fd) };
}

#[cfg(target_os = "linux")]
fn process_cwd(pid: libc::pid_t) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn process_cwd(pid: libc::pid_t) -> Option<String> {
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: info is a zeroed proc_vnodepathinfo of the declared size.
    let result = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            std::ptr::addr_of_mut!(info).cast(),
            size,
        )
    };
    if result <= 0 {
        return None;
    }
    let path = &info.pvi_cdir.vip_path;
    let bytes = unsafe {
        std::slice::from_raw_parts(path.as_ptr().cast::<u8>(), std::mem::size_of_val(path))
    };
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_cwd(_pid: libc::pid_t) -> Option<String> {
    None
}

fn invalid_command(error: std::ffi::NulError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

fn poisoned() -> io::Error {
    io::Error::other("PTY lock is poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silent_pane() -> Pty {
        Pty::spawn(
            &["sleep".to_owned(), "30".to_owned()],
            None,
            Size::new(80, 24),
            None,
            &HashMap::new(),
        )
        .expect("spawn silent pane")
    }

    fn queued(pty: &Pty) -> usize {
        pty.input
            .as_ref()
            .expect("live pane has an input queue")
            .queued
            .load(Ordering::SeqCst)
    }

    #[test]
    fn query_replies_are_dropped_once_the_pane_stops_reading() {
        let pty = silent_pane();
        let reply = vec![b'r'; 1024];
        for _ in 0..10_000 {
            pty.write_query_reply(&reply);
        }
        assert!(queued(&pty) <= REPLY_QUEUE_LIMIT, "queued {}", queued(&pty));
    }

    #[test]
    fn user_input_is_rejected_instead_of_growing_past_the_limit() {
        let pty = silent_pane();
        let chunk = vec![b'k'; 1024 * 1024];
        let mut accepted = 0;
        let error = loop {
            match pty.write(&chunk) {
                Ok(()) => accepted += 1,
                Err(error) => break error,
            }
            assert!(accepted <= 2 * INPUT_QUEUE_LIMIT / chunk.len(), "unbounded");
        };
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(queued(&pty) <= INPUT_QUEUE_LIMIT);
    }
}
