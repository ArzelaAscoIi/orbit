//! Disk cleanup: session folders under `<data>/work` and the dependency
//! caches under `<data>/repos/.deps` are made by sessions and never removed
//! by them, so they pile up (tens of GB). Once a day, quietly, this takes
//! back what can be rebuilt or is finished. Data loss is worse than disk
//! use: whenever a fact is unknown, the folder stays.
//!
//! - **Artifacts** (a): in a session folder nobody worked in for 7 days and
//!   no process has as its folder, the rebuildable folders inside its git
//!   checkouts (`node_modules`, `.venv`, `target`, `build`, `dist`, ...) go,
//!   but only those git ignores. A `node_modules` link into `.deps` loses
//!   just the link.
//! - **Whole folder** (b): not in use, and its ticket done or canceled for 3
//!   days or nothing touched for 30, and every checkout in it clean (nothing
//!   uncommitted or untracked) and pushed (every commit on a remote; a
//!   branch whose remote was deleted after merging counts only when the
//!   ticket is done), and nothing in it but checkouts and Orbit's own
//!   `.claude/`. A file of anyone's keeps the folder.
//! - **Dependency caches** (c): a `.deps/<lockfile>` no worktree links to,
//!   untouched for 7 days.
//! - **Shared clones** (d): `git worktree prune`, so removed folders leave
//!   no worktree entries; the clones themselves stay.
//!
//! Only `<data>/work` and `<data>/repos/.deps` are looked at: a folder the
//! user picked for a session lives elsewhere and is never touched. The app
//! runs a pass 2 minutes after start and then daily (settings.json
//! `noCleanup` turns it off) with the board's done tickets; `orbit cleanup`
//! runs one from a shell without them, so there only the 30-day rule
//! removes folders. Every step goes to `<data>/cleanup.log`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Rebuildable folders a checkout may hold (rule a).
const ARTIFACTS: &[&str] = &[
    "node_modules",
    ".venv",
    "venv",
    "target",
    "build",
    "dist",
    ".next",
    ".turbo",
    ".cache",
    "__pycache__",
    ".pytest_cache",
    ".gradle",
];

/// What a pass needs to know besides the files.
pub struct Facts {
    /// Folders processes work in right now (every process's working
    /// folder, every running Claude session's). `None`: unknown, so no
    /// session folder is touched.
    pub busy: Option<Vec<PathBuf>>,
    /// Ticket identifiers done or canceled, and since when (the board;
    /// empty from the shell).
    pub done: HashMap<String, SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Artifact,
    Link,
    Folder,
    Deps,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Artifact => "artifact",
            Kind::Link => "link",
            Kind::Folder => "folder",
            Kind::Deps => "deps",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub path: PathBuf,
    pub kind: Kind,
    pub bytes: u64,
}

/// What would go, by the rules above. Reads only; `apply` deletes.
pub fn plan(data: &Path, now: SystemTime, facts: &Facts) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut links = Vec::new();
    let folders: Vec<PathBuf> = dirs(&data.join("work"));
    for folder in &folders {
        let (checkouts, foreign) = contents(folder, 0);
        let found: Vec<(PathBuf, bool)> = checkouts.iter().flat_map(|c| artifacts(c, 0)).collect();
        links.extend(
            found
                .iter()
                .filter(|(_, link)| *link)
                .filter_map(|(path, _)| std::fs::read_link(path).ok()),
        );
        let Some(busy) = &facts.busy else { continue };
        if busy.iter().any(|b| b.starts_with(folder)) {
            continue;
        }
        let idle = now
            .duration_since(newest(folder, &checkouts))
            .unwrap_or_default();
        let name = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let done = facts
            .done
            .get(name)
            .is_some_and(|at| now.duration_since(*at).unwrap_or_default() >= 3 * DAY);
        if !foreign && (done || idle >= 30 * DAY) && checkouts.iter().all(|c| finished(c, done)) {
            steps.push(Step {
                bytes: size(folder),
                path: folder.clone(),
                kind: Kind::Folder,
            });
        } else if idle >= 7 * DAY {
            for (path, link) in found {
                if link {
                    steps.push(Step {
                        path,
                        kind: Kind::Link,
                        bytes: 0,
                    });
                } else if ignored(&path) {
                    steps.push(Step {
                        bytes: size(&path),
                        path,
                        kind: Kind::Artifact,
                    });
                }
            }
        }
    }
    for cache in dirs(&data.join("repos").join(".deps")) {
        let old =
            modified(&cache).is_some_and(|m| now.duration_since(m).unwrap_or_default() >= 7 * DAY);
        let linked = links.iter().any(|l| l.starts_with(&cache));
        if old && !linked {
            steps.push(Step {
                bytes: size(&cache),
                path: cache,
                kind: Kind::Deps,
            });
        }
    }
    steps
}

