#[derive(Debug)]
pub struct Capabilities {
    pub alternate_enter: Option<&'static [u8]>,
    pub alternate_leave: Option<&'static [u8]>,
    pub paste_enable: Option<&'static [u8]>,
    pub paste_disable: Option<&'static [u8]>,
    pub focus_enable: Option<&'static [u8]>,
    pub focus_disable: Option<&'static [u8]>,
    pub sync_begin: Option<&'static [u8]>,
    pub sync_end: Option<&'static [u8]>,
    pub clipboard: bool,
}

const ALTERNATE_ENTER: &[u8] = b"\x1b[?1049h";
const ALTERNATE_LEAVE: &[u8] = b"\x1b[?1049l";
const PASTE_ENABLE: &[u8] = b"\x1b[?2004h";
const PASTE_DISABLE: &[u8] = b"\x1b[?2004l";
const FOCUS_ENABLE: &[u8] = b"\x1b[?1004h";
const FOCUS_DISABLE: &[u8] = b"\x1b[?1004l";
const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";

const TMUX_256COLOR: Capabilities = Capabilities {
    alternate_enter: Some(ALTERNATE_ENTER),
    alternate_leave: Some(ALTERNATE_LEAVE),
    paste_enable: Some(PASTE_ENABLE),
    paste_disable: Some(PASTE_DISABLE),
    focus_enable: Some(FOCUS_ENABLE),
    focus_disable: Some(FOCUS_DISABLE),
    sync_begin: Some(SYNC_BEGIN),
    sync_end: Some(SYNC_END),
    clipboard: true,
};

const XTERM_GHOSTTY: Capabilities = TMUX_256COLOR;

const XTERM_256COLOR: Capabilities = Capabilities {
    sync_begin: None,
    sync_end: None,
    ..TMUX_256COLOR
};

const CONSERVATIVE: Capabilities = Capabilities {
    alternate_enter: Some(ALTERNATE_ENTER),
    alternate_leave: Some(ALTERNATE_LEAVE),
    paste_enable: None,
    paste_disable: None,
    focus_enable: None,
    focus_disable: None,
    sync_begin: None,
    sync_end: None,
    clipboard: false,
};

pub const EXTENDED_KEYS_ENABLE: &[u8] = b"\x1b[>4;2m";
pub const EXTENDED_KEYS_DISABLE: &[u8] = b"\x1b[>4m";

pub fn for_term(term: &str) -> &'static Capabilities {
    match term {
        "tmux-256color" => &TMUX_256COLOR,
        "xterm-ghostty" => &XTERM_GHOSTTY,
        "xterm-256color" => &XTERM_256COLOR,
        _ => &CONSERVATIVE,
    }
}
