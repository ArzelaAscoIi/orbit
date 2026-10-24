//! herdr, next to Orbit instead of instead of it: many people run their
//! agents in herdr (a terminal multiplexer for coding agents) and do not
//! want to switch. Orbit meets them there:
//!
//! - `read()`: herdr's agents and workspaces from its CLI (`herdr agent
//!   list`, `herdr workspace list`), shown in Orbit's sidebar with herdr's
//!   own states; `focus()` brings one forward in herdr and its terminal to
//!   the front; `start()` opens a ticket's Claude session as a herdr agent.
//! - `Config`: the user's `~/.config/herdr/config.toml` (keys, theme,
//!   notifications, sounds, sidebar), so Orbit can run with what they set up.
//! - `PLUGIN`: a herdr plugin that shows Orbit's "needs you" inside herdr
//!   and sends Linear and GitHub links clicked there to Orbit.
//!
//! Every call goes through the `herdr` on the user's PATH; without it (or
//! without a running herdr), Orbit shows nothing of herdr and says so only
//! in Settings.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// One agent herdr recognised in one of its panes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Agent {
    /// Its live name (`reviewer`), when it has one.
    pub name: Option<String>,
    pub pane_id: String,
    /// The kind herdr detected: `claude`, `codex`, ...
    pub kind: String,
    /// herdr's state: `idle`, `working`, `blocked`, `done`, `unknown`.
    pub status: String,
    pub workspace: String,
    pub cwd: String,
    pub title: String,
}

impl Agent {
    /// How herdr addresses it: its name, else its pane.
    pub fn target(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.pane_id)
    }

    /// herdr's state in Orbit's words, so it reads like Orbit's sessions:
    /// blocked is a prompt that waits, done a turn not looked at yet.
    pub fn label(&self) -> &'static str {
        match self.status.as_str() {
            "blocked" => "needs approval",
            "done" => "your turn",
            "working" => "working",
            "idle" => "idle",
            _ => "unknown",
        }
    }
}

/// The herdr binary on the user's PATH.
pub fn bin() -> Option<PathBuf> {
    crate::claude::which("herdr")
}

/// Runs `herdr <args>` and returns its JSON reply's `result`.
fn run(args: &[&str]) -> Result<Value> {
    let bin = bin().context("herdr is not installed")?;
    let out = std::process::Command::new(bin)
        .args(args)
        .output()
        .context("could not run herdr")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("herdr: {}", err.trim().lines().last().unwrap_or("failed"));
    }
    let reply: Value = serde_json::from_slice(&out.stdout).context("herdr answered no JSON")?;
    Ok(reply["result"].clone())
}

/// herdr's agents, with their workspaces' labels. Blocking.
pub fn read() -> Result<Vec<Agent>> {
    let agents = run(&["agent", "list"])?;
    let workspaces = run(&["workspace", "list"]).unwrap_or_default();
    Ok(agents_from(&agents, &workspaces))
}

/// The agents in an `agent list` result, their workspace ids replaced by
/// the labels in a `workspace list` result.
pub fn agents_from(agents: &Value, workspaces: &Value) -> Vec<Agent> {
    let labels: BTreeMap<&str, &str> = workspaces["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| Some((w["workspace_id"].as_str()?, w["label"].as_str()?)))
        .collect();
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    agents["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let workspace = a["workspace_id"].as_str().unwrap_or_default();
            Some(Agent {
                name: a["name"].as_str().map(str::to_owned),
                pane_id: a["pane_id"].as_str()?.to_owned(),
                kind: a["display_agent"]
                    .as_str()
                    .or_else(|| a["agent"].as_str())
                    .unwrap_or("agent")
                    .to_owned(),
                status: text(&a["agent_status"]),
                workspace: labels.get(workspace).map_or(workspace, |l| l).to_owned(),
                cwd: text(&a["cwd"]),
                title: text(&a["terminal_title_stripped"]),
            })
        })
        .collect()
}

/// Brings the agent forward in herdr, and the terminal herdr runs in to
/// the front. Blocking.
pub fn focus(target: &str) -> Result<()> {
    run(&["agent", "focus", target])?;
    crate::claude::show_app_of("herdr");
    Ok(())
}

/// Opens a ticket's Claude session as a herdr agent: a workspace of its own
/// in the ticket's folder, named after the ticket, with Orbit's brief and
/// hooks (`args`), so herdr shows it and Orbit still reads its status log.
/// Returns the agent's name. Blocking.
pub fn start(name: &str, label: &str, cwd: &std::path::Path, args: &[String]) -> Result<String> {
    let cwd = cwd.to_string_lossy();
    let created = run(&[
        "workspace",
        "create",
        "--cwd",
        &cwd,
        "--label",
        label,
        "--no-focus",
    ])?;
    let pane = created["root_pane"]["pane_id"]
        .as_str()
        .context("herdr made no pane")?;
    let name = agent_name(name);
    let mut argv: Vec<&str> = vec!["agent", "start", &name, "--kind", "claude", "--pane", pane];
    if !args.is_empty() {
        argv.push("--");
        argv.extend(args.iter().map(String::as_str));
    }
    run(&argv)?;
    Ok(name)
}

