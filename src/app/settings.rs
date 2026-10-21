//! settings.json, edited as text like VS Code's: the Settings view shows the
//! file, ⌘S saves and applies it. A key typed into it goes to the Keychain
//! and the file keeps `SAVED`; clearing that field removes the key. The
//! file is re-read when another copy of the app or an editor changes it,
//! unless the editor holds unsaved text.

use std::time::Duration;

use super::App;
use crate::secrets::{self, Secret};
use crate::store::{self, SAVED, Settings};

const FILE: &str = "settings.json";

impl App {
    /// The editor's text for these settings: keys only as `SAVED`.
    fn settings_json(&self, settings: &Settings) -> String {
        let mut shown = settings.clone();
        for (secret, field) in [
            (Secret::Anthropic, &mut shown.anthropic_api_key),
            (Secret::IncidentIo, &mut shown.incident_io_api_key),
            (Secret::Github, &mut shown.github_token),
        ] {
            *field = if self.has_key(secret) {
                SAVED.into()
            } else {
                String::new()
            };
        }
        serde_json::to_string_pretty(&shown).unwrap_or_default()
    }

    fn has_key(&self, secret: Secret) -> bool {
        match secret {
            Secret::Anthropic => self.has_own_anthropic_key(),
            Secret::IncidentIo => self.incident_key.is_some(),
            Secret::Github => self.github_token.is_some(),
            Secret::Linear => true,
        }
    }

    pub fn settings_dirty(&self) -> bool {
        self.settings_text != self.settings_loaded
    }

    /// Shows `settings` in the editor as the saved state.
    pub(super) fn show_settings(&mut self, settings: Settings) {
        self.settings_text = self.settings_json(&settings);
        self.settings_loaded = self.settings_text.clone();
        self.settings = settings;
    }

    /// Every frame: follows settings.json, profile.json and Laya's log on
    /// disk (each looked at every two seconds); a file with unsaved edits in
    /// its editor is left alone.
    pub(super) fn follow_settings(&mut self, ctx: &egui::Context) {
        let two = Duration::from_secs(2);
        if self
            .settings_seen
            .changed(&store::data_dir().join(FILE), two)
            && !self.settings_dirty()
        {
            let settings = store::load_settings();
            self.use_settings(&settings, ctx);
            self.show_settings(settings);
        }
        self.follow_profile();
        self.follow_onboarding(ctx);
        // The first look at the skills: nothing counts as new yet.
        if self.settings.profile_skills.is_none() && !crate::read_only() {
            self.remember_skills();
        }
        let log = store::data_dir().join("laya-shadow.jsonl");
        if self.laya_seen.changed(&log, two) {
            self.laya_agreement = crate::laya::agreement(&log);
        }
        let quick = store::data_dir().join("laya-quick.jsonl");
        if self.quick_seen.changed(&quick, two) {
            self.quick_stats = crate::laya::quick_stats(&quick);
        }
    }

    /// ⌘S in the editor: parse, move keys to the Keychain, write, apply.
    pub(super) fn save_settings_text(&mut self, ctx: &egui::Context) {
        let mut settings: Settings = match serde_json::from_str(&self.settings_text) {
            Ok(settings) => settings,
            Err(error) => {
                self.settings_error = Some(format!("not saved: {error}"));
                return;
            }
        };
        for (secret, key) in settings.take_keys() {
            if let Err(error) = self.set_key(secret, key) {
                self.settings_error = Some(error);
                return;
            }
        }
        if let Err(error) = store::save_settings(&settings) {
            self.settings_error = Some(format!("could not save settings: {error}"));
            return;
        }
        self.settings_error = None;
        self.use_settings(&settings, ctx);
        self.settings_seen.reset();
        self.show_settings(settings);
    }

    pub(super) fn revert_settings(&mut self) {
        self.settings_error = None;
        self.show_settings(store::load_settings());
    }

