//! What Claude Code knows about every session on the machine: the
//! `claude agents --json --all` list, what each transcript's last rows
//! say, and why a background session ended.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

use super::shell;

/// One session as `claude agents --json --all` reports it: interactive
/// terminals anywhere on the machine as well as background sessions.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    pub session_id: String,
    /// The short id; only background sessions have one.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub state: Option<String>,
    /// The process; only interactive sessions have one.
    #[serde(default)]
    pub pid: Option<u32>,
    /// The app an interactive session runs in ("/Applications/Ghostty.app"),
    /// or `Orbit` when this app's own terminal holds it.
    #[serde(skip)]
    pub host: Option<String>,
    /// The process's ancestors, nearest first: which shell pane holds it.
    #[serde(skip)]
    pub parents: Vec<u32>,
    /// What the transcript says, for running sessions.
    #[serde(skip)]
    pub activity: Option<Activity>,
    /// Why it ended, for background sessions that crashed or stopped.
    #[serde(skip)]
    pub failure: Option<String>,
    /// What the session's hooks last wrote, for sessions the app started.
    #[serde(skip)]
    pub report: Option<super::status::Report>,
    /// Its ticket or PR is done (the app sets it from the board): a turn
    /// that ended then reads "done", not "your turn".
    #[serde(skip)]
    pub finished: bool,
    /// When the user last had this session's terminal in front of them
    /// (`Sessions::mark_seen`): a turn that ended before then was looked at
    /// and reads "idle", not "your turn", until the next one.
    #[serde(skip)]
    pub seen_at: Option<std::time::SystemTime>,
    /// What the attached terminal's screen shows right now (screen.rs):
    /// the spinner and the permission prompt are seen before any report.
    #[serde(skip)]
    pub screen: Option<super::screen::Seen>,
}

impl Live {
    pub fn running(&self) -> bool {
        !matches!(
            self.state.as_deref(),
            Some("failed" | "stopped" | "crashed" | "done")
        )
    }

    /// "needs approval", "working", "your turn", "idle", "done" (its
    /// ticket or PR is), or how it ended.
    pub fn label(&self) -> &str {
        match self.live_label() {
            "your turn" | "idle" if self.finished => "done",
            label => label,
        }
    }

    fn live_label(&self) -> &str {
        use super::screen::Seen;
        if !self.running() {
            return self.state.as_deref().unwrap_or("stopped");
        }
        // The screen is the freshest word: a spinner or a permission prompt
        // in the attached terminal beats a report that has not landed yet.
        match self.screen {
            Some(Seen::Working) => return "working",
            Some(Seen::Blocked) => return "needs approval",
            Some(Seen::Prompt) | None => {}
        }
        if self.status == "busy" {
            return "working";
        }
        // Our own sessions say exactly what they wait for, and that wins over
        // a background session's `state`, which can stay "working" after its
        // turn ended while its `status` is already idle.
        if let Some(report) = self.report.as_ref().filter(|r| r.recent()) {
            use super::status::State;
            match report.state {
                // A prompt box on screen means the turn is over, whatever
                // a Stop hook that never ran would have said.
                State::Working if self.screen == Some(Seen::Prompt) && report.age() > 30 => {}
                State::Working => return "working",
                State::NeedsDecision => return "your turn",
                State::Turn if self.looked_at(Some(report.at)) => return "idle",
                State::Turn => return "your turn",
                State::NeedsApproval => return "needs approval",
                State::Pr | State::Failed | State::Ended | State::Note => {}
            }
        }
        if self.state.as_deref() == Some("working") {
            return "working";
        }
        // Interactive sessions report no state; the transcript says whether
        // Claude stopped at a tool it needs approved (it cannot go on) or
        // finished its turn (it could, if you answer).
        match &self.activity {
            Some(a) if a.recent() && a.turn == Turn::Tool => "needs approval",
            Some(a) if a.recent() && a.turn == Turn::Ended && self.looked_at(a.modified) => "idle",
            _ if self.state.as_deref() == Some("blocked") => "your turn",
            Some(a) if a.recent() && a.turn == Turn::Ended => "your turn",
            _ => "idle",
        }
    }

    /// Whether the user had the session in front of them since `at`: a
    /// finished turn they saw is not their turn any more, as herdr's
    /// `done` becomes `idle` once viewed.
    fn looked_at(&self, at: Option<std::time::SystemTime>) -> bool {
        matches!((self.seen_at, at), (Some(seen), Some(at)) if seen >= at)
    }

