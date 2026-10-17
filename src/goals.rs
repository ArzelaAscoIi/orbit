//! The week's goals, for Plan my day (today.rs). A detached Claude Code
//! session (the goals skill) reads what the user and their leads wrote down
//! (priority documents, the user's own channel posts, the initiatives they
//! own, meeting notes) and writes `goals.json` in its folder; the app only
//! reads that file. settings.json `goals`, the user's own words, wins over it.
//!
//! Ticket activity never decides a goal: a bulk edit or an ops bug touched
//! last hour ranks noise, not the week.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Goals {
    pub made_at: String,
    /// The Monday of the week they are for.
    pub week: String,
    pub summary: String,
    pub goals: Vec<Goal>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Goal {
    pub title: String,
    pub why: String,
    pub projects: Vec<String>,
    pub tickets: Vec<String>,
    pub blockers: Vec<Blocker>,
    pub sources: Vec<Source>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Blocker {
    pub what: String,
    pub ticket: Option<String>,
    pub owner: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Source {
    pub title: String,
    pub url: String,
}

pub fn file(folder: &Path) -> PathBuf {
    folder.join("goals.json")
}

/// The last run's goals. `None` before the first run; an error when the
/// file is there but not what the skill promised.
pub fn read(folder: &Path) -> Option<Result<Goals, String>> {
    let text = std::fs::read_to_string(file(folder)).ok()?;
    Some(
        serde_json::from_str::<Goals>(&text)
            .map(|mut g| {
                g.goals.retain(|goal| !goal.title.trim().is_empty());
                g
            })
            .map_err(|e| format!("goals.json is not readable: {e}")),
    )
}

/// The Monday of the week `date` (`2026-09-27`) is in, or of the next one
/// from Saturday on: a plan made on the weekend is for the week ahead.
pub fn week_of(date: &str) -> String {
    let Ok(day) = date.parse::<jiff::civil::Date>() else {
        return String::new();
    };
    let from_monday = i64::from(day.weekday().to_monday_zero_offset());
    let monday = if from_monday >= 5 {
        day.checked_add(jiff::Span::new().days(7 - from_monday))
    } else {
        day.checked_sub(jiff::Span::new().days(from_monday))
    };
    monday.map_or(String::new(), |d| d.to_string())
}

/// Whether the run's goals are for the week `date` plans: otherwise they
/// are found again before planning.
pub fn current(goals: &Goals, date: &str) -> bool {
    !goals.goals.is_empty() && goals.week == week_of(date)
}

/// The user's teams for the plan: the teams of the goals' own tickets (the
/// identifier's prefix), not every team they are a member of, which may be
/// a dozen. Empty without goals.
pub fn teams(goals: &Goals) -> Vec<String> {
    let mut keys: Vec<String> = goals
        .goals
        .iter()
        .flat_map(|g| &g.tickets)
        .filter_map(|t| t.split('-').next())
        .map(str::to_uppercase)
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The goals section of the plan's prompt: the user's own words first,
/// which win, then what the run found.
pub fn render(own: &str, found: Option<&Goals>) -> String {
    let mut out = String::new();
    if !own.trim().is_empty() {
        out.push_str("Stated by the user, and final:\n");
        out.push_str(own.trim());
        out.push('\n');
    }
    let Some(found) = found.filter(|g| !g.goals.is_empty()) else {
        return out;
    };
    out.push_str(&format!(
        "\nFound in what they and their leads wrote down (week of {}):\n",
        found.week
    ));
    for (n, goal) in found.goals.iter().enumerate() {
        out.push_str(&format!("{}. {}: {}\n", n + 1, goal.title, goal.why));
        if !goal.projects.is_empty() {
            out.push_str(&format!("   projects: {}\n", goal.projects.join(", ")));
        }
        if !goal.tickets.is_empty() {
            out.push_str(&format!("   tickets: {}\n", goal.tickets.join(", ")));
        }
        for b in &goal.blockers {
            let ticket = b.ticket.as_deref().unwrap_or("no ticket");
            out.push_str(&format!(
                "   blocked by: {} ({ticket}), owned by {}\n",
                b.what, b.owner
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_week_is_this_monday_or_from_saturday_the_next() {
        assert_eq!(week_of("2026-09-24"), "2026-09-21"); // Thursday
        assert_eq!(week_of("2026-09-28"), "2026-09-28"); // Monday
        assert_eq!(week_of("2026-09-26"), "2026-09-28"); // Saturday
        assert_eq!(week_of("2026-09-27"), "2026-09-28"); // Sunday
        assert_eq!(week_of("not a date"), "");
    }

    #[test]
    fn goals_json_reads_and_renders_with_the_users_words_first() {
        let text = r#"{"made_at":"x","week":"2026-09-28","summary":"s","goals":[
            {"title":"Ship the runtime","why":"prio doc: do this first",
             "projects":["Runtime GA"],"tickets":["RUN-1"],
             "blockers":[{"what":"ingress not ready","ticket":"OPS-9","owner":"Ops team"}],
             "sources":[{"title":"Prio doc","url":"https://example.com"}]},
            {"title":"  ","why":"dropped: no title"}]}"#;
        let goals: Goals = serde_json::from_str(text).unwrap();
        assert_eq!(goals.goals.len(), 2);
        let dir = std::env::temp_dir().join(format!("goals-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(file(&dir), text).unwrap();
        let read = read(&dir).unwrap().unwrap();
        assert_eq!(read.goals.len(), 1);
        assert!(current(&read, "2026-09-27"));
        assert!(!current(&read, "2026-10-05"));

        let prompt = render("Repo layout decision", Some(&read));
        let own = prompt.find("Repo layout decision").unwrap();
        let found = prompt.find("1. Ship the runtime").unwrap();
        assert!(own < found);
        assert!(prompt.contains("blocked by: ingress not ready (OPS-9), owned by Ops team"));
        assert_eq!(render("", None), "");
        // The goals' ticket teams, not the blockers' (OPS-9).
        assert_eq!(teams(&read), ["RUN"]);
        std::fs::remove_dir_all(dir).ok();
    }
}
