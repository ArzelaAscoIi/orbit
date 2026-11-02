//! What the other sessions do, told to each session, so they work as peers
//! instead of through one driver: like a Claude Project, the sessions share
//! standing rules and see each other's threads.
//!
//! - `<data>/project.md`: the user's (or any session's) standing rules for
//!   every session, e.g. "landing owns dependency PRs".
//! - Peers: every other status log (`<data>/status/*.log`) that is live (a
//!   line in the last `LIVE`, not ended), with its state, its note, and
//!   what it owns: its `claim` lines and the PRs it opened.
//!
//! The `hook` subcommand prints this at a prompt and after a command, only
//! when it differs from what the session last saw (a hash beside its log,
//! `<log>.peers`), so an unchanged project costs no tokens.

use std::path::Path;
use std::time::Duration;

use super::status::{self, State};

/// A log quiet this long belongs to a session that is not running.
const LIVE: Duration = Duration::from_secs(3 * 3600);
/// Most peers listed, the most recent first.
const MOST: usize = 15;

/// The project as the session with `log` should see it, if it changed since
/// it last did. Records what it saw.
pub fn news(log: &Path) -> Option<String> {
    let text = digest(log)?;
    let seen = log.with_extension("peers");
    let hash = crate::store::text_hash(&text).to_string();
    if std::fs::read_to_string(&seen).ok().as_deref() == Some(hash.as_str()) {
        return None;
    }
    let _ = std::fs::write(&seen, hash);
    Some(text)
}

/// The rules and the live peers of the session with `log`; None when there
/// is neither.
fn digest(log: &Path) -> Option<String> {
    let dir = log.parent()?;
    let rules = std::fs::read_to_string(dir.parent()?.join("project.md")).unwrap_or_default();
    let mut peers: Vec<(u64, String)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "log") && p != log)
        .filter_map(|p| {
            let key = p.file_stem()?.to_string_lossy().into_owned();
            peer(&key, &status::reports(&p))
        })
        .collect();
    peers.sort_by_key(|(age, _)| *age);
    peers.truncate(MOST);
    render(rules.trim(), &peers)
}

/// One live peer's line and how long ago it last reported.
fn peer(key: &str, reports: &[status::Report]) -> Option<(u64, String)> {
    let last = reports.last()?;
    let current = status::current(reports).map(|r| r.state);
    if last.age() > LIVE.as_secs() || current == Some(State::Ended) {
        return None;
    }
    let state = match current {
        Some(State::Working) => "working",
        Some(State::NeedsDecision | State::NeedsApproval) => "waiting on the user",
        _ => "idle",
    };
    let mut owns: Vec<&str> = reports
        .iter()
        .filter(|r| matches!(r.state, State::Claim | State::Pr) && !r.text.is_empty())
        .map(|r| r.text.as_str())
        .collect();
    owns.dedup();
    let mut line = format!("- `{key}` ({state})");
    if let Some(note) = status::note(reports) {
        line += &format!(": {}", note.text);
    }
    if !owns.is_empty() {
        line += &format!("; owns {}", owns.join(", "));
    }
    Some((last.age(), line))
}

fn render(rules: &str, peers: &[(u64, String)]) -> Option<String> {
    if rules.is_empty() && peers.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Orbit project update. Other sessions work beside you; skip a PR, ticket or \
         notification another live session owns, and claim what you take on.\n",
    );
    if !rules.is_empty() {
        out += &format!("\nProject rules (project.md):\n{rules}\n");
    }
    out += "\nLive sessions:\n";
    if peers.is_empty() {
        out += "- none\n";
    }
    for (_, line) in peers {
        out += line;
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_hears_its_live_peers_claims_once_and_never_an_ended_one() {
        let data = std::env::temp_dir().join(format!("orbit-peers-{}", std::process::id()));
        let dir = data.join("status");
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&dir).unwrap();
        let me = dir.join("inbox.log");
        let land = dir.join("land.log");
        let old = dir.join("old.log");
        status::report(&me, "working", "").unwrap();
        status::report(&land, "note", "Merging dependency PRs").unwrap();
        status::report(&land, "claim", "dependabot:acme/web").unwrap();
        status::report(&old, "working", "").unwrap();
        std::fs::write(&old, "1\tworking\t\n2\tclaim\tacme/api#1\n").unwrap();
        std::fs::write(data.join("project.md"), "Landing owns dependency PRs.").unwrap();

        let text = news(&me).unwrap();
        assert!(text.contains("Landing owns dependency PRs."));
        assert!(text.contains("- `land` (idle): Merging dependency PRs; owns dependabot:acme/web"));
        assert!(!text.contains("old") && !text.contains("`inbox`"));
        assert_eq!(news(&me), None, "unchanged: said once");

        status::report(&land, "turn", "").unwrap();
        std::fs::write(
            &land,
            std::fs::read_to_string(&land).unwrap() + "9\tended\t\n",
        )
        .unwrap();
        // The ended line is old, but it is the last: the peer is gone.
        let text = news(&me).unwrap();
        assert!(text.contains("- none"), "{text}");
        let _ = std::fs::remove_dir_all(&data);
    }
}
