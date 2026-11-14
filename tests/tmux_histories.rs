#![cfg(unix)]

#[path = "support/history.rs"]
#[allow(dead_code)]
mod history;
#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use history::{Pair, Rng};
use parity::{ExitStatus, INTENTIONAL_DIFFERENCES, Outcome, check_command};
use raw_pty::RawClient;

const DEFAULT_SEEDS: u64 = 64;
const WORKERS: usize = 4;
const COVERAGE_SEEDS: u64 = 10;
const SESSION_NAMES: [&str; 4] = ["a", "b", "c", "d"];
const WINDOW_NAMES: [&str; 4] = ["w1", "w2", "x", "y"];
const MISSING_SESSIONS: [&str; 3] = ["zz", "$99", "nosuch"];
const UNPARSED_TARGETS: [&str; 13] = ["=zz", "a.b.c", "..", "@", "@x", "::", "=", "%", "%x", "-", "+", "a:b:c", "zz:zz.zz"];
const MALFORMED_TARGETS: [&str; 18] = [
    "", ":", "::", "a:b:c", "%", "@", "$", "%x", "@x", "a:1.x", "=", "=zz", "..", "a.b.c", "-",
    "+", "{nosuch}", "zz:zz.zz",
];

const OBSERVATIONS: [&[&str]; 3] = [
    &[
        "list-sessions",
        "-F",
        "#{session_id} #{session_name} #{session_windows} #{session_attached}",
    ],
    &[
        "list-windows",
        "-a",
        "-F",
        "#{session_name}:#{window_index} #{window_id} #{window_name} #{window_width}x#{window_height} #{window_panes} #{window_active} #{window_last_flag}",
    ],
    &[
        "list-panes",
        "-a",
        "-F",
        "#{session_name} #{session_id} #{window_index} #{window_id} #{window_name} #{pane_index} #{pane_id} #{pane_width}x#{pane_height} #{pane_left},#{pane_top} #{pane_active}",
    ],
];

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum OpKind {
    NewSession,
    NewWindow,
    SplitWindow,
    KillSession,
    KillWindow,
    KillPane,
    SelectWindow,
    SelectPane,
    SwapWindow,
    SwapPane,
    JoinPane,
    BreakPane,
    MoveWindow,
    RenameSession,
    RenameWindow,
    ResizePane,
    SendKeys,
    InvalidTarget,
}

const WEIGHTS: [(OpKind, u64); 18] = [
    (OpKind::NewSession, 3),
    (OpKind::NewWindow, 6),
    (OpKind::SplitWindow, 7),
    (OpKind::KillSession, 2),
    (OpKind::KillWindow, 3),
    (OpKind::KillPane, 3),
    (OpKind::SelectWindow, 4),
    (OpKind::SelectPane, 4),
    (OpKind::SwapWindow, 3),
    (OpKind::SwapPane, 3),
    (OpKind::JoinPane, 4),
    (OpKind::BreakPane, 3),
    (OpKind::MoveWindow, 4),
    (OpKind::RenameSession, 2),
    (OpKind::RenameWindow, 3),
    (OpKind::ResizePane, 4),
    (OpKind::SendKeys, 3),
    (OpKind::InvalidTarget, 4),
];

#[derive(Clone, Debug)]
struct Op {
    kind: OpKind,
    args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exclusion {
    ResizeDirectionInWindowWithThreeOrMorePanes,
    PaneRemovalFromWindowWithThreeOrMorePanes,
    JoinPaneWithinOneWindow,
    MouseTarget,
    MalformedTargetGrammar,
    SessionSizeVariation,
    RelativeWindowPlacement,
}

impl Exclusion {
    fn reason(self) -> &'static str {
        match self {
            Self::ResizeDirectionInWindowWithThreeOrMorePanes => {
                "TMUX-15: tm resizes only the nearest enclosing split"
            }
            Self::MalformedTargetGrammar => {
                "TMUX-31: tm parses malformed session, window and pane targets differently"
            }
            Self::PaneRemovalFromWindowWithThreeOrMorePanes => {
                "TMUX-15: tm's binary layout tree gives a removed middle pane's space to the other side"
            }
            Self::RelativeWindowPlacement => {
                "TMUX-32: a +N or -N window slot places the new or moved window differently"
            }
            Self::SessionSizeVariation => {
                "TMUX-30: tm resizes a moved window to its new session; tmux keeps its size"
            }
            Self::MouseTarget => "tm has no mouse target: a bare = is not resolved like tmux",
            Self::JoinPaneWithinOneWindow => {
                "docs/parity.md join-pane -s (same window as -t): tm refuses, tmux re-lays out"
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Row {
    session: String,
    session_id: String,
    window: String,
    window_id: String,
    window_name: String,
    pane: String,
    pane_id: String,
}

struct Model {
    rows: Vec<Row>,
    session_columns: u64,
    session_rows: u64,
}

impl Model {
    fn parse(listing: &[u8], previous: &Model) -> Self {
        let rows = String::from_utf8_lossy(listing)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split(' ');
                Some(Row {
                    session: fields.next()?.to_owned(),
                    session_id: fields.next()?.to_owned(),
                    window: fields.next()?.to_owned(),
                    window_id: fields.next()?.to_owned(),
                    window_name: fields.next()?.to_owned(),
                    pane: fields.next()?.to_owned(),
                    pane_id: fields.next()?.to_owned(),
                })
            })
            .collect();
        Self {
            rows,
            session_columns: previous.session_columns,
            session_rows: previous.session_rows,
        }
    }

    fn panes_in_window(&self, row: &Row) -> usize {
        self.rows
            .iter()
            .filter(|other| other.session == row.session && other.window == row.window)
            .count()
    }

    fn most_panes_around(&self, target: &str) -> usize {
        if let Some(row) = self.row_for_pane_target(target) {
            return self.panes_in_window(row);
        }
        let (session, window) = match target.split_once(':') {
            Some((session, rest)) => (session, rest.split('.').next().unwrap_or("")),
            None => (target, ""),
        };
        self.rows
            .iter()
            .filter(|row| {
                row.session == session || row.session_id == session || row.window_id == target
            })
            .filter(|row| window.parse::<u32>().is_err() || row.window == window)
            .map(|row| self.panes_in_window(row))
            .max()
            .unwrap_or(0)
    }

    fn row_for_pane_target(&self, target: &str) -> Option<&Row> {
        self.rows.iter().find(|row| {
            target == row.pane_id
                || target == format!("{}:{}.{}", row.session, row.window, row.pane)
        })
    }
}

fn session_target(rng: &mut Rng, model: &Model) -> String {
    if model.rows.is_empty() || rng.chance(1, 5) {
        return rng.pick(&MISSING_SESSIONS).to_string();
    }
    let row = rng.pick(&model.rows);
    if rng.chance(1, 4) {
        row.session_id.clone()
    } else {
        row.session.clone()
    }
}

fn window_target(rng: &mut Rng, model: &Model) -> String {
    if model.rows.is_empty() {
        return "zz:1".to_owned();
    }
    let row = rng.pick(&model.rows);
    match rng.below(20) {
        0..=3 => format!("{}:{}", row.session, row.window),
        4..=5 => row.window_id.clone(),
        6..=7 => format!("{}:{}", row.session, row.window_name),
        8..=9 => format!("{}:", row.session),
        10..=13 => format!(
            "{}:{}",
            row.session,
            rng.pick(&["+", "-", "!", "^", "$", "+1", "-1", "+2"])
        ),
        14..=15 => format!("{}:{}", row.session, 9 + rng.below(3)),
        16 => "@99".to_owned(),
        17 => format!("{}:nosuch", row.session),
        18 => format!("zz:{}", row.window),
        _ => row.session.clone(),
    }
}

fn pane_target(rng: &mut Rng, model: &Model) -> String {
    if model.rows.is_empty() {
        return "zz:1.0".to_owned();
    }
    let row = rng.pick(&model.rows);
    match rng.below(20) {
        0..=4 => format!("{}:{}.{}", row.session, row.window, row.pane),
        5..=8 => row.pane_id.clone(),
        9..=10 => format!("{}:{}", row.session, row.window),
        11 => row.session.clone(),
        12..=14 => format!(
            "{}:{}.{}",
            row.session,
            row.window,
            rng.pick(&["+", "-", "+1", "-1"])
        ),
        15..=16 => format!("{}:{}.{}", row.session, row.window, 8 + rng.below(3)),
        17 => "%99".to_owned(),
        18 => format!("{}:{}.{}", row.session, 9 + rng.below(3), row.pane),
        _ => format!("zz:{}.{}", row.window, row.pane),
    }
}

fn exact_pane_target(rng: &mut Rng, model: &Model) -> String {
    if model.rows.is_empty() || rng.chance(1, 6) {
        return rng.pick(&["%99", "zz:1.0", "a:9.0"]).to_string();
    }
    let row = rng.pick(&model.rows);
    if rng.chance(1, 2) {
        row.pane_id.clone()
    } else {
        format!("{}:{}.{}", row.session, row.window, row.pane)
    }
}

fn slot_target(rng: &mut Rng, model: &Model) -> String {
    if model.rows.is_empty() || rng.chance(1, 6) {
        return window_target(rng, model);
    }
    let row = rng.pick(&model.rows);
    format!("{}:{}", row.session, rng.below(7))
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn sleeper(mut head: Vec<String>) -> Vec<String> {
    head.extend(args(&["--", "sleep", "300"]));
    head
}

fn pick_kind(rng: &mut Rng, model: &Model) -> OpKind {
    if model.rows.is_empty() && rng.chance(3, 4) {
        return OpKind::NewSession;
    }
    let total: u64 = WEIGHTS.iter().map(|(_, weight)| weight).sum();
    let mut roll = rng.below(total);
    for (kind, weight) in WEIGHTS {
        if roll < weight {
            return kind;
        }
        roll -= weight;
    }
    unreachable!("weights cover the roll")
}

fn exclusion(op: &Op, model: &Model) -> Option<Exclusion> {
    if op.args.iter().any(|arg| arg == "=") {
        return Some(Exclusion::MouseTarget);
    }
    if op.args.iter().any(|arg| UNPARSED_TARGETS.contains(&arg.as_str())) {
        return Some(Exclusion::MalformedTargetGrammar);
    }
    if op.kind == OpKind::NewSession {
        let value = |flag: &str| {
            op.args
                .windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1].clone())
        };
        if value("-x") != Some(model.session_columns.to_string())
            || value("-y") != Some(model.session_rows.to_string())
        {
            return Some(Exclusion::SessionSizeVariation);
        }
    }
    if matches!(
        op.kind,
        OpKind::NewWindow | OpKind::MoveWindow | OpKind::BreakPane
    ) {
        let relative = op.args.windows(2).any(|pair| {
            pair[0] == "-t"
                && pair[1]
                    .split(':')
                    .next_back()
                    .is_some_and(|part| part.starts_with(['+', '-']))
        });
        if relative {
            return Some(Exclusion::RelativeWindowPlacement);
        }
    }
    match op.kind {
        OpKind::ResizePane => {
            let target = op
                .args
                .windows(2)
                .find(|pair| pair[0] == "-t")
                .map(|pair| pair[1].as_str())?;
            (model.most_panes_around(target) >= 3)
                .then_some(Exclusion::ResizeDirectionInWindowWithThreeOrMorePanes)
        }
        OpKind::KillPane | OpKind::BreakPane => {
            let flag = if op.kind == OpKind::KillPane { "-t" } else { "-s" };
            let target = op
                .args
                .windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1].as_str())?;
            (model.most_panes_around(target) >= 3)
                .then_some(Exclusion::PaneRemovalFromWindowWithThreeOrMorePanes)
        }
        OpKind::JoinPane => {
            let value = |flag: &str| {
                op.args
                    .windows(2)
                    .find(|pair| pair[0] == flag)
                    .map(|pair| pair[1].as_str())
            };
            if model.most_panes_around(value("-s")?) >= 3 {
                return Some(Exclusion::PaneRemovalFromWindowWithThreeOrMorePanes);
            }
            let source = model.row_for_pane_target(value("-s")?)?;
            let target = model.row_for_pane_target(value("-t")?)?;
            (source.session == target.session && source.window == target.window)
                .then_some(Exclusion::JoinPaneWithinOneWindow)
        }
        _ => None,
    }
}

