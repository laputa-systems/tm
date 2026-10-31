use std::io;
use std::os::fd::AsFd;

use crate::model::Size;
use rustix::termios::{
    OptionalActions, SpecialCodeIndex, Termios, tcgetattr, tcgetwinsize, tcsetattr,
};
use vt100::Parser;

/// State needed to replay PTY output while preserving terminal sequences that
/// may be split across read boundaries. `vt100` handles the terminal state;
/// this small wrapper only expands DEC REP (`CSI Ps b`), which vt100 does not
/// currently implement.
#[derive(Clone, Debug)]
pub(crate) struct OutputState {
    pending: Vec<u8>,
    last_printed: Option<Vec<u8>>,
    wrap: bool,
    audible_bell: bool,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            last_printed: None,
            wrap: true,
            audible_bell: false,
        }
    }
}

impl OutputState {
    pub(crate) fn process(&mut self, parser: &mut Parser, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let escape_ready_len = incomplete_escape_start(&self.pending).unwrap_or(self.pending.len());
        let ready_len =
            incomplete_utf8_start(&self.pending[..escape_ready_len]).unwrap_or(escape_ready_len);
        let ready = self.pending.drain(..ready_len).collect::<Vec<_>>();
        replay_terminal_bytes(
            parser,
            &ready,
            &mut self.last_printed,
            &mut self.wrap,
            &mut self.audible_bell,
        );
    }

    /// Return whether the most recent processed bytes contained a standalone
    /// BEL. BEL terminators consumed as part of OSC/DCS/etc. are not reported.
    pub(crate) fn take_audible_bell(&mut self) -> bool {
        std::mem::take(&mut self.audible_bell)
    }

    pub(crate) fn finish(&mut self, parser: &mut Parser) {
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            parser.process(&pending);
        }
    }
}

/// Replay a retained PTY stream into a fresh parser using the same terminal
/// normalization as the live pane reader.
pub(crate) fn replay(parser: &mut Parser, bytes: &[u8]) {
    let mut state = OutputState::default();
    state.process(parser, bytes);
    state.finish(parser);
}

fn incomplete_escape_start(bytes: &[u8]) -> Option<usize> {
    let start = bytes.iter().rposition(|byte| *byte == 0x1b)?;
    escape_end(bytes, start).is_none().then_some(start)
}

fn incomplete_utf8_start(bytes: &[u8]) -> Option<usize> {
    let error = std::str::from_utf8(bytes).err()?;
    error.error_len().is_none().then_some(error.valid_up_to())
}

fn escape_end(bytes: &[u8], start: usize) -> Option<usize> {
    let kind = *bytes.get(start + 1)?;
    match kind {
        b'[' => bytes
            .iter()
            .enumerate()
            .skip(start + 2)
            .find(|(_, byte)| (0x40..=0x7e).contains(*byte))
            .map(|(index, _)| index + 1),
        b']' | b'P' | b'^' | b'_' => {
            let mut index = start + 2;
            while index < bytes.len() {
                if bytes[index] == 0x07 {
                    return Some(index + 1);
                }
                if bytes[index] == 0x1b {
                    return bytes
                        .get(index + 1)
                        .is_some_and(|byte| *byte == b'\\')
                        .then_some(index + 2);
                }
                index += 1;
            }
            None
        }
        b'#' => bytes.get(start + 2).map(|_| start + 3),
        _ => Some((start + 2).min(bytes.len())),
    }
}

fn repeat_count(sequence: &[u8]) -> Option<usize> {
    // DEC REP is CSI Ps b.  In particular, ESC b is a different (two-byte)
    // escape sequence and must not be sliced as if it were CSI b.
    if sequence.len() < 3 || !sequence.starts_with(b"\x1b[") || sequence.last() != Some(&b'b') {
        return None;
    }

    let body = &sequence[2..sequence.len() - 1];
    Some(
        body.split(|byte| *byte == b';')
            .next()
            .and_then(|value| {
                if value.is_empty() {
                    Some(1)
                } else {
                    std::str::from_utf8(value).ok()?.parse::<usize>().ok()
                }
            })
            .unwrap_or(1),
    )
}

