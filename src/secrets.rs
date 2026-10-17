//! The user's keys: Linear (required), Anthropic, GitHub and incident.io
//! (optional).
//!
//! An environment variable wins, so a `.env` runs the app with no Keychain
//! at all. Otherwise all keys live in one Keychain entry, read once per
//! run: macOS asks for permission per binary, so one entry means at most
//! one prompt after a rebuild, and `ORBIT_NO_KEYCHAIN=1` means none.
//! With the Keychain off, keys typed into the app go to `<data>/keys.json`,
//! readable by the user only, so they are still there after a restart.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum Secret {
    Linear,
    Anthropic,
    Github,
    IncidentIo,
}

impl Secret {
    fn env_var(self) -> &'static str {
        match self {
            Secret::Linear => "LINEAR_API_KEY",
            Secret::Anthropic => "ANTHROPIC_API_KEY",
            Secret::Github => "GITHUB_TOKEN",
            Secret::IncidentIo => "INCIDENT_IO_API_KEY",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Secret::Linear => "linear",
            Secret::Anthropic => "anthropic",
            Secret::Github => "github",
            Secret::IncidentIo => "incident-io",
        }
    }
}

/// A key's short name: "anthropic", "github", "incident-io".
pub fn name(secret: Secret) -> &'static str {
    secret.name()
}

/// The key from the environment, else from the Keychain.
pub fn get(secret: Secret) -> Option<String> {
    // Demo mode signs in and writes walkthroughs from fixtures (demo.rs).
    if crate::demo::on() {
        return matches!(secret, Secret::Linear | Secret::Anthropic).then(|| "demo".to_owned());
    }
    std::env::var(secret.env_var())
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| stored().lock().unwrap().get(secret.name()).cloned())
}

/// Remembers the key for later runs: the Keychain, else `keys.json`.
pub fn set(secret: Secret, value: &str) -> keyring::Result<()> {
    let mut map = stored().lock().unwrap();
    reread(&mut map);
    map.insert(secret.name().to_owned(), value.to_owned());
    write(&map)
}

pub fn clear(secret: Secret) -> keyring::Result<()> {
    let mut map = stored().lock().unwrap();
    reread(&mut map);
    map.remove(secret.name());
    write(&map)
}

/// The key as the Keychain has it now: another window of the app may have
/// replaced it (a refreshed Linear token) since this one read it.
pub fn fresh(secret: Secret) -> Option<String> {
    reread(&mut stored().lock().unwrap());
    get(secret)
}

/// Takes the Keychain entry (or `keys.json`) as it is now, so a write
/// changes one key without putting back another window's older ones.
fn reread(map: &mut BTreeMap<String, String>) {
    if !keychain_enabled() {
        *map = crate::store::load_at(&file());
    } else if let Ok(json) = entry().and_then(|e| e.get_password())
        && let Ok(now) = serde_json::from_str(&json)
    {
        *map = now;
    }
}

/// Where keys live with the Keychain off.
fn file() -> std::path::PathBuf {
    crate::store::data_dir().join("keys.json")
}

/// Written like `store::save_at` (temporary file, then renamed), but
/// readable by the user only: these are keys.
fn write_private(path: &std::path::Path, map: &BTreeMap<String, String>) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?
        .write_all(&serde_json::to_vec_pretty(map)?)?;
    std::fs::rename(tmp, path)
}

/// Where a saved key goes, for the Settings text.
pub fn place() -> &'static str {
    if keychain_enabled() {
        "the Keychain"
    } else {
        "keys.json in the data folder (Keychain off)"
    }
}

fn keychain_enabled() -> bool {
    crate::store::env_os("NO_KEYCHAIN").is_none()
}

fn entry() -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(crate::NAME, "secrets")
}

/// The one Keychain entry, read once. Older builds kept three entries;
/// they are folded in the first time and then removed.
fn stored() -> &'static Mutex<BTreeMap<String, String>> {
    static STORED: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    STORED.get_or_init(|| {
        let map = if !keychain_enabled() {
            crate::store::load_at(&file())
        } else {
            match entry().and_then(|e| e.get_password()) {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(_) => renamed().unwrap_or_else(migrate_legacy),
            }
        };
        Mutex::new(map)
    })
}

fn write(map: &BTreeMap<String, String>) -> keyring::Result<()> {
    if !keychain_enabled() {
        return write_private(&file(), map)
            .map_err(|e| keyring::Error::PlatformFailure(Box::new(e)));
    }
    let entry = entry()?;
    if map.is_empty() {
        return match entry.delete_credential() {
            Err(keyring::Error::NoEntry) => Ok(()),
            other => other,
        };
    }
    entry.set_password(&serde_json::to_string(map).unwrap_or_default())
}

/// After a rename: the entry the app kept under an earlier name, copied
/// to the new one (the old stays, so an older build still signs in).
fn renamed() -> Option<BTreeMap<String, String>> {
    let map: BTreeMap<String, String> = crate::PREVIOUS_NAMES.iter().find_map(|name| {
        let json = keyring::Entry::new(name, "secrets")
            .ok()?
            .get_password()
            .ok()?;
        serde_json::from_str(&json).ok()
    })?;
    let _ = write(&map);
    Some(map)
}

fn migrate_legacy() -> BTreeMap<String, String> {
    let legacy = [
        (Secret::Linear, "linear-api-key"),
        (Secret::Anthropic, "anthropic-api-key"),
        (Secret::Github, "github-token"),
    ];
    let mut map = BTreeMap::new();
    for (secret, account) in legacy {
        let Ok(old) = keyring::Entry::new(crate::NAME, account) else {
            continue;
        };
        if let Ok(value) = old.get_password() {
            map.insert(secret.name().to_owned(), value);
            let _ = old.delete_credential();
        }
    }
    if !map.is_empty() {
        let _ = write(&map);
    }
    map
}

/// The `sk-ant-…` key in whatever was pasted: the bare key, a quoted one,
/// an `x-api-key:` header, or a whole curl command.
pub fn anthropic_key_in(pasted: &str) -> Option<String> {
    let start = pasted.find("sk-ant-")?;
    let key: String = pasted[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (key.len() > "sk-ant-".len() + 10).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_file_is_private_and_reads_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("orbit-keys-{}", std::process::id()));
        let path = dir.join("keys.json");
        let map = BTreeMap::from([("anthropic".to_owned(), "k".to_owned())]);
        write_private(&path, &map).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let back: BTreeMap<String, String> = crate::store::load_at(&path);
        assert_eq!(back, map);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn anthropic_key_is_found_in_any_paste() {
        let key = "sk-ant-test-abcDEF123_xyz-000";
        for pasted in [
            key.to_owned(),
            format!("  \"{key}\"\n"),
            format!("--header \"x-api-key: {key}\" \\"),
        ] {
            assert_eq!(anthropic_key_in(&pasted).as_deref(), Some(key));
        }
        assert_eq!(anthropic_key_in("lin_api_123"), None);
    }
}