fn generate(rng: &mut Rng, model: &Model) -> Op {
    let mut last = None;
    for _ in 0..256 {
        let op = generate_unchecked(rng, model);
        match exclusion(&op, model) {
            None => return op,
            Some(excluded) => last = Some(excluded),
        }
    }
    panic!(
        "every candidate was excluded: {}",
        last.map_or("none", Exclusion::reason)
    );
}

fn generate_unchecked(rng: &mut Rng, model: &Model) -> Op {
    let kind = pick_kind(rng, model);
    let mut command = Vec::new();
    match kind {
        OpKind::NewSession => {
            command.extend(args(&["new-session", "-d", "-s", rng.pick(&SESSION_NAMES)]));
            let (columns, rows) = if rng.chance(1, 5) {
                (60 + rng.below(80), 10 + rng.below(30))
            } else {
                (model.session_columns, model.session_rows)
            };
            command.extend(args(&["-x", &columns.to_string(), "-y", &rows.to_string()]));
            command = sleeper(command);
        }
        OpKind::NewWindow => {
            command.extend(args(&["new-window", "-d"]));
            let target = if rng.chance(1, 2) {
                slot_target(rng, model)
            } else {
                window_target(rng, model)
            };
            match rng.below(4) {
                0 => command.push("-a".to_owned()),
                1 => command.push("-b".to_owned()),
                _ => {}
            }
            if rng.chance(1, 2) {
                command.extend(args(&["-n", rng.pick(&WINDOW_NAMES)]));
            }
            command.extend(["-t".to_owned(), target]);
            command = sleeper(command);
        }
        OpKind::SplitWindow => {
            command.extend(args(&["split-window", "-d"]));
            command.push(rng.pick(&["-v", "-h"]).to_string());
            if rng.chance(1, 4) {
                command.push("-b".to_owned());
            }
            match rng.below(4) {
                0 => command.extend(["-l".to_owned(), (1 + rng.below(12)).to_string()]),
                1 => command.extend(["-p".to_owned(), (10 + rng.below(80)).to_string()]),
                _ => {}
            }
            command.extend(["-t".to_owned(), pane_target(rng, model)]);
            command = sleeper(command);
        }
        OpKind::KillSession => {
            command.push("kill-session".to_owned());
            if rng.chance(1, 5) {
                command.push("-a".to_owned());
            }
            command.extend(["-t".to_owned(), session_target(rng, model)]);
        }
        OpKind::KillWindow => {
            command.push("kill-window".to_owned());
            if rng.chance(1, 5) {
                command.push("-a".to_owned());
            }
            command.extend(["-t".to_owned(), window_target(rng, model)]);
        }
        OpKind::KillPane => {
            command.push("kill-pane".to_owned());
            if rng.chance(1, 6) {
                command.push("-a".to_owned());
            }
            command.extend(["-t".to_owned(), pane_target(rng, model)]);
        }
        OpKind::SelectWindow => {
            command.push("select-window".to_owned());
            if rng.chance(1, 3) {
                command.push(rng.pick(&["-l", "-n", "-p"]).to_string());
                command.extend(["-t".to_owned(), session_target(rng, model)]);
            } else {
                command.extend(["-t".to_owned(), window_target(rng, model)]);
            }
        }
        OpKind::SelectPane => {
            command.push("select-pane".to_owned());
            if rng.chance(1, 3) {
                command.push(rng.pick(&["-L", "-R", "-U", "-D", "-l"]).to_string());
            }
            command.extend(["-t".to_owned(), pane_target(rng, model)]);
        }
        OpKind::SwapWindow => {
            command.push("swap-window".to_owned());
            if rng.chance(1, 3) {
                command.push("-d".to_owned());
            }
            command.extend([
                "-s".to_owned(),
                window_target(rng, model),
                "-t".to_owned(),
                window_target(rng, model),
            ]);
        }
        OpKind::SwapPane => {
            command.push("swap-pane".to_owned());
            if rng.chance(1, 3) {
                command.push("-d".to_owned());
            }
            if rng.chance(1, 4) {
                command.push(rng.pick(&["-U", "-D"]).to_string());
            }
            command.extend([
                "-s".to_owned(),
                pane_target(rng, model),
                "-t".to_owned(),
                pane_target(rng, model),
            ]);
        }
        OpKind::JoinPane => {
            command.push("join-pane".to_owned());
            if rng.chance(1, 3) {
                command.push("-d".to_owned());
            }
            command.push(rng.pick(&["-v", "-h"]).to_string());
            if rng.chance(1, 4) {
                command.push("-b".to_owned());
            }
            if rng.chance(1, 4) {
                command.extend(["-l".to_owned(), (1 + rng.below(12)).to_string()]);
            }
            command.extend([
                "-s".to_owned(),
                exact_pane_target(rng, model),
                "-t".to_owned(),
                exact_pane_target(rng, model),
            ]);
        }
        OpKind::BreakPane => {
            command.push("break-pane".to_owned());
            if rng.chance(1, 2) {
                command.push("-d".to_owned());
            }
            if rng.chance(1, 3) {
                command.extend(["-n".to_owned(), rng.pick(&WINDOW_NAMES).to_string()]);
            }
            if rng.chance(1, 2) {
                command.extend(["-t".to_owned(), slot_target(rng, model)]);
            }
            command.extend(["-s".to_owned(), pane_target(rng, model)]);
        }
        OpKind::MoveWindow => {
            command.push("move-window".to_owned());
            if rng.chance(1, 3) {
                command.push("-d".to_owned());
            }
            if rng.chance(1, 5) {
                command.push("-r".to_owned());
                command.extend(["-t".to_owned(), session_target(rng, model)]);
            } else {
                match rng.below(5) {
                    0 => command.push("-a".to_owned()),
                    1 => command.push("-k".to_owned()),
                    _ => {}
                }
                command.extend([
                    "-s".to_owned(),
                    window_target(rng, model),
                    "-t".to_owned(),
                    slot_target(rng, model),
                ]);
            }
        }
        OpKind::RenameSession => {
            command.extend(args(&["rename-session", "-t"]));
            command.push(session_target(rng, model));
            command.push(rng.pick(&SESSION_NAMES).to_string());
        }
        OpKind::RenameWindow => {
            command.extend(args(&["rename-window", "-t"]));
            command.push(window_target(rng, model));
            command.push(rng.pick(&WINDOW_NAMES).to_string());
        }
        OpKind::ResizePane => {
            command.extend(args(&["resize-pane", "-t"]));
            command.push(pane_target(rng, model));
            let amount = (1 + rng.below(15)).to_string();
            match rng.below(6) {
                0 => command.extend(["-x".to_owned(), amount]),
                1 => command.extend(["-y".to_owned(), amount]),
                n => {
                    command.push(["-L", "-R", "-U", "-D"][(n as usize) % 4].to_owned());
                    command.push(amount);
                }
            }
        }
        OpKind::SendKeys => {
            command.extend(args(&["send-keys", "-t"]));
            command.push(pane_target(rng, model));
            if rng.chance(1, 4) {
                command.push("Enter".to_owned());
            } else {
                command.extend(args(&["-l", "text"]));
            }
        }
        OpKind::InvalidTarget => {
            let bad = rng.pick(&MALFORMED_TARGETS).to_string();
            let good = pane_target(rng, model);
            command = match rng.below(10) {
                0 => args(&["select-window", "-t"]),
                1 => args(&["select-pane", "-t"]),
                2 => args(&["kill-pane", "-t"]),
                3 => args(&["rename-window", "-t"]),
                4 => args(&["list-panes", "-F", "#{pane_id}", "-t"]),
                5 => args(&["list-windows", "-F", "#{window_id}", "-t"]),
                6 => args(&["resize-pane", "-U", "-t"]),
                7 => args(&["send-keys", "-l", "-t"]),
                8 => vec![
                    "swap-pane".to_owned(),
                    "-s".to_owned(),
                    good,
                    "-t".to_owned(),
                ],
                _ => args(&["split-window", "-d", "-t"]),
            };
            command.push(bad);
            match command[0].as_str() {
                "rename-window" | "send-keys" => command.push("x".to_owned()),
                "split-window" => command = sleeper(command),
                _ => {}
            }
        }
    }
    Op { kind, args: command }
}