    /// Where an interactive session runs, by name: "Ghostty", "Orbit".
    pub fn host_name(&self) -> Option<&str> {
        let host = self.host.as_deref()?;
        Some(host.rsplit('/').next()?.trim_end_matches(".app"))
    }

    /// The folder's last path component, for naming a session.
    pub fn folder(&self) -> &str {
        self.cwd.rsplit('/').next().unwrap_or(&self.cwd)
    }
}

/// Where the conversation stands, read from the end of the transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    /// Claude answered with text and stopped.
    Ended,
    /// Claude called a tool that has not returned.
    Tool,
    /// Claude is thinking, or a tool result is on its way back to it.
    Working,
    /// The user sent a message.
    Prompted,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Activity {
    pub turn: Turn,
    /// The first line of Claude's last text answer.
    pub last_line: String,
    pub modified: Option<std::time::SystemTime>,
}

impl Activity {
    /// A turn that ended half a day ago is not waiting for anyone.
    fn recent(&self) -> bool {
        self.modified
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(12 * 3600))
    }

    /// "5m" since the transcript last changed.
    pub fn ago(&self) -> String {
        self.modified
            .and_then(|m| jiff::Timestamp::try_from(m).ok())
            .map(|t| crate::story::ago(&t.to_string()))
            .unwrap_or_default()
    }
}

fn list(all: bool) -> Result<Vec<Live>> {
    let mut command = shell::claude();
    command.args(["agents", "--json"]);
    if all {
        command.arg("--all");
    }
    Ok(serde_json::from_slice(&command.output()?.stdout)?)
}

/// Every session Claude Code is running, interactive or background.
pub(super) fn running() -> Result<Vec<Live>> {
    Ok(list(false)?.into_iter().filter(Live::running).collect())
}

/// Every session Claude Code knows, with what each transcript says and why
/// ended background sessions ended. Off the UI thread.
pub(super) fn rows() -> Result<Vec<Live>> {
    let mut rows = list(true)?;
    let log = config_dir()
        .map(|dir| tail(&dir.join("daemon.log"), 256 * 1024))
        .unwrap_or_default();
    let table = processes();
    for row in &mut rows {
        row.host = row.pid.and_then(|pid| host(pid, &table));
        row.parents = row
            .pid
            .map(|pid| ancestors(pid, &table))
            .unwrap_or_default();
        if row.running() {
            row.activity = transcript(&row.cwd, &row.session_id).map(|p| activity(&p));
        } else if let Some(short) = &row.id {
            row.failure = settled(&log, short);
        }
    }
    Ok(rows)
}

/// Every process: pid to (parent, command path), from one `ps`.
fn processes() -> std::collections::HashMap<u32, (u32, String)> {
    let out = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    out.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let parent = parts.next()?.parse().ok()?;
            Some((pid, (parent, parts.collect::<Vec<_>>().join(" "))))
        })
        .collect()
}

/// A process's parents up to init, nearest first.
fn ancestors(pid: u32, table: &std::collections::HashMap<u32, (u32, String)>) -> Vec<u32> {
    let mut out = Vec::new();
    let mut at = pid;
    // ponytail: 64 hops is deeper than any real process tree.
    while out.len() < 64 {
        match table.get(&at) {
            Some(&(parent, _)) if parent > 1 => {
                out.push(parent);
                at = parent;
            }
            _ => break,
        }
    }
    out
}

/// The app a process runs under: the first ancestor that is an `.app`
/// (the outermost bundle in its path), or `Orbit` when this app (any copy
/// of its binary) is the parent.
fn host(pid: u32, table: &std::collections::HashMap<u32, (u32, String)>) -> Option<String> {
    let me = std::env::current_exe().ok();
    let me = me
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    let mut at = table.get(&pid)?.0;
    // ponytail: 64 hops is deeper than any real process tree.
    for _ in 0..64 {
        let (parent, command) = table.get(&at)?;
        if let Some(end) = command.find(".app/") {
            return Some(command[..end + 4].to_owned());
        }
        if at == std::process::id() || me.is_some_and(|me| command.rsplit('/').next() == Some(me)) {
            return Some("Orbit".into());
        }
        if *parent <= 1 {
            return None;
        }
        at = *parent;
    }
    None
}

