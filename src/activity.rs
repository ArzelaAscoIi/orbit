//! The activity log: what the user did, for learning how the app is used.
//! Local only, `<data>/activity.jsonl`, one JSON line per action: when, the
//! action's name, the view and tab it happened in, and whether a key or a
//! click did it (`via`). Never what was typed,
//! a ticket's text, or a key, so a user can share the file as is. Off with
//! settings.json `activityLog: false`.
//!
//! Every copy of the app only appends, so nothing is read-modify-write. At
//! 5 MB the file moves to `activity.1.jsonl` and a new one starts.
//!
//! Groundwork for an operator (an agent that works the board the way the
//! user does, stepping back only where a human must approve): every action
//! has a `Gate` (`GATES`, held complete by a test), written on its line, and
//! a decision carries `ctx`, the situation it was taken in (the ticket's
//! lane, stage and ask, CI, the PR gate's facts, the session's state) as
//! enums and counts, with the target's id hashed: what to learn "when this,
//! the user does that" from, still with nothing anyone typed in it.

use std::io::Write;
use std::path::Path;

const FILE: &str = "activity.jsonl";
const LIMIT: u64 = 5 * 1024 * 1024;

/// A value's variant name, from its `Debug`: `Show(Story("…"))` is `Show`.
/// The name of a type the app defines, never the user's text inside it.
pub fn name(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}")
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Who may take an action, for the operator to come: what the user's own
/// clicks are reserved for, what it may only propose, what it may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Moving around and reading: windows, tabs, the sheet, opening a
    /// ticket. Nothing changes; an operator does not need it.
    Look,
    /// The app itself: keys, teams, settings, the profile, updates. Never
    /// an operator's.
    App,
    /// Starts or steers work that asks before it acts (a skill in the
    /// ticket's session, a board run, an investigation), or changes only
    /// local state. An operator may do it.
    Do,
    /// Changes something other people see (a Linear state or comment, a
    /// PR's label, reviewer or draft state, a new ticket, a closed PR): an
    /// operator proposes it, the user clicks, until it matches them.
    Queue,
    /// Approving, merging, answering a session's permission prompt: only
    /// the user's click, bound to what they saw (CLAUDE.md Safety).
    Yours,
}

