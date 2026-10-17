//! Autopilot: Run the board once an hour while it is on, and off again by
//! itself after `RUNS` finished runs, so it is switched on each morning. A
//! run only starts while none is working and the last one is settled: a
//! failed run or one stopped after an hour on a prompt does not count, and
//! `FAILS` of those in a row switch it off.
//!
//! `autopilot.json` is shared by every copy of the app: read-modify-write,
//! and a copy only starts a run after it wrote that run down.

use serde::{Deserialize, Serialize};

/// Runs before it switches itself off: a working day.
pub const RUNS: u32 = 8;
/// Between runs.
pub const EVERY_MS: u64 = 60 * 60 * 1000;
/// Failed or stuck runs in a row before it switches itself off.
pub const FAILS: u32 = 3;
const FILE: &str = "autopilot.json";

/// How the started run ended, from its session's hook reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Its turn ended: it reported.
    Finished,
    /// Claude Code stopped it with this error.
    Failed(String),
    /// Waiting on a permission or a question for longer than a run's hour.
    Stuck,
}

/// The outcome of the run started at `since_ms`, or `None` while it is
/// still going. Reports from before it are the last run's.
pub fn outcome(reports: &[crate::claude::status::Report], since_ms: u64) -> Option<Outcome> {
    use crate::claude::status::State;
    let since = std::time::UNIX_EPOCH + std::time::Duration::from_millis(since_ms);
    let last = reports
        .iter()
        .rev()
        .find(|r| !matches!(r.state, State::Pr | State::Ended))
        .filter(|r| r.at >= since)?;
    let waited = last
        .at
        .elapsed()
        .is_ok_and(|age| age.as_millis() as u64 >= EVERY_MS);
    match last.state {
        State::Turn => Some(Outcome::Finished),
        State::Failed => Some(Outcome::Failed(last.text.clone())),
        State::NeedsApproval | State::NeedsDecision if waited => Some(Outcome::Stuck),
        _ => None,
    }
}

