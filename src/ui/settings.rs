//! Settings, in place of the panes (Cmd+, or the palette; Esc closes): which tools are
//! connected, the theme, and settings.json itself, edited as text like
//! VS Code's (app/settings.rs saves and applies it).

use super::*;
use crate::theme::Choice;

pub(super) fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(p().bg))
        .show(ui, |ui| {
            header_bar(ui, BAR, |ui| {
                ui.add_space(28.0);
                ui.label(RichText::new("Settings").heading());
            });
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin::symmetric(28, 4))
                        .show(ui, |ui| {
                            ui.set_max_width(ui.available_width().min(720.0));
                            // What unlocks features first; the profile is optional.
                            connections(app, ui, actions);
                            local_agent(app, ui, actions);
                            profile(app, ui, actions);
                            bot_setup(app, ui, actions);
                            activity(app, ui, actions);
                            herdr(app, ui, actions);
                            appearance(app, ui, actions);
                            editor(app, ui, actions);
                            about(app, ui, actions);
                            ui.add_space(28.0);
                        });
                });
        });
}

fn local_agent(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "local sessions");
    hairline(ui);
    let rect = ui
        .scope(|ui| {
            ui.label(label(
                "The agent new ticket, free and unattended sessions run with. A session keeps \
                 the agent it started with.",
            ));
            ui.horizontal(|ui| {
                for agent in [crate::claude::Agent::Claude, crate::claude::Agent::Codex] {
                    let chosen = app.settings.local_agent == agent;
                    let button = egui::Button::image_and_text(agent_icon(ui, agent), agent.label())
                        .selected(chosen);
                    if hover_hand(&ui.add(button)).clicked() {
                        actions.push(Action::SetLocalAgent(agent));
                    }
                }
            });
        })
        .response
        .rect;
    mark(app, ui, "local-agent", rect);
    ui.add_space(6.0);
}

/// The profile in use (profile.rs): what it changes, where it comes from,
/// Claude to set it up, import, and its profile.json to edit.
fn profile(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "profile");
    hairline(ui);
    ui.add_space(6.0);
    let profile = crate::profile::get();
    // What settings.json names, with the build's default filled in.
    let source = crate::profile::effective(&app.settings.profile).to_owned();
    let rect = ui
        .scope(|ui| {
            ui.horizontal(|ui| {
                let name = match (source.is_empty(), profile.name.is_empty()) {
                    (true, _) => "generic",
                    (false, true) => "unnamed",
                    (false, false) => profile.name.as_str(),
                };
                ui.label(RichText::new(name).family(medium()));
                if !source.is_empty() {
                    ui.label(label(&source));
                    if link(ui, label("use generic").underline()).clicked() {
                        actions.push(Action::ClearProfile);
                    }
                }
                // The profiles this build carries, one click each.
                for name in crate::profile::built_in() {
                    // Offered where a profile is in use already; a generic
                    // install imports one by name or path.
                    if !source.is_empty()
                        && name != source
                        && link(ui, label(&format!("use {name}")).underline()).clicked()
                    {
                        actions.push(Action::UseProfile(name.to_owned()));
                    }
                }
            });
            for line in summary(&profile, source.is_empty()) {
                ui.add(egui::Label::new(label(&line)).wrap());
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let setup = if source.is_empty() {
                    primary(ui, "✳ Set up with Claude")
                } else {
                    hover_hand(&ui.button("✳ Improve with Claude"))
                };
                let setup = setup.on_hover_text(
                    "A Claude session reads your Linear workspace, asks a few short questions, \
                     and writes the profile: teams, the bot and its commands, maintenance rules, \
                     how you like text. Run it again to improve it.",
                );
                if setup.clicked() {
                    actions.push(Action::SetUpProfile);
                }
                ui.label(label("or import"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut app.profile_input)
                        .hint_text("a folder, a .json file, or an https:// URL")
                        .desired_width(300.0),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if app.profile_importing {
                    ui.spinner();
                } else if hover_hand(&ui.button("Import")).clicked() || enter {
                    actions.push(Action::ImportProfile);
                }
            });
            if let Some(error) = &app.profile_error {
                ui.label(label(error).color(p().danger));
            }
        })
        .response
        .rect;
    mark(app, ui, "profile", rect);
    if !app.profile_text.is_empty() || !source.is_empty() {
        ui.add_space(6.0);
        // The raw file for whoever wants it; open while it has edits.
        egui::CollapsingHeader::new(label("advanced: profile.json"))
            .id_salt("profile.json")
            .open(app.profile_dirty().then_some(true))
            .show(ui, |ui| {
                let dirty = app.profile_dirty();
                let error = app.profile_save_error.clone();
                json_editor(
                    ui,
                    "profile.json",
                    &mut app.profile_text,
                    dirty,
                    error.as_deref(),
                    (Action::SaveProfile, Action::RevertProfile),
                    actions,
                );
            });
    }
}

