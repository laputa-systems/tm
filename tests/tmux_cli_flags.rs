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
        for steps in cases {
            probe(test, &format!("{steps:?}"), steps);
        }
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
                &[&["select-window", "-l", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l", "-t", "S:2"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-l"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-n", "-l", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-p", "-n", "-t", "S"]],
                &[&["select-window", "-t", "S:3"], &["select-window", "-pl", "-t", "S"]],
                &[&["select-window", "S:3"]],
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
}