/// Deletes the steps (never anything outside `data`) and prunes the
/// shared clones' worktree lists; the bytes freed.
pub fn apply(data: &Path, steps: &[Step]) -> u64 {
    let mut freed = 0;
    for step in steps.iter().filter(|s| s.path.starts_with(data)) {
        // Taking an artifact out changes its parent's time: put it back,
        // so the folder does not look worked in and still ages to rule b.
        let parent = step.path.parent().map(Path::to_path_buf);
        let parent_time = parent.as_deref().and_then(modified);
        let gone = match step.kind {
            Kind::Link => std::fs::remove_file(&step.path),
            _ => std::fs::remove_dir_all(&step.path),
        };
        if gone.is_ok() {
            freed += step.bytes;
        }
        if let (Some(parent), Some(time)) = (parent, parent_time)
            && step.kind != Kind::Folder
        {
            let _ = std::fs::File::open(&parent)
                .and_then(|f| f.set_times(std::fs::FileTimes::new().set_modified(time)));
        }
    }
    for owner in dirs(&data.join("repos")) {
        for clone in dirs(&owner) {
            if clone.join(".git").exists() {
                let _ = git(&clone, &["worktree", "prune"]);
            }
        }
    }
    freed
}

/// One pass: plan, apply (unless `dry`), log. The steps and bytes freed.
pub fn run(data: &Path, facts: &Facts, dry: bool) -> (Vec<Step>, u64) {
    let steps = plan(data, SystemTime::now(), facts);
    if dry {
        return (steps, 0);
    }
    let freed = apply(data, &steps);
    let now = jiff::Timestamp::now();
    let mut log: String = steps
        .iter()
        .map(|s| format!("{now} {} {} {}\n", s.kind.word(), s.bytes, s.path.display()))
        .collect();
    log.push_str(&format!(
        "{now} summary freed={freed} storage={}\n",
        size(data)
    ));
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("cleanup.log"))
    {
        let _ = file.write_all(log.as_bytes());
    }
    (steps, freed)
}

/// Every working folder on the machine: each process's (`lsof`) and each
/// running Claude session's. `None` when `lsof` cannot say.
pub fn busy() -> Option<Vec<PathBuf>> {
    let out = Command::new("lsof")
        .args(["-a", "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    if out.stdout.is_empty() {
        return None;
    }
    let mut folders: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .map(PathBuf::from)
        .collect();
    folders.extend(crate::claude::running_cwds());
    Some(folders)
}

/// The last pass's line for Settings: "storage: 12.3 GB · freed 41.0 GB on
/// 18 Oct", or `None` before the first pass.
pub fn note(data: &Path) -> Option<String> {
    let log = std::fs::read_to_string(data.join("cleanup.log")).ok()?;
    let line = log.lines().rev().find(|l| l.contains(" summary "))?;
    let at: jiff::Timestamp = line.split(' ').next()?.parse().ok()?;
    let field = |k: &str| {
        line.split(' ')
            .find_map(|w| w.strip_prefix(k))
            .and_then(|v| v.parse::<u64>().ok())
    };
    let date = at.to_zoned(jiff::tz::TimeZone::system()).strftime("%-d %b");
    Some(format!(
        "storage: {} · freed {} on {date}",
        gb(field("storage=")?),
        gb(field("freed=")?)
    ))
}

pub fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

fn dirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// The git checkouts in `dir` (3 levels down at most), and whether
/// anything else is there: a file, a link, a folder too deep. Orbit's
/// own `.claude/` (and `.orbit*`, `.DS_Store`) do not count.
fn contents(dir: &Path, depth: usize) -> (Vec<PathBuf>, bool) {
    let mut checkouts = Vec::new();
    let mut foreign = false;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let real_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if depth == 0 && (name == ".claude" || name.starts_with(".orbit")) || name == ".DS_Store" {
            continue;
        }
        if real_dir && path.join(".git").exists() {
            checkouts.push(path);
        } else if real_dir && depth < 3 {
            let (more, other) = contents(&path, depth + 1);
            checkouts.extend(more);
            foreign |= other;
        } else {
            foreign = true;
        }
    }
    (checkouts, foreign)
}

