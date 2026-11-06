use std::collections::HashMap;

use vt100::Parser;

use crate::copy_mode::{CopyModeKeys, CopyModeOptions, CopyModeState};
use crate::pty::Pty;
use crate::terminal::{self, OutputState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Size {
    cols: u16,
    rows: u16,
}

impl Default for Size {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

impl Size {
    pub(crate) const MINIMUM: u16 = 1;
    pub(crate) const MAXIMUM: u16 = 10_000;

    pub(crate) const fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: Self::clamp(cols),
            rows: Self::clamp(rows),
        }
    }

    const fn clamp(dimension: u16) -> u16 {
        if dimension < Self::MINIMUM {
            Self::MINIMUM
        } else if dimension > Self::MAXIMUM {
            Self::MAXIMUM
        } else {
            dimension
        }
    }

    pub(crate) const fn cols(self) -> u16 {
        self.cols
    }

    pub(crate) const fn rows(self) -> u16 {
        self.rows
    }

    pub(crate) const fn with_cols(self, cols: u16) -> Self {
        Self::new(cols, self.rows)
    }

    pub(crate) const fn with_rows(self, rows: u16) -> Self {
        Self::new(self.cols, rows)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

/// A separator cell in the layout tree. `path` identifies the split node from
/// the root (`false` descends into the first child, `true` into the second),
/// which lets pointer-driven resizing update the exact border the user hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LayoutSeparator {
    pub axis: Axis,
    pub path: Vec<bool>,
    pub first_size: u16,
    pub coordinate: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EdgeMove {
    By(i32),
    To(u16),
}

/// A small layout tree. A split owns the separator cell, which keeps panes
/// from painting over one another and makes geometry changes deterministic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    Leaf(u64),
    Split {
        axis: Axis,
        first: Box<Layout>,
        second: Box<Layout>,
        first_size: Option<u16>,
    },
}

impl Layout {
    pub(crate) fn split_with_size(
        &mut self,
        target: u64,
        new_pane: u64,
        axis: Axis,
        before: bool,
        first_size: Option<u16>,
    ) -> bool {
        if matches!(self, Self::Leaf(id) if *id == target) {
            let old = std::mem::replace(self, Self::Leaf(new_pane));
            let (first, second) = if before {
                (Self::Leaf(new_pane), old)
            } else {
                (old, Self::Leaf(new_pane))
            };
            *self = Self::Split {
                axis,
                first: Box::new(first),
                second: Box::new(second),
                first_size,
            };
            return true;
        }

        match self {
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_with_size(target, new_pane, axis, before, first_size)
                    || second.split_with_size(target, new_pane, axis, before, first_size)
            }
        }
    }

    pub(crate) fn resize(&mut self, rect: Rect, target: u64, axis: Axis, change: EdgeMove) -> bool {
        let Self::Split {
            axis: split_axis,
            first,
            second,
            first_size,
        } = self
        else {
            return false;
        };
        let available = match split_axis {
            Axis::Horizontal => rect.cols.saturating_sub(1),
            Axis::Vertical => rect.rows.saturating_sub(1),
        };
        let current_first = first_size.unwrap_or(available.saturating_add(1) / 2).min(available);
        let target_in_first = first.contains(target);
        if !target_in_first && !second.contains(target) {
            return false;
        }
        let (first_rect, second_rect) = match split_axis {
            Axis::Horizontal => (
                Rect {
                    cols: current_first,
                    ..rect
                },
                Rect {
                    x: rect.x.saturating_add(current_first).saturating_add(1),
                    cols: available.saturating_sub(current_first),
                    ..rect
                },
            ),
            Axis::Vertical => (
                Rect {
                    rows: current_first,
                    ..rect
                },
                Rect {
                    y: rect.y.saturating_add(current_first).saturating_add(1),
                    rows: available.saturating_sub(current_first),
                    ..rect
                },
            ),
        };
        let nested = if target_in_first {
            first.resize(first_rect, target, axis, change)
        } else {
            second.resize(second_rect, target, axis, change)
        };
        if nested {
            return true;
        }
        if *split_axis != axis {
            return false;
        }
        let wanted = match change {
            EdgeMove::By(amount) => i64::from(current_first) + i64::from(amount),
            EdgeMove::To(size) if target_in_first => i64::from(size),
            EdgeMove::To(size) => i64::from(available) - i64::from(size),
        };
        let floor = i64::from(first.minimum_extent(axis));
        let ceiling = i64::from(available) - i64::from(second.minimum_extent(axis));
        if floor <= ceiling {
            let bounded = wanted.clamp(floor, ceiling).clamp(0, i64::from(u16::MAX));
            *first_size = Some(bounded as u16);
        }
        true
    }

    fn minimum_extent(&self, axis: Axis) -> u16 {
        match self {
            Self::Leaf(_) => 1,
            Self::Split {
                axis: split_axis,
                first,
                second,
                ..
            } if *split_axis == axis => first
                .minimum_extent(axis)
                .saturating_add(second.minimum_extent(axis))
                .saturating_add(1),
            Self::Split { first, second, .. } => {
                first.minimum_extent(axis).max(second.minimum_extent(axis))
            }
        }
    }

    fn contains(&self, target: u64) -> bool {
        match self {
            Self::Leaf(id) => *id == target,
            Self::Split { first, second, .. } => first.contains(target) || second.contains(target),
        }
    }

    pub(crate) fn swap_ids(&mut self, first_id: u64, second_id: u64) {
        match self {
            Self::Leaf(id) => {
                if *id == first_id {
                    *id = second_id;
                } else if *id == second_id {
                    *id = first_id;
                }
            }
            Self::Split { first, second, .. } => {
                first.swap_ids(first_id, second_id);
                second.swap_ids(first_id, second_id);
            }
        }
    }

    pub(crate) fn remap_ids(&mut self, mapping: &HashMap<u64, u64>) {
        match self {
            Self::Leaf(id) => {
                if let Some(replacement) = mapping.get(id).copied() {
                    *id = replacement;
                }
            }
            Self::Split { first, second, .. } => {
                first.remap_ids(mapping);
                second.remap_ids(mapping);
            }
        }
    }

    pub(crate) fn remove(&mut self, target: u64) -> bool {
        let replacement = match self {
            Self::Leaf(id) => return *id == target,
            Self::Split { first, second, .. } => {
                if matches!(first.as_ref(), Self::Leaf(id) if *id == target) {
                    Some((**second).clone())
                } else if matches!(second.as_ref(), Self::Leaf(id) if *id == target) {
                    Some((**first).clone())
                } else {
                    None
                }
            }
        };

        if let Some(replacement) = replacement {
            *self = replacement;
            true
        } else if let Self::Split { first, second, .. } = self {
            first.remove(target) || second.remove(target)
        } else {
            false
        }
    }

    pub(crate) fn rectangles(&self, rect: Rect, output: &mut HashMap<u64, Rect>) {
        match self {
            Self::Leaf(id) => {
                output.insert(*id, rect);
            }
            Self::Split {
                axis,
                first,
                second,
                first_size,
            } => match axis {
                Axis::Horizontal => {
                    let available = rect.cols.saturating_sub(1);
                    let first_cols = first_size
                        .unwrap_or(available.saturating_add(1) / 2)
                        .min(available);
                    let second_cols = available.saturating_sub(first_cols);
                    first.rectangles(
                        Rect {
                            cols: first_cols,
                            ..rect
                        },
                        output,
                    );
                    second.rectangles(
                        Rect {
                            x: rect.x.saturating_add(first_cols).saturating_add(1),
                            cols: second_cols,
                            ..rect
                        },
                        output,
                    );
                }
                Axis::Vertical => {
                    let available = rect.rows.saturating_sub(1);
                    let first_rows = first_size.unwrap_or(available.saturating_add(1) / 2).min(available);
                    let second_rows = available.saturating_sub(first_rows);
                    first.rectangles(
                        Rect {
                            rows: first_rows,
                            ..rect
                        },
                        output,
                    );
                    second.rectangles(
                        Rect {
                            y: rect.y.saturating_add(first_rows).saturating_add(1),
                            rows: second_rows,
                            ..rect
                        },
                        output,
                    );
                }
            },
        }
    }

    pub(crate) fn separators(&self, rect: Rect, output: &mut Vec<(u16, u16, Axis)>) {
        let Self::Split {
            axis,
            first,
            second,
            first_size,
        } = self
        else {
            return;
        };
        match axis {
            Axis::Horizontal => {
                let available = rect.cols.saturating_sub(1);
                let first_cols = first_size
                    .unwrap_or(available.saturating_add(1) / 2)
                    .min(available);
                let x = rect.x.saturating_add(first_cols);
                for y in rect.y..rect.y.saturating_add(rect.rows) {
                    output.push((x, y, *axis));
                }
                first.separators(
                    Rect {
                        cols: first_cols,
                        ..rect
                    },
                    output,
                );
                second.separators(
                    Rect {
                        x: x.saturating_add(1),
                        cols: available.saturating_sub(first_cols),
                        ..rect
                    },
                    output,
                );
            }
            Axis::Vertical => {
                let available = rect.rows.saturating_sub(1);
                let first_rows = first_size.unwrap_or(available.saturating_add(1) / 2).min(available);
                let y = rect.y.saturating_add(first_rows);
                for x in rect.x..rect.x.saturating_add(rect.cols) {
                    output.push((x, y, *axis));
                }
                first.separators(
                    Rect {
                        rows: first_rows,
                        ..rect
                    },
                    output,
                );
                second.separators(
                    Rect {
                        y: y.saturating_add(1),
                        rows: available.saturating_sub(first_rows),
                        ..rect
                    },
                    output,
                );
            }
        }
    }

    pub(crate) fn separator_at(&self, rect: Rect, x: u16, y: u16) -> Option<LayoutSeparator> {
        let mut path = Vec::new();
        self.separator_at_with_path(rect, x, y, &mut path)
    }

    fn separator_at_with_path(
        &self,
        rect: Rect,
        x: u16,
        y: u16,
        path: &mut Vec<bool>,
    ) -> Option<LayoutSeparator> {
        let Self::Split {
            axis,
            first,
            second,
            first_size,
        } = self
        else {
            return None;
        };
        match axis {
            Axis::Horizontal => {
                let available = rect.cols.saturating_sub(1);
                let first_cols = first_size
                    .unwrap_or(available.saturating_add(1) / 2)
                    .min(available);
                let separator_x = rect.x.saturating_add(first_cols);
                if x == separator_x && y >= rect.y && y < rect.y.saturating_add(rect.rows) {
                    return Some(LayoutSeparator {
                        axis: *axis,
                        path: path.clone(),
                        first_size: first_cols,
                        coordinate: separator_x,
                    });
                }
                let first_rect = Rect {
                    cols: first_cols,
                    ..rect
                };
                let second_rect = Rect {
                    x: separator_x.saturating_add(1),
                    cols: available.saturating_sub(first_cols),
                    ..rect
                };
                if x >= first_rect.x
                    && x < first_rect.x.saturating_add(first_rect.cols)
                    && y >= first_rect.y
                    && y < first_rect.y.saturating_add(first_rect.rows)
                {
                    path.push(false);
                    let result = first.separator_at_with_path(first_rect, x, y, path);
                    path.pop();
                    result
                } else if x >= second_rect.x
                    && x < second_rect.x.saturating_add(second_rect.cols)
                    && y >= second_rect.y
                    && y < second_rect.y.saturating_add(second_rect.rows)
                {
                    path.push(true);
                    let result = second.separator_at_with_path(second_rect, x, y, path);
                    path.pop();
                    result
                } else {
                    None
                }
            }
            Axis::Vertical => {
                let available = rect.rows.saturating_sub(1);
                let first_rows = first_size.unwrap_or(available.saturating_add(1) / 2).min(available);
                let separator_y = rect.y.saturating_add(first_rows);
                if y == separator_y && x >= rect.x && x < rect.x.saturating_add(rect.cols) {
                    return Some(LayoutSeparator {
                        axis: *axis,
                        path: path.clone(),
                        first_size: first_rows,
                        coordinate: separator_y,
                    });
                }
                let first_rect = Rect {
                    rows: first_rows,
                    ..rect
                };
                let second_rect = Rect {
                    y: separator_y.saturating_add(1),
                    rows: available.saturating_sub(first_rows),
                    ..rect
                };
                if x >= first_rect.x
                    && x < first_rect.x.saturating_add(first_rect.cols)
                    && y >= first_rect.y
                    && y < first_rect.y.saturating_add(first_rect.rows)
                {
                    path.push(false);
                    let result = first.separator_at_with_path(first_rect, x, y, path);
                    path.pop();
                    result
                } else if x >= second_rect.x
                    && x < second_rect.x.saturating_add(second_rect.cols)
                    && y >= second_rect.y
                    && y < second_rect.y.saturating_add(second_rect.rows)
                {
                    path.push(true);
                    let result = second.separator_at_with_path(second_rect, x, y, path);
                    path.pop();
                    result
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn set_separator_size(
        &mut self,
        rect: Rect,
        path: &[bool],
        axis: Axis,
        first_size: u16,
    ) -> bool {
        let Self::Split {
            axis: split_axis,
            first,
            second,
            first_size: current_size,
        } = self
        else {
            return false;
        };
        let available = match split_axis {
            Axis::Horizontal => rect.cols.saturating_sub(1),
            Axis::Vertical => rect.rows.saturating_sub(1),
        };
        if path.is_empty() {
            if *split_axis != axis {
                return false;
            }
            *current_size = Some(first_size.min(available));
            return true;
        }
        let default_first = available.saturating_add(1) / 2;
        let current_first = current_size.unwrap_or(default_first).min(available);
        let first_rect = match split_axis {
            Axis::Horizontal => Rect {
                cols: current_first,
                ..rect
            },
            Axis::Vertical => Rect {
                rows: current_first,
                ..rect
            },
        };
        let second_rect = match split_axis {
            Axis::Horizontal => Rect {
                x: rect.x.saturating_add(current_first).saturating_add(1),
                cols: available.saturating_sub(current_first),
                ..rect
            },
            Axis::Vertical => Rect {
                y: rect.y.saturating_add(current_first).saturating_add(1),
                rows: available.saturating_sub(current_first),
                ..rect
            },
        };
        if path[0] {
            second.set_separator_size(second_rect, &path[1..], axis, first_size)
        } else {
            first.set_separator_size(first_rect, &path[1..], axis, first_size)
        }
    }
}

pub(crate) struct Pane {
    pub id: u64,
    pub index: u32,
    pub rect: Rect,
    pub parser: Parser,
    pub pty: Pty,
    pub command: String,
    pub command_args: Vec<String>,
    /// The directory reported by the pane's shell. `start_path` remains the
    /// creation directory while `current_path` follows OSC 7 notifications.
    pub current_path: Option<String>,
    pub start_path: Option<String>,
    pub dead: bool,
    pub title: String,
    pub enabled: bool,
    pub copy_mode: Option<CopyModeState>,
    pub copy_source: Option<CopySource>,
    pub panes_mode: bool,
    pub copy_prompt_history: Vec<Vec<u8>>,
    pub raw_output: Vec<u8>,
    pub output_state: OutputState,
    pub history_floor: usize,
    pub focused: bool,
    pub active_point: u64,
    pub options: HashMap<String, String>,
}

/// A copy-mode target can display a snapshot from another pane (`copy-mode
/// -s`). The source is retained as terminal bytes so mode actions can rebuild
/// an isolated parser without mutating the target pane's live PTY screen.
#[derive(Clone, Debug)]
pub(crate) struct CopySource {
    pub raw_output: Vec<u8>,
    pub history_floor: usize,
}

impl Pane {
    pub(crate) fn new(
        id: u64,
        index: u32,
        size: Size,
        pty: Pty,
        command: String,
        command_args: Vec<String>,
    ) -> Self {
        Self {
            id,
            index,
            rect: Rect {
                x: 0,
                y: 0,
                cols: size.cols,
                rows: size.rows,
            },
            parser: Parser::new(size.rows, size.cols, 10_000),
            pty,
            command,
            command_args,
            current_path: None,
            start_path: None,
            dead: false,
            title: String::new(),
            enabled: true,
            copy_mode: None,
            copy_source: None,
            panes_mode: false,
            copy_prompt_history: Vec::new(),
            raw_output: Vec::new(),
            output_state: OutputState::default(),
            history_floor: 0,
            focused: false,
            active_point: 0,
            options: HashMap::new(),
        }
    }

    pub(crate) fn empty(id: u64, index: u32, size: Size) -> std::io::Result<Self> {
        Ok(Self::new(
            id,
            index,
            size,
            Pty::empty()?,
            String::new(),
            Vec::new(),
        ))
    }

    pub(crate) fn enter_copy_mode(
        &mut self,
        options: CopyModeOptions<'_>,
        prompt_history: &[Vec<u8>],
        source: Option<CopySource>,
        history_limit: usize,
    ) {
        let raw_output = source.as_ref().map_or_else(
            || self.raw_output.clone(),
            |source| source.raw_output.clone(),
        );
        let source_history_floor = source
            .as_ref()
            .map_or(options.history_floor, |source| source.history_floor);
        let mut source_parser = source
            .as_ref()
            .map(|_| Parser::new(self.rect.rows.max(1), self.rect.cols.max(1), history_limit));
        if let Some(parser) = source_parser.as_mut() {
            terminal::replay(parser, &raw_output);
        }
        let parser = source_parser
            .as_mut()
            .map_or(&mut self.parser, |parser| parser);
        let mut options = options;
        options.history_floor = source_history_floor;
        self.copy_mode = Some(CopyModeState::new(
            parser,
            options,
            &raw_output,
        ));
        self.copy_source = source;
        if let Some(mode) = self.copy_mode.as_mut() {
            mode.set_prompt_history(prompt_history);
        }
    }

    pub(crate) fn linked_clone(&self) -> Self {
        let mut parser = Parser::new(self.rect.rows.max(1), self.rect.cols.max(1), 10_000);
        terminal::replay(&mut parser, &self.raw_output);
        Self {
            id: self.id,
            index: self.index,
            rect: self.rect,
            parser,
            pty: self.pty.clone(),
            command: self.command.clone(),
            command_args: self.command_args.clone(),
            current_path: self.current_path.clone(),
            start_path: self.start_path.clone(),
            dead: self.dead,
            title: self.title.clone(),
            enabled: self.enabled,
            copy_mode: None,
            copy_source: None,
            panes_mode: false,
            copy_prompt_history: self.copy_prompt_history.clone(),
            raw_output: self.raw_output.clone(),
            output_state: OutputState::default(),
            history_floor: self.history_floor,
            focused: self.focused,
            active_point: self.active_point,
            options: self.options.clone(),
        }
    }
}

pub(crate) struct Window {
    pub id: u64,
    pub index: u32,
    pub name: String,
    /// The pane viewport excludes the client-owned status row. `size` is the
    /// viewport dimensions; this origin keeps pane and border coordinates in
    /// client-terminal space when the status bar is at the top.
    pub origin_y: u16,
    pub size: Size,
    pub layout: Layout,
    pub panes: Vec<Pane>,
    pub active_pane: u64,
    pub last_pane: Option<u64>,
    pub zoomed: bool,
    /// A BEL received while this window was being monitored. Selecting the
    /// window clears the alert for every linked winlink.
    pub bell_alert: bool,
    pub mode_keys: CopyModeKeys,
    pub word_separators: String,
    pub synchronize_panes: bool,
    pub options: HashMap<String, String>,
}

impl Window {
    pub(crate) fn new(id: u64, index: u32, name: String, size: Size, mut pane: Pane) -> Self {
        pane.active_point = 1;
        Self {
            id,
            index,
            name,
            origin_y: 0,
            size,
            layout: Layout::Leaf(pane.id),
            active_pane: pane.id,
            last_pane: None,
            zoomed: false,
            bell_alert: false,
            panes: vec![pane],
            mode_keys: CopyModeKeys::Emacs,
            word_separators: crate::copy_mode::DEFAULT_WORD_SEPARATORS.to_owned(),
            synchronize_panes: false,
            options: HashMap::new(),
        }
    }

    pub(crate) fn pane(&self, id: u64) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.id == id)
    }

    pub(crate) fn active(&self) -> Option<&Pane> {
        self.pane(self.active_pane)
    }

    pub(crate) fn linked_clone(&self) -> Self {
        Self {
            id: self.id,
            index: self.index,
            name: self.name.clone(),
            origin_y: self.origin_y,
            size: self.size,
            layout: self.layout.clone(),
            panes: self.panes.iter().map(Pane::linked_clone).collect(),
            active_pane: self.active_pane,
            last_pane: self.last_pane,
            zoomed: self.zoomed,
            bell_alert: self.bell_alert,
            mode_keys: self.mode_keys,
            word_separators: self.word_separators.clone(),
            synchronize_panes: self.synchronize_panes,
            options: self.options.clone(),
        }
    }

    pub(crate) fn reflow(&mut self) {
        let mut rectangles = HashMap::new();
        self.layout.rectangles(self.layout_rect(), &mut rectangles);
        for pane in &mut self.panes {
            if let Some(rect) = rectangles.get(&pane.id).copied() {
                pane.rect = rect;
                if self.zoomed && pane.id == self.active_pane {
                    pane.rect = Rect {
                        x: 0,
                        y: self.origin_y,
                        cols: self.size.cols,
                        rows: self.size.rows,
                    };
                }
                if resize_parser_to_rect(
                    &mut pane.parser,
                    pane.rect.rows.max(1),
                    pane.rect.cols.max(1),
                ) {
                    // The re-anchor is synthetic terminal output. Reset the
                    // PTY stream normalizer so its wrap/REP state cannot refer
                    // to bytes that were only used to restore the screen.
                    pane.output_state.reset_transient();
                }
            }
        }
    }

    /// The rectangle addressed by this window's layout tree. Pane rectangles
    /// remain in client-terminal coordinates so mouse events and rendering
    /// share one coordinate system even with a top status line.
    pub(crate) fn layout_rect(&self) -> Rect {
        Rect {
            x: 0,
            y: self.origin_y,
            cols: self.size.cols,
            rows: self.size.rows,
        }
    }

    pub(crate) fn set_viewport(&mut self, rect: Rect) {
        self.origin_y = rect.y;
        self.size = Size::new(rect.cols, rect.rows);
        self.reflow();
    }

    pub(crate) fn pane_for_index(&self, index: u32) -> Option<u64> {
        self.panes
            .iter()
            .find(|pane| pane.index == index)
            .map(|pane| pane.id)
    }

    pub(crate) fn activate_pane(&mut self, id: u64) {
        let point = self.panes.iter().map(|pane| pane.active_point).max().unwrap_or(0) + 1;
        self.active_pane = id;
        if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
            pane.active_point = point;
        }
    }

    pub(crate) fn lose_pane(&mut self, lost_id: u64, lost_position: usize) {
        if self.last_pane == Some(lost_id) {
            self.last_pane = None;
        }
        if self.active_pane != lost_id {
            return;
        }
        let next = self
            .last_pane
            .filter(|last| self.pane(*last).is_some())
            .or_else(|| {
                lost_position
                    .checked_sub(1)
                    .and_then(|position| self.panes.get(position))
                    .map(|pane| pane.id)
            })
            .or_else(|| self.panes.get(lost_position).map(|pane| pane.id));
        if let Some(next) = next {
            self.activate_pane(next);
        }
    }

    pub(crate) fn renumber_panes(&mut self) {
        self.panes
            .iter_mut()
            .enumerate()
            .for_each(|(index, pane)| pane.index = index as u32);
    }

    pub(crate) fn insert_pane(&mut self, relative_to: u64, before: bool, pane: Pane) {
        let position = self
            .panes
            .iter()
            .position(|existing| existing.id == relative_to)
            .map_or(self.panes.len(), |at| if before { at } else { at + 1 });
        self.panes.insert(position, pane);
        self.renumber_panes();
    }

    pub(crate) fn swap_panes(&mut self, first: u64, second: u64) -> bool {
        let Some(first_index) = self.panes.iter().position(|pane| pane.id == first) else {
            return false;
        };
        let Some(second_index) = self.panes.iter().position(|pane| pane.id == second) else {
            return false;
        };
        self.panes.swap(first_index, second_index);
        self.renumber_panes();
        self.layout.swap_ids(first, second);
        true
    }

    pub(crate) fn rotate_panes(&mut self, up: bool) {
        if self.panes.len() < 2 {
            return;
        }
        let active_position = self
            .panes
            .iter()
            .position(|pane| pane.id == self.active_pane)
            .unwrap_or(0);
        let old_ids = self.panes.iter().map(|pane| pane.id).collect::<Vec<_>>();
        if up {
            self.panes.rotate_left(1);
        } else {
            self.panes.rotate_right(1);
        }
        let new_ids = self.panes.iter().map(|pane| pane.id).collect::<Vec<_>>();
        let mut mapping = HashMap::new();
        for (position, old_id) in old_ids.iter().enumerate() {
            let new_position = if up {
                (position + 1) % old_ids.len()
            } else {
                (position + old_ids.len() - 1) % old_ids.len()
            };
            mapping.insert(*old_id, old_ids[new_position]);
        }
        self.layout.remap_ids(&mapping);
        self.panes
            .iter_mut()
            .enumerate()
            .for_each(|(index, pane)| pane.index = index as u32);
        self.active_pane = new_ids[active_position];
    }
}

/// Resize a pane's terminal model while keeping the newest visible rows when
/// a split shortens its height. `vt100::Screen::set_size` retains rows from
/// the top, which drops a shell prompt at the bottom of the pane until the
/// child redraws after SIGWINCH. Re-anchor the visible rows first so a frame
/// rendered between the resize and that redraw still represents the pane.
fn resize_parser_to_rect(parser: &mut Parser, rows: u16, cols: u16) -> bool {
    let screen = parser.screen();
    let (old_rows, old_cols) = screen.size();
    if old_rows <= rows {
        parser.screen_mut().set_size(rows, cols);
        return false;
    }

    let width = cols.min(old_cols);
    let bottom_offset = old_rows.saturating_sub(rows);
    let (cursor_row, cursor_col) = screen.cursor_position();
    let blank_rows_below_cursor = screen
        .rows(0, old_cols)
        .skip(usize::from(cursor_row) + 1)
        .collect::<Vec<_>>()
        .iter()
        .rev()
        .take_while(|row| row.trim().is_empty())
        .count();
    let keep_from = usize::from(bottom_offset).saturating_sub(blank_rows_below_cursor);
    let saved_rows = screen
        .rows_formatted(0, width)
        .skip(keep_from)
        .take(usize::from(rows))
        .collect::<Vec<_>>();
    let cursor_hidden = screen.hide_cursor();
    let attributes = screen.attributes_formatted();

    parser.screen_mut().set_size(rows, cols);

    let mut redraw = Vec::new();
    for (row, contents) in saved_rows.into_iter().enumerate() {
        redraw.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes());
        redraw.extend_from_slice(&contents);
        redraw.extend_from_slice(b"\x1b[K");
    }
    let mapped_row = usize::from(cursor_row)
        .saturating_sub(keep_from)
        .min(usize::from(rows.saturating_sub(1)));
    let mapped_col = cursor_col.min(cols.saturating_sub(1));
    redraw.extend_from_slice(
        format!(
            "\x1b[{};{}H{}",
            mapped_row + 1,
            mapped_col + 1,
            if cursor_hidden { "\x1b[?25l" } else { "\x1b[?25h" }
        )
        .as_bytes(),
    );
    redraw.extend_from_slice(&attributes);
    parser.process(&redraw);
    true
}

