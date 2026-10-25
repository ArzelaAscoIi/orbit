//! Orbit's own background sessions that nobody needs are stopped, quietly.
//!
//! Every ticket, PR, chat and run gets a `claude --bg` session, and Claude
//! Code keeps each one alive until it is stopped: weeks of work leave dozens
//! of idle processes holding gigabytes. `claude stop` ends the process and
//! keeps the transcript, and opening the session again resumes it, so a
//! session nobody looks at costs nothing to stop.
//!
//! Stopped: a Claude background session Orbit started (its key is saved),
//! running but not working (your turn, idle, waiting, blocked on a question
//! or a permission), not in front of the user (no terminal attached), not
//! protected (the board run or the clone's pass while on, a line waiting to
//! be typed), and quiet for `idle` (settings.json `idleSessionHours`,
//! default 12), or for an hour once its ticket or PR is done.
//! Never a session Orbit did not start, an interactive one, Codex or pi.
//! settings.json `keepIdleSessions` turns it off.

use std::path::Path;
use std::time::{Duration, SystemTime};

use super::agents::Live;
use super::status::Report;
use super::{Agent, Saved};

/// How long a session whose ticket or PR is done may sit before it stops.
pub const AFTER_DONE: Duration = Duration::from_secs(3600);

/// What decides one session.
#[derive(Clone, Debug)]
pub struct Seen {
    pub working: bool,
    pub in_front: bool,
    pub protected: bool,
    pub finished: bool,
    /// Its last sign of life: the newest report, the user's last look, or
    /// its start.
    pub last: SystemTime,
}

pub fn should_stop(s: &Seen, now: SystemTime, idle: Duration) -> bool {
    if s.working || s.in_front || s.protected {
        return false;
    }
    let quiet = now.duration_since(s.last).unwrap_or_default();
    quiet >= idle || (s.finished && quiet >= AFTER_DONE)
}

/// The hours setting as a duration; 0 means the default 12.
pub fn idle_after(hours: u32) -> Duration {
    Duration::from_secs(u64::from(if hours == 0 { 12 } else { hours }) * 3600)
}

/// The session's last sign of life.
pub fn last_activity(saved: &Saved, reports: &[Report], seen: Option<SystemTime>) -> SystemTime {
    let started = saved
        .started_at
        .parse::<jiff::Timestamp>()
        .map_or(SystemTime::UNIX_EPOCH, SystemTime::from);
    reports
        .iter()
        .map(|r| r.at)
        .chain(seen)
        .fold(started, SystemTime::max)
}

/// Claude Code's row says the session runs in the background and is not on
/// a turn right now.
pub fn idle_row(row: &Live) -> bool {
    row.kind == "background" && row.running() && !matches!(row.label(), "working" | "starting")
}

/// The sessions `orbit sessions-stop` would stop: Orbit's own Claude
/// background sessions quiet for `idle`. A program outside the app cannot
/// see what is in front or which tickets are done, so only the idle rule
/// applies there. (key, short id) pairs.
pub fn plan_cli(
    saved: &std::collections::BTreeMap<String, Saved>,
    rows: &[Live],
    reports: impl Fn(&str) -> Vec<Report>,
    now: SystemTime,
    idle: Duration,
) -> Vec<(String, String)> {
    saved
        .iter()
        .filter(|(_, s)| s.agent == Agent::Claude)
        .filter_map(|(key, s)| {
            let row = rows.iter().find(|r| r.session_id == s.session_id)?;
            let seen = Seen {
                working: !idle_row(row),
                in_front: false,
                protected: false,
                finished: false,
                last: last_activity(s, &reports(key), None),
            };
            should_stop(&seen, now, idle).then(|| {
                let short = row.id.clone().unwrap_or_else(|| s.short_id().to_owned());
                (key.clone(), short)
            })
        })
        .collect()
}

/// One line per stop in `<data>/sessions-stopped.log`.
pub fn log(data: &Path, key: &str, short: &str) {
    log_line(data, &format!("stopped {key} ({short}): idle"));
}

pub fn log_line(data: &Path, what: &str) {
    use std::io::Write;
    let line = format!("{} {what}\n", jiff::Timestamp::now());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("sessions-stopped.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = 3600;

    fn seen(hours_ago: u64) -> Seen {
        Seen {
            working: false,
            in_front: false,
            protected: false,
            finished: false,
            last: SystemTime::now() - Duration::from_secs(hours_ago * H),
        }
    }

    #[test]
    fn idle_sessions_stop_after_the_set_hours_and_busy_ones_never() {
        let (now, idle) = (SystemTime::now(), idle_after(0));
        assert_eq!(idle, Duration::from_secs(12 * H));
        assert!(!should_stop(&seen(11), now, idle), "11 h: kept");
        assert!(should_stop(&seen(13), now, idle), "13 h: stopped");
        let working = Seen {
            working: true,
            ..seen(48)
        };
        assert!(!should_stop(&working, now, idle));
        let in_front = Seen {
            in_front: true,
            ..seen(48)
        };
        assert!(!should_stop(&in_front, now, idle));
        let board_run = Seen {
            protected: true,
            ..seen(48)
        };
        assert!(!should_stop(&board_run, now, idle));
        let done = Seen {
            finished: true,
            ..seen(2)
        };
        assert!(should_stop(&done, now, idle), "done ticket, 2 h quiet");
        let just_done = Seen {
            finished: true,
            ..seen(0)
        };
        assert!(!should_stop(&just_done, now, idle));
        assert_eq!(idle_after(3), Duration::from_secs(3 * H));
    }

    fn saved(id: &str, agent: Agent, hours_ago: u64) -> Saved {
        let at = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(hours_ago as i64);
        Saved {
            agent,
            identifier: "ABC-1".into(),
            title: String::new(),
            session_id: id.into(),
            cwd: "/tmp".into(),
            started_at: at.to_string(),
            deny: Vec::new(),
        }
    }

    fn row(id: &str, kind: &str, state: &str) -> Live {
        serde_json::from_value(serde_json::json!({
            "sessionId": id, "id": &id[..8], "kind": kind, "status": state, "state": state
        }))
        .unwrap()
    }

    #[test]
    fn the_cli_lists_only_orbits_quiet_background_claude_sessions() {
        let mut map = std::collections::BTreeMap::new();
        map.insert("ABC-1".to_owned(), saved("aaaaaaaa-1", Agent::Claude, 30));
        map.insert("ABC-2".to_owned(), saved("bbbbbbbb-2", Agent::Claude, 2));
        map.insert("ABC-3".to_owned(), saved("cccccccc-3", Agent::Claude, 30));
        map.insert("ABC-4".to_owned(), saved("dddddddd-4", Agent::Codex, 30));
        let rows = vec![
            row("aaaaaaaa-1", "background", "idle"),
            row("bbbbbbbb-2", "background", "idle"),
            row("cccccccc-3", "background", "working"),
            row("dddddddd-4", "background", "idle"),
            // Not Orbit's: no saved key.
            row("eeeeeeee-5", "background", "idle"),
        ];
        let plan = plan_cli(
            &map,
            &rows,
            |_| Vec::new(),
            SystemTime::now(),
            idle_after(12),
        );
        assert_eq!(plan, [("ABC-1".to_owned(), "aaaaaaaa".to_owned())]);
        // A recent report keeps the old session.
        let recent = |_: &str| {
            vec![Report {
                at: SystemTime::now() - Duration::from_secs(H),
                state: super::super::status::State::Turn,
                text: String::new(),
            }]
        };
        assert!(plan_cli(&map, &rows, recent, SystemTime::now(), idle_after(12)).is_empty());
    }
}
