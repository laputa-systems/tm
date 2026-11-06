use std::path::Path;
use std::time::{Duration, Instant};

use super::parity::{Mux, Scratch};
use super::raw_pty::RawClient;

pub const TIMEOUT: Duration = Duration::from_secs(3);

pub struct Side {
    pub mux: Mux,
    pub client: RawClient,
}

pub struct Pair {
    pub tm: Side,
    pub reference: Side,
    _scratch: Scratch,
}

pub fn state(mux: &Mux) -> String {
    [
        mux.body(&[
            "list-sessions",
            "-F",
            "#{session_name} #{session_windows} #{session_attached}",
        ]),
        mux.body(&[
            "list-windows",
            "-a",
            "-F",
            "#{session_name}:#{window_index} #{window_name} #{window_active} #{window_zoomed_flag} #{window_panes}",
        ]),
        mux.body(&[
            "list-panes",
            "-a",
            "-F",
            "#{session_name}:#{window_index}.#{pane_index} #{pane_id} #{pane_active} #{pane_width}x#{pane_height} #{pane_left},#{pane_top} #{pane_current_path} #{pane_in_mode} #{pane_mode}",
        ]),
    ]
    .join("\n--\n")
}

pub fn poll<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Pair {
    pub fn new(label: &str, setup: impl Fn(&Mux, &Path)) -> Self {
        Self::sized(label, 12, 40, setup)
    }

    pub fn sized(label: &str, rows: u16, columns: u16, setup: impl Fn(&Mux, &Path)) -> Self {
        let scratch = Scratch::new(label);
        let cwd = scratch.root().join("work");
        std::fs::create_dir_all(cwd.join("sub")).expect("create working directory");
        let cwd = cwd.canonicalize().expect("canonical working directory");
        let sides = [Mux::tm(&scratch, "tm"), Mux::reference(&scratch, "reference")].map(|mux| {
            setup(&mux, &cwd);
            let client = RawClient::spawn(&mux, &format!("{label} {:?}", mux.implementation()), &["attach-session", "-t", "S"], rows, columns);
            client.wait_for_text("(S)", TIMEOUT);
            Side { mux, client }
        });
        let [tm, reference] = sides;
        Self { tm, reference, _scratch: scratch }
    }

    pub fn both(&mut self, mut action: impl FnMut(&mut Side)) {
        action(&mut self.tm);
        action(&mut self.reference);
    }

    pub fn assert_same_state(&self, step: &str) {
        let reference = state(&self.reference.mux);
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let tm = state(&self.tm.mux);
            if tm == reference || Instant::now() >= deadline {
                assert_eq!(tm, reference, "state after {step}");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn styled_rows_of(&self, rows: &[u16]) -> (Vec<String>, Vec<String>) {
        let styled = |side: &Side| rows.iter().map(|row| side.client.styled_row(*row)).collect();
        (styled(&self.tm), styled(&self.reference))
    }

    pub fn assert_same_styled_rows(&self, rows: &[u16], step: &str) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let (tm, reference) = self.styled_rows_of(rows);
            if tm == reference || Instant::now() >= deadline {
                assert_eq!(tm, reference, "styled rows {rows:?} after {step}");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn change_styled_rows(&mut self, rows: &[u16], step: &str, action: impl FnMut(&mut Side)) {
        let before = self.styled_rows_of(rows).1;
        self.both(action);
        poll(&format!("the reference rows to change after {step}"), || {
            (self.styled_rows_of(rows).1 != before).then_some(())
        });
        self.assert_same_styled_rows(rows, step);
    }

    pub fn press(&mut self, keys: &[&[u8]], step: &str) {
        let before = state(&self.reference.mux);
        let bytes = keys.concat();
        self.both(|side| side.client.write(&bytes));
        poll(&format!("the reference to react to {step}"), || {
            (state(&self.reference.mux) != before).then_some(())
        });
        self.assert_same_state(step);
    }

    pub fn press_unchanged(&mut self, keys: &[&[u8]], then: &[&[u8]], step: &str) {
        let before = state(&self.reference.mux);
        let bytes = keys.concat();
        self.both(|side| side.client.write(&bytes));
        let barrier = then.concat();
        self.both(|side| side.client.write(&barrier));
        poll(&format!("the reference to react to the barrier after {step}"), || {
            (state(&self.reference.mux) != before).then_some(())
        });
        self.assert_same_state(step);
    }

    pub fn screens_after(&mut self, text: &str, step: &str) -> String {
        self.both(|side| side.client.wait_for_text(text, TIMEOUT));
        let trimmed = |rows: Vec<String>| {
            rows.iter().map(|row| row.trim_end().to_owned()).collect::<Vec<_>>()
        };
        let reference = trimmed(self.reference.client.screen_rows());
        let tm = trimmed(self.tm.client.screen_rows());
        assert_eq!(tm, reference, "client screen after {step}");
        reference.join("\n")
    }
}

pub fn styled_rows(client: &RawClient) -> Vec<String> {
    let color = |color: vt100::Color| match color {
        vt100::Color::Default => '.'.to_string(),
        vt100::Color::Idx(index) => format!("{index:x}"),
        vt100::Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
    };
    client
        .screen_cells()
        .chunks(40)
        .map(|row| {
            let text: String = row
                .iter()
                .map(|(text, ..)| if text.is_empty() { " " } else { text })
                .collect();
            let foreground: String = row.iter().map(|(_, fg, ..)| color(*fg)).collect();
            let background: String = row.iter().map(|(_, _, bg, _)| color(*bg)).collect();
            let inverse: String = row.iter().map(|cell| if cell.3 { 'i' } else { '.' }).collect();
            format!("{}|{foreground}|{background}|{inverse}", text.trim_end())
        })
        .collect()
}
