//! Plan my day: a detached Claude Code session (the spot skill) reads the
//! week's goals and priority docs, then Linear, Slack, Notion, GitHub and
//! the observability and incident tools the way the user would, and writes
//! the day to `spots.json` in its folder: the focus, the work ranked by the
//! goals (what only the user can do, and what one session could do alone),
//! who to reach, blind spots, what can wait. The app only reads that file;
//! starting a spot starts a new session with the spot as its brief and adds
//! it to `done.json`, which the next run skips.
//!
//! The best `now` spots start on their own (`to_start`), at most
//! settings.json `work.autoStart` at once; filing a ticket and what is the
//! user's stay a click.
//!
//! Several copies of the app may run: `done.json` is read-modify-write.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Spots {
    pub made_at: String,
    pub summary: String,
    pub mode: Option<String>,
    /// Where today's attention goes, one line.
    pub focus: String,
    /// Best first: the goals decide the order.
    pub spots: Vec<Spot>,
    pub reach: Vec<Reach>,
    pub blind: Vec<Blind>,
    pub park: Vec<Park>,
}

/// Someone to talk to today.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Reach {
    pub who: String,
    pub about: String,
}

/// What nobody is moving but a priority depends on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Blind {
    pub what: String,
    pub ticket: Option<String>,
}

/// A ticket that can wait today.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Park {
    pub ticket: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Spot {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub ticket: Option<String>,
    pub repo: Option<String>,
    pub why: String,
    pub plan: String,
    pub done_when: String,
    /// "now" (a local session), "ticket" (the New ticket flow): the
    /// spotter's pick, the user may take the other; or "you": the user's
    /// own ticket to work on, which no session starts.
    pub route: String,
    /// A ticket's quick action, run in that ticket's own session: the
    /// fix-comments skill on a PR URL, or the build skill on the ticket (by
    /// the profile's names for them, or the generic ones).
    pub command: Option<String>,
    /// The whole task in the triage template's sections.
    pub description: String,
    /// For File a ticket, so New ticket needs no second reading: the team
    /// key, P0 to P4, and bug, feature, ux, question or chore.
    pub team: Option<String>,
    pub priority: Option<String>,
    pub ticket_kind: Option<String>,
    pub sources: Vec<String>,
}

impl Spot {
    /// The spot's quick action, if it is one the app runs: the spotter reads
    /// Slack and PRs, so anything else it wrote there is refused.
    pub fn quick(&self) -> Option<String> {
        self.quick_in(&crate::profile::get())
    }

    fn quick_in(&self, profile: &crate::profile::Profile) -> Option<String> {
        let ticket = self.ticket.as_deref()?;
        let command = self.command.as_deref()?.trim();
        let (skill, arg) = command.split_once(' ')?;
        let arg = arg.trim();
        let pr = arg.starts_with("https://github.com/")
            && arg.contains("/pull/")
            && arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/:.-_".contains(c));
        let is = |role: &str, generic: &str| {
            skill == format!("/{}", profile.skill(role)) || skill == format!("/{generic}")
        };
        // Always the profile's own skill name, whichever name was written.
        let (ok, role) = if is("fixComments", "fix-pr-comments") {
            (pr, "fixComments")
        } else if is("build", "build") {
            (arg == ticket, "build")
        } else {
            (false, "")
        };
        ok.then(|| format!("/{} {arg}", profile.skill(role)))
    }
}

/// A spot the user started, so the spotter does not find it again.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Started {
    pub id: String,
    pub title: String,
    pub ticket: Option<String>,
    pub started_at: String,
    /// The session doing it, or empty when it went to the New ticket flow.
    #[serde(default)]
    pub session: String,
}

/// The spots to start on their own now, best first: today's `now` spots
/// nobody started, while fewer than `cap` started ones still run.
// ponytail: two copies of the app may both start one; done.json is re-read
// right before, a lock file if it ever happens.
pub fn to_start<'a>(
    spots: &'a Spots,
    started: &[Started],
    running: usize,
    cap: usize,
    today: &str,
) -> Vec<&'a Spot> {
    if !spots.made_at.starts_with(today) {
        return Vec::new();
    }
    spots
        .spots
        .iter()
        .filter(|s| s.route == "now" && !started.iter().any(|d| d.id == s.id))
        .take(cap.saturating_sub(running))
        .collect()
}

pub fn spots_file(folder: &Path) -> PathBuf {
    folder.join("spots.json")
}

fn done_file(folder: &Path) -> PathBuf {
    folder.join("done.json")
}

/// The spotter's last answer. `None` before its first run; an error when
/// the file is there but not what the skill promised.
pub fn read(folder: &Path) -> Option<Result<Spots, String>> {
    let text = std::fs::read_to_string(spots_file(folder)).ok()?;
    Some(
        serde_json::from_str::<Spots>(&text)
            .map(|mut s| {
                s.spots
                    .retain(|spot| !spot.id.is_empty() && !spot.title.is_empty());
                s
            })
            .map_err(|e| format!("spots.json is not readable: {e}")),
    )
}

pub fn started(folder: &Path) -> Vec<Started> {
    crate::store::load_at(&done_file(folder))
}

/// Remembers a started spot, keeping what another copy of the app added.
pub fn remember(folder: &Path, spot: &Spot, session: &str) -> std::io::Result<()> {
    let mut done = started(folder);
    if done.iter().any(|d| d.id == spot.id) {
        return Ok(());
    }
    done.push(Started {
        id: spot.id.clone(),
        title: spot.title.clone(),
        ticket: spot.ticket.clone(),
        started_at: jiff::Zoned::now().strftime("%Y-%m-%d %H:%M").to_string(),
        session: session.to_owned(),
    });
    crate::store::save_at(&done_file(folder), &done)
}