/// The artifact folders in a checkout (3 levels down at most), each with
/// whether it is a link.
fn artifacts(dir: &Path, depth: usize) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if name == ".git" {
            continue;
        }
        if ARTIFACTS.contains(&name.as_str()) && (kind.is_dir() || kind.is_symlink()) {
            out.push((entry.path(), kind.is_symlink()));
        } else if kind.is_dir() && depth < 3 {
            out.extend(artifacts(&entry.path(), depth + 1));
        }
    }
    out
}

/// When anything in the folder last changed: files and folders a few
/// levels down (not inside artifacts or `.git`), and each checkout's git
/// index and HEAD (commits, checkouts, staging). Read without writing:
/// `git status` would refresh the index and make the folder look new.
fn newest(folder: &Path, checkouts: &[PathBuf]) -> SystemTime {
    fn walk(dir: &Path, depth: usize, newest: &mut SystemTime) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || ARTIFACTS.contains(&name.as_str()) {
                continue;
            }
            if let Some(m) = entry.metadata().ok().and_then(|m| m.modified().ok()) {
                *newest = (*newest).max(m);
            }
            if depth < 4 && entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&entry.path(), depth + 1, newest);
            }
        }
    }
    let mut newest = modified(folder).unwrap_or(SystemTime::UNIX_EPOCH);
    walk(folder, 0, &mut newest);
    for checkout in checkouts {
        let git_dir = git_dir(checkout);
        for file in ["index", "HEAD"] {
            if let Some(m) = modified(&git_dir.join(file)) {
                newest = newest.max(m);
            }
        }
    }
    newest
}

/// A checkout's git folder: `.git` itself, or where a worktree's `.git`
/// file points.
fn git_dir(checkout: &Path) -> PathBuf {
    let dot = checkout.join(".git");
    std::fs::read_to_string(&dot)
        .ok()
        .and_then(|t| t.trim().strip_prefix("gitdir: ").map(PathBuf::from))
        .map(|p| if p.is_absolute() { p } else { checkout.join(p) })
        .unwrap_or(dot)
}

/// Nothing in the checkout is lost with it: no uncommitted or untracked
/// change, and every commit on a remote. A branch whose remote branch was
/// deleted (merged and cleaned up on GitHub) counts as pushed only when
/// the ticket is done.
fn finished(checkout: &Path, done: bool) -> bool {
    let clean = git(checkout, &["--no-optional-locks", "status", "--porcelain"])
        .is_ok_and(|s| s.trim().is_empty());
    if !clean {
        return false;
    }
    let local = git(
        checkout,
        &["rev-list", "--count", "HEAD", "--not", "--remotes"],
    )
    .is_ok_and(|n| n.trim() == "0");
    let gone = || {
        git(checkout, &["symbolic-ref", "-q", "HEAD"])
            .and_then(|branch| {
                git(
                    checkout,
                    &["for-each-ref", "--format=%(upstream:track)", branch.trim()],
                )
            })
            .is_ok_and(|track| track.trim() == "[gone]")
    };
    local || (done && gone())
}

fn ignored(path: &Path) -> bool {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["check-ignore", "-q", "--"])
        .arg(name)
        .status()
        .is_ok_and(|s| s.success())
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    crate::claude::worktree::git(dir, args)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .ok()
}

