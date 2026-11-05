#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::time::{Duration, Instant};

use parity::{Mux, Scratch, compare_commands, observation_difference};

const TIMEOUT: Duration = Duration::from_secs(5);

struct Pair {
    tm: Mux,
    reference: Mux,
    _scratch: Scratch,
}

impl Pair {
    fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let tm = Mux::tm(&scratch, "tm");
        let reference = Mux::reference(&scratch, "reference");
        Self {
            tm,
            reference,
            _scratch: scratch,
        }
    }

    fn both(&self) -> [&Mux; 2] {
        [&self.tm, &self.reference]
    }

    fn start(&self, session: &str) {
        for mux in self.both() {
            mux.ok(&["new-session", "-d", "-s", session, "--", "sleep", "300"]);
        }
    }

    fn step(&self, test: &str, arguments: &[&str]) {
        compare_commands(test, &self.tm, &self.reference, &[arguments]);
        self.assert_same_topology(test, arguments);
    }

    fn assert_same_topology(&self, test: &str, after: &[&str]) {
        assert_eq!(
            observation_difference(
                &format!("{test}: topology after {after:?}"),
                &topology(&self.tm),
                &topology(&self.reference)
            ),
            None
        );
    }
}

fn sorted_lines(mux: &Mux, arguments: &[&str]) -> Vec<String> {
    let output = mux.run(arguments);
    if !output.status.success() {
        return vec!["<failed>".to_owned()];
    }
    let mut lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

fn topology(mux: &Mux) -> String {
    let names = sorted_lines(mux, &["list-sessions", "-F", "#{session_name}"]);
    let mut text = format!(
        "sessions {:?}\n",
        sorted_lines(
            mux,
            &[
                "list-sessions",
                "-F",
                "#{session_name} #{session_windows} #{session_attached} #{session_grouped} #{?session_grouped,#{session_group_size},-}",
            ],
        )
    );
    for name in names.iter().filter(|name| *name != "<failed>") {
        text.push_str(&format!(
            "windows {name} {:?}\n",
            sorted_lines(
                mux,
                &[
                    "list-windows",
                    "-t",
                    name,
                    "-F",
                    "#{window_index} #{window_width}x#{window_height} #{window_panes}",
                ],
            )
        ));
    }
    text.push_str(&format!(
        "clients {:?}\n",
        sorted_lines(
            mux,
            &[
                "list-clients",
                "-F",
                "#{client_session} #{client_width}x#{client_height}"
            ]
        )
    ));
    text
}

fn client_rows(mux: &Mux) -> Vec<(String, String)> {
    mux.body(&["list-clients", "-F", "#{client_name} #{client_session}"])
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(name, session)| (name.to_owned(), session.to_owned()))
        .collect()
}

fn wait_clients(mux: &Mux, expected: usize) -> Vec<(String, String)> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let rows = client_rows(mux);
        if rows.len() == expected || Instant::now() >= deadline {
            assert_eq!(rows.len(), expected, "{rows:?}");
            return rows;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn client_of(mux: &Mux, session: &str) -> String {
    client_rows(mux)
        .into_iter()
        .find(|(_, attached)| attached == session)
        .unwrap_or_else(|| panic!("no client attached to {session}"))
        .0
}

fn attach(mux: &Mux, label: &str, session: &str) -> raw_pty::RawClient {
    let client = raw_pty::RawClient::spawn(
        mux,
        label,
        &["attach-session", "-t", session],
        10,
        60,
    );
    client.wait_for_text(&format!("({session})"), TIMEOUT);
    client
}

fn assert_rejected(mux: &Mux, command: &str, arguments: &[&str], flag: &str) {
    let before = topology(mux);
    let output = mux.run(arguments);
    assert!(!output.status.success(), "{arguments:?} succeeded");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("tm: unsupported flag {flag} for {command}\n"),
        "{arguments:?}"
    );
    assert!(output.stdout.is_empty(), "{arguments:?}");
    assert_eq!(topology(mux), before, "{arguments:?} changed the topology");
}

fn buffer_observation(mux: &Mux) -> Vec<String> {
    let listing = mux.body(&["list-buffers", "-F", "#{buffer_name} #{buffer_size}"]);
    let mut observed = vec![listing.clone()];
    for name in listing.lines().filter_map(|line| line.split(' ').next()) {
        let path = mux.root().join(format!("saved-{name}"));
        mux.ok(&["save-buffer", "-b", name, &path.to_string_lossy()]);
        observed.push(format!(
            "{name}: {:?}",
            String::from_utf8_lossy(&std::fs::read(&path).expect("saved buffer"))
        ));
    }
    observed
}

fn assert_same_buffers(test: &str, pair: &Pair, after: &[&str]) {
    assert_eq!(
        observation_difference(
            &format!("{test}: buffers after {after:?}"),
            &buffer_observation(&pair.tm),
            &buffer_observation(&pair.reference)
        ),
        None
    );
}


struct Paster<'a> {
    mux: &'a Mux,
    file: std::path::PathBuf,
    seen: usize,
}

impl<'a> Paster<'a> {
    fn new(mux: &'a Mux) -> Self {
        let file = mux.root().join("pasted");
        let script = format!(
            "printf '\\033[?2004h'; stty raw -echo; echo ready; exec cat > {}",
            file.display()
        );
        mux.ok(&["new-session", "-d", "-s", "P", "-x", "40", "-y", "6", "--", "sh", "-c", &script]);
        assert!(mux.capture_until("P", "ready", TIMEOUT).contains("ready"));
        Self { mux, file, seen: 0 }
    }

    fn pasted_by(&mut self, arguments: &[&str]) -> (parity::Outcome, Vec<u8>) {
        let outcome = self.mux.outcome(arguments);
        self.mux.ok(&["send-keys", "-t", "P", "-l", "Z"]);
        let deadline = Instant::now() + TIMEOUT;
        let bytes = loop {
            let bytes = std::fs::read(&self.file).unwrap_or_default();
            if (bytes.len() > self.seen && bytes.ends_with(b"Z")) || Instant::now() >= deadline {
                break bytes;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let delta = bytes[self.seen.min(bytes.len())..].to_vec();
        self.seen = bytes.len();
        (outcome, delta)
    }
}

fn pane_input_steps(test: &str, seed: &[&[&str]], steps: &[&[&str]]) {
    let pair = Pair::new("pasteb");
    let mut tm = Paster::new(&pair.tm);
    let mut reference = Paster::new(&pair.reference);
    for arguments in seed {
        compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
    }
    for arguments in steps {
        let (tm_outcome, tm_bytes) = tm.pasted_by(arguments);
        let (reference_outcome, reference_bytes) = reference.pasted_by(arguments);
        assert_eq!(
            parity::first_difference(&tm_outcome, &reference_outcome),
            None,
            "{test}: {arguments:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&tm_bytes),
            String::from_utf8_lossy(&reference_bytes),
            "{test}: bytes pasted by {arguments:?}"
        );
        assert_same_buffers(test, &pair, arguments);
    }
}

fn content_pair(label: &str) -> Pair {
    let pair = Pair::new(label);
    for mux in pair.both() {
        mux.ok(&[
            "new-session", "-d", "-s", "S", "-x", "30", "-y", "6", "--", "sh", "-c",
            "printf 'one\\ntwo  \\n\\033[31mred\\033[0m\\nlong long long long long long long long long\\nready\\n'; sleep 300",
        ]);
        assert!(mux.capture_until("S", "ready", TIMEOUT).contains("ready"));
    }
    pair
}

mod sessions {
    use super::*;

    #[test]
    fn sessions_new_session_flags_match_the_reference() {
        let test = "sessions_new_session_flags_match_the_reference";
        let pair = Pair::new("newses");
        pair.step(test, &["new-session", "-d", "-s", "a", "--", "sleep", "300"]);
        pair.step(
            test,
            &["new-session", "-d", "-s", "b", "-x", "100", "-y", "30", "--", "sleep", "300"],
        );
        pair.step(test, &["new-session", "-ds", "c", "--", "sleep", "300"]);
        pair.step(test, &["new-session", "-d", "-sd", "--", "sleep", "300"]);
        pair.step(test, &["new-session", "-d", "-P", "-s", "e", "--", "sleep", "300"]);
        pair.step(
            test,
            &[
                "new-session", "-d", "-P", "-F", "#{session_name} #{session_windows}", "-s", "f",
                "--", "sleep", "300",
            ],
        );
        pair.step(
            test,
            &["new-session", "-d", "-F", "#{session_name}", "-s", "g", "--", "sleep", "300"],
        );
        pair.step(test, &["new-session", "-d", "-n", "win", "-s", "h", "--", "sleep", "300"]);
        compare_commands(
            test,
            &pair.tm,
            &pair.reference,
            &[&["list-windows", "-t", "h", "-F", "#{window_name}"]],
        );
        pair.step(test, &["new-session", "-d", "-c", "/tmp", "-s", "i", "--", "sleep", "300"]);
        compare_commands(
            test,
            &pair.tm,
            &pair.reference,
            &[&["display-message", "-p", "-t", "i", "#{pane_current_path}"]],
        );
        pair.step(test, &["new-session", "-d", "-E", "-s", "j", "--", "sleep", "300"]);
        pair.step(test, &["new-session", "-d", "-t", "a", "-s", "k"]);
        pair.step(test, &["new-session", "-d", "-s", "a"]);
        pair.step(test, &["new-session", "-A", "-d", "-s", "l", "--", "sleep", "300"]);
        pair.step(test, &["new-session", "-A", "-d", "-s", "a"]);
        pair.step(test, &["new-session", "-s", "m"]);
        pair.step(test, &["new-session", "-A", "-s", "n"]);
    }

    #[test]
    fn sessions_new_session_group_target_must_name_a_session() {
        let pair = Pair::new("newsesgro");
        pair.start("a");
        let before = topology(&pair.tm);
        let output = pair.tm.run(&["new-session", "-d", "-t", "nosuch", "-s", "b"]);
        assert!(!output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "tm: can't find session: nosuch\n"
        );
        assert_eq!(topology(&pair.tm), before);
        assert!(pair.reference.run(&["new-session", "-d", "-t", "nosuch", "-s", "b"]).status.success());
    }

    fn observe_attach(mux: &Mux, label: &str, arguments: &[&str]) -> (Vec<String>, String) {
        let mut client = raw_pty::RawClient::spawn(mux, label, arguments, 10, 60);
        client.wait_for_text("(S)", TIMEOUT);
        let rows = wait_clients(mux, 1);
        let attached = rows.iter().map(|(_, session)| session.clone()).collect();
        let topology = topology(mux);
        assert_eq!(client.detach(), Some(0));
        wait_clients(mux, 0);
        (attached, topology)
    }

    #[test]
    fn sessions_new_session_attach_forms_match_the_reference() {
        let test = "sessions_new_session_attach_forms_match_the_reference";
        for (label, arguments) in [
            ("no -d creates and attaches", &["new-session", "-s", "S", "--", "sleep", "300"][..]),
            ("-A without -d attaches an existing session", &["new-session", "-A", "-s", "S"][..]),
            ("-A with -d still attaches an existing session", &["new-session", "-A", "-d", "-s", "S"][..]),
        ] {
            let pair = Pair::new("newsesatt");
            if !arguments.contains(&"--") {
                pair.start("S");
            }
            let tm = observe_attach(&pair.tm, "tm", arguments);
            let reference = observe_attach(&pair.reference, "reference", arguments);
            assert_eq!(
                observation_difference(&format!("{test}: {label}"), &tm, &reference),
                None
            );
            assert_eq!(tm.0, vec!["S".to_owned()]);
        }
    }

    #[test]
    fn sessions_new_session_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("newsesrej");
        pair.start("a");
        for (flag, arguments) in [
            ("-D", &["new-session", "-D", "-d", "-s", "b"][..]),
            ("-X", &["new-session", "-X", "-d", "-s", "b"][..]),
            ("-e", &["new-session", "-d", "-e", "A=1", "-s", "b"][..]),
            ("-f", &["new-session", "-d", "-f", "read-only", "-s", "b"][..]),
            ("-A", &["new-session", "-A", "-d"][..]),
        ] {
            assert_rejected(&pair.tm, "new-session", arguments, flag);
        }
    }

    #[test]
    fn sessions_attach_session_flags_match_the_reference() {
        let test = "sessions_attach_session_flags_match_the_reference";
        let pair = Pair::new("att");
        pair.start("S");
        pair.start("T");
        for arguments in [
            &["attach-session", "-t", "S"][..],
            &["attach-session", "-E", "-t", "S"][..],
            &["attach-session", "-t", "nosuch"][..],
            &["attach-session", "-t", "S", "extra"][..],
        ] {
            pair.step(test, arguments);
        }
        let observe = |mux: &Mux, arguments: &[&str]| {
            let mut client = raw_pty::RawClient::spawn(mux, "attach", arguments, 10, 60);
            client.wait_for_text("(S)", TIMEOUT);
            wait_clients(mux, 1);
            let topology = topology(mux);
            assert_eq!(client.detach(), Some(0));
            wait_clients(mux, 0);
            topology
        };
        for arguments in [
            &["attach-session", "-t", "S"][..],
            &["attach-session", "-E", "-t", "S"][..],
            &["attach-session", "-t", "=S"][..],
        ] {
            assert_eq!(
                observation_difference(
                    &format!("{test}: attached topology for {arguments:?}"),
                    &observe(&pair.tm, arguments),
                    &observe(&pair.reference, arguments)
                ),
                None
            );
        }
    }

    #[test]
    fn sessions_attach_session_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("attrej");
        pair.start("S");
        for (flag, arguments) in [
            ("-d", &["attach-session", "-d", "-t", "S"][..]),
            ("-r", &["attach-session", "-r", "-t", "S"][..]),
            ("-c", &["attach-session", "-c", "/tmp", "-t", "S"][..]),
            ("-f", &["attach-session", "-f", "read-only", "-t", "S"][..]),
            ("-x", &["attach-session", "-x", "-t", "S"][..]),
            ("-y", &["attach-session", "-y", "10", "-t", "S"][..]),
        ] {
            assert_rejected(&pair.tm, "attach-session", arguments, flag);
        }
    }

    #[test]
    fn sessions_detach_client_flags_match_the_reference() {
        let test = "sessions_detach_client_flags_match_the_reference";
        let pair = Pair::new("det");
        pair.start("A");
        pair.start("B");
        pair.step(test, &["detach-client"]);
        pair.step(test, &["detach-client", "-t", "nosuch"]);
        pair.step(test, &["detach-client", "-a"]);

        let observe = |mux: &Mux| {
            let mut first = attach(mux, "first", "A");
            wait_clients(mux, 1);
            let mut second = attach(mux, "second", "B");
            wait_clients(mux, 2);
            let first_name = client_of(mux, "A");

            mux.ok(&["detach-client"]);
            let mut seen = vec![format!("default {:?}", second.finish(TIMEOUT))];
            wait_clients(mux, 1);
            seen.push(format!("left {:?}", client_rows(mux).into_iter().map(|row| row.1).collect::<Vec<_>>()));

            let mut second = attach(mux, "second", "B");
            wait_clients(mux, 2);
            mux.ok(&["detach-client", "-t", &first_name]);
            seen.push(format!("-t {:?}", first.finish(TIMEOUT)));
            seen.push(format!("left {:?}", wait_clients(mux, 1).into_iter().map(|row| row.1).collect::<Vec<_>>()));

            let mut third = attach(mux, "third", "A");
            wait_clients(mux, 2);
            let kept = client_of(mux, "B");
            mux.ok(&["detach-client", "-a", "-t", &kept]);
            seen.push(format!("-a {:?}", third.finish(TIMEOUT)));
            seen.push(format!("left {:?}", wait_clients(mux, 1).into_iter().map(|row| row.1).collect::<Vec<_>>()));
            mux.ok(&["detach-client", "-a"]);
            wait_clients(mux, 1);
            second.detach();
            seen
        };
        assert_eq!(
            observation_difference("detach-client effects", &observe(&pair.tm), &observe(&pair.reference)),
            None
        );
    }