struct Failure {
    step: usize,
    message: String,
}

struct History {
    pair: Pair,
    last: Vec<(Outcome, Outcome)>,
    model: Model,
}

impl History {
    fn new(label: &str, columns: u64, rows: u64) -> Result<Self, String> {
        let pair = Pair::new(label);
        let last = pair.observe("topology", &OBSERVATIONS)?;
        Ok(Self {
            pair,
            last,
            model: Model {
                rows: Vec::new(),
                session_columns: columns,
                session_rows: rows,
            },
        })
    }

    fn apply(&mut self, args: &[String]) -> Result<(), String> {
        let arguments: Vec<&str> = args.iter().map(String::as_str).collect();
        let (_, reference) = self.pair.both("topology", &arguments)?;
        let after = self.pair.observe("topology", &OBSERVATIONS)?;
        if reference.status != ExitStatus::Code(0)
            && let Some(difference) =
                history::same_observations("topology after a failed command", &self.last, &after)
        {
            return Err(format!(
                "{arguments:?} failed ({:?}) yet changed the topology\n{difference}",
                reference.status
            ));
        }
        self.model = Model::parse(&after[2].1.stdout, &self.model);
        self.last = after;
        Ok(())
    }

    fn dump(&self) -> String {
        history::observation_text(&self.last, &OBSERVATIONS)
    }
}

fn replay(label: &str, size: (u64, u64), ops: &[Op]) -> Option<(Failure, String)> {
    let mut history = match history::catching(|| History::new(label, size.0, size.1)) {
        Ok(history) => history,
        Err(message) => return Some((Failure { step: 0, message }, String::new())),
    };
    for (step, op) in ops.iter().enumerate() {
        if exclusion(op, &history.model).is_some() {
            return None;
        }
        if let Err(message) = history::catching(|| history.apply(&op.args)) {
            let dump = history.dump();
            return Some((Failure { step, message }, dump));
        }
    }
    None
}

fn render(ops: &[Op]) -> String {
    ops.iter()
        .enumerate()
        .map(|(index, op)| format!("  {index:>2} {:?}: {:?}", op.kind, op.args))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_seed(seed: u64) -> Result<Vec<OpKind>, String> {
    let mut rng = Rng::new(seed);
    let length = 24 + rng.below(17) as usize;
    let size = (60 + rng.below(80), 10 + rng.below(30));
    let label = format!("history-{seed}");
    let mut ops = Vec::new();
    let mut failure = None;
    {
        let mut history = History::new(&label, size.0, size.1)?;
        for step in 0..length {
            let op = generate(&mut rng, &history.model);
            ops.push(op.clone());
            if let Err(message) = history::catching(|| history.apply(&op.args)) {
                failure = Some((Failure { step, message }, history.dump()));
                break;
            }
        }
    }
    let Some((failure, dump)) = failure else {
        return Ok(ops.iter().map(|op| op.kind).collect());
    };
    let reduced = history::ddmin(&ops, &mut |subset| {
        replay(&format!("{label}-shrink"), size, subset).is_some()
    });
    let (reduced_failure, reduced_dump) = replay(&format!("{label}-final"), size, &reduced)
        .unwrap_or((
            Failure {
                step: failure.step,
                message: "reduced history no longer fails".to_owned(),
            },
            String::new(),
        ));
    let pair = Pair::new(&format!("{label}-version"));
    Err(format!(
        "history seed {seed} diverged at step {} ({})\n{}\n\nfull operations:\n{}\n\nobservations at the failure:\n{dump}\nreduced operations ({} of {}):\n{}\n\nreduced failure at step {}: {}\nreduced observations:\n{reduced_dump}\nreference: {}\nreplay: TM_HISTORY_SEED={seed} cargo test --features parity --test tmux_histories topology",
        failure.step,
        ops[failure.step].args.join(" "),
        failure.message,
        render(&ops),
        reduced.len(),
        ops.len(),
        render(&reduced),
        reduced_failure.step,
        reduced_failure.message,
        history::reference_version(&pair),
    ))
}

#[test]
fn topology_histories_match_the_reference() {
    let started = Instant::now();
    let seeds = history::seeds(&(0..DEFAULT_SEEDS).collect::<Vec<_>>());
    let next = AtomicUsize::new(0);
    let results = std::sync::Mutex::new(rustc_hash::FxHashMap::default());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&seed) = seeds.get(index) else {
                        return;
                    };
                    let result = run_seed(seed);
                    results.lock().expect("results").insert(seed, result);
                }
            });
        }
    });
    let results = results.into_inner().expect("results");
    let failures: Vec<&String> = by_seed(&results)
        .into_iter()
        .filter_map(|(_, result)| result.as_ref().err())
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} histories diverged\n\n{}",
        failures.len(),
        seeds.len(),
        failures
            .iter()
            .map(|failure| failure.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    eprintln!("topology: {} seeds in {:?}", seeds.len(), started.elapsed());
    if std::env::var_os("TM_HISTORY_SEED").is_none() {
        let seen: rustc_hash::FxHashSet<OpKind> = by_seed(&results)
            .into_iter()
            .filter(|(seed, _)| *seed < COVERAGE_SEEDS)
            .flat_map(|(_, result)| result.as_ref().expect("checked above").iter().copied())
            .collect();
        let missing: Vec<OpKind> = WEIGHTS
            .iter()
            .map(|(kind, _)| *kind)
            .filter(|kind| !seen.contains(kind))
            .collect();
        assert!(
            missing.is_empty(),
            "operations absent from the first {COVERAGE_SEEDS} seeds: {missing:?}"
        );
    }
}

