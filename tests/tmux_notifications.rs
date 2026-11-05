#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::time::Duration;

use parity::{Mux, Scratch};
use raw_pty::{osc52_sequences, Mark, RawClient};

const TIMEOUT: Duration = Duration::from_secs(5);

fn create_session(mux: &Mux, command: &str) {
    mux.ok(&[
        "new-session", "-d", "-s", "S", "-x", "40", "-y", "10", "--", "sh", "-c", command,
    ]);
}

fn attach(mux: &Mux, name: &str) -> RawClient {
    let client = RawClient::spawn(mux, name, &["attach-session", "-t", "S"], 10, 40);
    client.wait_for_text("(S)", TIMEOUT);
    client
}

fn both() -> (Scratch, Mux, Mux) {
    let scratch = Scratch::new("notify");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    (scratch, tm, reference)
}

const BELL_PANE: &str =
    "read line; printf '\\007'; echo BELLED; read line; echo BARRIER; sleep 30";

fn ring_and_pass_barrier(mux: &Mux, clients: &[&RawClient]) {
    mux.ok(&["send-keys", "-t", "S", "bell", "Enter"]);
    for client in clients {
        client.wait_for_text("BELLED", TIMEOUT);
    }
    mux.ok(&["send-keys", "-t", "S", "go", "Enter"]);
    for client in clients {
        client.wait_for_text("BARRIER", TIMEOUT);
    }
}

#[test]
fn audible_bell_reaches_each_attached_client_once() {
    let (_scratch, tm, reference) = both();
    for mux in [&tm, &reference] {
        create_session(mux, BELL_PANE);
        let mut first = attach(mux, "bell-first");
        let mut second = attach(mux, "bell-second");
        let first_mark = first.mark();
        let second_mark = second.mark();
        ring_and_pass_barrier(mux, &[&first, &second]);
        assert_eq!(first.count_since(first_mark, b"\x07"), 1, "{:?}", mux.implementation());
        assert_eq!(second.count_since(second_mark, b"\x07"), 1, "{:?}", mux.implementation());
        first.detach();
        second.detach();
    }
}

#[test]
fn visual_bell_suppresses_the_outer_bell() {
    let (_scratch, tm, reference) = both();
    for mux in [&tm, &reference] {
        create_session(mux, BELL_PANE);
        mux.ok(&["set-option", "-g", "visual-bell", "on"]);
        let mut client = attach(mux, "visual");
        let mark = client.mark();
        ring_and_pass_barrier(mux, &[&client]);
        assert_eq!(client.count_since(mark, b"\x07"), 0, "{:?}", mux.implementation());
        client.detach();
    }
}

#[test]
fn application_osc52_is_ignored_in_external_mode() {
    let (_scratch, tm, reference) = both();
    for mux in [&tm, &reference] {
        create_session(
            mux,
            "read line; printf '\\033]52;c;YXBw\\007'; echo BELLED; read line; echo BARRIER; sleep 30",
        );
        let mut client = attach(mux, "osc52");
        let mark = client.mark();
        ring_and_pass_barrier(mux, &[&client]);
        assert_eq!(client.count_since(mark, b"\x1b]52;"), 0, "{:?}", mux.implementation());
        assert!(mux.ok(&["list-buffers"]).is_empty(), "{:?}", mux.implementation());
        client.detach();
    }
}

fn copy_first_line(mux: &Mux) {
    mux.ok(&["copy-mode", "-t", "S"]);
    for command in [
        "history-top",
        "begin-selection",
        "cursor-down",
        "copy-selection-and-cancel",
    ] {
        mux.ok(&["send-keys", "-t", "S", "-X", command]);
    }
}

#[test]
fn tm_copy_emits_osc52_once_per_capable_client() {
    let (_scratch, tm, reference) = both();
    for mux in [&tm, &reference] {
        create_session(mux, "printf 'alpha\\nbeta\\n'; sleep 30");
        let mut client = attach(mux, "copy");
        client.wait_for_text("beta", TIMEOUT);
        let mark = client.mark();
        copy_first_line(mux);
        let payloads = client.wait_for_stream(mark, "OSC 52 clipboard write", TIMEOUT, |stream| {
            Some(osc52_sequences(stream)).filter(|payloads| !payloads.is_empty())
        });
        assert_eq!(payloads, [b"\x1b]52;;YWxwaGEK\x07".to_vec()], "{:?}", mux.implementation());

        mux.ok(&["set-option", "-g", "set-clipboard", "off"]);
        let off_mark: Mark = client.mark();
        copy_first_line(mux);
        mux.ok(&["send-keys", "-t", "S", "BARRIER"]);
        client.wait_for_text("BARRIER", TIMEOUT);
        assert!(
            osc52_sequences(&client.bytes_since(off_mark)).is_empty(),
            "{:?}",
            mux.implementation()
        );
        client.detach();
    }
}