    #[test]
    fn sessions_detach_client_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("detrej");
        pair.start("A");
        let mut client = attach(&pair.tm, "client", "A");
        wait_clients(&pair.tm, 1);
        for (flag, arguments) in [
            ("-s", &["detach-client", "-s", "A"][..]),
            ("-E", &["detach-client", "-E", "true"][..]),
            ("-P", &["detach-client", "-P"][..]),
        ] {
            assert_rejected(&pair.tm, "detach-client", arguments, flag);
        }
        assert_eq!(wait_clients(&pair.tm, 1).len(), 1);
        client.detach();
    }

    #[test]
    fn sessions_switch_client_flags_match_the_reference() {
        let test = "sessions_switch_client_flags_match_the_reference";
        let pair = Pair::new("swi");
        pair.start("A");
        pair.start("B");
        pair.step(test, &["switch-client", "-t", "B"]);
        let observe = |mux: &Mux| {
            let mut client = attach(mux, "client", "A");
            let name = wait_clients(mux, 1).remove(0).0;
            let mut seen = Vec::new();
            for arguments in [
                vec!["switch-client", "-t", "B"],
                vec!["switch-client", "-t", "=A"],
                vec!["switch-client", "-c", &name, "-t", "B"],
                vec!["switch-client", "-t", "nosuch"],
                vec!["switch-client", "-c", "nosuch", "-t", "A"],
            ] {
                let output = mux.outcome(&arguments);
                let sessions = wait_clients(mux, 1).remove(0).1;
                seen.push(format!(
                    "{:?} {:?} {:?} -> {sessions}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr).replace(&name, "<client>")
                ));
            }
            client.detach();
            seen
        };
        let tm = observe(&pair.tm);
        let reference = observe(&pair.reference);
        let tm = tm.iter().map(|line| line.replace("tm: ", "")).collect::<Vec<_>>();
        assert_eq!(observation_difference("switch-client effects", &tm, &reference), None);
    }

    #[test]
    fn sessions_switch_client_rejects_flags_and_targets_it_does_not_implement() {
        let pair = Pair::new("swirej");
        pair.start("A");
        pair.start("B");
        let mut client = attach(&pair.tm, "client", "A");
        wait_clients(&pair.tm, 1);
        for (flag, arguments) in [
            ("-E", &["switch-client", "-E", "-t", "B"][..]),
            ("-F", &["switch-client", "-F", "-t", "B"][..]),
            ("-l", &["switch-client", "-l"][..]),
            ("-n", &["switch-client", "-n"][..]),
            ("-p", &["switch-client", "-p"][..]),
            ("-r", &["switch-client", "-r", "-t", "B"][..]),
            ("-T", &["switch-client", "-T", "root", "-t", "B"][..]),
            ("-Z", &["switch-client", "-Z", "-t", "B"][..]),
            ("-O", &["switch-client", "-O", "name", "-t", "B"][..]),
        ] {
            assert_rejected(&pair.tm, "switch-client", arguments, flag);
        }
        for target in ["B:0", "B:0.0", "%0", "="] {
            let before = topology(&pair.tm);
            let output = pair.tm.run(&["switch-client", "-t", target]);
            assert!(!output.status.success());
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported target {target} for switch-client\n")
            );
            assert_eq!(topology(&pair.tm), before);
        }
        client.detach();
    }

    #[test]
    fn sessions_refresh_client_flags_match_the_reference() {
        let test = "sessions_refresh_client_flags_match_the_reference";
        let pair = Pair::new("ref");
        pair.start("A");
        pair.start("B");
        pair.step(test, &["refresh-client"]);
        pair.step(test, &["refresh-client", "-t", "nosuch"]);
        let observe = |mux: &Mux| {
            let mut first = attach(mux, "first", "A");
            wait_clients(mux, 1);
            let mut second = attach(mux, "second", "B");
            wait_clients(mux, 2);
            let first_name = client_of(mux, "A");
            let mut seen = Vec::new();
            for (arguments, expects_first) in [
                (vec!["refresh-client"], false),
                (vec!["refresh-client", "-t", &first_name], true),
            ] {
                let marks = (first.mark(), second.mark());
                let screens = (first.screen_text(), second.screen_text());
                mux.ok(&arguments);
                let (expected, other) = if expects_first {
                    (&first, &second)
                } else {
                    (&second, &first)
                };
                let (expected_mark, other_mark) = if expects_first {
                    (marks.0, marks.1)
                } else {
                    (marks.1, marks.0)
                };
                expected.wait_for_stream(expected_mark, "redraw", TIMEOUT, |bytes| {
                    (!bytes.is_empty()).then_some(())
                });
                mux.ok(&["list-clients"]);
                seen.push(
                    format!(
                        "{arguments:?}: other client redrawn={} screens unchanged={}",
                        !other.bytes_since(other_mark).is_empty(),
                        screens == (first.screen_text(), second.screen_text())
                    )
                    .replace(&first_name, "<client>"),
                );
            }
            first.detach();
            second.detach();
            seen
        };
        assert_eq!(
            observation_difference("refresh-client effects", &observe(&pair.tm), &observe(&pair.reference)),
            None
        );
    }

    #[test]
    fn sessions_refresh_client_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("refrej");
        pair.start("A");
        let mut client = attach(&pair.tm, "client", "A");
        wait_clients(&pair.tm, 1);
        for (flag, arguments) in [
            ("-S", &["refresh-client", "-S"][..]),
            ("-l", &["refresh-client", "-l"][..]),
            ("-r", &["refresh-client", "-r", "%0:x"][..]),
            ("-c", &["refresh-client", "-c"][..]),
            ("-C", &["refresh-client", "-C", "80x24"][..]),
            ("-f", &["refresh-client", "-f", "no-output"][..]),
            ("-F", &["refresh-client", "-F", "no-output"][..]),
            ("-A", &["refresh-client", "-A", "%0:on"][..]),
            ("-B", &["refresh-client", "-B", "n::x"][..]),
            ("-D", &["refresh-client", "-D"][..]),
            ("-L", &["refresh-client", "-L"][..]),
            ("-R", &["refresh-client", "-R"][..]),
            ("-U", &["refresh-client", "-U"][..]),
        ] {
            assert_rejected(&pair.tm, "refresh-client", arguments, flag);
        }
        client.detach();
    }

    #[test]
    fn sessions_list_clients_flags_match_the_reference() {
        let test = "sessions_list_clients_flags_match_the_reference";
        let pair = Pair::new("liscli");
        pair.start("A");
        pair.start("B");
        pair.step(test, &["list-clients", "-F", "#{client_session}"]);
        pair.step(test, &["list-clients", "-F"]);
        let observe = |mux: &Mux| {
            let mut first = attach(mux, "first", "A");
            wait_clients(mux, 1);
            let mut second = attach(mux, "second", "B");
            wait_clients(mux, 2);
            let format = "#{client_session} #{client_width}x#{client_height} #{client_termname}";
            let seen = (
                sorted_lines(mux, &["list-clients", "-F", format]),
                sorted_lines(mux, &["list-clients", "-F", "#{client_session}"]),
            );
            first.detach();
            second.detach();
            seen
        };
        assert_eq!(
            observation_difference("list-clients output", &observe(&pair.tm), &observe(&pair.reference)),
            None
        );
    }

    #[test]
    fn sessions_list_clients_names_and_default_format_are_tm_only() {
        let pair = Pair::new("lisclitmon");
        pair.start("A");
        let mut tm_client = attach(&pair.tm, "tm", "A");
        let mut reference_client = attach(&pair.reference, "reference", "A");
        let tm_row = pair.tm.body(&["list-clients"]);
        let reference_row = pair.reference.body(&["list-clients"]);
        assert!(tm_row.starts_with("client") && tm_row.ends_with(": A"), "{tm_row}");
        assert!(reference_row.starts_with("/dev/") && reference_row.contains(": A ["), "{reference_row}");
        tm_client.detach();
        reference_client.detach();
    }

    #[test]
    fn sessions_list_clients_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("lisclirej");
        pair.start("A");
        let mut client = attach(&pair.tm, "client", "A");
        wait_clients(&pair.tm, 1);
        for (flag, arguments) in [
            ("-f", &["list-clients", "-f", "1"][..]),
            ("-O", &["list-clients", "-O", "name"][..]),
            ("-r", &["list-clients", "-r"][..]),
            ("-t", &["list-clients", "-t", "A"][..]),
        ] {
            assert_rejected(&pair.tm, "list-clients", arguments, flag);
        }
        client.detach();
    }

    #[test]
    fn sessions_list_sessions_flags_match_the_reference() {
        let test = "sessions_list_sessions_flags_match_the_reference";
        let pair = Pair::new("lisses");
        pair.start("a");
        pair.start("b");
        pair.step(test, &["list-sessions", "-F", "#{session_name} #{session_windows} #{session_attached}"]);
        pair.step(test, &["list-sessions", "-F", "#{session_id}"]);
        pair.step(test, &["list-sessions", "-F"]);
        pair.step(test, &["list-sessions", "extra"]);
        let mut client = attach(&pair.tm, "tm", "a");
        let mut reference_client = attach(&pair.reference, "reference", "a");
        wait_clients(&pair.tm, 1);
        wait_clients(&pair.reference, 1);
        pair.step(test, &["list-sessions", "-F", "#{session_name} #{session_attached}"]);
        client.detach();
        reference_client.detach();
    }

    #[test]
    fn sessions_list_sessions_default_format_is_a_tm_only_prefix() {
        let pair = Pair::new("lissesdef");
        pair.start("a");
        let tm = pair.tm.body(&["list-sessions"]);
        let reference = pair.reference.body(&["list-sessions"]);
        assert_eq!(tm, "a: 1 windows");
        assert!(reference.starts_with("a: 1 windows (created "), "{reference}");
    }

    #[test]
    fn sessions_list_sessions_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("lissesrej");
        pair.start("a");
        for (flag, arguments) in [
            ("-f", &["list-sessions", "-f", "1"][..]),
            ("-a", &["list-sessions", "-a"][..]),
            ("-r", &["list-sessions", "-r"][..]),
            ("-O", &["list-sessions", "-O", "name"][..]),
        ] {
            assert_rejected(&pair.tm, "list-sessions", arguments, flag);
        }
    }

    #[test]
    fn sessions_has_session_flags_match_the_reference() {
        let test = "sessions_has_session_flags_match_the_reference";
        let pair = Pair::new("hasses");
        pair.start("alpha");
        pair.start("beta");
        for arguments in [
            &["has-session", "-t", "alpha"][..],
            &["has-session", "-t", "=alpha"][..],
            &["has-session", "-t", "alp"][..],
            &["has-session", "-t", "nosuch"][..],
            &["has-session", "-t"][..],
            &["has-session"][..],
            &["has-session", "alpha"][..],
        ] {
            pair.step(test, arguments);
        }
    }

    #[test]
    fn sessions_has_session_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("hassesrej");
        pair.start("a");
        assert_rejected(&pair.tm, "has-session", &["has-session", "-X", "-t", "a"], "-X");
    }

    #[test]
    fn sessions_kill_session_flags_match_the_reference() {
        let test = "sessions_kill_session_flags_match_the_reference";
        let pair = Pair::new("kilses");
        for name in ["a", "b", "c", "d"] {
            pair.start(name);
        }
        pair.step(test, &["kill-session", "-t", "nosuch"]);
        pair.step(test, &["kill-session", "-t", "b"]);
        pair.step(test, &["kill-session", "-t", "=c"]);
        pair.step(test, &["kill-session", "-t"]);
        pair.step(test, &["kill-session", "a"]);
        pair.start("e");
        pair.step(test, &["kill-session", "-a", "-t", "e"]);
        pair.step(test, &["kill-session", "-a", "-t", "nosuch"]);
    }

    #[test]
    fn sessions_kill_session_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("kilsesrej");
        pair.start("a");
        pair.start("b");
        for (flag, arguments) in [
            ("-C", &["kill-session", "-C", "-t", "a"][..]),
            ("-f", &["kill-session", "-f", "1", "-a", "-t", "a"][..]),
            ("-g", &["kill-session", "-g", "-t", "a"][..]),
        ] {
            assert_rejected(&pair.tm, "kill-session", arguments, flag);
        }
    }

    #[test]
    fn sessions_rename_session_flags_match_the_reference() {
        let test = "sessions_rename_session_flags_match_the_reference";
        let pair = Pair::new("renses");
        pair.start("a");
        pair.start("b");
        pair.step(test, &["rename-session", "-t", "a", "z"]);
        pair.step(test, &["rename-session", "-t", "b", "z"]);
        pair.step(test, &["rename-session", "-t", "nosuch", "y"]);
        pair.step(test, &["rename-session", "-t", "b", "--", "-dash"]);
        pair.step(test, &["rename-session", "-t", "z", "one", "two"]);
        pair.step(test, &["rename-session", "-t", "z"]);
        pair.step(test, &["rename-session", "-tz", "w"]);
    }

    #[test]
    fn sessions_rename_session_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("rensesrej");
        pair.start("a");
        assert_rejected(&pair.tm, "rename-session", &["rename-session", "-g", "-t", "a", "b"], "-g");
    }

    #[test]
    fn sessions_kill_server_accepts_no_flags() {
        let test = "sessions_kill_server_accepts_no_flags";
        let pair = Pair::new("kilser");
        pair.start("a");
        pair.step(test, &["kill-server", "extra"]);
        assert_rejected(&pair.tm, "kill-server", &["kill-server", "-x"], "-x");
        pair.start("b");
        for mux in pair.both() {
            mux.ok(&["kill-server"]);
            assert!(!mux.run(&["has-session", "-t", "a"]).status.success());
        }
    }

    #[test]
    fn sessions_default_session_is_the_most_recently_active_like_the_reference() {
        let test = "sessions_default_session_is_the_most_recently_active_like_the_reference";
        let pair = Pair::new("defses");
        for name in ["a", "b", "c"] {
            pair.start(name);
        }
        pair.step(test, &["rename-session", "newest"]);
        pair.step(test, &["has-session"]);
        pair.step(test, &["new-window", "-d"]);

        let observe = |mux: &Mux| {
            let mut seen = Vec::new();
            let mut first = attach(mux, "first", "a");
            wait_clients(mux, 1);
            first.detach();
            wait_clients(mux, 0);
            mux.ok(&["rename-session", "recent"]);
            seen.push(sorted_lines(mux, &["list-sessions", "-F", "#{session_name}"]));

            let mut second = raw_pty::RawClient::spawn(mux, "default", &["attach-session"], 10, 60);
            let attached = wait_clients(mux, 1).remove(0).1;
            seen.push(vec![format!("attached to {attached}")]);
            let mut third = raw_pty::RawClient::spawn(mux, "default2", &["attach-session"], 10, 60);
            let rows = wait_clients(mux, 2);
            let mut sessions = rows.into_iter().map(|row| row.1).collect::<Vec<_>>();
            sessions.sort();
            seen.push(sessions);
            second.detach();
            third.detach();
            wait_clients(mux, 0);
            seen
        };
        assert_eq!(
            observation_difference(test, &observe(&pair.tm), &observe(&pair.reference)),
            None
        );
    }

    #[test]
    fn sessions_tm_only_aliases_run_the_canonical_command() {
        let pair = Pair::new("ali");
        pair.start("a");
        pair.start("b");
        for (alias, canonical, arguments) in [
            ("a", "attach-session", vec!["-t", "a"]),
            ("kill", "kill-session", vec!["-t", "nosuch"]),
            ("refreshc", "refresh-client", vec![]),
        ] {
            let through_alias = pair.tm.outcome(&[&[alias][..], &arguments].concat());
            let through_name = pair.tm.outcome(&[&[canonical][..], &arguments].concat());
            assert_eq!(through_alias, through_name, "{alias}");
            assert!(
                !pair.reference.run(&[&[alias][..], &arguments].concat()).status.success(),
                "tmux accepted {alias}"
            );
        }
        compare_commands(
            "sessions_tm_only_aliases_run_the_canonical_command",
            &pair.tm,
            &pair.reference,
            &[&["refresh"]],
        );
        pair.tm.ok(&["kill", "-t", "b"]);
        assert_eq!(sorted_lines(&pair.tm, &["list-sessions", "-F", "#{session_name}"]), ["a"]);
    }
}

mod windows {
    use super::*;