fn replay_terminal_bytes(
    parser: &mut Parser,
    bytes: &[u8],
    last_printed: &mut Option<Vec<u8>>,
    wrap: &mut bool,
    audible_bell: &mut bool,
) {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b
            && let Some(end) = escape_end(bytes, index)
        {
            let sequence = &bytes[index..end];
            if sequence == b"\x1b#8" {
                parser.process(&screen_alignment(parser));
            } else if let Some(mode) = wrap_mode(sequence) {
                // vt100 intentionally leaves DECAWM to its caller.  It
                // still needs to see other private modes in a combined
                // sequence (for example ?7;25h), so process the escape
                // first and then update our local mode bit.
                parser.process(sequence);
                *wrap = mode;
            } else if sequence == b"\x1bc" {
                // RIS reconstructs vt100's Screen, including its mode
                // state. Keep the local DECAWM and REP state in lockstep.
                parser.process(sequence);
                *wrap = true;
                *last_printed = None;
            } else if let Some(count) = repeat_count(sequence) {
                if let Some(character) = last_printed.as_ref() {
                    for _ in 0..count {
                        write_terminal_character(parser, character, *wrap);
                    }
                }
            } else {
                parser.process(sequence);
            }
            index = end;
            continue;
        }

        let Some(character) = bytes.get(index).copied() else {
            break;
        };
        if character.is_ascii() {
            if character >= b' ' && character != 0x7f {
                write_terminal_character(parser, &[character], *wrap);
                *last_printed = Some(vec![character]);
            } else {
                if character == 0x07 {
                    *audible_bell = true;
                }
                parser.process(&[character]);
            }
            index += 1;
            continue;
        }
        let Some(value) = std::str::from_utf8(&bytes[index..]).ok() else {
            parser.process(&[character]);
            index += 1;
            continue;
        };
        let Some(character) = value.chars().next() else {
            break;
        };
        let length = character.len_utf8();
        write_terminal_character(parser, &bytes[index..index + length], *wrap);
        *last_printed = Some(bytes[index..index + length].to_vec());
        index += length;
    }
}

fn wrap_mode(sequence: &[u8]) -> Option<bool> {
    if sequence.len() < 5 || !sequence.starts_with(b"\x1b[?") {
        return None;
    }
    let mode = match sequence.last().copied() {
        Some(b'h') => true,
        Some(b'l') => false,
        _ => return None,
    };
    let mut includes_wrap = false;
    for value in sequence[3..sequence.len() - 1].split(|byte| *byte == b';') {
        let Ok(value) = std::str::from_utf8(value) else {
            return None;
        };
        let Ok(value) = value.parse::<u16>() else {
            return None;
        };
        includes_wrap |= value == 7;
    }
    includes_wrap.then_some(mode)
}

fn write_terminal_character(parser: &mut Parser, character: &[u8], wrap: bool) {
    let (row, col) = parser.screen().cursor_position();
    let cols = parser.screen().size().1;
    parser.process(character);
    if !wrap && col.saturating_add(1) >= cols {
        restore_no_wrap_cursor(parser, row);
    }
}

fn restore_no_wrap_cursor(parser: &mut Parser, row: u16) {
    let (rows, cols) = parser.screen().size();
    if cols == 0 || rows == 0 {
        return;
    }
    let mut position = Vec::new();
    position.extend_from_slice(b"\x1b[");
    position.extend_from_slice((u32::from(row.min(rows - 1)) + 1).to_string().as_bytes());
    position.extend_from_slice(b";");
    position.extend_from_slice(u32::from(cols).to_string().as_bytes());
    position.push(b'H');
    parser.process(&position);
}

fn screen_alignment(parser: &Parser) -> Vec<u8> {
    let (rows, cols) = parser.screen().size();
    let (cursor_row, cursor_col) = parser.screen().cursor_position();
    let mut output = Vec::new();
    output.extend_from_slice(b"\x1b[2J");
    for row in 0..rows {
        output.extend_from_slice(b"\x1b[");
        output.extend_from_slice((u32::from(row) + 1).to_string().as_bytes());
        output.extend_from_slice(b";1H");
        output.extend(std::iter::repeat_n(b'E', usize::from(cols)));
    }
    output.extend_from_slice(b"\x1b[");
    output.extend_from_slice((u32::from(cursor_row) + 1).to_string().as_bytes());
    output.push(b';');
    output.extend_from_slice((u32::from(cursor_col) + 1).to_string().as_bytes());
    output.push(b'H');
    output
}

pub(crate) fn size() -> Size {
    let stdin = io::stdin();
    match tcgetwinsize(stdin.as_fd()) {
        Ok(winsize) if winsize.ws_col > 0 && winsize.ws_row > 0 => {
            Size::new(winsize.ws_col, winsize.ws_row)
        }
        _ => Size::new(80, 24),
    }
}