    /// A key from the file: stored and used, or with "" removed.
    pub(super) fn set_key(&mut self, secret: Secret, key: String) -> Result<(), String> {
        if key.is_empty() {
            if !self.has_key(secret) {
                return Ok(());
            }
            return match secret {
                Secret::Anthropic => {
                    self.forget_anthropic_key(None);
                    Ok(())
                }
                _ => {
                    secrets::clear(secret).map_err(|e| format!("could not remove a key: {e}"))?;
                    match secret {
                        Secret::IncidentIo => {
                            self.incident_key = None;
                            self.incidents.clear();
                        }
                        _ => self.github_token = None,
                    }
                    Ok(())
                }
            };
        }
        match secret {
            Secret::Anthropic => {
                let key = secrets::anthropic_key_in(&key)
                    .ok_or("anthropicApiKey doesn't look like an Anthropic key (sk-ant-...)")?;
                self.enable_anthropic(key);
            }
            _ => {
                secrets::set(secret, &key).map_err(|e| format!("could not save a key: {e}"))?;
                match secret {
                    Secret::IncidentIo => {
                        self.incident_key = Some(key);
                        self.incidents_at = None;
                        self.incidents_error = None;
                    }
                    _ => {
                        self.github_token = Some(key);
                        self.reload_diff();
                    }
                }
            }
        }
        Ok(())
    }

    /// The profile's profile.json, when there is a profile folder.
    fn profile_file(&self) -> Option<std::path::PathBuf> {
        crate::profile::folder(&self.settings.profile).map(|f| f.join("profile.json"))
    }

    /// Remembers profile.json as it is now; the setup session saving it
    /// ends the onboarding.
    pub(super) fn start_onboarding(&mut self) {
        let mut watch = store::Watch::default();
        if let Some(file) = self.profile_file() {
            watch.changed(&file, Duration::ZERO);
        }
        self.onboarding = Some(watch);
    }

    /// The setup session saved a profile that loads: it is done, so it
    /// stops, and Settings opens on the connections for the keys.
    fn follow_onboarding(&mut self, ctx: &egui::Context) {
        let Some(file) = self.profile_file() else {
            return;
        };
        let Some(watch) = &mut self.onboarding else {
            return;
        };
        if !watch.changed(&file, Duration::from_secs(1))
            || crate::profile::load(&self.settings.profile).is_err()
        {
            return;
        }
        self.onboarding = None;
        self.claude.stop(crate::claude::PROFILE);
        self.follow_profile();
        self.remember_skills();
        self.apply(super::Action::Show(super::View::Settings), ctx);
    }

    /// The skills there are now, as the ones the profile knows.
    fn remember_skills(&mut self) {
        let names = self.skill_names();
        self.save_settings(|s| s.profile_skills = Some(names));
    }

    pub fn profile_dirty(&self) -> bool {
        self.profile_text != self.profile_loaded
    }