/// A herdr agent name for a ticket: `[a-z][a-z0-9_-]{0,31}`.
pub fn agent_name(identifier: &str) -> String {
    let mut name: String = identifier
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(32)
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        name = format!("t{}", name.chars().take(31).collect::<String>());
    }
    name
}

// ---- the user's herdr config

/// What Orbit takes from `~/.config/herdr/config.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub path: PathBuf,
    pub keys: Keys,
    /// `theme.name` (herdr's default: catppuccin).
    pub theme: String,
    /// `ui.toast.delivery`: off, herdr, terminal or system.
    pub toasts: String,
    pub sound: bool,
    pub done_sound: Option<PathBuf>,
    pub request_sound: Option<PathBuf>,
    /// `ui.agent_panel_sort = "priority"`: what needs the user first.
    pub needs_first: bool,
    pub start_collapsed: bool,
}

/// Where herdr reads its config: `HERDR_CONFIG_PATH`, else
/// `$XDG_CONFIG_HOME/herdr/config.toml`, else `~/.config/herdr/config.toml`.
pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HERDR_CONFIG_PATH") {
        return Some(path.into());
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::store::home().map(|h| h.join(".config")))?;
    Some(base.join("herdr").join("config.toml"))
}

/// The user's herdr config, if they have one.
pub fn config() -> Option<Config> {
    let path = config_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    parse(&text, path).ok()
}

pub fn parse(text: &str, path: PathBuf) -> Result<Config> {
    let doc: toml::Value = toml::from_str(text).context("herdr's config is not TOML")?;
    let get = |dotted: &str| -> Option<&toml::Value> {
        dotted.split('.').try_fold(&doc, |v, k| v.get(k))
    };
    let string = |dotted: &str, default: &str| {
        get(dotted)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_owned()
    };
    let flag =
        |dotted: &str, default: bool| get(dotted).and_then(|v| v.as_bool()).unwrap_or(default);
    let dir = path.parent().map(PathBuf::from).unwrap_or_default();
    let sound = |dotted: &str| {
        get(dotted)
            .or_else(|| get("ui.sound.path"))
            .and_then(|v| v.as_str())
            .map(|p| expand(p, &dir))
    };
    Ok(Config {
        keys: Keys::from(get("keys")),
        theme: string("theme.name", "catppuccin"),
        toasts: string("ui.toast.delivery", "off"),
        sound: flag("ui.sound.enabled", true),
        done_sound: sound("ui.sound.done_path"),
        request_sound: sound("ui.sound.request_path"),
        needs_first: string("ui.agent_panel_sort", "spaces") == "priority",
        start_collapsed: flag("ui.sidebar_start_collapsed", false),
        path,
    })
}

/// A path from herdr's config: `~` expanded, relative ones from its folder.
fn expand(path: &str, dir: &std::path::Path) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = crate::store::home()
    {
        return home.join(rest);
    }
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        dir.join(path)
    }
}

/// The closest of Orbit's themes to herdr's: its light ones to paper, its
/// dark ones to slate (Ghostty's dark grey), `terminal` to the system's.
pub fn theme(name: &str) -> crate::theme::Choice {
    use crate::theme::Choice;
    let name = name.to_lowercase();
    if name == "terminal" {
        Choice::System
    } else if ["latte", "light", "day", "dawn", "paper"]
        .iter()
        .any(|l| name.contains(l))
    {
        Choice::Paper
    } else {
        Choice::Slate
    }
}

// ---- keys

/// One herdr binding: after the prefix, or a direct chord.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub prefix: bool,
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

/// herdr's keymap: the prefix and each action's chords, herdr's defaults
/// with the user's `[keys]` on top.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Keys {
    pub prefix: Vec<Chord>,
    pub actions: BTreeMap<String, Vec<Chord>>,
}

/// herdr's default bindings for the actions Orbit has a counterpart for.
const DEFAULTS: &[(&str, &str)] = &[
    ("help", "prefix+?"),
    ("settings", "prefix+s"),
    ("new_workspace", "prefix+shift+n"),
    ("workspace_picker", "prefix+w"),
    ("goto", "prefix+g"),
    ("open_notification_target", "prefix+o"),
    ("new_tab", "prefix+c"),
    ("rename_tab", "prefix+shift+t"),
    ("previous_tab", "prefix+p"),
    ("next_tab", "prefix+n"),
    ("switch_tab", "prefix+1..9"),
    ("close_tab", "prefix+shift+x"),
    ("focus_pane_left", "prefix+h"),
    ("focus_pane_down", "prefix+j"),
    ("focus_pane_up", "prefix+k"),
    ("focus_pane_right", "prefix+l"),
    ("cycle_pane_next", "prefix+tab"),
    ("cycle_pane_previous", "prefix+shift+tab"),
    ("split_vertical", "prefix+v"),
    ("split_horizontal", "prefix+minus"),
    ("close_pane", "prefix+x"),
    ("zoom", "prefix+z"),
    ("toggle_sidebar", "prefix+b"),
];