/// Brings the app the first process named `name` runs under to the front
/// (a terminal running the herdr client), off the UI thread.
pub fn show_app_of(name: &str) -> bool {
    let table = processes();
    let Some(pid) = table
        .iter()
        .find_map(|(pid, (_, command))| (command.rsplit('/').next() == Some(name)).then_some(*pid))
    else {
        return false;
    };
    let Some(app) = host(pid, &table).filter(|h| h.ends_with(".app")) else {
        return false;
    };
    std::thread::spawn(move || {
        let _ = std::process::Command::new("open")
            .args(["-a", &app])
            .output();
    });
    true
}

/// Brings the app an interactive session runs in to the front, off the UI
/// thread. False when there is no such app (unknown, or Orbit itself).
pub fn show_host(row: &Live) -> bool {
    let Some(host) = row.host.clone().filter(|h| h.ends_with(".app")) else {
        return false;
    };
    std::thread::spawn(move || {
        let _ = std::process::Command::new("open")
            .args(["-a", &host])
            .output();
    });
    true
}

/// Why a background session ended: the reason on its last "bg settled
/// <id> (crashed): …" line in Claude Code's daemon log. One that settled
/// `done` finished its work: no failure to report.
fn settled(log: &str, short: &str) -> Option<String> {
    let needle = format!("bg settled {short} ");
    log.lines()
        .rev()
        .find_map(|line| line.split_once(&needle).map(|(_, rest)| rest))
        .map(|rest| {
            rest.trim_start_matches('(')
                .replacen("):", ":", 1)
                .replacen(')', "", 1)
        })
        .filter(|reason| reason.trim() != "done" && !reason.starts_with("done:"))
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".claude")))
}

/// Up to the last `bytes` of a file, starting at a whole line.
fn tail(path: &Path, bytes: u64) -> String {
    use std::io::{Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(bytes);
    let mut buf = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    match (start, text.find('\n')) {
        (0, _) | (_, None) => text,
        (_, Some(i)) => text[i + 1..].to_owned(),
    }
}

/// Where the conversation stands and what Claude said last.
fn activity(path: &Path) -> Activity {
    let (turn, last_line) = read_turn(&tail(path, 256 * 1024));
    Activity {
        turn,
        last_line,
        modified: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
    }
}

/// Walks the transcript backwards: the last conversational row decides the
/// turn, the last assistant text gives the line.
fn read_turn(text: &str) -> (Turn, String) {
    let mut turn = Turn::Unknown;
    let mut line = String::new();
    for row in text.lines().rev() {
        let Ok(row) = serde_json::from_str::<serde_json::Value>(row) else {
            continue;
        };
        if row["isSidechain"] == true {
            continue;
        }
        let content = &row["message"]["content"];
        let first = content
            .get(0)
            .map(|b| b["type"].as_str().unwrap_or_default());
        let seen = match (row["type"].as_str(), first) {
            (Some("assistant"), Some("text")) => {
                if line.is_empty() {
                    line = content[0]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .lines()
                        .map(str::trim)
                        .find(|l| !l.is_empty())
                        .map(|l| {
                            let l = l.trim_start_matches(['#', '>', '-', '*', ' ']);
                            crate::story::shorten_links(&crate::story::plain(l))
                        })
                        .unwrap_or_default();
                }
                Turn::Ended
            }
            (Some("assistant"), Some("tool_use")) => Turn::Tool,
            (Some("assistant"), _) => Turn::Working,
            (Some("user"), Some("tool_result")) => Turn::Working,
            (Some("user"), _) => Turn::Prompted,
            _ => continue,
        };
        if turn == Turn::Unknown {
            turn = seen;
        }
        if !line.is_empty() {
            break;
        }
    }
    (turn, line)
}

/// The transcript of a session, where Claude Code files it: under the
/// working folder with every character other than letters, digits and `-`
/// turned into `-`. Falls back to a search when that guess misses.
pub(super) fn transcript(cwd: &str, session_id: &str) -> Option<PathBuf> {
    let projects = config_dir()?.join("projects");
    let folder: String = cwd
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let file = format!("{session_id}.jsonl");
    let guess = projects.join(folder).join(&file);
    if guess.exists() {
        return Some(guess);
    }
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|project| project.path().join(&file))
        .find(|path| path.exists())
}