    fn fixture(label: &str) -> Pair {
        let pair = Pair::new(label);
        pair.start("S");
        for mux in pair.both() {
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "two", "--", "sleep", "300"]);
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "three", "--", "sleep", "300"]);
            mux.ok(&["split-window", "-d", "-l", "11", "-t", "S:two", "--", "sleep", "300"]);
        }
        pair.start("T");
        pair
    }

    fn window_topology(mux: &Mux) -> String {
        let mut text = String::new();
        for session in sorted_lines(mux, &["list-sessions", "-F", "#{session_name}"]) {
            let rows = sorted_lines(
                mux,
                &[
                    "list-windows",
                    "-t",
                    &session,
                    "-F",
                    "#{window_index} #{window_name} #{window_panes} #{window_width}x#{window_height} #{window_active}",
                ],
            );
            text.push_str(&format!("windows {session} {rows:?}\n"));
            for index in mux
                .body(&["list-windows", "-t", &session, "-F", "#{window_index}"])
                .lines()
            {
                let target = format!("{session}:{index}");
                text.push_str(&format!(
                    "panes {target} {:?}\n",
                    sorted_lines(
                        mux,
                        &[
                            "list-panes",
                            "-t",
                            &target,
                            "-F",
                            "#{pane_index} #{pane_width}x#{pane_height} #{pane_active}",
                        ],
                    )
                ));
            }
        }
        text
    }

    fn probe(test: &str, label: &str, steps: &[&[&str]]) {
        let pair = fixture("win");
        for arguments in steps {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
            assert_eq!(
                observation_difference(
                    &format!("{test}: {label}: topology after {arguments:?}"),
                    &window_topology(&pair.tm),
                    &window_topology(&pair.reference)
                ),
                None
            );
        }
    }

    fn probes(test: &str, cases: &[&[&[&str]]]) {
        let mut failed = Vec::new();
        for steps in cases {
            if std::panic::catch_unwind(|| probe(test, &format!("{steps:?}"), steps)).is_err() {
                failed.push(format!("{steps:?}"));
            }
        }
        assert!(failed.is_empty(), "FAILED CASES:\n{}", failed.join("\n"));
    }

    fn assert_window_rejected(command: &str, flag: &str, arguments: &[&str]) {
        let pair = fixture("winrej");
        let before = window_topology(&pair.tm);
        let output = pair.tm.run(arguments);
        assert!(!output.status.success(), "{arguments:?} succeeded");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("tm: unsupported flag {flag} for {command}\n"),
            "{arguments:?}"
        );
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert_eq!(window_topology(&pair.tm), before, "{arguments:?} changed the topology");
    }

    #[test]
    fn windows_new_window_flags_match_the_reference() {
        let test = "windows_new_window_flags_match_the_reference";
        probes(
            test,
            &[
                &[&["new-window", "-d", "-t", "S"]],
                &[&["new-window", "-t", "S"]],
                &[&["new-window", "-d", "-t", "S:9"]],
                &[&["new-window", "-d", "-t", "S:two"]],
                &[&["new-window", "-d", "-k", "-t", "S:two"]],
                &[&["new-window", "-d", "-k", "-t", "S:two", "--", "sleep", "300"]],
                &[&["new-window", "-d", "-t", "nosuch"]],
                &[&["new-window", "-d", "-n", "named", "-t", "S"]],
                &[&["new-window", "-d", "-c", "/tmp", "-t", "S"], &["display-message", "-p", "-t", "S:4", "#{pane_current_path}"]],
                &[&["new-window", "-d", "-P", "-t", "S"]],
                &[&["new-window", "-d", "-P", "-F", "#{window_index} #{window_name}", "-t", "S"]],
                &[&["new-window", "-d", "-F", "#{window_index}", "-t", "S"]],
                &[&["new-window", "-d", "-a", "-t", "S:1"]],
                &[&["new-window", "-d", "-a", "-t", "S:two"]],
                &[&["new-window", "-d", "-b", "-t", "S:two"]],
                &[&["new-window", "-d", "-b", "-t", "S:1"]],
                &[&["new-window", "-d", "-a", "-b", "-t", "S:two"]],
                &[&["new-window", "-d", "-a", "-k", "-t", "S:two"]],
                &[&["new-window", "-d", "-S", "-n", "two", "-t", "S"]],
                &[&["new-window", "-S", "-n", "two", "-t", "S"]],
                &[&["new-window", "-d", "-S", "-n", "fresh", "-t", "S"]],
                &[&["new-window", "-d", "-E", "-t", "S"]],
                &[&["new-window", "-d", "-E", "-t", "S", "--", "sleep", "300"]],
                &[&["new-window", "-d", "-t", "S", "--", "sleep", "300"]],
                &[&["new-window", "-d", "-t", "S", "sleep", "300"]],
                &[&["new-window", "-d", "-t", "S", "sleep 300"]],
                &[&["new-window", "-d", "-t", "S", "-n", "x", "--", "sleep", "300"]],
                &[&["new-window", "-dn", "joined", "-t", "S"]],
                &[&["new-window", "-d", "-t", "S:"]],
                &[&["new-window", "-d", "-t", "=S"]],
                &[&["new-window", "-d"]],
                &[&["new-window"]],
                &[&["new-window", "-d", "-n"]],
                &[&["new-window", "-d", "-t"]],
            ],
        );
    }

    #[test]
    fn windows_kill_window_flags_match_the_reference() {
        probes(
            "windows_kill_window_flags_match_the_reference",
            &[
                &[&["kill-window", "-t", "S:two"]],
                &[&["kill-window", "-t", "S:2"]],
                &[&["kill-window", "-t", "S:9"]],
                &[&["kill-window", "-t", "nosuch"]],
                &[&["kill-window", "-t", "S:3"], &["kill-window", "-t", "S:2"]],
                &[&["kill-window", "-t", "S:1"]],
                &[&["select-window", "-t", "S:3"], &["kill-window", "-t", "S:3"]],
                &[&["select-window", "-t", "S:2"], &["select-window", "-t", "S:3"], &["kill-window", "-t", "S:3"]],
                &[&["select-window", "-t", "S:2"], &["kill-window", "-t", "S:2"]],
                &[&["kill-window", "-a", "-t", "S:2"]],
                &[&["kill-window", "-a", "-t", "S:1"]],
                &[&["kill-window", "-a"]],
                &[&["kill-window", "-at", "S:3"]],
                &[&["kill-window", "-t", "T:1"]],
                &[&["kill-window", "-t", "T:1"], &["kill-window", "-t", "S:1"]],
                &[&["kill-window"]],
                &[&["kill-window", "S:2"]],
                &[&["kill-window", "-t"]],
            ],
        );
    }

    #[test]
    fn windows_kill_window_rejects_flags_it_does_not_implement() {
        assert_window_rejected("kill-window", "-f", &["kill-window", "-a", "-f", "1", "-t", "S:2"]);
    }

    #[test]
    fn windows_select_window_flags_match_the_reference() {
        probes(
            "windows_select_window_flags_match_the_reference",
            &[
                &[&["select-window", "-t", "S:3"]],
                &[&["select-window", "-t", "3"]],
                &[&["select-window", "-t", "two"]],
                &[&["select-window", "-t", "tw"]],
                &[&["select-window", "-t", "S:9"]],
                &[&["select-window", "-t", "nosuch"]],
                &[&["select-window", "-t", "S:+1"]],
                &[&["select-window", "-t", "S:-1"]],
                &[&["select-window", "-t", "S:$"]],
                &[&["select-window", "-t", "S:^"]],
                &[&["select-window"]],
                &[&["select-window", "-n", "-t", "S"]],
                &[&["select-window", "-n", "-t", "S"], &["select-window", "-n", "-t", "S"], &["select-window", "-n", "-t", "S"]],
                &[&["select-window", "-p", "-t", "S"]],
                &[&["select-window", "-n", "-t", "T"]],
                &[&["select-window", "-n", "-t", "two"]],
                &[&["select-window", "-n", "-t", "2"]],
                &[&["select-window", "-n", "-t", "S:2"]],
                &[&["select-window", "-p", "-t", "S:2"]],
                &[&["select-window", "-p", "-t", "three"]],
                &[&["select-window", "-p", "-t", "S:1"]],
                &[&["select-window", "-n", "-t", "S:9"]],
                &[&["select-window", "-n", "-t", "nosuch"]],
                &[&["select-window", "-n", "-t", "9"]],
                &[&["new-session", "-d", "-s", "2", "--", "sleep", "300"], &["select-window", "-n", "-t", "2"]],
                &[&["new-session", "-d", "-s", "two", "--", "sleep", "300"], &["select-window", "-p", "-t", "two"]],
                &[&["select-window", "-n", "-t", "T:1"]],
                &[&["select-window", "-p", "-t", "$0"]],
                &[&["select-window", "-l", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "S:2"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-n", "-l", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-p", "-n", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-pl", "-t", "S"]],
                &[&["select-window", "S:3"]],
                &[&["new-session", "-d", "-s", "2", "--", "sleep", "300"], &["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "2"]],
                &[&["new-session", "-d", "-s", "2", "--", "sleep", "300"], &["select-window", "-t", "S:3"], &["select-window", "-t", "2"]],
                &[&["new-session", "-d", "-s", "two", "--", "sleep", "300"], &["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "two"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "three"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "T"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "9"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "$0"]],
                &[&["select-window", "-t"]],
            ],
        );
    }

    #[test]
    fn windows_select_window_rejects_flags_it_does_not_implement() {
        assert_window_rejected("select-window", "-T", &["select-window", "-T", "-t", "S:1"]);
    }

    #[test]
    fn windows_next_previous_and_last_window_flags_match_the_reference() {
        let test = "windows_next_previous_and_last_window_flags_match_the_reference";
        probes(
            test,
            &[
                &[&["next-window", "-t", "S"]],
                &[&["next-window"]],
                &[&["next-window", "-t", "T"]],
                &[&["next-window", "-t", "nosuch"]],
                &[&["next-window", "-t", "=S"]],
                &[&["next-window", "-t", "S:2"]],
                &[&["next-window", "-t", "S"], &["next-window", "-t", "S"], &["next-window", "-t", "S"]],
                &[&["previous-window", "-t", "S"]],
                &[&["previous-window"]],
                &[&["previous-window", "-t", "T"]],
                &[&["previous-window", "-t", "nosuch"]],
                &[&["previous-window", "-t", "S"], &["previous-window", "-t", "S"]],
                &[&["last-window", "-t", "S"]],
                &[&["last-window"]],
                &[&["select-window", "-t", "S:3"], &["last-window", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["last-window", "-t", "S"], &["last-window", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["last-window", "-t", "T"]],
                &[&["select-window", "-t", "S:3"], &["last-window", "-t", "nosuch"]],
                &[&["select-window", "-t", "S:3"], &["last-window"]],
                &[&["select-window", "-t", "S:3"], &["last-window", "S"]],
            ],
        );
    }

    #[test]
    fn windows_next_previous_and_last_window_reject_flags_they_do_not_implement() {
        assert_window_rejected("next-window", "-a", &["next-window", "-a", "-t", "S"]);
        assert_window_rejected("previous-window", "-a", &["previous-window", "-a", "-t", "S"]);
    }

    #[test]
    fn windows_unlink_window_flags_match_the_reference() {
        probes(
            "windows_unlink_window_flags_match_the_reference",
            &[
                &[&["new-session", "-d", "-s", "G", "-t", "S"], &["unlink-window", "-t", "G:3"]],
                &[&["new-session", "-d", "-s", "G", "-t", "S"], &["unlink-window", "-t", "S:3"]],
                &[
                    &["new-session", "-d", "-s", "G", "-t", "S"],
                    &["rename-window", "-t", "G:3", "renamed"],
                    &["unlink-window", "-t", "S:3"],
                ],
                &[&["new-session", "-d", "-s", "G", "-t", "S"], &["unlink-window", "-k", "-t", "G:3"]],
                &[&["unlink-window", "-t", "S:3"]],
                &[&["unlink-window", "-k", "-t", "S:3"]],
                &[&["unlink-window", "-k", "-t", "T:1"]],
                &[&["unlink-window", "-t", "S:9"]],
                &[&["unlink-window"]],
                &[&["unlink-window", "S:3"]],
            ],
        );
    }

    #[test]
    fn windows_link_window_is_rejected() {
        assert_window_rejected("link-window", "-s", &["link-window", "-s", "S:3", "-t", "T:"]);
        assert_window_rejected("link-window", "-d", &["link-window", "-d", "-s", "S:3", "-t", "T:"]);
        assert_window_rejected("link-window", "-a", &["linkw", "-a", "-s", "S:3", "-t", "T:"]);
    }

    #[test]
    fn windows_move_window_flags_match_the_reference() {
        probes(
            "windows_move_window_flags_match_the_reference",
            &[
                &[&["move-window", "-s", "S:3", "-t", "T:"]],
                &[&["move-window", "-s", "S:3", "-t", "T:9"]],
                &[&["move-window", "-s", "S:3", "-t", "S:9"]],
                &[&["move-window", "-s", "S:3", "-t", "S:1"]],
                &[&["move-window", "-k", "-s", "S:3", "-t", "S:1"]],
                &[&["move-window", "-d", "-s", "S:3", "-t", "T:"]],
                &[&["move-window", "-a", "-s", "S:3", "-t", "S:1"]],
                &[&["move-window", "-a", "-s", "S:3", "-t", "T:1"]],
                &[&["move-window", "-ad", "-s", "S:3", "-t", "T:1"]],
                &[&["move-window", "-r", "-t", "S"]],
                &[&["move-window", "-s", "S:3", "-t", "S:7"], &["move-window", "-r", "-t", "S"]],
                &[&["move-window", "-r"]],
                &[&["move-window", "-r", "-s", "S:1", "-t", "S:5"]],
                &[&["move-window", "-s", "S:9", "-t", "T:"]],
                &[&["move-window", "-s", "nosuch:1", "-t", "T:"]],
                &[&["move-window", "-t", "T:5"]],
                &[&["move-window", "-s", "S:3"]],
                &[&["move-window", "-s", "two", "-t", "T:"]],
                &[&["move-window", "-s", "S:3", "-t", "nosuch:"]],
            ],
        );
    }

    #[test]
    fn windows_move_window_rejects_flags_it_does_not_implement() {
        assert_window_rejected("move-window", "-b", &["move-window", "-b", "-s", "S:3", "-t", "S:1"]);
    }

    #[test]
    fn windows_swap_window_flags_match_the_reference() {
        probes(
            "windows_swap_window_flags_match_the_reference",
            &[
                &[&["swap-window", "-s", "S:1", "-t", "S:3"]],
                &[&["swap-window", "-d", "-s", "S:1", "-t", "S:3"]],
                &[&["swap-window", "-s", "S:1", "-t", "T:1"]],
                &[&["swap-window", "-d", "-s", "S:3", "-t", "T:1"]],
                &[&["swap-window", "-t", "S:3"]],
                &[&["swap-window", "-s", "S:3"]],
                &[&["swap-window"]],
                &[&["swap-window", "-s", "S:9", "-t", "S:1"]],
                &[&["swap-window", "-s", "S:1", "-t", "S:9"]],
                &[&["swap-window", "-s", "S:1", "-t", "S:1"]],
                &[&["swap-window", "-s", "two", "-t", "three"]],
                &[&["swap-window", "-s", "S:1", "-t", "nosuch"]],
                &[&["swap-window", "-s", "S:2", "-t", "S:3"], &["swap-window", "-s", "S:2", "-t", "S:3"]],
            ],
        );
    }

    #[test]
    fn windows_swap_window_rejects_flags_it_does_not_implement() {
        assert_window_rejected("swap-window", "-r", &["swap-window", "-r", "-s", "S:1", "-t", "S:3"]);
    }

    #[test]
    fn windows_rotate_window_flags_match_the_reference() {
        probes(
            "windows_rotate_window_flags_match_the_reference",
            &[
                &[&["rotate-window", "-t", "S:2"]],
                &[&["rotate-window", "-D", "-t", "S:2"]],
                &[&["rotate-window", "-U", "-t", "S:2"]],
                &[&["rotate-window", "-UD", "-t", "S:2"]],
                &[&["rotate-window", "-DU", "-t", "S:2"]],
                &[&["rotate-window", "-t", "S:1"]],
                &[&["rotate-window", "-t", "S:9"]],
                &[&["rotate-window", "-t", "nosuch"]],
                &[&["rotate-window"]],
                &[&["rotate-window", "-U", "-t", "S:2"], &["rotate-window", "-U", "-t", "S:2"]],
                &[&["rotate-window", "S:2"]],
            ],
        );
    }

    #[test]
    fn windows_rotate_window_rejects_flags_it_does_not_implement() {
        assert_window_rejected("rotate-window", "-Z", &["rotate-window", "-Z", "-t", "S:2"]);
    }

    #[test]
    fn windows_rename_window_flags_match_the_reference() {
        probes(
            "windows_rename_window_flags_match_the_reference",
            &[
                &[&["rename-window", "-t", "S:2", "renamed"]],
                &[&["rename-window", "-t", "two", "renamed"]],
                &[&["rename-window", "renamed"]],
                &[&["rename-window", "-t", "S:9", "renamed"]],
                &[&["rename-window", "-t", "nosuch", "renamed"]],
                &[&["rename-window", "-t", "S:2"]],
                &[&["rename-window", "-t", "S:2", "a", "b"]],
                &[&["rename-window", "-t", "S:2", "--", "-dash"]],
                &[&["rename-window", "-t", "S:2", ""]],
                &[&["rename-window", "-t", "S:2", "#{session_name}"]],
                &[&["rename-window", "-tS:2", "joined"]],
                &[&["rename-window"]],
            ],
        );
    }

    #[test]
    fn windows_respawn_window_flags_match_the_reference() {
        probes(
            "windows_respawn_window_flags_match_the_reference",
            &[
                &[&["respawn-window", "-t", "S:3"]],
                &[&["respawn-window", "-k", "-t", "S:3"]],
                &[&["respawn-window", "-k", "-t", "S:2"]],
                &[&["respawn-window", "-k", "-t", "S:9"]],
                &[&["respawn-window", "-k", "-c", "/tmp", "-t", "S:3"], &["display-message", "-p", "-t", "S:3", "#{pane_current_path}"]],
                &[&["respawn-window", "-k", "-t", "S:3", "--", "sleep", "301"], &["display-message", "-p", "-t", "S:3", "#{pane_start_command}"]],
                &[&["respawn-window", "-k", "-t", "S:3", "sleep 302"]],
                &[&["respawn-window", "-kt", "S:3"]],
                &[&["respawn-window", "-k", "-t", "S:2"], &["list-panes", "-t", "S:2", "-F", "#{pane_index} #{pane_id}"]],
                &[&["respawn-window", "-k", "-t", "S:3"], &["list-panes", "-t", "S:3", "-F", "#{pane_id}"]],
                &[&["respawn-window", "-k"]],
                &[&["respawn-window"]],
            ],
        );
    }

    #[test]
    fn windows_respawn_window_rejects_flags_it_does_not_implement() {
        assert_window_rejected("respawn-window", "-e", &["respawn-window", "-k", "-e", "A=1", "-t", "S:3"]);
        assert_window_rejected("respawn-window", "-E", &["respawn-window", "-k", "-E", "-t", "S:3"]);
        assert_window_rejected("respawn-window", "-d", &["respawn-window", "-k", "-d", "-t", "S:3"]);
    }

    #[test]
    fn windows_select_layout_flags_match_the_reference() {
        probes(
            "windows_select_layout_flags_match_the_reference",
            &[
                &[&["select-layout", "-t", "S:2", "even-horizontal"]],
                &[&["select-layout", "-t", "S:2", "even-vertical"]],
                &[&["select-layout", "-t", "S:1", "even-horizontal"]],
                &[&["select-layout", "-t", "S:9", "even-horizontal"]],
                &[&["select-layout", "-t", "nosuch", "even-horizontal"]],
                &[&["select-layout", "even-horizontal"]],
                &[&["select-layout", "-t", "S:2", "even-horizontal", "extra"]],
            ],
        );
    }

    #[test]
    fn windows_select_layout_rejects_layouts_it_does_not_implement() {
        let pair = fixture("winlay");
        let before = window_topology(&pair.tm);
        for layout in ["tiled", "main-horizontal", "main-vertical", "bogus"] {
            let output = pair.tm.run(&["select-layout", "-t", "S:2", layout]);
            assert!(!output.status.success(), "{layout}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported layout {layout} for select-layout\n")
            );
        }
        for arguments in [&["select-layout", "-t", "S:2"][..], &["select-layout"][..]] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                "tm: select-layout requires a layout\n"
            );
        }
        assert_eq!(window_topology(&pair.tm), before);
    }

    #[test]
    fn windows_select_layout_rejects_flags_it_does_not_implement() {
        for flag in ["-E", "-n", "-o", "-p"] {
            assert_window_rejected("select-layout", flag, &["select-layout", flag, "-t", "S:2"]);
        }
    }

    #[test]
    fn windows_new_window_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-e", &["new-window", "-d", "-e", "A=1", "-t", "S"][..]),
            ("-I", &["new-window", "-d", "-I", "7", "-t", "S"][..]),
            ("-x", &["new-window", "-x", "-t", "S"][..]),
        ] {
            assert_window_rejected("new-window", flag, arguments);
        }
    }

    #[test]
    fn windows_list_windows_flags_match_the_reference() {
        let test = "windows_list_windows_flags_match_the_reference";
        let pair = fixture("lsw");
        let window_format = "#{window_index} #{window_name} #{window_panes} #{window_active}";
        for arguments in [
            &["list-windows", "-F", "#{window_index}"][..],
            &["lsw", "-F", "#{window_index}"],
            &["list-windows", "-t", "S", "-F", "#{window_index}"],
            &["list-windows", "-t", "T", "-F", "#{window_index}"],
            &["list-windows", "-t", "=S", "-F", "#{window_index}"],
            &["list-windows", "-tS", "-F", "#{window_index}"],
            &["list-windows", "-a"],
            &["list-windows", "-a", "-t", "T"],
            &["list-windows", "-F", window_format],
            &["list-windows", "-t", "S", "-F", "#{session_name}:#{window_id}:#{window_flags}"],
            &["list-windows", "-a", "-F", "#{session_name}:#{window_index}"],
            &["list-windows", "-F", ""],
            &["list-windows", "-f", "#{==:#{window_name},two}", "-F", window_format],
            &["list-windows", "-f", "#{window_active}", "-F", window_format],
            &["list-windows", "-f", "#{>:#{window_panes},1}", "-F", window_format],
            &["list-windows", "-a", "-f", "#{==:#{session_name},T}", "-F", "#{session_name}:#{window_index}"],
            &["list-windows", "-f", "0", "-F", window_format],
            &["list-windows", "-f", "false", "-F", window_format],
            &["list-windows", "-f", "", "-F", window_format],
            &["list-windows", "-t", "nosuch"],
            &["list-windows", "-t", "S:two", "-F", "#{window_index}"],
            &["list-windows", "-t", "--"],
            &["list-windows", "extra"],
            &["list-windows", "-t"],
            &["list-windows", "-F"],
            &["list-windows", "-f"],
        ] {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
        }
    }

    #[test]
    fn windows_list_windows_default_format_matches_except_the_layout() {
        let pair = fixture("lswdefault");
        for arguments in [&["list-windows"][..], &["list-windows", "-t", "T"]] {
            let tm = pair.tm.body(arguments);
            let reference = pair.reference.body(arguments);
            let without_layout = reference
                .lines()
                .map(|line| {
                    let start = line.find("[layout ").expect("layout column");
                    let end = line.rfind("] @").expect("window id column");
                    format!("{}[layout {}", &line[..start], &line[end..])
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(tm, without_layout, "{arguments:?}");
        }
    }

    #[test]
    fn windows_list_windows_rejects_flags_it_does_not_implement() {
        let pair = fixture("lswrej");
        for (flag, arguments) in [
            ("-O", &["list-windows", "-O", "name"][..]),
            ("-r", &["list-windows", "-r"]),
            ("-x", &["list-windows", "-x"]),
        ] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for list-windows\n"),
                "{arguments:?}"
            );
            assert!(output.stdout.is_empty(), "{arguments:?}");
        }
    }

    #[test]
    fn windows_offset_targets_that_are_not_counts_fail_like_the_reference() {
        let test = "windows_offset_targets_that_are_not_counts_fail_like_the_reference";
        let pair = fixture("offsets");
        for spelling in [
            "--", "++", "-0", "+0", "-x", "+x", "-1", "+1", "+2", "-2", "-10", "+-1", "- ", "-",
            "+", "{next}", "{previous}", "{last}", "!", "^", "$", "two", "tw", "=two", "=tw",
            "-two", "+two", "1-", "-1x", "+99999999999",
        ] {
            for command in ["list-panes", "select-window", "rename-window"] {
                let arguments: Vec<&str> = match command {
                    "rename-window" => vec![command, "-t", spelling, "renamed"],
                    "list-panes" => {
                        vec![command, "-t", spelling, "-F", "#{window_index}.#{pane_index}"]
                    }
                    _ => vec![command, "-t", spelling],
                };
                compare_commands(test, &pair.tm, &pair.reference, &[&arguments]);
                pair.assert_same_topology(test, &arguments);
            }
        }
    }
}

mod panes {
    use super::*;

    fn fixture(label: &str) -> Pair {
        let pair = Pair::new(label);
        pair.start("S");
        for mux in pair.both() {
            mux.ok(&["split-window", "-d", "-h", "-t", "S:1", "--", "sleep", "300"]);
            mux.ok(&["split-window", "-d", "-v", "-t", "S:1.1", "--", "sleep", "300"]);
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "two", "--", "sleep", "300"]);
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "three", "--", "sleep", "300"]);
        }
        pair.start("T");
        pair
    }

    fn pane_topology(mux: &Mux) -> String {
        let mut text = String::new();
        for session in sorted_lines(mux, &["list-sessions", "-F", "#{session_name}"]) {
            text.push_str(&format!(
                "windows {session} {:?}\n",
                sorted_lines(
                    mux,
                    &[
                        "list-windows",
                        "-t",
                        &session,
                        "-F",
                        "#{window_index} #{window_name} #{window_panes} #{window_active}",
                    ],
                )
            ));
        }
        text.push_str(&mux.body(&[
            "list-panes",
            "-a",
            "-F",
            "#{session_name}:#{window_index}.#{pane_index} #{pane_id} #{pane_width}x#{pane_height}@#{pane_left},#{pane_top} #{pane_active} #{pane_start_command} #{pane_input_off} #{pane_marked}",
        ]));
        text
    }

    fn probe(test: &str, label: &str, steps: &[&[&str]]) {
        let pair = fixture("pan");
        for arguments in steps {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
            assert_eq!(
                observation_difference(
                    &format!("{test}: {label}: topology after {arguments:?}"),
                    &pane_topology(&pair.tm),
                    &pane_topology(&pair.reference)
                ),
                None
            );
        }
    }

    fn probes(test: &str, cases: &[&[&[&str]]]) {
        let mut failed = Vec::new();
        for steps in cases {
            if std::panic::catch_unwind(|| probe(test, &format!("{steps:?}"), steps)).is_err() {
                failed.push(format!("{steps:?}"));
            }
        }
        assert!(failed.is_empty(), "FAILED CASES:\n{}", failed.join("\n"));
    }

    fn assert_pane_rejected(command: &str, flag: &str, arguments: &[&str]) {
        let pair = fixture("panrej");
        let before = pane_topology(&pair.tm);
        let output = pair.tm.run(arguments);
        assert!(!output.status.success(), "{arguments:?} succeeded");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("tm: unsupported flag {flag} for {command}\n"),
            "{arguments:?}"
        );
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert_eq!(pane_topology(&pair.tm), before, "{arguments:?} changed the topology");
    }

    #[test]
    fn panes_list_panes_flags_match_the_reference() {
        probes(
            "panes_list_panes_flags_match_the_reference",
            &[
                &[&["list-panes", "-F", "#{pane_index} #{pane_id} #{pane_active}"]],
                &[&["list-panes", "-t", "S:2", "-F", "#{window_name} #{pane_id}"]],
                &[&["list-panes", "-t", "S:1", "-F", "#{pane_id}"]],
                &[&["list-panes", "-t", "S", "-F", "#{pane_id}"]],
                &[&["list-panes", "-t", "two", "-F", "#{pane_id}"]],
                &[&["list-panes", "-t", "T", "-F", "#{pane_id}"]],
                &[&["list-panes", "-t", "nosuch", "-F", "#{pane_id}"]],
                &[&["list-panes", "-s", "-F", "#{window_index}.#{pane_index} #{pane_id}"]],
                &[&["list-panes", "-s", "-t", "S:3", "-F", "#{window_index}.#{pane_index} #{pane_id}"]],
                &[&["list-panes", "-s", "-t", "T", "-F", "#{window_index}.#{pane_index} #{pane_id}"]],
                &[&["list-panes", "-a", "-F", "#{session_name}:#{window_index}.#{pane_index} #{pane_id}"]],
                &[&["list-panes", "-a", "-t", "T", "-F", "#{session_name} #{pane_id}"]],
                &[&["list-panes", "-a", "-t", "nosuch", "-F", "#{pane_id}"]],
                &[&["list-panes", "-as", "-F", "#{session_name} #{pane_id}"]],
                &[&["list-panes", "-f", "#{pane_active}", "-F", "#{pane_id}"]],
                &[&["list-panes", "-f", "#{==:#{pane_index},1}", "-F", "#{pane_id}"]],
                &[&["list-panes", "-a", "-f", "#{==:#{window_name},two}", "-F", "#{pane_id}"]],
                &[&["list-panes", "-f", "0", "-F", "#{pane_id}"]],
                &[&["list-panes", "-f", "", "-F", "#{pane_id}"]],
                &[&["list-panes", "-f", "x", "-F", "#{pane_id}"]],
                &[&["list-panes", "-F"]],
                &[&["list-panes", "extra"]],
                &[&["list-panes", "-t"]],
            ],
        );
    }

    #[test]
    fn panes_list_panes_default_format_is_tm_only() {
        let pair = fixture("panlisdef");
        let tm = pair.tm.body(&["list-panes", "-a"]);
        let reference = pair.reference.body(&["list-panes", "-a"]);
        assert_eq!(tm.lines().count(), reference.lines().count());
        assert!(tm.starts_with("S:1.0: %0: 0 "), "{tm}");
        assert!(reference.starts_with("S:1.0: [40x24] [history 0/10000, "), "{reference}");
        let session = pair.tm.body(&["list-panes", "-s", "-t", "S:1"]);
        assert!(session.starts_with("1.0: %0: 0 "), "{session}");
    }

    #[test]
    fn panes_list_panes_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-O", &["list-panes", "-O", "name"][..]),
            ("-r", &["list-panes", "-r"][..]),
        ] {
            assert_pane_rejected("list-panes", flag, arguments);
        }
    }

    fn split_with(flags: &[&'static str]) -> Vec<&'static str> {
        [&["split-window"][..], flags, &["--", "sleep", "300"]].concat()
    }

    fn probe_split_cases(test: &str, cases: Vec<Vec<Vec<&str>>>) {
        for steps in &cases {
            let steps = steps.iter().map(Vec::as_slice).collect::<Vec<_>>();
            probe(test, &format!("{steps:?}"), &steps);
        }
    }

    #[test]
    fn panes_split_window_size_and_placement_flags_match_the_reference() {
        let cases: Vec<Vec<Vec<&str>>> = vec![
            vec![split_with(&["-t", "S:2"])],
            vec![split_with(&["-t", "S:2"]), split_with(&["-t", "S:2"]), split_with(&["-t", "S:2"]), split_with(&["-t", "S:2"])],
            vec![split_with(&["-t", "S:2", "-h"]), split_with(&["-t", "S:2", "-h"]), split_with(&["-t", "S:2", "-h"])],
            vec![split_with(&["-t", "S:2", "-b"]), split_with(&["-t", "S:2", "-b"]), split_with(&["-t", "S:2", "-b"])],
            vec![split_with(&["-t", "S:2", "-hb"]), split_with(&["-t", "S:2", "-hb"])],
            vec![split_with(&["-t", "S:2", "-v"])],
            vec![split_with(&["-t", "S:1.1"]), split_with(&["-t", "S:1.0"]), split_with(&["-t", "S:1.3", "-b"])],
            vec![split_with(&["-t", "S:1.0", "-d"])],
            vec![split_with(&["-t", "S:2", "-l", "5"])],
            vec![split_with(&["-t", "S:2", "-l", "30%"])],
            vec![split_with(&["-t", "S:2", "-h", "-l", "30%"])],
            vec![split_with(&["-t", "S:2", "-b", "-l", "7"])],
            vec![split_with(&["-t", "S:2", "-p", "30"])],
            vec![split_with(&["-t", "S:2", "-p", "30", "-b"])],
            vec![split_with(&["-t", "S:2", "-l", "3", "-p", "50"])],
            vec![split_with(&["-t", "S:2", "-l", "0"])],
            vec![split_with(&["-t", "S:2", "-l", "500"])],
            vec![split_with(&["-t", "S:2", "-l", "abc"])],
            vec![split_with(&["-t", "S:2", "-p", "101"])],
            vec![split_with(&["-t", "S:2", "-p", "x"])],
            vec![split_with(&["-t", "S:2", "-l"])],
            vec![split_with(&["-t", "S:2", "-Z"])],
            vec![split_with(&["-t", "S:1.0", "-Z"])],
        ];
        probe_split_cases("panes_split_window_size_and_placement_flags_match_the_reference", cases);
    }

    #[test]
    fn panes_split_window_command_and_format_flags_match_the_reference() {
        let cases: Vec<Vec<Vec<&str>>> = vec![
            vec![split_with(&["-t", "S:2", "-c", "/tmp"])],
            vec![split_with(&["-t", "S:2", "-P"])],
            vec![split_with(&["-t", "S:2", "-P", "-F", "#{pane_id} #{pane_index} #{pane_width}"])],
            vec![split_with(&["-t", "S:2", "-F", "#{pane_id}"])],
            vec![split_with(&["-t", "S:2", "-dP"])],
            vec![vec!["split-window", "-t", "S:2", "sleep 300"]],
            vec![vec!["split-window", "-t", "S:2", "sleep", "300"]],
            vec![vec!["split-window", "-t", "S:2", "sh", "-c", "sleep 300"]],
            vec![vec!["split-window", "-t", "S:2", "sleep 300; echo \"a b\""]],
            vec![vec!["split-window", "-t", "S:2", "sleep", "30#0"]],
            vec![vec!["split-window", "-t", "S:2", "-E"]],
            vec![vec!["split-window", "-t", "S:2", ""]],
            vec![vec!["split-window", "-t", "S:2", "-E", "sleep", "300"]],
            vec![vec!["split-window", "-t", "S:2", "-E", "-P", "-F", "#{pane_start_command}|#{pane_dead}"]],
            vec![vec!["split-window", "-t", "S:2", "-P", "-F", "#{pane_start_command}", "sleep 300"]],
            vec![vec!["split-window", "-t", "S:2", "-P", "-F", "#{pane_start_command}", "sleep", "it's a", "300"]],
            vec![split_with(&["-t", "nosuch"])],
            vec![split_with(&["-t", "S:9"])],
            vec![split_with(&["-t", "S:2.5"])],
            vec![split_with(&["-t", "%3"])],
            vec![split_with(&["-t", "T"])],
            vec![vec!["split-window", "-t"]],
            vec![split_with(&[])],
            vec![split_with(&["-dh"]), split_with(&["-dh"]), split_with(&["-dh"]), split_with(&["-dh"])],
            vec![split_with(&["-h", "-t", "S:1.0"]), split_with(&["-h", "-t", "S:1.0"]), split_with(&["-h", "-t", "S:1.0"]), split_with(&["-h", "-t", "S:1.0"]), split_with(&["-h", "-t", "S:1.0"])],
        ];
        probe_split_cases("panes_split_window_command_and_format_flags_match_the_reference", cases);
    }

    #[test]
    fn panes_split_window_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-B", &["split-window", "-B", "single", "-t", "S:2"][..]),
            ("-e", &["split-window", "-e", "A=1", "-t", "S:2"][..]),
            ("-f", &["split-window", "-f", "-t", "S:2"][..]),
            ("-Z (with -d)", &["split-window", "-dZ", "-t", "S:1.0"][..]),
            ("-I", &["split-window", "-I", "-t", "S:2"][..]),
            ("-k", &["split-window", "-k", "-t", "S:2"][..]),
            ("-m", &["split-window", "-m", "gone", "-t", "S:2"][..]),
            ("-R", &["split-window", "-R", "fg=red", "-t", "S:2"][..]),
            ("-s", &["split-window", "-s", "fg=red", "-t", "S:2"][..]),
            ("-S", &["split-window", "-S", "fg=red", "-t", "S:2"][..]),
            ("-T", &["split-window", "-T", "title", "-t", "S:2"][..]),
            ("-W", &["split-window", "-W", "-t", "S:2"][..]),
            ("-x", &["split-window", "-x", "10", "-t", "S:2"][..]),
        ] {
            assert_pane_rejected("split-window", flag, arguments);
        }
    }

    #[test]
    fn panes_new_pane_is_rejected() {
        let pair = fixture("pannew");
        let before = pane_topology(&pair.tm);
        for arguments in [
            &["new-pane", "-t", "S:2"][..],
            &["newp", "-L", "-t", "S:2"][..],
            &["new-pane"][..],
        ] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert!(output.stdout.is_empty(), "{arguments:?}");
            assert_eq!(pane_topology(&pair.tm), before, "{arguments:?}");
        }
        assert_pane_rejected("new-pane", "-t", &["new-pane", "-t", "S:2"]);
    }

    #[test]
    fn panes_kill_pane_flags_match_the_reference() {
        probes(
            "panes_kill_pane_flags_match_the_reference",
            &[
                &[&["kill-pane", "-t", "S:1.1"]],
                &[&["kill-pane", "-t", "S:1.0"], &["kill-pane", "-t", "S:1.0"]],
                &[&["kill-pane", "-t", "S:1.2"]],
                &[&["select-pane", "-t", "S:1.2"], &["kill-pane", "-t", "S:1.2"]],
                &[&["select-pane", "-t", "S:1.1"], &["kill-pane"]],
                &[&["kill-pane", "-a", "-t", "S:1.1"]],
                &[&["kill-pane", "-a", "-t", "S:1.0"]],
                &[&["kill-pane", "-a"]],
                &[&["kill-pane", "-a", "-t", "S:2"]],
                &[&["kill-pane", "-a", "-f", "#{==:#{pane_index},2}", "-t", "S:1.0"]],
                &[&["kill-pane", "-a", "-f", "#{pane_active}", "-t", "S:1.0"]],
                &[&["kill-pane", "-a", "-f", "0", "-t", "S:1.0"]],
                &[&["kill-pane", "-a", "-f", "1", "-t", "S:1.0"]],
                &[&["kill-pane", "-f", "1", "-t", "S:1.0"]],
                &[&["kill-pane", "-t", "S:2"]],
                &[&["kill-pane", "-t", "S:2"], &["kill-pane", "-t", "S:2"]],
                &[&["kill-pane", "-t", "T"]],
                &[&["kill-pane", "-t", "nosuch"]],
                &[&["kill-pane", "-t", "S:1.9"]],
                &[&["kill-pane", "-t", "%2"]],
                &[&["kill-pane", "-t"]],
                &[&["kill-pane", "extra"]],
            ],
        );
    }

    #[test]
    fn panes_kill_pane_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-K", &["kill-pane", "-K", "-t", "S:1.1"][..]),
            ("-F", &["kill-pane", "-F", "x", "-t", "S:1.1"][..]),
        ] {
            assert_pane_rejected("kill-pane", flag, arguments);
        }
    }

    #[test]
    fn panes_select_pane_navigation_flags_match_the_reference() {
        let title = "panes_select_pane_navigation_flags_match_the_reference";
        probes(
            title,
            &[
                &[&["select-pane", "-t", "S:1.1"]],
                &[&["select-pane", "-t", "S:1.2"], &["select-pane", "-t", "S:1.0"]],
                &[&["select-pane", "-t", "%2"]],
                &[&["select-pane", "-t", "S:2"]],
                &[&["select-pane", "-t", "T"]],
                &[&["select-pane", "-t", "nosuch"]],
                &[&["select-pane", "-t", "S:1.9"]],
                &[&["select-pane", "-t"]],
                &[&["select-pane"]],
                &[&["select-pane", "-L", "-t", "S:1.1"]],
                &[&["select-pane", "-R", "-t", "S:1.0"]],
                &[&["select-pane", "-t", "S:1.2"], &["select-pane", "-R", "-t", "S:1.0"]],
                &[&["select-pane", "-t", "S:1.1"], &["select-pane", "-R", "-t", "S:1.0"]],
                &[&["select-pane", "-U", "-t", "S:1.2"]],
                &[&["select-pane", "-D", "-t", "S:1.1"]],
                &[&["select-pane", "-L", "-t", "S:1.0"]],
                &[&["select-pane", "-R", "-t", "S:1.1"]],
                &[&["select-pane", "-U", "-t", "S:1.1"]],
                &[&["select-pane", "-D", "-t", "S:1.2"]],
                &[&["select-pane", "-U", "-t", "S:1.0"]],
                &[&["select-pane", "-D", "-t", "S:2"]],
                &[&["select-pane", "-L", "-R", "-t", "S:1.1"]],
                &[&["select-pane", "-DR", "-t", "S:1.1"]],
            ],
        );
    }

    #[test]
    fn panes_select_pane_state_flags_match_the_reference() {
        let title = "panes_select_pane_state_flags_match_the_reference";
        probes(
            title,
            &[
                &[&["select-pane", "-l"]],
                &[&["select-pane", "-l", "-t", "S:2"]],
                &[&["select-pane", "-t", "S:1.1"], &["select-pane", "-l"]],
                &[&["select-pane", "-t", "S:1.1"], &["select-pane", "-t", "S:1.2"], &["select-pane", "-l"], &["select-pane", "-l"]],
                &[&["split-window", "-d", "-t", "S:2", "--", "sleep", "300"], &["select-pane", "-l", "-t", "S:2"]],
                &[&["select-pane", "-t", "S:1.1"], &["select-pane", "-l", "-d"]],
                &[&["select-pane", "-t", "S:1.1"], &["select-pane", "-l", "-d"], &["select-pane", "-l", "-e"]],
                &[&["select-pane", "-t", "S:1.1"], &["select-pane", "-l", "-m"]],
                &[&["select-pane", "-t", "S:1.1"], &["select-pane", "-l", "-T", "x"]],
                &[&["select-pane", "-m", "-t", "S:1.1"]],
                &[&["select-pane", "-m", "-t", "S:1.1"], &["select-pane", "-m", "-t", "S:1.1"]],
                &[&["select-pane", "-m", "-t", "S:1.1"], &["select-pane", "-m", "-t", "S:1.2"]],
                &[&["select-pane", "-m", "-t", "S:1.1"], &["select-pane", "-M", "-t", "S:1.2"]],
                &[&["select-pane", "-M"]],
                &[&["select-pane", "-mM", "-t", "S:1.1"]],
                &[&["select-pane", "-m", "-L", "-t", "S:1.1"]],
                &[&["select-pane", "-m", "-d", "-t", "S:1.1"]],
                &[&["select-pane", "-d", "-t", "S:1.1"]],
                &[&["select-pane", "-d", "-t", "S:1.1"], &["select-pane", "-e", "-t", "S:1.1"]],
                &[&["select-pane", "-e", "-t", "S:1.1"]],
                &[&["select-pane", "-d", "-e", "-t", "S:1.1"]],
                &[&["select-pane", "-d", "-L", "-t", "S:1.1"]],
                &[&["select-pane", "-e", "-R", "-t", "S:1.0"]],
                &[&["select-pane", "-d", "-T", "x", "-t", "S:1.1"]],
                &[&["select-pane", "-T", "x", "-t", "S:1.1"], &["display-message", "-p", "-t", "S:1.1", "#{pane_title}|#{pane_index}"]],
                &[&["select-pane", "-T", "a b", "-L", "-t", "S:1.1"], &["display-message", "-p", "-t", "S:1.0", "#{pane_title}|#{pane_active}"], &["display-message", "-p", "-t", "S:1.1", "#{pane_active}"]],
                &[&["select-pane", "-T", "#{pane_index}", "-t", "S:1.2"], &["display-message", "-p", "-t", "S:1.2", "#{pane_title}"]],
                &[&["select-pane", "-T", "", "-t", "S:1.1"], &["display-message", "-p", "-t", "S:1.1", "[#{pane_title}]"]],
                &[&["select-pane", "-T"]],
            ],
        );
    }

    #[test]
    fn panes_select_pane_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-g", &["select-pane", "-g", "-t", "S:1.1"][..]),
            ("-P", &["select-pane", "-P", "fg=red", "-t", "S:1.1"][..]),
            ("-Z", &["select-pane", "-Z", "-t", "S:1.1"][..]),
            ("-n", &["select-pane", "-n", "-t", "S:1.1"][..]),
            ("-p", &["select-pane", "-p", "-t", "S:1.1"][..]),
        ] {
            assert_pane_rejected("select-pane", flag, arguments);
        }
    }

    #[test]
    fn panes_break_pane_flags_match_the_reference() {
        probes(
            "panes_break_pane_flags_match_the_reference",
            &[
                &[&["break-pane", "-s", "S:1.1"]],
                &[&["break-pane", "-s", "S:1.2"], &["break-pane", "-s", "S:1.0"]],
                &[&["break-pane", "-d", "-s", "S:1.1"]],
                &[&["break-pane"]],
                &[&["select-pane", "-t", "S:1.2"], &["break-pane"]],
                &[&["break-pane", "-n", "moved", "-s", "S:1.1"]],
                &[&["break-pane", "-n", "", "-s", "S:1.1"]],
                &[&["break-pane", "-P", "-s", "S:1.1"]],
                &[&["break-pane", "-P", "-F", "#{window_index} #{window_name} #{pane_id} #{pane_index}", "-s", "S:1.1"]],
                &[&["break-pane", "-F", "#{window_index}", "-s", "S:1.1"]],
                &[&["break-pane", "-dP", "-s", "S:1.2"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "S:7"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "7"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "S:2"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "S:"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "S"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "T:"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "T:5"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "nosuch:"]],
                &[&["break-pane", "-s", "S:2.0"]],
                &[&["break-pane", "-s", "S:2.0", "-t", "S:9"]],
                &[&["break-pane", "-s", "T:1", "-t", "S:"]],
                &[&["break-pane", "-d", "-n", "kept", "-s", "T:1", "-t", "S:8"]],
                &[&["break-pane", "-s", "S:1.1", "-t", "T:"], &["break-pane", "-s", "T:2.0", "-t", "S:5"]],
                &[&["break-pane", "-s", "nosuch"]],
                &[&["break-pane", "-s", "S:1.9"]],
                &[&["break-pane", "-s", "%2"]],
                &[&["break-pane", "-s"]],
                &[&["break-pane", "extra"]],
                &[&["split-window", "-d", "-t", "S:2", "sh", "-c", "sleep 300"], &["break-pane", "-s", "S:2.1"]],
                &[&["split-window", "-d", "-t", "S:2", "sleep 300"], &["break-pane", "-s", "S:2.1"]],
                &[&["split-window", "-d", "-t", "S:2", "/bin/sleep", "300"], &["break-pane", "-s", "S:2.1"]],
                &[&["split-window", "-d", "-t", "S:2"], &["break-pane", "-s", "S:2.1"]],
            ],
        );
    }

    #[test]
    fn panes_break_pane_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-a", &["break-pane", "-a", "-s", "S:1.1"][..]),
            ("-b", &["break-pane", "-b", "-s", "S:1.1"][..]),
            ("-W", &["break-pane", "-W", "-s", "S:1.1"][..]),
            ("-x", &["break-pane", "-x", "10", "-s", "S:1.1"][..]),
            ("-X", &["break-pane", "-X", "10", "-s", "S:1.1"][..]),
            ("-y", &["break-pane", "-y", "10", "-s", "S:1.1"][..]),
            ("-Y", &["break-pane", "-Y", "10", "-s", "S:1.1"][..]),
        ] {
            assert_pane_rejected("break-pane", flag, arguments);
        }
    }

    #[test]
    fn panes_join_pane_flags_match_the_reference() {
        probes(
            "panes_join_pane_flags_match_the_reference",
            &[
                &[&["join-pane", "-s", "S:2", "-t", "S:1.0"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.2"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.0", "-h"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.0", "-v"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-b"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-hb"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-d"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-l", "5"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-l", "30%"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-hl", "10"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-b", "-l", "5"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-p", "30"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-p", "x"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-l", "x"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-l", "0"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "-l", "99"]],
                &[&["join-pane", "-s", "S:1.2", "-t", "S:2"]],
                &[&["join-pane", "-s", "S:1.2", "-t", "S:2", "-h"]],
                &[&["join-pane", "-s", "S:1.0", "-t", "S:2"]],
                &[&["join-pane", "-s", "S:1.1", "-t", "S:1.1"]],
                &[&["join-pane", "-s", "T:1", "-t", "S:2"]],
                &[&["join-pane", "-s", "S:2", "-t", "T:1"]],
                &[&["join-pane", "-s", "S:2", "-t", "nosuch"]],
                &[&["join-pane", "-s", "nosuch", "-t", "S:1"]],
                &[&["join-pane", "-s", "S:2"]],
                &[&["join-pane", "-t", "S:1.1"]],
                &[&["join-pane"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:3"], &["join-pane", "-s", "S:2", "-t", "S:1"]],
                &[&["select-pane", "-m", "-t", "S:2"], &["join-pane", "-t", "S:1.1"]],
                &[&["join-pane", "-s", "S:2", "-t", "S:1.1", "extra"]],
            ],
        );
    }

    #[test]
    fn panes_join_pane_within_one_window_is_tm_only() {
        let pair = fixture("panjoin");
        let before = pane_topology(&pair.tm);
        let output = pair.tm.run(&["join-pane", "-s", "S:1.0", "-t", "S:1.1"]);
        assert!(!output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "tm: source and target panes must be in different windows\n"
        );
        assert_eq!(pane_topology(&pair.tm), before);
        assert!(pair.reference.run(&["join-pane", "-s", "S:1.0", "-t", "S:1.1"]).status.success());
    }

    #[test]
    fn panes_join_pane_rejects_flags_it_does_not_implement() {
        assert_pane_rejected("join-pane", "-f", &["join-pane", "-f", "-s", "S:2", "-t", "S:1.1"]);
    }

    #[test]
    fn panes_move_pane_flags_match_the_reference() {
        probes(
            "panes_move_pane_flags_match_the_reference",
            &[
                &[&["move-pane", "-s", "S:2", "-t", "S:1.1"]],
                &[&["movep", "-s", "S:2", "-t", "S:1.1", "-hb"]],
                &[&["move-pane", "-s", "S:2", "-t", "S:1.1", "-d", "-l", "5"]],
                &[&["move-pane", "-s", "S:2", "-t", "S:1.1", "-v"]],
                &[&["move-pane", "-s", "S:1.1", "-t", "S:2"]],
                &[&["move-pane", "-s", "nosuch", "-t", "S:2"]],
            ],
        );
    }

    #[test]
    fn panes_move_pane_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-f", &["move-pane", "-f", "-s", "S:2", "-t", "S:1.1"][..]),
            ("-p", &["move-pane", "-p", "30", "-s", "S:2", "-t", "S:1.1"][..]),
            ("-M", &["move-pane", "-M", "-s", "S:2", "-t", "S:1.1"][..]),
            ("-D", &["move-pane", "-D", "1", "-t", "S:1.1"][..]),
            ("-L", &["move-pane", "-L", "1", "-t", "S:1.1"][..]),
            ("-P", &["move-pane", "-P", "top", "-t", "S:1.1"][..]),
            ("-R", &["move-pane", "-R", "1", "-t", "S:1.1"][..]),
            ("-U", &["move-pane", "-U", "1", "-t", "S:1.1"][..]),
            ("-X", &["move-pane", "-X", "1", "-t", "S:1.1"][..]),
            ("-Y", &["move-pane", "-Y", "1", "-t", "S:1.1"][..]),
            ("-z", &["move-pane", "-z", "1", "-t", "S:1.1"][..]),
        ] {
            assert_pane_rejected("move-pane", flag, arguments);
        }
    }

    #[test]
    fn panes_swap_pane_flags_match_the_reference() {
        probes(
            "panes_swap_pane_flags_match_the_reference",
            &[
                &[&["swap-pane", "-s", "S:1.1", "-t", "S:1.2"]],
                &[&["swap-pane", "-s", "S:1.0", "-t", "S:1.2"]],
                &[&["swap-pane", "-s", "S:1.2", "-t", "S:1.0"]],
                &[&["swap-pane", "-d", "-s", "S:1.1", "-t", "S:1.2"]],
                &[&["swap-pane", "-s", "S:1.1", "-t", "S:1.1"]],
                &[&["swap-pane", "-D"]],
                &[&["swap-pane", "-U"]],
                &[&["swap-pane", "-D", "-t", "S:1.2"]],
                &[&["swap-pane", "-U", "-t", "S:1.0"]],
                &[&["swap-pane", "-U", "-t", "S:1.1"]],
                &[&["swap-pane", "-dD", "-t", "S:1.0"]],
                &[&["swap-pane", "-D", "-U", "-t", "S:1.1"]],
                &[&["swap-pane", "-D", "-s", "S:1.0", "-t", "S:1.1"]],
                &[&["swap-pane", "-s", "S:1.0", "-t", "S:2"]],
                &[&["swap-pane", "-s", "S:2", "-t", "S:1.1"]],
                &[&["swap-pane", "-d", "-s", "S:2", "-t", "S:1.1"]],
                &[&["swap-pane", "-s", "T:1", "-t", "S:1.1"]],
                &[&["swap-pane", "-s", "S:1.1", "-t", "T:1"]],
                &[&["swap-pane", "-s", "nosuch", "-t", "S:1.1"]],
                &[&["swap-pane", "-t", "S:1.1"]],
                &[&["swap-pane", "-s", "S:1.1"]],
                &[&["swap-pane"]],
                &[&["select-pane", "-m", "-t", "S:1.2"], &["swap-pane", "-t", "S:1.0"]],
                &[&["select-pane", "-m", "-t", "S:2"], &["swap-pane", "-t", "S:1.0"]],
                &[&["select-pane", "-m", "-t", "S:1.2"], &["swap-pane", "-t", "S:1.2"]],
                &[&["swap-pane", "-s", "S:1.1", "-t", "S:1.2", "extra"]],
            ],
        );
    }

    #[test]
    fn panes_swap_pane_rejects_flags_it_does_not_implement() {
        assert_pane_rejected("swap-pane", "-Z", &["swap-pane", "-Z", "-s", "S:1.1", "-t", "S:1.2"]);
    }

    #[test]
    fn panes_resize_pane_flags_match_the_reference() {
        probes(
            "panes_resize_pane_flags_match_the_reference",
            &[
                &[&["resize-pane", "-t", "S:1.0", "-R", "5"]],
                &[&["resize-pane", "-t", "S:1.0", "-L", "5"]],
                &[&["resize-pane", "-t", "S:1.1", "-L", "5"]],
                &[&["resize-pane", "-t", "S:1.1", "-U", "3"]],
                &[&["resize-pane", "-t", "S:1.2", "-U", "3"]],
                &[&["resize-pane", "-t", "S:1.1", "-D", "3"]],
                &[&["resize-pane", "-t", "S:1.1", "-D"]],
                &[&["resize-pane", "-t", "S:1.1", "-R"]],
                &[&["resize-pane", "-t", "S:1.1", "-D5"]],
                &[&["resize-pane", "-t", "S:1.1", "-D", "-1"]],
                &[&["resize-pane", "-t", "S:1.1", "-D", "x"]],
                &[&["resize-pane", "-t", "S:1.1", "-D", "5", "6"]],
                &[&["resize-pane", "-t", "S:1.1", "-D", "-R", "2"]],
                &[&["resize-pane", "-t", "S:1.1", "-D", "-L", "2"]],
                &[&["resize-pane", "-t", "S:1.1", "-U", "-D", "2"]],
                &[&["resize-pane", "-t", "S:1.0", "-x", "30"]],
                &[&["resize-pane", "-t", "S:1.0", "-x", "30%"]],
                &[&["resize-pane", "-t", "S:1.1", "-y", "5"]],
                &[&["resize-pane", "-t", "S:1.1", "-y", "50%"]],
                &[&["resize-pane", "-t", "S:1.0", "-x", "30", "-D", "2"]],
                &[&["resize-pane", "-t", "S:1.1", "-x", "30", "-y", "4"]],
                &[&["resize-pane", "-t", "S:1.1", "-y", "4", "-D", "2"]],
                &[&["resize-pane", "-t", "S:1.0", "-x", "x"]],
                &[&["resize-pane", "-t", "S:1.0", "-y", "0"]],
                &[&["resize-pane", "-t", "S:1.0", "-x", "0"]],
                &[&["resize-pane", "-t", "S:1.0", "-x", "999"]],
                &[&["resize-pane", "-t", "S:1.0", "-x", "101%"]],
                &[&["resize-pane", "-t", "S:1.0", "-x30"]],
                &[&["resize-pane", "-t", "S:1.0", "-Z"]],
                &[&["resize-pane", "-t", "S:1.0", "-Z"], &["resize-pane", "-t", "S:1.0", "-Z"]],
                &[&["resize-pane", "-t", "S:1.0", "-Z"], &["resize-pane", "-t", "S:1.1", "-R", "2"]],
                &[&["resize-pane", "-t", "S:1.0", "-Z", "-R", "2"]],
                &[&["resize-pane", "-t", "S:1.0", "-Z", "-x", "10"]],
                &[&["resize-pane", "-t", "S:2", "-R", "2"]],
                &[&["resize-pane", "-t", "S:2", "-x", "10"]],
                &[&["resize-pane", "-t", "S:2", "-Z"]],
                &[&["resize-pane", "-t", "nosuch", "-R"]],
                &[&["resize-pane", "-R", "3"]],
                &[&["resize-pane"]],
                &[&["resize-pane", "5"]],
                &[&["resize-pane", "-t", "S:1.0", "5"]],
                &[&["resizep", "-t", "S:1.0", "-R", "5"]],
                &[&["resize-pane", "-t", "S:1.0", "-R", "100"]],
                &[&["resize-pane", "-t", "S:1.0", "-R", "0"]],
                &[&["resize-pane", "-t", "S:1.0", "-L", "-3"]],
            ],
        );
    }

    #[test]
    fn panes_resize_pane_edges_and_zoom_match_the_reference() {
        probes(
            "panes_resize_pane_edges_and_zoom_match_the_reference",
            &[
                &[&["resize-pane", "-t", "S:1.1", "-Z"]],
                &[&["resize-pane", "-t", "S:1.1", "-Z"], &["resize-pane", "-t", "S:1.1", "-R", "2"]],
                &[&["resize-pane", "-t", "S:1.1", "-Z"], &["resize-pane", "-t", "S:1.1", "-Z"]],
                &[&["split-window", "-d", "-v", "-t", "S:2", "--", "sleep", "300"], &["resize-pane", "-t", "S:2.1", "-y", "5"]],
                &[&["split-window", "-d", "-v", "-t", "S:2", "--", "sleep", "300"], &["resize-pane", "-t", "S:2.0", "-U", "4"]],
                &[&["resize-pane", "-t", "S:1.1", "-x", "20", "-R", "3", "-U", "1"]],
                &[&["resize-pane", "-t", "S:1.2", "-y", "30%"]],
                &[&["resize-pane", "-t", "S:1.0", "-L", "1", "-R", "1"]],
            ],
        );
    }

    #[test]
    fn panes_resize_pane_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-M", &["resize-pane", "-M", "-t", "S:1.1"][..]),
            ("-T", &["resize-pane", "-T", "-t", "S:1.1"][..]),
        ] {
            assert_pane_rejected("resize-pane", flag, arguments);
        }
    }

    #[test]
    fn panes_capture_pane_flags_match_the_reference() {
        let test = "panes_capture_pane_flags_match_the_reference";
        for arguments in [
            &["capture-pane", "-p", "-t", "S"][..],
            &["capture-pane", "-t", "S"],
            &["capture-pane", "-b", "named", "-t", "S"],
            &["capture-pane", "-b", "named", "-p", "-t", "S"],
            &["capture-pane", "-e", "-t", "S"],
            &["capture-pane", "-J", "-t", "S"],
            &["capture-pane", "-N", "-t", "S"],
            &["capture-pane", "-pJN", "-t", "S"],
            &["capture-pane", "-S", "2", "-t", "S"],
            &["capture-pane", "-S", "1", "-E", "3", "-t", "S"],
            &["capture-pane", "-S", "-", "-E", "-", "-t", "S"],
            &["capture-pane", "-S", "-5", "-t", "S"],
            &["capture-pane", "-S", "x", "-t", "S"],
            &["capture-pane", "-E", "x", "-p", "-t", "S"],
            &["capture-pane", "-S", "9", "-E", "0", "-t", "S"],
            &["capture-pane", "-b", "", "-t", "S"],
            &["capture-pane", "-t", "nosuch"],
            &["capture-pane", "-t", "S", "extra"],
        ] {
            let pair = content_pair("cap");
            let before = pair.tm.body(&["list-buffers"]);
            assert_eq!(before, pair.reference.body(&["list-buffers"]));
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
            assert_eq!(
                observation_difference(
                    &format!("{test}: buffers after {arguments:?}"),
                    &buffer_observation(&pair.tm),
                    &buffer_observation(&pair.reference)
                ),
                None
            );
        }
        let pair = content_pair("capsame");
        for arguments in [
            &["capture-pane", "-b", "named", "-t", "S"][..],
            &["capture-pane", "-b", "named", "-t", "S"],
            &["capture-pane", "-t", "S"],
            &["capture-pane", "-t", "S"],
        ] {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
            assert_eq!(
                observation_difference(
                    &format!("{test}: buffers after {arguments:?}"),
                    &buffer_observation(&pair.tm),
                    &buffer_observation(&pair.reference)
                ),
                None
            );
        }
    }

    #[test]
    fn panes_capture_pane_rejects_flags_it_does_not_implement() {
        let pair = content_pair("caprej");
        for flag in ["-a", "-C", "-F", "-H", "-I", "-L", "-M", "-P", "-q", "-R", "-T"] {
            let output = pair.tm.run(&["capture-pane", "-p", flag, "-t", "S"]);
            assert!(!output.status.success(), "{flag}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for capture-pane\n")
            );
            assert!(output.stdout.is_empty(), "{flag}");
            assert_eq!(pair.tm.body(&["list-buffers"]), "", "{flag}");
        }
    }

    #[test]
    fn panes_clear_history_flags_match_the_reference() {
        let test = "panes_clear_history_flags_match_the_reference";
        let pair = Pair::new("clrhist");
        for mux in pair.both() {
            mux.ok(&[
                "new-session", "-d", "-s", "S", "-x", "20", "-y", "4", "--", "sh", "-c",
                "for i in 1 2 3 4 5 6 7 8; do echo line$i; done; echo ready; sleep 300",
            ]);
            assert!(mux.capture_until("S", "ready", TIMEOUT).contains("ready"));
        }
        let history = ["capture-pane", "-p", "-S", "-", "-t", "S"];
        compare_commands(test, &pair.tm, &pair.reference, &[&history]);
        compare_commands(test, &pair.tm, &pair.reference, &[&["clear-history", "-t", "S"], &history]);
        compare_commands(test, &pair.tm, &pair.reference, &[&["clearhist"], &history]);
        compare_commands(test, &pair.tm, &pair.reference, &[&["clear-history", "-t", "nosuch"]]);
        compare_commands(test, &pair.tm, &pair.reference, &[&["clear-history", "extra"]]);
        compare_commands(test, &pair.tm, &pair.reference, &[&["clear-history", "-t"]]);
    }

    #[test]
    fn panes_clear_history_rejects_flags_it_does_not_implement() {
        assert_pane_rejected("clear-history", "-H", &["clear-history", "-H", "-t", "S:1.1"]);
    }

    #[test]
    fn panes_respawn_pane_flags_match_the_reference() {
        let test = "panes_respawn_pane_flags_match_the_reference";
        let starts = ["list-panes", "-a", "-F", "#{session_name}:#{window_index}.#{pane_index} #{pane_start_command} #{pane_dead} #{pane_start_path}"];
        for steps in [
            &[&["respawn-pane", "-t", "S:1.1"][..]][..],
            &[&["respawn-pane", "-k", "-t", "S:1.1"]],
            &[&["respawn-pane", "-k", "-t", "S:1.1", "--", "sleep", "301"]],
            &[&["respawn-pane", "-k", "-t", "S:1.1", "sleep 302"]],
            &[&["respawn-pane", "-k", "-t", "S:1.1", "sh", "-c", "sleep 303"]],
            &[&["respawn-pane", "-kc", "/tmp", "-t", "S:1.1"]],
            &[&["respawn-pane", "-k", "-c", "/nonexistent", "-t", "S:1.1"]],
            &[&["respawn-pane", "-k", "-t", "S:1.1", ""]],
            &[&["respawnp", "-k", "-t", "S:2"]],
            &[&["respawn-pane", "-k", "-t", "nosuch"]],
            &[&["respawn-pane", "-k"]],
            &[&["respawn-pane", "-k", "-t", "S:1.0", "--", "sleep", "304"], &["respawn-pane", "-k", "-t", "S:1.0"]],
        ] {
            let pair = fixture("resp");
            for arguments in steps {
                compare_commands(test, &pair.tm, &pair.reference, &[arguments, &starts]);
                assert_eq!(
                    observation_difference(
                        &format!("{test}: topology after {arguments:?}"),
                        &pane_topology(&pair.tm),
                        &pane_topology(&pair.reference)
                    ),
                    None
                );
            }
        }
        let pair = fixture("respdead");
        for mux in pair.both() {
            mux.ok(&["set-option", "-g", "remain-on-exit", "on"]);
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "dead", "--", "true"]);
            let deadline = Instant::now() + TIMEOUT;
            while mux.body(&["display-message", "-p", "-t", "S:dead", "#{pane_dead}"]) != "1" {
                assert!(Instant::now() < deadline, "pane never died");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        for arguments in [
            &["respawn-pane", "-t", "S:dead"][..],
            &["respawn-pane", "-t", "S:dead", "--", "sleep", "305"],
        ] {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments, &starts]);
        }
    }

    #[test]
    fn panes_respawn_pane_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-d", &["respawn-pane", "-d", "-k", "-t", "S:1.1"][..]),
            ("-e", &["respawn-pane", "-e", "A=1", "-k", "-t", "S:1.1"][..]),
            ("-E", &["respawn-pane", "-E", "-k", "-t", "S:1.1"][..]),
        ] {
            assert_pane_rejected("respawn-pane", flag, arguments);
        }
    }

    #[test]
    fn panes_display_panes_needs_an_attached_client_in_tm() {
        let pair = fixture("dpclient");
        let before = pane_topology(&pair.tm);
        for (arguments, message) in [
            (&["display-panes"][..], "tm: chooser requires an attached client\n"),
            (&["display-panes", "select-pane -t %%"], "tm: chooser requires an attached client\n"),
            (&["display-panes", "-t", "S:1.1"], "tm: chooser target has no attached client\n"),
        ] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(String::from_utf8_lossy(&output.stderr), message, "{arguments:?}");
            assert_eq!(pane_topology(&pair.tm), before, "{arguments:?}");
        }
        assert!(pair.reference.run(&["display-panes", "-t", "S:1.1"]).status.success());
    }

    #[test]
    fn panes_display_panes_rejects_flags_it_does_not_implement() {
        for (flag, arguments) in [
            ("-d", &["display-panes", "-d", "100"][..]),
            ("-k", &["display-panes", "-k"][..]),
            ("-N", &["display-panes", "-N"][..]),
            ("-s", &["display-panes", "-s", "S:1"][..]),
            ("-Z", &["display-panes", "-Z"][..]),
        ] {
            assert_pane_rejected("display-panes", flag, arguments);
        }
    }

    fn pipe_file(mux: &Mux, name: &str) -> std::path::PathBuf {
        mux.root().join(name)
    }

    fn wait_file(path: &std::path::Path, expected: &str) -> String {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if text.contains(expected) || Instant::now() >= deadline {
                return text;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_pipe_flag(mux: &Mux, expected: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while mux.body(&["display-message", "-p", "-t", "S", "#{pane_pipe}"]) != expected {
            assert!(Instant::now() < deadline, "{:?} pane_pipe", mux.implementation());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn typed_through_pipe(mux: &Mux, name: &str, marker: &str, expect_pipe: bool) -> String {
        mux.ok(&["send-keys", "-t", "S", "-l", marker]);
        mux.ok(&["send-keys", "-t", "S", "Enter"]);
        assert!(mux.capture_until("S", marker, TIMEOUT).contains(marker));
        let path = pipe_file(mux, name);
        if expect_pipe {
            wait_file(&path, marker)
        } else {
            std::fs::read_to_string(&path).unwrap_or_default()
        }
    }

    struct PipeStep {
        arguments: Vec<String>,
        pipe_after: &'static str,
        marker: &'static str,
    }

    fn pipe_step(arguments: &[&str], pipe_after: &'static str, marker: &'static str) -> PipeStep {
        PipeStep {
            arguments: arguments.iter().map(|argument| (*argument).to_owned()).collect(),
            pipe_after,
            marker,
        }
    }

    fn run_pipe_cases(test: &str, cases: &[Vec<PipeStep>]) {
        for case in cases {
            let pair = Pair::new("pipe");
            for mux in pair.both() {
                mux.ok(&["new-session", "-d", "-s", "S", "-x", "40", "-y", "6", "--", "cat"]);
            }
            for step in case {
                let mut observed = Vec::new();
                for mux in pair.both() {
                    let out = mux.root().join("OUT").to_string_lossy().into_owned();
                    let arguments: Vec<String> = step
                        .arguments
                        .iter()
                        .map(|argument| argument.replace("OUT", &out))
                        .collect();
                    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
                    let outcome = mux.outcome(&arguments);
                    wait_pipe_flag(mux, step.pipe_after);
                    let expect_pipe = step.pipe_after == "1";
                    let name = if step.arguments.iter().any(|argument| argument.contains("OUT2")) {
                        "OUT2"
                    } else {
                        "OUT"
                    };
                    let text = typed_through_pipe(mux, name, step.marker, expect_pipe);
                    observed.push((
                        parity::normalize(&outcome.stderr, mux),
                        text,
                        outcome.status,
                    ));
                }
                assert_eq!(observed[0], observed[1], "{test}: {:?}", step.arguments);
            }
        }
    }

    #[test]
    fn panes_pipe_pane_flags_match_the_reference() {
        run_pipe_cases(
            "panes_pipe_pane_flags_match_the_reference",
            &[
                vec![pipe_step(&["pipe-pane", "-O", "-t", "S", "cat >> OUT"], "1", "m1")],
                vec![pipe_step(&["pipe-pane", "-t", "S", "--", "cat >> OUT"], "1", "m1")],
                vec![
                    pipe_step(&["pipe-pane", "-t", "S", "cat >> OUT"], "1", "m1"),
                    pipe_step(&["pipe-pane", "-t", "S", "cat >> OUT2"], "1", "m2"),
                ],
                vec![
                    pipe_step(&["pipe-pane", "-t", "S", "cat >> OUT"], "1", "m1"),
                    pipe_step(&["pipe-pane", "-t", "S"], "0", "m2"),
                ],
                vec![
                    pipe_step(&["pipe-pane", "-t", "S", "cat >> OUT"], "1", "m1"),
                    pipe_step(&["pipe-pane", "-t", "S", ""], "0", "m2"),
                ],
            ],
        );
    }

    #[test]
    fn panes_pipe_pane_toggle_matches_the_reference() {
        run_pipe_cases(
            "panes_pipe_pane_toggle_matches_the_reference",
            &[
                vec![
                    pipe_step(&["pipe-pane", "-o", "-t", "S", "cat >> OUT"], "1", "m1"),
                    pipe_step(&["pipe-pane", "-o", "-t", "S", "cat >> OUT"], "0", "m2"),
                    pipe_step(&["pipe-pane", "-o", "-t", "S", "cat >> OUT"], "1", "m3"),
                ],
                vec![
                    pipe_step(&["pipe-pane", "-t", "S", "cat >> OUT"], "1", "m1"),
                    pipe_step(&["pipe-pane", "-o", "-t", "S"], "0", "m2"),
                ],
            ],
        );
    }

    #[test]
    fn panes_pipe_pane_expands_formats_in_the_command() {
        let pair = Pair::new("pipeexp");
        for mux in pair.both() {
            mux.ok(&["new-session", "-d", "-s", "S", "-x", "40", "-y", "6", "--", "cat"]);
            let path = mux.root().join("EXPANDED");
            mux.ok(&["pipe-pane", "-t", "S", &format!("cat > {}_#{{pane_id}}", path.display())]);
            let text = typed_through_pipe(mux, "EXPANDED_%0", "mx", true);
            assert!(text.contains("mx"), "{:?}", mux.implementation());
        }
    }

    #[test]
    fn panes_pipe_pane_rejects_flags_it_does_not_implement() {
        assert_pane_rejected("pipe-pane", "-I", &["pipe-pane", "-I", "-t", "S:1.1", "cat"]);
        assert_pane_rejected("pipe-pane", "-I", &["pipe-pane", "-IO", "-t", "S:1.1", "cat"]);
    }
}

mod buffers {
    use super::*;

    fn buffer_pair(label: &str) -> Pair {
        let pair = Pair::new(label);
        for mux in pair.both() {
            mux.ok(&["new-session", "-d", "-s", "S", "-x", "40", "-y", "6", "--", "sleep", "300"]);
        }
        pair
    }

    fn run_steps(test: &str, pair: &Pair, steps: &[&[&str]]) {
        for arguments in steps {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
            assert_same_buffers(test, pair, arguments);
        }
    }

    fn write_both(pair: &Pair, name: &str, bytes: &[u8]) {
        for mux in pair.both() {
            std::fs::write(mux.root().join(name), bytes).expect("write fixture file");
        }
    }

    fn assert_rejected_everywhere(command: &str, cases: &[(&str, &[&str])]) {
        let pair = buffer_pair("bufrej");
        pair.tm.ok(&["set-buffer", "-b", "keep", "data"]);
        let before = buffer_observation(&pair.tm);
        for (flag, arguments) in cases {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?} succeeded");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for {command}\n"),
                "{arguments:?}"
            );
            assert!(output.stdout.is_empty(), "{arguments:?}");
            assert_eq!(buffer_observation(&pair.tm), before, "{arguments:?}");
        }
    }

    #[test]
    fn buffers_set_buffer_flags_match_the_reference() {
        let test = "buffers_set_buffer_flags_match_the_reference";
        let pair = buffer_pair("setb");
        run_steps(
            test,
            &pair,
            &[
                &["set-buffer"],
                &["set-buffer", ""],
                &["set-buffer", "-b", "named", ""],
                &["set-buffer", "-a", "first"],
                &["set-buffer", "second"],
                &["setb", "-b", "named", "gamma"],
                &["set-buffer", "-b", "named", "-a", "++"],
                &["set-buffer", "-a", "third"],
                &["set-buffer", "-b", "fresh", "-a", "x"],
                &["set-buffer", "-b", "buffer0", "-a", "!"],
                &["set-buffer", "-b", "named", "-n", "renamed"],
                &["set-buffer", "-n", "top"],
                &["set-buffer", "-b", "nosuch", "-n", "y"],
                &["set-buffer", "-b", "renamed", "-n", "top"],
                &["set-buffer", "-b", "renamed", "-n", ""],
                &["set-buffer", "-b", "", "x"],
                &["set-buffer", "-b", "renamed", "-n", "z", "ignored"],
                &["set-buffer", "a", "b"],
                &["set-buffer", "--", "-x"],
                &["set-buffer", "-b"],
                &["set-buffer", "-b", "tab\tname", "tabbed"],
                &["set-buffer", "-b", "back\\slash", "slashed"],
                &["set-buffer", "-b", "back\\slash", "-a", "+"],
                &["set-buffer", "-b", "back\\slash", "-n", "also\\"],
                &["set-buffer", "-b", "caf\u{e9}", "unicode name"],
                &["set-buffer", "-b", "ctl\u{7f}", "delete"],
                &["set-buffer", "-b", "back", "-n", "bad\nname"],
                &["set-buffer", "-b", "multi", "line1\nline2\n"],
                &["set-buffer", "h\u{e9}llo \u{1f600}"],
                &["set-buffer", "-bjoined", "attached"],
                &["set-buffer", "-ab", "joined", "more"],
            ],
        );
    }

    #[test]
    fn buffers_set_buffer_without_buffers_matches_the_reference() {
        let test = "buffers_set_buffer_without_buffers_matches_the_reference";
        let pair = buffer_pair("setbnone");
        run_steps(
            test,
            &pair,
            &[&["set-buffer", "-n", "y"], &["set-buffer", "-b", "only", "-n", "y"]],
        );
    }

    #[test]
    fn buffers_set_buffer_rejects_flags_it_does_not_implement() {
        assert_rejected_everywhere(
            "set-buffer",
            &[
                ("-w", &["set-buffer", "-w", "data"]),
                ("-t", &["set-buffer", "-t", "client", "data"]),
            ],
        );
    }

    #[test]
    fn buffers_show_buffer_flags_match_the_reference() {
        let test = "buffers_show_buffer_flags_match_the_reference";
        let pair = buffer_pair("showb");
        run_steps(
            test,
            &pair,
            &[
                &["show-buffer"],
                &["show-buffer", "-b", "named"],
                &["set-buffer", "plain"],
                &["set-buffer", "-b", "named", "with\nnewline\n"],
                &["show-buffer"],
                &["showb", "-b", "named"],
                &["show-buffer", "-b", "nosuch"],
                &["show-buffer", "-b", ""],
                &["show-buffer", "extra"],
                &["show-buffer", "-b"],
            ],
        );
    }

    #[test]
    fn buffers_list_buffers_flags_match_the_reference() {
        let test = "buffers_list_buffers_flags_match_the_reference";
        let pair = buffer_pair("lsb");
        let long = "0123456789".repeat(25);
        run_steps(
            test,
            &pair,
            &[
                &["list-buffers"],
                &["list-buffers", "-F", "#{buffer_name}"],
                &["set-buffer", "plain"],
                &["set-buffer", "-b", "named", "second\nline"],
                &["set-buffer", "tab\there \u{1b}[0m \\ end"],
                &["set-buffer", "-b", "long", &long],
                &["set-buffer", "h\u{e9}llo \u{1f600}"],
                &["list-buffers"],
                &["lsb"],
                &["list-buffers", "-F", "#{buffer_name}:#{buffer_size}:#{buffer_sample}"],
                &["list-buffers", "-F", ""],
                &["list-buffers", "-f", "#{==:#{buffer_name},named}"],
                &["list-buffers", "-f", "#{m:*line*,#{buffer_sample}}", "-F", "#{buffer_name}"],
                &["list-buffers", "-f", "0"],
                &["list-buffers", "-f", ""],
                &["list-buffers", "-F"],
                &["list-buffers", "extra"],
                &["set-buffer", "-b", "named", "replaced"],
                &["set-buffer", "-b", "plain2", "p"],
                &["list-buffers", "-F", "#{buffer_name}"],
                &["delete-buffer", "-b", "buffer0"],
                &["set-buffer", "again"],
                &["list-buffers", "-F", "#{buffer_name}"],
            ],
        );
    }

    #[test]
    fn buffers_list_buffers_rejects_flags_it_does_not_implement() {
        assert_rejected_everywhere(
            "list-buffers",
            &[
                ("-O", &["list-buffers", "-O", "name"]),
                ("-r", &["list-buffers", "-r"]),
                ("-q", &["list-buffers", "-q"]),
            ],
        );
    }

    #[test]
    fn buffers_delete_buffer_flags_match_the_reference() {
        let test = "buffers_delete_buffer_flags_match_the_reference";
        let pair = buffer_pair("delb");
        run_steps(
            test,
            &pair,
            &[
                &["delete-buffer"],
                &["delete-buffer", "-b", "nosuch"],
                &["set-buffer", "-b", "keep", "named"],
                &["delete-buffer"],
                &["set-buffer", "one"],
                &["set-buffer", "two"],
                &["delete-buffer"],
                &["deleteb", "-b", "keep"],
                &["delete-buffer", "-b", ""],
                &["delete-buffer", "extra"],
                &["delete-buffer", "-b"],
            ],
        );
    }

    fn run_rooted_steps(test: &str, pair: &Pair, files: &[&str], steps: &[&[&str]]) {
        for step in steps {
            let outcomes = pair.both().map(|mux| {
                let root = mux.root().to_string_lossy().into_owned();
                let arguments = step
                    .iter()
                    .map(|argument| argument.replace("ROOT", &root))
                    .collect::<Vec<_>>();
                let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
                mux.outcome(&arguments)
            });
            assert_eq!(
                parity::first_difference(&outcomes[0], &outcomes[1]),
                None,
                "{test}: {step:?}"
            );
            assert_same_buffers(test, pair, step);
            let contents = pair.both().map(|mux| {
                files
                    .iter()
                    .map(|file| std::fs::read(mux.root().join(file)).ok())
                    .collect::<Vec<_>>()
            });
            assert_eq!(contents[0], contents[1], "{test}: files after {step:?}");
        }
    }

    #[test]
    fn buffers_load_buffer_flags_match_the_reference() {
        let test = "buffers_load_buffer_flags_match_the_reference";
        let pair = buffer_pair("loadb");
        write_both(&pair, "text", b"loaded\nlines\n");
        write_both(&pair, "empty", b"");
        write_both(&pair, "binary", b"\xff\x00\x1b[0m");
        for mux in pair.both() {
            std::fs::create_dir(mux.root().join("dir")).expect("dir");
        }
        run_rooted_steps(
            test,
            &pair,
            &[],
            &[
                &["load-buffer", "ROOT/text"],
                &["loadb", "-b", "named", "ROOT/text"],
                &["load-buffer", "ROOT/empty"],
                &["load-buffer", "-b", "emptyname", "ROOT/empty"],
                &["load-buffer", "ROOT/binary"],
                &["load-buffer", "ROOT/missing"],
                &["load-buffer", "-b", "never", "ROOT/missing"],
                &["load-buffer", "ROOT/dir"],
                &["load-buffer", "-b", "", "ROOT/text"],
                &["load-buffer"],
                &["load-buffer", "ROOT/text", "ROOT/text"],
                &["load-buffer", "-b"],
                &["load-buffer", "-b", "named", "ROOT/binary"],
                &["load-buffer", "-bjoined", "ROOT/text"],
            ],
        );
    }

    #[test]
    fn buffers_load_and_save_paths_are_not_format_expanded_in_tm() {
        let pair = buffer_pair("pathfmt");
        let literal = pair.tm.root().join("#{session_name}");
        std::fs::write(&literal, b"literal").expect("write");
        pair.tm.ok(&["load-buffer", &literal.to_string_lossy()]);
        assert_eq!(pair.tm.ok(&["show-buffer"]), "literal");
        let saved = pair.tm.root().join("saved-#{session_name}");
        pair.tm.ok(&["save-buffer", &saved.to_string_lossy()]);
        assert_eq!(std::fs::read(&saved).expect("saved"), b"literal");
    }

    #[test]
    fn buffers_show_buffer_replaces_non_utf8_bytes_in_tm() {
        let pair = buffer_pair("showbin");
        let file = pair.tm.root().join("binary");
        std::fs::write(&file, b"a\xffb").expect("write");
        pair.tm.ok(&["load-buffer", &file.to_string_lossy()]);
        assert_eq!(pair.tm.ok(&["show-buffer"]), "a\u{fffd}b");
    }

    #[test]
    fn buffers_load_buffer_rejects_flags_it_does_not_implement() {
        assert_rejected_everywhere(
            "load-buffer",
            &[
                ("-w", &["load-buffer", "-w", "file"]),
                ("-t", &["load-buffer", "-t", "client", "file"]),
            ],
        );
    }

    #[test]
    fn buffers_save_buffer_flags_match_the_reference() {
        let test = "buffers_save_buffer_flags_match_the_reference";
        let pair = buffer_pair("saveb");
        std::fs::create_dir(pair.tm.root().join("dir")).expect("dir");
        std::fs::create_dir(pair.reference.root().join("dir")).expect("dir");
        run_rooted_steps(
            test,
            &pair,
            &["a", "n", "relative", "also"],
            &[
                &["save-buffer", "ROOT/a"],
                &["set-buffer", "first\nline"],
                &["set-buffer", "-b", "named", "second"],
                &["save-buffer", "ROOT/a"],
                &["save-buffer", "-a", "ROOT/a"],
                &["save-buffer", "-a", "ROOT/fresh"],
                &["saveb", "-b", "named", "ROOT/n"],
                &["save-buffer", "-ab", "named", "ROOT/n"],
                &["save-buffer", "-b", "nosuch", "ROOT/n"],
                &["save-buffer", "-b", "", "ROOT/n"],
                &["save-buffer"],
                &["save-buffer", "ROOT/a", "ROOT/n"],
                &["save-buffer", "ROOT/missing/x"],
                &["save-buffer", "ROOT/dir"],
                &["save-buffer", "-"],
                &["save-buffer", "-b", "named", "-"],
                &["save-buffer", "relative"],
                &["save-buffer", "-a", "relative"],
                &["save-buffer", "-b"],
                &["save-buffer", "--", "also"],
            ],
        );
    }

    #[test]
    fn buffers_save_buffer_rejects_flags_it_does_not_implement() {
        assert_rejected_everywhere(
            "save-buffer",
            &[("-x", &["save-buffer", "-x", "file"]), ("-w", &["save-buffer", "-w", "file"])],
        );
    }

    #[test]
    fn buffers_commands_reject_flags_tmux_does_not_define() {
        assert_rejected_everywhere(
            "show-buffer",
            &[("-x", &["show-buffer", "-x"]), ("-a", &["show-buffer", "-a"])],
        );
        assert_rejected_everywhere("delete-buffer", &[("-x", &["delete-buffer", "-x"])]);
        assert_rejected_everywhere("load-buffer", &[("-x", &["load-buffer", "-x", "file"])]);
        assert_rejected_everywhere("set-buffer", &[("-x", &["set-buffer", "-x", "data"])]);
        assert_rejected_everywhere("list-buffers", &[("-x", &["list-buffers", "-x"])]);
        assert_rejected_everywhere("paste-buffer", &[("-x", &["paste-buffer", "-x"])]);
    }

    #[test]
    fn buffers_paste_buffer_rejects_flags_it_does_not_implement() {
        assert_rejected_everywhere("paste-buffer", &[("-S", &["paste-buffer", "-S"])]);
    }

    #[test]
    fn buffers_paste_buffer_flags_match_the_reference() {
        pane_input_steps(
            "buffers_paste_buffer_flags_match_the_reference",
            &[
                &["set-buffer", "-b", "lines", "one\ntwo\n\nthree"],
                &["set-buffer", "-b", "trailing", "end\n"],
                &["set-buffer", "top\tword"],
            ],
            &[
                &["paste-buffer", "-t", "P"],
                &["paste-buffer", "-b", "lines", "-t", "P"],
                &["pasteb", "-b", "lines", "-t", "P"],
                &["paste-buffer", "-b", "trailing", "-t", "P"],
                &["paste-buffer", "-r", "-b", "lines", "-t", "P"],
                &["paste-buffer", "-s", "|", "-b", "lines", "-t", "P"],
                &["paste-buffer", "-s", "", "-b", "lines", "-t", "P"],
                &["paste-buffer", "-rs", "<>", "-b", "lines", "-t", "P"],
                &["paste-buffer", "-p", "-b", "lines", "-t", "P"],
                &["paste-buffer", "-p", "-r", "-b", "trailing", "-t", "P"],
                &["paste-buffer", "-b", "nosuch", "-t", "P"],
                &["paste-buffer", "-t", "nosuch"],
                &["paste-buffer", "-b", "lines", "-t", "nosuch"],
                &["paste-buffer", "extra", "-t", "P"],
                &["paste-buffer", "-b"],
                &["paste-buffer", "-d", "-b", "trailing", "-t", "P"],
                &["paste-buffer", "-d", "-t", "P"],
                &["paste-buffer", "-t", "P"],
                &["paste-buffer", "-d", "-b", "nosuch", "-t", "P"],
            ],
        );
    }

    #[test]
    fn buffers_paste_buffer_without_buffers_matches_the_reference() {
        pane_input_steps(
            "buffers_paste_buffer_without_buffers_matches_the_reference",
            &[],
            &[&["paste-buffer", "-t", "P"], &["paste-buffer", "-d", "-p", "-t", "P"]],
        );
    }

    #[test]
    fn buffers_paste_buffer_escapes_control_bytes_like_the_reference() {
        let controls = (1u8..0x20)
            .filter(|byte| *byte != b'\n')
            .chain([0x7f])
            .map(char::from)
            .collect::<String>();
        pane_input_steps(
            "buffers_paste_buffer_escapes_control_bytes_like_the_reference",
            &[
                &["set-buffer", "-b", "controls", &controls],
                &["set-buffer", "-b", "mixed", "a\u{1b}[31mred\\x\u{e9}\u{1f600}\u{85}\nb\tc"],
            ],
            &[
                &["paste-buffer", "-b", "controls", "-t", "P"],
                &["paste-buffer", "-b", "mixed", "-t", "P"],
                &["paste-buffer", "-r", "-b", "mixed", "-t", "P"],
            ],
        );
    }
}