impl Keys {
    fn from(table: Option<&toml::Value>) -> Self {
        let bindings = |v: &toml::Value| -> Vec<Chord> {
            match v {
                toml::Value::String(s) => chord(s).into_iter().collect(),
                toml::Value::Array(a) => a.iter().filter_map(|v| chord(v.as_str()?)).collect(),
                _ => Vec::new(),
            }
        };
        let prefix = table
            .and_then(|t| t.get("prefix"))
            .map(bindings)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| chord("ctrl+b").into_iter().collect());
        let mut actions: BTreeMap<String, Vec<Chord>> = DEFAULTS
            .iter()
            .map(|(a, k)| (a.to_string(), chord(k).into_iter().collect()))
            .collect();
        if let Some(toml::Value::Table(t)) = table {
            for (action, value) in t {
                if action != "prefix" && DEFAULTS.iter().any(|(a, _)| a == action) {
                    actions.insert(action.clone(), bindings(value));
                }
            }
        }
        Keys { prefix, actions }
    }

    /// The action a key pressed after the prefix (`prefix`) or alone
    /// starts, with the window number for `switch_tab` (`1..9`).
    pub fn action(&self, pressed: &Chord) -> Option<(&str, Option<usize>)> {
        for (action, chords) in &self.actions {
            for c in chords {
                if c.prefix != pressed.prefix
                    || (c.ctrl, c.alt, c.shift, c.cmd)
                        != (pressed.ctrl, pressed.alt, pressed.shift, pressed.cmd)
                {
                    continue;
                }
                if c.key == "1..9" {
                    if let Some(n) = pressed
                        .key
                        .parse::<usize>()
                        .ok()
                        .filter(|n| (1..=9).contains(n))
                    {
                        return Some((action, Some(n - 1)));
                    }
                } else if c.key == pressed.key {
                    return Some((action, None));
                }
            }
        }
        None
    }
}

/// One binding string (`prefix+shift+t`, `ctrl+alt+h`, `prefix+minus`).
pub fn chord(text: &str) -> Option<Chord> {
    let mut chord = Chord {
        prefix: false,
        key: String::new(),
        ctrl: false,
        alt: false,
        shift: false,
        cmd: false,
    };
    for part in text.trim().to_lowercase().split('+') {
        match part {
            "prefix" => chord.prefix = true,
            "ctrl" | "control" => chord.ctrl = true,
            "alt" | "option" | "meta" => chord.alt = true,
            "shift" => chord.shift = true,
            "cmd" | "super" | "command" => chord.cmd = true,
            "" => {}
            key => chord.key = key.to_owned(),
        }
    }
    (!chord.key.is_empty()).then_some(chord)
}

/// The herdr plugin (`herdr plugin link <folder>`): its manifest and the
/// script it runs, written into Orbit's data folder by Settings.
pub const PLUGIN: &[(&str, &str)] = &[
    (
        "herdr-plugin.toml",
        include_str!("../herdr-plugin/herdr-plugin.toml"),
    ),
    ("orbit.sh", include_str!("../herdr-plugin/orbit.sh")),
];

