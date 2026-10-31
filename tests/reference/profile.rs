pub struct OptionSpec<'a> {
    pub key: &'a str,
    pub value: &'a str,
    pub scope: &'a str,
}

pub struct BindingSpec<'a> {
    pub key: &'a str,
    pub repeat: bool,
    pub commands: &'a [&'a [&'a str]],
}

pub fn option_scope(key: &str) -> Option<&'static str> {
    match key {
        "default-terminal"
        | "terminal-features"
        | "set-clipboard"
        | "extended-keys"
        | "extended-keys-format"
        | "escape-time"
        | "focus-events" => Some("-s"),
        "base-index"
        | "bell-action"
        | "history-limit"
        | "mouse"
        | "prefix"
        | "renumber-windows"
        | "set-titles"
        | "status-bg"
        | "status-fg"
        | "status-left"
        | "status-left-length"
        | "status-position"
        | "status-right"
        | "visual-bell" => Some("-g"),
        "automatic-rename"
        | "mode-keys"
        | "monitor-activity"
        | "monitor-bell"
        | "window-status-current-format"
        | "window-status-format" => Some("-gw"),
        _ => None,
    }
}

pub fn reference_config() -> &'static str {
    include_str!("tmux-3.8.conf")
}

pub fn baseline_config() -> &'static str {
    include_str!("tmux-3.8-baseline.conf")
}

pub fn render_reference_config(options: &[OptionSpec<'_>], bindings: &[BindingSpec<'_>]) -> String {
    let mut lines = vec!["set -s buffer-limit 50".to_owned()];
    for option in options {
        let scope = option_scope(option.key)
            .unwrap_or_else(|| panic!("option {} has no reference scope", option.key));
        lines.push(format!(
            "set {scope} {} {}",
            option.key,
            quote(option.value)
        ));
    }
    lines.push("unbind -a -T prefix".to_owned());
    for binding in bindings {
        let commands = binding
            .commands
            .iter()
            .map(|command| {
                command
                    .iter()
                    .map(|token| {
                        quote(match *token {
                            "__detach" => "detach-client",
                            "send" => "send-keys",
                            token => token,
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join(" \\; ");
        lines.push(format!(
            "bind {}-T prefix {} {}",
            if binding.repeat { "-r " } else { "" },
            quote(binding.key),
            commands
        ));
    }
    lines.join("\n") + "\n"
}

pub fn render_baseline_config() -> String {
    [
        "set -s buffer-limit 50",
        "set -g history-limit 10000",
        "set -gw mode-keys emacs",
        "set -g prefix C-b",
        "set -g mouse off",
        "set -g base-index 0",
        "set -g renumber-windows off",
        "set -gw remain-on-exit off",
    ]
    .join("\n")
        + "\n"
}

pub fn assert_config_matches(path: &str, expected: &str) {
    let actual = std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("read reference config {path}: {error}");
    });
    let actual = actual
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    for line in expected
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        assert!(
            actual.contains(&line),
            "reference config {path} is missing generated line: {line}"
        );
    }
    for line in &actual {
        assert!(
            expected.lines().any(|candidate| candidate.trim() == *line),
            "reference config {path} has a line the compiled profile does not generate: {line}"
        );
    }
}

fn quote(value: &str) -> String {
    if value.contains('\'') {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("'{value}'")
    }
}
