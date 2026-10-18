//! The app's files: where they live, how they are read and written, and
//! the hash that says whether a cached result still matches its source.

use std::ffi::OsString;
use std::path::PathBuf;

/// `ORBIT_<name>`, else `CONTROL_ROOM_<name>`: the app's old name, so a
/// setup made before the rename keeps working.
pub fn env_os(name: &str) -> Option<OsString> {
    std::env::var_os(format!("ORBIT_{name}"))
        .or_else(|| std::env::var_os(format!("CONTROL_ROOM_{name}")))
}

/// Where the app keeps its files. `ORBIT_DATA` points a test run at
/// a folder of its own, away from the real sessions and caches.
pub fn data_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        if let Some(dir) = env_os("DATA") {
            return PathBuf::from(dir);
        }
        let dir_of = |name: &str| {
            directories::ProjectDirs::from("", "", name).map(|d| d.data_dir().to_path_buf())
        };
        let Some(dir) = dir_of(crate::NAME) else {
            return PathBuf::from(".orbit");
        };
        let previous = crate::PREVIOUS_NAMES.iter().filter_map(|n| dir_of(n));
        moved(dir, previous)
    })
    .clone()
}

/// After a rename, the data folder a user already has moves to the new
/// name, and a link at the old path points to it: saved sessions hold
/// absolute paths into it, and Claude Code keys their transcripts by those
/// paths, so both keep resolving. A new user just gets the new folder.
fn moved(dir: PathBuf, previous: impl IntoIterator<Item = PathBuf>) -> PathBuf {
    if dir.exists() {
        return dir;
    }
    // A real folder, not the link an earlier move left.
    let old = previous
        .into_iter()
        .find(|old| old.is_dir() && !old.is_symlink());
    if let Some(old) = old
        && std::fs::rename(&old, &dir).is_ok()
    {
        let _ = std::os::unix::fs::symlink(&dir, &old);
    }
    dir
}

/// A JSON file in the data folder, or the default when it is missing or
/// unreadable. A broken cache starts over rather than failing.
pub fn load<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    load_at(&data_dir().join(name))
}

pub fn load_at<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Written to a temporary file and renamed, so a crash never leaves half
/// a file behind.
pub fn save<T: serde::Serialize>(name: &str, value: &T) -> std::io::Result<()> {
    save_at(&data_dir().join(name), value)
}

pub fn save_at<T: serde::Serialize>(path: &std::path::Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(tmp, path)
}

