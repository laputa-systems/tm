use std::time::Duration;

use ptytest::{
    CommandSpec, ExitStatus, ProtocolProfile, PtyTest, Scenario, ScreenSnapshot, Size, TestEnv,
};

use super::parity::Mux;

pub fn spawn_attached(
    mux: &Mux,
    name: &str,
    arguments: &[&str],
    size: Size,
) -> PtyTest {
    spawn_attached_with_profile(
        mux,
        name,
        arguments,
        size,
        ProtocolProfile::disabled(),
    )
}

pub fn spawn_attached_with_profile(
    mux: &Mux,
    name: &str,
    arguments: &[&str],
    size: Size,
    profile: ProtocolProfile,
) -> PtyTest {
    let environment = TestEnv::hermetic_utf8("en_US.UTF-8")
        .or_else(|_| TestEnv::hermetic())
        .expect("create hermetic attached environment")
        .env("TERM", mux.term())
        .env("TMUX_TMPDIR", mux.root())
        .env("TMPDIR", mux.root())
        .remove_env("TM_SOCKET")
        .remove_env("TMUX")
        .remove_env("TMUX_PANE")
        .remove_env("TM_CONFIG");
    let mut command = CommandSpec::new(mux.binary())
        .arg("-S")
        .arg(mux.socket());
    if let Some(config) = mux.config() {
        command = command.arg("-f").arg(config);
    }
    let command = command.args(arguments);
    let scenario = Scenario::new(format!(
        "parity {} {name}",
        match mux.implementation() {
            super::parity::Implementation::Tm => "tm",
            super::parity::Implementation::Reference => "tmux",
        }
    ))
    .expect("valid scenario label")
    .command(command)
    .size(size)
    .environment(environment)
    .protocol_profile(profile);
    PtyTest::spawn(scenario).expect("spawn attached parity client")
}

pub fn wait_for_text(terminal: &mut PtyTest, text: &str, timeout: Duration) -> ScreenSnapshot {
    terminal
        .wait_for_screen(
            terminal.deadline(timeout),
            format!("screen contains {text:?}"),
            |screen| screen.contains(text),
        )
        .unwrap_or_else(|error| panic!("wait for {text:?}: {error}"))
}

pub fn detach(terminal: &mut PtyTest, baseline: &ptytest::TerminalBaseline) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(5)), b"\x01d")
        .expect("send detach");
    assert_eq!(
        terminal
            .wait_for_exit(terminal.deadline(Duration::from_secs(5)))
            .expect("wait for detach"),
        ExitStatus::Code(0)
    );
    terminal
        .assert_terminal_restored(baseline)
        .expect("restore terminal on detach");
    terminal
        .finish(terminal.deadline(Duration::from_secs(5)))
        .expect("reap detached client");
}
