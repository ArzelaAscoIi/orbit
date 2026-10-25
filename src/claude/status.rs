//! What a session reports about itself, without spending a token on it.
//!
//! Every session the app starts gets Claude Code hooks (`--settings`) that
//! run this binary as `orbit hook <log>` at turn start,
//! at a question to the user, at a permission prompt, at turn end, on a
//! failure and at exit. Each run appends one line to the ticket's log:
//!
//! ```text
//! <unix seconds>\t<state>\t<one line of text>
//! ```
//!
//! The app reads the last line to say what the session needs, so "your
//! turn" means Claude asked something or finished, never merely "idle".
//! Modelled on firstmate's status protocol, with the hooks writing the
//! lines instead of the agent.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::story::{Event, Tone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// A prompt went in; Claude is on it.
    Working,
    /// Claude asked the user a question (`AskUserQuestion`) and waits.
    NeedsDecision,
    /// Claude waits for a tool permission.
    NeedsApproval,
    /// The turn ended; the text is Claude's last line.
    Turn,
    /// A pull request URL showed up in a command's output.
    Pr,
    Failed,
    Ended,
    /// A one-line summary the session wrote itself (`orbit report <log>
    /// note "…"`): display only, never a state.
    Note,
}

impl State {
    fn tag(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsDecision => "needs-decision",
            Self::NeedsApproval => "needs-approval",
            Self::Turn => "turn",
            Self::Pr => "pr",
            Self::Failed => "failed",
            Self::Ended => "ended",
            Self::Note => "note",
        }
    }

    fn parse(tag: &str) -> Option<Self> {
        Some(match tag {
            "working" => Self::Working,
            "needs-decision" => Self::NeedsDecision,
            "needs-approval" => Self::NeedsApproval,
            "turn" => Self::Turn,
            "pr" => Self::Pr,
            "failed" => Self::Failed,
            "ended" => Self::Ended,
            "note" => Self::Note,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub at: SystemTime,
    pub state: State,
    pub text: String,
}

impl Report {
    /// A report from half a day ago says nothing about now.
    pub fn recent(&self) -> bool {
        self.at
            .elapsed()
            .is_ok_and(|age| age < Duration::from_secs(12 * 3600))
    }

    /// Seconds since the report.
    pub fn age(&self) -> u64 {
        self.at.elapsed().map_or(0, |d| d.as_secs())
    }

    /// When, in the form Linear's timestamps have.
    pub fn timestamp(&self) -> String {
        jiff::Timestamp::try_from(self.at)
            .map(|t| t.to_string())
            .unwrap_or_default()
    }
}

/// The log of the session with this key.
pub fn path(key: &str) -> PathBuf {
    crate::store::data_dir()
        .join("status")
        .join(format!("{key}.log"))
}

/// `path` as one shell word: in single quotes, a quote in it closed,
/// escaped and reopened, so a data folder named `it's` cannot split or
/// inject into the commands the hooks and briefs give the shell.
pub fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// The `--settings` JSON that makes a session write `log`.
pub fn settings(log: &Path) -> Result<String> {
    let exe = std::env::current_exe().context("no path to this binary")?;
    let command = format!("{} hook {}", quote(&exe), quote(log));
    let hook = |matcher: Option<&str>| {
        let mut entry = json!({ "hooks": [{ "type": "command", "command": command }] });
        if let Some(matcher) = matcher {
            entry["matcher"] = json!(matcher);
        }
        json!([entry])
    };
    let settings = json!({ "hooks": {
        "UserPromptSubmit": hook(None),
        "PreToolUse": hook(Some("AskUserQuestion")),
        "PostToolUse": hook(Some("Bash")),
        "Notification": hook(Some("permission_prompt")),
        "Stop": hook(None),
        "StopFailure": hook(None),
        "SessionEnd": hook(None),
    }});
    Ok(with_co_author(settings, crate::profile::get().attribution.co_author()).to_string())
}

/// The session's commits name the app as co-author beside Claude. Claude
/// Code's `attribution.commit` replaces its own trailer, so Claude's line
/// goes in again first.
fn with_co_author(mut settings: Value, co_author: Option<String>) -> Value {
    if let Some(line) = co_author {
        settings["attribution"] = json!({
            "commit": format!("Co-Authored-By: Claude <noreply@anthropic.com>\n{line}"),
        });
    }
    settings
}

/// The `hook` subcommand: reads the event on stdin, appends its line.
pub fn hook(log: &Path) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let event: Value = serde_json::from_str(&input).context("hook input is not JSON")?;
    let Some((state, text)) = line(&event) else {
        return Ok(());
    };
    append(log, state, &text)
}

/// The `report` subcommand, run by the session itself (the brief names the
/// command): `orbit report <log> <state> <text…>`. A `note` is a one-line
/// summary shown on the session's row; the other states say what the
/// session waits for, as the hooks would. Ending is Claude Code's to say.
pub fn report(log: &Path, state: &str, text: &str) -> Result<()> {
    let state = match State::parse(state) {
        Some(State::Ended | State::Pr) | None => bail!(
            "state {state:?} is not one of note, working, needs-decision, needs-approval, turn, failed"
        ),
        Some(state) => state,
    };
    append(log, state, text)
}

/// The exact command a session runs to report: the binary and its own log,
/// so it works from any shell of the session, env or not.
pub fn report_command(key: &str) -> Result<String> {
    let exe = std::env::current_exe().context("no path to this binary")?;
    Ok(format!("{} report {}", quote(&exe), quote(&path(key))))
}

/// The paragraph every session's brief ends with: how to put a one-line
/// summary on its row in the app.
pub fn progress_hint(key: &str) -> Option<String> {
    let command = report_command(key).ok()?;
    Some(format!(
        "## {name}
This session runs inside {name}, which shows its row to the user. Put a \
         one-line summary of what you are on there whenever it changes (a sentence, under 80 \
         characters, no secrets): `{command} note \"<summary>\"`. The `orbit` skill in this \
         folder has the details. Never put an @-mention followed by a word and `!` in it.",
        name = crate::NAME
    ))
}

/// One line onto the log: the time, the state and the text's first line.
fn append(log: &Path, state: State, text: &str) -> Result<()> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let text = text.lines().next().unwrap_or_default().trim();
    let text: String = text.chars().take(300).collect();
    let entry = format!("{at}\t{}\t{text}\n", state.tag());
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(log)
        .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()))
        .with_context(|| format!("cannot append to {}", log.display()))
}

