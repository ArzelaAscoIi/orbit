//! Updating the app in place: the newest published release of `REPO`
//! (never a draft; a release candidate only while this build is one), its
//! `*-macos.zip` downloaded with the user's `gh`, checked against the
//! release's `.sha256` and with `codesign --verify`, unzipped next to the
//! running Orbit.app and swapped in for it. The caller relaunches. A build
//! not running from an .app (`cargo run`) is never touched.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// "owner/repo" of `crate::REPO`.
fn repo() -> &'static str {
    crate::REPO.trim_start_matches("https://github.com/")
}

/// The newest release's tag when it is newer than this build.
pub fn check() -> Result<Option<String>> {
    let path = format!("repos/{}/releases?per_page=20", repo());
    let rows = crate::claude::gh(&[
        "api",
        &path,
        "--jq",
        r#".[] | "\(.tag_name)\t\(.prerelease)\t\(.draft)""#,
    ])?;
    Ok(pick(&rows, crate::VERSION))
}

/// From `tag<TAB>prerelease<TAB>draft` rows, newest first: the first one
/// this build may take that is newer than `current`. Drafts never;
/// release candidates only while `current` is one.
fn pick(rows: &str, current: &str) -> Option<String> {
    let candidate_ok = current.contains('-');
    rows.lines()
        .filter_map(|row| {
            let mut parts = row.split('\t');
            Some((parts.next()?, parts.next()?, parts.next()?))
        })
        .filter(|(_, pre, draft)| *draft != "true" && (candidate_ok || *pre != "true"))
        .map(|(tag, ..)| tag.trim())
        .find(|tag| !tag.is_empty())
        .filter(|tag| newer(tag, current))
        .map(str::to_owned)
}

/// Replaces the running Orbit.app with release `tag`'s; returns its path.
pub fn install(tag: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let Some(app) = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
    else {
        bail!("not running from an .app: pull and rebuild instead");
    };
    // Next to the app, so the swap is a rename on one volume.
    let parent = app.parent().context("the app has no folder")?;
    let work = parent.join(format!(".orbit-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let result = swap(tag, app, &work);
    let _ = std::fs::remove_dir_all(&work);
    result.map(|()| app.to_path_buf())
}

fn swap(tag: &str, app: &Path, work: &Path) -> Result<()> {
    let dir = work.to_string_lossy();
    crate::claude::gh(&[
        "release",
        "download",
        tag,
        "-R",
        repo(),
        "-p",
        "*-macos.zip",
        "-p",
        "*-macos.zip.sha256",
        "-D",
        &dir,
    ])?;
    let zip = std::fs::read_dir(work)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "zip"))
        .context("the release has no macOS zip")?;
    let sum = std::fs::read_to_string(zip.with_extension("zip.sha256"))
        .context("the release has no .sha256 for its zip: not installing it")?;
    let actual = run("shasum", &["-a", "256"], &zip)?;
    if !same_sum(&sum, &actual) {
        bail!("the downloaded zip does not match the release's .sha256: not installing it");
    }
    let status = std::process::Command::new("ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(work)
        .status()?;
    if !status.success() {
        bail!("could not unzip {}", zip.display());
    }
    let new = std::fs::read_dir(work)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .context("the zip has no .app")?;
    run("codesign", &["--verify", "--deep", "--strict"], &new)
        .context("the new app's signature does not hold: not installing it")?;
    // The running binary stays mapped; the old bundle moves aside first so
    // a failed rename puts it back.
    let old = work.join("old.app");
    std::fs::rename(app, &old)?;
    if let Err(error) = std::fs::rename(&new, app) {
        std::fs::rename(&old, app)?;
        return Err(error.into());
    }
    Ok(())
}

/// Runs `program args path`; its output, or an error with what it said.
fn run(program: &str, args: &[&str], path: &Path) -> Result<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .arg(path)
        .output()
        .with_context(|| format!("could not run {program}"))?;
    if !out.status.success() {
        bail!("{program}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether two `shasum` lines (`<hex>  <file>`) name the same hash.
fn same_sum(expected: &str, actual: &str) -> bool {
    let hash = |s: &str| {
        s.split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase()
    };
    let expected = hash(expected);
    expected.len() == 64 && expected == hash(actual)
}

/// Whether tag `latest` ("v0.0.2-rc1") is a later version than `current`
/// ("0.0.1"): numbers compared as numbers, a release after its candidates.
fn newer(latest: &str, current: &str) -> bool {
    fn key(v: &str) -> (Vec<u64>, bool, u64) {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
        let numbers = core.split('.').map(|n| n.parse().unwrap_or(0)).collect();
        let pre_number = pre
            .map(|p| p.trim_start_matches(|c: char| !c.is_ascii_digit()))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        (numbers, pre.is_none(), pre_number)
    }
    !latest.is_empty() && key(latest) > key(current)
}

#[cfg(test)]
mod tests {
    use super::{newer, pick, same_sum};

    #[test]
    fn updates_skip_drafts_and_candidates_and_need_a_matching_sum() {
        let rows = "v0.2.0\tfalse\ttrue\nv0.1.1-rc1\ttrue\tfalse\nv0.1.0\tfalse\tfalse\n";
        assert_eq!(pick(rows, "0.0.9").as_deref(), Some("v0.1.0"));
        assert_eq!(pick(rows, "0.1.0"), None);
        // A candidate build follows candidates.
        assert_eq!(pick(rows, "0.1.1-rc0").as_deref(), Some("v0.1.1-rc1"));
        let hex = "a".repeat(64);
        assert!(same_sum(
            &format!("{hex}  orbit.zip\n"),
            &format!("{hex}  /tmp/x/orbit.zip\n")
        ));
        assert!(!same_sum(
            &format!("{hex}  a"),
            &format!("{}  a", "b".repeat(64))
        ));
        assert!(!same_sum("", ""));
    }

    #[test]
    fn compares_versions_and_candidates() {
        assert!(newer("v0.0.1-rc10", "0.0.1-rc9"));
        assert!(newer("v0.0.1", "0.0.1-rc3"));
        assert!(newer("v0.1.0", "0.0.9"));
        assert!(!newer("v0.0.1-rc3", "0.0.1-rc3"));
        assert!(!newer("v0.0.1-rc3", "0.0.1"));
        assert!(!newer("", "0.0.1"));
    }
}