/// FNV-1a: the same on every build and machine, so a note written by one
/// teammate's app reads as fresh or stale the same way in another's.
pub fn text_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `settings.json`, the one place the app is configured, like VS Code's:
/// edited in Settings (or any editor; it is re-read when it changes).
/// Every field has a default, so a partial or older file still loads.
///
/// Keys pasted into it (`anthropicApiKey`, `incidentIoApiKey`,
/// `githubToken`) move to the Keychain when saved from the app; the file
/// then says `SAVED` instead, so no key sits in it or on screen.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Keys of the Linear teams to watch.
    pub teams: Vec<String>,
    /// The profile (profile.rs): a folder used in place, or a `.json` file
    /// or `https://` URL imported from. Empty: generic.
    pub profile: String,
    /// The palette: phosphor, amber, graphite, slate (the default), paper or system (theme.rs).
    pub theme: crate::theme::Choice,
    /// Agent used for newly created local sessions. Saved sessions keep theirs.
    pub local_agent: crate::claude::Agent,
    /// The palette's Debug actions: one pane per tool, each after `setup`.
    pub debug: Vec<DebugEnv>,
    /// What Investigate opens beside the incident's Claude session.
    pub investigate: DebugEnv,
    /// How Plan my day looks (the spot skill): the user's own tuning.
    pub work: Work,
    /// The clone (its chat on the right, Cmd+J; its hourly passes; its
    /// missions): the skills it works with, and what it should always know.
    pub clone: Clone,
    /// This week's goals in the user's own words, for Plan my day: they
    /// win over what the goals run finds (goals.rs). Empty: the run decides.
    pub goals: String,
    /// A local Laya server for the New ticket intake's decisions (laya.rs).
    pub laya: Laya,
    /// The local activity log (activity.rs): on unless set to false.
    pub activity_log: Option<bool>,
    /// Mac notifications when something needs the user (notify.rs): on
    /// unless set to false.
    pub notifications: Option<bool>,
    /// The menu bar icon (tray.rs): sessions, what needs you and For you
    /// one click away; on unless set to false.
    pub menu_bar: Option<bool>,
    /// The skills there were when the profile was last set up with Claude;
    /// one not in it is new, and For you offers to update the profile.
    pub profile_skills: Option<Vec<String>>,
    /// Where UI test plans open (the Testing tab's browser).
    pub test_app_url: String,
    /// The GitHub Actions workflows that release to prod (repo + workflow),
    /// each with its latest run on the Alerts view (releases.rs).
    pub releases: Vec<crate::releases::Release>,
    /// Repositories (`owner/repo`) pinned on the Repos view and in Cmd+K,
    /// on top of the ones your PRs, the board's PRs and the watched
    /// deploys name.
    pub repos: Vec<String>,
    /// Haiku and Sonnet for the helpers (ai.rs); moves to the Keychain on save.
    pub anthropic_api_key: String,
    /// incident.io's REST API (incidents.rs); moves to the Keychain on save.
    pub incident_io_api_key: String,
    /// GitHub, where `gh` is not signed in (github.rs); moves to the Keychain on save.
    pub github_token: String,
    /// Use the user's herdr config (herdr.rs): its prefix keys, and what
    /// "Use my herdr config" copied. Unset: on when a herdr config exists.
    pub herdr: Option<bool>,
    /// A sound when a session waits on the user: a prompt to approve (always)
    /// or a turn that ended (only while Orbit is not in front). Off unless set.
    pub sound: Option<bool>,
    /// The finished-turn sound file; empty: macOS's Glass.
    pub done_sound: String,
    /// The needs-approval sound file; empty: macOS's Ping.
    pub request_sound: String,
    /// Unattended runs (Run the board) may use broad tools without asking:
    /// any gh or git command, python3, curl, base64. Off: only the gh and git
    /// subcommands the skill needs; gh api writes and pushes to main refused.
    pub broad_unattended_tools: bool,
    /// Every folder an Orbit shell pane is in (home included) is marked
    /// trusted for Claude Code. Off: only Orbit's own session folders
    /// (<data>/work, <data>/repos); a repo you cd into asks in Claude Code.
    pub trust_shell_folders: bool,
    /// Never clean up old session folders. Off (the default): once a day
    /// Orbit removes what can be rebuilt or is finished in <data>/work and
    /// <data>/repos/.deps (cleanup.rs); unpushed or uncommitted work stays.
    pub no_cleanup: bool,
}

/// Laya (laya.rs), optional and exchangeable with Haiku for the New ticket
/// intake's bug-or-not. It runs inside the app once downloaded (Settings).
/// `mode`: `use` (Laya decides when its confidence is at or over
/// `min_confidence`, Haiku otherwise; the default, and the app downloads it
/// once on its own), `shadow` (Haiku decides, Laya is compared) or `off`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Laya {
    pub mode: String,
    pub min_confidence: f64,
}

impl Default for Laya {
    fn default() -> Self {
        Self {
            mode: "use".into(),
            min_confidence: 0.8,
        }
    }
}

impl Laya {
    /// `Some(use it)` while it is on.
    pub fn on(&self) -> Option<bool> {
        match self.mode.trim() {
            "shadow" => Some(false),
            "use" => Some(true),
            _ => None,
        }
    }
}

/// Plan my day, tuned per person: `instructions` go with every run (a
/// role, what to favour or skip), and each mode is its own button that adds
/// its instructions on top.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Work {
    pub instructions: String,
    pub modes: Vec<WorkMode>,
    /// How many of the plan's `now` spots run on their own at once; 0 starts
    /// none (each waits for Fix now).
    #[serde(rename = "autoStart")]
    pub auto_start: usize,
}

/// settings.json `clone`: what the user's clone works with.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Clone {
    /// Skill folders its chats and passes lead with (generic or the
    /// profile's: operate, mission, ship, …); empty: operate and
    /// mission.
    pub skills: Vec<String>,
    /// What it should always know or do, in the user's words.
    pub instructions: String,
    /// The hourly pass's jobs (claude::PASS_TASKS: alerts, notifications,
    /// foryou, prs, sessions, initiatives); unset: all of them; an empty
    /// list: none, the pass is paused.
    pub tasks: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WorkMode {
    pub name: String,
    pub instructions: String,
}