mod keys {
    use super::*;

    fn assert_rejected(command: &str, cases: &[(&str, &[&str])]) {
        let pair = Pair::new("keysrej");
        pair.start("S");
        let before = topology(&pair.tm);
        for (flag, arguments) in cases {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?} succeeded");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for {command}\n"),
                "{arguments:?}"
            );
            assert!(output.stdout.is_empty(), "{arguments:?}");
            assert_eq!(topology(&pair.tm), before, "{arguments:?}");
        }
    }

    #[test]
    fn keys_send_keys_key_names_and_literals_match_the_reference() {
        pane_input_steps(
            "keys_send_keys_key_names_and_literals_match_the_reference",
            &[],
            &[
                &["send-keys", "-t", "P", "abc"],
                &["send", "-t", "P", "a", "b", "c"],
                &["send-keys", "-t", "P", "-l", "a b"],
                &["send-keys", "-t", "P", "-l", "a", "b"],
                &["send-keys", "-t", "P", "-l", "Enter"],
                &["send-keys", "-t", "P", "-l", "h\u{e9}llo \u{1f600}"],
                &["send-keys", "-t", "P", "Enter", "Tab", "Space", "Escape"],
                &["send-keys", "-t", "P", "C-a", "C-z", "C-[", "C-Space", "C-@", "C-A"],
                &["send-keys", "-t", "P", "M-x", "M-a"],
                &["send-keys", "-t", "P", "Up", "Down", "Left", "Right", "BTab"],
                &["send-keys", "-t", "P", "NoSuchKey", "x"],
                &["send-keys", "-t", "P", ""],
                &["send-keys", "-t", "P", "--", "-l"],
                &["send-keys", "-t", "P", "-l", "--", "-x"],
                &["send-keys", "-tP", "attached"],
                &["send-keys", "-t", "nosuch", "a"],
                &["send-keys", "-t", "P"],
                &["send-keys", "-t"],
            ],
        );
    }

    #[test]
    fn keys_send_keys_repeat_count_matches_the_reference() {
        pane_input_steps(
            "keys_send_keys_repeat_count_matches_the_reference",
            &[],
            &[
                &["send-keys", "-t", "P", "-N", "3", "ab"],
                &["send-keys", "-t", "P", "-N3", "x", "y"],
                &["send-keys", "-t", "P", "-lN", "2", "ab", "cd"],
                &["send-keys", "-t", "P", "-N", "1", "Enter"],
                &["send-keys", "-t", "P", "-N", "2"],
                &["send-keys", "-t", "P", "-N", "0", "x"],
                &["send-keys", "-t", "P", "-N", "-1", "x"],
                &["send-keys", "-t", "P", "-N", "x", "x"],
                &["send-keys", "-t", "P", "-N", "", "x"],
                &["send-keys", "-t", "P", "-N", "99999999999", "x"],
                &["send-keys", "-t", "P", "-N", "4294967296", "x"],
                &["send-keys", "-t", "P", "-N"],
            ],
        );
    }

    #[test]
    fn keys_send_keys_reset_matches_the_reference() {
        pane_input_steps(
            "keys_send_keys_reset_matches_the_reference",
            &[&["set-buffer", "-b", "text", "pasted"]],
            &[
                &["paste-buffer", "-p", "-b", "text", "-t", "P"],
                &["send-keys", "-t", "P", "-R"],
                &["paste-buffer", "-p", "-b", "text", "-t", "P"],
                &["send-keys", "-R", "-t", "P", "after"],
                &["paste-buffer", "-p", "-b", "text", "-t", "P"],
            ],
        );
    }

    #[test]
    fn keys_send_keys_copy_mode_command_matches_the_reference() {
        pane_input_steps(
            "keys_send_keys_copy_mode_command_matches_the_reference",
            &[],
            &[
                &["send-keys", "-t", "P", "-X", "cancel"],
                &["send-keys", "-t", "P", "-X"],
                &["send-keys", "-t", "P", "-N", "2", "-X", "cursor-down"],
                &["send-keys", "-t", "nosuch", "-X", "cancel"],
            ],
        );
        let pair = Pair::new("keyscopy");
        for mux in pair.both() {
            mux.ok(&["new-session", "-d", "-s", "S", "-x", "30", "-y", "6", "--", "sleep", "300"]);
            mux.ok(&["copy-mode", "-t", "S"]);
        }
        for arguments in [
            &["send-keys", "-t", "S", "-X"][..],
            &["send-keys", "-t", "S", "-N", "2", "-X", "cursor-left"],
            &["send-keys", "-t", "S", "-X", "cancel"],
            &["send-keys", "-t", "S", "-X", "cancel"],
        ] {
            compare_commands("keys_send_keys_copy_mode_command_matches_the_reference", &pair.tm, &pair.reference, &[arguments, &["display-message", "-p", "-t", "S", "#{pane_in_mode}"]]);
        }
    }

    #[test]
    fn keys_send_keys_copy_selection_flags_match_the_reference() {
        let test = "keys_send_keys_copy_selection_flags_match_the_reference";
        let pair = Pair::new("keyssel");
        for mux in pair.both() {
            mux.ok(&[
                "new-session", "-d", "-s", "S", "-x", "30", "-y", "6", "--", "sh", "-c",
                "printf 'alpha\\nbeta\\nready\\n'; sleep 300",
            ]);
            assert!(mux.capture_until("S", "ready", TIMEOUT).contains("ready"));
        }
        for finish in [
            &["send-keys", "-t", "S", "-X", "copy-selection-and-cancel", "-P"][..],
            &["send-keys", "-t", "S", "-X", "copy-selection-and-cancel", "-C"],
            &["send-keys", "-t", "S", "-X", "copy-selection-and-cancel", "-PC"],
            &["send-keys", "-t", "S", "-X", "copy-selection-and-cancel", "-P", "-C"],
            &["send-keys", "-t", "S", "-X", "copy-selection-and-cancel"],
            &["send-keys", "-t", "S", "-X", "copy-selection", "-C", "named-"],
        ] {
            for mux in pair.both() {
                mux.ok(&["copy-mode", "-t", "S"]);
                mux.ok(&["send-keys", "-t", "S", "-X", "top-line"]);
                mux.ok(&["send-keys", "-t", "S", "-X", "select-line"]);
            }
            compare_commands(test, &pair.tm, &pair.reference, &[finish]);
            for mux in pair.both() {
                let _ = mux.run(&["send-keys", "-t", "S", "-X", "cancel"]);
            }
            assert_same_buffers(test, &pair, finish);
        }
    }

    #[test]
    fn keys_send_keys_unknown_copy_mode_command_fails_in_tm() {
        let pair = Pair::new("keysunknown");
        pair.start("S");
        pair.tm.ok(&["copy-mode", "-t", "S"]);
        let output = pair.tm.run(&["send-keys", "-t", "S", "-X", "nosuch-command"]);
        assert!(!output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "tm: unknown copy-mode command: nosuch-command\n"
        );
    }

    #[test]
    fn keys_send_keys_rejects_flags_it_does_not_implement() {
        assert_rejected(
            "send-keys",
            &[
                ("-c", &["send-keys", "-c", "client", "-t", "S", "x"]),
                ("-F", &["send-keys", "-F", "-t", "S", "x"]),
                ("-H", &["send-keys", "-H", "-t", "S", "41"]),
                ("-K", &["send-keys", "-K", "-t", "S", "x"]),
                ("-M", &["send-keys", "-M", "-t", "S"]),
                ("-C", &["send-keys", "-C", "-t", "S", "x"]),
                ("-P", &["send-keys", "-P", "-t", "S", "x"]),
                ("-x", &["send-keys", "-x", "-t", "S", "x"]),
            ],
        );
    }

    #[test]
    fn keys_send_prefix_flags_match_the_reference() {
        let pair = Pair::new("sendprefix");
        for mux in pair.both() {
            let file = mux.root().join("pasted");
            let script = format!("stty raw -echo; echo ready; exec cat > {}", file.display());
            mux.ok(&["new-session", "-d", "-s", "S", "-x", "30", "-y", "6", "--", "sh", "-c", &script]);
            assert!(mux.capture_until("S", "ready", TIMEOUT).contains("ready"));
            mux.ok(&["new-window", "-d", "-t", "S", "--", "sleep", "300"]);
        }
        for arguments in [
            &["send-prefix"][..],
            &["send-prefix", "-t", "S:1"],
            &["send-prefix", "-tS:1"],
            &["send-prefix", "-t", "nosuch"],
            &["send-prefix", "extra"],
            &["send-prefix", "-t"],
        ] {
            compare_commands("keys_send_prefix_flags_match_the_reference", &pair.tm, &pair.reference, &[arguments]);
        }
        let seen = pair.both().map(|mux| {
            mux.ok(&["send-keys", "-t", "S:1", "-l", "Z"]);
            let file = mux.root().join("pasted");
            let deadline = Instant::now() + TIMEOUT;
            loop {
                let bytes = std::fs::read(&file).unwrap_or_default();
                if bytes.ends_with(b"Z") || Instant::now() >= deadline {
                    break bytes;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        assert_eq!(seen[0], seen[1]);
        assert!(seen[0].starts_with(&[1, 1]), "{:?}", seen[0]);
    }

    #[test]
    fn keys_send_prefix_rejects_the_secondary_prefix() {
        assert_rejected("send-prefix", &[("-2", &["send-prefix", "-2", "-t", "S"])]);
    }
}

mod display {
    use super::*;

    fn probes(test: &str, commands: &[&[&str]]) {
        let pair = Pair::new("display");
        pair.start("S");
        for mux in pair.both() {
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "two", "--", "sleep", "300"]);
        }
        for arguments in commands {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
        }
    }

    #[test]
    fn display_display_message_flags_match_the_reference() {
        probes(
            "display_display_message_flags_match_the_reference",
            &[
                &["display-message", "-p", "#{session_name}:#{window_index}.#{pane_index}"],
                &["display", "-p", "#{window_name}"],
                &["display-message", "-p", "-t", "S:two", "#{window_name}"],
                &["display-message", "-pt", "S:two", "#{window_name}"],
                &["display-message", "-p", "-F", "#{window_name}"],
                &["display-message", "-pF", "#{window_name}", "-t", "S:two"],
                &["display-message", "-p", "-F", "#{window_name}", "extra"],
                &["display-message", "-p", "-l", "#{window_name} literal"],
                &["display-message", "-pl", "-t", "S:two", "#{window_name}"],
                &["display-message", "-p", "--", "-dash"],
                &["display-message", "-p", "%Y"],
                &["display-message", "-p", "100%%|%H:%M|%d-%b-%y"],
                &["display-message", "-p", "%"],
                &["display-message", "-l", "-p", "%Y"],
                &["display-message", "-p", ""],
                &["display-message", "-p", "a", "b"],
                &["display-message", "-p"],
                &["display-message", "-p", "-t"],
                &["display-message", "-p", "#{window_name}", "-t", "S:two"],
                &["display-message", "#{window_name}"],
                &["display-message", "-t", "S:two", "#{window_name}"],
            ],
        );
    }

    #[test]
    fn display_display_message_default_template_matches_the_reference() {
        let pair = Pair::new("displaydefault");
        pair.start("S");
        let tm = pair.tm.body(&["display-message", "-p"]);
        let reference = pair.reference.body(&["display-message", "-p"]);
        let (tm_head, _) = tm.rsplit_once(" - (").expect("template tail");
        let (reference_head, _) = reference.rsplit_once(" - (").expect("template tail");
        assert_eq!(tm_head, reference_head);
        assert_eq!(tm.len(), reference.len());
    }

    #[test]
    fn display_display_message_unresolved_targets_fail_in_tm() {
        let pair = Pair::new("displayfail");
        pair.start("S");
        for (target, message) in [
            ("nosuch", "can't find pane: nosuch"),
            ("S:nosuch", "can't find window: nosuch"),
            ("S:1.7", "can't find pane: 7"),
            ("%9", "can't find pane: %9"),
        ] {
            let output = pair.tm.run(&["display-message", "-p", "-t", target, "#{pane_id}"]);
            assert!(!output.status.success(), "{target}");
            assert_eq!(String::from_utf8_lossy(&output.stderr), format!("tm: {message}\n"));
            assert!(output.stdout.is_empty(), "{target}");
        }
    }

    #[test]
    fn display_display_message_without_p_prints_nothing_in_tm() {
        let pair = Pair::new("displayquiet");
        pair.start("S");
        let output = pair.tm.run(&["display-message", "#{pane_id}"]);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn display_display_message_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("displayrej");
        pair.start("S");
        for (flag, arguments) in [
            ("-a", &["display-message", "-p", "-a"][..]),
            ("-C", &["display-message", "-C", "x"]),
            ("-c", &["display-message", "-p", "-c", "client", "x"]),
            ("-d", &["display-message", "-d", "10", "x"]),
            ("-I", &["display-message", "-I"]),
            ("-j", &["display-message", "-p", "-j", "{}"]),
            ("-N", &["display-message", "-N", "x"]),
            ("-v", &["display-message", "-p", "-v", "x"]),
            ("-v", &["display-message", "-pv", "x"]),
            ("-x", &["display-message", "-p", "-x", "x"]),
        ] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for display-message\n"),
                "{arguments:?}"
            );
            assert!(output.stdout.is_empty(), "{arguments:?}");
        }
    }

    #[test]
    fn display_run_shell_flags_match_the_reference() {
        let pair = Pair::new("runshell");
        pair.start("S");
        for mux in pair.both() {
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "two", "--", "sleep", "300"]);
        }
        let test = "display_run_shell_flags_match_the_reference";
        for arguments in [
            &["run-shell", "echo hi"][..],
            &["run", "printf a"],
            &["run-shell", "printf 'a\\nb\\n'"],
            &["run-shell", "printf 'no newline'"],
            &["run-shell", "printf '\\n\\n'"],
            &["run-shell", "printf ''"],
            &["run-shell", "true"],
            &["run-shell", "false"],
            &["run-shell", "exit 3"],
            &["run-shell", "printf out; exit 2"],
            &["run-shell", "echo err >&2"],
            &["run-shell", "echo err >&2; exit 4"],
            &["run-shell", "kill -9 $$"],
            &["run-shell", "echo #{session_name}:#{window_index}"],
            &["run-shell", "-t", "S:two", "echo #{window_name}"],
            &["run-shell", "-t", "S:two", "echo hidden; exit 5"],
            &["run-shell", "pwd"],
            &["run-shell", "echo $0"],
            &["run-shell", "-b", "echo hidden"],
            &["run-shell", "-b", "exit 3"],
            &["run-shell", "-bd", "0.1", "echo hidden"],
            &["run-shell", "-d", "0.1", "echo delayed"],
            &["run-shell", "-d", "0", "echo delayed"],
            &["run-shell", "-d", "1e-1", "echo delayed"],
            &["run-shell", "-d", "x", "echo no"],
            &["run-shell", "-d", "1x", "echo no"],
            &["run-shell", "-d", "0.1"],
            &["run-shell", "-d"],
            &["run-shell", "-s", "ignored", "echo hi"],
            &["run-shell", "--", "echo dashed"],
            &["run-shell"],
            &["run-shell", "-t", "S:two"],
            &["run-shell", "-b"],
        ] {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
        }
        for mux in pair.both() {
            let marker = mux.root().join("marker");
            let command = format!("touch {}", marker.display());
            mux.ok(&["run-shell", "-b", &command]);
            let deadline = Instant::now() + TIMEOUT;
            while !marker.exists() {
                assert!(Instant::now() < deadline, "background command never ran");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn display_run_shell_delay_waits_in_both() {
        let pair = Pair::new("runshelldelay");
        pair.start("S");
        for mux in pair.both() {
            let started = Instant::now();
            mux.ok(&["run-shell", "-d", "0.3", "true"]);
            assert!(started.elapsed() >= Duration::from_millis(300), "{:?}", mux.implementation());
            let started = Instant::now();
            mux.ok(&["run-shell", "-bd", "0.3", "true"]);
            assert!(started.elapsed() < Duration::from_millis(300), "{:?}", mux.implementation());
        }
    }

    #[test]
    fn display_run_shell_joins_arguments_and_fails_unresolved_targets_in_tm() {
        let pair = Pair::new("runshelltm");
        pair.start("S");
        assert_eq!(pair.tm.ok(&["run-shell", "echo", "joined", "words"]), "joined words\n");
        let output = pair.tm.run(&["run-shell", "-t", "nosuch", "echo hidden"]);
        assert!(!output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stderr), "tm: can't find pane: nosuch\n");
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn display_run_shell_rejects_flags_it_does_not_implement() {
        let pair = Pair::new("runshellrej");
        pair.start("S");
        for (flag, arguments) in [
            ("-C", &["run-shell", "-C", "display-message -p x"][..]),
            ("-E", &["run-shell", "-E", "echo x"]),
            ("-c", &["run-shell", "-c", "/", "echo x"]),
            ("-x", &["run-shell", "-x", "echo x"]),
        ] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for run-shell\n"),
                "{arguments:?}"
            );
            assert!(output.stdout.is_empty(), "{arguments:?}");
        }
    }
}