pub(crate) struct Session {
    pub id: u64,
    pub name: String,
    pub size: Size,
    pub windows: Vec<Window>,
    pub active_window: u32,
    pub last_window: Option<u32>,
    pub base_index: u32,
    pub renumber_windows: bool,
    pub next_window_index: u32,
    pub cwd: Option<String>,
    pub options: HashMap<String, String>,
    pub environment: HashMap<String, Option<String>>,
    pub activity: u64,
    pub group: Option<u64>,
}

impl Session {
    pub(crate) fn active_window(&self) -> Option<&Window> {
        self.windows
            .iter()
            .find(|window| window.index == self.active_window)
    }

    pub(crate) fn select_after_removing(&mut self, removed: u32) {
        if self.last_window == Some(removed) {
            self.last_window = None;
        }
        if self.active_window != removed || self.windows.is_empty() {
            return;
        }
        let exists = |index: &u32| self.windows.iter().any(|window| window.index == *index);
        let previous = self
            .windows
            .iter()
            .map(|window| window.index)
            .filter(|index| *index < removed)
            .max()
            .or_else(|| self.windows.iter().map(|window| window.index).max());
        self.active_window = self
            .last_window
            .filter(exists)
            .or(previous)
            .unwrap_or(self.windows[0].index);
        self.last_window = None;
    }

