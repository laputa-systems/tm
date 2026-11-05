#![cfg(unix)]

#[path = "support/history.rs"]
#[allow(dead_code)]
mod history;
#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use history::{Pair, Rng};
use parity::{ExitStatus, Outcome};

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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
    let results = std::sync::Mutex::new(BTreeMap::new());
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
    let failures: Vec<&String> = results
        .values()
        .filter_map(|result| result.as_ref().err())
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
        let seen: BTreeSet<OpKind> = results
            .iter()
            .filter(|(seed, _)| **seed < COVERAGE_SEEDS)
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