mod options {
    use super::*;

    fn fixture(label: &str) -> Pair {
        let pair = Pair::new(label);
        pair.start("S");
        pair.start("T");
        pair
    }

    fn compare_all(test: &str, pair: &Pair, commands: &[&[&str]]) {
        for arguments in commands {
            compare_commands(test, &pair.tm, &pair.reference, &[arguments]);
        }
    }

    fn assert_rejected_in_tm(pair: &Pair, command: &str, cases: &[(&str, &[&str])]) {
        for (flag, arguments) in cases {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for {command}\n"),
                "{arguments:?}"
            );
            assert!(output.stdout.is_empty(), "{arguments:?}");
        }
    }

    #[test]
    fn options_show_options_flags_match_the_reference() {
        let pair = fixture("optshow");
        compare_all(
            "options_show_options_flags_match_the_reference",
            &pair,
            &[
            &["show-options", "-v", "mouse"],
            &["show-options", "-g", "mouse"],
            &["show-options", "-s", "mouse"],
            &["show-options", "-w", "mouse"],
            &["show-options", "-p", "mouse"],
            &["show-options", "-gw", "mouse"],
            &["show-options", "-gw", "mode-keys"],
            &["show-options", "-gs", "mode-keys"],
            &["show-options", "mode-keys"],
            &["show-options", "-wv", "mode-keys"],
            &["show-options", "-gp", "mode-keys"],
            &["show-options", "-s", "buffer-limit"],
            &["show-options", "-sv", "buffer-limit"],
            &["show-options", "-sq", "buffer-limit"],
            &["show-options", "-sv", "escape-time"],
            &["show-options", "-A", "mouse"],
            &["show-options", "-gA", "mode-keys"],
            &["show-options", "-wA", "mode-keys"],
            &["show-options", "-pA", "mode-keys"],
            &["show-options", "-gv", "history-limit"],
            &["show-options", "-gv", "status-left"],
            &["show-options", "-gv", "status-right"],
            &["show-options", "-g", "status-right"],
            &["show-options", "-gqv", "nosuch"],
            &["show-options", "-gq", "nosuch"],
            &["show-options", "nosuch"],
            &["show-options", "-sq", "nosuch"],
            &["show-options", "-t", "nosuch", "mouse"],
            &["show-options", "-t", "nosuch", "-g", "mouse"],
            &["show-options", "-wt", "S:0", "mode-keys"],
            &["show-options", "-wt", "S:1", "mode-keys"],
            &["show-options", "-wAt", "S:1", "mode-keys"],
            &["show-options", "-pt", "S:1.0", "-A", "mode-keys"],
            &["show-options", "-gv", "synchronize-panes"],
            &["show-options", "-wv", "synchronize-panes"],
            &["show-options", "-gv", "remain-on-exit"],
            &["show-options", "-gv", "word-separators"],
            &["show-options", "-gv", "extended-keys"],
            &["show-options", "-sv", "set-clipboard"],
            &["show-options", "-sv", "focus-events"],
            &["show-options", "a", "b"],
            &["show-options", "--", "mouse"],
            &["show-options", "-gv", "mou"],
            &["show-options", "-gv", "@foo"],
            &["show-options", "-gqv", "@foo"],
            &["show-options", "-g", "@foo"],
            &["show-options", "-t"],
            &["show-options", "-gv", "base-index"],
            &["show-options", "-v", "base-index"],
            &["show-options", "-Av", "base-index"],
            &["show-options", "-sv", "default-terminal"],
            &["show-options", "-sv", "terminal-features"],
            &["show-options", "-s", "terminal-features"],
            &["show-options", "-gv", "renumber-windows"],
            &["show-window-options", "-gv", "mode-keys"],
            &["show-window-options", "-v", "mode-keys"],
            &["show-window-options", "mode-keys"],
            &["show-window-options", "-g", "mode-keys"],
            &["show-window-options", "-t", "S:1", "mode-keys"],
            &["show-window-options", "-t", "S:0", "mode-keys"],
            &["show-window-options", "-gv", "mouse"],
            ],
        );
    }

    #[test]
    fn options_set_option_flags_match_the_reference() {
        let pair = fixture("optset");
        compare_all(
            "options_set_option_flags_match_the_reference",
            &pair,
            &[
            &["set-option"],
            &["set-option", "mouse"],
            &["set-option", "-g", "mouse"],
            &["show-options", "-g", "mouse"],
            &["set-option", "-g", "mouse"],
            &["show-options", "-g", "mouse"],
            &["set-option", "-g", "mouse", "1"],
            &["show-options", "-g", "mouse"],
            &["set-option", "-g", "mouse", "YES"],
            &["show-options", "-g", "mouse"],
            &["set-option", "-g", "mouse", "No"],
            &["show-options", "-g", "mouse"],
            &["set-option", "-g", "mouse", "0"],
            &["show-options", "-g", "mouse"],
            &["set-option", "-g", "mouse", ""],
            &["show-options", "-g", "mouse"],
            &["set-option", "-g", "status-left"],
            &["set-option", "-g", "history-limit"],
            &["set-option", "-g", "mode-keys"],
            &["show-options", "-gv", "mode-keys"],
            &["set-option", "-gw", "mode-keys", "vi"],
            &["show-options", "-gv", "mode-keys"],
            &["set-option", "-g", "mode-keys", "emacs"],
            &["set-option", "-u", "mouse"],
            &["set-option", "-gu", "nosuch"],
            &["set-option", "-gq", "nosuch", "on"],
            &["set-option", "nosuch", "on"],
            &["set-option", "-g", "mou", "off"],
            &["set-option", "-g", "a", "x"],
            &["set-option", "-g", "@foo", "bar"],
            &["show-options", "-gv", "@foo"],
            &["set-option", "@foo", "bar"],
            &["show-options", "@foo"],
            &["show-options", "-v", "@foo"],
            &["set-option", "-w", "@foo", "bar"],
            &["show-options", "-w", "@foo"],
            &["set-option", "-s", "@foo", "bar"],
            &["show-options", "-s", "@foo"],
            &["set-option", "-p", "@foo", "bar"],
            &["show-options", "-p", "@foo"],
            &["set-option", "-gu", "@foo"],
            &["show-options", "-gv", "@foo"],
            &["set-option", "-g", "@foo"],
            &["set-option", "-t", "nosuch", "mouse", "on"],
            &["set-option", "-t", "nosuch", "-g", "mouse", "on"],
            &["set-option", "-qt", "nosuch", "mouse", "on"],
            &["set-option", "-s", "mouse", "on"],
            &["set-option", "-w", "mouse", "on"],
            &["show-options", "mouse"],
            &["show-options", "-w", "mouse"],
            &["set-option", "-s", "mode-keys", "vi"],
            &["show-options", "-wv", "mode-keys"],
            &["set-option", "-w", "buffer-limit", "5"],
            &["show-options", "-sv", "buffer-limit"],
            &["set-option", "-s", "buffer-limit", "7"],
            &["show-options", "-sv", "buffer-limit"],
            &["set-option", "buffer-limit", "9"],
            &["show-options", "-gv", "buffer-limit"],
            &["set-option", "-g", "mouse", "maybe"],
            &["set-option", "-g", "mode-keys", "foo"],
            &["set-option", "mouse", "on", "off"],
            &["set-option", "-gt", "S:", "mouse", "on"],
            &["set-option", "-t", "S:1", "mode-keys", "vi"],
            &["show-options", "-wv", "-t", "S:1", "mode-keys"],
            &["show-options", "-wv", "-t", "S:2", "mode-keys"],
            &["set-option", "-t", "S:9", "mode-keys", "vi"],
            &["set-option", "-t", "S:1", "-u", "mode-keys"],
            &["show-options", "-wAv", "-t", "S:1", "mode-keys"],
            &["set-option", "-p", "-t", "S:1.0", "mode-keys", "vi"],
            &["set-option", "-p", "-t", "S:1.0", "synchronize-panes", "on"],
            &["set-option", "-p", "-t", "S:1.9", "synchronize-panes", "on"],
            &["set-option", "-pt", "S:1.0", "@pp", "x"],
            &["show-options", "-pt", "S:1.0", "@pp"],
            &["show-options", "-pt", "S:1.0", "-A", "mode-keys"],
            &["set-option", "-t", "S", "mouse", "on"],
            &["show-options", "-t", "S", "mouse"],
            &["show-options", "-t", "T", "mouse"],
            &["show-options", "-t", "S", "-A", "mouse"],
            &["set-option", "-u", "-t", "S", "mouse"],
            &["show-options", "-t", "S", "mouse"],
            &["set-option", "-t", "S", "base-index", "3"],
            &["show-options", "-t", "S", "-v", "base-index"],
            &["show-options", "-t", "T", "-v", "base-index"],
            &["set-option", "-t", "S", "renumber-windows", "off"],
            &["show-options", "-t", "S", "renumber-windows"],
            &["set-window-option"],
            &["set-window-option", "mode-keys", "vi"],
            &["show-window-options", "mode-keys"],
            &["set-window-option", "-g", "mode-keys", "vi"],
            &["show-window-options", "-g", "mode-keys"],
            &["set-window-option", "-t", "S:9", "mode-keys", "vi"],
            &["set-window-option", "-t", "S:1", "mode-keys", "emacs"],
            &["set-window-option", "-t", "S:1.0", "mode-keys", "emacs"],
            &["set-window-option", "mouse", "on"],
            &["set-window-option", "-q", "nosuch", "vi"],
            &["set-window-option", "mode-keys"],
            &["set-window-option", "-u", "mode-keys"],
            &["show-window-options", "mode-keys"],
            &["set-option", "-gq", "mouse", "on"],
            &["set-option", "-g", "status-left", "a b"],
            &["show-options", "-g", "status-left"],
            &["set-option", "-g", "@foo", "x y"],
            &["show-options", "-g", "@foo"],
            ],
        );
    }

    #[test]
    fn options_values_are_validated_and_normalized_like_the_reference() {
        let pair = fixture("optvalue");
        compare_all(
            "options_values_are_validated_and_normalized_like_the_reference",
            &pair,
            &[
                &["set-option", "-g", "history-limit", "abc"],
                &["set-option", "-g", "history-limit", ""],
                &["set-option", "-g", "history-limit", "-1"],
                &["set-option", "-g", "history-limit", "2147483648"],
                &["set-option", "-g", "history-limit", "99999999999999999999999999999999999999999"],
                &["set-option", "-g", "history-limit", " 7"],
                &["set-option", "-g", "history-limit", "+8"],
                &["set-option", "-g", "history-limit", "007"],
                &["show-options", "-gv", "history-limit"],
                &["set-option", "-g", "history-limit", "12x"],
                &["set-option", "-g", "status-left-length", "32768"],
                &["set-option", "-g", "status-left-length", "32767"],
                &["show-options", "-gv", "status-left-length"],
                &["set-option", "-g", "display-panes-time", "0"],
                &["set-option", "-g", "base-index", "-1"],
                &["set-option", "-g", "status-position", "left"],
                &["set-option", "-g", "status-position", "top"],
                &["show-options", "-gv", "status-position"],
                &["set-option", "-g", "status-position"],
                &["show-options", "-gv", "status-position"],
                &["set-option", "-g", "status", "2"],
                &["set-option", "-g", "status", "3x"],
                &["set-option", "-g", "mode-keys", "vi"],
                &["set-option", "-g", "mode-keys"],
                &["show-options", "-gv", "mode-keys"],
                &["set-option", "-g", "mode-keys"],
                &["show-options", "-gv", "mode-keys"],
                &["set-option", "-g", "extended-keys", "always"],
                &["set-option", "-g", "extended-keys"],
                &["show-options", "-gv", "extended-keys"],
                &["set-option", "-g", "mouse", "TRUE"],
                &["set-option", "-g", "mouse", "On"],
                &["set-option", "-g", "mouse", "OFF"],
                &["show-options", "-gv", "mouse"],
                &["set-option", "-g", "mouse"],
                &["show-options", "-gv", "mouse"],
                &["set-option", "-g", "mouse", ""],
                &["show-options", "-gv", "mouse"],
                &["set-option", "-g", "mouse", "2"],
                &["set-option", "-g", "buffer-limit", "0"],
                &["set-option", "-g", "buffer-limit", "9"],
                &["show-options", "-gv", "buffer-limit"],
                &["set-option", "-g", "@empty", ""],
                &["show-options", "-g", "@empty"],
                &["show-options", "-gv", "@empty"],
            ],
        );
    }

    #[test]
    fn options_style_and_colour_values_are_validated_like_the_reference() {
        const STYLES: &[&str] = &[
            "fg=notacolour",
            "fg=red,bg=blue",
            "FG=Red, BG=Blue",
            "fg=#ff00AA,bg=#12345",
            "fg=#gg0000",
            "fg=colour256",
            "fg=colour255,bg=COLOUR0",
            "fg=color7,us=brightred",
            "fg=brightwhite,bg=90,us=default",
            "fg=terminal,bg=themered",
            "fg=AliceBlue,bg=darkslategray4,us=LightGoldenrod1",
            "fg=grey,bg=gray50,fill=grey101",
            "fg=orange1,bg=Orange",
            "fill=blue,fill=",
            "xg=red",
            "fg=",
            "bold,dim,underscore,reverse,italics,strikethrough,overline",
            "bold|dim,blink",
            "curly-underscore,double-underscore,dotted-underscore,dashed-underscore,acs",
            "bold|",
            "|bold",
            "bold,nonsense",
            "nobold,nodim,noreverse",
            "NOBOLD,Bold",
            "noattr,nolink,nolist,noalign,norange,noignore",
            "nonsense",
            "no",
            "nolink=x",
            "none",
            "default",
            "DEFAULT,Ignore,push-default,pop-default,set-default",
            "nodefault",
            "align=left,align=centre,align=right,align=absolute-centre",
            "align=middle",
            "align=",
            "list=on,list=focus,list=left-marker,list=right-marker",
            "list=off",
            "list=",
            "range=left,range=right",
            "range=left|1",
            "range=control|9",
            "range=control|10",
            "range=control",
            "range=pane|%12",
            "range=pane|12",
            "range=pane|%",
            "range=window|3",
            "range=window|x",
            "range=session|$4",
            "range=session|4",
            "range=user|anything",
            "range=user",
            "range=left|",
            "range=unknown",
            "range=unknown|x",
            "dim=50",
            "dim=50%",
            "dim=101",
            "dim=",
            "width=10,pad=2",
            "width=100%",
            "width=101%",
            "width=%",
            "width=-1",
            "pad=x",
            "link=https://example.com",
            "link=",
            "fg=red bg=blue\nbold",
            "  ,, fg=red ,, ",
            "",
            " ",
            "fg=#{status_fg_colour}",
            "fg=#{x},nonsense",
            "nonsense #{x}",
        ];
        const COLOURS: &[&str] = &[
            "red",
            "RED",
            "1",
            "9",
            "7",
            "colour5",
            "COLOUR255",
            "color7",
            "colour256",
            "colour 5",
            "colour+5",
            "colour",
            "#ff00AA",
            "#ff00a",
            "#gg0000",
            "default",
            "terminal",
            "themered",
            "ThemeLightGrey",
            "brightred",
            "90",
            "98",
            "AliceBlue",
            "darkslategray4",
            "grey",
            "gray50",
            "gray33",
            "grey101",
            "grey0",
            "orange",
            "Orange1",
            "not a colour",
            " red",
            "",
            "#{x}",
        ];
        let pair = fixture("optstylecolour");
        for (option, value) in [
            ("status-style", "bold"),
            ("window-style", "bold"),
            ("cursor-colour", "red"),
            ("status-bg", "red"),
            ("dark-theme-red", "red"),
            ("clock-mode-colour", "red"),
            ("status-fg", "red"),
        ] {
            for mux in pair.both() {
                mux.ok(&["set-option", "-g", option, value]);
            }
        }
        for style in STYLES {
            for option in ["status-style", "window-style"] {
                for arguments in [
                    vec!["set-option", "-g", option, style],
                    vec!["show-options", "-gv", option],
                ] {
                    compare_commands(
                        "options_style_and_colour_values_are_validated_like_the_reference",
                        &pair.tm,
                        &pair.reference,
                        &[&arguments],
                    );
                }
            }
        }
        for colour in COLOURS {
            for option in ["cursor-colour", "status-bg", "dark-theme-red", "clock-mode-colour"] {
                for arguments in [
                    vec!["set-option", "-g", option, colour],
                    vec!["show-options", "-gv", option],
                ] {
                    compare_commands(
                        "options_style_and_colour_values_are_validated_like_the_reference",
                        &pair.tm,
                        &pair.reference,
                        &[&arguments],
                    );
                }
            }
        }
        compare_all(
            "options_style_and_colour_values_are_validated_like_the_reference",
            &pair,
            &[
                &["set-option", "-g", "status-style"],
                &["set-option", "-g", "status-bg"],
                &["set-option", "-g", "cursor-colour"],
                &["set-option", "-g", "status-fg", "nonsense"],
                &["set-option", "-g", "status-fg", "ColoUr9"],
                &["show-options", "-gv", "status-fg"],
            ],
        );
    }

    #[test]
    fn options_show_options_quotes_values_like_the_reference() {
        let pair = fixture("optquote");
        for value in [
            "", "a b", "a\"b", "a'b", "a\\b", "a#b;c$d", "~x", " x", "{x}", "a=b,c", "\u{e9}",
            "a\nb", "a\tb", "#{x}", "(x)*?[y]<>|&!", "100%", "%",
        ] {
            compare_all(
                "options_show_options_quotes_values_like_the_reference",
                &pair,
                &[
                    &["set-option", "-g", "@quoted", value],
                    &["show-options", "-g", "@quoted"],
                    &["show-options", "-gv", "@quoted"],
                ],
            );
        }
    }

    #[test]
    fn options_compiled_values_match_the_reference() {
        let pair = fixture("optcompiled");
        let profile = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference/tmux-3.8.conf"),
        )
        .expect("reference profile");
        let mut checked = 0;
        for line in profile.lines() {
            let mut words = line.split_whitespace();
            if words.next() != Some("set") {
                continue;
            }
            let name = words.nth(1).expect("option name");
            compare_all(
                "options_compiled_values_match_the_reference",
                &pair,
                &[&["show-options", "-gv", name], &["show-options", "-g", name]],
            );
            checked += 1;
        }
        assert!(checked > 20, "{checked}");
    }

    #[test]
    fn options_session_window_and_pane_tables_are_separate() {
        let pair = fixture("opttables");
        for mux in pair.both() {
            mux.ok(&["new-window", "-d", "-t", "S", "-n", "two", "--", "sleep", "300"]);
            mux.ok(&["split-window", "-d", "-t", "S:two", "--", "sleep", "300"]);
        }
        compare_all(
            "options_session_window_and_pane_tables_are_separate",
            &pair,
            &[
                &["set-option", "-t", "S", "status-left", "SESSION"],
                &["show-options", "-t", "S", "-v", "status-left"],
                &["show-options", "-t", "T", "-v", "status-left"],
                &["show-options", "-t", "T", "-Av", "status-left"],
                &["show-options", "-t", "T", "-A", "status-left"],
                &["show-options", "-g", "-v", "status-left"],
                &["set-option", "-u", "-t", "S", "status-left"],
                &["show-options", "-t", "S", "-v", "status-left"],
                &["set-option", "-w", "-t", "S:two", "mode-keys", "vi"],
                &["show-options", "-w", "-t", "S:two", "mode-keys"],
                &["show-options", "-w", "-t", "S:1", "mode-keys"],
                &["show-options", "-wA", "-t", "S:1", "mode-keys"],
                &["show-window-options", "-t", "S:two", "mode-keys"],
                &["show-window-options", "-t", "S:two", "-v", "mode-keys"],
                &["set-option", "-p", "-t", "S:two.1", "synchronize-panes", "on"],
                &["set-option", "-w", "-t", "S:two", "synchronize-panes", "on"],
                &["show-options", "-w", "-t", "S:two", "synchronize-panes"],
                &["show-options", "-p", "-t", "S:two.1", "synchronize-panes"],
                &["show-options", "-p", "-t", "S:two.0", "-A", "synchronize-panes"],
                &["set-option", "-p", "-t", "S:two.1", "@pane-only", "yes"],
                &["show-options", "-p", "-t", "S:two.1", "@pane-only"],
                &["show-options", "-p", "-t", "S:two.0", "@pane-only"],
                &["show-options", "-w", "-t", "S:two", "@pane-only"],
                &["set-option", "-p", "-t", "S:two.1", "-u", "@pane-only"],
                &["show-options", "-p", "-t", "S:two.1", "@pane-only"],
                &["set-option", "-t", "S", "base-index", "3"],
                &["show-options", "-t", "S", "-v", "base-index"],
                &["show-options", "-t", "T", "-v", "base-index"],
                &["list-windows", "-t", "S", "-F", "#{window_index}"],
                &["set-option", "-u", "-t", "S", "base-index"],
                &["set-option", "-t", "S", "renumber-windows", "off"],
                &["show-options", "-t", "S", "renumber-windows"],
                &["set-option", "-s", "-t", "S", "buffer-limit", "7"],
                &["show-options", "-s", "-v", "buffer-limit"],
                &["show-options", "-t", "S", "buffer-limit"],
            ],
        );
    }

    #[test]
    fn options_set_window_option_flags_match_the_reference() {
        let pair = fixture("optsetw");
        compare_all(
            "options_set_window_option_flags_match_the_reference",
            &pair,
            &[
                &["set-window-option"],
                &["set-window-option", "mode-keys", "vi"],
                &["show-window-options", "mode-keys"],
                &["show-window-options", "-v", "mode-keys"],
                &["show-window-options", "-g", "mode-keys"],
                &["set-window-option", "-g", "mode-keys", "vi"],
                &["show-window-options", "-gv", "mode-keys"],
                &["set-window-option", "-t", "S:9", "mode-keys", "vi"],
                &["set-window-option", "-t", "S:1", "mode-keys", "emacs"],
                &["set-window-option", "-t", "S:1.0", "mode-keys", "emacs"],
                &["show-window-options", "-t", "S:1", "-v", "mode-keys"],
                &["set-window-option", "mouse", "on"],
                &["set-window-option", "-q", "nosuch", "vi"],
                &["set-window-option", "nosuch", "vi"],
                &["set-window-option", "mode-keys"],
                &["show-window-options", "-gv", "mode-keys"],
                &["set-window-option", "-u", "mode-keys"],
                &["show-window-options", "mode-keys"],
                &["show-window-options", "nosuch"],
                &["show-window-options", "-t", "nosuch", "mode-keys"],
                &["show-window-options", "a", "b"],
                &["show-window-options", "-g", "@missing"],
                &["set-window-option", "-g", "@wtag", "w"],
                &["show-window-options", "-g", "@wtag"],
                &["show-window-options", "-gv", "@wtag"],
                &["set-window-option", "@local", "w"],
                &["show-window-options", "@local"],
            ],
        );
    }

    #[test]
    fn options_rejects_flags_it_does_not_implement() {
        let pair = fixture("optrej");
        assert_rejected_in_tm(
            &pair,
            "set-option",
            &[
                ("-a", &["set-option", "-a", "status-left", "x"][..]),
                ("-o", &["set-option", "-o", "status-left", "x"]),
                ("-F", &["set-option", "-F", "@x", "#{session_name}"]),
                ("-U", &["set-option", "-U", "mode-keys"]),
                ("-x", &["set-option", "-x", "mouse", "on"]),
            ],
        );
        assert_rejected_in_tm(
            &pair,
            "set-window-option",
            &[
                ("-a", &["set-window-option", "-a", "mode-keys", "vi"][..]),
                ("-o", &["set-window-option", "-o", "mode-keys", "vi"]),
                ("-F", &["set-window-option", "-F", "mode-keys", "vi"]),
                ("-w", &["set-window-option", "-w", "mode-keys", "vi"]),
                ("-p", &["set-window-option", "-p", "mode-keys", "vi"]),
                ("-s", &["set-window-option", "-s", "mode-keys", "vi"]),
            ],
        );
        assert_rejected_in_tm(
            &pair,
            "show-options",
            &[
                ("-F", &["show-options", "-F", "x", "mouse"][..]),
                ("-H", &["show-options", "-H", "-g"]),
                ("-x", &["show-options", "-x"]),
            ],
        );
        assert_rejected_in_tm(
            &pair,
            "show-window-options",
            &[
                ("-F", &["show-window-options", "-F", "x", "mode-keys"][..]),
                ("-w", &["show-window-options", "-w", "mode-keys"]),
                ("-A", &["show-window-options", "-A", "mode-keys"]),
                ("-q", &["show-window-options", "-q", "mode-keys"]),
                ("-p", &["show-window-options", "-p", "mode-keys"]),
                ("-s", &["show-window-options", "-s", "mode-keys"]),
            ],
        );
    }

    #[test]
    fn options_names_tm_does_not_implement_are_errors_in_tm() {
        let pair = fixture("optunsupported");
        for (arguments, message) in [
            (
                &["show-options", "-gv", "activity-action"][..],
                "tm: unsupported option: activity-action\n",
            ),
            (
                &["show-options", "-gqv", "activity-action"],
                "tm: unsupported option: activity-action\n",
            ),
            (
                &["show-options", "-gv", "display-panes-color"],
                "tm: unsupported option: display-panes-colour\n",
            ),
            (
                &["show-options", "-sv", "backspace"],
                "tm: unsupported option: backspace\n",
            ),
            (
                &["show-options", "-gqv", "terminal-features[0]"],
                "tm: unsupported array element: terminal-features[0]\n",
            ),
            (
                &["set-option", "-g", "terminal-features[0]", "x"],
                "tm: unsupported array element: terminal-features[0]\n",
            ),
            (
                &["set-option", "-gu", "mouse"],
                "tm: unsupported reset of global option mouse\n",
            ),
            (
                &["set-window-option", "-gu", "mode-keys"],
                "tm: unsupported reset of global option mode-keys\n",
            ),
        ] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(String::from_utf8_lossy(&output.stderr), message, "{arguments:?}");
            assert!(output.stdout.is_empty(), "{arguments:?}");
            assert!(pair.reference.run(arguments).status.success(), "{arguments:?}");
        }
    }

    #[test]
    fn options_listings_hold_only_values_the_reference_also_lists() {
        let pair = fixture("optlist");
        for arguments in [
            &["show-options", "-g"][..],
            &["show-options", "-s"],
            &["show-options", "-gw"],
            &["show-window-options", "-g"],
            &["show-options", "-gA"],
        ] {
            let reference = pair.reference.body(arguments);
            let reference = reference.lines().collect::<Vec<_>>();
            let listing = pair.tm.body(arguments);
            assert!(listing.lines().count() > 3, "{arguments:?}");
            for line in listing.lines() {
                assert!(reference.contains(&line), "{arguments:?}: {line}");
            }
        }
        for arguments in [
            &["show-options"][..],
            &["show-options", "-w"],
            &["show-window-options"],
            &["show-options", "-p"],
            &["show-options", "-t", "T"],
        ] {
            assert_eq!(
                pair.tm.body(arguments),
                pair.reference.body(arguments),
                "{arguments:?}"
            );
        }
    }
}

