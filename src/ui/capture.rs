//! The New ticket window (Cmd+T, src/capture.rs): the note, then what
//! Haiku read with its few questions, then the created ticket. The text
//! fields borrow the window's strings directly; everything else is an
//! Action.

use super::*;

/// Shown while Laya is off: what it would add.
const LAYA_TIP: &str = "tip: Laya, an open model inside the app, can decide bug or not \
offline and free, 98% right where it is sure. Try it in Settings";

pub(super) fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let laya_off = app.settings.laya.on().is_none();
    let keyed = app.has_anthropic_key();
    let teams = app.teams.clone();
    let default = app.default_team();
    let Some(capture) = app.capture.as_mut() else {
        return;
    };
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("New ticket")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
        .frame(
            Frame::new()
                .fill(p().bg)
                .stroke(Stroke::new(1.0, p().signal))
                .inner_margin(PAD),
        )
        .show(&ctx, |ui| {
            ui.set_width(540.0);
            // Long notes grow the fields; scroll inside the screen so the buttons stay reachable.
            let max = ctx.content_rect().height() - 80.0 - 2.0 * f32::from(PAD) - 24.0;
            egui::ScrollArea::vertical().max_height(max.max(200.0)).show(ui, |ui| {
            if let Some((identifier, url)) = &capture.created {
                let (kicker, line) = if capture.added {
                    ("▲ added to an existing ticket", format!("Your report is on {identifier}"))
                } else {
                    ("▲ created, triaged", format!("{identifier} is in Backlog"))
                };
                ui.label(label(kicker).color(p().signal));
                ui.add_space(6.0);
                ui.label(RichText::new(line).size(16.0).family(semibold()));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if primary(ui, "Open in Linear ↗").clicked() {
                        actions.push(Action::Open(url.clone()));
                    }
                    if ui.button("Another one").clicked() {
                        actions.push(Action::CloseCapture);
                        actions.push(Action::NewTicket);
                    }
                });
                return;
            }
            match capture.intake.as_mut() {
                None => note(capture, keyed, (&teams, &default), ui, actions),
                Some(intake) => {
                    let busy = capture.busy;
                    ui.label(label(&format!(
                        "// {} · {} · {} · {}",
                        intake.kind, intake.team, intake.area, intake.priority
                    )));
                    if !intake.by_laya.is_empty() {
                        ui.horizontal(|ui| {
                            laya_chip(
                                ui,
                                "Laya, the model inside the app, was sure and disagreed with \
                                 Haiku, so its answer stands",
                            );
                            ui.label(label(&format!("decided: {}", intake.by_laya.join(", "))));
                        });
                    }
                    if laya_off
                        && link(ui, label(LAYA_TIP))
                            .on_hover_text(
                                "Laya is an open-weight decision model: it answers typed \
                                 questions in one pass instead of writing text. Tested on 385 \
                                 triaged tickets: 96% right on bug or not (Haiku stays for \
                                 everything else). Opens Settings.",
                            )
                            .clicked()
                    {
                        actions.push(Action::Show(View::Settings));
                    }
                    if let Some(error) = &intake.laya_error {
                        ui.horizontal(|ui| {
                            laya_chip(ui, "Laya could not answer; Haiku's answers stand");
                            ui.label(label(error)).on_hover_text(
                                "Haiku's answers stand. Download Laya in Settings, connections.",
                            );
                        });
                    }
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut intake.title).desired_width(f32::INFINITY),
                    );
                    // Filed already? Then the report goes there, not into a new ticket.
                    if !capture.duplicates.is_empty() {
                        ui.add_space(10.0);
                        ui.label(label("// possibly filed already").color(p().signal));
                        for (i, d) in capture.duplicates.iter().enumerate() {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(label(&format!("{} · {} · {}", d.identifier, d.team, d.state)));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if !busy
                                        && ui
                                            .small_button("Add my report here")
                                            .on_hover_text("Comments your note on this ticket instead of filing a new one")
                                            .clicked()
                                    {
                                        actions.push(Action::AddToDuplicate(i));
                                    }
                                });
                            });
                            ui.add(egui::Label::new(RichText::new(&d.title).family(medium())).wrap());
                            ui.add(egui::Label::new(RichText::new(&d.evidence).color(p().text_2)).wrap());
                        }
                        ui.add_space(4.0);
                        hairline(ui);
                    }
                    if intake.obvious && capture.duplicates.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("It just makes sense, so it goes straight in, triaged.")
                                .color(p().text_2),
                        );
                    } else {
                        for (question, answer) in
                            intake.questions.iter().zip(capture.answers.iter_mut())
                        {
                            ui.add_space(8.0);
                            ui.add(egui::Label::new(RichText::new(question)).wrap());
                            ui.add(
                                egui::TextEdit::multiline(answer)
                                    .hint_text("optional")
                                    .desired_rows(1)
                                    .desired_width(f32::INFINITY),
                            );
                        }
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if busy {
                            ui.spinner();
                            ui.label(
                                RichText::new("Writing the ticket and filing it…")
                                    .color(p().text_2),
                            );
                        } else {
                            let file = primary(
                                ui,
                                if capture.duplicates.is_empty() {
                                    "✳ File a ticket"
                                } else {
                                    "✳ Not a duplicate: file a ticket"
                                },
                            );
                            if file.clicked() {
                                actions.push(Action::CreateTicket);
                            }
                            fix_now(ui, actions);
                        }
                    });
                }
            }
            if let Some(error) = &capture.error {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(error).small().color(p().danger)).wrap());
            }
            });
        });
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if !open || escape {
        actions.push(Action::CloseCapture);
    }
}

