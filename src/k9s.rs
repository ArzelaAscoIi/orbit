//! The profile's k9s pieces for the debug panes (`Profile::k9s`): the
//! entries of its `aliases.yaml`, `views.yaml` and `plugins.yaml` go into
//! the files of the same name in the user's k9s config dir (`K9S_CONFIG_DIR`,
//! else the OS default k9s itself uses), kept between two marker lines so a
//! changed profile replaces its own block and never touches the user's
//! entries. Every other file is a script a plugin calls: written to
//! `<data>/bin`, and the plugins block names it by full path, so the user's
//! PATH does not matter. Run before a k9s pane opens; nothing to do without
//! a profile.
//!
//! ponytail: a text append, not a YAML merge. Every k9s file is one root
//! key with one block mapping under it, so a block at the end, at the
//! file's own indent, is that mapping's next entries. A YAML parser if a
//! user's file ever defeats it.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::profile::Profile;

pub fn install(profile: &Profile) -> Result<()> {
    if profile.k9s.is_empty() {
        return Ok(());
    }
    let bin = crate::store::data_dir().join("bin");
    let mut scripts = Vec::new();
    for (name, text) in profile.k9s.iter().filter(|(n, _)| !n.contains('.')) {
        std::fs::create_dir_all(&bin)?;
        // Absolute: k9s runs the plugin from its own cwd.
        let path = std::fs::canonicalize(&bin)?.join(name);
        std::fs::write(&path, text).with_context(|| path.display().to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        scripts.push((name.clone(), path));
    }
    let dir = config_dir();
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    for (name, text) in profile.k9s.iter().filter(|(n, _)| n.ends_with(".yaml")) {
        let mut text = text.clone();
        for (script, path) in &scripts {
            let quoted = serde_json::to_string(&path.display().to_string()).unwrap_or_default();
            text = text.replace(&format!("command: {script}"), &format!("command: {quoted}"));
        }
        let file = dir.join(name);
        let existing = std::fs::read_to_string(&file).unwrap_or_default();
        let merged = merged(&existing, &text, &profile.name)
            .with_context(|| format!("profile k9s/{name}"))?;
        if merged != existing {
            std::fs::write(&file, merged).with_context(|| file.display().to_string())?;
        }
    }
    Ok(())
}

/// Where k9s reads its files: `K9S_CONFIG_DIR`, else the XDG config dir
/// (`~/Library/Application Support/k9s` on a Mac, `~/.config/k9s` elsewhere).
fn config_dir() -> PathBuf {
    std::env::var_os("K9S_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            let config = if cfg!(target_os = "macos") {
                "Library/Application Support"
            } else {
                ".config"
            };
            crate::store::home().map(|h| h.join(config).join("k9s"))
        })
        .unwrap_or_else(|| PathBuf::from("k9s"))
}

fn markers(profile: &str) -> (String, String) {
    let name = if profile.trim().is_empty() {
        "profile"
    } else {
        profile.trim()
    };
    (
        format!("# orbit:{name} begin (managed, edit the profile instead)"),
        format!("# orbit:{name} end"),
    )
}

fn is_content(line: &str) -> bool {
    let t = line.trim_start();
    !t.is_empty() && !t.starts_with('#')
}

/// The first indent under the root key: how deep this file nests its
/// entries. `None` when nothing is nested yet.
fn indent_of(lines: &[&str]) -> Option<usize> {
    lines
        .iter()
        .filter(|l| is_content(l))
        .map(|l| l.len() - l.trim_start().len())
        .find(|&n| n > 0)
}

/// The key of an entry line at `indent`: `workers:` or `workers: pod a=b`.
fn key_at(line: &str, indent: usize) -> Option<&str> {
    if !is_content(line) || line.len() - line.trim_start().len() != indent {
        return None;
    }
    line.trim().split_once(':').map(|(k, _)| k.trim())
}