mod environment {
    use super::*;

    fn fixture(label: &str) -> Pair {
        let pair = Pair::new(label);
        pair.start("S");
        pair.start("T");
        pair
    }

    fn spawned_environment(mux: &Mux, session: &str, variables: &[&str]) -> String {
        let marker = mux.root().join(format!("env-{session}"));
        let marker = marker.to_string_lossy().into_owned();
        let printer = variables
            .iter()
            .map(|name| format!("printf '%s=%s;' {name} \"${{{name}-unset}}\""))
            .collect::<Vec<_>>()
            .join("; ");
        mux.ok(&[
            "new-window", "-d", "-t", session, "--", "sh", "-c",
            &format!("({printer}) > '{marker}.tmp'; mv '{marker}.tmp' '{marker}'; sleep 300"),
        ]);
        let deadline = Instant::now() + TIMEOUT;
        while !std::path::Path::new(&marker).exists() {
            assert!(Instant::now() < deadline, "spawned pane never wrote {marker}");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::read_to_string(&marker).expect("read environment marker")
    }

    #[test]
    fn environment_set_and_show_environment_flags_match_the_reference() {
        let pair = fixture("envflags");
        for arguments in [
            &["set-environment", "FOO", "bar"][..],
            &["show-environment", "FOO"],
            &["show-environment", "-g", "FOO"],
            &["set-environment", "-g", "GFOO", "gbar"],
            &["show-environment", "-g", "GFOO"],
            &["show-environment", "GFOO"],
            &["show-environment", "-t", "S", "FOO"],
            &["show-environment", "-t", "T", "FOO"],
            &["show-environment", "-s", "FOO"],
            &["show-environment", "-gs", "GFOO"],
            &["show-environment", "HFOO"],
            &["set-environment", "-r", "RFOO"],
            &["show-environment", "-s", "RFOO"],
            &["set-environment", "-u", "UFOO"],
            &["show-environment", "UFOO"],
            &["show-environment", "-s", "UFOO"],
            &["show-environment", "-gs", "UFOO"],
            &["set-environment", "-gu", "GFOO"],
            &["show-environment", "-g", "GFOO"],
            &["set-environment", "-gr", "GRFOO"],
            &["show-environment", "-g", "GRFOO"],
            &["show-environment", "-gs", "GRFOO"],
            &["show-environment", "FFOO"],
            &["set-environment", "-t", "T", "TFOO", "tbar"],
            &["show-environment", "-t", "T", "TFOO"],
            &["show-environment", "TFOO"],
            &["set-environment", "-t", "nosuch", "XFOO", "x"],
            &["set-environment"],
            &["set-environment", "FOO"],
            &["set-environment", "FOO", "bar", "baz"],
            &["set-environment", "A=B", "x"],
            &["set-environment", "", "x"],
            &["set-environment", "-u", "A=B"],
            &["set-environment", "-u", ""],
            &["set-environment", "FOO", ""],
            &["show-environment", "FOO"],
            &["show-environment", "-s", "FOO"],
            &["show-environment", "nosuch"],
            &["show-environment", "-g", "nosuch"],
            &["show-environment", "a", "b"],
            &["show-environment", "-t", "nosuch"],
            &["set-environment", "-g", "EQ", "a b\"c$d"],
            &["show-environment", "-g", "EQ"],
            &["show-environment", "-gs", "EQ"],
            &["set-environment", "-gr", "EQ"],
            &["show-environment", "-gs", "EQ"],
            &["show-environment", "-g", "EQ"],
            &["show-environment", "--", "FOO"],
            &["show-environment", "-t"],
            &["set-environment", "--", "FOO", "bar"],
        ] {
            compare_commands(
                "environment_set_and_show_environment_flags_match_the_reference",
                &pair.tm,
                &pair.reference,
                &[arguments],
            );
        }
    }

    fn listing_without_update_environment(
        mux: &Mux,
        skipped: &[String],
        arguments: &[&str],
    ) -> Vec<String> {
        mux.body(arguments)
            .lines()
            .filter(|line| {
                let name = match line.strip_prefix("unset ") {
                    Some(rest) => rest.trim_end_matches(';'),
                    None => line.trim_start_matches('-').split('=').next().unwrap_or_default(),
                };
                !skipped.iter().any(|skipped| skipped == name)
            })
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn environment_session_listing_matches_the_reference_except_update_environment() {
        let pair = fixture("envlist");
        let skipped = pair
            .reference
            .body(&["show-options", "-gv", "update-environment"])
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(skipped.len() > 3, "{skipped:?}");
        for arguments in [
            &["set-environment", "-t", "S", "ZED", "z"][..],
            &["set-environment", "-t", "S", "ALPHA", "a b"],
            &["set-environment", "-t", "S", "MID", "q\"$`\\"],
            &["set-environment", "-t", "S", "-r", "GONE"],
            &["set-environment", "-t", "S", "-r", "ZED"],
            &["set-environment", "-t", "S", "-u", "ZED", "v"],
            &["set-environment", "-t", "S", "-r", "ZED", "v"],
        ] {
            compare_commands(
                "environment_session_listing_matches_the_reference_except_update_environment",
                &pair.tm,
                &pair.reference,
                &[arguments],
            );
        }
        for arguments in [
            &["show-environment", "-t", "S"][..],
            &["show-environment", "-s", "-t", "S"],
            &["show-environment", "-t", "T"],
        ] {
            assert_eq!(
                listing_without_update_environment(&pair.tm, &skipped, arguments),
                listing_without_update_environment(&pair.reference, &skipped, arguments),
                "{arguments:?}"
            );
        }
        for mux in pair.both() {
            mux.ok(&["set-environment", "-t", "S", "-u", "ZED"]);
        }
        assert_eq!(
            listing_without_update_environment(&pair.tm, &skipped, &["show-environment", "-t", "S"]),
            listing_without_update_environment(
                &pair.reference,
                &skipped,
                &["show-environment", "-t", "S"]
            ),
        );
        pair.tm.ok(&["set-environment", "-g", "LISTED", "x"]);
        assert!(pair.tm.body(&["show-environment", "-g"]).lines().any(|line| line == "LISTED=x"));
    }

    #[test]
    fn environment_spawned_panes_see_global_and_session_variables_like_the_reference() {
        let pair = fixture("envspawn");
        for mux in pair.both() {
            mux.ok(&["set-environment", "-g", "GLOBAL_ONLY", "g"]);
            mux.ok(&["set-environment", "-g", "SHADOWED", "global"]);
            mux.ok(&["set-environment", "-t", "S", "SHADOWED", "session"]);
            mux.ok(&["set-environment", "-g", "CLEARED", "global"]);
            mux.ok(&["set-environment", "-t", "S", "-r", "CLEARED"]);
            mux.ok(&["set-environment", "-g", "REMOVED", "global"]);
            mux.ok(&["set-environment", "-g", "-u", "REMOVED"]);
        }
        let variables = ["GLOBAL_ONLY", "SHADOWED", "CLEARED", "REMOVED"];
        for session in ["S", "T"] {
            assert_eq!(
                spawned_environment(&pair.tm, session, &variables),
                spawned_environment(&pair.reference, session, &variables),
                "{session}"
            );
        }
    }

    #[test]
    fn environment_rejects_flags_it_does_not_implement() {
        let pair = fixture("envrej");
        for (command, flag, arguments) in [
            ("set-environment", "-h", &["set-environment", "-h", "A", "b"][..]),
            ("set-environment", "-F", &["set-environment", "-F", "A", "#{session_name}"]),
            ("set-environment", "-s", &["set-environment", "-s", "A", "b"]),
            ("show-environment", "-h", &["show-environment", "-h"]),
            ("show-environment", "-F", &["show-environment", "-F", "x", "A"]),
            ("show-environment", "-x", &["show-environment", "-x"]),
        ] {
            let output = pair.tm.run(arguments);
            assert!(!output.status.success(), "{arguments:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag {flag} for {command}\n"),
                "{arguments:?}"
            );
            assert!(output.stdout.is_empty(), "{arguments:?}");
        }
    }
}

mod modes {
    use super::*;

