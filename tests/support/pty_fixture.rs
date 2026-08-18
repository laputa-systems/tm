#![cfg(unix)]

//! Small deterministic child used by `tests/attached_client_pty.rs`.
//!
//! The fixture speaks a line-oriented protocol over its PTY. Its output is
//! deliberately terminal-shaped, so the integration test can parse the
//! complete outer client stream with `vt100` instead of matching a byte
//! transcript.

use std::io::{self, Write};
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};

static RESIZED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigwinch(_: libc::c_int) {
    RESIZED.store(true, Ordering::Release);
}

fn main() -> io::Result<()> {
    install_sigwinch_handler()?;

    // Clear the pane and put the marker at a stable cursor position. This is
    // the barrier that tells the outer test that the complete daemon/PTY/
    // attach/render path is live.
    emit(b"\x1b[2J\x1b[HVT_FIXTURE_READY\r\n")?;

    let mut input = Vec::new();
    loop {
        input.clear();
        if !read_line_with_resize(&mut input)? {
            return Ok(());
        }
        while input.last().is_some_and(|byte| matches!(byte, b'\n' | b'\r')) {
            input.pop();
        }
        match input.as_slice() {
            b"input" => emit(b"INPUT_ACK\r\n")?,
            b"begin-resize" => {
                RESIZED.store(false, Ordering::Release);
                emit(b"RESIZE_WAIT\r\n")?;
                // The daemon's resize operation delivers SIGWINCH to the
                // pane's process group. Polling stdin keeps this loop bounded
                // and signal-aware without a readiness sleep.
                while !RESIZED.load(Ordering::Acquire) {
                    wait_for_input(50)?;
                }
                emit(b"RESIZE_SIGNAL\r\n")?;
            }
            b"measure" => {
                let (rows, cols) = winsize(0)?;
                let message = format!("RESIZE_ACK:{rows}x{cols}\r\n");
                emit(message.as_bytes())?;
            }
            b"quit" => {
                emit(b"FIXTURE_DONE\r\n")?;
                // Keep the PTY alive until the client sends its explicit
                // detach sequence. This makes FIXTURE_DONE an observable
                // output barrier rather than racing the daemon's EOF cleanup
                // against the outer VT parser.
            }
            _ => emit(b"FIXTURE_UNKNOWN\r\n")?,
        }
    }
}

fn emit(bytes: &[u8]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(bytes)?;
    stdout.flush()
}

fn install_sigwinch_handler() -> io::Result<()> {
    // SAFETY: `sigaction` is initialized with a function pointer to the
    // process-local async-signal-safe handler and an empty signal mask.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_sigwinch as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = 0;
        if libc::sigaction(libc::SIGWINCH, &action, std::ptr::null_mut()) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn read_line_with_resize(buffer: &mut Vec<u8>) -> io::Result<bool> {
    loop {
        wait_for_input(50)?;
        let mut byte = [0_u8; 1];
        // SAFETY: fd 0 is the fixture's PTY slave inherited from the test.
        let read = unsafe { libc::read(0, byte.as_mut_ptr().cast(), 1) };
        if read == 1 {
            buffer.push(byte[0]);
            if byte[0] == b'\n' {
                return Ok(true);
            }
            continue;
        }
        if read == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted
            || error.kind() == io::ErrorKind::WouldBlock
        {
            continue;
        }
        // macOS reports a PTY peer disappearing as EIO rather than EOF.
        if error.raw_os_error() == Some(libc::EIO) {
            return Ok(false);
        }
        return Err(error);
    }
}

fn wait_for_input(timeout_ms: i32) -> io::Result<()> {
    let mut pollfd = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `pollfd` points to one valid descriptor owned by this process.
    let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
    if result == -1 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

fn winsize(fd: RawFd) -> io::Result<(u16, u16)> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `size` is valid writable storage for TIOCGWINSZ.
    let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ.into(), &mut size) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((size.ws_row, size.ws_col))
}