/// The line one hook event turns into, if any.
fn line(event: &Value) -> Option<(State, String)> {
    let text = |key: &str| event[key].as_str().unwrap_or_default().to_owned();
    match event["hook_event_name"].as_str()? {
        "UserPromptSubmit" => Some((State::Working, text("prompt"))),
        "PreToolUse" => {
            let question = event["tool_input"]["questions"][0]["question"].as_str()?;
            Some((State::NeedsDecision, question.to_owned()))
        }
        "PostToolUse" => pull_request(&event["tool_response"].to_string()).map(|u| (State::Pr, u)),
        "Notification" => Some((State::NeedsApproval, text("message"))),
        "Stop" if event["stop_hook_active"] != true => {
            Some((State::Turn, text("last_assistant_message")))
        }
        "StopFailure" => Some((State::Failed, text("error"))),
        "SessionEnd" => Some((State::Ended, text("reason"))),
        _ => None,
    }
}

/// The first GitHub pull request URL in the text.
fn pull_request(text: &str) -> Option<String> {
    text.match_indices("https://github.com/")
        .find_map(|(start, _)| {
            let url: String = text[start..]
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\\' | ')' | '>' | '\''))
                .collect();
            let mut parts = url.trim_start_matches("https://github.com/").split('/');
            let (owner, repo, pull, number) =
                (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            (pull == "pull"
                && !owner.is_empty()
                && !repo.is_empty()
                && number.parse::<u64>().is_ok())
            .then(|| format!("https://github.com/{owner}/{repo}/pull/{number}"))
        })
}

