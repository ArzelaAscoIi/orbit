//! One git worktree per ticket and repo, so a session never builds in a
//! shared checkout and never on top of someone's half-done branch.
//!
//! `orbit worktree <repos> <owner/repo> <branch>` runs inside a
//! ticket session (the brief names it; the build skill calls it instead of
//! cloning). It keeps one clone per repo under `<repos>` that nobody works
//! in, fetches it, and adds `./<repo>` in the current folder as a worktree
//! on `<branch>`:
//! - the local branch, if one exists: its unlanded work is never discarded;
//! - else the remote branch, tracked (fix mode on an open PR);
//! - else a new branch from the freshly fetched default branch.
//!
//! A `./<repo>` that already exists is reused as it is. Either way the path
//! is printed only after the isolation check: its top level is itself, not
//! the shared clone. Nothing here removes a worktree.
//!
//! JavaScript dependencies come along (`deps`): the worktree's folder and
//! each folder under it with a `package.json` and a lockfile get
//! `node_modules` linked from `<repos>/.deps/<lockfile hash>`, installed
//! there once with the lockfile's own tool, so a session can run the repo's
//! lint and tests before it pushes. A failed install is said and skipped:
//! the worktree still works, CI checks it.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The subcommand. Prints the worktree's path on success.
pub fn run(repos: &Path, slug: &str, branch: &str) -> Result<()> {
    let here = std::env::current_dir()?;
    let path = add(repos, slug, branch, &here)?;
    println!("{}", path.display());
    Ok(())
}

fn add(repos: &Path, slug: &str, branch: &str, folder: &Path) -> Result<PathBuf> {
    let Some((_, repo)) = slug
        .split_once('/')
        .filter(|(o, r)| name_ok(o) && name_ok(r))
    else {
        bail!("expected <owner>/<repo>, got {slug:?}");
    };
    if branch.is_empty() || branch.starts_with('-') || branch.contains("..") {
        bail!("not a branch name: {branch:?}");
    }
    let shared = repos.join(slug);
    let target = folder.join(repo);
    if !target.exists() {
        if shared.join(".git").exists() {
            git(&shared, &["fetch", "--prune", "origin"])?;
            // Forgets worktrees whose folder is gone; touches no files.
            git(&shared, &["worktree", "prune"])?;
        } else {
            std::fs::create_dir_all(repos)?;
            super::gh(&["repo", "clone", slug, &shared.to_string_lossy()])?;
        }
        let target_s = target.to_string_lossy();
        let local = format!("refs/heads/{branch}");
        let remote = format!("refs/remotes/origin/{branch}");
        if has_ref(&shared, &local) {
            git(&shared, &["worktree", "add", &target_s, branch])?;
        } else if has_ref(&shared, &remote) {
            let start = format!("origin/{branch}");
            git(
                &shared,
                &[
                    "worktree", "add", "--track", "-b", branch, &target_s, &start,
                ],
            )?;
        } else {
            let base = git(
                &shared,
                &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
            )?;
            git(
                &shared,
                &[
                    "worktree",
                    "add",
                    "--no-track",
                    "-b",
                    branch,
                    &target_s,
                    base.trim(),
                ],
            )?;
        }
    }
    isolated(&target, &shared)?;
    deps(repos, &target);
    Ok(target)
}

/// The install that keeps to a folder's lockfile, by the lockfile there.
fn installer(dir: &Path) -> Option<(&'static str, &'static [&'static str])> {
    if dir.join("pnpm-lock.yaml").exists() {
        Some(("pnpm-lock.yaml", &["pnpm", "install", "--frozen-lockfile"]))
    } else if dir.join("yarn.lock").exists() && dir.join(".yarnrc.yml").exists() {
        Some(("yarn.lock", &["yarn", "install", "--immutable"]))
    } else if dir.join("yarn.lock").exists() {
        Some(("yarn.lock", &["yarn", "install", "--frozen-lockfile"]))
    } else if dir.join("package-lock.json").exists() {
        Some(("package-lock.json", &["npm", "ci"]))
    } else {
        None
    }
}

/// `node_modules` for the worktree's root and its first-level folders that
/// need them (module comment).
fn deps(repos: &Path, target: &Path) {
    let subdirs = std::fs::read_dir(target)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.ends_with(".git"));
    for dir in std::iter::once(target.to_path_buf()).chain(subdirs) {
        if let Err(error) = deps_in(repos, &dir) {
            eprintln!("no node_modules in {}: {error:#}", dir.display());
        }
    }
}

