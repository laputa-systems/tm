use crate::protocol::OptionFlags;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionLevel {
    Server,
    Session,
    Window,
    WindowOrPane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionKind {
    Flag,
    Number { minimum: i64, maximum: i64 },
    Choice(&'static [&'static str]),
    Other,
}

pub(crate) struct OptionSpec {
    pub name: &'static str,
    pub level: OptionLevel,
    pub kind: OptionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionTable {
    Server,
    GlobalSession,
    GlobalWindow,
    Session,
    Window,
    Pane,
}

#[derive(Debug)]
pub(crate) enum OptionNameError {
    Invalid,
    Ambiguous,
    ArrayKey,
}

pub(crate) enum OptionName {
    User(String),
    Builtin(&'static OptionSpec),
}

impl OptionName {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::User(name) => name,
            Self::Builtin(spec) => spec.name,
        }
    }
}

const RENAMED_OPTIONS: &[(&str, &str)] = &[
    ("display-panes-color", "display-panes-colour"),
    ("display-panes-active-color", "display-panes-active-colour"),
    ("clock-mode-color", "clock-mode-colour"),
    ("cursor-color", "cursor-colour"),
    ("prompt-cursor-color", "prompt-cursor-colour"),
    ("prompt-command-cursor-color", "prompt-command-cursor-colour"),
    ("pane-colors", "pane-colours"),
];

pub(crate) fn match_option(argument: &str) -> Result<OptionName, OptionNameError> {
    if argument.starts_with('@') {
        return Ok(OptionName::User(argument.to_owned()));
    }
    if argument.contains('[') {
        return Err(OptionNameError::ArrayKey);
    }
    let name = RENAMED_OPTIONS
        .iter()
        .find(|(from, _)| *from == argument)
        .map_or(argument, |(_, to)| to);
    let mut found = None;
    for spec in OPTION_TABLE {
        if spec.name == name {
            return Ok(OptionName::Builtin(spec));
        }
        if spec.name.starts_with(name) {
            if found.is_some() {
                return Err(OptionNameError::Ambiguous);
            }
            found = Some(spec);
        }
    }
    found.map(OptionName::Builtin).ok_or(OptionNameError::Invalid)
}

pub(crate) fn table_for_name(name: &OptionName, flags: OptionFlags) -> OptionTable {
    let level = match name {
        OptionName::User(_) => return table_for_flags(flags),
        OptionName::Builtin(spec) => spec.level,
    };
    match level {
        OptionLevel::Server => OptionTable::Server,
        OptionLevel::Session if flags.global => OptionTable::GlobalSession,
        OptionLevel::Session => OptionTable::Session,
        OptionLevel::WindowOrPane if flags.pane => OptionTable::Pane,
        OptionLevel::Window | OptionLevel::WindowOrPane if flags.global => {
            OptionTable::GlobalWindow
        }
        OptionLevel::Window | OptionLevel::WindowOrPane => OptionTable::Window,
    }
}

pub(crate) fn table_for_flags(flags: OptionFlags) -> OptionTable {
    if flags.server {
        OptionTable::Server
    } else if flags.pane {
        OptionTable::Pane
    } else if flags.window {
        if flags.global {
            OptionTable::GlobalWindow
        } else {
            OptionTable::Window
        }
    } else if flags.global {
        OptionTable::GlobalSession
    } else {
        OptionTable::Session
    }
}

pub(crate) fn level_of_global_table(table: OptionTable) -> Option<&'static [OptionLevel]> {
    match table {
        OptionTable::Server => Some(&[OptionLevel::Server]),
        OptionTable::GlobalSession => Some(&[OptionLevel::Session]),
        OptionTable::GlobalWindow => Some(&[OptionLevel::Window, OptionLevel::WindowOrPane]),
        OptionTable::Session | OptionTable::Window | OptionTable::Pane => None,
    }
}

pub(crate) fn option_spec(name: &str) -> Option<&'static OptionSpec> {
    OPTION_TABLE.iter().find(|spec| spec.name == name)
}

