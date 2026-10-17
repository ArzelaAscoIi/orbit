//! Laya: an open-weight "System 1" decision model (huggingface.co/
//! convaiinnovations/laya). It answers typed questions about a text in one
//! forward pass, with a calibrated confidence, and never writes text.
//!
//! Optional and exchangeable with Haiku (settings.json `laya.mode`: `off`,
//! `shadow`, `use`), for one decision in the New ticket intake: is the note
//! a bug report? That is the one Laya earned. On the 166 labelled tickets
//! people triaged, the question below scores 96% (always saying "bug"
//! scores 92%), 98% on the 68% it answers with confidence 0.8 or more, and
//! 90% on the short notes in tests/laya-notes.json with no confident
//! mistake. Team (38% against a 24% baseline) and priority (43% against
//! 46%) were no better than guessing, so Haiku keeps those. Re-run with
//! `eval_against_labelled_tickets` and `real_laya_tells_bugs_from_wishes`.
//!
//! Haiku answers first and writes everything else. In `shadow` mode Laya's
//! answer is only logged; in `use` mode it replaces Haiku's kind when it is
//! confident and they disagree. Every comparison goes to `laya-shadow.jsonl`,
//! so Settings shows how often the two agree.
//!
//! Quick actions (`QUICK`): on the open ticket Laya answers a few more
//! yes-or-no questions, and a confident yes proposes the action that goes
//! with it. None of these is evaluated yet the way BUG was, so every answer
//! and every click goes to `laya-quick.jsonl` (ticket ids, never text):
//! what it proposed against what the user did, per question, in Settings.
//! `shadow` only logs; `use` shows them.
//!
//! Laya runs inside the app (laya/model.rs): the checkpoint is downloaded
//! once from Hugging Face (Settings), then every note is answered on the CPU
//! in a few hundred milliseconds, offline.

mod model;

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};

use crate::capture::Intake;

pub use model::{dir, downloaded};

/// The question, word for word as evaluated. Phrasing matters: "a request
/// for something new?" scored 10%, "something broken: an error, a crash, a
/// failure, or behaviour that differs from what is expected?" 95% but called
/// short bug notes "no bug" with confidence.
const BUG: &str = "Does the text describe a defect: something that fails, errors, loops, or \
does the wrong thing?";

/// Laya's answer: a bug report or not, and its calibrated confidence.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub bug: bool,
    pub confidence: f64,
}

/// The loaded model, shared: loading maps 840 MB and builds the network,
/// so it happens once per run.
fn loaded() -> Result<Arc<model::Laya>> {
    static LAYA: Mutex<Option<Arc<model::Laya>>> = Mutex::new(None);
    let mut slot = LAYA.lock().map_err(|_| anyhow!("Laya crashed earlier"))?;
    if let Some(laya) = slot.as_ref() {
        return Ok(laya.clone());
    }
    let dir = dir();
    if !downloaded(&dir) {
        return Err(anyhow!("Laya is not downloaded yet: Settings, connections"));
    }
    let laya = Arc::new(model::Laya::load(&dir)?);
    *slot = Some(laya.clone());
    Ok(laya)
}

/// Asks Laya about a note. Blocking and CPU-heavy: call it from
/// `spawn_blocking`.
pub fn decide(note: &str) -> Result<Decision> {
    let answer = loaded()?.noul(note, BUG)?;
    Ok(Decision {
        bug: answer.p >= 0.5,
        confidence: answer.confidence,
    })
}

/// A yes-or-no question about the open ticket; a yes at `min` confidence
/// or more proposes the quick action with the same `id` (ui/ticket.rs
/// `quick_actions`). Phrasing and `min` come from `real_quick_answers`, a
/// handful of tickets: good enough to propose, not yet measured.
/// "Is it a question rather than a change?" was tried in five phrasings
/// and never told a support question from a vague bug, so it is not here.
/// "Should the reporter check the fix?" was tried in four phrasings on six
/// tickets (first-hand reports, a sweep, a CVE, a relayed ask) and was
/// confidently wrong both ways: it cannot tell who wrote a text from how it
/// reads. Request review from the reporter is a plain rule instead
/// (ui/ticket.rs `request_reporter`).
pub struct Quick {
    pub id: &'static str,
    pub question: &'static str,
    pub min: f64,
}