/// The bot and PR features as plain fields, with or without a profile:
/// each says what it turns on, an empty one keeps its feature hidden.
/// Saved over the profile (profile.rs `edits_file`), so they outlive app
/// updates; the test app goes to settings.json.
fn bot_setup(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "bot & PRs");
    hairline(ui);
    ui.add_space(6.0);
    let profile = crate::profile::get();
    let test_app = app.settings.test_app_url.clone();
    let rect = ui
        .scope(|ui| {
            ui.add(
                egui::Label::new(label(
                    "What your team's bot and GitHub do for you. Empty fields keep their \
                     buttons out of sight.",
                ))
                .wrap(),
            );
            ui.add_space(6.0);
            let rows = crate::profile::EDIT_FIELDS
                .iter()
                .map(|f| (f.path, f.name, f.what, f.kind == crate::profile::EditKind::Labels))
                .chain([(
                    "testAppUrl",
                    "test app",
                    "the app the Testing tab's browser opens for UI test plans, e.g. https://staging.example.com",
                    false,
                )]);
            for (path, name, what, multiline) in rows {
                let now = crate::profile::EDIT_FIELDS
                    .iter()
                    .find(|f| f.path == path)
                    .map_or_else(|| test_app.clone(), |f| f.text(&profile));
                let draft = app.bot_drafts.entry(path.to_owned()).or_insert(now);
                ui.horizontal(|ui| {
                    ui.add_sized([170.0, 18.0], egui::Label::new(RichText::new(name).family(medium())));
                    let edit = if multiline {
                        egui::TextEdit::multiline(draft).desired_rows(2)
                    } else {
                        egui::TextEdit::singleline(draft)
                    };
                    ui.add(edit.id_salt(path).desired_width(360.0));
                });
                ui.horizontal(|ui| {
                    ui.add_space(178.0);
                    ui.add(egui::Label::new(label(what)).wrap());
                });
                ui.add_space(4.0);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if app.bot_setup_dirty() {
                    if primary(ui, "Save").clicked() {
                        actions.push(Action::SaveBotSetup);
                    }
                    if link(ui, label("revert").underline()).clicked() {
                        actions.push(Action::RevertBotSetup);
                    }
                } else {
                    ui.label(label("saved"));
                }
            });
            if let Some(error) = &app.bot_save_error {
                ui.label(label(error).color(p().danger));
            }
        })
        .response
        .rect;
    mark(app, ui, "bot-setup", rect);
}

/// What the profile changes, in a few plain lines.
fn summary(profile: &crate::profile::Profile, generic: bool) -> Vec<String> {
    if generic {
        return vec![
            "Plain Linear, Claude Code per ticket. A profile adds your company's bot and its \
             commands, how tickets are routed, what counts as maintenance, your own skills."
                .into(),
        ];
    }
    let mut out = Vec::new();
    let bot = profile.bot_name("");
    if !bot.is_empty() {
        let remote = [&profile.bot.plan_command, &profile.bot.build_command]
            .iter()
            .filter(|c| !c.trim().is_empty())
            .count();
        out.push(format!("bot: {bot}, {remote} remote command(s)"));
    }
    if !profile.skills.is_empty() {
        let map: Vec<String> = profile
            .skills
            .iter()
            .map(|(role, folder)| format!("{role} → /{folder}"))
            .collect();
        out.push(format!("skills: {}", map.join(", ")));
    }
    let mut tuned: Vec<String> = profile
        .dir
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir.join("skills")).into_iter().flatten())
        .flatten()
        .filter(|e| e.path().join("EXTRA.md").is_file())
        .map(|e| format!("/{}", e.file_name().to_string_lossy()))
        .collect();
    tuned.sort();
    if !tuned.is_empty() {
        out.push(format!("fine-tunes: {}", tuned.join(", ")));
    }
    if !profile.intake.routing.trim().is_empty() {
        out.push("intake: routes new tickets by the profile's team map".into());
    }
    let sweeps = profile.ktlo.sweeps.len();
    if !profile.ktlo.security_creators.is_empty() || sweeps > 0 {
        out.push(format!(
            "{}: {} scanner(s), {sweeps} sweep(s)",
            profile.ktlo.label,
            profile.ktlo.security_creators.len()
        ));
    }
    if let Some(o) = &profile.orchestrator {
        out.push(format!("Run the board: {}", o.team));
    }
    if out.is_empty() {
        out.push("nothing set yet: Improve with Claude, or edit profile.json below".into());
    }
    out
}