/// The session's current report: the last state line. A pull request line
/// is a fact, not a state (`pr` has it), and a note is display only (`note`).
pub fn current(reports: &[Report]) -> Option<&Report> {
    reports
        .iter()
        .rev()
        .find(|r| !matches!(r.state, State::Pr | State::Note))
}

/// The session's own last one-line summary, if it wrote one.
pub fn note(reports: &[Report]) -> Option<&Report> {
    reports.iter().rev().find(|r| r.state == State::Note)
}

/// The run's last failure (an API error ending a turn), until a later
/// turn ends well: a pass that failed says so until one gets through.
pub fn failed(reports: &[Report]) -> Option<&Report> {
    reports
        .iter()
        .rev()
        .find(|r| matches!(r.state, State::Failed | State::Turn))
        .filter(|r| r.state == State::Failed)
}

/// The last pull request the session's commands mentioned.
pub fn pr(reports: &[Report]) -> Option<&Report> {
    reports.iter().rev().find(|r| r.state == State::Pr)
}

/// What the session did, as timeline events next to Linear's comments:
/// the questions it asked, the PR it opened, what it answered, how it
/// failed. Turn starts, approvals and exits are mechanics, not news.
pub fn events(reports: &[Report]) -> Vec<Event> {
    reports
        .iter()
        .filter_map(|r| {
            let (verb, tone) = match r.state {
                State::NeedsDecision => ("asks", Tone::Claude),
                State::Pr => ("opened a PR", Tone::Claude),
                State::Turn if !r.text.is_empty() => ("answered", Tone::Claude),
                State::Failed => ("failed", Tone::Problem),
                _ => return None,
            };
            Some(Event {
                at: r.timestamp(),
                who: "Claude".to_owned(),
                verb,
                tone,
                summary: r.text.clone(),
                body: r.text.clone(),
            })
        })
        .collect()
}