pub const QUICK: &[Quick] = &[
    // "Clear enough to start?" called a vague note ready and a full repro
    // not; asking for the repro and the expected result split them.
    Quick {
        id: "ready",
        question: "Does the text describe the steps to reproduce a problem and the expected \
                   result?",
        min: 0.65,
    },
    Quick {
        id: "repro",
        question: "Is the text a bug report that does not say how to reproduce the problem?",
        min: 0.7,
    },
];

/// Claude's last message in a session: does it wait on the user (a
/// question, a decision) rather than report something done? Not evaluated
/// the way BUG was, only on six messages (`real_session_asks`): it nudges
/// the Sessions order at confidence 0.7 or more, and its rows carry the
/// Laya chip. Picked from five phrasings: "ask the reader a question or wait
/// for their answer" called a real ask done at 0.73, "end with a question"
/// and "is the writer waiting" were wrong both ways; this one got both clear
/// asks and all three done reports, and left the unclear one at 0.50.
const ASKS: &str = "Does the writer ask the reader to decide or answer something?";

/// Laya's answer about each session's last message, `(key, yes, confidence)`.
/// Blocking and CPU-heavy (a few hundred milliseconds each): call it from
/// `spawn_blocking`.
pub fn asks(texts: &[(String, String)]) -> Result<Vec<(String, bool, f64)>> {
    let laya = loaded()?;
    texts
        .iter()
        .map(|(key, text)| {
            let answer = laya.noul(text, ASKS)?;
            Ok((key.clone(), answer.p >= 0.5, answer.confidence))
        })
        .collect()
}

/// Laya's answer to one quick question.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub id: &'static str,
    pub yes: bool,
    pub confidence: f64,
}

/// The ticket as Laya reads it, most telling first: Laya keeps only what
/// fits in 512 tokens, so the description is cut to leave room for the
/// latest comment.
pub fn ticket_text(title: &str, description: &str, last_comment: &str) -> String {
    let cut = |text: &str, n: usize| text.chars().take(n).collect::<String>();
    let mut text = format!("{title}\n\n{}", cut(description.trim(), 1200));
    if !last_comment.trim().is_empty() {
        text.push_str(&format!(
            "\n\nLatest comment: {}",
            cut(last_comment.trim(), 600)
        ));
    }
    text
}

/// Every quick question about `text`. Blocking and CPU-heavy (about a
/// second for all): call it from `spawn_blocking`.
pub fn quick(text: &str) -> Result<Vec<Proposal>> {
    let laya = loaded()?;
    QUICK
        .iter()
        .map(|q| {
            let answer = laya.noul(text, q.question)?;
            Ok(Proposal {
                id: q.id,
                yes: answer.p >= 0.5,
                confidence: answer.confidence,
            })
        })
        .collect()
}

/// The ids of the answers that propose their action: a yes at the
/// question's `min` confidence or more.
pub fn shown(answers: &[Proposal]) -> Vec<&'static str> {
    answers
        .iter()
        .filter(|a| a.yes && QUICK.iter().any(|q| q.id == a.id && a.confidence >= q.min))
        .map(|a| a.id)
        .collect()
}

/// One line of `laya-quick.jsonl`: an answer (with `shown` when it was
/// offered), or a click on a proposal (`clicked`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct QuickLine {
    pub ticket: String,
    pub question: String,
    pub yes: bool,
    pub confidence: f64,
    pub shown: bool,
    pub clicked: bool,
}

