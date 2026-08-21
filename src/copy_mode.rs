use vt100::Parser;

/// The keymap used by copy mode. The mode state is deliberately independent
/// from the live PTY so scrolling and selection remain deterministic while the
/// pane continues to receive output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyModeKeys {
    Emacs,
    Vi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyPromptKind {
    SearchForward,
    SearchBackward,
    SearchForwardIncremental,
    SearchBackwardIncremental,
    GotoLine,
    JumpForward,
    JumpBackward,
    JumpToForward,
    JumpToBackward,
}

#[derive(Debug)]
struct CopyPrompt {
    kind: CopyPromptKind,
    input: Vec<u8>,
    cursor: usize,
    history_index: usize,
    quoted: bool,
    yank_buffer: Vec<u8>,
}

impl CopyPrompt {
    fn move_left(&mut self) {
        self.cursor = previous_utf8_boundary(&self.input, self.cursor);
    }

    fn move_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor += std::str::from_utf8(&self.input[self.cursor..])
                .ok()
                .and_then(|value| value.chars().next())
                .map_or(1, char::len_utf8);
        }
    }

    fn move_start(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.input.len();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = previous_utf8_boundary(&self.input, self.cursor);
        self.input.drain(start..self.cursor);
        self.cursor = start;
    }

    fn delete(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let end = self.cursor
            + std::str::from_utf8(&self.input[self.cursor..])
                .ok()
                .and_then(|value| value.chars().next())
                .map_or(1, char::len_utf8);
        self.input.drain(self.cursor..end);
    }

    fn kill_to_start(&mut self) -> Vec<u8> {
        let killed = self.input[..self.cursor].to_vec();
        self.input.drain(..self.cursor);
        self.cursor = 0;
        killed
    }

    fn kill_to_end(&mut self) -> Vec<u8> {
        let killed = self.input[self.cursor..].to_vec();
        self.input.truncate(self.cursor);
        killed
    }

    fn kill_word(&mut self) -> Vec<u8> {
        let original_cursor = self.cursor;
        let mut start = self.cursor;
        while start > 0 {
            let previous = previous_utf8_boundary(&self.input, start);
            let character = std::str::from_utf8(&self.input[previous..start])
                .ok()
                .and_then(|value| value.chars().next())
                .unwrap_or(' ');
            if !character.is_whitespace() {
                break;
            }
            start = previous;
        }
        while start > 0 {
            let previous = previous_utf8_boundary(&self.input, start);
            let character = std::str::from_utf8(&self.input[previous..start])
                .ok()
                .and_then(|value| value.chars().next())
                .unwrap_or(' ');
            if character.is_whitespace() {
                break;
            }
            start = previous;
        }
        let killed = self.input[start..original_cursor].to_vec();
        self.input.drain(start..self.cursor);
        self.cursor = start;
        killed
    }

    fn history_up(&mut self, history: &[Vec<u8>]) {
        if history.is_empty() {
            return;
        }
        self.history_index = self.history_index.saturating_sub(1);
        self.input = history[self.history_index].clone();
        self.cursor = self.input.len();
    }

    fn history_down(&mut self, history: &[Vec<u8>]) {
        if self.history_index + 1 < history.len() {
            self.history_index += 1;
            self.input = history[self.history_index].clone();
        } else {
            self.history_index = history.len();
            self.input.clear();
        }
        self.cursor = self.input.len();
    }

    fn yank(&mut self) {
        if self.yank_buffer.is_empty() {
            return;
        }
        self.input
            .splice(self.cursor..self.cursor, self.yank_buffer.iter().copied());
        self.cursor += self.yank_buffer.len();
    }

    fn insert(&mut self, byte: u8) {
        self.input.insert(self.cursor, byte);
        self.cursor += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionMode {
    Char,
    Word,
    Line,
}

pub(crate) const DEFAULT_WORD_SEPARATORS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CopyPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyLineNumberMode {
    Off,
    Default,
    Absolute,
    Relative,
    Hybrid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CopyAction {
    Cancel,
    HistoryTop,
    HistoryBottom,
    StartOfLine,
    EndOfLine,
    BeginSelection,
    SelectLine,
    SelectWord,
    ClearSelection,
    StopSelection,
    OtherEnd,
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    ScrollUp,
    ScrollDown,
    ScrollTop,
    ScrollBottom,
    ScrollMiddle,
    ScrollToMouse(Option<usize>),
    RecenterTopBottom,
    PageUp,
    PageDown,
    CopySelection,
    CopySelectionNoClear,
    CopySelectionAndCancel,
    CopySelectionWithOptions {
        prefix: Option<String>,
        clear: bool,
        cancel: bool,
        set_paste: bool,
        set_clipboard: bool,
    },
    CopyEndOfLine,
    CopyEndOfLineAndCancel,
    CopyLine,
    CopyLineAndCancel,
    CopyLineWithOptions {
        prefix: Option<String>,
        whole_line: bool,
        cancel: bool,
        set_paste: bool,
        set_clipboard: bool,
    },
    CopyPipe {
        command: String,
        clear: bool,
        cancel: bool,
        store: bool,
    },
    CopyPipeWithOptions {
        command: String,
        prefix: Option<String>,
        clear: bool,
        cancel: bool,
        store: bool,
        set_paste: bool,
        set_clipboard: bool,
    },
    CopyPipeEndOfLine {
        command: String,
        cancel: bool,
    },
    CopyPipeLine {
        command: String,
        cancel: bool,
    },
    CopyPipeLineWithOptions {
        command: String,
        prefix: Option<String>,
        whole_line: bool,
        cancel: bool,
        set_paste: bool,
        set_clipboard: bool,
    },
    AppendSelection,
    AppendSelectionAndCancel,
    CursorDownAndCancel,
    PageDownAndCancel,
    RectangleToggle,
    RectangleOn,
    RectangleOff,
    SelectionMode(SelectionMode),
    TopLine,
    MiddleLine,
    BottomLine,
    CursorCentreVertical,
    HalfPageUp,
    HalfPageDown,
    HalfPageDownAndCancel,
    BackToIndentation,
    CursorCentreHorizontal,
    ScrollDownAndCancel,
    ScrollExitOn,
    ScrollExitOff,
    ScrollExitToggle,
    SetMark,
    JumpToMark,
    JumpForward(String),
    JumpBackward(String),
    JumpToForward(String),
    JumpToBackward(String),
    JumpAgain,
    JumpReverse,
    PreviousParagraph,
    NextParagraph,
    PreviousMatchingBracket,
    NextMatchingBracket,
    SearchForward(String),
    SearchForwardText(String),
    SearchBackward(String),
    SearchBackwardText(String),
    SearchForwardIncremental(String),
    SearchBackwardIncremental(String),
    NextPrompt,
    PreviousPrompt,
    /// Re-read the live pane while preserving the current copy-mode state.
    RefreshFromPane,
    RefreshOn,
    RefreshOff,
    RefreshToggle,
    LineNumbersOn,
    LineNumbersOff,
    LineNumbersToggle,
    TogglePosition,
    SearchAgain,
    SearchReverse,
    GotoLine(usize),
    NextWord,
    NextWordEnd,
    PreviousWord,
    PreviousSpace,
    NextSpace,
    NextSpaceEnd,
}

/// tmux folds search text only when every ASCII letter in the query is
/// lowercase; an uppercase query remains case-sensitive.
fn search_is_lowercase(search: &str) -> bool {
    search
        .bytes()
        .all(|character| character == character.to_ascii_lowercase())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyScrollbarHit {
    BeforeSlider,
    Slider,
    AfterSlider,
}

pub(crate) fn scrollbar_geometry_for(
    total_rows: usize,
    viewport_rows: usize,
    max_scroll: usize,
    scroll_offset: usize,
) -> (usize, usize) {
    let viewport_rows = viewport_rows.max(1);
    let total_rows = total_rows.max(1);
    let slider_rows = (viewport_rows
        .saturating_mul(viewport_rows)
        .checked_div(total_rows)
        .unwrap_or(1))
    .max(1)
    .min(viewport_rows);
    if max_scroll == 0 {
        return (0, slider_rows);
    }
    let slider_max = viewport_rows.saturating_sub(slider_rows);
    let offset_from_top = max_scroll.saturating_sub(scroll_offset).min(max_scroll);
    let slider_top = (viewport_rows
        .saturating_add(1)
        .saturating_mul(offset_from_top)
        / total_rows)
        .min(slider_max);
    (slider_top, slider_rows)
}

pub(crate) fn scrollbar_hit_for(
    total_rows: usize,
    viewport_rows: usize,
    max_scroll: usize,
    scroll_offset: usize,
    row: usize,
) -> CopyScrollbarHit {
    if max_scroll == 0 {
        return CopyScrollbarHit::Slider;
    }
    let (slider_top, slider_rows) =
        scrollbar_geometry_for(total_rows, viewport_rows, max_scroll, scroll_offset);
    if row < slider_top {
        CopyScrollbarHit::BeforeSlider
    } else if row < slider_top.saturating_add(slider_rows) {
        CopyScrollbarHit::Slider
    } else {
        CopyScrollbarHit::AfterSlider
    }
}

#[derive(Debug)]
pub(crate) struct CopyModeState {
    pub cursor: CopyPosition,
    pub cursor_x: usize,
    pub anchor: Option<CopyPosition>,
    pub selection_active: bool,
    pub rectangle: bool,
    pub selection_mode: SelectionMode,
    pub mark: Option<CopyPosition>,
    pub scroll_offset: usize,
    pub exit_on_scroll: bool,
    pub kill_on_exit: bool,
    pub hide_position: bool,
    pub refresh_active: bool,
    pub line_numbers: bool,
    pub line_number_mode: CopyLineNumberMode,
    pub keys: CopyModeKeys,
    wrap_search: bool,
    word_separators: String,
    raw_output: Vec<u8>,
    history_floor: usize,
    last_search: Option<(String, bool, bool)>,
    search_origin: Option<CopyPosition>,
    last_jump: Option<(String, bool, bool)>,
    prompt_history: Vec<Vec<u8>>,
    prompt: Option<CopyPrompt>,
    pending_repeat: usize,
    mouse_anchor: Option<CopyPosition>,
}

#[derive(Debug, Default)]
pub(crate) struct CopyActionResult {
    pub copied: Option<String>,
    pub append: bool,
    pub exit: bool,
    pub kill_pane: bool,
    pub pipe_command: Option<String>,
    pub store_buffer: bool,
    pub buffer_prefix: Option<String>,
    pub set_paste: bool,
    pub set_clipboard: bool,
    pub refresh_now: bool,
}

impl CopyModeState {
    pub(crate) fn new(
        parser: &mut Parser,
        keys: CopyModeKeys,
        exit_on_scroll: bool,
        kill_on_exit: bool,
        hide_position: bool,
        wrap_search: bool,
        word_separators: &str,
        raw_output: &[u8],
        history_floor: usize,
    ) -> Self {
        let (history, live) = history_rows(parser);
        let live_row = usize::from(parser.screen().cursor_position().0);
        let live_col = usize::from(parser.screen().cursor_position().1);
        let scroll_offset = 0;
        let cursor = CopyPosition {
            row: history
                .len()
                .saturating_add(live_row)
                .min(history.len().saturating_add(live.len()).saturating_sub(1)),
            col: live_col,
        };
        let mut state = Self {
            cursor,
            cursor_x: 0,
            anchor: None,
            selection_active: false,
            rectangle: false,
            selection_mode: SelectionMode::Char,
            mark: None,
            scroll_offset,
            exit_on_scroll,
            kill_on_exit,
            hide_position,
            refresh_active: false,
            line_numbers: false,
            line_number_mode: CopyLineNumberMode::Off,
            keys,
            wrap_search,
            word_separators: word_separators.to_owned(),
            raw_output: raw_output.to_vec(),
            history_floor,
            last_search: None,
            search_origin: None,
            last_jump: None,
            prompt_history: Vec::new(),
            prompt: None,
            pending_repeat: 0,
            mouse_anchor: None,
        };
        state.update_cursor_x(parser);
        state.sync_parser(parser);
        state
    }

    pub(crate) fn mouse_position(
        &mut self,
        parser: &mut Parser,
        viewport_row: usize,
        viewport_col: usize,
        begin_selection: bool,
        finish_selection: bool,
    ) {
        self.set_mouse_cursor(parser, viewport_row, viewport_col);
        if begin_selection {
            self.anchor = self.mouse_anchor.take().or(Some(self.cursor));
            self.selection_active = true;
        } else if finish_selection {
            self.mouse_anchor = None;
            self.selection_active = false;
        } else if self.anchor.is_some() {
            self.selection_active = true;
        }
        self.update_cursor_x(parser);
        self.sync_parser(parser);
    }

    /// Remember a copy-mode mouse press without exposing a selection until
    /// the terminal sends the first button-motion event.
    pub(crate) fn mouse_down_position(
        &mut self,
        parser: &mut Parser,
        viewport_row: usize,
        viewport_col: usize,
    ) {
        self.set_mouse_cursor(parser, viewport_row, viewport_col);
        self.anchor = None;
        self.selection_active = false;
        self.mouse_anchor = Some(self.cursor);
        self.update_cursor_x(parser);
        self.sync_parser(parser);
    }

    fn set_mouse_cursor(&mut self, parser: &mut Parser, viewport_row: usize, viewport_col: usize) {
        let viewport_start = self.viewport_start(parser);
        let total_rows = self.total_rows(parser);
        let (history, live) = self.history_rows(parser);
        self.cursor.row = viewport_start
            .saturating_add(viewport_row)
            .min(total_rows.saturating_sub(1));
        let line = history
            .into_iter()
            .chain(live)
            .nth(self.cursor.row)
            .unwrap_or_default();
        self.cursor.col = display_column_to_char_index(&line, viewport_col);
    }

    pub(crate) fn execute(
        &mut self,
        parser: &mut Parser,
        action: CopyAction,
        repeat: usize,
    ) -> CopyActionResult {
        // A keyboard action or mouse wheel cancels a press that has not yet
        // produced motion. Otherwise a later, unrelated motion event could
        // accidentally turn that old click into a selection.
        self.mouse_anchor = None;
        let repeat = repeat.max(1);
        let mut result = CopyActionResult::default();
        result.set_paste = true;
        result.set_clipboard = true;
        match action {
            CopyAction::Cancel => {
                result.exit = true;
            }
            CopyAction::HistoryTop => {
                let (history, _) = self.history_rows(parser);
                self.scroll_offset = history.len();
                if self.anchor.is_none() {
                    self.cursor.row = 0;
                    self.cursor.col = 0;
                }
            }
            CopyAction::HistoryBottom => {
                self.scroll_offset = 0;
                if self.anchor.is_none() {
                    self.cursor.row = self.bottom_row(parser);
                    self.cursor.col = self
                        .cursor
                        .col
                        .min(self.line_width(parser, self.cursor.row));
                }
            }
            CopyAction::StartOfLine => self.cursor.col = 0,
            CopyAction::EndOfLine => self.cursor.col = self.line_width(parser, self.cursor.row),
            CopyAction::BeginSelection => {
                self.selection_mode = SelectionMode::Char;
                self.anchor = Some(self.cursor);
                self.selection_active = true;
            }
            CopyAction::SelectLine => {
                self.selection_mode = SelectionMode::Line;
                let start_row = self.cursor.row;
                self.anchor = Some(CopyPosition {
                    row: start_row,
                    col: 0,
                });
                self.cursor.row = start_row
                    .saturating_add(repeat.saturating_sub(1))
                    .min(self.bottom_row(parser));
                self.cursor.col = self.line_width(parser, self.cursor.row);
                self.selection_active = true;
            }
            CopyAction::SelectWord => {
                self.selection_mode = SelectionMode::Word;
                let (history, live) = self.history_rows(parser);
                let lines = history.into_iter().chain(live).collect::<Vec<_>>();
                let start =
                    previous_word(&lines, self.cursor, &self.word_separators, false, self.keys);
                let end = next_word(&lines, start, &self.word_separators, false, true, self.keys);
                self.anchor = Some(start);
                self.cursor = end;
                self.selection_active = true;
            }
            CopyAction::ClearSelection => {
                self.anchor = None;
                self.selection_active = false;
            }
            CopyAction::StopSelection => self.selection_active = false,
            CopyAction::OtherEnd => {
                if !repeat.is_multiple_of(2)
                    && let Some(anchor) = self.anchor
                {
                    if self.selection_active {
                        let cursor = self.cursor;
                        self.cursor = anchor;
                        self.anchor = Some(cursor);
                    } else {
                        // tmux keeps the selection endpoints while stopping
                        // the drag. The first `other-end` resumes the end
                        // endpoint; the next one moves to the opposite end.
                        self.selection_active = true;
                    }
                }
            }
            CopyAction::CursorUp => {
                self.cursor.row = self.cursor.row.saturating_sub(repeat);
                self.keep_cursor_visible(parser);
            }
            CopyAction::CursorDown => {
                self.cursor.row = self
                    .cursor
                    .row
                    .saturating_add(repeat)
                    .min(self.total_rows(parser).saturating_sub(1));
                self.keep_cursor_visible(parser);
            }
            CopyAction::CursorDownAndCancel => {
                let old_row = self.cursor.row;
                self.cursor.row = self
                    .cursor
                    .row
                    .saturating_add(repeat)
                    .min(self.total_rows(parser).saturating_sub(1));
                self.keep_cursor_visible(parser);
                if old_row == self.cursor.row && self.scroll_offset == 0 {
                    result.exit = true;
                }
            }
            CopyAction::CursorLeft => self.move_cursor_left(parser, repeat),
            CopyAction::CursorRight => self.move_cursor_right(parser, repeat),
            CopyAction::ScrollUp => self.scroll_by(parser, repeat, true, &mut result),
            CopyAction::ScrollDown => self.scroll_by(parser, repeat, false, &mut result),
            CopyAction::ScrollTop => self.scroll_cursor_to_viewport(parser, 0),
            CopyAction::ScrollBottom => {
                let rows = usize::from(parser.screen().size().0).saturating_sub(1);
                self.scroll_cursor_to_viewport(parser, rows);
            }
            CopyAction::ScrollMiddle => {
                let rows = usize::from(parser.screen().size().0).saturating_sub(1) / 2;
                self.scroll_cursor_to_viewport(parser, rows);
            }
            CopyAction::ScrollToMouse(Some(row)) => self.scroll_to_mouse(parser, row),
            // A detached command has no mouse coordinate to apply.
            CopyAction::ScrollToMouse(None) => {}
            CopyAction::PageUp => {
                let rows = usize::from(parser.screen().size().0).max(1);
                self.scroll_by(parser, rows.saturating_mul(repeat), true, &mut result);
            }
            CopyAction::PageDown => {
                let rows = usize::from(parser.screen().size().0).max(1);
                self.scroll_by(parser, rows.saturating_mul(repeat), false, &mut result);
            }
            CopyAction::PageDownAndCancel => {
                let rows = usize::from(parser.screen().size().0).max(1);
                self.scroll_by(parser, rows.saturating_mul(repeat), false, &mut result);
                if self.scroll_offset == 0 {
                    result.exit = true;
                }
            }
            CopyAction::RecenterTopBottom => {
                let max = self.max_scroll(parser);
                self.scroll_offset = max.min(self.scroll_offset.saturating_add(1));
                if self.anchor.is_none() {
                    self.keep_cursor_visible(parser);
                }
            }
            CopyAction::CopySelection
            | CopyAction::CopySelectionNoClear
            | CopyAction::CopySelectionAndCancel => {
                result.copied = self.selection(parser);
                if matches!(action, CopyAction::CopySelection) {
                    self.anchor = None;
                    self.selection_active = false;
                }
                if matches!(action, CopyAction::CopySelectionAndCancel) {
                    result.exit = true;
                }
            }
            CopyAction::CopySelectionWithOptions {
                prefix,
                clear,
                cancel,
                set_paste,
                set_clipboard,
            } => {
                result.copied = self.selection(parser);
                result.buffer_prefix = prefix;
                result.set_paste = set_paste;
                result.set_clipboard = set_clipboard;
                if clear {
                    self.anchor = None;
                    self.selection_active = false;
                }
                if cancel {
                    result.exit = true;
                }
            }
            CopyAction::CopyEndOfLine | CopyAction::CopyEndOfLineAndCancel => {
                result.copied = self.copy_line_range(parser, false);
                if matches!(action, CopyAction::CopyEndOfLineAndCancel) {
                    result.exit = true;
                }
            }
            CopyAction::CopyLine | CopyAction::CopyLineAndCancel => {
                result.copied = self.copy_line_range(parser, true);
                if matches!(action, CopyAction::CopyLineAndCancel) {
                    result.exit = true;
                }
            }
            CopyAction::CopyLineWithOptions {
                prefix,
                whole_line,
                cancel,
                set_paste,
                set_clipboard,
            } => {
                result.copied = self.copy_line_range(parser, whole_line);
                result.buffer_prefix = prefix;
                result.set_paste = set_paste;
                result.set_clipboard = set_clipboard;
                if cancel {
                    result.exit = true;
                }
            }
            CopyAction::CopyPipe {
                command,
                clear,
                cancel,
                store,
            } => {
                result.copied = self.selection(parser);
                result.pipe_command = Some(command);
                result.store_buffer = store;
                if clear {
                    self.anchor = None;
                    self.selection_active = false;
                }
                if cancel {
                    result.exit = true;
                }
            }
            CopyAction::CopyPipeWithOptions {
                command,
                prefix,
                clear,
                cancel,
                store,
                set_paste,
                set_clipboard,
            } => {
                result.copied = self.selection(parser);
                result.pipe_command = Some(command);
                result.buffer_prefix = prefix;
                result.store_buffer = store;
                result.set_paste = set_paste;
                result.set_clipboard = set_clipboard;
                if clear {
                    self.anchor = None;
                    self.selection_active = false;
                }
                if cancel {
                    result.exit = true;
                }
            }
            CopyAction::CopyPipeEndOfLine { command, cancel } => {
                result.copied = self.copy_line_range(parser, false);
                result.pipe_command = Some(command);
                result.store_buffer = true;
                if cancel {
                    result.exit = true;
                }
            }
            CopyAction::CopyPipeLine { command, cancel } => {
                result.copied = self.copy_line_range(parser, true);
                result.pipe_command = Some(command);
                result.store_buffer = true;
                if cancel {
                    result.exit = true;
                }
            }
            CopyAction::CopyPipeLineWithOptions {
                command,
                prefix,
                whole_line,
                cancel,
                set_paste,
                set_clipboard,
            } => {
                result.copied = self.copy_line_range(parser, whole_line);
                result.pipe_command = Some(command);
                result.buffer_prefix = prefix;
                result.store_buffer = true;
                result.set_paste = set_paste;
                result.set_clipboard = set_clipboard;
                if cancel {
                    result.exit = true;
                }
            }
            CopyAction::AppendSelection | CopyAction::AppendSelectionAndCancel => {
                result.copied = self.selection(parser);
                result.append = true;
                self.anchor = None;
                self.selection_active = false;
                if matches!(action, CopyAction::AppendSelectionAndCancel) {
                    result.exit = true;
                }
            }
            CopyAction::RectangleToggle => self.rectangle = !self.rectangle,
            CopyAction::RectangleOn => self.rectangle = true,
            CopyAction::RectangleOff => self.rectangle = false,
            CopyAction::SelectionMode(mode) => self.selection_mode = mode,
            CopyAction::TopLine => self.move_to_viewport_line(parser, 0),
            CopyAction::MiddleLine => {
                let rows = usize::from(parser.screen().size().0);
                self.move_to_viewport_line(parser, rows / 2);
            }
            CopyAction::BottomLine => {
                let rows = usize::from(parser.screen().size().0);
                self.move_to_viewport_line(parser, rows.saturating_sub(1));
            }
            CopyAction::CursorCentreVertical => {
                let rows = usize::from(parser.screen().size().0).max(1);
                self.move_to_viewport_line(parser, rows / 2);
            }
            CopyAction::HalfPageUp => {
                let rows = usize::from(parser.screen().size().0).max(1) / 2;
                self.scroll_by(
                    parser,
                    rows.max(1).saturating_mul(repeat),
                    true,
                    &mut result,
                );
            }
            CopyAction::HalfPageDown => {
                let rows = usize::from(parser.screen().size().0).max(1) / 2;
                self.scroll_by(
                    parser,
                    rows.max(1).saturating_mul(repeat),
                    false,
                    &mut result,
                );
            }
            CopyAction::HalfPageDownAndCancel => {
                let rows = usize::from(parser.screen().size().0).max(1) / 2;
                self.scroll_by(
                    parser,
                    rows.max(1).saturating_mul(repeat),
                    false,
                    &mut result,
                );
                if self.scroll_offset == 0 {
                    result.exit = true;
                }
            }
            CopyAction::BackToIndentation => {
                let (history, live) = self.history_rows(parser);
                if let Some(line) = history.into_iter().chain(live).nth(self.cursor.row) {
                    self.cursor.col = line
                        .chars()
                        .position(|character| !character.is_whitespace())
                        .unwrap_or(0);
                }
            }
            CopyAction::CursorCentreHorizontal => {
                let width = self.line_width(parser, self.cursor.row);
                let cols = usize::from(parser.screen().size().1);
                self.cursor.col = self.cursor.col.saturating_sub(cols / 2).min(width);
            }
            CopyAction::ScrollDownAndCancel => {
                self.scroll_by(parser, repeat, false, &mut result);
                if self.scroll_offset == 0 {
                    result.exit = true;
                }
            }
            CopyAction::ScrollExitOn => self.exit_on_scroll = true,
            CopyAction::ScrollExitOff => self.exit_on_scroll = false,
            CopyAction::ScrollExitToggle => self.exit_on_scroll = !self.exit_on_scroll,
            CopyAction::SetMark => self.mark = Some(self.cursor),
            CopyAction::JumpToMark => {
                if let Some(mark) = self.mark {
                    self.cursor = mark;
                    self.keep_cursor_visible(parser);
                }
            }
            CopyAction::SearchForward(search) => {
                for _ in 0..repeat {
                    self.search(parser, search.clone(), true, true);
                }
                self.search_origin = None;
            }
            CopyAction::SearchForwardText(search) => {
                for _ in 0..repeat {
                    self.search(parser, search.clone(), true, false);
                }
                self.search_origin = None;
            }
            CopyAction::SearchBackward(search) => {
                for _ in 0..repeat {
                    self.search(parser, search.clone(), false, true);
                }
                self.search_origin = None;
            }
            CopyAction::SearchBackwardText(search) => {
                for _ in 0..repeat {
                    self.search(parser, search.clone(), false, false);
                }
                self.search_origin = None;
            }
            CopyAction::SearchForwardIncremental(search) => {
                self.search_incremental(parser, search, true)
            }
            CopyAction::SearchBackwardIncremental(search) => {
                self.search_incremental(parser, search, false)
            }
            CopyAction::SearchAgain => {
                if let Some((search, forward, regex)) = self.last_search.clone() {
                    for _ in 0..repeat {
                        self.search(parser, search.clone(), forward, regex);
                    }
                }
            }
            CopyAction::SearchReverse => {
                if let Some((search, forward, regex)) = self.last_search.clone() {
                    for _ in 0..repeat {
                        self.search(parser, search.clone(), !forward, regex);
                        // tmux keeps searchtype as the original direction;
                        // search-reverse changes only this invocation.
                        self.last_search = Some((search.clone(), forward, regex));
                    }
                }
            }
            CopyAction::JumpForward(ref character)
            | CopyAction::JumpBackward(ref character)
            | CopyAction::JumpToForward(ref character)
            | CopyAction::JumpToBackward(ref character) => {
                let forward = matches!(
                    action,
                    CopyAction::JumpForward(_) | CopyAction::JumpToForward(_)
                );
                let to = matches!(
                    action,
                    CopyAction::JumpToForward(_) | CopyAction::JumpToBackward(_)
                );
                self.last_jump = Some((character.clone(), forward, to));
                for _ in 0..repeat {
                    self.jump(parser, character, &action);
                }
            }
            CopyAction::JumpAgain => {
                if let Some((character, forward, to)) = self.last_jump.clone() {
                    let action = if forward {
                        if to {
                            CopyAction::JumpToForward(character.clone())
                        } else {
                            CopyAction::JumpForward(character.clone())
                        }
                    } else if to {
                        CopyAction::JumpToBackward(character.clone())
                    } else {
                        CopyAction::JumpBackward(character.clone())
                    };
                    for _ in 0..repeat {
                        self.jump(parser, &character, &action);
                    }
                }
            }
            CopyAction::JumpReverse => {
                if let Some((character, forward, to)) = self.last_jump.clone() {
                    let action = if !forward {
                        if to {
                            CopyAction::JumpToForward(character.clone())
                        } else {
                            CopyAction::JumpForward(character.clone())
                        }
                    } else if to {
                        CopyAction::JumpToBackward(character.clone())
                    } else {
                        CopyAction::JumpBackward(character.clone())
                    };
                    self.last_jump = Some((character.clone(), !forward, to));
                    for _ in 0..repeat {
                        self.jump(parser, &character, &action);
                    }
                }
            }
            CopyAction::PreviousParagraph => {
                for _ in 0..repeat {
                    self.move_paragraph(parser, false);
                }
            }
            CopyAction::NextParagraph => {
                for _ in 0..repeat {
                    self.move_paragraph(parser, true);
                }
            }
            CopyAction::PreviousMatchingBracket => {
                for _ in 0..repeat {
                    self.match_bracket(parser, false);
                }
            }
            CopyAction::NextMatchingBracket => {
                for _ in 0..repeat {
                    self.match_bracket(parser, true);
                }
            }
            CopyAction::NextPrompt => self.move_prompt(parser, true),
            CopyAction::PreviousPrompt => self.move_prompt(parser, false),
            CopyAction::RefreshFromPane
            | CopyAction::RefreshOn
            | CopyAction::RefreshOff
            | CopyAction::RefreshToggle => match action {
                CopyAction::RefreshFromPane => result.refresh_now = true,
                CopyAction::RefreshOn => self.refresh_active = true,
                CopyAction::RefreshOff => self.refresh_active = false,
                CopyAction::RefreshToggle => self.refresh_active = !self.refresh_active,
                _ => unreachable!(),
            },
            CopyAction::TogglePosition => self.hide_position = !self.hide_position,
            CopyAction::LineNumbersOn => {
                self.line_numbers = true;
                if self.line_number_mode == CopyLineNumberMode::Off {
                    self.line_number_mode = CopyLineNumberMode::Default;
                }
            }
            CopyAction::LineNumbersOff => self.line_numbers = false,
            CopyAction::LineNumbersToggle => {
                self.line_numbers = !self.line_numbers;
                if self.line_numbers && self.line_number_mode == CopyLineNumberMode::Off {
                    self.line_number_mode = CopyLineNumberMode::Default;
                }
            }
            CopyAction::GotoLine(line) => {
                self.cursor.row = line.saturating_sub(1).min(self.bottom_row(parser));
                self.cursor.col = self
                    .cursor
                    .col
                    .min(self.line_width(parser, self.cursor.row));
                self.keep_cursor_visible(parser);
            }
            CopyAction::NextWord
            | CopyAction::NextWordEnd
            | CopyAction::PreviousWord
            | CopyAction::PreviousSpace
            | CopyAction::NextSpace
            | CopyAction::NextSpaceEnd => self.move_word(parser, action, repeat),
        }
        self.update_cursor_x(parser);
        self.sync_parser(parser);
        if result.exit {
            result.kill_pane = self.kill_on_exit;
            parser.screen_mut().set_scrollback(0);
        }
        result
    }

    pub(crate) fn pane_mode(&self) -> &'static str {
        "copy-mode"
    }

    pub(crate) fn selection_present(&self) -> bool {
        self.anchor.is_some()
    }

    pub(crate) fn selection_is_active(&self) -> bool {
        self.selection_active
    }

    pub(crate) fn selection_mode_name(&self) -> &'static str {
        match self.selection_mode {
            SelectionMode::Char => "char",
            SelectionMode::Word => "word",
            SelectionMode::Line => "line",
        }
    }

    pub(crate) fn scroll_position(&self) -> usize {
        self.scroll_offset
    }

    /// Classify a row on the visible scrollbar track. The track is derived
    /// from retained history and viewport state rather than rendered styling,
    /// so mouse interaction remains testable without a terminal.
    pub(crate) fn scrollbar_hit(&self, parser: &mut Parser, row: usize) -> CopyScrollbarHit {
        let viewport_rows = usize::from(parser.screen().size().0).max(1);
        let total_rows = self.total_rows(parser);
        let max_scroll = self.max_scroll(parser);
        scrollbar_hit_for(
            total_rows,
            viewport_rows,
            max_scroll,
            self.scroll_offset,
            row,
        )
    }

    pub(crate) fn scrollbar_slider_offset(&self, parser: &mut Parser, row: usize) -> Option<usize> {
        let viewport_rows = usize::from(parser.screen().size().0).max(1);
        let total_rows = self.total_rows(parser);
        let max_scroll = self.max_scroll(parser);
        let (slider_top, slider_rows) =
            scrollbar_geometry_for(total_rows, viewport_rows, max_scroll, self.scroll_offset);
        (row >= slider_top && row < slider_top.saturating_add(slider_rows))
            .then_some(row.saturating_sub(slider_top))
    }

    pub(crate) fn position_limit(&mut self, parser: &mut Parser) -> usize {
        self.history_rows(parser).0.len()
    }

    pub(crate) fn rectangle_selection(&self) -> bool {
        self.rectangle
    }

    pub(crate) fn word_separators(&self) -> &str {
        &self.word_separators
    }

    pub(crate) fn line_numbers(&self) -> bool {
        self.line_numbers
    }

    pub(crate) fn refresh_active(&self) -> bool {
        self.refresh_active
    }

    /// Reconcile copy mode with the pane's current terminal snapshot after a
    /// `refresh-from-pane` action. The parser already contains the latest PTY
    /// output; this updates the raw stream used for tab-cell reconstruction
    /// and keeps the logical cursor inside the new history bounds.
    pub(crate) fn refresh_now(&mut self, parser: &mut Parser, raw_output: &[u8]) {
        self.raw_output.clear();
        self.raw_output.extend_from_slice(raw_output);
        if self.scroll_offset == 0 && !self.selection_present() {
            self.cursor.row = self.bottom_row(parser);
            self.cursor.col = self.line_width(parser, self.cursor.row);
        } else {
            self.clamp_to_parser(parser);
        }
        self.sync_parser(parser);
    }

    /// Apply an output-driven refresh while automatic refresh is enabled.
    /// Selections remain stable, while a view at the live edge follows new
    /// output in the same way tmux's refresh timer does.
    pub(crate) fn refresh_live(&mut self, parser: &mut Parser, raw_output: &[u8]) {
        if !self.refresh_active || self.selection_present() {
            return;
        }
        self.raw_output.clear();
        self.raw_output.extend_from_slice(raw_output);
        if self.scroll_offset == 0 {
            self.cursor.row = self.bottom_row(parser);
            self.cursor.col = self.line_width(parser, self.cursor.row);
        } else {
            self.clamp_to_parser(parser);
        }
        self.sync_parser(parser);
    }

    fn clamp_to_parser(&mut self, parser: &mut Parser) {
        let total_rows = self.total_rows(parser);
        if total_rows == 0 {
            self.cursor = CopyPosition { row: 0, col: 0 };
            self.scroll_offset = 0;
            return;
        }
        self.cursor.row = self.cursor.row.min(total_rows.saturating_sub(1));
        self.cursor.col = self
            .cursor
            .col
            .min(self.line_width(parser, self.cursor.row));
        self.scroll_offset = self.scroll_offset.min(self.max_scroll(parser));
    }

    pub(crate) fn set_line_number_mode(&mut self, mode: CopyLineNumberMode) {
        self.line_number_mode = mode;
        self.line_numbers = mode != CopyLineNumberMode::Off;
    }

    pub(crate) fn line_number_value(&self, row: usize) -> usize {
        match self.line_number_mode {
            CopyLineNumberMode::Off | CopyLineNumberMode::Default => row.saturating_add(1),
            CopyLineNumberMode::Absolute => row.saturating_add(1),
            CopyLineNumberMode::Relative => row.abs_diff(self.cursor.row),
            CopyLineNumberMode::Hybrid => {
                if row == self.cursor.row {
                    row.saturating_add(1)
                } else {
                    row.abs_diff(self.cursor.row)
                }
            }
        }
    }

    /// Return the cursor location relative to the viewport currently exposed
    /// by the parser while copy mode is active. The copy cursor is stored in
    /// logical history coordinates; attached rendering needs screen-relative
    /// coordinates after `sync_parser` has selected the scrollback offset.
    pub(crate) fn cursor_viewport(&self, parser: &mut Parser) -> (usize, usize) {
        let viewport_start = self.viewport_start(parser);
        let viewport_rows = usize::from(parser.screen().size().0).max(1);
        let row = self
            .cursor
            .row
            .saturating_sub(viewport_start)
            .min(viewport_rows.saturating_sub(1));
        (row, self.cursor_x)
    }

    pub(crate) fn viewport_start(&self, parser: &mut Parser) -> usize {
        let (history, live) = self.history_rows(parser);
        let total_rows = history.len().saturating_add(live.len()).max(1);
        let viewport_rows = usize::from(parser.screen().size().0).max(1);
        total_rows
            .saturating_sub(viewport_rows)
            .saturating_sub(self.scroll_offset)
    }

    /// Whether a logical copy-mode cell is inside the retained selection.
    /// Rendering uses logical rows because the parser viewport can be moved
    /// independently of the selection endpoints.
    pub(crate) fn cell_selected(&self, row: usize, col: usize) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };
        let (start, end) = if anchor.row < self.cursor.row
            || (anchor.row == self.cursor.row && anchor.col <= self.cursor.col)
        {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        };
        if row < start.row || row > end.row {
            return false;
        }
        match self.selection_mode {
            SelectionMode::Line => true,
            SelectionMode::Word | SelectionMode::Char if self.rectangle => {
                col >= start.col.min(end.col) && col <= start.col.max(end.col)
            }
            SelectionMode::Word | SelectionMode::Char => {
                if start.row == end.row {
                    col >= start.col && col <= end.col
                } else if row == start.row {
                    col >= start.col
                } else if row == end.row {
                    col <= end.col
                } else {
                    true
                }
            }
        }
    }

    pub(crate) fn begin_prompt(&mut self, kind: CopyPromptKind) {
        self.pending_repeat = 0;
        let incremental_search = matches!(
            kind,
            CopyPromptKind::SearchForwardIncremental | CopyPromptKind::SearchBackwardIncremental
        );
        self.search_origin = matches!(
            kind,
            CopyPromptKind::SearchForward
                | CopyPromptKind::SearchBackward
                | CopyPromptKind::SearchForwardIncremental
                | CopyPromptKind::SearchBackwardIncremental
        )
        .then_some(self.cursor);
        let input = if incremental_search {
            {
                self.last_search
                    .as_ref()
                    .map_or_else(Vec::new, |(search, _, _)| search.as_bytes().to_vec())
            }
        } else {
            Default::default()
        };
        self.prompt = Some(CopyPrompt {
            kind,
            cursor: input.len(),
            input,
            history_index: self.prompt_history.len(),
            quoted: false,
            yank_buffer: Vec::new(),
        });
    }

    pub(crate) fn set_prompt_history(&mut self, history: &[Vec<u8>]) {
        self.prompt_history = history.to_vec();
    }

    pub(crate) fn prompt_history(&self) -> &[Vec<u8>] {
        &self.prompt_history
    }

    pub(crate) fn prompt_display(&self) -> Option<String> {
        let prompt = self.prompt.as_ref()?;
        let label = match prompt.kind {
            CopyPromptKind::SearchForward
            | CopyPromptKind::SearchBackward
            | CopyPromptKind::SearchForwardIncremental
            | CopyPromptKind::SearchBackwardIncremental => "(search)",
            CopyPromptKind::GotoLine => "(goto line)",
            CopyPromptKind::JumpForward => "(jump forward)",
            CopyPromptKind::JumpBackward => "(jump backward)",
            CopyPromptKind::JumpToForward => "(jump to forward)",
            CopyPromptKind::JumpToBackward => "(jump to backward)",
        };
        Some(format!("{label} {}", display_prompt_input(&prompt.input)))
    }

    fn record_prompt_history(&mut self, input: &[u8]) {
        if input.is_empty() || self.prompt_history.last().is_some_and(|last| last == input) {
            return;
        }
        self.prompt_history.push(input.to_vec());
        const PROMPT_HISTORY_LIMIT: usize = 100;
        if self.prompt_history.len() > PROMPT_HISTORY_LIMIT {
            self.prompt_history.remove(0);
        }
    }

    pub(crate) fn feed_repeat_digit(&mut self, byte: u8) -> bool {
        if byte == b'0' {
            if self.pending_repeat == 0 {
                return false;
            }
        } else if !(b'1'..=b'9').contains(&byte) {
            return false;
        }
        self.pending_repeat = self
            .pending_repeat
            .saturating_mul(10)
            .saturating_add(usize::from(byte - b'0'));
        true
    }

    pub(crate) fn take_repeat(&mut self) -> usize {
        std::mem::take(&mut self.pending_repeat).max(1)
    }

    fn incremental_search_action(kind: CopyPromptKind, input: &[u8]) -> Option<CopyAction> {
        let input = String::from_utf8_lossy(input).into_owned();
        match kind {
            CopyPromptKind::SearchForwardIncremental => {
                Some(CopyAction::SearchForwardIncremental(input))
            }
            CopyPromptKind::SearchBackwardIncremental => {
                Some(CopyAction::SearchBackwardIncremental(input))
            }
            _ => None,
        }
    }

    /// Consume bytes belonging to an attached-client command prompt. The
    /// outer server keeps calling this method for each input packet, so a
    /// search entered over multiple `send-keys` requests has the same state
    /// transition as a real tmux command prompt.
    pub(crate) fn feed_prompt(&mut self, bytes: &[u8]) -> Option<(Option<CopyAction>, usize)> {
        let mut prompt = self.prompt.take()?;
        let history = self.prompt_history.clone();
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            match prompt.kind {
                CopyPromptKind::JumpForward
                | CopyPromptKind::JumpBackward
                | CopyPromptKind::JumpToForward
                | CopyPromptKind::JumpToBackward => {
                    if byte == 0x1b {
                        return Some((None, index + 1));
                    }
                    prompt.input.push(byte);
                    let Ok(input) = std::str::from_utf8(&prompt.input) else {
                        index += 1;
                        continue;
                    };
                    let Some(character) = input.chars().next() else {
                        index += 1;
                        continue;
                    };
                    let kind = prompt.kind;
                    let action = match kind {
                        CopyPromptKind::JumpForward => {
                            CopyAction::JumpForward(character.to_string())
                        }
                        CopyPromptKind::JumpBackward => {
                            CopyAction::JumpBackward(character.to_string())
                        }
                        CopyPromptKind::JumpToForward => {
                            CopyAction::JumpToForward(character.to_string())
                        }
                        CopyPromptKind::JumpToBackward => {
                            CopyAction::JumpToBackward(character.to_string())
                        }
                        _ => unreachable!(),
                    };
                    return Some((Some(action), index + 1));
                }
                _ => {
                    let remaining = &bytes[index..];
                    if prompt.quoted {
                        prompt.quoted = false;
                        prompt.insert(byte);
                        index += 1;
                        continue;
                    }
                    let escape = if remaining.starts_with(b"\x1b[D") {
                        Some((3, 0))
                    } else if remaining.starts_with(b"\x1b[C") {
                        Some((3, 1))
                    } else if remaining.starts_with(b"\x1b[1~") {
                        Some((4, 2))
                    } else if remaining.starts_with(b"\x1b[H") || remaining.starts_with(b"\x1bOH") {
                        Some((3, 2))
                    } else if remaining.starts_with(b"\x1b[4~") {
                        Some((4, 3))
                    } else if remaining.starts_with(b"\x1b[F") || remaining.starts_with(b"\x1bOF") {
                        Some((3, 3))
                    } else if remaining.starts_with(b"\x1b[A") {
                        Some((3, 5))
                    } else if remaining.starts_with(b"\x1b[B") {
                        Some((3, 6))
                    } else if remaining.starts_with(b"\x1b[3~") {
                        Some((4, 4))
                    } else {
                        None
                    };
                    if let Some((consumed, action)) = escape {
                        match action {
                            0 => prompt.move_left(),
                            1 => prompt.move_right(),
                            2 => prompt.move_start(),
                            3 => prompt.move_end(),
                            4 => prompt.delete(),
                            5 => prompt.history_up(&history),
                            6 => prompt.history_down(&history),
                            _ => unreachable!(),
                        }
                        index += consumed;
                        let action = Self::incremental_search_action(prompt.kind, &prompt.input);
                        self.prompt = Some(prompt);
                        return Some((action, index));
                    }
                    if byte == 0x1b {
                        return Some((
                            Self::incremental_search_action(prompt.kind, &[]),
                            index + 1,
                        ));
                    }
                    if byte == b'\r' || byte == b'\n' {
                        let kind = prompt.kind;
                        let input_bytes = std::mem::take(&mut prompt.input);
                        self.record_prompt_history(&input_bytes);
                        let input = String::from_utf8_lossy(&input_bytes).into_owned();
                        let action = match kind {
                            CopyPromptKind::SearchForward => CopyAction::SearchForward(input),
                            CopyPromptKind::SearchForwardIncremental => {
                                CopyAction::SearchForwardText(input)
                            }
                            CopyPromptKind::SearchBackward => CopyAction::SearchBackward(input),
                            CopyPromptKind::SearchBackwardIncremental => {
                                CopyAction::SearchBackwardText(input)
                            }
                            CopyPromptKind::GotoLine => {
                                CopyAction::GotoLine(input.parse::<usize>().unwrap_or(0))
                            }
                            _ => unreachable!(),
                        };
                        return Some((Some(action), index + 1));
                    }
                    match byte {
                        0x01 => prompt.move_start(),
                        0x02 => prompt.move_left(),
                        0x04 => prompt.delete(),
                        0x05 => prompt.move_end(),
                        0x06 => prompt.move_right(),
                        0x08 | 0x7f => prompt.backspace(),
                        0x0b => prompt.yank_buffer = prompt.kill_to_end(),
                        0x15 => prompt.yank_buffer = prompt.kill_to_start(),
                        0x16 => prompt.quoted = true,
                        0x17 => prompt.yank_buffer = prompt.kill_word(),
                        0x19 => prompt.yank(),
                        _ => prompt.insert(byte),
                    }
                }
            }
            index += 1;
        }
        let action = Self::incremental_search_action(prompt.kind, &prompt.input);
        self.prompt = Some(prompt);
        Some((action, bytes.len()))
    }

    /// Return the word under the copy cursor using the same separator class
    /// that drives word motion. This is exposed by tmux as
    /// `copy_cursor_word` and consumed by the Vi `#` and `*` bindings.
    pub(crate) fn cursor_word(&self, parser: &mut Parser) -> String {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        let Some(line) = lines.get(self.cursor.row) else {
            return String::new();
        };
        let chars = line.chars().collect::<Vec<_>>();
        let Some(&character) = chars.get(self.cursor.col.min(chars.len().saturating_sub(1))) else {
            return String::new();
        };
        if character.is_whitespace() {
            return String::new();
        }
        let separator = is_separator(character, &self.word_separators);
        let mut start = self.cursor.col.min(chars.len().saturating_sub(1));
        while start > 0 {
            let previous = chars[start - 1];
            if previous.is_whitespace()
                || is_separator(previous, &self.word_separators) != separator
            {
                break;
            }
            start -= 1;
        }
        let mut end = start;
        while end + 1 < chars.len() {
            let next = chars[end + 1];
            if next.is_whitespace() || is_separator(next, &self.word_separators) != separator {
                break;
            }
            end += 1;
        }
        chars[start..=end].iter().collect()
    }

    /// Return the logical line containing the copy cursor. tmux exposes this
    /// as `copy_cursor_line` so bindings and position formats can inspect the
    /// current mode context without scraping the rendered terminal.
    pub(crate) fn cursor_line(&self, parser: &mut Parser) -> String {
        let (history, live) = self.history_rows(parser);
        history
            .into_iter()
            .chain(live)
            .nth(self.cursor.row)
            .unwrap_or_default()
    }

    fn total_rows(&self, parser: &mut Parser) -> usize {
        let (history, live) = self.history_rows(parser);
        history.len().saturating_add(live.len()).max(1)
    }

    fn bottom_row(&self, parser: &mut Parser) -> usize {
        self.total_rows(parser).saturating_sub(1)
    }

    fn max_scroll(&self, parser: &mut Parser) -> usize {
        let (history, _) = self.history_rows(parser);
        history.len()
    }

    fn scroll_by(
        &mut self,
        parser: &mut Parser,
        amount: usize,
        up: bool,
        result: &mut CopyActionResult,
    ) {
        let old = self.scroll_offset;
        let new = if up {
            old.saturating_add(amount).min(self.max_scroll(parser))
        } else {
            old.saturating_sub(amount)
        };
        let moved = if up {
            new.saturating_sub(old)
        } else {
            old.saturating_sub(new)
        };
        self.scroll_offset = new;
        if self.anchor.is_none() && moved > 0 {
            if up {
                self.cursor.row = self.cursor.row.saturating_sub(moved);
            } else {
                self.cursor.row = self
                    .cursor
                    .row
                    .saturating_add(moved)
                    .min(self.bottom_row(parser));
            }
        }
        if !up && new == 0 && self.exit_on_scroll && self.anchor.is_none() {
            result.exit = true;
        }
    }

    fn scroll_to_mouse(&mut self, parser: &mut Parser, row: usize) {
        let viewport_rows = usize::from(parser.screen().size().0).max(1);
        let total_rows = self.total_rows(parser);
        let max_scroll = self.max_scroll(parser);
        if max_scroll == 0 {
            return;
        }
        let (_, slider_rows) =
            scrollbar_geometry_for(total_rows, viewport_rows, max_scroll, self.scroll_offset);
        let slider_top = row.min(viewport_rows.saturating_sub(slider_rows));
        let offset_from_top = slider_top
            .saturating_mul(total_rows)
            .checked_div(viewport_rows)
            .unwrap_or(max_scroll)
            .min(max_scroll);
        let target = max_scroll.saturating_sub(offset_from_top);
        let old = self.scroll_offset;
        self.scroll_offset = target;
        if self.anchor.is_none() {
            if target > old {
                self.cursor.row = self.cursor.row.saturating_sub(target - old);
            } else {
                self.cursor.row = self
                    .cursor
                    .row
                    .saturating_add(old - target)
                    .min(self.bottom_row(parser));
            }
        }
    }

    fn scroll_cursor_to_viewport(&mut self, parser: &mut Parser, line: usize) {
        let rows = usize::from(parser.screen().size().0).max(1);
        let total = self.total_rows(parser);
        let max_start = total.saturating_sub(rows);
        let desired_start = self.cursor.row.saturating_sub(line.min(rows - 1));
        let start = desired_start.min(max_start);
        self.scroll_offset = total
            .saturating_sub(rows.saturating_add(start))
            .min(self.max_scroll(parser));
    }

    fn copy_line_range(&mut self, parser: &mut Parser, whole_line: bool) -> Option<String> {
        let saved_cursor = self.cursor;
        let row = self.cursor.row;
        let width = self.line_width(parser, row);
        self.anchor = Some(CopyPosition {
            row,
            col: if whole_line { 0 } else { self.cursor.col },
        });
        self.cursor.col = width;
        self.selection_mode = SelectionMode::Char;
        self.selection_active = true;
        let copied = self.selection(parser);
        self.cursor = saved_cursor;
        self.anchor = None;
        self.selection_active = false;
        self.selection_mode = SelectionMode::Char;
        copied
    }

    fn jump(&mut self, parser: &mut Parser, needle: &str, action: &CopyAction) {
        let Some(character) = needle.chars().next() else {
            return;
        };
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        let forward = matches!(
            action,
            CopyAction::JumpForward(_) | CopyAction::JumpToForward(_)
        );
        let to = matches!(
            action,
            CopyAction::JumpToForward(_) | CopyAction::JumpToBackward(_)
        );
        let found = if forward {
            (self.cursor.row..lines.len()).find_map(|row| {
                let start = if row == self.cursor.row {
                    self.cursor.col.saturating_add(1)
                } else {
                    0
                };
                lines[row]
                    .chars()
                    .enumerate()
                    .skip(start)
                    .find(|(_, value)| *value == character)
                    .map(|(col, _)| (row, col))
            })
        } else {
            (0..=self.cursor.row).rev().find_map(|row| {
                let end = if row == self.cursor.row {
                    self.cursor.col
                } else {
                    lines[row].chars().count()
                };
                lines[row]
                    .chars()
                    .enumerate()
                    .take(end)
                    .filter(|(_, value)| *value == character)
                    .last()
                    .map(|(col, _)| (row, col))
            })
        };
        if let Some((row, col)) = found {
            self.cursor = CopyPosition {
                row,
                col: if to {
                    if forward {
                        col.saturating_sub(1)
                    } else {
                        col.saturating_add(1)
                    }
                } else {
                    col
                },
            };
            self.keep_cursor_visible(parser);
        }
    }

    fn move_paragraph(&mut self, parser: &mut Parser, next: bool) {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        let is_blank = |line: &str| line.trim().is_empty();
        if next {
            let mut row = self.cursor.row;
            while row + 1 < lines.len() && is_blank(&lines[row]) {
                row += 1;
            }
            while row + 1 < lines.len() && !is_blank(&lines[row]) {
                row += 1;
            }
            self.cursor.row = row;
            self.cursor.col = self.line_width(parser, row);
        } else {
            let mut row = self.cursor.row;
            while row > 0 && is_blank(&lines[row]) {
                row -= 1;
            }
            while row > 0 && !is_blank(&lines[row]) {
                row -= 1;
            }
            self.cursor.row = row;
            self.cursor.col = 0;
        }
        self.keep_cursor_visible(parser);
    }

    fn match_bracket(&mut self, parser: &mut Parser, next: bool) {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
        if next && self.keys == CopyModeKeys::Vi {
            let on_closing_bracket = lines
                .get(self.cursor.row)
                .and_then(|line| line.chars().nth(self.cursor.col))
                .is_some_and(|character| pairs.iter().any(|(_, close)| *close == character));
            if on_closing_bracket {
                self.match_bracket(parser, false);
                return;
            }
        }
        let Some((row, col, open, close, mut depth)) = (if next {
            lines.get(self.cursor.row).and_then(|line| {
                line.chars()
                    .enumerate()
                    .skip(self.cursor.col)
                    .find_map(|(col, value)| {
                        pairs.iter().find_map(|(open, close)| {
                            (value == *open).then_some((self.cursor.row, col, *open, *close, 1))
                        })
                    })
            })
        } else {
            lines.get(self.cursor.row).and_then(|line| {
                line.chars()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .enumerate()
                    .take(self.cursor.col.saturating_add(1))
                    .rev()
                    .find_map(|(col, value)| {
                        pairs.iter().find_map(|(open, close)| {
                            (value == *close).then_some((self.cursor.row, col, *open, *close, 1))
                        })
                    })
            })
        }) else {
            return;
        };
        if next {
            for row_index in row..lines.len() {
                let start = if row_index == row { col + 1 } else { 0 };
                for (column, value) in lines[row_index].chars().enumerate().skip(start) {
                    if value == open {
                        depth += 1;
                    } else if value == close {
                        depth -= 1;
                        if depth == 0 {
                            self.cursor = CopyPosition {
                                row: row_index,
                                col: column,
                            };
                            self.keep_cursor_visible(parser);
                            return;
                        }
                    }
                }
            }
        } else {
            for row_index in (0..=row).rev() {
                let end = if row_index == row {
                    col
                } else {
                    lines[row_index].chars().count()
                };
                for (column, value) in lines[row_index]
                    .chars()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .enumerate()
                    .take(end)
                    .rev()
                {
                    if value == close {
                        depth += 1;
                    } else if value == open {
                        depth -= 1;
                        if depth == 0 {
                            self.cursor = CopyPosition {
                                row: row_index,
                                col: column,
                            };
                            self.keep_cursor_visible(parser);
                            return;
                        }
                    }
                }
            }
        }
    }

    fn move_prompt(&mut self, parser: &mut Parser, next: bool) {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        let is_prompt = |line: &str| {
            let trimmed = line.trim_start();
            trimmed.starts_with("$ ")
                || trimmed.starts_with("# ")
                || trimmed.starts_with("> ")
                || trimmed.ends_with("$ ")
                || trimmed.ends_with("# ")
                || trimmed.ends_with("> ")
        };
        let found = if next {
            ((self.cursor.row.saturating_add(1))..lines.len()).find(|row| is_prompt(&lines[*row]))
        } else {
            (0..self.cursor.row)
                .rev()
                .find(|row| is_prompt(&lines[*row]))
        };
        if let Some(row) = found {
            self.cursor.row = row;
            self.cursor.col = 0;
            self.keep_cursor_visible(parser);
        }
    }

    fn line_width(&self, parser: &mut Parser, row: usize) -> usize {
        let (history, live) = self.history_rows(parser);
        history
            .iter()
            .chain(live.iter())
            .nth(row)
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }

    fn move_cursor_left(&mut self, parser: &mut Parser, repeat: usize) {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        for _ in 0..repeat {
            if self.cursor.col > 0 {
                self.cursor.col -= 1;
            } else if self.cursor.row > 0 {
                self.cursor.row -= 1;
                self.cursor.col = lines[self.cursor.row].chars().count().saturating_sub(1);
            }
        }
    }

    fn move_cursor_right(&mut self, parser: &mut Parser, repeat: usize) {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        for _ in 0..repeat {
            let width = lines[self.cursor.row].chars().count();
            if self.cursor.col + 1 < width {
                self.cursor.col += 1;
            } else if self.cursor.row + 1 < lines.len() {
                self.cursor.row += 1;
                self.cursor.col = 0;
            } else if width > 0 {
                self.cursor.col = width - 1;
            }
        }
        self.keep_cursor_visible(parser);
    }

    fn update_cursor_x(&mut self, parser: &mut Parser) {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        self.cursor_x = lines
            .get(self.cursor.row)
            .map(|line| display_width(line.chars().take(self.cursor.col)))
            .unwrap_or(0);
    }

    fn keep_cursor_visible(&mut self, parser: &mut Parser) {
        let (_, cols) = parser.screen().size();
        let rows = usize::from(parser.screen().size().0);
        let total = self.total_rows(parser);
        let max_scroll = self.max_scroll(parser);
        let mut start = total.saturating_sub(rows.saturating_add(self.scroll_offset));
        if self.cursor.row < start {
            self.scroll_offset = self
                .scroll_offset
                .saturating_add(start - self.cursor.row)
                .min(max_scroll);
            start = total.saturating_sub(rows.saturating_add(self.scroll_offset));
        }
        if self.cursor.row >= start.saturating_add(rows) {
            let desired_start = self.cursor.row.saturating_sub(rows.saturating_sub(1));
            self.scroll_offset = total
                .saturating_sub(rows.saturating_add(desired_start))
                .min(max_scroll);
        }
        self.cursor.col = self.cursor.col.min(usize::from(cols));
    }

    fn move_to_viewport_line(&mut self, parser: &mut Parser, line: usize) {
        let rows = usize::from(parser.screen().size().0).max(1);
        let start = self
            .total_rows(parser)
            .saturating_sub(rows.saturating_add(self.scroll_offset));
        self.cursor.row = start
            .saturating_add(line.min(rows.saturating_sub(1)))
            .min(self.bottom_row(parser));
        self.cursor.col = self
            .cursor
            .col
            .min(self.line_width(parser, self.cursor.row));
    }

    fn regex_find_last(text: &str, pattern: &str) -> Option<(usize, usize)> {
        let mut offset = 0;
        let mut found = None;
        while offset <= text.len() {
            let Some((start, end)) = crate::server::copy_mode_regex_find(
                pattern,
                &text[offset..],
                search_is_lowercase(pattern),
            ) else {
                break;
            };
            let global_start = offset + start;
            let global_end = offset + end;
            found = Some((global_start, global_end));
            // Advance by one character from the match start rather than to
            // its end: tmux's backward regex search preserves overlapping
            // matches (for example, both occurrences of `aba` in `ababa`).
            if let Some(character) = text[global_start..].chars().next() {
                offset = global_start.saturating_add(character.len_utf8());
            } else {
                break;
            }
        }
        found
    }

    fn literal_find(text: &str, pattern: &str, ignore_case: bool) -> Option<(usize, usize)> {
        if !ignore_case {
            let start = text.find(pattern)?;
            return Some((start, start + pattern.len()));
        }
        let pattern = pattern.chars().collect::<Vec<_>>();
        for (start, _) in text.char_indices() {
            let mut end = start;
            let matched = pattern.iter().all(|expected| {
                let Some(actual) = text[end..].chars().next() else {
                    return false;
                };
                if !actual.eq_ignore_ascii_case(expected) {
                    return false;
                }
                end += actual.len_utf8();
                true
            });
            if matched {
                return Some((start, end));
            }
        }
        None
    }

    fn literal_find_last(text: &str, pattern: &str, ignore_case: bool) -> Option<(usize, usize)> {
        if !ignore_case {
            let start = text.rfind(pattern)?;
            return Some((start, start + pattern.len()));
        }
        let mut offset = 0;
        let mut found = None;
        while offset <= text.len() {
            let Some((start, end)) = Self::literal_find(&text[offset..], pattern, true) else {
                break;
            };
            let global_start = offset + start;
            let global_end = offset + end;
            found = Some((global_start, global_end));
            if global_end > global_start {
                offset = global_end;
            } else if let Some(character) = text[global_start..].chars().next() {
                offset = global_start.saturating_add(character.len_utf8());
            } else {
                break;
            }
        }
        found
    }

    /// Search cyclically through the retained rows, matching tmux's copy-mode
    /// behavior when a search crosses either end of the scrollback.
    fn search(&mut self, parser: &mut Parser, search: String, forward: bool, regex: bool) {
        if search.is_empty() {
            return;
        }
        let ignore_case = search_is_lowercase(&search);
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        let current = self.cursor;
        let found = if forward {
            let first_pass = (current.row..lines.len()).find_map(|row| {
                let start = if row == current.row {
                    if self.keys == CopyModeKeys::Vi {
                        current.col.saturating_add(1)
                    } else {
                        current.col
                    }
                } else {
                    0
                };
                let offset = char_to_byte_offset(&lines[row], start);
                if regex {
                    crate::server::copy_mode_regex_find(&search, &lines[row][offset..], ignore_case)
                        .map(|(match_start, match_end)| {
                            (row, offset + match_start, offset + match_end)
                        })
                } else {
                    Self::literal_find(&lines[row][offset..], &search, ignore_case).map(
                        |(match_start, match_end)| (row, offset + match_start, offset + match_end),
                    )
                }
            });
            if self.wrap_search {
                first_pass.or_else(|| {
                    (0..=current.row).find_map(|row| {
                        if regex {
                            crate::server::copy_mode_regex_find(&search, &lines[row], ignore_case)
                                .map(|(match_start, match_end)| (row, match_start, match_end))
                        } else {
                            Self::literal_find(&lines[row], &search, ignore_case)
                                .map(|(match_start, match_end)| (row, match_start, match_end))
                        }
                    })
                })
            } else {
                first_pass
            }
        } else {
            let first_pass = (0..=current.row).rev().find_map(|row| {
                let end = if row == current.row {
                    char_to_byte_offset(&lines[row], current.col)
                } else {
                    lines[row].len()
                };
                if regex {
                    Self::regex_find_last(&lines[row][..end], &search)
                        .map(|(match_start, match_end)| (row, match_start, match_end))
                } else {
                    Self::literal_find_last(&lines[row][..end], &search, ignore_case)
                        .map(|(match_start, match_end)| (row, match_start, match_end))
                }
            });
            if self.wrap_search {
                first_pass.or_else(|| {
                    ((current.row + 1)..lines.len())
                        .rev()
                        .chain(std::iter::once(current.row))
                        .find_map(|row| {
                            if regex {
                                Self::regex_find_last(&lines[row], &search)
                                    .map(|(match_start, match_end)| (row, match_start, match_end))
                            } else {
                                Self::literal_find_last(&lines[row], &search, ignore_case)
                                    .map(|(match_start, match_end)| (row, match_start, match_end))
                            }
                        })
                })
            } else {
                first_pass
            }
        };
        if let Some((row, byte_col, match_end)) = found {
            let match_col = lines[row][..byte_col].chars().count();
            let match_end_col = lines[row][..match_end].chars().count();
            self.cursor = CopyPosition {
                row,
                col: if forward && self.keys == CopyModeKeys::Emacs {
                    match_end_col
                } else {
                    match_col
                },
            };
            self.last_search = Some((search, forward, regex));
            self.keep_cursor_visible(parser);
        } else {
            self.last_search = Some((search, forward, regex));
        }
    }

    fn search_incremental(&mut self, parser: &mut Parser, search: String, forward: bool) {
        let origin = match self.search_origin {
            Some(origin) => origin,
            None => {
                let origin = self.cursor;
                self.search_origin = Some(origin);
                origin
            }
        };
        self.cursor = origin;
        self.keep_cursor_visible(parser);
        if search.is_empty() {
            self.last_search = None;
            return;
        }
        self.search(parser, search, forward, false);
    }

    fn sync_parser(&self, parser: &mut Parser) {
        parser.screen_mut().set_scrollback(self.scroll_offset);
    }

    fn history_rows(&self, parser: &mut Parser) -> (Vec<String>, Vec<String>) {
        let (mut history, mut live) = history_rows(parser);
        let floor = self.history_floor.min(history.len());
        history.drain(..floor);
        restore_tab_cells(&mut history, &mut live, &self.raw_output);
        (history, live)
    }

    fn move_word(&mut self, parser: &mut Parser, action: CopyAction, repeat: usize) {
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        let mut position = self.cursor;
        for _ in 0..repeat {
            match action {
                CopyAction::PreviousWord | CopyAction::PreviousSpace => {
                    position = previous_word(
                        &lines,
                        position,
                        &self.word_separators,
                        matches!(action, CopyAction::PreviousSpace),
                        self.keys,
                    );
                }
                CopyAction::NextWord
                | CopyAction::NextWordEnd
                | CopyAction::NextSpace
                | CopyAction::NextSpaceEnd => {
                    position = next_word(
                        &lines,
                        position,
                        &self.word_separators,
                        matches!(action, CopyAction::NextSpace | CopyAction::NextSpaceEnd),
                        matches!(action, CopyAction::NextWordEnd | CopyAction::NextSpaceEnd),
                        self.keys,
                    );
                }
                _ => {}
            }
        }
        self.cursor = position;
        self.keep_cursor_visible(parser);
    }

    fn selection(&self, parser: &mut Parser) -> Option<String> {
        let anchor = self.anchor?;
        let reversed = anchor.row > self.cursor.row
            || (anchor.row == self.cursor.row && anchor.col > self.cursor.col);
        let (start, end) = if anchor.row < self.cursor.row
            || (anchor.row == self.cursor.row && anchor.col <= self.cursor.col)
        {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        };
        let (history, live) = self.history_rows(parser);
        let lines = history.into_iter().chain(live).collect::<Vec<_>>();
        if start.row >= lines.len() {
            return Some(String::new());
        }
        let end_row = end.row.min(lines.len().saturating_sub(1));
        let mut output = String::new();
        if self.selection_mode == SelectionMode::Line {
            for row in start.row..=end_row {
                output.push_str(&lines[row]);
                if row != end_row {
                    output.push('\n');
                }
            }
            return Some(output);
        }
        if self.rectangle {
            let from = start.col.min(end.col);
            let to = start.col.max(end.col).saturating_add(1);
            for row in start.row..=end_row {
                let line = &lines[row];
                let width = to.saturating_sub(from);
                let segment = line.chars().skip(from).take(width).collect::<String>();
                let missing = width.saturating_sub(segment.chars().count());
                output.push_str(&segment);
                output.extend(std::iter::repeat_n(' ', missing));
                if row != end_row {
                    output.push('\n');
                }
            }
            return Some(output);
        }
        for row in start.row..=end_row {
            let line = &lines[row];
            let from = if row == start.row { start.col } else { 0 };
            let to = if row == end.row {
                if reversed && end.col == 0 {
                    line.chars().count()
                } else if self.keys == CopyModeKeys::Vi {
                    end.col.saturating_add(1)
                } else {
                    end.col
                }
            } else {
                line.chars().count()
            };
            output.extend(line.chars().skip(from).take(to.saturating_sub(from)));
            if row != end_row {
                output.push('\n');
            }
        }
        Some(output)
    }
}