    /// Follows profile.json on disk (the setup session writes it, an editor
    /// may): reloaded and put in use when it changed and nothing is typed.
    pub(super) fn follow_profile(&mut self) {
        let Some(file) = self.profile_file() else {
            self.profile_text.clear();
            self.profile_loaded.clear();
            return;
        };
        if !self.profile_seen.changed(&file, Duration::from_secs(2)) || self.profile_dirty() {
            return;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        self.profile_text = text.clone();
        self.profile_loaded = text;
        if file.is_file() {
            match crate::profile::load(&self.settings.profile) {
                Ok(profile) => {
                    crate::profile::set(profile);
                    self.profile_error = None;
                    // The settings it brings (releases: KTLO's repos) in
                    // use at once: settings read again, their repos asked now.
                    self.settings_seen.reset();
                    self.dependabot_at = None;
                    self.releases_at = None;
                }
                Err(error) => self.profile_error = Some(format!("{error:#}")),
            }
        }
    }

    /// Whether a bot & PRs field is typed differently from what is in use.
    pub fn bot_setup_dirty(&self) -> bool {
        let profile = crate::profile::get();
        self.bot_drafts.iter().any(|(path, text)| {
            let now = crate::profile::EDIT_FIELDS
                .iter()
                .find(|f| f.path == path)
                .map_or_else(|| self.settings.test_app_url.clone(), |f| f.text(&profile));
            text.trim() != now.trim()
        })
    }

    /// Save in Settings' bot & PRs: the profile fields go to the edits
    /// file (only what differs from the profile), the test app to
    /// settings.json; both in use at once.
    pub(super) fn save_bot_setup(&mut self) {
        let base = match crate::profile::load_base(&self.settings.profile) {
            Ok(base) => base,
            Err(error) => {
                self.bot_save_error = Some(format!("not saved: {error:#}"));
                return;
            }
        };
        let file = crate::profile::edits_file();
        let old = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let edits = crate::profile::edited(old, &base, &self.bot_drafts);
        if let Err(error) = crate::store::save_at(&file, &edits) {
            self.bot_save_error = Some(format!("could not save: {error}"));
            return;
        }
        if let Some(url) = self.bot_drafts.get("testAppUrl").cloned() {
            self.save_settings(|s| s.test_app_url = url.trim().to_owned());
        }
        match crate::profile::load(&self.settings.profile) {
            Ok(profile) => crate::profile::set(profile),
            Err(error) => {
                self.bot_save_error = Some(format!("{error:#}"));
                return;
            }
        }
        self.bot_drafts.clear();
        self.bot_save_error = None;
    }

    /// ⌘S in the profile editor: checked as a profile, written, in use.
    pub(super) fn save_profile_text(&mut self) {
        let Some(file) = self.profile_file() else {
            return;
        };
        if let Err(error) = serde_json::from_str::<crate::profile::Profile>(&self.profile_text) {
            self.profile_save_error = Some(format!("not saved: {error}"));
            return;
        }
        if let Err(error) = std::fs::write(&file, &self.profile_text) {
            self.profile_save_error = Some(format!("could not save: {error}"));
            return;
        }
        self.profile_save_error = None;
        self.profile_loaded = self.profile_text.clone();
        self.profile_seen.reset();
        self.follow_profile();
    }

    /// Puts a profile in use and remembers it in settings.json. The
    /// settings it brings stay defaults under the file (store::load_settings),
    /// so a later version of the profile reaches the user too.
    pub(super) fn use_profile(
        &mut self,
        source: String,
        profile: crate::profile::Profile,
        ctx: &egui::Context,
    ) {
        crate::profile::set(profile);
        self.profile_error = None;
        self.profile_input.clear();
        // Only the choice changes; the rest of the file stays as it was.
        let mut file: serde_json::Value = store::load(FILE);
        file["profile"] = source.into();
        if let Err(error) = store::save(FILE, &file) {
            self.settings_error = Some(format!("could not save settings: {error}"));
        }
        let settings = store::load_settings();
        self.use_settings(&settings, ctx);
        self.settings_seen.reset();
        self.show_settings(settings);
        self.apply(super::Action::Refresh, ctx);
    }

    /// Puts the settings to work: theme at once, teams with a reload.
    fn use_settings(&mut self, settings: &Settings, ctx: &egui::Context) {
        if settings.profile != self.settings.profile {
            match crate::profile::load(&settings.profile) {
                Ok(profile) => {
                    crate::profile::set(profile);
                    self.profile_error = None;
                }
                Err(error) => self.profile_error = Some(format!("{error:#}")),
            }
        }
        self.theme = settings.theme;
        crate::theme::follow(ctx, settings.theme);
        if settings.teams != self.settings.teams && !settings.teams.is_empty() {
            self.teams = settings.teams.clone();
            self.apply(super::Action::Refresh, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::store::Watch;
    use std::time::Duration;

    #[test]
    fn onboarding_waits_for_the_profile_to_be_saved() {
        let dir = std::env::temp_dir().join(format!("orbit-onboard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("profile.json");
        // No profile yet when the session starts (start_onboarding).
        let mut watch = Watch::default();
        watch.changed(&file, Duration::ZERO);
        assert!(!watch.changed(&file, Duration::ZERO));
        // The session moves its file into place: that ends the onboarding.
        std::fs::write(&file, "{}").unwrap();
        assert!(watch.changed(&file, Duration::ZERO));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