/// Per quick question, in `QUICK` order: `(id, asked, shown, clicked)`.
pub fn quick_stats(path: &Path) -> Vec<(&'static str, usize, usize, usize)> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<QuickLine> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    QUICK
        .iter()
        .map(|q| {
            let of = |f: fn(&QuickLine) -> bool| {
                lines.iter().filter(|l| l.question == q.id && f(l)).count()
            };
            (q.id, of(|l| !l.clicked), of(|l| l.shown), of(|l| l.clicked))
        })
        .collect()
}

/// Downloads the checkpoint, reporting `(done, total)` bytes.
pub async fn download(http: &reqwest::Client, progress: impl Fn(u64, u64)) -> Result<()> {
    model::download(http, &dir(), progress).await
}

/// One comparison with Haiku, for the shadow log.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Compared {
    pub field: String,
    pub haiku: String,
    pub laya: String,
    pub confidence: f64,
    /// Laya's answer replaced Haiku's.
    pub used: bool,
}

/// Puts Laya's answer into Haiku's kind when `use_it`, it is confident and
/// the two disagree: a bug stays or becomes `bug`; a note Laya calls no bug
/// becomes a `feature` only if Haiku had called it a bug. Says how the two
/// compared either way.
pub fn merge(intake: &mut Intake, laya: &Decision, use_it: bool, min: f64) -> Compared {
    let name = |bug: bool| if bug { "bug" } else { "not bug" }.to_owned();
    let haiku = intake.kind == "bug";
    let used = use_it && laya.confidence >= min && laya.bug != haiku;
    if used {
        intake.kind = if laya.bug { "bug" } else { "feature" }.into();
    }
    Compared {
        field: "bug".into(),
        haiku: name(haiku),
        laya: name(laya.bug),
        confidence: laya.confidence,
        used,
    }
}