/// Returns the stable logical history as scrollback rows followed by the live
/// screen rows. `vt100` keeps this data in two grids, so both views are read
/// explicitly and the parser is restored to its prior scroll position.
pub(crate) fn history_rows(parser: &mut Parser) -> (Vec<String>, Vec<String>) {
    let saved = parser.screen().scrollback();
    let (rows, cols) = parser.screen().size();
    parser.screen_mut().set_scrollback(usize::MAX);
    let max_scroll = parser.screen().scrollback();
    let page_rows = usize::from(rows).max(1);
    let mut history = Vec::with_capacity(max_scroll);
    let mut start = 0;
    while start < max_scroll {
        let offset = max_scroll - start;
        parser.screen_mut().set_scrollback(offset);
        let take = offset.min(page_rows);
        history.extend(parser.screen().rows(0, cols).take(take));
        start += take;
    }
    parser.screen_mut().set_scrollback(0);
    let live = parser.screen().rows(0, cols).collect::<Vec<_>>();
    parser.screen_mut().set_scrollback(saved);
    (history, live)
}

fn line_char(line: &str, col: usize) -> Option<char> {
    line.chars().nth(col)
}

fn char_to_byte_offset(line: &str, col: usize) -> usize {
    line.char_indices()
        .nth(col)
        .map(|(offset, _)| offset)
        .unwrap_or(line.len())
}