/// The first message of the session that does a spot: the spot, then the
/// rules every unattended job keeps.
pub fn brief(spot: &Spot) -> String {
    let mut out = format!("Do this for me on your own: {}\n\n", spot.title);
    if let Some(ticket) = &spot.ticket {
        out.push_str(&format!(
            "Ticket: {ticket}. Read it and its comments first.\n"
        ));
    }
    if let Some(repo) = &spot.repo {
        out.push_str(&format!("Repository: {repo}.\n"));
    }
    out.push_str(&format!(
        "Why: {}\n\nPlan:\n{}\n\nDone when: {}\n",
        spot.why, spot.plan, spot.done_when
    ));
    if !spot.description.trim().is_empty() {
        out.push_str(&format!("\n{}\n", spot.description.trim()));
    }
    if !spot.sources.is_empty() {
        out.push_str(&format!("\nSources:\n- {}\n", spot.sources.join("\n- ")));
    }
    out.push_str(
        "\nRules: work to something I only have to review. You may push a branch and open \
         a draft PR. Never merge or approve, never force-push, and check `gh pr view --json \
         state` before pushing to an existing branch. Do not \
         post to Slack, Linear, incident.io or any customer without asking me first. No \
         customer data in prompts or files. When you are done, tell me in five lines or fewer \
         what you did and what I should look at.",
    );
    // The company's own house rules (profile.rules).
    let rules = crate::profile::get().rules.trim().to_owned();
    if !rules.is_empty() {
        out.push_str(&format!(" {rules}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_best_now_spots_start_until_the_cap_and_only_today() {
        let spot = |id: &str, route: &str| Spot {
            id: id.into(),
            title: id.into(),
            route: route.into(),
            ..Default::default()
        };
        let spots = Spots {
            made_at: "2026-09-27T08:00:00+02:00".into(),
            spots: vec![
                spot("mine", "you"),
                spot("done", "now"),
                spot("file", "ticket"),
                spot("a", "now"),
                spot("b", "now"),
                spot("c", "now"),
            ],
            ..Default::default()
        };
        let started = [Started {
            id: "done".into(),
            ..Default::default()
        }];
        let ids = |v: Vec<&Spot>| v.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        let today = "2026-09-27";
        assert_eq!(ids(to_start(&spots, &started, 0, 2, today)), ["a", "b"]);
        assert_eq!(ids(to_start(&spots, &started, 1, 2, today)), ["a"]);
        assert!(to_start(&spots, &started, 2, 2, today).is_empty());
        assert!(to_start(&spots, &started, 0, 0, today).is_empty());
        assert!(to_start(&spots, &started, 0, 2, "2026-09-28").is_empty());
    }

    #[test]
    fn spots_read_from_the_skill_file_and_a_started_one_is_remembered_once() {
        let dir = std::env::temp_dir().join(format!("mcr-work-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read(&dir), None);
        std::fs::write(
            spots_file(&dir),
            r#"{"made_at":"2026-09-25T10:00:00+02:00","summary":"Worker alerts and one CVE",
                "spots":[{"id":"abc-715-cve","title":"Bump js-yaml for ABC-715","kind":"cve",
                "ticket":"ABC-715","repo":"acme/web-ui","why":"Socket critical",
                "plan":"bump\nrun tests","done_when":"a draft PR","sources":["https://linear.app/x"]},
                {"title":"no id, dropped"}]}"#,
        )
        .unwrap();
        let spots = read(&dir).unwrap().unwrap();
        assert_eq!(spots.spots.len(), 1);
        let spot = &spots.spots[0];
        let brief = brief(spot);
        assert!(brief.starts_with("Do this for me on your own: Bump js-yaml"));
        assert!(brief.contains("Ticket: ABC-715") && brief.contains("Never merge"));
        assert!(!brief.contains("@Robo"));

        // Quick actions: only the two the app knows, on the spot's ticket,
        // run under the profile's names for the skills.
        let profile = crate::profile::Profile {
            skills: [
                ("fixComments".to_owned(), "acme-fix".to_owned()),
                ("build".to_owned(), "acme-build".to_owned()),
            ]
            .into(),
            ..Default::default()
        };
        let quick = |command: &str, profile: &crate::profile::Profile| {
            Spot {
                ticket: Some("ABC-715".into()),
                command: Some(command.into()),
                ..Spot::default()
            }
            .quick_in(profile)
        };
        let generic = crate::profile::Profile::default();
        let pr = "https://github.com/acme/app/pull/6410";
        assert_eq!(
            quick(&format!("/fix-pr-comments {pr}"), &generic),
            Some(format!("/fix-pr-comments {pr}"))
        );
        assert_eq!(
            quick(&format!("/fix-pr-comments {pr}"), &profile),
            Some(format!("/acme-fix {pr}"))
        );
        assert_eq!(
            quick("/acme-build ABC-715", &profile).as_deref(),
            Some("/acme-build ABC-715")
        );
        assert_eq!(quick("/build DEV-1", &generic), None);
        assert_eq!(
            quick(&format!("/fix-pr-comments {pr}; rm -rf ~"), &generic),
            None
        );
        assert_eq!(quick("/something-else x", &profile), None);
        // A profile that fine-tunes the app's skills keeps their names.
        let acme = crate::profile::acme();
        assert_eq!(
            quick("/build ABC-715", &acme).as_deref(),
            Some("/build ABC-715")
        );

        remember(&dir, spot, "free:1").unwrap();
        remember(&dir, spot, "free:2").unwrap();
        assert_eq!(started(&dir).len(), 1);
        assert_eq!(started(&dir)[0].session, "free:1");

        std::fs::write(spots_file(&dir), "{half").unwrap();
        assert!(read(&dir).unwrap().is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
