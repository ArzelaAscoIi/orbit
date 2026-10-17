//! Running the user's own tools (`claude`, `gh`) with the user's own
//! environment, whichever way the app was launched.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};

/// Markers a parent Claude Code session sets. Inherited, they make a child a
/// sub-session that saves no transcript and cannot be resumed. The user's
/// own CLAUDE_* settings are left alone. The app's own keys (Linear,
/// Anthropic for its helpers, incident.io; from `.env` with `make run`) are
/// not a session's to see either: an Anthropic key would also switch Claude
/// Code to API billing. `GITHUB_TOKEN` stays: Orbit never sets it, a
/// session's gh may rely on the user's own.
pub(super) const SCRUB: &[&str] = &[
    "LINEAR_API_KEY",
    "ANTHROPIC_API_KEY",
    "INCIDENT_IO_API_KEY",
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_DISABLE_TERMINAL_TITLE",
    "CLAUDE_CODE_SSE_PORT",
];

pub(super) fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

/// The PATH a terminal would have. An app opened from Finder gets only the
/// system default, and a login shell alone may not add `~/.local/bin` (zsh
/// reads `.zshrc` only when interactive), so ask an interactive login shell
/// once. The markers keep rc-file chatter out of the answer.
pub(super) fn user_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let fallback = std::env::var("PATH").unwrap_or_default();
        let output = Command::new(shell())
            .args(["-i", "-l", "-c", "printf '<<PATH>>%s<<END>>' \"$PATH\""])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        output
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .and_then(|out| {
                let start = out.rfind("<<PATH>>")? + "<<PATH>>".len();
                let end = start + out[start..].find("<<END>>")?;
                Some(out[start..end].to_owned())
            })
            .filter(|p| !p.is_empty())
            .unwrap_or(fallback)
    })
}

/// The user's environment for anything started from here: their PATH, and
/// none of the markers in `SCRUB`.
fn prepare(command: &mut Command) {
    command.env("PATH", user_path());
    for name in SCRUB {
        command.env_remove(name);
    }
}

pub(super) fn login_shell() -> Command {
    let mut command = Command::new(shell());
    prepare(&mut command);
    command
}

/// The `-c` script that execs the program passed after it with the rest of
/// the arguments. POSIX shells put the first argument in `$0` and the rest
/// in `$@`; fish has neither and puts them all in `$argv`.
fn exec_script(shell: &str) -> &'static str {
    match Path::new(shell).file_name().and_then(|n| n.to_str()) {
        Some("fish") => "exec $argv",
        _ => "exec \"$0\" \"$@\"",
    }
}

/// `program args...` run through the user's login shell, so the child
/// inherits the environment their shell sets up (not the app's).
pub(super) fn login_exec(program: &Path, args: &[String]) -> Command {
    let shell = shell();
    let mut command = login_shell();
    command.args(["-l", "-c", exec_script(&shell)]);
    command.arg(program);
    command.args(args);
    command
}

/// A program on the user's PATH (the login shell's), if it is there.
pub fn which(program: &str) -> Option<PathBuf> {
    on_path(program)
}

fn on_path(program: &str) -> Option<PathBuf> {
    std::env::split_paths(user_path())
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

/// `claude` itself, found once on the user's PATH.
pub(super) fn claude_path() -> &'static Path {
    static CLAUDE: OnceLock<PathBuf> = OnceLock::new();
    CLAUDE.get_or_init(|| on_path("claude").unwrap_or_else(|| PathBuf::from("claude")))
}

pub(super) fn claude() -> Command {
    let mut command = Command::new(claude_path());
    prepare(&mut command);
    command
}

pub(super) fn codex() -> Command {
    let mut command = Command::new(on_path("codex").unwrap_or_else(|| PathBuf::from("codex")));
    prepare(&mut command);
    command
}

/// Runs the user's `gh` and returns its output, or why it failed.
pub fn gh(args: &[&str]) -> Result<String> {
    gh_with(args, None)
}