/// `existing` (the user's file, maybe empty) with the entries of `piece`
/// (a whole k9s file: root key, then entries) as its block, written at the
/// user's indent; a block already there is replaced. An entry the user
/// already has under their own hand (same key outside the block) is theirs
/// and left out: two keys alike would break the file.
pub fn merged(existing: &str, piece: &str, profile: &str) -> Result<String> {
    let piece_lines: Vec<&str> = piece.lines().collect();
    let root_at = piece_lines
        .iter()
        .position(|l| is_content(l))
        .context("the profile's file is empty")?;
    let root = piece_lines[root_at].trim_end();
    if root.len() - root.trim_start().len() != 0 || !root.ends_with(':') {
        bail!("the profile's file must start with a root key like views:, not {root:?}");
    }
    let body = &piece_lines[root_at + 1..];
    let from = indent_of(body).context("the profile's file has no entries")?;
    let old: Vec<&str> = existing.lines().collect();
    let to = indent_of(&old).unwrap_or(from);
    let (begin, end) = markers(profile);
    let block_at = old.iter().position(|l| l.trim_end() == begin);
    let block_end = block_at.map_or(0, |b| {
        old[b..]
            .iter()
            .position(|l| l.trim_end() == end)
            .map_or(old.len(), |i| b + i + 1)
    });
    let theirs: Vec<&str> = old
        .iter()
        .enumerate()
        .filter(|(i, _)| block_at.is_none_or(|b| *i < b || *i >= block_end))
        .filter_map(|(_, l)| key_at(l, to))
        .collect();

    // Entries: a key line at the piece's indent and what follows it; the
    // comments and blank lines before a key belong to it.
    let mut entries: Vec<Vec<&str>> = vec![vec![]];
    let trimmed: Vec<&str> = body
        .iter()
        .rev()
        .skip_while(|l| l.trim().is_empty())
        .copied()
        .collect();
    for line in trimmed.into_iter().rev() {
        let at_top = !line.trim().is_empty() && line.len() - line.trim_start().len() == from;
        let has_key = entries
            .last()
            .is_some_and(|e| e.iter().any(|l| key_at(l, from).is_some()));
        if at_top && has_key && (key_at(line, from).is_some() || line.trim_start().starts_with('#'))
        {
            entries.push(vec![]);
        }
        entries.last_mut().unwrap().push(line);
    }
    let mut block = vec![begin.clone()];
    for entry in entries {
        let key = entry.iter().find_map(|l| key_at(l, from));
        if key.is_none_or(|k| theirs.contains(&k)) {
            continue;
        }
        for line in entry {
            if line.trim().is_empty() {
                block.push(String::new());
                continue;
            }
            let lead = line.len() - line.trim_start().len();
            block.push(format!(
                "{}{}",
                " ".repeat(lead * to / from),
                line.trim_start()
            ));
        }
    }
    block.push(end.clone());
    let block = if block.len() == 2 {
        None
    } else {
        Some(block.join("\n"))
    };

    if let Some(b) = block_at {
        let mut out: Vec<String> = old[..b].iter().map(|l| l.to_string()).collect();
        out.extend(block);
        out.extend(old[block_end..].iter().map(|l| l.to_string()));
        return Ok(out.join("\n") + "\n");
    }
    let Some(block) = block else {
        return Ok(existing.to_owned());
    };
    let mut out = existing.trim_end().to_owned();
    if !old.iter().any(|l| l.trim_end() == root) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(root);
    }
    Ok(format!("{out}\n{block}\n"))
}

#[cfg(test)]
mod tests {

    use super::*;

    const PIECE: &str = "views:\n  # pool pods\n  workers:\n    columns:\n      - NAME\n";

    #[test]
    fn a_missing_file_gets_the_root_and_the_block() {
        assert_eq!(
            merged("", PIECE, "acme").unwrap(),
            "views:\n# orbit:acme begin (managed, edit the profile instead)\n  # pool pods\n  workers:\n    columns:\n      - NAME\n# orbit:acme end\n"
        );
    }

    #[test]
    fn the_users_entries_stay_and_the_block_takes_their_indent() {
        let theirs = "views:\n    v1/pods:\n        columns:\n            - DEV\n";
        let out = merged(theirs, PIECE, "acme").unwrap();
        assert!(out.starts_with(theirs), "{out}");
        assert!(
            out.contains("\n    workers:\n        columns:\n            - NAME\n"),
            "{out}"
        );
    }

    #[test]
    fn a_second_install_replaces_only_its_block() {
        let theirs = "aliases:\n  dp: deployments\n";
        let once = merged(theirs, "aliases:\n  workers: pod a=b\n", "acme").unwrap();
        let mut with_tail = once.clone();
        with_tail.push_str("  sec: v1/secrets\n");
        let twice = merged(&with_tail, "aliases:\n  workers: pod a=c\n", "acme").unwrap();
        assert_eq!(
            twice,
            "aliases:\n  dp: deployments\n# orbit:acme begin (managed, edit the profile instead)\n  workers: pod a=c\n# orbit:acme end\n  sec: v1/secrets\n"
        );
        assert_eq!(
            merged(&twice, "aliases:\n  workers: pod a=c\n", "acme").unwrap(),
            twice
        );
    }

    #[test]
    fn an_entry_the_user_already_has_is_left_to_them() {
        let theirs = "views:\n  workers:\n    columns:\n      - DEV\n";
        assert_eq!(merged(theirs, PIECE, "acme").unwrap(), theirs);
        let two = "views:\n  # one\n  a:\n    columns:\n      - X\n  # two\n  workers:\n    columns:\n      - Y\n";
        assert_eq!(
            merged(theirs, two, "acme").unwrap(),
            "views:\n  workers:\n    columns:\n      - DEV\n# orbit:acme begin (managed, edit the profile instead)\n  # one\n  a:\n    columns:\n      - X\n# orbit:acme end\n"
        );
    }

    /// Against real dirs: `ORBIT_DATA=target/dev-data K9S_CONFIG_DIR=<a copy of
    /// your k9s dir> cargo test k9s::tests::installs -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn installs_the_profile_into_the_env_dirs() {
        install(&crate::profile::get()).unwrap();
        println!(
            "{}",
            std::fs::read_to_string(config_dir().join("plugins.yaml")).unwrap()
        );
    }

    #[test]
    fn a_piece_without_a_root_key_is_refused() {
        assert!(merged("", "  workers: x\n", "acme").is_err());
        assert!(merged("", "views: {}\n", "acme").is_err());
    }
}