#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalModes {
    pub alternate: bool,
    pub mouse: bool,
    pub focus: bool,
    pub paste: bool,
    pub extkeys: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Capabilities {
    pub alternate_enter: Option<&'static [u8]>,
    pub alternate_leave: Option<&'static [u8]>,
    pub mouse_enable: Option<&'static [u8]>,
    pub mouse_disable: Option<&'static [u8]>,
    pub paste_enable: Option<&'static [u8]>,
    pub paste_disable: Option<&'static [u8]>,
    pub focus_enable: Option<&'static [u8]>,
    pub focus_disable: Option<&'static [u8]>,
    pub sync_begin: Option<&'static [u8]>,
    pub sync_end: Option<&'static [u8]>,
    pub extkeys_enable: Option<&'static [u8]>,
    pub extkeys_disable: Option<&'static [u8]>,
    pub clipboard: bool,
}

const ALTERNATE_ENTER: &[u8] = b"\x1b[?1049h";
const ALTERNATE_LEAVE: &[u8] = b"\x1b[?1049l";
const MOUSE_ENABLE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const MOUSE_DISABLE: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1006l";
const PASTE_ENABLE: &[u8] = b"\x1b[?2004h";
const PASTE_DISABLE: &[u8] = b"\x1b[?2004l";
const FOCUS_ENABLE: &[u8] = b"\x1b[?1004h";
const FOCUS_DISABLE: &[u8] = b"\x1b[?1004l";
const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";
const EXTKEYS_ENABLE: &[u8] = b"\x1b[>4;2m";
const EXTKEYS_DISABLE: &[u8] = b"\x1b[>4m";