/// Every line of the log, oldest first.
pub fn reports(log: &Path) -> Vec<Report> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, '\t');
            let at = parts.next()?.parse().ok()?;
            let state = State::parse(parts.next()?)?;
            Some(Report {
                at: UNIX_EPOCH + Duration::from_secs(at),
                state,
                text: parts.next().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {

    /// What Orbit's pi extension writes (pi-orbit.ts, through `report`):
    /// the same states as Claude's hooks, so the row reads them alike.
    #[test]
    fn a_pi_sessions_reports_read_like_claudes() {
        let log = std::env::temp_dir().join(format!("orbit-pi-status-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        report(&log, "working", "").unwrap();
        report(&log, "needs-approval", "make deploy").unwrap();
        assert_eq!(
            current(&reports(&log)).map(|r| r.state),
            Some(State::NeedsApproval)
        );
        report(&log, "working", "").unwrap();
        report(&log, "turn", "").unwrap();
        assert_eq!(current(&reports(&log)).map(|r| r.state), Some(State::Turn));
        let _ = std::fs::remove_file(&log);
    }

    use super::*;

    #[test]
    fn a_failed_run_shows_until_a_later_turn_ends_well() {
        let at = |state| Report {
            at: UNIX_EPOCH,
            state,
            text: "server_error".into(),
        };
        let failed_then = |more: &[State]| {
            let mut r = vec![at(State::Turn), at(State::Failed)];
            r.extend(more.iter().map(|s| at(*s)));
            failed(&r).is_some()
        };
        assert!(failed_then(&[]));
        assert!(failed_then(&[State::Working]), "a retry still running");
        assert!(
            !failed_then(&[State::Working, State::Turn]),
            "one got through"
        );
        assert!(failed(&[at(State::Turn)]).is_none());
    }

    #[test]
    fn commits_name_the_app_as_co_author_beside_claude_when_the_profile_has_an_email() {
        let plain = with_co_author(json!({"hooks": {}}), None);
        assert!(plain.get("attribution").is_none());
        let line = "Co-Authored-By: orbit <orbit@acme.example>".to_owned();
        let signed = with_co_author(json!({"hooks": {}}), Some(line));
        assert_eq!(
            signed["attribution"]["commit"],
            "Co-Authored-By: Claude <noreply@anthropic.com>\nCo-Authored-By: orbit <orbit@acme.example>"
        );
        assert!(signed.get("hooks").is_some());
    }

    #[test]
    fn hook_events_become_lines() {
        let ask = json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": "Squash or merge?"}]}});
        assert_eq!(
            line(&ask),
            Some((State::NeedsDecision, "Squash or merge?".into()))
        );
        let stop = json!({"hook_event_name": "Stop", "stop_hook_active": false,
            "last_assistant_message": "Opened the PR.\nDetails follow."});
        assert_eq!(line(&stop).unwrap().0, State::Turn);
        let again = json!({"hook_event_name": "Stop", "stop_hook_active": true});
        assert_eq!(line(&again), None);
        let bash = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_response": {"stdout": "https://github.com/acme/web/pull/42\n"}});
        assert_eq!(
            line(&bash),
            Some((State::Pr, "https://github.com/acme/web/pull/42".into()))
        );
        let quiet = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_response": {"stdout": "https://github.com/acme/web/issues/7"}});
        assert_eq!(line(&quiet), None);
    }

    #[test]
    fn last_state_wins_over_pr_facts() {
        let dir = std::env::temp_dir().join(format!("orbit-status-{}", std::process::id()));
        let log = dir.join("DEV-1.log");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &log,
            "1\tworking\tfix it\n2\tpr\thttps://github.com/o/r/pull/3\n3\tneeds-decision\tSquash?\n",
        )
        .unwrap();
        let reports = reports(&log);
        let report = current(&reports).unwrap();
        assert_eq!(report.state, State::NeedsDecision);
        assert_eq!(report.text, "Squash?");
        assert_eq!(pr(&reports).unwrap().text, "https://github.com/o/r/pull/3");
        let events = events(&reports);
        assert_eq!(
            events.iter().map(|e| e.verb).collect::<Vec<_>>(),
            ["opened a PR", "asks"]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_session_reports_a_note_that_never_passes_for_a_state() {
        let dir = std::env::temp_dir().join(format!("orbit-report-{}", std::process::id()));
        let log = dir.join("DEV-2.log");
        std::fs::create_dir_all(&dir).unwrap();
        report(&log, "working", "on the tests").unwrap();
        report(&log, "note", "writing the parser\nsecond line dropped").unwrap();
        assert!(
            report(&log, "ended", "x").is_err(),
            "ending is Claude Code's"
        );
        assert!(report(&log, "nonsense", "x").is_err());
        let reports = reports(&log);
        assert_eq!(current(&reports).unwrap().state, State::Working);
        assert_eq!(note(&reports).unwrap().text, "writing the parser");
        assert!(
            events(&reports).is_empty(),
            "a note is not news on the timeline"
        );
        std::fs::remove_dir_all(dir).unwrap();
        let hint = progress_hint("DEV-2").unwrap();
        assert!(hint.contains("report '") && hint.contains("DEV-2.log' note"));
    }

    #[test]
    fn settings_cover_every_event() {
        let json: Value =
            serde_json::from_str(&settings(Path::new("/tmp/x.log")).unwrap()).unwrap();
        for event in [
            "UserPromptSubmit",
            "PreToolUse",
            "Notification",
            "Stop",
            "StopFailure",
            "SessionEnd",
        ] {
            let command = json["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap();
            assert!(command.ends_with("hook '/tmp/x.log'"), "{event}: {command}");
        }
        assert_eq!(json["hooks"]["PreToolUse"][0]["matcher"], "AskUserQuestion");
    }
}