fn display_width(chars: impl Iterator<Item = char>) -> usize {
    let mut width = 0;
    for character in chars {
        width += if character == '\t' {
            8 - width % 8
        } else if is_wide(character) {
            2
        } else {
            1
        };
    }
    width
}

/// Convert an attached-terminal cell column into the character column used by
/// copy-mode state. A wide character owns two terminal cells and a tab owns
/// the cells up to its next tab stop; clicking either part must address the
/// same logical character.
pub(crate) fn display_column_to_char_index(line: &str, column: usize) -> usize {
    let mut display_column: usize = 0;
    for (index, character) in line.chars().enumerate() {
        let width = if character == '\t' {
            8 - display_column % 8
        } else if is_wide(character) {
            2
        } else {
            1
        };
        if column < display_column.saturating_add(width) {
            return index;
        }
        display_column = display_column.saturating_add(width);
    }
    line.chars().count()
}

fn is_wide(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
    )
}

fn restore_tab_cells(history: &mut [String], live: &mut [String], raw_output: &[u8]) {
    let raw_lines = raw_terminal_lines(raw_output);
    for (index, raw_line) in raw_lines.into_iter().enumerate() {
        if !raw_line.contains('\t') {
            continue;
        }
        if let Some(line) = history.get_mut(index) {
            *line = raw_line;
        } else if let Some(line) = live.get_mut(index.saturating_sub(history.len())) {
            *line = raw_line;
        }
    }
}

