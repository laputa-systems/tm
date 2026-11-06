use std::panic::{AssertUnwindSafe, catch_unwind};

use super::parity::{
    INTENTIONAL_DIFFERENCES, Mux, Outcome, Scratch, check_command, observation_difference,
};

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mixed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
        Self(if mixed == 0 { 1 } else { mixed })
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "empty range");
        self.next() % bound
    }

    pub fn index(&mut self, len: usize) -> usize {
        self.below(len as u64) as usize
    }

    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.index(items.len())]
    }
}

pub fn seeds(defaults: &[u64]) -> Vec<u64> {
    match std::env::var("TM_HISTORY_SEED") {
        Ok(value) => vec![
            value
                .trim()
                .parse()
                .unwrap_or_else(|error| panic!("TM_HISTORY_SEED={value:?}: {error}")),
        ],
        Err(std::env::VarError::NotPresent) => defaults.to_vec(),
        Err(error) => panic!("TM_HISTORY_SEED: {error}"),
    }
}

pub fn ddmin<T: Clone>(items: &[T], fails: &mut dyn FnMut(&[T]) -> bool) -> Vec<T> {
    let mut current = items.to_vec();
    let mut parts = 2;
    while current.len() >= 2 {
        let chunk = current.len().div_ceil(parts);
        let mut reduced = false;
        let mut start = 0;
        while start < current.len() {
            let end = (start + chunk).min(current.len());
            let complement: Vec<T> = current[..start]
                .iter()
                .chain(&current[end..])
                .cloned()
                .collect();
            if !complement.is_empty() && fails(&complement) {
                current = complement;
                parts = (parts - 1).max(2);
                reduced = true;
                break;
            }
            start = end;
        }
        if !reduced {
            if parts >= current.len() {
                break;
            }
            parts = (parts * 2).min(current.len());
        }
    }
    current
}

pub fn reference_pair(label: &str) -> Pair {
    let scratch = Scratch::new(label);
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    Pair {
        tm,
        reference,
        _scratch: scratch,
    }
}

pub struct Pair {
    pub tm: Mux,
    pub reference: Mux,
    _scratch: Scratch,
}

impl Pair {
    pub fn new(label: &str) -> Self {
        reference_pair(label)
    }

    pub fn both(&self, test: &str, arguments: &[&str]) -> Result<(Outcome, Outcome), String> {
        let tm = self.tm.outcome(arguments);
        let reference = self.reference.outcome(arguments);
        check_command(INTENTIONAL_DIFFERENCES, test, arguments, &tm, &reference)?;
        Ok((tm, reference))
    }

    pub fn observe(
        &self,
        test: &str,
        commands: &[&[&str]],
    ) -> Result<Vec<(Outcome, Outcome)>, String> {
        commands
            .iter()
            .map(|arguments| self.both(test, arguments))
            .collect()
    }
}

pub fn observation_text(observations: &[(Outcome, Outcome)], commands: &[&[&str]]) -> String {
    let mut text = String::new();
    for ((tm, reference), arguments) in observations.iter().zip(commands) {
        for (who, outcome) in [("tm ", tm), ("ref", reference)] {
            text.push_str(&format!(
                "{who} {arguments:?} -> {:?}\n{}{}",
                outcome.status,
                String::from_utf8_lossy(&outcome.stdout),
                String::from_utf8_lossy(&outcome.stderr)
            ));
        }
    }
    text
}

pub fn same_observations(
    what: &str,
    before: &[(Outcome, Outcome)],
    after: &[(Outcome, Outcome)],
) -> Option<String> {
    let project = |pairs: &[(Outcome, Outcome)]| {
        pairs
            .iter()
            .map(|(tm, reference)| (tm.stdout.clone(), reference.stdout.clone()))
            .collect::<Vec<_>>()
    };
    observation_difference(what, &project(after), &project(before))
}

pub fn catching<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(result) => result,
        Err(payload) => Err(payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_owned()))
            .unwrap_or_else(|| "panic".to_owned())),
    }
}

pub fn reference_version(pair: &Pair) -> String {
    let output = std::process::Command::new(pair.reference.binary())
        .arg("-V")
        .output()
        .expect("run reference -V");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