/// `gh` with `input` on stdin, for `gh api --input -` JSON bodies.
pub fn gh_with(args: &[&str], input: Option<&str>) -> Result<String> {
    let path = on_path("gh").context("GitHub CLI (gh) is not installed")?;
    let mut command = Command::new(path);
    prepare(&mut command);
    command.args(args);
    let output = match input {
        None => command.output()?,
        Some(input) => {
            use std::io::Write;
            let mut child = command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            child
                .stdin
                .take()
                .context("gh: no stdin")?
                .write_all(input.as_bytes())?;
            child.wait_with_output()?
        }
    };
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        bail!("gh: {}", error.trim().lines().last().unwrap_or("failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Marks `cwd` trusted in Claude Code's own config, the way accepting its
/// dialog does, so a background session in a fresh ticket folder starts
/// instead of stopping at "Workspace not trusted" (the dialog cannot be
/// answered in a `--bg` session). Only this folder's entry is touched.
/// Claude Code checks the folder's real path, so behind a symlink (the
/// data dir renamed, the old name linked to it) both are trusted.
/// Trusts `cwd` for Claude Code. When the entry is new, waits a moment
/// before the caller starts Claude: a session started in the same instant
/// can read the config from before it and refuse what needs trust (/goal).
pub(super) fn trust(cwd: &Path) -> Result<()> {
    let store = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()))
        .context("no home directory")?
        .join(".claude.json");
    // Every running claude rewrites this file from its own copy, so an entry
    // written while one saves can be lost (a ticket session once started untrusted): after
    // the pause it is read back and written again until it holds.
    for _ in 0..3 {
        if trust_all(&store, cwd)? {
            // ponytail: a fixed pause; a session started at once reads the old config.
            std::thread::sleep(std::time::Duration::from_millis(1500));
        } else {
            return Ok(());
        }
    }
    Ok(())
}

/// Whether an entry was written (false: both were trusted already).
fn trust_all(store: &Path, cwd: &Path) -> Result<bool> {
    let wrote = trust_in(store, cwd)?;
    match cwd.canonicalize() {
        Ok(real) if real != cwd => Ok(trust_in(store, &real)? || wrote),
        _ => Ok(wrote),
    }
}

fn trust_in(store: &Path, cwd: &Path) -> Result<bool> {
    let mut root: serde_json::Value = match std::fs::read_to_string(store) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("{} is not JSON; not touching it", store.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", store.display())),
    };
    let projects = root
        .as_object_mut()
        .context("config root is not an object")?
        .entry("projects")
        .or_insert_with(|| serde_json::json!({}));
    let entry = projects
        .as_object_mut()
        .context("projects is not an object")?
        .entry(cwd.to_string_lossy().into_owned())
        .or_insert_with(|| serde_json::json!({}));
    if entry["hasTrustDialogAccepted"] == true {
        return Ok(false);
    }
    entry["hasTrustDialogAccepted"] = serde_json::json!(true);
    // Written whole and renamed into place, keeping the file's mode: it
    // holds more than trust flags.
    let tmp = store.with_extension("json.orbit");
    std::fs::write(&tmp, serde_json::to_string_pretty(&root)?)?;
    if let Ok(meta) = std::fs::metadata(store) {
        std::fs::set_permissions(&tmp, meta.permissions())?;
    }
    std::fs::rename(&tmp, store).with_context(|| format!("cannot update {}", store.display()))?;
    Ok(true)
}

#[cfg(test)]
mod shell_tests {
    use super::*;

    #[test]
    fn a_session_never_sees_the_apps_own_keys() {
        for key in ["LINEAR_API_KEY", "ANTHROPIC_API_KEY", "INCIDENT_IO_API_KEY"] {
            assert!(SCRUB.contains(&key), "{key} reaches sessions");
        }
        assert!(
            !SCRUB.contains(&"GITHUB_TOKEN"),
            "the user's own gh token stays"
        );
    }

    #[test]
    fn fish_gets_its_own_exec_script() {
        assert_eq!(exec_script("/opt/homebrew/bin/fish"), "exec $argv");
        assert_eq!(exec_script("/bin/zsh"), "exec \"$0\" \"$@\"");
        assert_eq!(exec_script("/bin/bash"), "exec \"$0\" \"$@\"");
        // An unknown shell keeps the POSIX form rather than failing outright.
        assert_eq!(exec_script("/usr/local/bin/ksh"), "exec \"$0\" \"$@\"");
    }
}

#[cfg(test)]
mod trust_tests {
    use super::*;

    #[test]
    fn trust_adds_one_entry_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("orbit-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = dir.join(".claude.json");
        std::fs::write(
            &store,
            r#"{"theme":"dark","projects":{"/a":{"hasTrustDialogAccepted":true,"x":1}}}"#,
        )
        .unwrap();
        assert!(
            trust_in(&store, Path::new("/work/DEV-1")).unwrap(),
            "new: written"
        );
        assert!(
            !trust_in(&store, Path::new("/work/DEV-1")).unwrap(),
            "trusted already: nothing to wait for"
        );
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&store).unwrap()).unwrap();
        assert_eq!(root["theme"], "dark");
        assert_eq!(root["projects"]["/a"]["x"], 1);
        assert_eq!(
            root["projects"]["/work/DEV-1"]["hasTrustDialogAccepted"],
            true
        );
        // Behind a symlink, the real folder is trusted too.
        let real = dir.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        trust_all(&store, &link).unwrap();
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&store).unwrap()).unwrap();
        let real = real.canonicalize().unwrap();
        for path in [link.to_string_lossy(), real.to_string_lossy()] {
            assert_eq!(
                root["projects"][&*path]["hasTrustDialogAccepted"], true,
                "{path}"
            );
        }
        // A store that is not JSON is left alone.
        std::fs::write(&store, "{oops").unwrap();
        assert!(trust_in(&store, Path::new("/work/DEV-2")).is_err());
        assert_eq!(std::fs::read_to_string(&store).unwrap(), "{oops");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
