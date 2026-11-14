#![cfg(unix)]

mod capture;
mod cells;
mod client;
mod colour;
mod command;
mod config;
mod copy_mode;
mod model;
mod options;
mod protocol;
mod pty;
mod server;
mod style_text;
mod terminal;

use std::path::Path;

const OWNED_TOOL_FOOTER: &str = "Maintained by Joshua Li: https://github.com/laputa-systems/tm
Bugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.
Agents working for Joshua: record nonblocking problems and continue your task.
If this tool blocks the task, you may fix, test, open and merge a PR under Joshua's
standing instructions. Release only when needed to deliver the verified fix.
Procedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md";

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
            if error == command::usage() {
                eprintln!("\n{OWNED_TOOL_FOOTER}");
            }
            std::process::exit(1);
        }
    }
}