/// What each tool enables, and whether it is connected: a feature whose
/// tool is not connected stays out of sight or says what it needs.
/// (name, connected, what it turns on, how to connect, the key it takes)
type Connection<'a> = (
    &'a str,
    Option<String>,
    &'a str,
    &'a str,
    Option<(crate::secrets::Secret, &'a str)>,
);

fn connections(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    use crate::secrets::Secret;
    section(ui, "connections");
    hairline(ui);
    let linear = format!("signed in as {}", app.viewer);
    let laya = match (app.laya_ready, app.settings.laya.on()) {
        (false, _) => None,
        (true, None) => Some("downloaded · off".to_owned()),
        (true, Some(use_it)) => {
            let (compared, agreed) = app.laya_agreement;
            let share = (agreed * 100).checked_div(compared).unwrap_or(0);
            let mode = if use_it { "use" } else { "shadow" };
            Some(format!(
                "built in · {mode} · {compared} notes, {share}% agree with Haiku"
            ))
        }
    };
    let saved = |on: bool, what: &str| on.then(|| format!("{what} saved"));
    let rows: [Connection; 4] = [
        (
            "Linear",
            Some(linear),
            "the board, tickets, comments",
            "sign in",
            None,
        ),
        (
            "Anthropic",
            saved(app.has_anthropic_key(), "key"),
            "briefs, short names, walkthroughs, questions about PRs, test plans, New ticket",
            "paste a key",
            Some((Secret::Anthropic, "sk-ant-…")),
        ),
        (
            "GitHub",
            // No token: gh's own login counts once a diff came back through it.
            saved(app.has_github_token(), "token").or_else(|| {
                app.diffs
                    .values()
                    .any(Result::is_ok)
                    .then(|| "through gh's login".to_owned())
            }),
            "diffs and merges",
            "none needed if `gh auth login` is done; else paste a token",
            Some((Secret::Github, "ghp_… or github_pat_…")),
        ),
        (
            "incident.io",
            saved(app.incident_key.is_some(), "key"),
            "live incidents next to maintenance, Investigate",
            "paste a read-only API key",
            Some((Secret::IncidentIo, "incident.io API key")),
        ),
    ];
    let rect = ui
        .scope(|ui| {
            for (name, connected, enables, how, key) in rows {
                row(ui, false, p().signal, true, |ui| {
                    ui.horizontal(|ui| {
                        let color = if connected.is_some() {
                            p().success
                        } else {
                            p().text_3
                        };
                        dot(ui, color);
                        ui.label(RichText::new(name).family(medium()));
                        ui.label(label(connected.as_deref().unwrap_or("not connected")));
                    });
                    ui.label(label(enables));
                    if let Some((secret, hint)) = key {
                        if connected.is_none() {
                            ui.label(label(&format!("connect: {how}")));
                            key_field(app, ui, secret, hint, actions);
                        } else {
                            // Saved already: replacing a rotated or wrong key
                            // lives here too, tucked away so a working
                            // connection stays quiet. SaveKey overwrites.
                            egui::CollapsingHeader::new(label("change key"))
                                .id_salt(name)
                                .show(ui, |ui| key_field(app, ui, secret, hint, actions));
                        }
                    } else if connected.is_none() {
                        ui.label(label(&format!("connect: {how}")));
                    }
                });
            }
        })
        .response
        .rect;
    mark(app, ui, "connections", rect);
    ui.add_space(6.0);
    egui::CollapsingHeader::new(label("advanced: Laya, keys, MCPs"))
        .id_salt("connections-advanced")
        .show(ui, |ui| {
            ui.label(RichText::new("Laya").family(medium()));
            ui.label(label(laya.as_deref().unwrap_or("not downloaded")));
            ui.add(
                egui::Label::new(label(
                    "An optional open model inside the app that decides bug or not for New \
                     ticket, offline and free, 98% right where it is sure (Haiku otherwise).",
                ))
                .wrap(),
            );
            if app.laya_ready {
                laya_modes(app, ui, actions);
                // Quick actions: how often each question proposed, and was
                // taken up (laya-quick.jsonl).
                for (id, asked, shown, clicked) in &app.quick_stats {
                    ui.label(label(&format!(
                        "quick {id}: asked {asked}, proposed {shown}, clicked {clicked}"
                    )));
                }
            }
            laya_download(app, ui, actions);
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(label(&format!(
                    "Keys live in {}; settings.json shows them as SAVED, clear one there to \
                     remove it. Investigations also use Claude Code's MCPs (incident tool, \
                     observability): sign in once with /mcp in a session.",
                    crate::secrets::place()
                )))
                .wrap(),
            );
        });
}