/// Every `Action` variant's gate. The test in app/mod.rs holds it to the
/// enum, so a new action is classified the day it is added.
pub const GATES: &[(&str, Gate)] = &[
    // Yours.
    ("Approve", Gate::Yours),
    ("Merge", Gate::Yours),
    ("AutoMerge", Gate::Yours),
    ("LandBatch", Gate::Yours),
    ("CodexAnswer", Gate::Yours),
    // Queue: seen by others.
    ("SetState", Gate::Queue),
    ("MoveTeam", Gate::Queue),
    ("SendComment", Gate::Queue),
    ("DraftComment", Gate::Look),
    ("PalettePicked", Gate::Look),
    ("LoadReviewers", Gate::Look),
    ("AskBot", Gate::Queue),
    ("Yolo", Gate::Queue),
    ("ClosePr", Gate::Queue),
    ("RequestReview", Gate::Queue),
    ("AddLabel", Gate::Queue),
    ("ReadyPr", Gate::Queue),
    ("Draft", Gate::Queue),
    ("DiscardDrafts", Gate::Queue),
    ("Review", Gate::Queue),
    ("ReviewSummary", Gate::Yours),
    ("UpdateBranch", Gate::Queue),
    ("CancelAutoMerge", Gate::Queue),
    ("AiReview", Gate::Queue),
    ("Quick", Gate::Queue),
    ("TestPlan", Gate::Queue),
    ("CreateTicket", Gate::Queue),
    ("AddToDuplicate", Gate::Queue),
    ("FileAsIs", Gate::Queue),
    // Do: work that asks before it acts, or local.
    ("RunSkill", Gate::Do),
    ("Claude", Gate::Do),
    ("Triage", Gate::Do),
    ("Attach", Gate::Do),
    ("WithAgent", Gate::Do),
    ("FreshSession", Gate::Do),
    ("StopSession", Gate::Do),
    ("CloseSession", Gate::Do),
    ("Operate", Gate::Do),
    ("Drive", Gate::Queue),
    ("StopDrive", Gate::Do),
    ("Instruct", Gate::Do),
    ("ToggleChat", Gate::Look),
    ("NewChat", Gate::Do),
    ("ChatShow", Gate::Look),
    ("Steer", Gate::Do),
    ("CloneWindow", Gate::Look),
    ("Clone", Gate::Do),
    ("PassTask", Gate::App),
    ("NewSession", Gate::Do),
    ("CodexPrompt", Gate::Do),
    ("Guide", Gate::Do),
    ("GuideStep", Gate::Do),
    ("AskPr", Gate::Do),
    ("NoteForApproval", Gate::Do),
    ("SetApprovalNote", Gate::Do),
    ("HandleInbox", Gate::Do),
    ("InboxDone", Gate::Do),
    ("MarkRead", Gate::Do),
    ("FindWork", Gate::Do),
    ("ClearKtlo", Gate::Do),
    ("WakeBot", Gate::Do),
    ("ShipReviews", Gate::Do),
    ("Orchestrate", Gate::Do),
    ("Autopilot", Gate::Do),
    ("SpotWork", Gate::Do),
    ("FindGoals", Gate::Do),
    ("Investigate", Gate::Do),
    ("InvestigateAlert", Gate::Do),
    ("FixRelease", Gate::Do),
    ("Debug", Gate::Do),
    ("ReadNote", Gate::Do),
    ("FixNote", Gate::Do),
    ("NewTicket", Gate::Do),
    ("NewTicketWith", Gate::Do),
    ("CloseCapture", Gate::Do),
    ("ViewedFile", Gate::Do),
    ("Refresh", Gate::Do),
    ("ReloadDiff", Gate::Do),
    ("RefreshGate", Gate::Look),
    ("ReviewAgain", Gate::Do),
    ("RefreshRepo", Gate::Do),
    ("HerdrStart", Gate::Do),
    // Look.
    ("Show", Gate::Look),
    ("Sheet", Gate::Look),
    ("SheetAt", Gate::Look),
    ("OpenTicket", Gate::Look),
    ("NextForYou", Gate::Look),
    ("GoWindow", Gate::Look),
    ("Select", Gate::Look),
    ("Focus", Gate::Look),
    ("Open", Gate::Look),
    ("Link", Gate::Look),
    ("Tab", Gate::Look),
    ("DiffPr", Gate::Look),
    ("UnifiedDiff", Gate::Look),
    ("DiffAll", Gate::Look),
    ("SplitClaude", Gate::Look),
    ("FoldFile", Gate::Look),
    ("FoldFolder", Gate::Look),
    ("OpenSession", Gate::Look),
    ("InboxOpen", Gate::Look),
    ("OpenPr", Gate::Look),
    ("NewTab", Gate::Look),
    ("RenameWindow", Gate::Look),
    ("MoveWindow", Gate::Look),
    ("TogglePalette", Gate::Look),
    ("PaletteQuery", Gate::Look),
    ("DismissToast", Gate::Look),
    ("SessionsFiltered", Gate::Look),
    ("SheetQuery", Gate::Look),
    ("HerdrFocus", Gate::Look),
    ("Find", Gate::Look),
    ("Split", Gate::Look),
    ("CloseWindow", Gate::Look),
    ("ReopenWindow", Gate::Look),
    ("ClosePane", Gate::Look),
    ("FocusPane", Gate::Look),
    ("CyclePane", Gate::Look),
    ("ZoomPane", Gate::Look),
    ("FullScreen", Gate::Look),
    ("PaneRatio", Gate::Look),
    ("NewShell", Gate::Look),
    ("ShellWindow", Gate::Look),
    ("EditorWindow", Gate::Look),
    ("SaveEditor", Gate::Do),
    ("NewWindow", Gate::Look),
    ("BrowserGo", Gate::Look),
    ("BrowserBack", Gate::Look),
    ("BrowserReload", Gate::Look),
    ("BrowserFindOpen", Gate::Look),
    ("BrowserFindClose", Gate::Look),
    ("BrowserFind", Gate::Look),
    ("BrowserWindow", Gate::Look),
    ("LoadImage", Gate::Look),
    ("Tutorial", Gate::Look),
    // App.
    ("SignIn", Gate::App),
    ("SignInWithLinear", Gate::App),
    ("SignOut", Gate::App),
    ("SaveKey", Gate::App),
    ("PickTeams", Gate::App),
    ("ToggleTeam", Gate::App),
    ("SaveTeams", Gate::App),
    ("SetTheme", Gate::App),
    ("SaveSettings", Gate::App),
    ("SetLocalAgent", Gate::App),
    ("RevertSettings", Gate::App),
    ("DownloadLaya", Gate::App),
    ("Update", Gate::App),
    ("ImportProfile", Gate::App),
    ("ClearProfile", Gate::App),
    ("UseProfile", Gate::App),
    ("Reveal", Gate::App),
    ("SetActivityLog", Gate::App),
    ("SaveProfile", Gate::App),
    ("RevertProfile", Gate::App),
    ("SaveBotSetup", Gate::App),
    ("RevertBotSetup", Gate::App),
    ("SetUpProfile", Gate::App),
    ("SetLayaMode", Gate::App),
    ("Improve", Gate::App),
    ("UseHerdrConfig", Gate::App),
    ("SetHerdrKeys", Gate::App),
    ("InstallHerdrPlugin", Gate::App),
    ("SetSound", Gate::App),
];