    const OBSERVE_MODE: &str = "#{?pane_in_mode,#{pane_mode}:#{scroll_position},none}";

    fn fixture(label: &str) -> Pair {
        let pair = Pair::new(label);
        for mux in pair.both() {
            mux.ok(&[
                "new-session", "-d", "-s", "S", "-x", "40", "-y", "8", "--", "sh", "-c",
                "seq 1 30; echo AAAA; sleep 300",
            ]);
            mux.ok(&["split-window", "-d", "-t", "S", "--", "sh", "-c", "echo BBBB; sleep 300"]);
            assert!(mux.capture_until("S.0", "AAAA", TIMEOUT).contains("AAAA"));
            assert!(mux.capture_until("S.1", "BBBB", TIMEOUT).contains("BBBB"));
        }
        pair
    }

    fn observe(test: &str, pair: &Pair, arguments: &[&str]) {
        compare_commands(
            test,
            &pair.tm,
            &pair.reference,
            &[
                &["copy-mode", "-q", "-t", "S.0"],
                &["copy-mode", "-q", "-t", "S.1"],
                arguments,
                &["display-message", "-p", "-t", "S.0", OBSERVE_MODE],
                &["display-message", "-p", "-t", "S.1", OBSERVE_MODE],
            ],
        );
    }