impl Default for Work {
    fn default() -> Self {
        let mode = |name: &str, instructions: &str| WorkMode {
            name: name.into(),
            instructions: instructions.into(),
        };
        Self {
            instructions: String::new(),
            auto_start: 2,
            modes: vec![
                mode(
                    "Maintenance sweep",
                    "Only maintenance: CVE and Security tickets with no progress, alerts that \
                     repeat, incident follow-ups, flaky or red CI. Prefer fixes a session \
                     can finish now.",
                ),
                mode(
                    "Product research",
                    "Only product work: before each spot, research it (the tickets, support \
                     asks, customer mentions in Slack, usage) and put the evidence in the \
                     description. Prefer tickets for the team over quick fixes.",
                ),
            ],
        }
    }
}

impl Work {
    /// What follows the spot command: the standing instructions, then the
    /// mode's, if one was picked.
    pub fn tuning(&self, mode: Option<&str>) -> String {
        let mut out = Vec::new();
        if !self.instructions.trim().is_empty() {
            out.push(self.instructions.trim().to_owned());
        }
        if let Some(mode) = mode.and_then(|m| self.modes.iter().find(|x| x.name == m)) {
            out.push(format!("Mode: {}. {}", mode.name, mode.instructions.trim()));
        }
        out.join("\n\n")
    }
}

/// An environment to debug in: `setup` runs first in every pane but
/// Claude's (Granted's `assume prod`, a kubectl context, a VPN), then each
/// pane's tool. Claude starts plain: it reaches the environment through
/// its own tools, not the shell's credentials.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DebugEnv {
    pub name: String,
    pub setup: String,
    pub panes: Vec<String>,
}

impl DebugEnv {
    /// What a pane types: the setup, then the tool; Claude alone.
    pub fn command(&self, tool: &str) -> String {
        let claude = tool.split_whitespace().next() == Some("claude");
        match self.setup.trim() {
            "" => tool.to_owned(),
            _ if claude => tool.to_owned(),
            setup => format!("{setup} && {tool}"),
        }
    }
}

/// A file the app follows: `changed` says whether its time changed since
/// the last look, looking at most every `every`. True on the first look.
#[derive(Debug, Default)]
pub struct Watch {
    checked: Option<std::time::Instant>,
    seen: Option<Option<std::time::SystemTime>>,
}

impl Watch {
    pub fn changed(&mut self, path: &std::path::Path, every: std::time::Duration) -> bool {
        if self.checked.is_some_and(|at| at.elapsed() < every) {
            return false;
        }
        self.checked = Some(std::time::Instant::now());
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let changed = self.seen != Some(modified);
        self.seen = Some(modified);
        changed
    }