pub(crate) fn option_value_from_text(
    name: &OptionName,
    current: Option<&str>,
    value: Option<&str>,
) -> Result<String, String> {
    let OptionName::Builtin(spec) = name else {
        return value.map(str::to_owned).ok_or_else(|| "empty value".to_owned());
    };
    match (spec.kind, value) {
        (OptionKind::Flag, value) => match value.filter(|value| !value.is_empty()) {
            None => match current {
                Some("on") => Ok("off".to_owned()),
                Some("off") => Ok("on".to_owned()),
                _ => Err(format!("unsupported toggle of {}", spec.name)),
            },
            Some("1") => Ok("on".to_owned()),
            Some("0") => Ok("off".to_owned()),
            Some(value) => match value.to_ascii_lowercase().as_str() {
                "on" | "yes" => Ok("on".to_owned()),
                "off" | "no" => Ok("off".to_owned()),
                _ => Err(format!("bad value: {value}")),
            },
        },
        (OptionKind::Choice(choices), None) => {
            let index = choices
                .iter()
                .position(|choice| Some(*choice) == current)
                .ok_or_else(|| format!("unsupported toggle of {}", spec.name))?;
            let index = if index < 2 { 1 - index } else { index };
            Ok(choices[index].to_owned())
        }
        (OptionKind::Choice(choices), Some(value)) => choices
            .iter()
            .find(|choice| **choice == value)
            .map(|choice| (*choice).to_owned())
            .ok_or_else(|| format!("unknown value: {value}")),
        (OptionKind::Number { .. } | OptionKind::Other, None) => Err("empty value".to_owned()),
        (OptionKind::Number { minimum, maximum }, Some(value)) => {
            number_in_range(value, minimum, maximum).map(|number| number.to_string())
        }
        (OptionKind::Other, Some(value)) => Ok(value.to_owned()),
    }
}

fn number_in_range(value: &str, minimum: i64, maximum: i64) -> Result<i64, String> {
    let trimmed = value.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("value is invalid: {value}"));
    }
    let magnitude = digits.parse::<i128>().unwrap_or(i128::MAX);
    let number = if negative { -magnitude } else { magnitude };
    if number < i128::from(minimum) {
        Err(format!("value is too small: {value}"))
    } else if number > i128::from(maximum) {
        Err(format!("value is too large: {value}"))
    } else {
        i64::try_from(number).map_err(|_| format!("value is invalid: {value}"))
    }
}