fn assert_history(label: &str, steps: &[&[&str]]) {
    let ops: Vec<Op> = steps
        .iter()
        .map(|step| Op {
            kind: OpKind::InvalidTarget,
            args: args(step),
        })
        .collect();
    if let Some((failure, dump)) = replay("reg", (80, 24), &ops) {
        panic!(
            "{label} diverged at step {} ({:?}): {}\n{dump}",
            failure.step, steps[failure.step], failure.message
        );
    }
}

const SLEEP: [&str; 3] = ["--", "sleep", "300"];

fn session(name: &str) -> Vec<&str> {
    let mut command = vec!["new-session", "-d", "-s", name];
    command.extend(SLEEP);
    command
}

#[test]
fn regression_list_sessions_orders_by_name() {
    assert_history(
        "list-sessions-order",
        &[&session("c"), &session("b"), &session("a")],
    );
}

#[test]
fn regression_new_window_after_an_absent_index_uses_that_index() {
    assert_history(
        "new-window-absent-index",
        &[
            &session("d"),
            &["new-window", "-d", "-a", "-t", "d:6", "--", "sleep", "300"],
            &["new-window", "-d", "-b", "-t", "d:11", "--", "sleep", "300"],
        ],
    );
}

#[test]
fn regression_missing_session_errors_read_cant_find_session() {
    assert_history(
        "missing-session-text",
        &[
            &session("a"),
            &["new-window", "-d", "-t", "zz:1", "--", "sleep", "300"],
            &["new-window", "-d", "-t", "$99:1", "--", "sleep", "300"],
        ],
    );
}

#[test]
fn regression_swap_pane_direction_still_resolves_the_source() {
    assert_history(
        "swap-pane-direction-source",
        &[
            &session("a"),
            &["swap-pane", "-U", "-s", "%99", "-t", "a:1.0"],
            &["swap-pane", "-s", "b:4.1", "-t", "%99"],
        ],
    );
}

#[test]
fn regression_move_window_renumber_ignores_a_missing_session() {
    assert_history(
        "move-window-renumber-missing",
        &[&session("a"), &["move-window", "-r", "-t", "nosuch"]],
    );
}

#[test]
fn regression_pane_offsets_wrap_around_the_window() {
    assert_history(
        "pane-offsets",
        &[
            &session("b"),
            &["split-window", "-d", "-h", "-t", "b:1.+1", "--", "sleep", "300"],
            &["resize-pane", "-t", "b:1.-1", "-U", "8"],
            &["send-keys", "-t", "b:1.-1", "Enter"],
            &["select-pane", "-t", "b:1.+1"],
        ],
    );
}

#[test]
fn regression_break_pane_of_a_sole_pane_into_another_session_survives() {
    assert_history(
        "break-pane-sole-pane-other-session",
        &[
            &session("c"),
            &session("a"),
            &["break-pane", "-s", "c:1.0"],
        ],
    );
}

#[test]
fn regression_break_pane_onto_its_own_index_reads_same_index() {
    assert_history(
        "break-pane-same-index",
        &[
            &session("a"),
            &["break-pane", "-d", "-t", "a:1", "-s", "%0"],
            &["break-pane", "-t", "a:$", "-s", "a:1.0"],
        ],
    );
}

#[test]
fn regression_ambiguous_window_name_cannot_be_found() {
    assert_history(
        "ambiguous-window-name",
        &[
            &session("b"),
            &["new-window", "-d", "--", "sleep", "300"],
            &["kill-window", "-t", "b:sleep"],
        ],
    );
}

#[test]
fn regression_kill_window_all_keeps_a_sole_window_index() {
    assert_history(
        "kill-window-all-sole-window",
        &[
            &session("d"),
            &["move-window", "-s", "d:1", "-t", "d:6"],
            &["kill-window", "-a", "-t", "d:6"],
        ],
    );
}

#[test]
fn regression_kill_window_all_renumbers_every_session() {
    assert_history(
        "kill-window-all-renumbers-all",
        &[
            &session("b"),
            &session("d"),
            &["move-window", "-s", "b:1", "-t", "b:5"],
            &["new-window", "-d", "-t", "d:3", "--", "sleep", "300"],
            &["kill-window", "-a", "-t", "d:1"],
        ],
    );
}

#[test]
fn regression_killing_a_windows_last_pane_selects_the_previous_window() {
    assert_history(
        "kill-last-pane-selects-previous-window",
        &[
            &session("b"),
            &["new-window", "-d", "-n", "x", "-t", "b:-1", "--", "sleep", "300"],
            &["new-window", "-d", "-b", "-t", "b:", "--", "sleep", "300"],
            &["kill-pane", "-t", "b"],
        ],
    );
}

#[test]
fn regression_new_window_picks_the_lowest_free_index() {
    assert_history(
        "new-window-lowest-free-index",
        &[
            &session("b"),
            &["new-window", "-d", "-a", "-n", "y", "-t", "b:6", "--", "sleep", "300"],
            &["new-window", "-d", "-t", "b:3", "--", "sleep", "300"],
            &["kill-pane", "-t", "b:6.-"],
            &["new-window", "-d", "-b", "-t", "b:2", "--", "sleep", "300"],
            &["new-window", "-d", "-n", "w2", "-t", "b", "--", "sleep", "300"],
            &["break-pane", "-d", "-t", "b", "-s", "b:1.0"],
        ],
    );
}

#[test]
fn regression_break_pane_keeps_windows_in_index_order() {
    assert_history(
        "break-pane-index-order",
        &[
            &session("a"),
            &["split-window", "-d", "-h", "-t", "a:1.0", "--", "sleep", "300"],
            &["break-pane", "-t", "a:10", "-s", "a"],
            &["split-window", "-d", "-v", "-l", "10", "-t", "a:1.0", "--", "sleep", "300"],
            &["break-pane", "-t", "a:5", "-s", "a:1.-"],
            &["kill-window", "-t", "a:-1"],
        ],
    );
}

#[test]
fn regression_moving_the_current_window_keeps_the_last_window() {
    assert_history(
        "move-current-window-last-flag",
        &[
            &session("c"),
            &["split-window", "-d", "-h", "-t", "c:1.0", "--", "sleep", "300"],
            &["break-pane", "-s", "c:1.1"],
            &["break-pane", "-t", "c:6", "-s", "c:2"],
        ],
    );
}

const ATTACHED_SEEDS: u64 = 48;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AttachedExclusion {
    SharedSessionClientSizes,
    UnattachedWindowSize,
    KilledClientTerminalRestore,
    ResizeOfScrolledWindow,
    ResizeOfSplitWindow,
    WindowRemovalFromHistoryStack,
}

impl AttachedExclusion {
    fn reason(self) -> &'static str {
        match self {
            Self::SharedSessionClientSizes => {
                "TMUX-35: tm draws a client whose size differs from a shared session's window differently"
            }
            Self::UnattachedWindowSize => {
                "TMUX-34: tm resizes every window of a session to its client; tmux resizes only a displayed window"
            }
            Self::ResizeOfScrolledWindow => {
                "TMUX-27: tm keeps fewer scrollback rows than tmux when a window with output history is resized"
            }
            Self::ResizeOfSplitWindow => {
                "TMUX-37: tm and tmux divide the rows or columns of a resized window with two or more panes differently"
            }
            Self::WindowRemovalFromHistoryStack => {
                "TMUX-36: tm keeps one last window; tmux keeps a stack, so removing the current window of a session with two windows leaves a different last window"
            }
            Self::KilledClientTerminalRestore => {
                "TMUX-33: tmux's server restores the terminal of a SIGKILLed client; tm's client restores its own"
            }
        }
    }
}