    #[test]
    fn modes_copy_mode_flags_match_the_reference() {
        let test = "modes_copy_mode_flags_match_the_reference";
        let pair = fixture("cmflags");
        for arguments in [
            &["copy-mode", "-t", "S.0"][..],
            &["copy-mode", "-t", "S.1"],
            &["copy-mode", "-u", "-t", "S.0"],
            &["copy-mode", "-d", "-t", "S.0"],
            &["copy-mode", "-u", "-d", "-t", "S.0"],
            &["copy-mode", "-e", "-t", "S.0"],
            &["copy-mode", "-e", "-d", "-t", "S.0"],
            &["copy-mode", "-e", "-u", "-t", "S.0"],
            &["copy-mode", "-H", "-t", "S.0"],
            &["copy-mode", "-s", "S.0", "-t", "S.1"],
            &["copy-mode", "-s", "S.0", "-t", "S.0"],
            &["copy-mode", "-s", "nosuch", "-t", "S.1"],
            &["copy-mode", "-t", "nosuch"],
            &["copy-mode", "-q", "-t", "S.0"],
            &["copy-mode", "extra"],
        ] {
            observe(test, &pair, arguments);
        }
        for mux in pair.both() {
            mux.ok(&["copy-mode", "-t", "S.1"]);
            mux.ok(&["copy-mode", "-q", "-t", "S.1"]);
        }
        compare_commands(
            test,
            &pair.tm,
            &pair.reference,
            &[
                &["display-message", "-p", "-t", "S.1", OBSERVE_MODE],
                &["send-keys", "-t", "S.1", "-X", "cancel"],
            ],
        );
    }

    #[test]
    fn modes_copy_mode_scroll_exit_and_paging_match_the_reference() {
        let test = "modes_copy_mode_scroll_exit_and_paging_match_the_reference";
        let pair = fixture("cmscroll");
        let probe = ["display-message", "-p", "-t", "S.0", OBSERVE_MODE];
        for (entry, keys) in [
            (&["copy-mode", "-e", "-u", "-t", "S.0"][..], &["scroll-down", "scroll-down", "page-down", "scroll-down"][..]),
            (&["copy-mode", "-e", "-t", "S.0"], &["scroll-up", "scroll-down", "scroll-down"]),
            (&["copy-mode", "-u", "-d", "-t", "S.0"], &["page-up", "page-up", "page-down"]),
            (&["copy-mode", "-u", "-t", "S.0"], &["page-up", "page-down", "page-down"]),
        ] {
            for mux in pair.both() {
                let _ = mux.run(&["copy-mode", "-q", "-t", "S.0"]);
            }
            compare_commands(test, &pair.tm, &pair.reference, &[entry, &probe]);
            for key in keys {
                compare_commands(
                    test,
                    &pair.tm,
                    &pair.reference,
                    &[&["send-keys", "-t", "S.0", "-X", key], &probe],
                );
            }
        }
    }

    fn position_indicator_visible(mux: &Mux, label: &str, hide_position: bool) -> bool {
        mux.ok(&["new-session", "-d", "-s", "S", "-x", "40", "-y", "8", "--", "sh", "-c", "seq 1 30; sleep 300"]);
        let client = attach(mux, label, "S");
        let command: &[&str] = if hide_position {
            &["copy-mode", "-H", "-t", "S"]
        } else {
            &["copy-mode", "-t", "S"]
        };
        mux.ok(command);
        mux.ok(&["set-option", "-g", "status-left", "MARK"]);
        client.wait_for_text("MARK", TIMEOUT);
        client.screen_text().lines().next().unwrap_or_default().contains("[0/")
    }

    #[test]
    fn modes_copy_mode_hide_position_matches_the_reference() {
        let test = "modes_copy_mode_hide_position_matches_the_reference";
        for hide_position in [false, true] {
            let pair = Pair::new(if hide_position { "cmhide" } else { "cmshow" });
            let tm = position_indicator_visible(&pair.tm, "tm", hide_position);
            let reference = position_indicator_visible(&pair.reference, "reference", hide_position);
            assert_eq!(observation_difference(test, &tm, &reference), None);
            assert_eq!(tm, !hide_position);
        }
    }

    #[test]
    fn modes_copy_mode_rejects_flags_it_does_not_implement() {
        let pair = fixture("cmrej");
        for command in ["copy-mode", "copy-mode-and-page"] {
            for flag in ["-k", "-M", "-S"] {
                let output = pair.tm.run(&[command, flag, "-t", "S.0"]);
                assert!(!output.status.success(), "{command} {flag}");
                assert_eq!(
                    String::from_utf8_lossy(&output.stderr),
                    format!("tm: unsupported flag {flag} for {command}\n")
                );
                assert_eq!(pair.tm.body(&["display-message", "-p", "-t", "S.0", OBSERVE_MODE]), "none");
            }
        }
        for mux in pair.both() {
            let output = mux.run(&["copy-mode", "-x"]);
            assert!(!output.status.success());
        }
    }

    #[test]
    fn modes_choose_commands_reject_flags_it_does_not_implement() {
        let pair = fixture("chrej");
        for (command, accepted) in [
            ("choose-tree", "F:f:GhK:kNO:rswyZ"),
            ("choose-buffer", "F:f:K:kNO:ryZ"),
            ("choose-client", "F:f:hiK:kNO:ryZ"),
        ] {
            let mut flags = accepted.chars().peekable();
            while let Some(flag) = flags.next() {
                let takes_value = flags.next_if_eq(&':').is_some();
                let name = format!("-{flag}");
                let mut arguments = vec![command, &name];
                if takes_value {
                    arguments.push("x");
                }
                let output = pair.tm.run(&arguments);
                assert!(!output.status.success(), "{arguments:?}");
                assert_eq!(
                    String::from_utf8_lossy(&output.stderr),
                    format!("tm: unsupported flag {name} for {command}\n"),
                    "{arguments:?}"
                );
            }
            let output = pair.tm.run(&[command, "template"]);
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag template for {command}\n")
            );
        }
        for command in ["choose-buffer", "choose-client"] {
            let output = pair.tm.run(&[command, "-G"]);
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("tm: unsupported flag -G for {command}\n")
            );
            assert!(!pair.reference.run(&[command, "-G"]).status.success());
        }
        assert_eq!(pair.tm.body(&["display-message", "-p", "-t", "S.0", OBSERVE_MODE]), "none");
    }

    #[test]
    fn modes_choose_commands_open_a_client_mode_in_tm() {
        let pair = fixture("chtm");
        for (command, headless) in [
            ("choose-tree", "tm: choose-tree requires an attached client\n"),
            ("choose-buffer", "tm: chooser requires an attached client\n"),
            ("choose-client", "tm: chooser requires an attached client\n"),
        ] {
            let output = pair.tm.run(&[command]);
            assert!(!output.status.success(), "{command}");
            assert_eq!(String::from_utf8_lossy(&output.stderr), headless);
            assert!(pair.reference.run(&[command]).status.success(), "{command}");
            let _ = pair.reference.run(&["copy-mode", "-q", "-t", "S.0"]);
        }
        let client = attach(&pair.tm, "chtm", "S");
        pair.tm.ok(&["set-buffer", "-b", "keep", "data"]);
        pair.tm.ok(&["choose-tree", "-t", "S"]);
        client.wait_for_text("  1: sh", TIMEOUT);
        pair.tm.ok(&["choose-tree", "-t", "S"]);
        pair.tm.ok(&["choose-buffer", "-t", "S"]);
        pair.tm.ok(&["choose-client", "-t", "S"]);
    }

    #[test]
    fn modes_tm_only_commands_and_aliases_are_unknown_to_the_reference() {
        let pair = fixture("tmonly");
        for (alias, canonical, arguments) in [
            ("choose", "choose-tree", vec!["-t", "S"]),
            ("chooseb", "choose-buffer", vec!["-t", "S"]),
            ("choosec", "choose-client", vec!["-t", "S"]),
            ("copy-mode-and-page", "copy-mode", vec!["-u", "-t", "S.0"]),
        ] {
            let through_alias = pair.tm.outcome(&[&[alias][..], &arguments].concat());
            let through_name = pair.tm.outcome(&[&[canonical][..], &arguments].concat());
            assert_eq!(through_alias, through_name, "{alias}");
            assert!(
                !pair.reference.run(&[&[alias][..], &arguments].concat()).status.success(),
                "tmux accepted {alias}"
            );
            let _ = pair.tm.run(&["copy-mode", "-q", "-t", "S.0"]);
        }
        assert_eq!(pair.tm.body(&["display-message", "-p", "-t", "S.0", OBSERVE_MODE]), "none");
        pair.tm.ok(&["copy-mode-and-page", "-t", "S.0"]);
        pair.reference.ok(&["copy-mode", "-u", "-t", "S.0"]);
        assert_eq!(
            pair.tm.body(&["display-message", "-p", "-t", "S.0", OBSERVE_MODE]),
            pair.reference.body(&["display-message", "-p", "-t", "S.0", OBSERVE_MODE])
        );
        for help in ["help", "-h", "--help"] {
            let output = pair.tm.run(&[help]);
            assert!(!output.status.success(), "{help}");
            assert!(output.stdout.is_empty(), "{help}");
            assert!(
                String::from_utf8_lossy(&output.stderr).starts_with("tm: usage: tm [-S socket]"),
                "{help}"
            );
        }
        assert!(!pair.reference.run(&["help"]).status.success());
    }
}