    /// The next look counts as a change: after the app wrote the file.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// settings.json over the profile's settings (profile.rs): what the file
/// sets wins, what it leaves out comes from the profile, then the default.
/// Read after the profile is in use.
pub fn load_settings() -> Settings {
    let file: serde_json::Value = load("settings.json");
    over_profile(file, &crate::profile::get().settings)
}

/// Writes settings.json with only what differs from the defaults (the
/// profile's settings over the app's), so a setting the user never changed
/// keeps following the profile.
pub fn save_settings(settings: &Settings) -> std::io::Result<()> {
    save(
        "settings.json",
        &own_part(settings, &crate::profile::get().settings),
    )
}

fn own_part(settings: &Settings, profile: &serde_json::Value) -> serde_json::Value {
    let baseline =
        serde_json::to_value(over_profile(serde_json::json!({}), profile)).unwrap_or_default();
    let mut own = serde_json::Map::new();
    if let Ok(serde_json::Value::Object(all)) = serde_json::to_value(settings) {
        for (key, value) in all {
            if baseline.get(&key) != Some(&value) {
                own.insert(key, value);
            }
        }
    }
    own.into()
}

fn over_profile(file: serde_json::Value, profile: &serde_json::Value) -> Settings {
    let mut merged = match profile {
        serde_json::Value::Object(p) => p.clone(),
        _ => serde_json::Map::new(),
    };
    // Keys never come from a profile.
    merged.retain(|k, _| !(k.ends_with("ApiKey") || k.ends_with("Token") || k == "profile"));
    if let serde_json::Value::Object(file) = file {
        merged.extend(file);
    }
    serde_json::from_value(merged.into()).unwrap_or_default()
}

/// What a key field says once its key is in the Keychain.
pub const SAVED: &str = "SAVED";

impl Settings {
    /// Takes the keys typed into the file out of it: `(which, key)` for
    /// each new one, `(which, "")` for one cleared to remove it. The fields
    /// that held a key now say `SAVED`.
    pub fn take_keys(&mut self) -> Vec<(crate::secrets::Secret, String)> {
        use crate::secrets::Secret;
        let mut out = Vec::new();
        for (which, field) in [
            (Secret::Anthropic, &mut self.anthropic_api_key),
            (Secret::IncidentIo, &mut self.incident_io_api_key),
            (Secret::Github, &mut self.github_token),
        ] {
            let value = field.trim().to_owned();
            if value == SAVED {
                continue;
            }
            out.push((which, value.clone()));
            *field = if value.is_empty() {
                String::new()
            } else {
                SAVED.into()
            };
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every settings.json field says what it is for, right where it is
    /// declared: the Settings view shows the file with the defaults filled
    /// in, and a field without a sentence is one nobody can set with
    /// confidence (herdr diffs its config keys against its reference the
    /// same way).
    #[test]
    fn every_settings_field_has_a_doc_comment() {
        let source = include_str!("store.rs");
        let body = source
            .split("pub struct Settings {")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap();
        let lines: Vec<&str> = body.lines().map(str::trim).collect();
        let mut bare = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("pub ")
                && !lines[..i]
                    .iter()
                    .rev()
                    .take_while(|l| l.starts_with("///") || l.starts_with("#["))
                    .any(|l| l.starts_with("///"))
            {
                bare.push(*line);
            }
        }
        assert!(
            bare.is_empty(),
            "settings fields without a doc comment: {bare:?}"
        );
    }

    #[test]
    fn an_old_settings_file_loads_and_keys_move_out_of_it() {
        // The file before this change had only teams and theme.
        let old: Settings = serde_json::from_str(r#"{"teams":["DEV"],"theme":"paper"}"#).unwrap();
        assert_eq!(old.teams, vec!["DEV"]);
        assert!(
            old.debug.is_empty() && old.test_app_url.is_empty(),
            "no company defaults"
        );
        let prod = DebugEnv {
            name: "prod".into(),
            setup: "login prod".into(),
            panes: vec!["k9s".into()],
        };
        assert_eq!(prod.command("k9s"), "login prod && k9s");
        assert_eq!(prod.command("claude"), "claude", "Claude starts plain");
        assert_eq!(DebugEnv::default().command("claude"), "claude");
        let work = Work {
            instructions: "I lead API.".into(),
            ..Work::default()
        };
        assert_eq!(work.tuning(None), "I lead API.");
        assert!(
            work.tuning(Some("Maintenance sweep"))
                .starts_with("I lead API.\n\nMode: Maintenance sweep. Only maintenance")
        );
        assert_eq!(Work::default().tuning(Some("no such mode")), "");
        // Laya is on (use) unless a mode says otherwise; off turns it off;
        // an older file's url or python (from when it ran outside the app)
        // is ignored.
        assert_eq!(old.laya.on(), Some(true));
        let off: Laya = serde_json::from_str(r#"{"mode":"off"}"#).unwrap();
        assert_eq!(off.on(), None);
        let laya: Laya = serde_json::from_str(r#"{"mode":"use","python":"/x"}"#).unwrap();
        assert_eq!((laya.on(), laya.min_confidence), (Some(true), 0.8));
        let shadow: Laya = serde_json::from_str(r#"{"mode":"shadow"}"#).unwrap();
        assert_eq!(shadow.on(), Some(false));

        let mut edited = Settings {
            anthropic_api_key: "sk-ant-new".into(),
            incident_io_api_key: SAVED.into(),
            ..Settings::default()
        };
        let keys = edited.take_keys();
        use crate::secrets::Secret;
        assert!(keys.contains(&(Secret::Anthropic, "sk-ant-new".into())));
        // SAVED stays as it is; an empty field asks to remove its key.
        assert!(!keys.iter().any(|(s, _)| *s == Secret::IncidentIo));
        assert!(keys.contains(&(Secret::Github, String::new())));
        assert_eq!(edited.anthropic_api_key, SAVED);
        let json = serde_json::to_string(&edited).unwrap();
        assert!(!json.contains("sk-ant-new"));
    }

    #[test]
    fn a_watch_sees_the_first_look_and_each_change() {
        let path = std::env::temp_dir().join(format!("orbit-watch-{}", std::process::id()));
        let mut watch = Watch::default();
        let now = std::time::Duration::ZERO;
        assert!(watch.changed(&path, now), "the first look, even of no file");
        assert!(!watch.changed(&path, now));
        std::fs::write(&path, "a").unwrap();
        assert!(watch.changed(&path, now));
        assert!(
            !watch.changed(&path, std::time::Duration::from_secs(60)),
            "not yet"
        );
        watch.reset();
        assert!(watch.changed(&path, now));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn saving_writes_only_what_differs_so_the_profile_keeps_flowing() {
        let profile = serde_json::json!({
            "testAppUrl": "https://staging.example.com",
            "debug": [{"name": "prod", "setup": "login prod", "panes": ["k9s"]}],
        });
        // Read with the profile under it, one thing changed, saved.
        let mut s = over_profile(serde_json::json!({"teams": ["OPS"]}), &profile);
        s.theme = crate::theme::Choice::Paper;
        let own = own_part(&s, &profile);
        assert_eq!(own, serde_json::json!({"teams": ["OPS"], "theme": "paper"}));
        // A value the user set to something else is kept, even if empty.
        s.debug.clear();
        assert_eq!(own_part(&s, &profile)["debug"], serde_json::json!([]));
    }

    #[test]
    fn the_file_wins_over_the_profile_which_fills_the_rest() {
        let profile = serde_json::json!({
            "testAppUrl": "https://staging.example.com",
            "debug": [{"name": "prod", "setup": "login prod", "panes": ["k9s"]}],
            "anthropicApiKey": "never-from-a-profile",
        });
        let file = serde_json::json!({ "testAppUrl": "http://localhost:3000", "teams": ["OPS"] });
        let s = over_profile(file, &profile);
        assert_eq!(s.test_app_url, "http://localhost:3000");
        assert_eq!(
            (s.teams.as_slice(), s.debug[0].setup.as_str()),
            (&["OPS".to_owned()][..], "login prod")
        );
        assert_eq!(s.anthropic_api_key, "");
        assert_eq!(
            over_profile(serde_json::json!({}), &serde_json::Value::Null),
            Settings::default()
        );
    }

    #[test]
    fn a_rename_moves_the_folder_and_leaves_a_link_behind() {
        let root = std::env::temp_dir().join(format!("orbit-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (new, old, older) = (root.join("Orbit"), root.join("Old"), root.join("Older"));
        std::fs::create_dir_all(old.join("work/DEV-1")).unwrap();
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(old.join("sessions.json"), "{}").unwrap();
        let previous = [root.join("Missing"), old.clone(), older.clone()];
        assert_eq!(moved(new.clone(), previous.clone()), new);
        assert!(new.join("sessions.json").is_file());
        // A session's saved path into the old folder still resolves.
        assert!(old.is_symlink() && old.join("work/DEV-1").is_dir());
        assert!(
            older.is_dir() && !older.is_symlink(),
            "only the newest moves"
        );
        // Next start: nothing moves again.
        assert_eq!(moved(new.clone(), previous), new);
        assert!(older.is_dir() && !older.is_symlink());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn hash_is_pinned_and_round_trips_as_hex() {
        // Pinned: every build must agree, or teammates' notes go stale.
        assert_eq!(text_hash(""), 0xcbf2_9ce4_8422_2325);
        let stored = format!("{:x}", text_hash("renaming a report"));
        assert_eq!(
            u64::from_str_radix(&stored, 16).unwrap(),
            text_hash("renaming a report")
        );
    }
}