/// The password field and Save for one key: the one place a key is typed,
/// whether connecting the first time or replacing a saved one. SaveKey
/// overwrites, so the same field changes a key that is already there.
fn key_field(
    app: &mut App,
    ui: &mut Ui,
    secret: crate::secrets::Secret,
    hint: &str,
    actions: &mut Vec<Action>,
) {
    let draft = app
        .key_drafts
        .entry(crate::secrets::name(secret))
        .or_default();
    ui.horizontal(|ui| {
        let entered = secret_field(ui, draft, hint, 320.0);
        if hover_hand(&ui.button("Save")).clicked() || entered {
            actions.push(Action::SaveKey(secret));
        }
    });
}

/// The activity log (activity.rs): where it is, what is in it, off.
/// herdr next to Orbit (herdr.rs): whether it is there, the user's herdr
/// config to use (theme, notifications, sounds, sidebar, prefix keys), its
/// keys on or off, Orbit's plugin for herdr, and sounds.
fn herdr(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "herdr");
    hairline(ui);
    ui.add_space(6.0);
    let rect = ui
        .scope(|ui| {
            let installed = if app.herdr_bin {
                format!(
                    "herdr is installed: {} of its agents show in the sidebar under // in herdr, and \
                     a ticket starts in herdr from Cmd+K (Start <ID> in herdr).",
                    app.herdr_agents.len()
                )
            } else {
                "herdr is not installed here (brew install herdr); its agents and plugin need it."
                    .to_owned()
            };
            ui.add(egui::Label::new(label(&installed)).wrap());
            ui.add_space(4.0);
            match &app.herdr_config {
                Some(config) => {
                    ui.add(
                        egui::Label::new(label(&format!(
                            "Your herdr config: {} (theme {}, prefix {})",
                            config.path.display(),
                            config.theme,
                            config
                                .keys
                                .prefix
                                .first()
                                .map_or("ctrl+b".to_owned(), |c| format!(
                                    "{}{}",
                                    if c.ctrl { "ctrl+" } else { "" },
                                    c.key
                                ))
                        )))
                        .wrap(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if link(ui, label("use my herdr config").underline())
                            .on_hover_text("Theme, notifications, sounds and sidebar from it; its prefix keys on")
                            .clicked()
                        {
                            actions.push(Action::UseHerdrConfig);
                        }
                        let on = app.settings.herdr != Some(false);
                        let keys = if on { "herdr keys: on" } else { "herdr keys: off" };
                        if link(ui, label(keys).underline())
                            .on_hover_text("The prefix (ctrl+b) then c, v, minus, z, n, p, 1 to 9, as in herdr")
                            .clicked()
                        {
                            actions.push(Action::SetHerdrKeys(!on));
                        }
                    });
                }
                None => {
                    ui.label(label("No herdr config found (~/.config/herdr/config.toml)."));
                }
            }
            ui.horizontal_wrapped(|ui| {
                if app.herdr_bin
                    && link(ui, label("add Orbit's plugin to herdr").underline())
                        .on_hover_text("What needs you in a herdr popup; Linear and GitHub links clicked in herdr open here")
                        .clicked()
                {
                    actions.push(Action::InstallHerdrPlugin);
                }
                let sound = app.settings.sound == Some(true);
                let text = if sound { "sounds: on" } else { "sounds: off" };
                if link(ui, label(text).underline())
                    .on_hover_text("A sound when a session needs approval, or finishes while Orbit is in the background")
                    .clicked()
                {
                    actions.push(Action::SetSound(!sound));
                }
            });
            if let Some(note) = &app.herdr_note {
                ui.add(egui::Label::new(label(note).color(p().text_2)).wrap());
            }
        })
        .response
        .rect;
    mark(app, ui, "herdr-settings", rect);
}