/// An action's gate, by its name (`name`).
pub fn gate(event: &str) -> Option<Gate> {
    GATES.iter().find(|(n, _)| *n == event).map(|(_, g)| *g)
}

/// A target's id as the log keeps it: a short hash, so a decision can be
/// followed to what came of it without the log naming the ticket or PR.
pub fn hashed(id: &str) -> String {
    format!("{:08x}", crate::store::text_hash(id) as u32)
}

/// The line for one action, `detail` being the variant of its argument when
/// that says where it went (Show's view, Tab's tab), and `via` `key`,
/// `click` or empty (the app did it). `ctx`: the situation a decision was
/// taken in (App::situation), enums and counts only.
pub fn line(
    at_ms: u128,
    event: &str,
    detail: &str,
    view: &str,
    tab: &str,
    via: &str,
    ctx: Option<serde_json::Value>,
) -> String {
    let mut line = serde_json::json!({
        "t": at_ms, "event": event, "detail": detail, "view": view, "tab": tab, "via": via,
    });
    if let Some(gate) = gate(event) {
        line["gate"] = name(&gate).to_lowercase().into();
    }
    if let Some(ctx) = ctx {
        line["ctx"] = ctx;
    }
    line.to_string()
}

/// Appends a line; a log that cannot be written is skipped, never an error.
pub fn append(dir: &Path, line: &str) {
    let path = dir.join(FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LIMIT) {
        let _ = std::fs::rename(&path, dir.join("activity.1.jsonl"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{line}");
    }
}

pub fn path(dir: &Path) -> std::path::PathBuf {
    dir.join(FILE)
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    #[allow(dead_code)]
    enum Example {
        Show(Where),
        NewTicketWith(String),
        Save,
    }
    #[derive(Debug)]
    #[allow(dead_code)]
    enum Where {
        Story(String),
    }

    #[test]
    fn only_names_are_logged_never_what_was_typed() {
        let typed = Example::NewTicketWith("customer Acme cannot log in, key sk-ant-123".into());
        assert_eq!(name(&typed), "NewTicketWith");
        assert_eq!(
            name(&Example::Show(Where::Story("secret plans".into()))),
            "Show"
        );
        assert_eq!(name(&Where::Story("x".into())), "Story");
        assert_eq!(name(&Example::Save), "Save");
        let l = line(1, "NewTicketWith", "", "ForYou", "Walkthrough", "key", None);
        assert!(!l.contains("Acme") && !l.contains("sk-ant"));
        // A decision carries its gate and its situation, the id hashed.
        let ctx = serde_json::json!({"kind": "pr", "id": hashed("https://github.com/o/r/pull/1")});
        let l: serde_json::Value = serde_json::from_str(&line(
            2,
            "Approve",
            "",
            "ForYou",
            "Diff",
            "click",
            Some(ctx),
        ))
        .unwrap();
        assert_eq!(l["gate"], "yours");
        assert_eq!(l["ctx"]["kind"], "pr");
        assert!(!l.to_string().contains("github.com"));
        assert_eq!(gate("GoWindow"), Some(Gate::Look));
        assert_eq!(gate("NoSuchAction"), None);
    }

    #[test]
    fn the_log_appends_and_moves_aside_when_big() {
        let dir = std::env::temp_dir().join(format!("orbit-activity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        append(&dir, &line(1, "Show", "Stories", "ForYou", "", "key", None));
        append(&dir, &line(2, "Refresh", "", "Stories", "", "click", None));
        let text = std::fs::read_to_string(path(&dir)).unwrap();
        assert_eq!(text.lines().count(), 2);
        let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(
            (
                first["event"].as_str(),
                first["detail"].as_str(),
                first["via"].as_str()
            ),
            (Some("Show"), Some("Stories"), Some("key"))
        );
        // Over the limit: moved aside, a new file starts.
        std::fs::write(path(&dir), vec![b'x'; (LIMIT + 1) as usize]).unwrap();
        append(&dir, &line(3, "Refresh", "", "Stories", "", "", None));
        assert!(dir.join("activity.1.jsonl").is_file());
        assert_eq!(
            std::fs::read_to_string(path(&dir)).unwrap().lines().count(),
            1
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