/// Appends a comparison to the shadow log, one JSON line. Every copy of the
/// app only appends, so no read-modify-write is needed.
pub fn log(path: &Path, compared: &impl serde::Serialize) -> std::io::Result<()> {
    use std::io::Write;
    let mut line = serde_json::to_string(compared).unwrap_or_default();
    line.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

/// `(compared, agreed)` over the whole shadow log.
pub fn agreement(path: &Path) -> (usize, usize) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let compared: Vec<Compared> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let agreed = compared.iter().filter(|c| c.haiku == c.laya).count();
    (compared.len(), agreed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn only_a_confident_disagreement_changes_the_kind_and_only_in_use_mode() {
        let haiku = |kind: &str| Intake {
            kind: kind.into(),
            title: "Filter the dashboard".into(),
            team: "ECO".into(),
            ..Intake::default()
        };
        let bug = Decision {
            bug: true,
            confidence: 0.9,
        };
        let not_bug = Decision {
            bug: false,
            confidence: 0.9,
        };

        // Shadow: nothing changes, the comparison is still made.
        let mut shadow = haiku("feature");
        let compared = merge(&mut shadow, &bug, false, 0.8);
        assert_eq!(shadow, haiku("feature"));
        assert_eq!(
            (
                compared.haiku.as_str(),
                compared.laya.as_str(),
                compared.used
            ),
            ("not bug", "bug", false)
        );

        // Use: confident and different, so Laya's call wins.
        let mut used = haiku("feature");
        assert!(merge(&mut used, &bug, true, 0.8).used);
        assert_eq!(used.kind, "bug");
        let mut used = haiku("bug");
        merge(&mut used, &not_bug, true, 0.8);
        assert_eq!(used.kind, "feature");
        assert_eq!(
            (used.title.as_str(), used.team.as_str()),
            ("Filter the dashboard", "ECO")
        );

        // Not a bug to Laya, and Haiku said ux: agreement, nothing changes.
        let mut ux = haiku("ux");
        assert!(!merge(&mut ux, &not_bug, true, 0.8).used);
        assert_eq!(ux.kind, "ux");

        // Below the threshold Haiku's answer stands.
        let mut unsure = haiku("feature");
        merge(
            &mut unsure,
            &Decision {
                bug: true,
                confidence: 0.6,
            },
            true,
            0.8,
        );
        assert_eq!(unsure.kind, "feature");
    }

    #[test]
    fn the_shadow_log_appends_and_counts_agreement() {
        let path = std::env::temp_dir().join(format!("laya-shadow-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(agreement(&path), (0, 0));
        let bug = Decision {
            bug: true,
            confidence: 0.9,
        };
        for kind in ["bug", "feature"] {
            let mut intake = Intake {
                kind: kind.into(),
                ..Intake::default()
            };
            log(&path, &merge(&mut intake, &bug, false, 0.8)).unwrap();
        }
        assert_eq!(agreement(&path), (2, 1));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn ticket_text_keeps_room_for_the_latest_comment() {
        let long = "x".repeat(5000);
        let text = ticket_text("Title", &long, "Any update?");
        assert!(text.starts_with("Title\n\n"));
        assert!(text.ends_with("Latest comment: Any update?"));
        assert!(text.len() < 1300);
        assert_eq!(ticket_text("T", "d", " "), "T\n\nd");
    }

    #[test]
    fn a_quick_action_shows_only_for_a_yes_at_its_questions_min() {
        let a = |id, yes, confidence| Proposal {
            id,
            yes,
            confidence,
        };
        let ready = QUICK.iter().find(|q| q.id == "ready").unwrap().min;
        assert_eq!(shown(&[a("ready", true, ready)]), ["ready"]);
        assert!(shown(&[a("ready", true, ready - 0.01)]).is_empty());
        assert!(shown(&[a("ready", false, 0.99)]).is_empty(), "a sure no");
        assert!(
            shown(&[a("gone", true, 0.99)]).is_empty(),
            "no such question"
        );
    }

    #[test]
    fn quick_stats_count_answers_shows_and_clicks_per_question() {
        let path = std::env::temp_dir().join(format!("laya-quick-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let line = |question: &str, shown: bool, clicked: bool| QuickLine {
            ticket: "DEV-1".into(),
            question: question.into(),
            shown,
            clicked,
            ..QuickLine::default()
        };
        for l in [
            line("ready", true, false),
            line("ready", false, false),
            line("ready", false, true),
            line("repro", false, false),
        ] {
            log(&path, &l).unwrap();
        }
        let stats = quick_stats(&path);
        assert_eq!(stats[0], ("ready", 2, 1, 1));
        assert_eq!(stats[1], ("repro", 1, 0, 0));
        std::fs::remove_file(&path).unwrap();
    }

    /// The quick questions on five plain tickets, printed, for a look at
    /// what Laya proposes. Needs the checkpoint:
    /// Sessions' last messages, asking and done:
    /// `cargo test --release real_session_asks -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_session_asks() {
        let texts: Vec<(String, String)> = [
            ("ask-1", "I found no review feedback left to fix on the PR, but it now conflicts with main in two files. I need your call on whether to resolve that."),
            ("ask-2", "Should I open the PR now, or change something first?"),
            ("ask-3", "Which of the two options do you prefer: keep the flag or remove it?"),
            ("done-1", "Pushed the fix to the PR branch; CI is green and the ticket is updated."),
            ("done-2", "All 4 tests pass and the migration is committed as a22a4ef."),
            ("done-3", "PR opened: https://github.com/o/r/pull/12. Nothing else to do."),
        ]
        .iter()
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .collect();
        for (key, yes, confidence) in asks(&texts).unwrap() {
            println!("{key}: waits on you {yes} ({confidence:.2})");
        }
    }

    /// `cargo test --release real_quick -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_quick_answers() {
        for (title, body) in [
            (
                "Save fails with a 500 on emoji names",
                "Renaming a report to 'test 🚀' and clicking Save shows 'Internal error'. \
                 Steps: open a report, rename it with an emoji, click Save. Expected: saved.",
            ),
            (
                "Report broken",
                "My report is not working. It says disconnect.",
            ),
            (
                "How do I rotate an API key?",
                "Where in the UI can I rotate the workspace API key without downtime?",
            ),
            (
                "Add CSV export to the jobs table",
                "Customers want to export the jobs table as CSV from the Jobs page.",
            ),
            (
                "Question about rate limits",
                "What is the rate limit for the search endpoint on the enterprise plan?",
            ),
        ] {
            let answers = quick(&ticket_text(title, body, "")).unwrap();
            println!("{title}: {answers:?}");
        }
    }

    /// The built-in Laya on the hand-written notes in tests/laya-notes.json.
    /// Needs the checkpoint (Settings downloads it):
    /// `cargo test --release real_laya -- --ignored`
    #[test]
    #[ignore]
    fn real_laya_tells_bugs_from_wishes() {
        let notes: Vec<Value> =
            serde_json::from_str(include_str!("../tests/laya-notes.json")).unwrap();
        let mut wrong = Vec::new();
        for note in &notes {
            let text = note["note"].as_str().unwrap();
            let d = decide(text).unwrap();
            if d.bug != note["bug"].as_bool().unwrap() {
                wrong.push((text.to_owned(), d));
            }
        }
        // 96% on real tickets; allow one miss in ten here, none confident.
        assert!(
            wrong.len() * 10 <= notes.len(),
            "{} of {} wrong: {wrong:#?}",
            wrong.len(),
            notes.len()
        );
        assert!(
            wrong.iter().all(|(_, d)| d.confidence < 0.8),
            "a confident mistake: {wrong:#?}"
        );
    }

    /// The built-in Laya against tickets people triaged: accuracy against
    /// the always-"bug" baseline, and by confidence. Build the file with
    /// `scripts/laya-eval.py` (it writes to target/: ticket text stays out of
    /// the repo), then:
    /// `LAYA_EVAL=target/laya-eval.json cargo test --release eval -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn eval_against_labelled_tickets() {
        let path = std::env::var("LAYA_EVAL").expect("set LAYA_EVAL");
        let cases: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        // (a bug to people, a bug to Laya, confidence)
        let mut rows = Vec::new();
        for case in cases.iter().filter(|c| c["kind"].is_string()) {
            let d = decide(case["note"].as_str().unwrap()).unwrap();
            rows.push((case["kind"] == "bug", d.bug, d.confidence));
        }
        let n = rows.len();
        assert!(n > 0, "no labelled tickets");
        let share = |k: usize, of: usize| 100.0 * k as f64 / of.max(1) as f64;
        let right = rows.iter().filter(|r| r.0 == r.1).count();
        let bugs = rows.iter().filter(|r| r.0).count();
        let baseline = bugs.max(n - bugs);
        println!(
            "bug: n {n}, accuracy {:.0}%, baseline {:.0}%",
            share(right, n),
            share(baseline, n)
        );
        for min in [0.5, 0.7, 0.8, 0.9] {
            let kept: Vec<_> = rows.iter().filter(|r| r.2 >= min).collect();
            let ok = kept.iter().filter(|r| r.0 == r.1).count();
            println!(
                "  confidence >= {min}: covers {:.0}%, accuracy {:.0}%",
                share(kept.len(), n),
                share(ok, kept.len())
            );
        }
        // Worth asking only while it beats always guessing the common kind,
        // and is nearly always right where use mode would trust it.
        assert!(right >= baseline, "Laya does not beat the baseline");
        let trusted: Vec<_> = rows.iter().filter(|r| r.2 >= 0.8).collect();
        let trusted_ok = trusted.iter().filter(|r| r.0 == r.1).count();
        assert!(
            share(trusted_ok, trusted.len()) >= 95.0,
            "under 95% right at 0.8"
        );
    }
}