fn raw_terminal_lines(raw_output: &[u8]) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut index = 0;
    while index < raw_output.len() {
        let byte = raw_output[index];
        if byte == 0x1b {
            index += 1;
            if raw_output.get(index) == Some(&b']') {
                index += 1;
                while index < raw_output.len()
                    && raw_output[index] != 0x07
                    && !(raw_output[index] == 0x1b && raw_output.get(index + 1) == Some(&b'\\'))
                {
                    index += 1;
                }
                if raw_output.get(index) == Some(&0x1b) {
                    index += 2;
                } else {
                    index += 1;
                }
            } else {
                while index < raw_output.len() && !(0x40..=0x7e).contains(&raw_output[index]) {
                    index += 1;
                }
                index += usize::from(index < raw_output.len());
            }
            continue;
        }
        match byte {
            b'\n' => lines.push(String::new()),
            // PTYs commonly translate LF to CRLF. Keep the accumulated text
            // and let the following LF create the row; a full cursor-aware
            // replay handles explicit carriage-return redraws separately.
            b'\r' => {}
            0x20..=0x7e | b'\t' => lines
                .last_mut()
                .expect("at least one raw line")
                .push(byte as char),
            _ => {}
        }
        index += 1;
    }
    lines
}

fn is_separator(character: char, separators: &str) -> bool {
    !character.is_whitespace() && separators.contains(character)
}