const ATTACHED_OBSERVATIONS: [&[&str]; 3] = [
    OBSERVATIONS[0],
    &[
        "list-windows",
        "-a",
        "-F",
        "#{session_name}:#{window_index} #{window_id} #{window_name} #{window_width}x#{window_height} #{window_panes} #{window_active} #{window_last_flag} #{session_attached}",
    ],
    &[
        "list-panes",
        "-a",
        "-F",
        "#{session_name} #{session_id} #{window_index} #{window_id} #{window_name} #{pane_index} #{pane_id} #{pane_width}x#{pane_height} #{pane_left},#{pane_top} #{pane_active} #{window_active} #{session_attached}",
    ],
];

fn mask_unattached_window_sizes(command: &[&str], stdout: &[u8]) -> String {
    let size_columns: &[usize] = match command[1] {
        "-a" if command[0] == "list-windows" => &[3],
        "-a" if command[0] == "list-panes" => &[7, 8],
        _ => return String::from_utf8_lossy(stdout).into_owned(),
    };
    let (active_column, attached_column) = if command[0] == "list-windows" { (5, 7) } else { (10, 11) };
    String::from_utf8_lossy(stdout)
        .lines()
        .map(|line| {
            let mut fields: Vec<&str> = line.split(' ').collect();
            let shown = fields.get(active_column) == Some(&"1")
                && fields.get(attached_column).is_some_and(|attached| *attached != "0");
            if !shown {
                for column in size_columns {
                    if let Some(field) = fields.get_mut(*column) {
                        *field = "-";
                    }
                }
            }
            fields.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
const CONVERGE_TIMEOUT: Duration = Duration::from_secs(6);
const SPAWN_TIMEOUT: Duration = Duration::from_secs(5);
const SLOT_SIZES: [[(u16, u16); 3]; 2] = [[(10, 40), (12, 44), (10, 48)], [(8, 30), (9, 34), (8, 28)]];
const LIST_CLIENTS: [&str; 3] = [
    "list-clients",
    "-F",
    "#{session_name} #{client_width}x#{client_height} #{client_termname}",
];
const CLIENT_NAMES: [&str; 3] = ["list-clients", "-F", "#{client_name} #{client_width}"];
const FLOOD: [&str; 4] = ["--", "sh", "-c", "seq 1 1500; sleep 300"];
const IN_PANE_FORMAT: &str = "#{session_name}:#{window_index}.#{pane_index}";
const IN_PANE_COMMANDS: [&str; 7] = [
    "display-message -p #{session_name}:#{window_index}.#{pane_index}",
    "split-window -d -P -F #{session_name}:#{window_index}.#{pane_index} -- sleep 300",
    "new-window -d -P -F #{session_name}:#{window_index}.#{pane_index} -- sleep 300",
    "next-window",
    "break-pane -d -P -F #{session_name}:#{window_index}.#{pane_index}",
    "move-window -d -t {other}:99",
    "rename-session r{n}",
];
const IN_PANE_TIMEOUT: Duration = Duration::from_secs(6);
const PROMPT_COMMANDS: [&str; 5] = ["new-window", "rename-window", "split-window", "next-window", "rename-session"];

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum AttachedKind {
    Attach,
    DetachKey,
    DetachCommand,
    SwitchClient,
    ClientCommand,
    OutsideCommand,
    ResizeClient,
    NewSession,
    ChildExit,
    WindowFlood,
    KillClientRendering,
    DetachDuringRedraw,
    InPane,
}

const ATTACHED_WEIGHTS: [(AttachedKind, u64); 13] = [
    (AttachedKind::Attach, 7),
    (AttachedKind::DetachKey, 3),
    (AttachedKind::DetachCommand, 2),
    (AttachedKind::SwitchClient, 4),
    (AttachedKind::ClientCommand, 6),
    (AttachedKind::OutsideCommand, 3),
    (AttachedKind::ResizeClient, 4),
    (AttachedKind::NewSession, 3),
    (AttachedKind::ChildExit, 3),
    (AttachedKind::WindowFlood, 3),
    (AttachedKind::KillClientRendering, 2),
    (AttachedKind::DetachDuringRedraw, 2),
    (AttachedKind::InPane, 4),
];

#[derive(Clone, Debug)]
struct AttachedOp {
    kind: AttachedKind,
    slot: usize,
    text: String,
    size: (u16, u16),
}

struct Seat {
    tm: RawClient,
    reference: RawClient,
}

struct World {
    seats: [Option<Seat>; 2],
    pair: Pair,
    sessions: Vec<String>,
    client_sessions: [Option<String>; 2],
    client_sizes: [Option<(u16, u16)>; 2],
    window_counts: std::collections::HashMap<String, usize>,
    active_window_sizes: std::collections::HashMap<String, (u16, u16)>,
    scrolled_sessions: rustc_hash::FxHashSet<String>,
    split_sessions: rustc_hash::FxHashSet<String>,
    scrolled_windows: rustc_hash::FxHashSet<String>,
}

fn slot_of(width: u64) -> usize {
    usize::from(width < 38)
}

fn sort_lines(bytes: &[u8]) -> Vec<String> {
    let mut lines: Vec<String> = String::from_utf8_lossy(bytes).lines().map(str::to_owned).collect();
    lines.sort();
    lines
}

/// Seed order, so failure reports stay in the order the seeds were scheduled.
fn by_seed<V>(results: &rustc_hash::FxHashMap<u64, V>) -> Vec<(u64, &V)> {
    let mut seeds: Vec<u64> = results.keys().copied().collect();
    seeds.sort_unstable();
    seeds.into_iter().map(|seed| (seed, &results[&seed])).collect()
}

fn client_name(mux: &parity::Mux, slot: usize) -> Option<String> {
    let listing = mux.body(&CLIENT_NAMES);
    listing.lines().find_map(|line| {
        let (name, width) = line.split_once(' ')?;
        (slot_of(width.parse().ok()?) == slot).then(|| name.to_owned())
    })
}

fn screen_difference(slot: usize, tm: &RawClient, reference: &RawClient) -> Option<String> {
    let rows = |client: &RawClient| -> Vec<String> {
        client.screen_rows().iter().map(|row| row.trim_end().to_owned()).collect()
    };
    let tm_rows = rows(tm);
    let reference_rows = rows(reference);
    if tm_rows != reference_rows {
        return Some(format!(
            "client slot {slot} screen differs\n tm:\n{}\nref:\n{}",
            tm_rows.join("\n"),
            reference_rows.join("\n")
        ));
    }
    parity::observation_difference("client slot modes", &tm.modes(), &reference.modes())
}

impl World {
    fn new(label: &str) -> Result<Self, String> {
        let pair = Pair::new(label);
        let mut world = Self {
            seats: [None, None],
            pair,
            sessions: Vec::new(),
            client_sessions: [None, None],
            client_sizes: [None, None],
            window_counts: std::collections::HashMap::new(),
            active_window_sizes: std::collections::HashMap::new(),
            scrolled_sessions: rustc_hash::FxHashSet::default(),
            split_sessions: rustc_hash::FxHashSet::default(),
            scrolled_windows: rustc_hash::FxHashSet::default(),
        };
        for name in ["a", "b"] {
            let command = ["new-session", "-d", "-s", name, "--", "sh", "-c", "echo hello-$0; exec sleep 300", name];
            world.pair.both("attached", &command)?;
        }
        world.refresh_model()?;
        Ok(world)
    }

    fn refresh_model(&mut self) -> Result<(), String> {
        let sessions = self.pair.reference.body(&["list-sessions", "-F", "#{session_name}"]);
        self.sessions = sessions.lines().map(str::to_owned).collect();
        self.window_counts.clear();
        self.active_window_sizes.clear();
        self.scrolled_sessions.clear();
        self.split_sessions.clear();
        let windows = self.pair.reference.body(&[
            "list-windows",
            "-a",
            "-F",
            "#{session_name} #{window_name} #{window_active} #{window_width}x#{window_height} #{window_panes} #{window_id} #{history_size}",
        ]);
        for line in windows.lines() {
            let fields: Vec<&str> = line.split(' ').collect();
            let [session, name, active, size, panes, id, history] = fields[..] else { continue };
            if panes != "1" {
                self.split_sessions.insert(session.to_owned());
            }
            *self.window_counts.entry(session.to_owned()).or_default() += 1;
            if name == "burst" || history.parse::<usize>().is_ok_and(|count| count > 0) {
                self.scrolled_windows.insert(id.to_owned());
            }
            if self.scrolled_windows.contains(id) {
                self.scrolled_sessions.insert(session.to_owned());
            }
            if active == "1"
                && let Some((columns, rows)) = size.split_once('x')
                && let (Ok(columns), Ok(rows)) = (columns.parse(), rows.parse())
            {
                self.active_window_sizes.insert(session.to_owned(), (columns, rows));
            }
        }
        let panes = self.pair.reference.body(&["list-panes", "-a", "-F", "#{session_name} #{pane_history_size}"]);
        for line in panes.lines() {
            let (session_name, history) = line.split_once(' ').unwrap_or(("", "0"));
            if history.parse::<usize>().is_ok_and(|count| count > 0) {
                self.scrolled_sessions.insert(session_name.to_owned());
            }
        }
        let clients = self.pair.reference.body(&LIST_CLIENTS);
        self.client_sessions = [None, None];
        for line in clients.lines() {
            let mut fields = line.split(' ');
            let (Some(session), Some(size)) = (fields.next(), fields.next()) else {
                continue;
            };
            let width: u64 = size.split('x').next().unwrap_or("0").parse().unwrap_or(0);
            self.client_sessions[slot_of(width)] = Some(session.to_owned());
        }
        Ok(())
    }

    fn per_impl(&self, arguments: &[&str], reference_arguments: &[&str]) -> Result<(), String> {
        let tm = self.pair.tm.outcome(arguments);
        let reference = self.pair.reference.outcome(reference_arguments);
        check_command(INTENTIONAL_DIFFERENCES, "attached", arguments, &tm, &reference)
    }

    fn reap(&mut self, slot: usize, killed: Option<AttachedExclusion>) -> Result<(), String> {
        let Some(mut seat) = self.seats[slot].take() else {
            return Ok(());
        };
        self.client_sizes[slot] = None;
        let tm_status = seat.tm.finish(SPAWN_TIMEOUT);
        let reference_status = seat.reference.finish(SPAWN_TIMEOUT);
        if killed == Some(AttachedExclusion::KilledClientTerminalRestore) {
            return match parity::observation_difference(
                &format!("client slot {slot} exit status ({})", AttachedExclusion::KilledClientTerminalRestore.reason()),
                &tm_status,
                &reference_status,
            ) {
                Some(difference) => Err(difference),
                None => Ok(()),
            };
        }
        let tm_exit = (
            tm_status,
            seat.tm.screen_rows().iter().map(|row| row.trim_end().to_owned()).collect::<Vec<_>>(),
            seat.tm.modes(),
            seat.tm.termios(),
        );
        let reference_exit = (
            reference_status,
            seat.reference.screen_rows().iter().map(|row| row.trim_end().to_owned()).collect::<Vec<_>>(),
            seat.reference.modes(),
            seat.reference.termios(),
        );
        match parity::observation_difference(
            &format!("client slot {slot} exit status, final screen, outer modes and termios"),
            &tm_exit,
            &reference_exit,
        ) {
            Some(difference) => Err(difference),
            None => Ok(()),
        }
    }

    fn converge_difference(&self) -> Option<String> {
        for observation in ATTACHED_OBSERVATIONS {
            let (tm, reference) = (
                self.pair.tm.outcome(observation),
                self.pair.reference.outcome(observation),
            );
            if tm.status != reference.status || tm.stderr != reference.stderr {
                return Some(format!("{observation:?}: status or stderr differ\n tm: {tm:?}\nref: {reference:?}"));
            }
            if let Some(difference) = parity::observation_difference(
                &format!("{observation:?} ({})", AttachedExclusion::UnattachedWindowSize.reason()),
                &mask_unattached_window_sizes(observation, &tm.stdout),
                &mask_unattached_window_sizes(observation, &reference.stdout),
            ) {
                return Some(difference);
            }
        }
        let tm = self.pair.tm.outcome(&LIST_CLIENTS);
        let reference = self.pair.reference.outcome(&LIST_CLIENTS);
        if let Some(difference) = parity::observation_difference(
            "list-clients",
            &sort_lines(&tm.stdout),
            &sort_lines(&reference.stdout),
        ) {
            return Some(difference);
        }
        for (slot, seat) in self.seats.iter().enumerate() {
            let Some(seat) = seat else { continue };
            if let Some(difference) = parity::observation_difference(
                &format!("client slot {slot} closed"),
                &seat.tm.is_closed(),
                &seat.reference.is_closed(),
            ) {
                return Some(difference);
            }
            if seat.tm.is_closed() {
                continue;
            }
            if let Some(difference) = screen_difference(slot, &seat.tm, &seat.reference) {
                return Some(difference);
            }
        }
        None
    }

    fn converge(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + CONVERGE_TIMEOUT;
        while let Some(difference) = self.converge_difference() {
            if Instant::now() >= deadline {
                return Err(difference);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        for slot in 0..2 {
            if self.seats[slot].as_ref().is_some_and(|seat| seat.tm.is_closed()) {
                self.reap(slot, None)?;
            }
        }
        self.refresh_model()
    }

    fn applicable(&self, op: &AttachedOp) -> bool {
        let attached = self.seats[op.slot].is_some();
        let exists = |name: &str| self.sessions.iter().any(|session| session == name);
        match op.kind {
            AttachedKind::Attach => !attached && exists(&op.text),
            AttachedKind::DetachKey
            | AttachedKind::DetachCommand
            | AttachedKind::ResizeClient
            | AttachedKind::ClientCommand => attached,
            AttachedKind::SwitchClient => attached && exists(&op.text),
            AttachedKind::KillClientRendering | AttachedKind::DetachDuringRedraw => {
                attached && self.client_sessions[op.slot].as_deref().is_some_and(exists)
            }
            AttachedKind::OutsideCommand | AttachedKind::NewSession => true,
            AttachedKind::ChildExit | AttachedKind::WindowFlood => exists(&op.text),
            AttachedKind::InPane => exists(op.text.split(' ').next().unwrap_or_default()),
        }
    }

    fn in_pane(&self, text: &str) -> Result<(), String> {
        let mut parts = text.splitn(3, ' ');
        let session = parts.next().ok_or("in-pane operation names no session")?;
        let mode = parts.next().ok_or("in-pane operation names no mode")?;
        let command = parts.next().ok_or("in-pane operation names no command")?;
        let other = self.sessions.iter().find(|name| *name != session).map_or("zz", String::as_str);
        let command = command.replace("{other}", other);
        let words: Vec<&str> = command.split(' ').collect();
        let target = format!("{session}:");
        let mut open = vec!["new-window", "-d", "-t", &target, "-n", "i", "-P", "-F", "#{pane_id}", "--"];
        open.extend(parity::SILENT_SHELL);
        let (opened, _) = self.pair.both("attached", &open)?;
        let pane = String::from_utf8_lossy(&opened.stdout).trim().to_owned();
        let killed = mode == "killed";
        for mux in [&self.pair.tm, &self.pair.reference] {
            mux.type_in_pane(&pane, &words, killed);
        }
        if killed {
            self.pair.both("attached", &["kill-pane", "-t", &pane])?;
            for mux in [&self.pair.tm, &self.pair.reference] {
                mux.release_in_pane();
            }
        }
        let outputs = [
            self.pair.tm.in_pane_output(IN_PANE_TIMEOUT)?,
            self.pair.reference.in_pane_output(IN_PANE_TIMEOUT)?,
        ];
        if outputs[0] != outputs[1] {
            return Err(format!(
                "in-pane {mode} {command:?} from {session} ({IN_PANE_FORMAT})\n tm: {:?}\nref: {:?}",
                outputs[0], outputs[1]
            ));
        }
        if !killed {
            let (active, _) = self.pair.both("attached", &["display-message", "-p", "-t", &pane, "#{window_active}"])?;
            if active.stdout == b"0\n" {
                self.pair.both("attached", &["kill-pane", "-t", &pane])?;
            }
        }
        Ok(())
    }

    fn apply(&mut self, op: &AttachedOp) -> Result<(), String> {
        if !self.applicable(op) {
            return Ok(());
        }
        let slot = op.slot;
        match op.kind {
            AttachedKind::Attach => {
                let (rows, columns) = op.size;
                let arguments = ["attach-session", "-t", op.text.as_str()];
                let tm = RawClient::spawn(&self.pair.tm, "tm", &arguments, rows, columns);
                let reference = RawClient::spawn(&self.pair.reference, "reference", &arguments, rows, columns);
                let needle = format!("({})", op.text);
                tm.wait_for_text(&needle, SPAWN_TIMEOUT);
                reference.wait_for_text(&needle, SPAWN_TIMEOUT);
                self.seats[slot] = Some(Seat { tm, reference });
                self.client_sizes[slot] = Some(op.size);
            }
            AttachedKind::DetachKey | AttachedKind::DetachCommand => {
                if op.kind == AttachedKind::DetachKey {
                    let seat = self.seats[slot].as_mut().expect("applicable");
                    seat.tm.write(b"\x01d");
                    seat.reference.write(b"\x01d");
                } else {
                    let tm_name = client_name(&self.pair.tm, slot).ok_or("tm lists no such client")?;
                    let reference_name =
                        client_name(&self.pair.reference, slot).ok_or("reference lists no such client")?;
                    self.per_impl(&["detach-client", "-t", &tm_name], &["detach-client", "-t", &reference_name])?;
                }
                self.reap(slot, None)?;
            }
            AttachedKind::SwitchClient => {
                let tm_name = client_name(&self.pair.tm, slot).ok_or("tm lists no such client")?;
                let reference_name =
                    client_name(&self.pair.reference, slot).ok_or("reference lists no such client")?;
                self.per_impl(
                    &["switch-client", "-c", &tm_name, "-t", &op.text],
                    &["switch-client", "-c", &reference_name, "-t", &op.text],
                )?;
            }
            AttachedKind::ClientCommand => {
                let seat = self.seats[slot].as_mut().expect("applicable");
                let mut bytes = op.text.as_bytes().to_vec();
                bytes.push(b'\r');
                for client in [&mut seat.tm, &mut seat.reference] {
                    client.write(b"\x01/");
                    client.wait_for_screen("command prompt", SPAWN_TIMEOUT, |screen| {
                        let rows = screen.size().0.get();
                        screen.rows(0, screen.size().1.get()).nth(usize::from(rows) - 1).is_some_and(|row| row.starts_with(':'))
                    });
                    client.write(&bytes);
                }
            }
            AttachedKind::OutsideCommand => {
                let words: Vec<&str> = op.text.split(' ').collect();
                self.pair.both("attached", &words)?;
            }
            AttachedKind::ResizeClient => {
                let seat = self.seats[slot].as_ref().expect("applicable");
                seat.tm.resize(op.size.0, op.size.1);
                seat.reference.resize(op.size.0, op.size.1);
                self.client_sizes[slot] = Some(op.size);
            }
            AttachedKind::NewSession => {
                let (name, script) = op.text.split_once(' ').expect("session and script");
                self.pair.both("attached", &["new-session", "-d", "-s", name, "--", "sh", "-c", script])?;
            }
            AttachedKind::ChildExit => {
                let target = format!("{}:", op.text);
                self.pair.both("attached", &["send-keys", "-t", &target, "go", "Enter"])?;
            }
            AttachedKind::WindowFlood => {
                let target = format!("{}:", op.text);
                self.pair.both("attached", &["new-window", "-t", &target, "-n", "flood", "--", "sh", "-c", "seq 1 150; exit"])?;
            }
            AttachedKind::InPane => self.in_pane(&op.text)?,
            AttachedKind::KillClientRendering | AttachedKind::DetachDuringRedraw => {
                let session = self.client_sessions[slot].clone().expect("applicable");
                let target = format!("{session}:");
                let mut flood = vec!["new-window", "-t", &target, "-n", "burst"];
                flood.extend(FLOOD);
                self.pair.both("attached", &flood)?;
                let seat = self.seats[slot].as_mut().expect("applicable");
                if op.kind == AttachedKind::KillClientRendering {
                    seat.tm.signal(libc::SIGKILL);
                    seat.reference.signal(libc::SIGKILL);
                    self.reap(slot, Some(AttachedExclusion::KilledClientTerminalRestore))?;
                } else {
                    seat.tm.write(b"\x01d");
                    seat.reference.write(b"\x01d");
                    self.reap(slot, None)?;
                }
            }
        }
        self.converge()
    }

    fn dump(&self) -> String {
        let mut text = String::new();
        for (slot, seat) in self.seats.iter().enumerate() {
            if let Some(seat) = seat {
                text.push_str(&format!(
                    "slot {slot} tm:\n{}\nslot {slot} ref:\n{}\n",
                    seat.tm.screen_rows().join("\n"),
                    seat.reference.screen_rows().join("\n")
                ));
            }
        }
        let commands: Vec<&[&str]> = ATTACHED_OBSERVATIONS.iter().copied().chain([&LIST_CLIENTS[..]]).collect();
        let observations: Vec<(Outcome, Outcome)> = commands
            .iter()
            .map(|command| (self.pair.tm.outcome(command), self.pair.reference.outcome(command)))
            .collect();
        text.push_str(&history::observation_text(&observations, &commands));
        text
    }
}

fn attached_exclusion(op: &AttachedOp, world: &World) -> Option<AttachedExclusion> {
    let other = 1 - op.slot;
    let (session, size) = match op.kind {
        AttachedKind::Attach | AttachedKind::SwitchClient => (Some(op.text.clone()), op.size),
        AttachedKind::ResizeClient => (world.client_sessions[op.slot].clone(), op.size),
        AttachedKind::WindowFlood | AttachedKind::ChildExit => {
            return (world.window_counts.get(&op.text).copied().unwrap_or(0) >= 2)
                .then_some(AttachedExclusion::WindowRemovalFromHistoryStack);
        }
        _ => return None,
    };
    let session = session?;
    let size = if op.kind == AttachedKind::SwitchClient {
        world.client_sizes[op.slot].unwrap_or(size)
    } else {
        size
    };
    if world.client_sessions[other].as_ref() == Some(&session) && Some(size) != world.client_sizes[other] {
        return Some(AttachedExclusion::SharedSessionClientSizes);
    }
    let window_size = (size.1, size.0.saturating_sub(1));
    let resizes = world.active_window_sizes.get(&session).is_some_and(|current| *current != window_size);
    if resizes && world.scrolled_sessions.contains(&session) {
        return Some(AttachedExclusion::ResizeOfScrolledWindow);
    }
    if resizes && world.split_sessions.contains(&session) {
        return Some(AttachedExclusion::ResizeOfSplitWindow);
    }
    None
}

fn pick_attached_kind(rng: &mut Rng) -> AttachedKind {
    let total: u64 = ATTACHED_WEIGHTS.iter().map(|(_, weight)| weight).sum();
    let mut roll = rng.below(total);
    for (kind, weight) in ATTACHED_WEIGHTS {
        if roll < weight {
            return kind;
        }
        roll -= weight;
    }
    unreachable!("weights cover the roll")
}

fn generate_attached(rng: &mut Rng, world: &World, counter: &mut u32) -> Option<AttachedOp> {
    let sessions: Vec<&String> = world.sessions.iter().collect();
    for _ in 0..40 {
        let kind = pick_attached_kind(rng);
        let slot = rng.index(2);
        let size = *rng.pick(&SLOT_SIZES[slot]);
        *counter += 1;
        let n = *counter;
        let session = if sessions.is_empty() { "a".to_owned() } else { (*rng.pick(&sessions)).clone() };
        let attached_session = world.client_sessions[slot].clone().unwrap_or_else(|| session.clone());
        let text = match kind {
            AttachedKind::Attach | AttachedKind::SwitchClient => session,
            AttachedKind::DetachKey
            | AttachedKind::DetachCommand
            | AttachedKind::ResizeClient
            | AttachedKind::KillClientRendering
            | AttachedKind::DetachDuringRedraw => String::new(),
            AttachedKind::ClientCommand => match *rng.pick(&PROMPT_COMMANDS) {
                "new-window" => format!("new-window -n p{n} -- sleep 300"),
                "rename-window" => format!("rename-window r{n}"),
                "split-window" => "split-window -d -- sleep 300".to_owned(),
                "rename-session" => format!("rename-session s{n}"),
                other => other.to_owned(),
            },
            AttachedKind::OutsideCommand => match rng.below(3) {
                0 => format!("new-window -d -n o{n} -- sleep 300"),
                1 => format!("rename-window o{n}"),
                _ => "split-window -d -- sleep 300".to_owned(),
            },
            AttachedKind::NewSession => {
                if rng.chance(1, 2) {
                    format!("e{n} read x; seq 1 120")
                } else {
                    format!("n{n} echo hello-n{n}; exec sleep 300")
                }
            }
            AttachedKind::ChildExit => {
                let readers: Vec<&&String> = sessions.iter().filter(|name| name.starts_with('e')).collect();
                if readers.is_empty() { session } else { (**rng.pick(&readers)).clone() }
            }
            AttachedKind::WindowFlood => attached_session,
            AttachedKind::InPane => {
                let mode = if rng.chance(1, 4) { "killed" } else { "alive" };
                let command = rng.pick(&IN_PANE_COMMANDS).replace("{n}", &n.to_string());
                format!("{session} {mode} {command}")
            }
        };
        let op = AttachedOp { kind, slot, text, size };
        if world.applicable(&op) && attached_exclusion(&op, world).is_none() {
            return Some(op);
        }
    }
    None
}

fn render_attached(ops: &[AttachedOp]) -> String {
    ops.iter()
        .enumerate()
        .map(|(index, op)| format!("  {index:>2} {:?} slot {} {:?} {:?}", op.kind, op.slot, op.text, op.size))
        .collect::<Vec<_>>()
        .join("\n")
}

enum Replay {
    Passed,
    Excluded,
    Failed(Failure, String),
}

fn replay_attached(label: &str, ops: &[AttachedOp]) -> Replay {
    let mut world = match history::catching(|| World::new(label)) {
        Ok(world) => world,
        Err(message) => return Replay::Failed(Failure { step: 0, message }, String::new()),
    };
    for (step, op) in ops.iter().enumerate() {
        if world.applicable(op) && attached_exclusion(op, &world).is_some() {
            return Replay::Excluded;
        }
        if let Err(message) = history::catching(|| world.apply(op)) {
            return Replay::Failed(Failure { step, message }, world.dump());
        }
    }
    Replay::Passed
}

fn run_attached_seed(seed: u64) -> Result<Vec<AttachedKind>, String> {
    let mut rng = Rng::new(seed);
    let length = 14 + rng.below(8) as usize;
    let label = format!("attached-{seed}");
    let mut ops = Vec::new();
    let mut failure = None;
    {
        let mut world = World::new(&label)?;
        let mut counter = 0;
        for step in 0..length {
            let Some(op) = generate_attached(&mut rng, &world, &mut counter) else {
                continue;
            };
            ops.push(op.clone());
            if let Err(message) = history::catching(|| world.apply(&op)) {
                failure = Some((Failure { step, message }, world.dump()));
                break;
            }
        }
    }
    let Some((failure, dump)) = failure else {
        return Ok(ops.iter().map(|op| op.kind).collect());
    };
    let reduced = history::ddmin(&ops, &mut |subset| {
        matches!(replay_attached(&format!("{label}-shrink"), subset), Replay::Failed(..))
    });
    let (reduced_failure, reduced_dump) = match replay_attached(&format!("{label}-final"), &reduced) {
        Replay::Failed(failure, dump) => (failure, dump),
        _ => (
            Failure {
                step: failure.step,
                message: "reduced history no longer fails".to_owned(),
            },
            String::new(),
        ),
    };
    let pair = Pair::new(&format!("{label}-version"));
    Err(format!(
        "attached history seed {seed} diverged at step {} ({:?})\n{}\n\nfull operations:\n{}\n\nobservations at the failure:\n{dump}\nreduced operations ({} of {}):\n{}\n\nreduced failure at step {}: {}\nreduced observations:\n{reduced_dump}\nreference: {}\nreplay: TM_HISTORY_SEED={seed} cargo test --features parity --test tmux_histories attached",
        failure.step,
        ops[failure.step],
        failure.message,
        render_attached(&ops),
        reduced.len(),
        ops.len(),
        render_attached(&reduced),
        reduced_failure.step,
        reduced_failure.message,
        history::reference_version(&pair),
    ))
}

#[test]
fn attached_histories_match_the_reference() {
    let started = Instant::now();
    let seeds = history::seeds(&(0..ATTACHED_SEEDS).collect::<Vec<_>>());
    let next = AtomicUsize::new(0);
    let results = std::sync::Mutex::new(rustc_hash::FxHashMap::default());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&seed) = seeds.get(index) else {
                        return;
                    };
                    let result = run_attached_seed(seed);
                    results.lock().expect("results").insert(seed, result);
                }
            });
        }
    });
    let results = results.into_inner().expect("results");
    let failures: Vec<&String> = by_seed(&results)
        .into_iter()
        .filter_map(|(_, result)| result.as_ref().err())
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} attached histories diverged\n\n{}",
        failures.len(),
        seeds.len(),
        failures
            .iter()
            .map(|failure| failure.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    eprintln!("attached: {} seeds in {:?}", seeds.len(), started.elapsed());
    if std::env::var_os("TM_HISTORY_SEED").is_none() {
        let seen: rustc_hash::FxHashSet<AttachedKind> = by_seed(&results)
            .into_iter()
            .filter(|(seed, _)| *seed < COVERAGE_SEEDS)
            .flat_map(|(_, result)| result.as_ref().expect("checked above").iter().copied())
            .collect();
        let missing: Vec<AttachedKind> = ATTACHED_WEIGHTS
            .iter()
            .map(|(kind, _)| *kind)
            .filter(|kind| !seen.contains(kind))
            .collect();
        assert!(
            missing.is_empty(),
            "attached operations absent from the first {COVERAGE_SEEDS} seeds: {missing:?}"
        );
    }
}