pub(crate) const OPTION_TABLE: &[OptionSpec] = &[
    OptionSpec { name: "backspace", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "buffer-limit", level: OptionLevel::Server, kind: OptionKind::Number { minimum: 1, maximum: 2147483647 } },
    OptionSpec { name: "clear-on-attach", level: OptionLevel::Server, kind: OptionKind::Flag },
    OptionSpec { name: "command-alias", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "codepoint-widths", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "copy-command", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "cursor-colour", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "cursor-style", level: OptionLevel::WindowOrPane, kind: OptionKind::Choice(&["default", "blinking-block", "block", "blinking-underline", "underline", "blinking-bar", "bar"]) },
    OptionSpec { name: "default-client-command", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "default-terminal", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "editor", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "escape-time", level: OptionLevel::Server, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "exit-empty", level: OptionLevel::Server, kind: OptionKind::Flag },
    OptionSpec { name: "exit-unattached", level: OptionLevel::Server, kind: OptionKind::Flag },
    OptionSpec { name: "extended-keys", level: OptionLevel::Server, kind: OptionKind::Choice(&["off", "on", "always"]) },
    OptionSpec { name: "extended-keys-format", level: OptionLevel::Server, kind: OptionKind::Choice(&["csi-u", "xterm"]) },
    OptionSpec { name: "focus-events", level: OptionLevel::Server, kind: OptionKind::Flag },
    OptionSpec { name: "get-clipboard", level: OptionLevel::Server, kind: OptionKind::Choice(&["off", "buffer", "request", "both"]) },
    OptionSpec { name: "history-file", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "input-buffer-size", level: OptionLevel::Server, kind: OptionKind::Number { minimum: 1048576, maximum: 4294967295 } },
    OptionSpec { name: "menu-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "menu-selected-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "menu-border-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "menu-border-lines", level: OptionLevel::Window, kind: OptionKind::Choice(&["single", "double", "heavy", "simple", "rounded", "padded", "none"]) },
    OptionSpec { name: "message-limit", level: OptionLevel::Server, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "prefix-timeout", level: OptionLevel::Server, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "prompt-history-limit", level: OptionLevel::Server, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "set-clipboard", level: OptionLevel::Server, kind: OptionKind::Choice(&["off", "external", "on"]) },
    OptionSpec { name: "terminal-overrides", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "terminal-features", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "theme", level: OptionLevel::Server, kind: OptionKind::Choice(&["detect", "terminal", "light", "dark"]) },
    OptionSpec { name: "dark-theme-black", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-white", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-light-grey", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-dark-grey", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-green", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-yellow", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-red", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-blue", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-cyan", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "dark-theme-magenta", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-black", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-white", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-light-grey", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-dark-grey", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-green", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-yellow", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-red", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-blue", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-cyan", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "light-theme-magenta", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "user-keys", level: OptionLevel::Server, kind: OptionKind::Other },
    OptionSpec { name: "variation-selector-always-wide", level: OptionLevel::Server, kind: OptionKind::Flag },
    OptionSpec { name: "activity-action", level: OptionLevel::Session, kind: OptionKind::Choice(&["none", "any", "current", "other"]) },
    OptionSpec { name: "assume-paste-time", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "base-index", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "bell-action", level: OptionLevel::Session, kind: OptionKind::Choice(&["none", "any", "current", "other"]) },
    OptionSpec { name: "default-command", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "default-shell", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "default-size", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "destroy-unattached", level: OptionLevel::Session, kind: OptionKind::Choice(&["off", "on", "keep-last", "keep-group"]) },
    OptionSpec { name: "detach-on-destroy", level: OptionLevel::Session, kind: OptionKind::Choice(&["off", "on", "no-detached", "previous", "next"]) },
    OptionSpec { name: "display-time", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "focus-follows-mouse", level: OptionLevel::Session, kind: OptionKind::Flag },
    OptionSpec { name: "history-limit", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "initial-repeat-time", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2000000 } },
    OptionSpec { name: "key-table", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "lock-after-time", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "lock-command", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "message-command-style", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "message-format", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "message-line", level: OptionLevel::Session, kind: OptionKind::Choice(&["0", "1", "2", "3", "4"]) },
    OptionSpec { name: "message-style", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "mouse", level: OptionLevel::Session, kind: OptionKind::Flag },
    OptionSpec { name: "prefix", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "prefix2", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "renumber-windows", level: OptionLevel::Session, kind: OptionKind::Flag },
    OptionSpec { name: "repeat-time", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2000000 } },
    OptionSpec { name: "set-titles", level: OptionLevel::Session, kind: OptionKind::Flag },
    OptionSpec { name: "set-titles-string", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "silence-action", level: OptionLevel::Session, kind: OptionKind::Choice(&["none", "any", "current", "other"]) },
    OptionSpec { name: "status", level: OptionLevel::Session, kind: OptionKind::Choice(&["off", "on", "2", "3", "4", "5"]) },
    OptionSpec { name: "status-bg", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "status-fg", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "status-format", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "status-interval", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "status-justify", level: OptionLevel::Session, kind: OptionKind::Choice(&["left", "centre", "right", "absolute-centre"]) },
    OptionSpec { name: "status-keys", level: OptionLevel::Session, kind: OptionKind::Choice(&["emacs", "vi"]) },
    OptionSpec { name: "status-left", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "status-left-length", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 32767 } },
    OptionSpec { name: "status-left-style", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "status-position", level: OptionLevel::Session, kind: OptionKind::Choice(&["top", "bottom"]) },
    OptionSpec { name: "status-right", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "status-right-length", level: OptionLevel::Session, kind: OptionKind::Number { minimum: 0, maximum: 32767 } },
    OptionSpec { name: "status-right-style", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "status-style", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "pane-status-current-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "pane-status-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "prompt-cursor-colour", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "prompt-command-cursor-colour", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "prompt-cursor-style", level: OptionLevel::Session, kind: OptionKind::Choice(&["default", "blinking-block", "block", "blinking-underline", "underline", "blinking-bar", "bar"]) },
    OptionSpec { name: "prompt-command-cursor-style", level: OptionLevel::Session, kind: OptionKind::Choice(&["default", "blinking-block", "block", "blinking-underline", "underline", "blinking-bar", "bar"]) },
    OptionSpec { name: "session-status-current-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "session-status-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "update-environment", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "visual-activity", level: OptionLevel::Session, kind: OptionKind::Choice(&["off", "on", "both"]) },
    OptionSpec { name: "visual-bell", level: OptionLevel::Session, kind: OptionKind::Choice(&["off", "on", "both"]) },
    OptionSpec { name: "visual-silence", level: OptionLevel::Session, kind: OptionKind::Choice(&["off", "on", "both"]) },
    OptionSpec { name: "word-separators", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "aggressive-resize", level: OptionLevel::Window, kind: OptionKind::Flag },
    OptionSpec { name: "allow-passthrough", level: OptionLevel::WindowOrPane, kind: OptionKind::Choice(&["off", "on", "all"]) },
    OptionSpec { name: "allow-rename", level: OptionLevel::WindowOrPane, kind: OptionKind::Flag },
    OptionSpec { name: "allow-set-title", level: OptionLevel::WindowOrPane, kind: OptionKind::Flag },
    OptionSpec { name: "alternate-screen", level: OptionLevel::WindowOrPane, kind: OptionKind::Flag },
    OptionSpec { name: "automatic-rename", level: OptionLevel::Window, kind: OptionKind::Flag },
    OptionSpec { name: "automatic-rename-format", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "clock-mode-colour", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "clock-mode-style", level: OptionLevel::Window, kind: OptionKind::Choice(&["12", "24", "12-with-seconds", "24-with-seconds"]) },
    OptionSpec { name: "display-panes-active-colour", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "display-panes-border-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "display-panes-colour", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "display-panes-format", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "display-panes-time", level: OptionLevel::Window, kind: OptionKind::Number { minimum: 1, maximum: 2147483647 } },
    OptionSpec { name: "copy-mode-match-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-current-match-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-mark-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-position-format", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-position-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-selection-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-current-line-number-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-current-line-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-line-number-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "copy-mode-line-numbers", level: OptionLevel::Window, kind: OptionKind::Choice(&["off", "default", "absolute", "relative", "hybrid"]) },
    OptionSpec { name: "fill-character", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "main-pane-height", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "main-pane-width", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "mode-keys", level: OptionLevel::Window, kind: OptionKind::Choice(&["emacs", "vi"]) },
    OptionSpec { name: "mode-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "monitor-activity", level: OptionLevel::Window, kind: OptionKind::Flag },
    OptionSpec { name: "monitor-bell", level: OptionLevel::Window, kind: OptionKind::Flag },
    OptionSpec { name: "monitor-silence", level: OptionLevel::Window, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "other-pane-height", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "other-pane-width", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "pane-active-border-style", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-base-index", level: OptionLevel::Window, kind: OptionKind::Number { minimum: 0, maximum: 65535 } },
    OptionSpec { name: "pane-border-format", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-border-indicators", level: OptionLevel::Window, kind: OptionKind::Choice(&["off", "colour", "arrows", "both"]) },
    OptionSpec { name: "pane-border-lines", level: OptionLevel::WindowOrPane, kind: OptionKind::Choice(&["single", "double", "heavy", "simple", "number", "spaces", "none"]) },
    OptionSpec { name: "pane-border-status", level: OptionLevel::WindowOrPane, kind: OptionKind::Choice(&["off", "top", "bottom", "top-floating", "bottom-floating"]) },
    OptionSpec { name: "pane-border-style", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-colours", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-scrollbars", level: OptionLevel::Window, kind: OptionKind::Choice(&["off", "modal", "on", "auto-hide"]) },
    OptionSpec { name: "pane-scrollbars-timeout", level: OptionLevel::Window, kind: OptionKind::Number { minimum: 0, maximum: 2147483647 } },
    OptionSpec { name: "pane-scrollbars-style", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-scrollbars-position", level: OptionLevel::Window, kind: OptionKind::Choice(&["right", "left"]) },
    OptionSpec { name: "popup-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "popup-border-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "popup-border-lines", level: OptionLevel::Window, kind: OptionKind::Choice(&["single", "double", "heavy", "simple", "rounded", "padded", "none"]) },
    OptionSpec { name: "remain-on-exit", level: OptionLevel::WindowOrPane, kind: OptionKind::Choice(&["off", "on", "failed", "key", "failed-key"]) },
    OptionSpec { name: "remain-on-exit-format", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "scroll-on-clear", level: OptionLevel::WindowOrPane, kind: OptionKind::Flag },
    OptionSpec { name: "switch-mode-match-style", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "synchronize-panes", level: OptionLevel::WindowOrPane, kind: OptionKind::Flag },
    OptionSpec { name: "tiled-layout-max-columns", level: OptionLevel::Window, kind: OptionKind::Number { minimum: 0, maximum: 65535 } },
    OptionSpec { name: "tree-mode-border-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "tree-mode-preview-format", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "tree-mode-preview-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "tree-mode-selection-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-active-style", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "window-pane-current-status-format", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-pane-status-format", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-size", level: OptionLevel::Window, kind: OptionKind::Choice(&["largest", "smallest", "manual", "latest"]) },
    OptionSpec { name: "window-style", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "window-status-activity-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-status-bell-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-status-current-format", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-status-current-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-status-format", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-status-last-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-status-separator", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-status-style", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "wrap-search", level: OptionLevel::Window, kind: OptionKind::Flag },
    OptionSpec { name: "xterm-keys", level: OptionLevel::Window, kind: OptionKind::Flag },
    OptionSpec { name: "after-bind-key", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-capture-pane", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-copy-mode", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-display-message", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-display-panes", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-kill-pane", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-list-buffers", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-list-clients", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-list-keys", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-list-panes", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-list-sessions", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-list-windows", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-load-buffer", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-lock-server", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-new-session", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-new-window", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-paste-buffer", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-pipe-pane", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-refresh-client", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-rename-session", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-rename-window", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-resize-pane", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-resize-window", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-save-buffer", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-select-layout", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-select-pane", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-select-window", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-send-keys", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-set-buffer", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-set-environment", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-set-hook", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-set-option", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-show-environment", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-show-messages", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-show-options", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-split-window", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-swap-window", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "after-unbind-key", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "alert-activity", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "alert-bell", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "alert-silence", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-active", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-attached", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-created", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-closed", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-detached", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-focus-in", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-focus-out", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-resized", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-session-changed", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-light-theme", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "client-dark-theme", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "command-error", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "marked-pane-changed", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "pane-activity", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-bell", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-command-finished", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-command-started", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-created", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-died", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-exited", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-focus-in", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-focus-out", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-mode-changed", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-mode-entered", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-mode-exited", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-moved", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-prompt-closed", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-prompt-opened", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-resized", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-set-clipboard", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-shell-prompt", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "pane-title-changed", level: OptionLevel::WindowOrPane, kind: OptionKind::Other },
    OptionSpec { name: "session-closed", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "session-created", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "session-added-to-group", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "session-renamed", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "session-removed-from-group", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "session-window-changed", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "window-created", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-closed", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-layout-changed", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-linked", level: OptionLevel::Session, kind: OptionKind::Other },
    OptionSpec { name: "window-pane-changed", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-renamed", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-resized", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-unzoomed", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-zoomed", level: OptionLevel::Window, kind: OptionKind::Other },
    OptionSpec { name: "window-unlinked", level: OptionLevel::Session, kind: OptionKind::Other },
];