/// The Mac notification for a run that did not finish; a login error says
/// how to fix it.
pub fn failure_line(outcome: &Outcome, off: bool) -> String {
    let what = match outcome {
        Outcome::Failed(error) if error.contains("auth") || error.contains("login") => {
            format!("The board run failed ({error}): run `claude /login` in a terminal.")
        }
        Outcome::Failed(error) => format!("The board run failed ({error})."),
        Outcome::Stuck | Outcome::Finished => {
            "The board run waited an hour on a prompt; it was stopped.".to_owned()
        }
    };
    match off {
        true => format!("Autopilot is off after {FAILS} runs that did not finish. {what}"),
        false => format!("Autopilot: {what} It did not count; the next run starts in an hour."),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Autopilot {
    pub on: bool,
    /// Runs since it was switched on.
    pub runs: u32,
    /// When the last run started, unix millis; 0 runs at once.
    pub last_ms: u64,
    /// The last run started and is not settled yet.
    pub pending: bool,
    /// Runs in a row that failed or got stuck.
    pub failed: u32,
}

impl Autopilot {
    /// Whether a run is due at `now_ms`.
    pub fn due(&self, now_ms: u64) -> bool {
        let waited = self.last_ms == 0 || now_ms.saturating_sub(self.last_ms) >= EVERY_MS;
        self.on && self.runs < RUNS && waited && !self.pending
    }

    /// Writes the run's start down; it counts once it is settled.
    pub fn ran(&mut self, now_ms: u64) {
        self.last_ms = now_ms;
        self.pending = true;
    }

    /// Counts a finished run (the last switches it off); a failed or stuck
    /// one only counts towards `FAILS`.
    pub fn settle(&mut self, outcome: &Outcome) {
        self.pending = false;
        if *outcome == Outcome::Finished {
            self.runs += 1;
            self.failed = 0;
        } else {
            self.failed += 1;
        }
        self.on = self.on && self.runs < RUNS && self.failed < FAILS;
    }
}

/// Settles the pending run: re-reads the file (another copy may have
/// settled it) and writes the outcome. The new state when this copy did.
pub fn settle(outcome: &Outcome) -> Option<Autopilot> {
    let mut state = load();
    if !state.pending {
        return None;
    }
    state.settle(outcome);
    save(&state).ok()?;
    Some(state)
}

pub fn load() -> Autopilot {
    crate::store::load(FILE)
}

pub fn save(state: &Autopilot) -> std::io::Result<()> {
    crate::store::save(FILE, state)
}

/// Switched on: eight fresh runs, the first at once. Off: stays off.
pub fn switch(on: bool) -> std::io::Result<Autopilot> {
    let state = match on {
        true => Autopilot {
            on: true,
            ..Default::default()
        },
        false => Autopilot {
            on: false,
            ..load()
        },
    };
    save(&state)?;
    Ok(state)
}

/// Takes the run that is due now, if any: re-reads the file (another copy
/// may have taken it) and writes the run down before it is started.
// ponytail: two copies checking in the same instant can both take it; the
// busy check on the orchestrator stops the second, a lock file if not.
pub fn take(now_ms: u64) -> Option<Autopilot> {
    let mut state = load();
    if !state.due(now_ms) {
        return None;
    }
    state.ran(now_ms);
    save(&state).ok()?;
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_hourly_and_switches_itself_off_after_the_last() {
        let mut a = Autopilot {
            on: true,
            ..Default::default()
        };
        assert!(a.due(5), "the first run is at once");
        a.ran(5);
        assert!(!a.due(5 + EVERY_MS), "not while the last is unsettled");
        a.settle(&Outcome::Finished);
        assert!(!a.due(5 + EVERY_MS - 1));
        assert!(a.due(5 + EVERY_MS));
        for i in 2..=RUNS {
            a.ran(i as u64 * EVERY_MS);
            a.settle(&Outcome::Finished);
        }
        assert_eq!(a.runs, RUNS);
        assert!(!a.on);
        assert!(!a.due(u64::MAX));
        assert!(!Autopilot::default().due(u64::MAX), "off never runs");
    }

    #[test]
    fn a_failed_or_stuck_run_does_not_count_and_three_in_a_row_switch_it_off() {
        let mut a = Autopilot {
            on: true,
            ..Default::default()
        };
        a.ran(1);
        a.settle(&Outcome::Failed("server_error".into()));
        assert_eq!((a.runs, a.failed, a.on), (0, 1, true));
        a.ran(2);
        a.settle(&Outcome::Finished);
        assert_eq!(
            (a.runs, a.failed),
            (1, 0),
            "a finished run resets the fails"
        );
        for i in 0..FAILS {
            a.ran(3 + i as u64);
            a.settle(&Outcome::Stuck);
        }
        assert!(!a.on);
        let login = failure_line(&Outcome::Failed("authentication_failed".into()), false);
        assert!(login.contains("claude /login"), "{login}");
        assert!(failure_line(&Outcome::Stuck, true).starts_with("Autopilot is off"));
    }

    #[test]
    fn the_run_is_read_from_its_last_report() {
        use crate::claude::status::{Report, State};
        use std::time::{Duration, SystemTime};
        let at = |ago: u64, state, text: &str| Report {
            at: SystemTime::now() - Duration::from_secs(ago),
            state,
            text: text.into(),
        };
        let working = at(10, State::Working, "/orchestrate");
        let start = |ago: u64| {
            (SystemTime::now() - Duration::from_secs(ago))
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
        };
        let outcome = |r: &[Report]| outcome(r, start(4000));
        assert_eq!(outcome(std::slice::from_ref(&working)), None);
        // The last run's report, before this one's first: still going.
        assert_eq!(
            super::outcome(&[at(5, State::Turn, "## Ready")], start(1)),
            None
        );
        assert_eq!(
            outcome(&[
                working.clone(),
                at(5, State::Turn, "## Ready"),
                at(1, State::Ended, "")
            ]),
            Some(Outcome::Finished)
        );
        assert_eq!(
            outcome(&[
                working.clone(),
                at(5, State::Failed, "authentication_failed")
            ]),
            Some(Outcome::Failed("authentication_failed".into()))
        );
        assert_eq!(
            outcome(&[working.clone(), at(60, State::NeedsApproval, "")]),
            None
        );
        assert_eq!(
            outcome(&[working, at(3601, State::NeedsApproval, "")]),
            Some(Outcome::Stuck)
        );
    }
}