fn assert_attached_history(label: &str, ops: &[AttachedOp]) {
    match replay_attached("reg-attached", ops) {
        Replay::Passed => {}
        Replay::Excluded => panic!("{label}: the history hits an exclusion"),
        Replay::Failed(failure, dump) => panic!(
            "{label} diverged at step {} ({:?}): {}\n{dump}",
            failure.step, ops[failure.step], failure.message
        ),
    }
}

fn op(kind: AttachedKind, slot: usize, text: &str) -> AttachedOp {
    AttachedOp {
        kind,
        slot,
        text: text.to_owned(),
        size: SLOT_SIZES[slot][0],
    }
}

#[test]
fn regression_attached_list_clients_exposes_session_fields() {
    assert_attached_history(
        "list-clients session fields",
        &[op(AttachedKind::Attach, 0, "b"), op(AttachedKind::SwitchClient, 0, "a")],
    );
}

#[test]
fn regression_attached_detach_prints_the_session_and_restores_the_terminal() {
    assert_attached_history(
        "detach by key",
        &[op(AttachedKind::Attach, 0, "a"), op(AttachedKind::DetachKey, 0, "")],
    );
    assert_attached_history(
        "detach-client",
        &[
            op(AttachedKind::Attach, 1, "b"),
            op(AttachedKind::SwitchClient, 1, "a"),
            op(AttachedKind::DetachCommand, 1, ""),
        ],
    );
}