/// The first step: what is wrong or wanted, with any links.
fn note(
    capture: &mut crate::app::Capture,
    keyed: bool,
    (teams, default): (&[String], &str),
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    ui.label(label(
        "// what is wrong, or what is wanted? paste links too",
    ));
    // Said before anything is typed: without a key there is no reading.
    if !keyed {
        ui.horizontal_wrapped(|ui| {
            ui.label(label(
                "no Anthropic key: it is filed as written, first line as the title ·",
            ));
            if link(ui, label("add a key").underline()).clicked() {
                actions.push(Action::Show(View::Settings));
            }
        });
        // Where it lands: the default team, or any watched one.
        ui.horizontal(|ui| {
            ui.label(label("files into"));
            let shown = if capture.team.is_empty() {
                default
            } else {
                &capture.team
            };
            if teams.len() > 1 {
                egui::ComboBox::from_id_salt("capture-team")
                    .selected_text(shown.to_owned())
                    .show_ui(ui, |ui| {
                        for team in teams {
                            ui.selectable_value(&mut capture.team, team.clone(), team);
                        }
                    });
            } else {
                ui.label(label(shown).color(p().text));
            }
            ui.label(label("· Backlog"));
        });
    }
    ui.add_space(4.0);
    let field = ui.add(
        egui::TextEdit::multiline(&mut capture.note)
            .hint_text("e.g. the dashboard reloads when I switch tabs, https://…")
            .desired_rows(6)
            .desired_width(f32::INFINITY),
    );
    if !capture.busy && capture.note.is_empty() {
        field.request_focus();
    }
    let send =
        field.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if capture.busy {
            ui.spinner();
            ui.label(RichText::new("Haiku is reading it…").color(p().text_2));
        } else {
            let has_note = !capture.note.trim().is_empty();
            if keyed {
                if primary(ui, "✳ Next").clicked() || send {
                    actions.push(Action::ReadNote);
                }
            } else if (primary(ui, "File as-is").clicked() || send) && has_note {
                actions.push(Action::FileAsIs);
            }
            if has_note {
                fix_now(ui, actions);
            }
            if keyed {
                ui.label(label(
                    "⌘ enter · Haiku sorts it: bug or feature, team, priority",
                ));
                if has_note && link(ui, label("or file as-is").underline()).clicked() {
                    actions.push(Action::FileAsIs);
                }
            }
        }
    });
}

/// The other way out: no ticket, a local Claude session does it now, like
/// Work's Fix now.
fn fix_now(ui: &mut Ui, actions: &mut Vec<Action>) {
    let button = hover_hand(&ui.button("✳ Fix now")).on_hover_text(
        "No ticket: a local Claude session does it now, with the note (and its answers) as \
         the brief",
    );
    if button.clicked() {
        actions.push(Action::FixNote);
    }
}
