#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
mod pty_harness;
#[path = "support/split_input.rs"]
#[allow(dead_code)]
mod split_input;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size};
use raw_pty::RawClient;

fn create_session(mux: &Mux, session: &str, windows: usize) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "one",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    for index in 1..windows {
        mux.ok(&[
            "new-window",
            "-d",
            "-t",
            session,
            "-n",
            &format!("w{index}"),
        ]);
    }
}

fn create_byte_session(mux: &Mux, session: &str, count: usize) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-x",
        "80",
        "-y",
        "24",
        "--",
        "sh",
        "-c",
        &format!("stty -echo raw; od -An -tx1 -N{count}; sleep 30"),
    ]);
}

fn attach(mux: &Mux, session: &str, name: &str) -> (PtyTest, ptytest::TerminalBaseline) {
    let size = Size::new(80, 24).expect("constant size");
    let mut terminal =
        pty_harness::spawn_attached(mux, name, &["attach-session", "-t", session], size);
    pty_harness::wait_for_text(&mut terminal, &format!("({session})"), Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn send(terminal: &mut PtyTest, bytes: &[u8]) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), bytes)
        .expect("send client bytes");
}

fn wait_hex(mux: &Mux, session: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let captured = mux.ok(&["capture-pane", "-p", "-t", session]);
        let actual = captured
            .split_whitespace()
            .filter(|token| token.len() == 2 && token.chars().all(|c| c.is_ascii_hexdigit()))
            .collect::<Vec<_>>()
            .join(" ");
        if actual == expected || Instant::now() >= deadline {
            assert_eq!(actual, expected, "pane bytes: {captured:?}");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn active_window(mux: &Mux, session: &str) -> String {
    mux.body(&[
        "display-message",
        "-p",
        "-t",
        session,
        "#{window_index}",
    ])
}

fn wait_window(mux: &Mux, session: &str, expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value = active_window(mux, session);
        if value == expected || Instant::now() >= deadline {
            return value;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn lone_escape_after_prefix_is_forwarded_once() {
    let scratch = Scratch::new("input-lone-escape");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 1);
    let (mut client, baseline) = attach(&tm, "S", "escape");
    send(&mut client, b"\x01");
    std::thread::sleep(Duration::from_millis(20));
    send(&mut client, b"\x1b");
    wait_hex(&tm, "S", "1b");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn unknown_key_after_prefix_drops_the_prefix() {
    let scratch = Scratch::new("input-unknown");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 1);
    let (mut client, baseline) = attach(&tm, "S", "unknown");
    send(&mut client, b"\x01q");
    wait_hex(&tm, "S", "71");
    pty_harness::detach(&mut client, &baseline);
}

fn prefix_bytes_reach_the_pane(mux: &Mux, label: &str, keys: &[u8], count: usize, expected: &str) {
    create_byte_session(mux, "S", count);
    let mut client = RawClient::spawn(mux, label, &["attach-session", "-t", "S"], 24, 80);
    client.wait_for_text("(S)", Duration::from_secs(3));
    client.write(keys);
    wait_hex(mux, "S", expected);
    assert_eq!(client.detach(), Some(0));
}

#[test]
fn literal_prefix_bindings_match_reference_bytes() {
    let scratch = Scratch::new("in-pfx");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        prefix_bytes_reach_the_pane(mux, "prefix-bytes", b"\x01\x13", 2, "01 01");
    }
    let scratch = Scratch::new("in-sp");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        prefix_bytes_reach_the_pane(mux, "send-prefix", b"\x01b", 1, "01");
    }
}

#[test]
fn cli_send_prefix_writes_the_configured_prefix_like_the_reference() {
    let scratch = Scratch::new("input-cli-send-prefix");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        create_byte_session(mux, "S", 3);
        mux.capture_until("S", "", Duration::from_secs(3));
    }
    parity::compare_commands(
        "cli_send_prefix_writes_the_configured_prefix_like_the_reference",
        &tm,
        &reference,
        &[
            &["send-prefix", "-t", "S"],
            &["set-option", "-g", "prefix", "C-b"],
            &["send-prefix", "-t", "S"],
            &["send-keys", "-t", "S", "x"],
        ],
    );
    for mux in [&tm, &reference] {
        wait_hex(mux, "S", "01 02 78");
    }
}