/// Bytes on disk, by `du` (fast on big trees, and counts what is really
/// used).
fn size(path: &Path) -> u64 {
    Command::new("du")
        .arg("-sk")
        .arg(path)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()
                .and_then(|k| k.parse::<u64>().ok())
        })
        .map_or(0, |k| k * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            ok.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    /// A data folder with one remote and a clone of it at `work/<name>/app`,
    /// pushed, with `node_modules` ignored and installed.
    fn setup(test: &str, name: &str) -> (PathBuf, PathBuf) {
        let data =
            std::env::temp_dir().join(format!("orbit-cleanup-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let remote = data.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        sh(&remote, &["init", "-q", "--bare"]);
        let app = data.join("work").join(name).join("app");
        std::fs::create_dir_all(app.parent().unwrap()).unwrap();
        sh(
            &data,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                app.to_str().unwrap(),
            ],
        );
        std::fs::write(app.join(".gitignore"), "node_modules\n").unwrap();
        std::fs::write(app.join("main.rs"), "fn main() {}\n").unwrap();
        sh(&app, &["add", "."]);
        sh(&app, &["commit", "-qm", "first"]);
        sh(&app, &["push", "-q", "origin", "HEAD:main"]);
        sh(&app, &["branch", "-q", "--set-upstream-to=origin/main"]);
        std::fs::create_dir_all(app.join("node_modules/left-pad")).unwrap();
        std::fs::write(app.join("node_modules/left-pad/index.js"), "x").unwrap();
        (data, app)
    }

    /// Everything under `dir` (git's own files included) last touched
    /// `days` ago.
    fn age(dir: &Path, days: u32) {
        let then = SystemTime::now() - days * DAY;
        fn walk(p: &Path, t: SystemTime) {
            if let Ok(m) = std::fs::symlink_metadata(p) {
                if m.is_dir() {
                    for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                        walk(&e.path(), t);
                    }
                }
                if !m.is_symlink() {
                    let _ = std::fs::File::open(p)
                        .and_then(|f| f.set_times(std::fs::FileTimes::new().set_modified(t)));
                }
            }
        }
        walk(dir, then);
    }

    fn facts() -> Facts {
        Facts {
            busy: Some(Vec::new()),
            done: HashMap::new(),
        }
    }

    fn kinds(steps: &[Step]) -> Vec<(String, Kind)> {
        steps
            .iter()
            .map(|s| {
                (
                    s.path.file_name().unwrap().to_string_lossy().into_owned(),
                    s.kind,
                )
            })
            .collect()
    }

    #[test]
    fn a_quiet_folder_loses_its_ignored_artifacts_and_keeps_tracked_ones() {
        let (data, app) = setup("artifacts", "ABC-1");
        // A tracked build/ folder is source, not an artifact.
        std::fs::create_dir_all(app.join("build")).unwrap();
        std::fs::write(app.join("build/config.json"), "{}").unwrap();
        sh(&app, &["add", "build"]);
        sh(&app, &["commit", "-qm", "build config"]);
        // Unpushed, so the folder itself stays and only artifacts go.
        age(&data, 10);
        let steps = plan(&data, SystemTime::now(), &facts());
        assert_eq!(kinds(&steps), [("node_modules".into(), Kind::Artifact)]);
        apply(&data, &steps);
        assert!(!app.join("node_modules").exists());
        assert!(app.join("build/config.json").exists());
        // Taking it out does not make the folder look worked in.
        let idle = SystemTime::now()
            .duration_since(newest(app.parent().unwrap(), std::slice::from_ref(&app)));
        assert!(idle.unwrap() >= 9 * DAY);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_recent_or_busy_folder_is_left_alone() {
        let (data, app) = setup("recent", "ABC-2");
        assert!(
            plan(&data, SystemTime::now(), &facts()).is_empty(),
            "worked in today"
        );
        age(&data, 40);
        let busy = Facts {
            busy: Some(vec![app.join("src")]),
            done: HashMap::new(),
        };
        assert!(
            plan(&data, SystemTime::now(), &busy).is_empty(),
            "a process works there"
        );
        let unknown = Facts {
            busy: None,
            done: HashMap::new(),
        };
        assert!(
            plan(&data, SystemTime::now(), &unknown).is_empty(),
            "not known what runs"
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_finished_folder_goes_after_30_days_or_3_days_after_its_ticket() {
        let (data, _) = setup("finished", "ABC-3");
        age(&data, 31);
        let steps = plan(&data, SystemTime::now(), &facts());
        assert_eq!(kinds(&steps), [("ABC-3".into(), Kind::Folder)]);
        apply(&data, &steps);
        assert!(!data.join("work/ABC-3").exists());
        let _ = std::fs::remove_dir_all(&data);

        let (data, _) = setup("done", "ABC-4");
        age(&data, 4);
        let mut done = facts();
        done.done
            .insert("ABC-4".into(), SystemTime::now() - 4 * DAY);
        assert_eq!(
            kinds(&plan(&data, SystemTime::now(), &done)),
            [("ABC-4".into(), Kind::Folder)]
        );
        done.done.insert("ABC-4".into(), SystemTime::now() - DAY);
        assert!(
            plan(&data, SystemTime::now(), &done).is_empty(),
            "done only yesterday"
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn uncommitted_unpushed_or_foreign_work_keeps_the_folder() {
        let (data, app) = setup("dirty", "ABC-5");
        std::fs::write(app.join("notes.txt"), "mine").unwrap();
        age(&data, 40);
        assert!(
            !plan(&data, SystemTime::now(), &facts())
                .iter()
                .any(|s| s.kind == Kind::Folder),
            "untracked file"
        );
        let _ = std::fs::remove_dir_all(&data);

        let (data, app) = setup("unpushed", "ABC-6");
        std::fs::write(app.join("main.rs"), "fn main() { todo!() }\n").unwrap();
        sh(&app, &["commit", "-qam", "local only"]);
        age(&data, 40);
        assert!(
            !plan(&data, SystemTime::now(), &facts())
                .iter()
                .any(|s| s.kind == Kind::Folder)
        );
        let _ = std::fs::remove_dir_all(&data);

        let (data, app) = setup("foreign", "ABC-7");
        std::fs::write(app.parent().unwrap().join("report.md"), "the user's").unwrap();
        age(&data, 40);
        let steps = plan(&data, SystemTime::now(), &facts());
        assert_eq!(
            kinds(&steps),
            [("node_modules".into(), Kind::Artifact)],
            "artifacts still go"
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_squash_merged_branch_goes_only_with_its_ticket_done() {
        let (data, app) = setup("gone", "ABC-8");
        sh(&app, &["switch", "-q", "-c", "abc-8"]);
        std::fs::write(app.join("main.rs"), "fn main() { 1; }\n").unwrap();
        sh(&app, &["commit", "-qam", "feature"]);
        sh(&app, &["push", "-q", "-u", "origin", "abc-8"]);
        // Merged and the branch deleted on the remote.
        sh(&app, &["push", "-q", "origin", "--delete", "abc-8"]);
        sh(&app, &["fetch", "-q", "--prune"]);
        age(&data, 40);
        assert!(
            !plan(&data, SystemTime::now(), &facts())
                .iter()
                .any(|s| s.kind == Kind::Folder)
        );
        let mut done = facts();
        done.done
            .insert("ABC-8".into(), SystemTime::now() - 5 * DAY);
        assert!(
            plan(&data, SystemTime::now(), &done)
                .iter()
                .any(|s| s.kind == Kind::Folder)
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_dependency_cache_goes_once_nothing_links_to_it() {
        let (data, app) = setup("deps", "ABC-9");
        let deps = data.join("repos/.deps");
        let used = deps.join("aaa/node_modules");
        let unused = deps.join("bbb/node_modules");
        std::fs::create_dir_all(&used).unwrap();
        std::fs::create_dir_all(&unused).unwrap();
        std::fs::remove_dir_all(app.join("node_modules")).unwrap();
        std::os::unix::fs::symlink(&used, app.join("node_modules")).unwrap();
        age(&deps, 10);
        let steps = plan(&data, SystemTime::now(), &facts());
        assert_eq!(kinds(&steps), [("bbb".into(), Kind::Deps)]);
        // Only <data>/work and <data>/repos/.deps are ever looked at.
        assert!(
            steps
                .iter()
                .all(|s| s.path.starts_with(data.join("work")) || s.path.starts_with(&deps))
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn the_settings_line_reads_the_last_pass() {
        let data = std::env::temp_dir().join(format!("orbit-cleanup-note-{}", std::process::id()));
        std::fs::create_dir_all(&data).unwrap();
        assert_eq!(note(&data), None);
        std::fs::write(
            data.join("cleanup.log"),
            "2026-10-18T10:00:00Z folder 100 /x\n2026-10-18T10:00:00Z summary freed=41000000000 storage=12300000000\n",
        )
        .unwrap();
        let line = note(&data).unwrap();
        assert!(
            line.starts_with("storage: 12.3 GB · freed 41.0 GB on 18 Oct"),
            "{line}"
        );
        let _ = std::fs::remove_dir_all(&data);
    }
}
