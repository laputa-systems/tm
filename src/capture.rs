use vt100::{Cell, Color, Parser};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CaptureStyle {
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
    foreground: Color,
    background: Color,
}

impl CaptureStyle {
    fn of_cell(cell: &Cell) -> Self {
        if cell.has_contents() || cell.is_wide_continuation() {
            Self {
                bold: cell.bold(),
                dim: cell.dim(),
                italic: cell.italic(),
                underline: cell.underline(),
                inverse: cell.inverse(),
                foreground: cell.fgcolor(),
                background: cell.bgcolor(),
            }
        } else {
            Self {
                background: cell.bgcolor(),
                ..Self::default()
            }
        }
    }

    fn attributes(self) -> [bool; 5] {
        [
            self.bold,
            self.dim,
            self.italic,
            self.underline,
            self.inverse,
        ]
    }
}

const ATTRIBUTE_CODES: [u16; 5] = [1, 2, 3, 4, 7];

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureFormat {
    pub(crate) escape: bool,
    pub(crate) join: bool,
    pub(crate) preserve_trailing: bool,
}

pub(crate) struct CapturedRow {
    pub(crate) text: Vec<u8>,
    pub(crate) wrapped: bool,
}

pub(crate) fn capture_row(
    parser: &mut Parser,
    history_rows: usize,
    row: usize,
    format: CaptureFormat,
    previous: &mut CaptureStyle,
) -> CapturedRow {
    let saved = parser.screen().scrollback();
    let (rows, cols) = parser.screen().size();
    let (offset, visible_row) = if row < history_rows {
        (history_rows - row, 0)
    } else {
        (0, (row - history_rows).min(usize::from(rows).saturating_sub(1)))
    };
    parser.screen_mut().set_scrollback(offset);
    let visible_row = u16::try_from(visible_row).unwrap_or(u16::MAX);
    let screen = parser.screen();
    let cells = (0..cols)
        .filter_map(|column| screen.cell(visible_row, column).cloned())
        .collect::<Vec<_>>();
    let wrapped = screen.row_wrapped(visible_row);
    parser.screen_mut().set_scrollback(saved);

    let end = if format.join {
        written_extent(&cells)
    } else {
        allocated_extent(&cells)
    };
    let mut text = Vec::new();
    for cell in cells.iter().take(end) {
        if cell.is_wide_continuation() {
            continue;
        }
        if format.escape {
            let style = CaptureStyle::of_cell(cell);
            write_style_transition(&mut text, *previous, style);
            *previous = style;
        }
        if cell.has_contents() {
            text.extend_from_slice(cell.contents().as_bytes());
        } else {
            text.push(b' ');
        }
    }
    if !format.join && !format.preserve_trailing {
        while text.last() == Some(&b' ') {
            text.pop();
        }
    }
    CapturedRow { text, wrapped }
}

fn written_extent(cells: &[Cell]) -> usize {
    cells
        .iter()
        .rposition(|cell| cell.has_contents() || cell.is_wide_continuation())
        .map_or(0, |index| index + 1)
}

/// tmux keeps cleared cells after the last written one up to the capacity its
/// line storage grew to, which depends on how each run of identically styled
/// cells extended the line. vt100 keeps no such history, so the capacity is
/// rebuilt from the cells that currently carry content or a background.
fn allocated_extent(cells: &[Cell]) -> usize {
    let width = cells.len();
    let mut capacity = 0;
    let mut run_style: Option<CaptureStyle> = None;
    for (index, cell) in cells.iter().enumerate() {
        let touched =
            cell.has_contents() || cell.is_wide_continuation() || cell.bgcolor() != Color::Default;
        let style = touched.then(|| CaptureStyle::of_cell(cell));
        let run_ends = match (run_style, style) {
            (Some(open), Some(next)) => open != next,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if run_ends {
            capacity = capacity.max(grown_capacity(index, capacity, width));
        }
        run_style = style;
    }
    if run_style.is_some() {
        capacity = capacity.max(grown_capacity(width, capacity, width));
    }
    capacity
}

fn grown_capacity(needed: usize, capacity: usize, width: usize) -> usize {
    if needed <= capacity {
        capacity
    } else if needed < width / 4 {
        width / 4
    } else if needed < width / 2 {
        width / 2
    } else {
        width
    }
}

fn write_style_transition(out: &mut Vec<u8>, last: CaptureStyle, style: CaptureStyle) {
    let mut codes: Vec<u16> = Vec::new();
    let mut last_attributes = last.attributes();
    let removed = last_attributes
        .iter()
        .zip(style.attributes())
        .any(|(was, is)| *was && !is);
    if removed {
        codes.push(0);
        last_attributes = [false; 5];
    }
    for ((was, is), code) in last_attributes
        .iter()
        .zip(style.attributes())
        .zip(ATTRIBUTE_CODES)
    {
        if is && !*was {
            codes.push(code);
        }
    }
    write_sgr(out, &codes);
    write_color_transition(out, removed, color_codes(last.foreground, 30), color_codes(style.foreground, 30));
    write_color_transition(out, removed, color_codes(last.background, 40), color_codes(style.background, 40));
}

fn write_color_transition(out: &mut Vec<u8>, reset: bool, old: Vec<u16>, new: Vec<u16>) {
    if !reset && old == new {
        return;
    }
    if reset && matches!(new.first(), Some(39 | 49)) {
        return;
    }
    write_sgr(out, &new);
}

fn write_sgr(out: &mut Vec<u8>, codes: &[u16]) {
    if codes.is_empty() {
        return;
    }
    out.extend_from_slice(b"\x1b[");
    for (index, code) in codes.iter().enumerate() {
        if index > 0 {
            out.push(b';');
        }
        out.extend_from_slice(code.to_string().as_bytes());
    }
    out.push(b'm');
}

fn color_codes(color: Color, base: u16) -> Vec<u16> {
    match color {
        Color::Default => vec![base + 9],
        Color::Idx(index) if index < 8 => vec![base + u16::from(index)],
        Color::Idx(index) if index < 16 => vec![base + 52 + u16::from(index)],
        Color::Idx(index) => vec![base + 8, 5, u16::from(index)],
        Color::Rgb(red, green, blue) => vec![
            base + 8,
            2,
            u16::from(red),
            u16::from(green),
            u16::from(blue),
        ],
    }
}