#[test]
fn cli_send_prefix_rejects_the_secondary_prefix_it_cannot_send() {
    let scratch = Scratch::new("input-cli-send-prefix-2");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        create_byte_session(mux, "S", 1);
    }
    assert_eq!(reference.body(&["show-options", "-g", "prefix2"]), "prefix2 None");
    let accepted = reference.run(&["send-prefix", "-2", "-t", "S"]);
    assert!(accepted.status.success());
    let rejected = tm.run(&["send-prefix", "-2", "-t", "S"]);
    assert!(!rejected.status.success());
    assert_eq!(
        String::from_utf8_lossy(&rejected.stderr).trim_end(),
        "tm: unsupported flag -2 for send-prefix"
    );
    for mux in [&tm, &reference] {
        mux.ok(&["send-keys", "-t", "S", "x"]);
        wait_hex(mux, "S", "78");
    }
}

#[test]
fn alt_and_incomplete_sequences_are_forwarded_once() {
    let scratch = Scratch::new("input-alt");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 2);
    let (mut client, baseline) = attach(&tm, "S", "alt");
    send(&mut client, b"\x1bx");
    wait_hex(&tm, "S", "1b 78");
    pty_harness::detach(&mut client, &baseline);

    let scratch = Scratch::new("input-incomplete");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 2);
    let (mut client, baseline) = attach(&tm, "S", "incomplete");
    send(&mut client, b"\x1b[");
    wait_hex(&tm, "S", "1b 5b");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn fragmented_prefix_binding_is_atomic_at_every_byte_boundary() {
    let scratch = Scratch::new("input-fragmented");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", 1);
    let (mut client, baseline) = attach(&tm, "S", "fragmented");
    for byte in b"\x01%" {
        send(&mut client, &[*byte]);
    }
    assert_eq!(
        wait_window(&tm, "S", "1"),
        "1",
        "fragmented binding changed the active window"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let panes = tm.body(&["display-message", "-p", "-t", "S", "#{window_panes}"]);
        if panes == "2" || Instant::now() >= deadline {
            assert_eq!(panes, "2");
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn repeat_deadline_and_switching_follow_the_500ms_contract() {
    let scratch = Scratch::new("input-repeat");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", 2);
    let (mut client, baseline) = attach(&tm, "S", "repeat");

    send(&mut client, b"\x01\x1b[1;5C");
    assert_eq!(wait_window(&tm, "S", "2"), "2");
    std::thread::sleep(Duration::from_millis(650));
    send(&mut client, b"\x1b[1;5C");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        active_window(&tm, "S"),
        "2",
        "expired repeat mode still executed the binding"
    );
    assert_eq!(
        wait_window(&tm, "S", "2"),
        "2",
        "expired repeat mode changed the window"
    );

    send(&mut client, b"\x01\x1b[1;5C");
    assert_eq!(wait_window(&tm, "S", "1"), "1");
    send(&mut client, b"\x1b[1;5D");
    assert_eq!(
        wait_window(&tm, "S", "2"),
        "2",
        "repeat mode did not switch to another repeatable key"
    );
    pty_harness::detach(&mut client, &baseline);
}

fn split_class(label: &str, modes: &str, inputs: &[&[u8]]) -> Vec<split_input::Observed> {
    let scratch = Scratch::new(label);
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    let mut per_input: Vec<Vec<split_input::Observed>> = Vec::new();
    for mux in [&tm, &reference] {
        let mut sink = split_input::Sink::start(mux, "S", "sink", modes);
        let mut client = split_input::attach(mux, "S");
        let observed: Vec<_> = inputs
            .iter()
            .map(|input| split_input::observe(&mut sink, &mut client, input))
            .collect();
        for (input, observed) in inputs.iter().zip(&observed) {
            split_input::assert_split_invariant(&format!("{label} {input:?}"), mux, observed);
        }
        assert_eq!(client.detach(), Some(0));
        per_input.push(observed);
    }
    let reference_observed = per_input.pop().expect("reference observations");
    let tm_observed = per_input.pop().expect("tm observations");
    for ((input, tm), reference) in inputs.iter().zip(&tm_observed).zip(&reference_observed) {
        assert_eq!(tm, reference, "{label} {input:?}: tm pane bytes differ from the reference");
    }
    tm_observed
}

#[test]
fn split_prefix_bindings_deliver_the_unsplit_pane_bytes() {
    let observed = split_class("in-split-prefix", "", &[b"\x01b", b"\x01\x13"]);
    assert_eq!(observed[0].unsplit, b"\x01");
    assert_eq!(observed[1].unsplit, b"\x01\x01");
}

#[test]
fn split_extended_keys_deliver_the_unsplit_pane_bytes() {
    let observed = split_class(
        "in-split-extended",
        "\\033[>4;2m",
        &[b"\x1b[98;5u\x1b[13;2u", b"\x1b[27;5;105~\x1b[27;6;66~"],
    );
    assert!(!observed[0].unsplit.is_empty());
    assert!(!observed[1].unsplit.is_empty());
}

#[test]
fn split_sgr_mouse_delivers_the_unsplit_pane_bytes() {
    let observed = split_class(
        "in-split-mouse",
        "\\033[?1000h\\033[?1006h",
        &[b"\x1b[<0;10;5M\x1b[<0;10;5m", b"\x1b[<64;3;4M"],
    );
    assert_eq!(observed[0].unsplit, b"\x1b[<0;10;5M\x1b[<0;10;5m");
    assert_eq!(observed[1].unsplit, b"\x1b[<64;3;4M");
}

#[test]
fn split_focus_events_deliver_the_unsplit_pane_bytes() {
    let observed = split_class("in-split-focus", "\\033[?1004h", &[b"\x1b[O\x1b[I"]);
    assert_eq!(observed[0].unsplit, b"\x1b[O\x1b[I");
}

#[test]
fn split_prefix_window_switch_follows_the_reference() {
    let scratch = Scratch::new("in-split-switch");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    let input: &[u8] = b"\x01\x1b[1;5C";
    for mux in [&tm, &reference] {
        create_session(mux, "S", 2);
        let mut client = RawClient::spawn(mux, "switch", &["attach-session", "-t", "S"], 24, 80);
        client.wait_for_text("(S)", Duration::from_secs(3));
        let first = mux.body(&["display-message", "-p", "-t", "S", "#{window_index}"]);
        let mut current: usize = first.parse().expect("window index");
        let next = |current: usize| if current == 1 { 2 } else { 1 };
        let mut deliveries: Vec<Vec<&[u8]>> = vec![vec![input]];
        deliveries.extend((1..input.len()).map(|at| {
            let (head, tail) = input.split_at(at);
            vec![head, tail]
        }));
        deliveries.push(input.chunks(1).collect());
        for chunks in &deliveries {
            for chunk in chunks {
                client.write(chunk);
            }
            current = next(current);
            assert_eq!(
                wait_window(mux, "S", &current.to_string()),
                current.to_string(),
                "{:?}: split {chunks:?} did not switch window",
                mux.implementation()
            );
        }
        assert_eq!(client.detach(), Some(0));
    }
}

// tmux 3.8 raises the partial-key delay to 500 ms while the outer terminal
// has not answered its DA2, XDA, DECRQM 2026 and OSC 10/11 queries, and the raw
// PTY runner answers only DA1, so the pause must exceed that raised delay.
const PAUSE_PAST_UNANSWERED_QUERY_ESCAPE_DELAY: Duration = Duration::from_millis(700);

#[test]
fn escape_then_a_delay_longer_than_escape_time_follows_the_reference() {
    let scratch = Scratch::new("in-escape-delay");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    let sequence: &[u8] = b"\x1b[27;5;105~";
    let mut observed = Vec::new();
    for mux in [&tm, &reference] {
        let mut sink = split_input::Sink::start(mux, "S", "sink", "");
        assert_eq!(mux.body(&["show-options", "-s", "escape-time"]), "escape-time 0");
        let mut client = split_input::attach(mux, "S");
        let unsplit = sink.deliver(&mut client, &[sequence], "unsplit");
        let delayed = sink.deliver_paused(
            &mut client,
            &sequence[..1],
            PAUSE_PAST_UNANSWERED_QUERY_ESCAPE_DELAY,
            &sequence[1..],
        );
        assert_eq!(client.detach(), Some(0));
        observed.push((unsplit, delayed));
    }
    assert_eq!(observed[0].0, b"\t");
    assert_eq!(observed[0].1, sequence, "a delayed Escape is forwarded on its own");
    assert_eq!(observed[0], observed[1]);
}

#[test]
fn focus_in_reaches_a_requesting_pane_when_a_client_attaches() {
    let scratch = Scratch::new("in-attach-focus");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        let mut sink = split_input::Sink::start(mux, "S", "sink", "\\033[?1004h");
        let mut client = split_input::attach(mux, "S");
        assert_eq!(
            sink.deliver(&mut client, &[], "attach"),
            b"\x1b[I",
            "{:?}",
            mux.implementation()
        );
        assert_eq!(client.detach(), Some(0));
    }
}
