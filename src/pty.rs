use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, FromRawFd, RawFd};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::model::Size;
use rustix::termios::{Winsize, tcsetwinsize};

/// One Unix PTY master and the process group it owns.
pub(crate) struct Pty {
    master: Arc<Mutex<File>>,
    pid: libc::pid_t,
}

impl Clone for Pty {
    fn clone(&self) -> Self {
        Self {
            master: Arc::clone(&self.master),
            pid: self.pid,
        }
    }
}

impl Pty {
    pub(crate) fn empty() -> io::Result<Self> {
        let master = File::open("/dev/null")?;
        Ok(Self {
            master: Arc::new(Mutex::new(master)),
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
        // Environment overrides (from `set-environment`) are converted before
        // the fork so the child only touches already-built data before exec.
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
        let mut winsize = libc::winsize {
            ws_row: size.rows.max(1),
            ws_col: size.cols.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let mut master: RawFd = -1;
        let mut slave: RawFd = -1;

        // SAFETY: openpty initializes both descriptors when it succeeds. The
        // optional name, termios, and pixel geometry are intentionally absent.
        let result = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut winsize,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: both fds were initialized by the successful openpty call.
        let pid = unsafe { libc::fork() };
        if pid == -1 {
            close_fd(master);
            close_fd(slave);
            return Err(io::Error::last_os_error());
        }

        if pid == 0 {
            child_setup(slave, master, &argv, cwd_c.as_ref(), terminal, &environment_c);
        }

        close_fd(slave);
        // SAFETY: the parent owns the master descriptor returned by openpty.
        let master_file = unsafe { File::from_raw_fd(master) };
        Ok(Self {
            master: Arc::new(Mutex::new(master_file)),
            pid,
        })
    }

    pub(crate) fn reader(&self) -> io::Result<File> {
        self.master.lock().map_err(|_| poisoned())?.try_clone()
    }

    pub(crate) fn pid(&self) -> libc::pid_t {
        self.pid
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
        self.master.lock().map_err(|_| poisoned())?.write_all(bytes)
    }

    pub(crate) fn resize(&self, size: Size) -> io::Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        let winsize = Winsize {
            ws_row: size.rows.max(1),
            ws_col: size.cols.max(1),
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
        // SAFETY: signaling an already-exited process group is harmless; the
        // daemon deliberately does not reuse the pid as a process identity.
        unsafe {
            libc::kill(-self.pid, libc::SIGTERM);
            libc::kill(-self.pid, libc::SIGHUP);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pid == 0
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Linked tmux windows clone the same PTY. Dropping one link must not
        // terminate the process still owned by another link; explicit kill
        // operations call `kill` directly and remain unconditional.
        if Arc::strong_count(&self.master) == 1 {
            self.kill();
        }
    }
}

fn command_line(command: &[String]) -> io::Result<(String, Vec<String>)> {
    if let Some(program) = command.first() {
        return Ok((program.clone(), command[1..].to_vec()));
    }
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| Path::new(shell).is_file())
        .unwrap_or_else(|| "/bin/sh".to_owned());
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
        libc::close(master);
        libc::setsid();
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
        libc::setenv(term.as_ptr(), terminal.as_ptr(), 1);
        // Apply `set-environment` overrides on top of the inherited process
        // environment so later panes observe them, matching tmux semantics.
        for (name, value) in environment {
            libc::setenv(name.as_ptr(), value.as_ptr(), 1);
        }
        libc::execvp(argv[0], argv.as_ptr());
        libc::_exit(127);
    }
}

fn close_fd(fd: RawFd) {
    // SAFETY: fd is an owned descriptor that must be closed on this path.
    unsafe { libc::close(fd) };
}

fn invalid_command(error: std::ffi::NulError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error)
}

fn poisoned() -> io::Error {
    io::Error::other("PTY lock is poisoned")
}