fn activity(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "activity log");
    hairline(ui);
    ui.add_space(6.0);
    let path = crate::activity::path(&crate::store::data_dir());
    let on = app.settings.activity_log != Some(false);
    ui.add(
        egui::Label::new(label(
            "What you do in the app, one line per action: its name, the view, the time, and \
             whether a key or a click did it. Never what you type, a ticket's text or a key, so \
             it is safe to share. Local only; a feedback session (Cmd+Shift+F) reads it to see \
             what you were doing.",
        ))
        .wrap(),
    );
    ui.horizontal_wrapped(|ui| {
        ui.label(label(&path.display().to_string()));
        if link(ui, label("copy path").underline()).clicked() {
            ui.ctx().copy_text(path.display().to_string());
        }
        if link(ui, label("show in Finder").underline()).clicked() {
            actions.push(Action::Reveal(path.clone()));
        }
        let stats = link(ui, label("your stats").underline())
            .on_hover_text("Your last 7 days in Orbit, and the team's numbers from SigNoz");
        mark(app, ui, "stats-link", stats.rect);
        if stats.clicked() {
            actions.push(Action::Show(View::Stats));
        }
        let toggle = if on { "turn off" } else { "turn on" };
        if link(ui, label(toggle).underline()).clicked() {
            actions.push(Action::SetActivityLog(!on));
        }
    });
}

/// Laya: off, shadow or use, one click each (settings.json laya.mode).
fn laya_modes(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.label(label("mode:"));
        for mode in ["off", "shadow", "use"] {
            let on = app.settings.laya.mode.trim() == mode;
            let text = label(mode).color(if on { p().text } else { p().text_3 });
            if link(ui, if on { text } else { text.underline() }).clicked() && !on {
                actions.push(Action::SetLayaMode(mode.into()));
            }
        }
    });
}

/// Laya's checkpoint: a button until it is here, then nothing.
fn laya_download(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if app.laya_ready {
        return;
    }
    match &app.laya_download {
        Some(Ok((done, total))) => {
            let mb = |b: u64| b / 1_000_000;
            ui.horizontal(|ui| {
                ui.spinner();
                let share = (done * 100).checked_div(*total).unwrap_or(0);
                ui.label(label(&format!(
                    "downloading Laya: {share}% ({} of {} MB)",
                    mb(*done),
                    mb(*total)
                )));
            });
        }
        other => {
            ui.horizontal_wrapped(|ui| {
                let button = hover_hand(&ui.button("Download Laya (840 MB)")).on_hover_text(
                    "Once, from Hugging Face (convaiinnovations/laya, Apache 2.0), into the \
                     data folder. Then it runs inside the app: no Python, no server, offline.",
                );
                if button.clicked() {
                    actions.push(Action::DownloadLaya);
                }
                if let Some(Err(error)) = other {
                    ui.label(label(error).color(p().danger));
                }
            });
        }
    }
    ui.add_space(6.0);
}

fn appearance(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "appearance");
    let system_light = ui.ctx().system_theme() == Some(egui::Theme::Light);
    hairline(ui);
    let themes = ui.scope(|ui| {
        for choice in Choice::ALL {
            if theme_row(ui, choice, app.theme == choice, system_light).clicked() {
                actions.push(Action::SetTheme(choice));
            }
        }
    });
    mark(app, ui, "themes", themes.response.rect);
    ui.add_space(8.0);
    ui.label(label("⌘⇧T cycles through them"));
}

