use std::io;
use std::os::fd::AsFd;

use crate::model::Size;
use rustix::termios::{
    tcgetattr, tcgetwinsize, tcsetattr, OptionalActions, SpecialCodeIndex, Termios,
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
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            last_printed: None,
            wrap: true,
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
        replay_terminal_bytes(parser, &ready, &mut self.last_printed, &mut self.wrap);
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
    (sequence.last() == Some(&b'b')).then(|| {
        let body = &sequence[2..sequence.len().saturating_sub(1)];
        body.split(|byte| *byte == b';')
            .next()
            .and_then(|value| {
                if value.is_empty() {
                    Some(1)
                } else {
                    std::str::from_utf8(value).ok()?.parse::<usize>().ok()
                }
            })
            .unwrap_or(1)
    })
}

fn replay_terminal_bytes(
    parser: &mut Parser,
    bytes: &[u8],
    last_printed: &mut Option<Vec<u8>>,
    wrap: &mut bool,
) {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            if let Some(end) = escape_end(bytes, index) {
                let sequence = &bytes[index..end];
                if sequence == b"\x1b#8" {
                    parser.process(&screen_alignment(parser));
                } else if let Some(mode) = wrap_mode(sequence) {
                    *wrap = mode;
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
        }

        let Some(character) = bytes.get(index).copied() else {
            break;
        };
        if character.is_ascii() {
            if character >= b' ' && character != 0x7f {
                write_terminal_character(parser, &[character], *wrap);
                *last_printed = Some(vec![character]);
            } else {
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
    if sequence.len() < 5 || !sequence.starts_with(b"\x1b[") {
        return None;
    }
    match sequence.last().copied() {
        Some(b'h') if &sequence[2..sequence.len() - 1] == b"?7" => Some(true),
        Some(b'l') if &sequence[2..sequence.len() - 1] == b"?7" => Some(false),
        _ => None,
    }
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
    fn output_state_honors_split_utf8_characters() {
        let mut parser = Parser::new(3, 10, 100);
        let mut state = OutputState::default();
        state.process(&mut parser, b"\xe3");
        state.process(&mut parser, b"\x81\x82");
        assert!(parser.screen().contents().contains('あ'));
    }
}