fn previous_word(
    lines: &[String],
    mut position: CopyPosition,
    separators: &str,
    spaces: bool,
    _keys: CopyModeKeys,
) -> CopyPosition {
    let separators = if spaces { "" } else { separators };
    position.col = position.col.min(lines[position.row].chars().count());

    // Match grid_reader_cursor_previous_word: first find a non-whitespace
    // cell to the left, then walk back until the word/separator class changes.
    let mut word_is_letters = None;
    if position.col >= lines[position.row].chars().count()
        || line_char(&lines[position.row], position.col)
            .is_some_and(|character| character.is_whitespace())
    {
        loop {
            if position.col > 0 {
                position.col -= 1;
                if let Some(character) = line_char(&lines[position.row], position.col)
                    .filter(|character| !character.is_whitespace())
                {
                    word_is_letters = Some(!is_separator(character, separators));
                    break;
                }
            } else if position.row > 0 {
                position.row -= 1;
                position.col = lines[position.row].chars().count();
            } else {
                return CopyPosition { row: 0, col: 0 };
            }
        }
    } else if let Some(character) =
        line_char(&lines[position.row], position.col).filter(|character| !character.is_whitespace())
    {
        word_is_letters = Some(!is_separator(character, separators));
    }

    let Some(word_is_letters) = word_is_letters else {
        return position;
    };
    loop {
        let old = position;
        if position.col == 0 {
            if position.row == 0 {
                return old;
            }
            position.row -= 1;
            position.col = lines[position.row].chars().count();
        }
        if position.col > 0 {
            position.col -= 1;
        }
        let Some(character) = line_char(&lines[position.row], position.col) else {
            return old;
        };
        if character.is_whitespace() || word_is_letters != !is_separator(character, separators) {
            return old;
        }
    }
}