impl Capabilities {
    pub(crate) fn for_term(term: &str, features: &str) -> Self {
        let mut capabilities = match term {
            "tmux-256color" | "xterm-ghostty" => Self {
                alternate_enter: Some(ALTERNATE_ENTER),
                alternate_leave: Some(ALTERNATE_LEAVE),
                mouse_enable: Some(MOUSE_ENABLE),
                mouse_disable: Some(MOUSE_DISABLE),
                paste_enable: Some(PASTE_ENABLE),
                paste_disable: Some(PASTE_DISABLE),
                focus_enable: Some(FOCUS_ENABLE),
                focus_disable: Some(FOCUS_DISABLE),
                sync_begin: Some(SYNC_BEGIN),
                sync_end: Some(SYNC_END),
                extkeys_enable: None,
                extkeys_disable: None,
                clipboard: true,
            },
            "xterm-256color" => Self {
                sync_begin: None,
                sync_end: None,
                ..Self::for_term("tmux-256color", "")
            },
            _ => Self {
                alternate_enter: Some(ALTERNATE_ENTER),
                alternate_leave: Some(ALTERNATE_LEAVE),
                mouse_enable: None,
                mouse_disable: None,
                paste_enable: None,
                paste_disable: None,
                focus_enable: None,
                focus_disable: None,
                sync_begin: None,
                sync_end: None,
                extkeys_enable: None,
                extkeys_disable: None,
                clipboard: false,
            },
        };
        for entry in features.split([',', ' ']).filter(|entry| !entry.is_empty()) {
            let mut parts = entry.split(':');
            let Some(pattern) = parts.next() else {
                continue;
            };
            if !wildcard_match(pattern, term) {
                continue;
            }
            for feature in parts {
                match feature.to_ascii_lowercase().as_str() {
                    "sync" => {
                        capabilities.sync_begin = Some(SYNC_BEGIN);
                        capabilities.sync_end = Some(SYNC_END);
                    }
                    "clipboard" => capabilities.clipboard = true,
                    "focus" => {
                        capabilities.focus_enable = Some(FOCUS_ENABLE);
                        capabilities.focus_disable = Some(FOCUS_DISABLE);
                    }
                    "bpaste" => {
                        capabilities.paste_enable = Some(PASTE_ENABLE);
                        capabilities.paste_disable = Some(PASTE_DISABLE);
                    }
                    "extkeys" => {
                        capabilities.extkeys_enable = Some(EXTKEYS_ENABLE);
                        capabilities.extkeys_disable = Some(EXTKEYS_DISABLE);
                    }
                    _ => {}
                }
            }
        }
        capabilities
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star = None;
    let mut star_value = 0;
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            star_value = value_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_value += 1;
            value_index = star_value;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

pub(crate) fn mode_diff(
    previous: TerminalModes,
    desired: TerminalModes,
    capabilities: &Capabilities,
    output: &mut Vec<u8>,
) {
    if previous.alternate != desired.alternate {
        if desired.alternate {
            if let Some(bytes) = capabilities.alternate_enter {
                output.extend_from_slice(bytes);
            }
        } else if let Some(bytes) = capabilities.alternate_leave {
            output.extend_from_slice(bytes);
        }
    }
    if previous.mouse != desired.mouse {
        if desired.mouse {
            if let Some(bytes) = capabilities.mouse_enable {
                output.extend_from_slice(bytes);
            }
        } else if let Some(bytes) = capabilities.mouse_disable {
            output.extend_from_slice(bytes);
        }
    }
    if previous.paste != desired.paste {
        if desired.paste {
            if let Some(bytes) = capabilities.paste_enable {
                output.extend_from_slice(bytes);
            }
        } else if let Some(bytes) = capabilities.paste_disable {
            output.extend_from_slice(bytes);
        }
    }
    if previous.focus != desired.focus {
        if desired.focus {
            if let Some(bytes) = capabilities.focus_enable {
                output.extend_from_slice(bytes);
            }
        } else if let Some(bytes) = capabilities.focus_disable {
            output.extend_from_slice(bytes);
        }
    }
    if previous.extkeys != desired.extkeys {
        if desired.extkeys {
            if let Some(bytes) = capabilities.extkeys_enable {
                output.extend_from_slice(bytes);
            }
        } else if let Some(bytes) = capabilities.extkeys_disable {
            output.extend_from_slice(bytes);
        }
    }
}

pub(crate) fn restore_sequence(capabilities: &Capabilities) -> Vec<u8> {
    let mut restore = Vec::new();
    mode_diff(
        TerminalModes {
            alternate: true,
            mouse: true,
            paste: true,
            focus: true,
            extkeys: true,
        },
        TerminalModes::default(),
        capabilities,
        &mut restore,
    );
    if let Some(bytes) = capabilities.sync_end {
        restore.extend_from_slice(bytes);
    }
    restore.extend_from_slice(b"\x1b[?25h\x1b[0m");
    restore
}

pub(crate) struct RawTerminal {
    original: Termios,
    active: bool,
}

impl RawTerminal {
    pub(crate) fn enter() -> io::Result<Self> {
        let stdin = io::stdin();
        let original = tcgetattr(stdin.as_fd()).map_err(io::Error::from)?;
        let mut raw = original.clone();
        raw.make_raw();
        raw.special_codes[SpecialCodeIndex::VMIN] = 1;
        raw.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(stdin.as_fd(), OptionalActions::Now, &raw).map_err(io::Error::from)?;
        Ok(Self {
            original,
            active: true,
        })
    }

    pub(crate) fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        let stdin = io::stdin();
        let result =
            tcsetattr(stdin.as_fd(), OptionalActions::Now, &self.original).map_err(io::Error::from);
        self.active = false;
        result
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    struct ScreenSnapshot {
        contents: Vec<u8>,
        cursor: (u16, u16),
        alternate: bool,
        scrollback: usize,
        fgcolor: vt100::Color,
        bgcolor: vt100::Color,
        bold: bool,
        dim: bool,
        italic: bool,
        underline: bool,
        inverse: bool,
        wrapped_rows: Vec<bool>,
    }

    fn snapshot(parser: &Parser) -> ScreenSnapshot {
        let screen = parser.screen();
        let (rows, _) = screen.size();
        ScreenSnapshot {
            contents: screen.contents_formatted(),
            cursor: screen.cursor_position(),
            alternate: screen.alternate_screen(),
            scrollback: screen.scrollback(),
            fgcolor: screen.fgcolor(),
            bgcolor: screen.bgcolor(),
            bold: screen.bold(),
            dim: screen.dim(),
            italic: screen.italic(),
            underline: screen.underline(),
            inverse: screen.inverse(),
            wrapped_rows: (0..rows).map(|row| screen.row_wrapped(row)).collect(),
        }
    }

    /// Exercise all byte boundaries in addition to the live one-shot path.
    /// PTY reads can split an escape, UTF-8 scalar, OSC, or REP sequence at
    /// any position, so this is the deterministic oracle for OutputState.
    fn assert_chunk_invariant(bytes: &[u8], rows: u16, cols: u16) {
        let mut expected_parser = Parser::new(rows, cols, 100);
        let mut expected_state = OutputState::default();
        expected_state.process(&mut expected_parser, bytes);
        expected_state.finish(&mut expected_parser);
        let expected = snapshot(&expected_parser);

        for chunk_size in 1..=bytes.len().max(1) {
            let mut parser = Parser::new(rows, cols, 100);
            let mut state = OutputState::default();
            for chunk in bytes.chunks(chunk_size) {
                state.process(&mut parser, chunk);
            }
            state.finish(&mut parser);
            assert_eq!(snapshot(&parser), expected, "chunk size {chunk_size}");
        }

        let mut replay_parser = Parser::new(rows, cols, 100);
        replay(&mut replay_parser, bytes);
        assert_eq!(snapshot(&replay_parser), expected, "replay diverged");
    }

    #[test]
    fn output_state_expands_split_dec_repeat_sequences() {
        let mut parser = Parser::new(3, 10, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"A\x1b[4");
        state.process(&mut parser, b"bB");
        assert!(parser.screen().contents().starts_with("AAAAAB"));
    }

    #[test]
    fn output_state_expands_split_screen_alignment_sequences() {
        let mut parser = Parser::new(3, 4, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"\x1b#");
        state.process(&mut parser, b"8");
        assert!(parser.screen().rows(0, 4).all(|row| row == "EEEE"));
    }

    #[test]
    fn output_state_honors_split_no_wrap_mode_sequences() {
        let mut parser = Parser::new(3, 5, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"\x1b[?7");
        state.process(&mut parser, b"labcdeF");
        assert_eq!(parser.screen().rows(0, 5).next(), Some("abcdF".to_owned()));
    }

    #[test]
    fn output_state_tracks_wrap_in_combined_private_mode_sequences() {
        let mut parser = Parser::new(3, 5, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"\x1b[?7;25labcdeF");
        assert_eq!(parser.screen().rows(0, 5).next(), Some("abcdF".to_owned()));
        assert!(parser.screen().hide_cursor());
    }

    #[test]
    fn output_state_honors_split_utf8_characters() {
        let mut parser = Parser::new(3, 10, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"\xe3");
        state.process(&mut parser, b"\x81\x82");
        assert!(parser.screen().contents().contains('あ'));
    }

    #[test]
    fn output_state_does_not_treat_esc_b_as_dec_repeat() {
        let mut parser = Parser::new(2, 8, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"A\x1bbB");
        state.finish(&mut parser);
        assert_eq!(parser.screen().rows(0, 8).next(), Some("AB".to_owned()));
    }

    #[test]
    fn repeat_count_only_accepts_csi_repeat_sequences() {
        assert_eq!(repeat_count(b"\x1bb"), None);
        assert_eq!(repeat_count(b"\x1b[b"), Some(1));
        assert_eq!(repeat_count(b"\x1b[4b"), Some(4));
        assert_eq!(repeat_count(b"\x1b[4;2b"), Some(4));
    }

    #[test]
    fn output_state_ris_resets_local_wrap_and_repeat_state() {
        let mut parser = Parser::new(2, 5, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"A\x1b[?7lbcde\x1bc\x1b[2bF");
        state.finish(&mut parser);

        // RIS clears the old screen, restores DECAWM, and clears the
        // preceding graphic character used by REP.
        assert_eq!(parser.screen().rows(0, 5).next(), Some("F".to_owned()));
        assert_eq!(parser.screen().cursor_position(), (0, 1));
    }

    #[test]
    fn output_state_preserves_wrap_mode_across_alternate_screen_switches() {
        let bytes = b"\x1b[?7labcde\x1b[?1049h12345F\x1b[?1049lZ";
        assert_chunk_invariant(bytes, 2, 5);

        let mut parser = Parser::new(2, 5, 100);
        replay(&mut parser, bytes);
        assert!(!parser.screen().alternate_screen());
        assert_eq!(parser.screen().rows(0, 5).next(), Some("abcdZ".to_owned()));
    }

    #[test]
    fn output_state_is_chunk_invariant_for_vt100_fixture() {
        let bytes = b"\x1b[2J\x1b[1;1Hplain \x1b[31mred\x1b[0m \xc3\xa9\xe3\x81\x82\xcc\x81\r\n\x1b[2;3HAB\x1b[4bC\x1b[?25l\x1b]2;window title\x07\x1b]7;file:///tmp/demo\x1b\\\x1b[?25h";
        assert_chunk_invariant(bytes, 4, 20);
    }

    #[test]
    fn output_state_is_chunk_invariant_for_reset_alt_and_malformed_sequences() {
        let bytes = b"prefix\x1b[?7l012345\x1b[?1049hALT\x1b[?1049l\x1bcpost\x1b[31\x1b[?25h\x1b]2;unterminated";
        assert_chunk_invariant(bytes, 3, 6);
    }

    #[test]
    fn output_state_reports_only_standalone_audible_bells() {
        let mut parser = Parser::new(2, 20, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"\x1b]2;title\x07");
        assert!(!state.take_audible_bell());

        state.process(&mut parser, b"\x1b]7;file:///tmp\x1b");
        assert!(!state.take_audible_bell());
        state.process(&mut parser, b"\\\x07");
        assert!(state.take_audible_bell());
    }
}