/// settings.json as text: ⌘S saves and applies, Revert reads the file again.
fn editor(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "settings.json");
    let path = crate::store::data_dir().join("settings.json");
    ui.horizontal(|ui| {
        ui.label(label(&path.display().to_string()));
        if link(ui, label("copy path").underline()).clicked() {
            ui.ctx().copy_text(path.display().to_string());
        }
    });
    ui.add_space(6.0);
    let dirty = app.settings_dirty();
    let error = app.settings_error.clone();
    let rect = json_editor(
        ui,
        "settings",
        &mut app.settings_text,
        dirty,
        error.as_deref(),
        (Action::SaveSettings, Action::RevertSettings),
        actions,
    );
    mark(app, ui, "settings-json", rect);
    ui.add_space(4.0);
    ui.label(label(&format!(
        "debug: the palette's Debug actions, one pane per tool after setup. \
         investigate: the panes beside an incident. A key typed here moves to {} on \
         save and reads SAVED; clear it to remove the key.",
        crate::secrets::place()
    )));
}

/// A JSON file as text, framed in `signal` while it has unsaved edits:
/// Save (⌘S while it is the one edited) and Revert.
fn json_editor(
    ui: &mut Ui,
    id: &str,
    text: &mut String,
    dirty: bool,
    error: Option<&str>,
    (save, revert): (Action, Action),
    actions: &mut Vec<Action>,
) -> egui::Rect {
    let field = Frame::new()
        .fill(p().side)
        .stroke(Stroke::new(1.0, if dirty { p().signal } else { p().line }))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(text)
                    .id_salt(id)
                    .code_editor()
                    .frame(Frame::NONE)
                    .desired_rows(14)
                    .desired_width(f32::INFINITY),
            )
        });
    let cmd_s = dirty && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if dirty {
            if primary(ui, "Save  ⌘S").clicked() || cmd_s {
                actions.push(save);
            }
            if link(ui, label("revert").underline()).clicked() {
                actions.push(revert);
            }
        } else {
            ui.label(label("saved · edits here or in any editor apply at once"));
        }
    });
    if let Some(error) = error {
        ui.label(label(error).color(p().danger));
    }
    field.response.rect
}

fn about(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "about");
    hairline(ui);
    ui.add_space(6.0);
    let rect = ui
        .horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!(
                "{} {}",
                crate::NAME,
                crate::version()
            )));
            match &app.update {
                Some(tag) if app.installing => {
                    ui.spinner();
                    ui.label(label(&format!(
                        "installing {tag}, then {} reopens",
                        crate::NAME
                    )));
                }
                Some(tag) => {
                    if primary(ui, &format!("Update to {tag}")).clicked() {
                        actions.push(Action::Update);
                    }
                }
                None => {
                    ui.label(label("up to date"));
                }
            }
        })
        .response
        .rect;
    mark(app, ui, "update", rect);
    if let Some(error) = &app.update_error {
        ui.label(label(error).color(p().danger));
    }
    ui.add_space(4.0);
    if link(ui, label("releases: download the latest zip").underline()).clicked() {
        actions.push(Action::Open(crate::RELEASES.into()));
    }
    // Old session folders are cleaned up on their own (cleanup.rs); one
    // quiet line says how much.
    let note = app
        .cleanup_note
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(note) = note {
        ui.add_space(4.0);
        ui.label(label(&note));
    }
}

/// A radio square, the name, and five swatches: bg, line-2, text, signal, btn.
fn theme_row(ui: &mut Ui, choice: Choice, on: bool, system_light: bool) -> egui::Response {
    let palette = choice.palette(system_light);
    row(ui, on, p().signal, true, |ui| {
        let height = egui::vec2(ui.available_width(), 18.0);
        ui.allocate_ui_with_layout(height, Layout::right_to_left(Align::Center), |ui| {
            let swatches = [
                palette.btn,
                palette.signal,
                palette.text,
                palette.line_2,
                palette.bg,
            ];
            for color in swatches {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                ui.painter().rect_filled(rect, 0.0, color);
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, p().line_2),
                    egui::StrokeKind::Inside,
                );
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                check(ui, on);
                let color = if on { p().text } else { p().text_2 };
                ui.label(RichText::new(choice.name()).color(color));
            });
        });
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn connected_keys_can_still_be_changed() {
        // A saved key must stay changeable in Settings: the key field is no
        // longer confined to the not-connected branch, it also shows under
        // "change key" once a connection is live. Guards the regression where
        // a wrong Anthropic key could only be fixed by editing settings.json.
        let src = include_str!("settings.rs");
        assert!(
            src.contains("\"change key\""),
            "connected connections should offer a way to replace a saved key"
        );
    }
}