fn next_position(lines: &[String], position: &mut CopyPosition) -> bool {
    let width = lines[position.row].chars().count();
    if position.col < width {
        position.col += 1;
        true
    } else if position.row + 1 < lines.len() {
        position.row += 1;
        position.col = 0;
        true
    } else {
        false
    }
}

fn next_word(
    lines: &[String],
    mut position: CopyPosition,
    separators: &str,
    spaces: bool,
    end: bool,
    keys: CopyModeKeys,
) -> CopyPosition {
    let separators = if spaces { "" } else { separators };
    if end && keys == CopyModeKeys::Vi {
        let width = lines[position.row].chars().count();
        if position.col + 1 >= width
            && line_char(&lines[position.row], position.col)
                .is_some_and(|character| !character.is_whitespace())
        {
            return position;
        }
        if line_char(&lines[position.row], position.col)
            .is_some_and(|character| !character.is_whitespace())
        {
            let _ = next_position(lines, &mut position);
        }
    }

    if end {
        loop {
            let Some(character) = line_char(&lines[position.row], position.col) else {
                if !next_position(lines, &mut position) {
                    return position;
                }
                continue;
            };
            if character.is_whitespace() {
                if !next_position(lines, &mut position) {
                    return position;
                }
                continue;
            }
            let separator = is_separator(character, separators);
            while let Some(character) = line_char(&lines[position.row], position.col) {
                if character.is_whitespace() || is_separator(character, separators) != separator {
                    break;
                }
                if !next_position(lines, &mut position) {
                    break;
                }
            }
            if keys == CopyModeKeys::Vi && position.col > 0 {
                position.col -= 1;
            }
            return position;
        }
    }

    loop {
        let Some(character) = line_char(&lines[position.row], position.col) else {
            if !next_position(lines, &mut position) {
                return position;
            }
            continue;
        };
        if !character.is_whitespace() {
            let separator = is_separator(character, separators);
            loop {
                let Some(character) = line_char(&lines[position.row], position.col) else {
                    break;
                };
                if character.is_whitespace() || is_separator(character, separators) != separator {
                    break;
                }
                if !next_position(lines, &mut position) {
                    return position;
                }
            }
        }
        loop {
            let Some(character) = line_char(&lines[position.row], position.col) else {
                if !next_position(lines, &mut position) {
                    return position;
                }
                continue;
            };
            if !character.is_whitespace() {
                return position;
            }
            if !next_position(lines, &mut position) {
                return position;
            }
        }
    }
}

fn previous_utf8_boundary(bytes: &[u8], cursor: usize) -> usize {
    let mut position = cursor.min(bytes.len()).saturating_sub(1);
    while position > 0 && (bytes[position] & 0xc0) == 0x80 {
        position -= 1;
    }
    position
}

pub(crate) fn display_prompt_input(input: &[u8]) -> String {
    let mut output = String::new();
    let mut index = 0;
    while index < input.len() {
        let byte = input[index];
        if byte < 0x20 {
            output.push('^');
            output.push(char::from(byte + b'@'));
            index += 1;
        } else if byte == 0x7f {
            output.push_str("^?");
            index += 1;
        } else if byte < 0x80 {
            output.push(char::from(byte));
            index += 1;
        } else if let Ok(value) = std::str::from_utf8(&input[index..]) {
            output.push_str(value);
            break;
        } else {
            output.push('�');
            index += 1;
        }
    }
    output
}
