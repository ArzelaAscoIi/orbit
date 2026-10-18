//! pi (pi.dev) as a session's agent: the command line that starts or
//! resumes it, the environment its Orbit extension reads, and the extension
//! itself (`pi-orbit.ts`), written into the data folder when it changed.
//!
//! A pi session is an interactive `pi` in a terminal Orbit owns (mod.rs
//! `open_pi`). Its id is Orbit's: `--session-id` creates the session the
//! first time and resumes it after, so a restart of the app (which ends the
//! process) loses nothing but the running turn.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;

use super::{Agent, Saved, Spec, status};

/// The extension, embedded; its header carries the version.
const EXTENSION: &str = include_str!("pi-orbit.ts");

/// Where the extension is written for pi to load (`-e`).
fn extension_path(data: &Path) -> PathBuf {
    data.join("pi").join("orbit.ts")
}

/// Writes the extension when it is missing or differs from this build's.
fn write_extension(data: &Path) -> Result<PathBuf> {
    let path = extension_path(data);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(EXTENSION) {
        std::fs::create_dir_all(path.parent().unwrap_or(data))?;
        std::fs::write(&path, EXTENSION)
            .with_context(|| format!("cannot write {}", path.display()))?;
    }
    Ok(path)
}

/// The session to save and `pi`'s arguments: a new id the first time, the
/// saved one after; the first message only on the first start.
pub(super) fn launch(
    spec: &Spec,
    saved: Option<&Saved>,
    work: &Path,
    data: &Path,
) -> Result<(Saved, Vec<String>)> {
    let cwd = saved.map_or_else(|| work.join(&spec.folder), |s| s.cwd.clone());
    std::fs::create_dir_all(&cwd).with_context(|| format!("cannot create {}", cwd.display()))?;
    let mut skills = Vec::new();
    for (folder, body) in &spec.skills {
        let dir = cwd.join(".claude/skills").join(folder);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("SKILL.md"), body)?;
        skills.push(dir);
    }
    let extension = write_extension(data)?;
    let resume = saved.is_some();
    let saved = Saved {
        agent: Agent::Pi,
        identifier: spec.identifier.clone(),
        title: spec.title.clone(),
        session_id: saved.map_or_else(
            || uuid::Uuid::new_v4().to_string(),
            |s| s.session_id.clone(),
        ),
        cwd,
        started_at: saved.map_or_else(
            || jiff::Timestamp::now().to_string(),
            |s| s.started_at.clone(),
        ),
        deny: Vec::new(),
    };
    let args = args(spec, &saved.session_id, &extension, &skills, resume);
    Ok((saved, args))
}

/// `pi`'s arguments for this session.
fn args(spec: &Spec, id: &str, extension: &Path, skills: &[PathBuf], resume: bool) -> Vec<String> {
    let context = match status::progress_hint(&spec.key) {
        Some(hint) => format!("{}\n\n{hint}", spec.context),
        None => spec.context.clone(),
    };
    let mut args = vec![
        "--session-id".to_owned(),
        id.to_owned(),
        "--name".into(),
        format!("{} {}", spec.identifier, spec.title),
        "--append-system-prompt".into(),
        context,
        "-e".into(),
        extension.to_string_lossy().into_owned(),
    ];
    for dir in skills {
        args.extend(["--skill".into(), dir.to_string_lossy().into_owned()]);
    }
    if !resume && let Some(prompt) = &spec.prompt {
        args.push(prompt.clone());
    }
    args
}

/// What the extension reads: where to report, Orbit itself, and the
/// session's rules in Claude Code's form (`Bash(git push *)`).
pub(super) fn env(spec: &Spec) -> Result<Vec<(String, String)>> {
    let exe = std::env::current_exe().context("no path to this binary")?;
    Ok(vec![
        ("ORBIT_SESSION_KEY".into(), spec.key.clone()),
        (
            "ORBIT_STATUS_LOG".into(),
            status::path(&spec.key).to_string_lossy().into_owned(),
        ),
        ("ORBIT_BIN".into(), exe.to_string_lossy().into_owned()),
        (
            "ORBIT_PI_RULES".into(),
            json!({"allow": spec.allow, "deny": spec.deny}).to_string(),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(prompt: Option<&str>) -> Spec {
        Spec {
            key: "DEV-1".into(),
            identifier: "DEV-1".into(),
            title: "Fix export".into(),
            folder: "DEV-1".into(),
            context: "brief".into(),
            prompt: prompt.map(str::to_owned),
            skills: vec![("build".into(), "# build".into())],
            allow: vec!["Bash(git commit *)".into()],
            deny: vec!["Bash(gh pr merge*)".into()],
        }
    }

    #[test]
    fn a_pi_session_starts_with_its_id_brief_extension_and_skills_and_resumes_without_the_prompt() {
        let tmp = std::env::temp_dir().join(format!("orbit-pi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let (work, data) = (tmp.join("work"), tmp.join("data"));
        let (saved, args) = launch(&spec(Some("/build DEV-1")), None, &work, &data).unwrap();
        assert_eq!(saved.agent, Agent::Pi);
        assert!(uuid::Uuid::parse_str(&saved.session_id).is_ok());
        assert_eq!(saved.cwd, work.join("DEV-1"));
        let at = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .map(|i| args[i + 1].clone())
        };
        assert_eq!(
            at("--session-id").as_deref(),
            Some(saved.session_id.as_str())
        );
        assert!(at("--append-system-prompt").unwrap().starts_with("brief"));
        assert_eq!(
            at("-e"),
            Some(data.join("pi/orbit.ts").to_string_lossy().into_owned())
        );
        assert!(at("--skill").unwrap().ends_with(".claude/skills/build"));
        assert!(work.join("DEV-1/.claude/skills/build/SKILL.md").is_file());
        assert_eq!(args.last().map(String::as_str), Some("/build DEV-1"));
        // Opened again: the same session, no first message a second time.
        let (again, args) =
            launch(&spec(Some("/build DEV-1")), Some(&saved), &work, &data).unwrap();
        assert_eq!(again.session_id, saved.session_id);
        assert!(!args.contains(&"/build DEV-1".to_owned()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_extension_is_written_with_its_version_and_kept_current() {
        let data = std::env::temp_dir().join(format!("orbit-pi-ext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let path = write_extension(&data).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("ORBIT_PI_VERSION=")
        );
        std::fs::write(&path, "old").unwrap();
        write_extension(&data).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXTENSION);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn the_extension_gets_the_sessions_rules_and_where_to_report() {
        let env: std::collections::HashMap<_, _> = env(&spec(None)).unwrap().into_iter().collect();
        let rules: serde_json::Value = serde_json::from_str(&env["ORBIT_PI_RULES"]).unwrap();
        assert_eq!(rules["allow"][0], "Bash(git commit *)");
        assert_eq!(rules["deny"][0], "Bash(gh pr merge*)");
        assert!(env["ORBIT_STATUS_LOG"].ends_with("status/DEV-1.log"));
        assert!(!env["ORBIT_BIN"].is_empty());
    }
}