    pub(crate) fn select_window(&mut self, index: u32) {
        if self.active_window != index {
            self.last_window = Some(self.active_window);
            self.active_window = index;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_split_allocates_a_separator_and_reuses_all_columns() {
        let mut layout = Layout::Leaf(1);
        assert!(layout.split_with_size(1, 2, Axis::Horizontal, false, None));
        let mut rectangles = HashMap::new();
        layout.rectangles(
            Rect {
                x: 0,
                y: 0,
                cols: 80,
                rows: 24,
            },
            &mut rectangles,
        );
        assert_eq!(rectangles[&1].cols + rectangles[&2].cols + 1, 80);
        assert_eq!(rectangles[&1].rows, 24);
        assert_eq!(rectangles[&2].x, rectangles[&1].cols + 1);
    }

    #[test]
    fn removing_a_pane_collapses_its_parent_split() {
        let mut layout = Layout::Leaf(1);
        assert!(layout.split_with_size(1, 2, Axis::Vertical, false, None));
        assert!(layout.remove(2));
        assert_eq!(layout, Layout::Leaf(1));
    }

    #[test]
    fn nested_splits_keep_each_pane_reachable() {
        let mut layout = Layout::Leaf(1);
        assert!(layout.split_with_size(1, 2, Axis::Horizontal, false, None));
        assert!(layout.split_with_size(2, 3, Axis::Vertical, false, None));
        let mut rectangles = HashMap::new();
        layout.rectangles(
            Rect {
                x: 0,
                y: 0,
                cols: 80,
                rows: 24,
            },
            &mut rectangles,
        );
        assert_eq!(rectangles.len(), 3);
    }

    #[test]
    fn separators_follow_split_geometry_and_capture_resize_path() {
        let mut layout = Layout::Leaf(1);
        assert!(layout.split_with_size(1, 2, Axis::Horizontal, false, None));
        let rect = Rect {
            x: 0,
            y: 0,
            cols: 20,
            rows: 8,
        };
        let mut separators = Vec::new();
        layout.separators(rect, &mut separators);
        assert_eq!(separators.len(), 8);
        assert!(
            separators
                .iter()
                .all(|(x, _, axis)| { *x == 10 && *axis == Axis::Horizontal })
        );
        let hit = layout
            .separator_at(rect, 10, 3)
            .expect("vertical separator");
        assert_eq!(hit.axis, Axis::Horizontal);
        assert!(hit.path.is_empty());
        assert_eq!(hit.first_size, 10);
        assert!(layout.set_separator_size(rect, &hit.path, hit.axis, 6));
        let mut rectangles = HashMap::new();
        layout.rectangles(rect, &mut rectangles);
        assert_eq!(rectangles[&1].cols, 6);
        assert_eq!(rectangles[&2].x, 7);

        let mut vertical = Layout::Leaf(3);
        assert!(vertical.split_with_size(3, 4, Axis::Vertical, false, None));
        let vertical_rect = Rect {
            x: 0,
            y: 0,
            cols: 12,
            rows: 8,
        };
        let hit = vertical
            .separator_at(vertical_rect, 4, 4)
            .expect("horizontal separator");
        assert_eq!(hit.axis, Axis::Vertical);
        assert_eq!(hit.coordinate, 4);
        assert!(vertical.set_separator_size(vertical_rect, &hit.path, hit.axis, 2));
        let mut vertical_rectangles = HashMap::new();
        vertical.rectangles(vertical_rect, &mut vertical_rectangles);
        assert_eq!(vertical_rectangles[&3].rows, 2);
        assert_eq!(vertical_rectangles[&4].y, 3);
    }

    #[test]
    fn shrinking_a_pane_keeps_its_newest_visible_rows() {
        let mut pane = Pane::empty(1, 0, Size::new(20, 6)).expect("empty resize pane");
        pane.parser
            .process(b"line-0\r\nline-1\r\nline-2\r\nline-3\r\nline-4\r\nline-5");
        let mut window = Window::new(1, 0, "resize".to_owned(), Size::new(20, 6), pane);
        window.layout = Layout::Split {
            axis: Axis::Vertical,
            first: Box::new(Layout::Leaf(1)),
            second: Box::new(Layout::Leaf(2)),
            first_size: None,
        };
        window
            .panes
            .push(Pane::empty(2, 1, Size::new(20, 6)).expect("second resize pane"));
        window.reflow();
        assert_eq!(window.panes[0].parser.screen().contents(), "line-3\nline-4\nline-5");
    }
}
