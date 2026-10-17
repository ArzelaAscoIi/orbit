//! Before the board: sign in with Linear (or a key), and which teams to watch.

use super::*;

pub(super) fn sign_in(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let checking = app.checking();
    let error = app.sign_in_error.clone();
    let waiting = app.waiting_for_browser();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(p().bg))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.24);
                card_frame().inner_margin(28).show(ui, |ui| {
                    ui.set_width(400.0);
                    ui.label(RichText::new(crate::NAME).size(26.0).family(medium()));
                    ui.label(
                        RichText::new("Follow what the bot is doing, and what it needs from you.")
                            .color(p().text_2),
                    );
                    ui.add_space(18.0);
                    if checking {
                        centered_row(ui, |ui| {
                            ui.spinner();
                            ui.label(RichText::new("Checking with Linear…").color(p().text_2));
                        });
                        return;
                    }
                    if waiting {
                        centered_row(ui, |ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("Finish signing in in your browser…")
                                    .color(p().text_2),
                            );
                        });
                    } else if crate::oauth::client_id().is_some() {
                        if primary(ui, "Sign in with Linear").clicked() {
                            actions.push(Action::SignInWithLinear);
                        }
                    } else {
                        key_sign_in(app, ui, actions);
                    }
                    if let Some(error) = &error {
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(RichText::new(error).small().color(p().danger)).wrap(),
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Saved in your Keychain, so you only do this once.")
                            .small()
                            .color(p().text_3),
                    );
                    ui.add_space(12.0);
                    egui::CollapsingHeader::new(
                        RichText::new("Other options").small().color(p().text_2),
                    )
                    .show(ui, |ui| {
                        if crate::oauth::client_id().is_some() {
                            key_sign_in(app, ui, actions);
                            ui.add_space(10.0);
                        }
                        // A company profile can bring its own Linear sign-in.
                        ui.label(RichText::new("Use a company profile").small());
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut app.profile_input)
                                    .hint_text("a folder or a .json file")
                                    .desired_width(260.0),
                            );
                            if ui.button("Import").clicked() {
                                actions.push(Action::ImportProfile);
                            }
                        });
                        if let Some(error) = &app.profile_error {
                            ui.label(label(error).color(p().danger));
                        }
                    });
                });
            });
        });
}

/// Which Linear teams to watch: on first sign-in, and from the sidebar.
/// One row per team; click a row to pick it.
pub(super) fn team_picker(
    teams: &[(String, String)],
    picked: &[String],
    can_cancel: bool,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(p().bg))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.12).max(24.0));
                card_frame().show(ui, |ui| {
                    ui.set_width(380.0);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.label(
                            RichText::new("Which teams should it watch?")
                                .size(19.0)
                                .family(medium())
                                .color(p().text),
                        );
                        ui.label(
                            RichText::new("Their open tickets, plus anything assigned to you.")
                                .color(p().text_2),
                        );
                        ui.add_space(14.0);
                        if teams.is_empty() {
                            ui.spinner();
                        }
                        let mut sorted: Vec<&(String, String)> = teams.iter().collect();
                        sorted.sort();
                        egui::ScrollArea::vertical()
                            .max_height(360.0)
                            .show(ui, |ui| {
                                for (key, name) in sorted {
                                    let on = picked.contains(key);
                                    if team_row(ui, key, name, on).clicked() {
                                        actions.push(Action::ToggleTeam(key.clone()));
                                    }
                                }
                            });
                        ui.add_space(14.0);
                        let label = match picked.len() {
                            0 => "Pick at least one team".to_owned(),
                            1 => "Watch 1 team".to_owned(),
                            n => format!("Watch {n} teams"),
                        };
                        // Nothing picked: a plain muted button, not an alarm.
                        let ready = !picked.is_empty();
                        let (fill, text) = if ready {
                            (p().btn, p().on_btn)
                        } else {
                            (p().control, p().text_3)
                        };
                        let save = ui.add_enabled(
                            ready,
                            egui::Button::new(RichText::new(label).family(medium()).color(text))
                                .fill(fill)
                                .stroke(Stroke::NONE)
                                .min_size(egui::vec2(ui.available_width(), 34.0)),
                        );
                        if save.clicked() {
                            actions.push(Action::SaveTeams);
                        }
                        if can_cancel {
                            ui.add_space(4.0);
                            ui.vertical_centered(|ui| {
                                if link(ui, RichText::new("Cancel").small().color(p().text_3))
                                    .clicked()
                                {
                                    // Reopen with the saved teams and save them again.
                                    actions.push(Action::PickTeams);
                                    actions.push(Action::SaveTeams);
                                }
                            });
                        }
                    });
                });
            });
        });
}

fn team_row(ui: &mut Ui, key: &str, name: &str, on: bool) -> egui::Response {
    let color = if on { p().text } else { p().text_2 };
    row(ui, false, p().signal, true, |ui| {
        ui.horizontal(|ui| {
            check(ui, on);
            ui.add_sized([44.0, 18.0], egui::Label::new(label(key).color(color)));
            ui.label(RichText::new(name).color(color));
        });
    })
}

/// A Linear API key: the way in without an OAuth app.
fn key_sign_in(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.label(RichText::new("Paste a Linear API key").small());
    let entered = secret_field(ui, &mut app.key_input, "lin_api_…", f32::INFINITY);
    ui.add_space(6.0);
    if ui.button("Sign in with key").clicked() || entered {
        actions.push(Action::SignIn);
    }
    ui.add_space(6.0);
    let help = RichText::new("Create one in Linear: Settings > Security & access ↗")
        .small()
        .underline()
        .color(p().text);
    if link(ui, help).clicked() {
        actions.push(Action::Open(
            "https://linear.app/settings/account/security".into(),
        ));
    }
}

/// A spinner over its line, both centred like the title above them.
fn centered_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        add(ui);
    });
}
