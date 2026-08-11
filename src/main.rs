#![cfg(unix)]

mod client;
mod command;
mod config;
mod copy_mode;
mod model;
mod protocol;
mod pty;
mod server;
mod terminal;

use std::path::Path;

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.first().map(String::as_str) == Some("--daemon") {
        let Some(path) = arguments.get(1) else {
            eprintln!("tm --daemon requires a socket path");
            std::process::exit(2);
        };
        if let Err(error) = server::run_daemon(Path::new(path)) {
            eprintln!("tm daemon: {error}");
            std::process::exit(1);
        }
        return;
    }
    if arguments
        .first()
        .is_some_and(|arg| arg == "-V" || arg == "--version")
    {
        println!("tm {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    match command::parse(&arguments).and_then(client::run) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("tm: {error}");
            std::process::exit(1);
        }
    }
}
