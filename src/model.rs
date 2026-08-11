use std::collections::HashMap;

use vt100::Parser;

use crate::copy_mode::{CopyModeKeys, CopyModeState};
use crate::pty::Pty;
use crate::terminal::{self, OutputState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl Size {
    pub(crate) const fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }

    pub(crate) fn bounded(self) -> Self {
        Self {
            cols: self.cols.max(1),
            rows: self.rows.max(1),
        }
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
        _full: bool,
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
                first.split_with_size(target, new_pane, axis, before, _full, first_size)
                    || second.split_with_size(target, new_pane, axis, before, _full, first_size)
            }
        }
    }

    pub(crate) fn resize(
        &mut self,
        rect: Rect,
        target: u64,
        axis: Axis,
        delta: i32,
        absolute: Option<u16>,
    ) -> bool {
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
        let current_first = first_size.unwrap_or(available / 2).min(available);
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
        if first.contains(target) {
            if *split_axis == axis {
                let desired = absolute.unwrap_or_else(|| {
                    current_first
                        .saturating_add_signed(delta as i16)
                        .min(available)
                });
                *first_size = Some(desired.min(available));
                true
            } else {
                first.resize(first_rect, target, axis, delta, absolute)
            }
        } else if second.contains(target) {
            if *split_axis == axis {
                let second_current = available.saturating_sub(current_first);
                let desired_second = absolute.unwrap_or_else(|| {
                    second_current
                        .saturating_add_signed(delta as i16)
                        .min(available)
                });
                *first_size = Some(available.saturating_sub(desired_second.min(available)));
                true
            } else {
                second.resize(second_rect, target, axis, delta, absolute)
            }
        } else {
            false
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
                    let first_cols = first_size.unwrap_or(available / 2).min(available);
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
                    let first_rows = first_size.unwrap_or(available / 2).min(available);
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
    pub options: HashMap<String, String>,
    /// A full split keeps its split-axis size but spans the window on the
    /// perpendicular axis. This is the non-floating geometry used by
    /// `split-window -f`.
    pub full_axis: Option<Axis>,
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
        let size = size.bounded();
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
            options: HashMap::new(),
            full_axis: None,
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
        keys: CopyModeKeys,
        exit_on_scroll: bool,
        kill_on_exit: bool,
        hide_position: bool,
        wrap_search: bool,
        word_separators: &str,
        history_floor: usize,
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
            .map_or(history_floor, |source| source.history_floor);
        let mut source_parser = source
            .as_ref()
            .map(|_| Parser::new(self.rect.rows.max(1), self.rect.cols.max(1), history_limit));
        if let Some(parser) = source_parser.as_mut() {
            terminal::replay(parser, &raw_output);
        }
        let parser = source_parser
            .as_mut()
            .map_or(&mut self.parser, |parser| parser);
        self.copy_mode = Some(CopyModeState::new(
            parser,
            keys,
            exit_on_scroll,
            kill_on_exit,
            hide_position,
            wrap_search,
            word_separators,
            &raw_output,
            source_history_floor,
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
            options: self.options.clone(),
            full_axis: self.full_axis,
        }
    }
}

pub(crate) struct Window {
    pub id: u64,
    pub index: u32,
    pub name: String,
    pub size: Size,
    pub layout: Layout,
    pub panes: Vec<Pane>,
    pub active_pane: u64,
    pub last_pane: Option<u64>,
    pub zoomed: bool,
    /// A BEL received while this window was being monitored. Selecting the
    /// window clears the alert for every linked winlink.
    pub bell_alert: bool,
    pub next_pane_index: u32,
    pub mode_keys: CopyModeKeys,
    pub word_separators: String,
    pub synchronize_panes: bool,
    pub options: HashMap<String, String>,
}

impl Window {
    pub(crate) fn new(id: u64, index: u32, name: String, size: Size, pane: Pane) -> Self {
        Self {
            id,
            index,
            name,
            size: size.bounded(),
            layout: Layout::Leaf(pane.id),
            active_pane: pane.id,
            last_pane: None,
            zoomed: false,
            bell_alert: false,
            panes: vec![pane],
            next_pane_index: 1,
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
            size: self.size,
            layout: self.layout.clone(),
            panes: self.panes.iter().map(Pane::linked_clone).collect(),
            active_pane: self.active_pane,
            last_pane: self.last_pane,
            zoomed: self.zoomed,
            bell_alert: self.bell_alert,
            next_pane_index: self.next_pane_index,
            mode_keys: self.mode_keys,
            word_separators: self.word_separators.clone(),
            synchronize_panes: self.synchronize_panes,
            options: self.options.clone(),
        }
    }

    pub(crate) fn reflow(&mut self) {
        let mut rectangles = HashMap::new();
        self.layout.rectangles(
            Rect {
                x: 0,
                y: 0,
                cols: self.size.cols,
                rows: self.size.rows,
            },
            &mut rectangles,
        );
        for pane in &mut self.panes {
            if let Some(rect) = rectangles.get(&pane.id).copied() {
                pane.rect = match pane.full_axis {
                    Some(Axis::Horizontal) => Rect {
                        y: 0,
                        rows: self.size.rows,
                        ..rect
                    },
                    Some(Axis::Vertical) => Rect {
                        x: 0,
                        cols: self.size.cols,
                        ..rect
                    },
                    None => rect,
                };
                if self.zoomed && pane.id == self.active_pane {
                    pane.rect = Rect {
                        x: 0,
                        y: 0,
                        cols: self.size.cols,
                        rows: self.size.rows,
                    };
                }
                pane.parser
                    .screen_mut()
                    .set_size(pane.rect.rows.max(1), pane.rect.cols.max(1));
            }
        }
    }

    pub(crate) fn pane_for_index(&self, index: u32) -> Option<u64> {
        self.panes
            .iter()
            .find(|pane| pane.index == index)
            .map(|pane| pane.id)
    }

    pub(crate) fn swap_panes(&mut self, first: u64, second: u64) -> bool {
        let Some(first_index) = self.panes.iter().position(|pane| pane.id == first) else {
            return false;
        };
        let Some(second_index) = self.panes.iter().position(|pane| pane.id == second) else {
            return false;
        };
        self.panes.swap(first_index, second_index);
        self.panes
            .iter_mut()
            .enumerate()
            .for_each(|(index, pane)| pane.index = index as u32);
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
}

impl Session {
    pub(crate) fn active_window(&self) -> Option<&Window> {
        self.windows
            .iter()
            .find(|window| window.index == self.active_window)
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
        assert!(layout.split_with_size(1, 2, Axis::Horizontal, false, false, None));
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
        assert!(layout.split_with_size(1, 2, Axis::Vertical, false, false, None));
        assert!(layout.remove(2));
        assert_eq!(layout, Layout::Leaf(1));
    }

    #[test]
    fn nested_splits_keep_each_pane_reachable() {
        let mut layout = Layout::Leaf(1);
        assert!(layout.split_with_size(1, 2, Axis::Horizontal, false, false, None));
        assert!(layout.split_with_size(2, 3, Axis::Vertical, false, false, None));
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
}