#[test]
fn regression_attached_session_end_prints_exited() {
    assert_attached_history(
        "session child exit",
        &[
            op(AttachedKind::NewSession, 0, "e1 read x; seq 1 120"),
            op(AttachedKind::Attach, 0, "e1"),
            op(AttachedKind::ChildExit, 0, "e1"),
        ],
    );
}

#[test]
fn regression_attached_status_line_has_no_padding_after_the_left_part() {
    assert_attached_history("status line", &[op(AttachedKind::Attach, 0, "a")]);
}

#[test]
fn regression_attached_command_prompt_runs_against_the_client_session() {
    assert_attached_history(
        "command prompt",
        &[
            op(AttachedKind::Attach, 0, "b"),
            op(AttachedKind::ClientCommand, 0, "new-window -n p1 -- sleep 300"),
            op(AttachedKind::ClientCommand, 0, "rename-window r2"),
        ],
    );
}

#[test]
fn regression_attached_window_listings_report_session_attached() {
    assert_attached_history(
        "session_attached",
        &[
            op(AttachedKind::Attach, 0, "a"),
            AttachedOp {
                size: SLOT_SIZES[0][0],
                ..op(AttachedKind::Attach, 1, "a")
            },
        ],
    );
}

#[test]
fn regression_attached_exited_window_selects_the_previous_window() {
    assert_attached_history(
        "window exit",
        &[
            op(AttachedKind::Attach, 0, "a"),
            op(AttachedKind::WindowFlood, 0, "a"),
        ],
    );
}

#[test]
fn regression_attached_killed_client_leaves_the_session_alive() {
    assert_attached_history(
        "client SIGKILL",
        &[
            op(AttachedKind::Attach, 0, "b"),
            op(AttachedKind::KillClientRendering, 0, ""),
            op(AttachedKind::Attach, 0, "b"),
            op(AttachedKind::DetachDuringRedraw, 0, ""),
        ],
    );
}