/// Writes the plugin into `<data>/herdr-plugin` and links it into herdr.
/// Blocking.
pub fn install_plugin() -> Result<PathBuf> {
    let dir = crate::store::data_dir().join("herdr-plugin");
    std::fs::create_dir_all(&dir)?;
    for (name, body) in PLUGIN {
        let path = dir.join(name);
        std::fs::write(&path, body)?;
        if name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    let dir_s = dir.to_string_lossy().into_owned();
    run(&["plugin", "link", &dir_s])?;
    Ok(dir)
}

// ---- what the plugin asks the app

/// The file the app keeps current with what needs the user (notify.rs
/// lines), for `orbit needs` in herdr's pane.
pub fn needs_path() -> PathBuf {
    crate::store::data_dir().join("needs.txt")
}

/// The file `orbit open <url>` appends to and the app takes links from.
pub fn opens_path() -> PathBuf {
    crate::store::data_dir().join("open.txt")
}

/// `orbit open <url>`: hands a link to the running app, one per line.
pub fn queue_open(path: &std::path::Path, url: &str) -> Result<()> {
    use std::io::Write;
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!("not a link: {url}");
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{url}")?;
    Ok(())
}

/// The links waiting for the app, taken: the file is moved aside first, so
/// a link written meanwhile goes to the next take, never lost.
pub fn take_opens(path: &std::path::Path) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }
    let taken = path.with_extension("taking");
    if std::fs::rename(path, &taken).is_err() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(&taken).unwrap_or_default();
    let _ = std::fs::remove_file(&taken);
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("http"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn herdrs_agents_read_with_their_workspace_and_orbits_words() {
        // As herdr 0.9.3 answers `agent list` and `workspace list`.
        let agents = json!({"agents": [{"agent": "claude", "agent_status": "blocked",
            "cwd": "/tmp", "name": "reviewer", "pane_id": "w1:p1", "tab_id": "w1:t1",
            "terminal_title_stripped": "fix tests", "workspace_id": "w1"},
            {"agent": "codex", "agent_status": "done", "pane_id": "w2:p3", "workspace_id": "w2"}],
            "type": "agent_list"});
        let workspaces = json!({"workspaces": [{"workspace_id": "w1", "label": "api"}]});
        let read = agents_from(&agents, &workspaces);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].target(), "reviewer");
        assert_eq!(read[0].workspace, "api");
        assert_eq!(read[0].label(), "needs approval");
        assert_eq!(read[1].target(), "w2:p3", "no name: its pane");
        assert_eq!(read[1].workspace, "w2");
        assert_eq!(read[1].label(), "your turn");
    }

    #[test]
    fn a_tickets_agent_name_is_one_herdr_accepts() {
        assert_eq!(agent_name("DEV-192"), "dev-192");
        assert_eq!(agent_name("web-api#30"), "web-api-30");
        assert_eq!(agent_name("42"), "t42");
        assert!(agent_name(&"x".repeat(50)).len() <= 32);
    }

    #[test]
    fn the_users_herdr_config_brings_their_keys_theme_and_sounds() {
        let config = parse(
            r#"
            [theme]
            name = "catppuccin-latte"
            [keys]
            prefix = ["ctrl+a", "ctrl+s"]
            new_tab = "prefix+t"
            split_vertical = ["prefix+v", "ctrl+alt+d"]
            [ui]
            agent_panel_sort = "priority"
            [ui.toast]
            delivery = "system"
            [ui.sound]
            done_path = "/s/done.mp3"
            "#,
            PathBuf::from("/c/herdr/config.toml"),
        )
        .unwrap();
        assert_eq!(theme(&config.theme), crate::theme::Choice::Paper);
        assert_eq!(theme("catppuccin"), crate::theme::Choice::Slate);
        assert_eq!(config.keys.prefix.len(), 2);
        assert_eq!(config.keys.prefix[0].key, "a");
        let after = |key: &str, shift: bool| Chord {
            prefix: true,
            key: key.into(),
            ctrl: false,
            alt: false,
            shift,
            cmd: false,
        };
        assert_eq!(
            config.keys.action(&after("t", false)),
            Some(("new_tab", None))
        );
        assert_eq!(config.keys.action(&after("c", false)), None, "c moved to t");
        assert_eq!(
            config.keys.action(&after("4", false)),
            Some(("switch_tab", Some(3)))
        );
        assert_eq!(
            config.keys.action(&after("x", true)),
            Some(("close_tab", None))
        );
        let direct = chord("ctrl+alt+d").unwrap();
        assert_eq!(config.keys.action(&direct), Some(("split_vertical", None)));
        assert!(config.needs_first);
        assert_eq!(config.toasts, "system");
        assert!(config.sound);
        assert_eq!(config.done_sound, Some(PathBuf::from("/s/done.mp3")));
        assert_eq!(config.request_sound, None);
        // No config at all: herdr's defaults.
        let plain = parse("", PathBuf::from("/c/herdr/config.toml")).unwrap();
        assert_eq!(plain.keys.prefix[0].key, "b");
        assert!(plain.keys.prefix[0].ctrl);
        assert_eq!(
            plain.keys.action(&after("minus", false)),
            Some(("split_horizontal", None))
        );
    }

    #[test]
    fn links_from_herdr_reach_the_app_once_and_only_links() {
        let dir = std::env::temp_dir().join(format!("orbit-opens-{}", std::process::id()));
        let path = dir.join("open.txt");
        queue_open(&path, "https://linear.app/a/issue/A-1").unwrap();
        queue_open(&path, " https://github.com/o/r/pull/2 ").unwrap();
        assert!(queue_open(&path, "rm -rf /").is_err(), "only links go in");
        assert_eq!(
            take_opens(&path),
            [
                "https://linear.app/a/issue/A-1",
                "https://github.com/o/r/pull/2"
            ]
        );
        assert!(take_opens(&path).is_empty(), "taken once");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