fn deps_in(repos: &Path, dir: &Path) -> Result<()> {
    let modules = dir.join("node_modules");
    let Some((lock, install)) = installer(dir) else {
        return Ok(());
    };
    if !dir.join("package.json").exists() || modules.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(dir.join(lock))?;
    let cache = repos
        .join(".deps")
        .join(format!("{:016x}", crate::store::text_hash(&text)));
    let cached = cache.join("node_modules");
    if !cached.exists() {
        // Installed here, then kept for the next worktree with this lockfile.
        let status = Command::new(install[0])
            .args(&install[1..])
            .current_dir(dir)
            .status()
            .with_context(|| format!("could not run {}", install[0]))?;
        if !status.success() {
            bail!("{} failed", install.join(" "));
        }
        std::fs::create_dir_all(&cache)?;
        if std::fs::rename(&modules, &cached).is_err() {
            // Another session cached the same lockfile first: keep this one.
            return Ok(());
        }
    }
    std::os::unix::fs::symlink(&cached, &modules)?;
    Ok(())
}

/// The worktree is its own top level and not the shared clone.
fn isolated(target: &Path, shared: &Path) -> Result<()> {
    let top = git(target, &["rev-parse", "--show-toplevel"])?;
    let top = Path::new(top.trim()).canonicalize()?;
    let target = target.canonicalize()?;
    if top != target || shared.canonicalize().is_ok_and(|s| s == top) {
        bail!(
            "{} is not its own worktree (top level {})",
            target.display(),
            top.display()
        );
    }
    Ok(())
}

fn has_ref(repo: &Path, name: &str) -> bool {
    git(repo, &["show-ref", "--verify", "--quiet", name]).is_ok()
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("could not run git")?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        bail!("git {}: {}", args.join(" "), error.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A GitHub owner or repo name: letters, digits, `.`, `_`, `-`; never `.`,
/// `..` or a leading `-`, so it cannot climb out of `repos` or pass as a
/// flag to `gh repo clone`.
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// What the brief tells a session to run, with `<owner/repo> <branch>` after it.
pub fn command() -> Result<String> {
    let exe = std::env::current_exe().context("no path to this binary")?;
    let repos = crate::store::data_dir().join("repos");
    let quote = super::status::quote;
    Ok(format!("{} worktree {}", quote(&exe), quote(&repos)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worktree_never_leaves_its_folders() {
        let at = std::env::temp_dir().join(format!("orbit-slug-{}", std::process::id()));
        for (slug, branch) in [
            ("a/../../x", "b"),
            ("../x", "b"),
            ("-o/r", "b"),
            ("o/--upload-pack=x", "b"),
            ("o/..", "b"),
            ("o/r", "-b"),
            ("o/r", "a/../../b"),
        ] {
            let err = add(&at.join("repos"), slug, branch, &at).unwrap_err();
            assert!(!err.to_string().is_empty(), "{slug} {branch}");
        }
        assert!(name_ok("acme.web_ui-2") && !name_ok("a/b"));
        assert_eq!(
            super::super::status::quote(Path::new("/it's/x")),
            r"'/it'\''s/x'"
        );
    }

    #[test]
    fn a_worktree_gets_node_modules_from_the_cache_of_its_lockfile() {
        let tmp = std::env::temp_dir().join(format!("mcr-deps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let (repos, ui) = (tmp.join("repos"), tmp.join("wt/ui"));
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::write(ui.join("package.json"), "{}").unwrap();
        std::fs::write(ui.join("yarn.lock"), "lock").unwrap();
        assert_eq!(
            installer(&ui).unwrap().1,
            ["yarn", "install", "--frozen-lockfile"]
        );
        // Installed once for this lockfile: linked, nothing runs.
        let cached = repos
            .join(".deps")
            .join(format!("{:016x}", crate::store::text_hash("lock")))
            .join("node_modules");
        std::fs::create_dir_all(cached.join("eslint")).unwrap();
        deps(&repos, &tmp.join("wt"));
        assert!(ui.join("node_modules/eslint").exists());
        assert!(installer(&tmp).is_none(), "no lockfile, no install");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn worktree_is_fresh_then_reused_and_keeps_work() {
        let tmp = std::env::temp_dir().join(format!("mcr-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // An "origin" with one commit on main, and a shared clone of it.
        let origin = tmp.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "root",
            ],
        ] {
            git(&origin, args).unwrap();
        }
        let repos = tmp.join("repos");
        let shared = repos.join("acme/app");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        git(
            &tmp,
            &[
                "clone",
                "-q",
                &origin.to_string_lossy(),
                &shared.to_string_lossy(),
            ],
        )
        .unwrap();

        let folder = tmp.join("DEV-1");
        std::fs::create_dir_all(&folder).unwrap();
        let path = add(&repos, "acme/app", "dev-1-build", &folder).unwrap();
        assert_eq!(path, folder.join("app"));
        let branch = git(&path, &["branch", "--show-current"]).unwrap();
        assert_eq!(branch.trim(), "dev-1-build");

        // Work in it, then ask again: the same worktree, the work still there.
        std::fs::write(path.join("wip.txt"), "unlanded").unwrap();
        assert_eq!(
            add(&repos, "acme/app", "dev-1-build", &folder).unwrap(),
            path
        );
        assert!(path.join("wip.txt").exists());

        // The shared clone itself never passes as a worktree.
        assert!(isolated(&shared, &shared).is_err());
        assert!(add(&repos, "no-slash", "b", &folder).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
