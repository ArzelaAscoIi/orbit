//! Built-in profiles: every folder under `profiles/` (profile.json,
//! skills/<name>/SKILL.md and k9s/*) becomes a bundle compiled into the app, and
//! `profiles/default` names the one a fresh settings.json starts with
//! (src/profile.rs). Delete a folder, or the default, for a build without it.
//!
//! `ORBIT_RELEASES_REPO` (`owner/repo` or its GitHub URL) names the repo this
//! build updates from and sends feedback PRs to; unset: Cargo.toml's
//! `repository`. It reaches the app as `ORBIT_REPO_URL`.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=profiles");
    println!("cargo::rerun-if-env-changed=ORBIT_RELEASES_REPO");
    let repo = match std::env::var("ORBIT_RELEASES_REPO") {
        Ok(r) if r.starts_with("https://") => r.trim_end_matches('/').to_owned(),
        Ok(r) if !r.trim().is_empty() => format!("https://github.com/{}", r.trim()),
        _ => std::env::var("CARGO_PKG_REPOSITORY").unwrap(),
    };
    println!("cargo::rustc-env=ORBIT_REPO_URL={repo}");
    let out = std::env::var("OUT_DIR").unwrap();
    let mut list = String::from("pub const BUILT_IN: &[(&str, &str)] = &[\n");
    let mut names: Vec<String> = std::fs::read_dir("profiles")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("profile.json").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in &names {
        let dir = Path::new("profiles").join(name);
        let text = std::fs::read_to_string(dir.join("profile.json")).unwrap();
        let mut profile: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("profiles/{name}/profile.json: {e}"));
        let mut files = serde_json::Map::new();
        let mut extras = serde_json::Map::new();
        for skill in std::fs::read_dir(dir.join("skills"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let folder = skill.file_name().to_string_lossy().into_owned();
            if let Ok(text) = std::fs::read_to_string(skill.path().join("SKILL.md")) {
                files.insert(folder.clone(), text.into());
            }
            if let Ok(text) = std::fs::read_to_string(skill.path().join("EXTRA.md")) {
                extras.insert(folder, text.into());
            }
        }
        profile["skillFiles"] = files.into();
        profile["skillExtras"] = extras.into();
        let mut k9s = serde_json::Map::new();
        for file in std::fs::read_dir(dir.join("k9s"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Ok(text) = std::fs::read_to_string(file.path()) {
                k9s.insert(file.file_name().to_string_lossy().into_owned(), text.into());
            }
        }
        profile["k9s"] = k9s.into();
        let bundle = Path::new(&out).join(format!("profile-{name}.json"));
        std::fs::write(&bundle, profile.to_string()).unwrap();
        writeln!(
            list,
            "    ({name:?}, include_str!(concat!(env!(\"OUT_DIR\"), \"/profile-{name}.json\"))),"
        )
        .unwrap();
    }
    list.push_str("];\n");
    let default = std::fs::read_to_string("profiles/default")
        .ok()
        .map(|d| d.trim().to_owned())
        .filter(|d| names.contains(d));
    writeln!(list, "pub const DEFAULT: Option<&str> = {default:?};").unwrap();
    std::fs::write(Path::new(&out).join("profiles.rs"), list).unwrap();
}