/// What a session is about, for the Sessions search: Claude Code's title
/// for it and the user's own messages, lowercased.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct About {
    /// The session's `custom-title`, else its `ai-title`.
    pub title: Option<String>,
    pub text: String,
}

/// Enough of a long conversation to find it by.
const ABOUT_BYTES: usize = 32 * 1024;

/// `About` for each (session id, cwd), read again only when a transcript
/// changed. Off the UI thread.
pub(super) fn about(sessions: &[(String, String)]) -> std::collections::HashMap<String, About> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::SystemTime;
    static SEEN: Mutex<Option<HashMap<PathBuf, (SystemTime, About)>>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let seen = seen.get_or_insert_with(HashMap::new);
    let mut out = HashMap::new();
    for (id, cwd) in sessions {
        let Some(path) = transcript(cwd, id) else {
            continue;
        };
        let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
            continue;
        };
        let about = match seen.get(&path) {
            Some((at, about)) if *at == modified => about.clone(),
            _ => {
                let about = read_about(&std::fs::read_to_string(&path).unwrap_or_default());
                seen.insert(path, (modified, about.clone()));
                about
            }
        };
        out.insert(id.clone(), about);
    }
    out
}

fn read_about(transcript: &str) -> About {
    let (mut custom, mut ai, mut text) = (None, None, String::new());
    for row in transcript.lines() {
        let Ok(row) = serde_json::from_str::<serde_json::Value>(row) else {
            continue;
        };
        let words = match row["type"].as_str() {
            Some("custom-title") => {
                custom = row["customTitle"].as_str().map(String::from);
                continue;
            }
            Some("ai-title") => {
                ai = row["aiTitle"].as_str().map(String::from);
                continue;
            }
            Some("user") if row["isMeta"] != true && row["isSidechain"] != true => {
                let content = &row["message"]["content"];
                match content.as_str() {
                    Some(t) => t.to_owned(),
                    None => content
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|b| b["type"] == "text")
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                }
            }
            _ => continue,
        };
        if text.len() < ABOUT_BYTES {
            text.push_str(&words.to_lowercase());
            text.push('\n');
        }
    }
    let title = custom.or(ai);
    if let Some(title) = &title {
        text.insert_str(0, &format!("{}\n", title.to_lowercase()));
    }
    About { title, text }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interactive_session_is_found_in_the_app_it_runs_in() {
        let table: std::collections::HashMap<u32, (u32, String)> = [
            (10, (9, "claude".to_owned())),
            (9, (8, "-/bin/zsh".to_owned())),
            (8, (1, "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper".to_owned())),
            (20, (19, "claude".to_owned())),
            (19, (1, "/usr/sbin/sshd".to_owned())),
        ]
        .into();
        assert_eq!(
            host(10, &table).as_deref(),
            Some("/Applications/Visual Studio Code.app")
        );
        assert_eq!(host(20, &table), None);
        let row = Live {
            host: host(10, &table),
            ..serde_json::from_str(r#"{"sessionId":"s"}"#).unwrap()
        };
        assert_eq!(row.host_name(), Some("Visual Studio Code"));
    }

    #[test]
    fn a_transcript_is_found_by_its_title_and_the_users_words() {
        let rows = [
            r#"{"type":"user","message":{"role":"user","content":"Ship the workers to prod"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"caveat"}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"secret output"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"text","text":"and the deploy config"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"sure"}]}}"#,
            r#"{"type":"ai-title","aiTitle":"Workers prod rollout"}"#,
        ];
        let about = read_about(&rows.join("\n"));
        assert_eq!(about.title.as_deref(), Some("Workers prod rollout"));
        assert!(about.text.contains("ship the workers to prod"));
        assert!(about.text.contains("deploy config"));
        assert!(!about.text.contains("caveat"));
        assert!(!about.text.contains("secret output"));
        assert!(!about.text.contains("sure"));
        let named = read_about(&format!(
            "{}\n{}",
            rows[5], r#"{"type":"custom-title","customTitle":"DEV-1 fix"}"#
        ));
        assert_eq!(named.title.as_deref(), Some("DEV-1 fix"));
    }

    #[test]
    fn a_finished_turn_on_done_work_reads_done() {
        let mut row: Live = serde_json::from_value(serde_json::json!({
            "sessionId": "s", "status": "idle", "state": "blocked"
        }))
        .unwrap();
        assert_eq!(row.label(), "your turn");
        row.finished = true;
        assert_eq!(row.label(), "done");
        row.status = "busy".into();
        assert_eq!(row.label(), "working", "still working is still working");
    }

    /// The board run's row after its run: Claude Code says idle, its
    /// background `state` still says working, its Stop hook wrote the turn.
    #[test]
    fn a_finished_turn_wins_over_a_background_state_left_at_working() {
        use super::super::status::{Report, State};
        let mut row: Live = serde_json::from_value(serde_json::json!({
            "sessionId": "s", "kind": "background", "status": "idle", "state": "working"
        }))
        .unwrap();
        assert_eq!(row.label(), "working", "no report yet: Claude Code's word");
        row.report = Some(Report {
            at: std::time::SystemTime::now(),
            state: State::Turn,
            text: "Run done".into(),
        });
        assert_eq!(row.label(), "your turn");
        row.status = "busy".into();
        assert_eq!(row.label(), "working", "busy is busy");
    }

    #[test]
    fn a_turn_looked_at_reads_idle_until_the_next_one() {
        use super::super::status::{Report, State};
        use std::time::{Duration, SystemTime};
        let mut row: Live = serde_json::from_value(serde_json::json!({
            "sessionId": "s", "status": "idle"
        }))
        .unwrap();
        let at = SystemTime::now() - Duration::from_secs(60);
        row.report = Some(Report {
            at,
            state: State::Turn,
            text: "Done.".into(),
        });
        assert_eq!(row.label(), "your turn");
        row.seen_at = Some(at - Duration::from_secs(1));
        assert_eq!(row.label(), "your turn", "looked at before the turn ended");
        row.seen_at = Some(at + Duration::from_secs(1));
        assert_eq!(row.label(), "idle");
        // A question stays the user's turn however often they look at it.
        row.report.as_mut().unwrap().state = State::NeedsDecision;
        assert_eq!(row.label(), "your turn");
        // The next turn is new again.
        row.report = Some(Report {
            at: at + Duration::from_secs(10),
            state: State::Turn,
            text: "And this.".into(),
        });
        assert_eq!(row.label(), "your turn");
        // The screen beats a report: a spinner means working, a permission
        // prompt means needs approval, whatever the hooks last wrote.
        row.screen = Some(super::super::screen::Seen::Working);
        assert_eq!(row.label(), "working");
        row.screen = Some(super::super::screen::Seen::Blocked);
        assert_eq!(row.label(), "needs approval");
        // A prompt box while the hooks still say working: a Stop hook that
        // never ran, once the report is old enough.
        row.screen = Some(super::super::screen::Seen::Prompt);
        row.report = Some(Report {
            at: SystemTime::now() - Duration::from_secs(5),
            state: State::Working,
            text: "go".into(),
        });
        assert_eq!(row.label(), "working");
        row.report.as_mut().unwrap().at = SystemTime::now() - Duration::from_secs(60);
        assert_eq!(row.label(), "idle");
    }

    #[test]
    fn transcript_tail_says_who_is_waiting() {
        let rows = [
            r#"{"type":"user","message":{"role":"user","content":"fix it"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"thinking"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"\n**Done.** Pushed the fix to hub#7.\nMore."}]}}"#,
            r#"{"type":"system","subtype":"turn_duration"}"#,
            r#"{"type":"last-prompt","lastPrompt":"fix it"}"#,
        ];
        assert_eq!(
            read_turn(&rows.join("\n")),
            (Turn::Ended, "Done. Pushed the fix to hub#7.".into())
        );

        let tool = [
            rows[0],
            rows[2],
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use"}]}}"#,
        ];
        assert_eq!(read_turn(&tool.join("\n")).0, Turn::Tool);

        let log = "[t] [bg] bg claimed-spare 6aa39c8e (shell)\n[t] [bg] bg settled 6aa39c8e (crashed): source session 0b4d not found\n";
        assert_eq!(
            settled(log, "6aa39c8e").as_deref(),
            Some("crashed: source session 0b4d not found")
        );
        // Settled done: it finished, nothing failed.
        let done = "[t] [bg] bg settled 1a2b3c4d (done)\n";
        assert_eq!(settled(done, "1a2b3c4d"), None);
    }
}
