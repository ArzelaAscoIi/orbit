//! Claude Code sessions: the attached terminal, the prompts around it, and
//! the list of every session on the machine.

use super::*;
use crate::claude::codex::{self, Entry, Trouble};

/// The Claude tab for one session key: a ticket id, the triage key, or
/// `sid:<session id>` for a session the app did not start.
/// `busy`: the pane that already draws this session, so this one points
/// there instead of drawing the same terminal twice.
pub(super) fn pane(
    app: &mut App,
    key: &str,
    busy: Option<u64>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    if let Some(id) = busy {
        section(ui, app.claude.agent_of(key).name());
        ui.label(RichText::new("This session is open in a pane.").color(p().text_2));
        ui.add_space(8.0);
        if ui.button("Go to it").clicked() {
            actions.push(Action::FocusPane(id));
        }
        return;
    }
    if key.starts_with("sid:") {
        external(app, key, ui, actions);
        return;
    }
    let agent = app.claude.agent_of(key);
    // A Codex session once it exists; before that, the start below.
    if agent == crate::claude::Agent::Codex
        && (key.starts_with("cid:")
            || app.claude.saved.contains_key(key)
            || app.claude.starting.contains(key))
    {
        codex_pane(app, key, ui, actions);
        return;
    }
    let attached = app.claude.is_attached(key);
    let starting = app.claude.starting.contains(key);
    let live = app.claude.live_of(key).cloned();
    let saved = app.claude.saved.get(key).cloned();
    let triage = key == crate::claude::TRIAGE;
    let name = match (&saved, app.card(key)) {
        (Some(saved), _) => app.session_name(key, saved),
        (None, Some(card)) => format!("{}  {}", card.issue.identifier, card.issue.title),
        (None, None) => "Triage".to_owned(),
    };

    // Stop first, right to left, so the truncated folder gets what is left.
    let row = egui::vec2(ui.available_width(), 22.0);
    ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
        if live.is_some()
            && ui
                .small_button("Stop")
                .on_hover_text("Stops the background session. The conversation is kept and resumes when you open it again.")
                .clicked()
        {
            actions.push(Action::StopSession(key.to_owned()));
        }
        // The ticket's walkthrough on the left, this session on the right.
        if app.card(key).is_some() {
            let split = ui
                .small_button("Split")
                .on_hover_text("The walkthrough on the left, this session on the right");
            mark(app, ui, "split-claude", split.rect);
            if split.clicked() {
                actions.push(Action::SplitClaude);
            }
        }
        // The session pushed to the ticket's PR: back to the review.
        if let Some(pr) = app
            .card(key)
            .and_then(|c| c.prs.iter().find(|p| app.changed(&p.url).is_some()))
            && primary(ui, "Review the changes →")
                .on_hover_text("New commits on the PR: its diff, loaded again, with a fresh walkthrough")
                .clicked()
        {
            actions.push(Action::ReviewAgain(pr.url.clone()));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let status = match &live {
                _ if app.claude.stopping.contains(key) => "stopping…",
                Some(l) => l.label(),
                None => "stopped",
            };
            state(ui, status, status_color(status), p().text_2);
            ui.add(agent_icon(ui, agent));
            if let Some(saved) = &saved {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{} · {}", saved.short_id(), saved.cwd.display()))
                            .small()
                            .monospace()
                            .color(p().text_3),
                    )
                    .truncate(),
                );
            }
        });
    });
    // What the session says it is on (its own `report note`), and what the
    // app has to say about its start (fresh, no transcript): one line each.
    let summary = app
        .claude
        .status
        .get(key)
        .and_then(|reports| crate::claude::status::note(reports))
        .map(|r| r.text.clone());
    let notes = [summary, app.claude.notes.get(key).cloned()];
    let notes: Vec<String> = notes.into_iter().flatten().collect();
    if !notes.is_empty() {
        let rect = ui
            .horizontal_wrapped(|ui| {
                for note in &notes {
                    ui.add(egui::Label::new(label(note).color(p().text_2)).truncate());
                }
            })
            .response
            .rect;
        mark(app, ui, "session-note", rect);
    }
    ui.add_space(6.0);

    if attached {
        super::tutorial::mark(app, ui, "terminal", ui.available_rect_before_wrap());
        let terminal = app.claude.attached.get_mut(key).expect("attached");
        actions.extend(crate::claude::show(ui, terminal).map(Action::Link));
        return;
    }
    // Looking at a running session attaches to it; no button in between.
    if live.is_some()
        && saved.is_some()
        && !starting
        && app.claude.auto_attached.insert(key.to_owned())
    {
        actions.push(Action::Attach(key.to_owned()));
    }
    ui.add_space(ui.available_height() * 0.2);
    ui.vertical_centered(|ui| {
        card_frame().show(ui, |ui| {
            ui.set_width(460.0);
            if starting {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let text = format!("Starting {} in the background…", agent.short());
                    ui.label(RichText::new(text).size(16.0));
                });
                return;
            }
            if saved.is_some() && live.is_none() {
                ended(app, key, &name, ui, actions);
                return;
            }
            let (title, button) = match (&saved, triage) {
                (Some(_), _) => ("Running in the background".to_owned(), "Attach"),
                (None, true) => (format!("Triage with {}", agent.short()), "Start triage"),
                (None, false) => (
                    format!("Work on it with {}", agent.short()),
                    "Start session",
                ),
            };
            ui.label(RichText::new(title).size(18.0).family(medium()));
            ui.add(egui::Label::new(RichText::new(&name).color(p().text)).truncate());
            ui.add_space(4.0);
            let text = if triage {
                format!(
                    "Runs the triage skill in its own {} session: clusters the Triage \
                     column, folds duplicates into one ticket, and proposes what to accept into \
                     Todo, park, cancel or question. Nothing is written until you approve.",
                    agent.label()
                )
            } else {
                format!(
                    "Runs your own {} in the background, with your settings, MCP servers \
                     and skills, in this ticket's folder. It keeps running when you switch \
                     tickets or quit the app, and picks up where it left off.",
                    agent.label()
                )
            };
            ui.add(egui::Label::new(RichText::new(text).color(p().text_2)).wrap());
            ui.add_space(12.0);
            let default = app.claude.default_agent;
            centered_row(ui, key, |ui| {
                if saved.is_some() {
                    if agent_primary(ui, agent, button).clicked() {
                        actions.push(Action::Attach(key.to_owned()));
                    }
                } else {
                    let start = |_| Action::Attach(key.to_owned());
                    actions.extend(agent_choice(ui, agent, default, button, true, start));
                }
            });
        });
    });
}

/// One step of a Codex conversation: what was asked (quiet), the answer
/// (markdown), and each command or tool on one mono line with how it
/// ended; the output folds open by itself when the command failed.
fn log_entry(ui: &mut Ui, key: &str, n: usize, entry: &Entry) {
    let failed = entry.failed();
    let tone = if failed { p().danger } else { p().text_2 };
    match entry {
        Entry::You(text) => {
            ui.label(label("you"));
            ui.add(egui::Label::new(RichText::new(text).color(p().text_2)).wrap());
        }
        Entry::Codex(text) => markdown(ui, text),
        Entry::Command {
            command,
            status,
            exit,
            ms,
            output,
        } => {
            let mut how = match exit {
                Some(code) => format!("exit {code}"),
                None => status.clone(),
            };
            if let Some(ms) = ms {
                how.push_str(&format!(" · {:.1}s", *ms as f64 / 1000.0));
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("$ {command}"))
                            .monospace()
                            .color(tone),
                    )
                    .truncate(),
                );
                ui.label(label(&how).color(tone));
            });
            if !output.trim().is_empty() {
                egui::CollapsingHeader::new(label("output"))
                    .id_salt(("codex-output", key, n))
                    .default_open(failed)
                    .show(ui, |ui| mono_block(ui, output));
            }
        }
        Entry::Tool {
            name,
            status,
            error,
        } => {
            ui.label(label(&format!("tool {name} · {status}")).color(tone));
            if let Some(error) = error {
                mono_block(ui, error);
            }
        }
        Entry::Note(text) => {
            ui.add(egui::Label::new(label(text)).wrap());
        }
    }
}

/// Output or an error as it came, its last 40 lines, selectable.
fn mono_block(ui: &mut Ui, text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(40)..].join("\n");
    Frame::new()
        .stroke(Stroke::new(1.0, p().line))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(RichText::new(tail).monospace().small().color(p().text_2))
                    .selectable(true)
                    .wrap(),
            );
        });
}

/// Why the last turn failed, as one card: what went wrong in plain words,
/// Codex's message, the usage when that is the cause, what to do next, and
/// the details to copy.
fn failure_card(
    app: &App,
    key: &str,
    view: &codex::View,
    failure: &codex::Failure,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let trouble = failure.trouble();
    let external = key.starts_with("cid:");
    Frame::new()
        .stroke(Stroke::new(1.0, p().danger))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(failure.title())
                    .family(medium())
                    .color(p().danger),
            );
            ui.add(egui::Label::new(RichText::new(&failure.message).color(p().text)).wrap());
            if view.usage {
                ui.add_space(6.0);
                match app.claude.codex_limits() {
                    Some(limits) => {
                        let now = jiff::Timestamp::now().as_second();
                        let (windows, notes) = codex::usage(limits, now, clock);
                        for window in &windows {
                            usage_row(ui, window);
                        }
                        for note in &notes {
                            ui.label(label(note));
                        }
                    }
                    None => {
                        ui.label(label("reading the account's usage…"));
                    }
                }
            }
            let next = match trouble {
                Trouble::Usage => {
                    Some("Codex goes on after the reset; Claude Code can take this ticket now.")
                }
                Trouble::TooLong => {
                    Some("Start fresh gives Codex the brief again in a new conversation.")
                }
                Trouble::Unreachable => Some("This usually passes: Retry asks Codex to go on."),
                Trouble::SignedOut => {
                    Some("Run codex login in a shell (Cmd+N, then Shell), then Retry.")
                }
                Trouble::Other => None,
            };
            if let Some(next) = next {
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(next).color(p().text_2)).wrap());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if matches!(
                    trouble,
                    Trouble::Unreachable | Trouble::SignedOut | Trouble::Other
                ) && primary(ui, "Retry").clicked()
                {
                    actions.push(Action::CodexPrompt {
                        key: key.to_owned(),
                        text: "Continue where you stopped.".into(),
                    });
                }
                // A new conversation with the agent picked: out of usage,
                // the other one goes first even when Codex is the default.
                if !external {
                    let default = app.claude.default_agent;
                    let agent = match trouble {
                        Trouble::Usage => crate::claude::Agent::Claude,
                        _ => default,
                    };
                    let first = matches!(trouble, Trouble::Usage | Trouble::TooLong);
                    let fresh = |_| Action::FreshSession(key.to_owned());
                    actions.extend(agent_choice(
                        ui,
                        agent,
                        default,
                        "Start fresh",
                        first,
                        fresh,
                    ));
                }
                if hover_hand(&ui.button("Copy details")).clicked() {
                    ui.ctx().copy_text(failure.report());
                }
            });
            egui::CollapsingHeader::new(label("details"))
                .id_salt(("codex-failure", key))
                .show(ui, |ui| mono_block(ui, &failure.report()));
        });
}

/// "5 h   ▮▮▮▮▮▮  100% used · resets Mon 19:29 (in 39m)".
fn usage_row(ui: &mut Ui, window: &codex::Window) {
    ui.horizontal(|ui| {
        let span = RichText::new(&window.span)
            .monospace()
            .small()
            .color(p().text_2);
        let size = egui::vec2(52.0, 14.0);
        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
            ui.label(span);
        });
        let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 6.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, p().line_2);
        let mut used = rect;
        used.set_width(rect.width() * (window.used.clamp(0, 100) as f32 / 100.0));
        let fill = if window.used >= 100 {
            p().danger
        } else {
            p().text_2
        };
        ui.painter().rect_filled(used, 0.0, fill);
        let mut text = format!("{}% used", window.used);
        if let Some(resets) = &window.resets {
            text.push_str(&format!(" · resets {resets}"));
        }
        ui.label(label(&text));
    });
}

/// A Codex reset time on the user's clock: "Mon 16:49".
fn clock(at: i64) -> String {
    jiff::Timestamp::from_second(at)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%a %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn codex_pane(app: &App, key: &str, ui: &mut Ui, actions: &mut Vec<Action>) {
    super::tutorial::mark(app, ui, "terminal", ui.available_rect_before_wrap());
    let saved = app.claude.saved.get(key);
    let external = key.starts_with("cid:");
    // Borrowed: the log can hold long command output, too much to copy
    // every frame.
    static EMPTY: std::sync::LazyLock<codex::View> = std::sync::LazyLock::new(Default::default);
    let view = app.claude.codex_view(key).unwrap_or(&EMPTY);
    let loaded = app.claude.codex_view(key).is_some();
    ui.horizontal(|ui| {
        dot(ui, status_color(&view.status));
        ui.label(label(if view.status.is_empty() {
            "starting"
        } else {
            &view.status
        }));
        ui.add(agent_icon(ui, crate::claude::Agent::Codex));
        ui.label(label("Codex"));
        if let Some(saved) = saved {
            ui.label(label(saved.short_id()));
        }
        if view.turn.is_some() && ui.small_button("Stop").clicked() {
            actions.push(Action::StopSession(key.to_owned()));
        }
    });
    hairline(ui);
    // The conversation, and at its end why the last turn failed: both
    // scroll together, so the card sits where the run stopped.
    let height = (ui.available_height() - 130.0).max(80.0);
    egui::ScrollArea::vertical()
        .max_height(height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add_space(6.0);
            for (n, (_, entry)) in view.log.iter().enumerate() {
                log_entry(ui, key, n, entry);
                ui.add_space(10.0);
            }
            if !view.stream.is_empty() {
                markdown(ui, &view.stream);
            }
            if let Some(failure) = &view.failure {
                failure_card(app, key, view, failure, ui, actions);
            }
        });
    if let Some(approval) = &view.pending {
        ui.label(
            RichText::new(&approval.title)
                .family(medium())
                .color(p().danger),
        );
        if approval.question {
            let mut answers = serde_json::Map::new();
            for question in approval.params["questions"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Some(id) = question["id"].as_str() else {
                    continue;
                };
                ui.add(egui::Label::new(label(question["question"].as_str().unwrap_or(id))).wrap());
                let field = egui::Id::new(("codex-answer", key, id));
                let mut answer = ui.data(|d| d.get_temp::<String>(field)).unwrap_or_default();
                ui.add(egui::TextEdit::singleline(&mut answer).desired_width(ui.available_width()));
                ui.data_mut(|d| d.insert_temp(field, answer.clone()));
                answers.insert(id.to_owned(), serde_json::json!({"answers": [answer]}));
            }
            if primary(ui, "Answer").clicked() {
                actions.push(Action::CodexAnswer {
                    key: key.to_owned(),
                    response: serde_json::json!({"answers": answers}),
                });
            }
        } else {
            ui.add(egui::Label::new(label(&approval.detail)).wrap());
            ui.horizontal(|ui| {
                if primary(ui, "Allow once").clicked() {
                    let response = if approval.method == "item/permissions/requestApproval" {
                        serde_json::json!({"permissions": approval.params["permissions"]})
                    } else {
                        serde_json::json!({"decision":"accept"})
                    };
                    actions.push(Action::CodexAnswer {
                        key: key.to_owned(),
                        response,
                    });
                }
                if ui.button("Decline").clicked() {
                    let response = if approval.method == "item/permissions/requestApproval" {
                        serde_json::json!({"permissions": {}})
                    } else {
                        serde_json::json!({"decision":"decline"})
                    };
                    actions.push(Action::CodexAnswer {
                        key: key.to_owned(),
                        response,
                    });
                }
            });
        }
    }
    // The box's text, and the line the app last typed into it (a skill
    // command left for the user to finish): a new one replaces the text.
    // Out of usage: say until when, where the user would type.
    let hint = match (&view.failure, app.claude.codex_limits()) {
        (Some(f), Some(limits)) if f.trouble() == Trouble::Usage => {
            let (windows, _) = codex::usage(limits, jiff::Timestamp::now().as_second(), clock);
            windows
                .iter()
                .find(|w| w.used >= 100)
                .and_then(|w| w.resets.clone())
                .map_or("Ask Codex…".to_owned(), |at| {
                    format!("Codex is out of usage until {at}")
                })
        }
        _ => "Ask Codex…".to_owned(),
    };
    let id = egui::Id::new(("codex-prompt", key));
    let (mut draft, typed) = ui
        .data(|d| d.get_temp::<(String, String)>(id))
        .unwrap_or_default();
    if view.draft != typed {
        draft.clone_from(&view.draft);
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::multiline(&mut draft)
                .desired_rows(2)
                .hint_text(hint)
                .desired_width(ui.available_width() - 76.0),
        );
        if ui
            // While a turn runs, Send steers it (turn/steer).
            .add_enabled(
                (saved.is_some() || external) && loaded && view.pending.is_none(),
                egui::Button::new(if view.turn.is_some() { "Steer" } else { "Send" }),
            )
            .clicked()
        {
            actions.push(Action::CodexPrompt {
                key: key.to_owned(),
                text: draft.trim().to_owned(),
            });
            draft.clear();
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (draft, view.draft.clone())));
}

/// A session that ended: say why, and offer only what can work.
fn ended(app: &App, key: &str, name: &str, ui: &mut Ui, actions: &mut Vec<Action>) {
    let row = app.claude.row_of(key).filter(|r| !r.running());
    let resumable = app.claude.resumable(key);
    ui.label(
        RichText::new("The session ended")
            .size(18.0)
            .family(medium()),
    );
    ui.add(egui::Label::new(RichText::new(name).color(p().text)).truncate());
    ui.add_space(8.0);
    let reason = row
        .and_then(|r| r.failure.clone().or_else(|| r.state.clone()))
        .unwrap_or_else(|| "Claude Code no longer lists it.".into());
    Frame::new()
        .stroke(Stroke::new(1.0, p().danger))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(RichText::new(&reason).monospace().small().color(p().danger))
                    .selectable(true)
                    .wrap(),
            );
        });
    ui.add_space(8.0);
    let hint = if resumable {
        "Resume picks up the conversation. Start fresh begins a new one in the same folder."
    } else {
        "It never got a message, so there is nothing to resume. Start fresh begins a new session in the same folder."
    };
    ui.add(egui::Label::new(RichText::new(hint).color(p().text_2)).wrap());
    ui.add_space(12.0);
    // Start fresh with the default agent, or another from the chevron.
    let default = app.claude.default_agent;
    centered_row(ui, key, |ui| {
        if resumable && primary(ui, "Resume session").clicked() {
            actions.push(Action::Attach(key.to_owned()));
        }
        let fresh = |_| Action::FreshSession(key.to_owned());
        let first = !resumable;
        actions.extend(agent_choice(
            ui,
            default,
            default,
            "Start fresh",
            first,
            fresh,
        ));
        if hover_hand(&ui.button("Copy details")).clicked() {
            ui.ctx().copy_text(reason.clone());
        }
    });
}

/// A session the app did not start: an interactive terminal somewhere on
/// the machine, or someone else's background session.
fn external(app: &mut App, key: &str, ui: &mut Ui, actions: &mut Vec<Action>) {
    let sid = key.trim_start_matches("sid:");
    let Some(row) = app.claude.rows.get(sid).cloned() else {
        ui.label(RichText::new("That session is gone.").color(p().text_2));
        return;
    };
    ui.horizontal(|ui| {
        state(ui, row.label(), status_color(row.label()), p().text_2);
        ui.label(
            RichText::new(&row.cwd)
                .small()
                .monospace()
                .color(p().text_3),
        );
    });
    ui.add_space(6.0);
    if app.claude.is_attached(key) {
        super::tutorial::mark(app, ui, "terminal", ui.available_rect_before_wrap());
        let terminal = app.claude.attached.get_mut(key).expect("attached");
        actions.extend(crate::claude::show(ui, terminal).map(Action::Link));
        return;
    }
    ui.add_space(ui.available_height() * 0.2);
    ui.vertical_centered(|ui| {
        card_frame().show(ui, |ui| {
            ui.set_width(460.0);
            let title = row.name.clone().unwrap_or_else(|| row.folder().to_owned());
            ui.label(RichText::new(title).size(18.0).family(medium()));
            ui.label(RichText::new(row.folder()).monospace().color(p().text_3));
            ui.add_space(8.0);
            if let Some(activity) = &row.activity
                && !activity.last_line.is_empty()
            {
                ui.label(
                    RichText::new(format!("Claude, {} ago", activity.ago()))
                        .small()
                        .color(p().text_3),
                );
                ui.add(egui::Label::new(RichText::new(&activity.last_line).color(p().text)).wrap());
                ui.add_space(10.0);
            }
            if row.kind == "background" && row.running() {
                if primary(ui, "Attach").clicked() {
                    actions.push(Action::Attach(key.to_owned()));
                }
            } else if row.running() {
                let host = row.host_name().filter(|h| *h != "Orbit");
                if let Some(host) = host
                    && primary(ui, &format!("Show in {host}")).clicked()
                {
                    actions.push(Action::OpenSession(key.to_owned()));
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "Started {}, so it is answered there.",
                            origin(&row)
                        ))
                        .color(p().text_2),
                    )
                    .wrap(),
                );
            }
        });
    });
}

/// Where a session Orbit did not start runs: "in Ghostty", "in an Orbit
/// shell", "in the background".
fn origin(row: &crate::claude::Live) -> String {
    match row.host_name() {
        _ if row.kind == "background" => "in the background, not by Orbit".into(),
        Some("Orbit") => "in an Orbit shell".into(),
        Some(host) => format!("in {host}"),
        None => "in a terminal".into(),
    }
}

/// One session in the Sessions list.
pub(super) struct Item {
    pub key: String,
    title: String,
    place: String,
    line: String,
    ago: String,
    modified: Option<std::time::SystemTime>,
    label: String,
    /// Everything the search looks in, lowercased: title, folder, last
    /// line, and what the transcript is about.
    pub(super) text: String,
    /// How much it makes sense to look at it now, and why in a few words;
    /// `laya` when Laya's answer moved it.
    score: f32,
    why: String,
    laya: bool,
}

/// What a session asks of the user now: what it needs, how recent it is,
/// its ticket (ranked in Plan my day, asking for the user, or done), and
/// Laya's read of its last message (`(waits on the user, confidence)`).
/// Higher first; the words say why.
fn relevance(
    label: &str,
    hours: Option<f64>,
    card: Option<&Card>,
    plan_rank: Option<usize>,
    asks: Option<(bool, f64)>,
) -> (f32, String, bool) {
    let mut why: Vec<String> = Vec::new();
    // herdr's order: blocked, then done (a turn not looked at), then
    // working, then idle; a failure sits with the turns, it needs a look.
    let mut score = match label {
        "needs approval" => 5.0,
        "your turn" => 3.0,
        "failed" => 2.5,
        "working" => 2.0,
        "idle" => 1.0,
        "stopped" => 0.5,
        _ => 0.0,
    };
    if matches!(label, "needs approval" | "failed" | "your turn") {
        why.push(label.to_owned());
    }
    let mut laya = false;
    match asks {
        Some((true, c)) if c >= 0.7 => {
            score += 2.0;
            why.push("asks you something".into());
            laya = true;
        }
        Some((false, c)) if c >= 0.7 && label == "your turn" => {
            score -= 1.0;
            why.push("reports it done".into());
            laya = true;
        }
        _ => {}
    }
    if let Some(rank) = plan_rank {
        score += (3.0 - 0.5 * rank as f32).max(0.5);
        why.push(format!("plan #{}", rank + 1));
    }
    if let Some(card) = card {
        if card.stage == Stage::Done {
            score -= 3.0;
            why.push("ticket done".into());
        } else if let Some(ask) = card.ask {
            score += 1.5;
            why.push(ask.label().to_lowercase());
        }
    }
    if let Some(h) = hours {
        score += 2.0 * (-h / 12.0).exp() as f32;
    }
    (score, why.join(" · "), laya)
}

fn group(label: &str) -> usize {
    match label {
        "needs approval" => 0,
        "working" => 1,
        "your turn" => 2,
        "idle" => 3,
        _ => 4,
    }
}

/// Every Claude Code session on the machine, grouped by what it needs,
/// most recently active first in each group: ticket sessions, free ones,
/// the board run, and the ones the app did not start.
pub(super) fn items(app: &App) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for row in app.claude.rows.values() {
        let saved_key = app.claude.key_of(&row.session_id);
        // Ended sessions only matter when they are one of ours.
        if !row.running() && saved_key.is_none() {
            continue;
        }
        let (key, title, place) = match saved_key {
            Some(key) => {
                let saved = &app.claude.saved[key];
                (key.clone(), app.session_name(key, saved), String::new())
            }
            // Not one of ours: where it runs, so it can be found.
            None => (
                format!("sid:{}", row.session_id),
                row.name.clone().unwrap_or_else(|| row.folder().to_owned()),
                format!("{} · {}", row.folder(), origin(row)),
            ),
        };
        // The session's own summary first (what it is on), then what the
        // app knows: a failure, the hooks' last line, the transcript's.
        let summary = saved_key
            .and_then(|k| app.claude.status.get(k))
            .and_then(|reports| crate::claude::status::note(reports))
            .map(|r| r.text.clone())
            .or_else(|| saved_key.and_then(|k| app.claude.notes.get(k).cloned()));
        let line = summary
            .or_else(|| row.failure.clone())
            .or_else(|| row.report.as_ref().map(|r| r.text.clone()))
            .or_else(|| row.activity.as_ref().map(|a| a.last_line.clone()))
            .unwrap_or_default();
        items.push(Item {
            key,
            title,
            place,
            line,
            ago: row.activity.as_ref().map(|a| a.ago()).unwrap_or_default(),
            modified: row.activity.as_ref().and_then(|a| a.modified),
            label: row.label().to_owned(),
            text: String::new(),
            score: 0.0,
            why: String::new(),
            laya: false,
        });
    }
    for (id, row) in app.claude.codex_external() {
        if app.claude.key_of(id).is_some() {
            continue;
        }
        let label = match crate::claude::codex::status(&row["status"]) {
            "your turn" => "idle",
            other => other,
        };
        let title = row["name"]
            .as_str()
            .or_else(|| row["preview"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Codex session");
        items.push(Item {
            key: format!("cid:{id}"),
            title: title.to_owned(),
            place: row["cwd"].as_str().unwrap_or_default().to_owned(),
            line: app
                .claude
                .codex_view(&format!("cid:{id}"))
                .map_or(String::new(), |v| v.last_line.clone()),
            ago: String::new(),
            modified: None,
            label: label.into(),
            text: String::new(),
            score: 0.0,
            why: String::new(),
            laya: false,
        });
    }
    // The app's own sessions that Claude Code no longer lists: stopped, and
    // opening one resumes it (or starts fresh without a transcript).
    for (key, saved) in &app.claude.saved {
        if !items.iter().any(|i| &i.key == key) {
            items.push(Item {
                key: key.clone(),
                title: app.session_name(key, saved),
                place: saved.cwd.display().to_string(),
                line: String::new(),
                ago: story::ago(&saved.started_at),
                modified: None,
                label: "stopped".into(),
                text: String::new(),
                score: 0.0,
                why: String::new(),
                laya: false,
            });
        }
    }
    let plan = app.spots.as_ref().and_then(|s| s.as_ref().ok());
    for item in &mut items {
        let id = app
            .claude
            .saved
            .get(&item.key)
            .map_or(item.key.trim_start_matches("sid:"), |s| {
                s.session_id.as_str()
            });
        let about = app.claude.about.get(id);
        // An unnamed session goes by Claude Code's title for it.
        if let Some(title) = about.and_then(|a| a.title.as_ref())
            && (item.title.ends_with("untitled")
                || item.place.split(" · ").next() == Some(item.title.as_str()))
        {
            item.title = title.clone();
        }
        // The state is a word to search by too ("your turn", "working").
        item.text = format!(
            "{}\n{}\n{}\n{}\n{}",
            item.title,
            item.place,
            item.line,
            item.label,
            about.map_or("", |a| a.text.as_str())
        )
        .to_lowercase();
        let card = app.card(&item.key);
        let rank = card.zip(plan).and_then(|(c, plan)| {
            plan.spots
                .iter()
                .position(|s| s.ticket.as_deref() == Some(c.issue.identifier.as_str()))
        });
        let hours = item
            .modified
            .and_then(|m| m.elapsed().ok())
            .map(|d| d.as_secs_f64() / 3600.0);
        let asks = app
            .session_asks
            .get(&item.key)
            .map(|(_, yes, c)| (*yes, *c));
        (item.score, item.why, item.laya) = relevance(&item.label, hours, card, rank, asks);
    }
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.modified.cmp(&a.modified))
    });
    items
}

/// Relevance order: the first few are for now.
const NOW: usize = 5;

/// The Sessions view in the sheet: the board run, a new session, and every
/// session, one line each. The cursor's line is selected.
pub(super) fn list(app: &App, cursor: Option<&bar::Entry>, ui: &mut Ui, actions: &mut Vec<Action>) {
    let mut items = items(app);
    let panel = ui.scope(|ui| {
        let count = |g| items.iter().filter(|i| group(&i.label) == g).count();
        let subtitle = format!(
            "{} need approval · {} working · {} your turn · {} idle",
            count(0),
            count(1),
            count(2),
            count(3)
        );
        title_bar(ui, "Sessions", &subtitle, bar::hints(&app.view));
        // The search first: the view is for finding a session. A new one
        // without a ticket is Cmd+K's "New session: <what>".
        let rect = sheet_search(
            app,
            ui,
            "search sessions: title, folder, what you asked, a state (your turn)",
            actions,
        );
        mark(app, ui, "session-search", rect);
        let searching = !app.sheet_query.trim().is_empty();
        if searching {
            items.retain(|i| matches_all(&i.text, &app.sheet_query));
        }
        // The operator's row, and the board run's under a profile with one:
        // as tall as they drew last frame, so a bigger font or a wrapped
        // row never pushes the board run under the bottom bar.
        let footer_id = egui::Id::new("sessions-footer");
        let footer = ui.data(|d| d.get_temp::<f32>(footer_id)).unwrap_or(
            if crate::profile::get().orchestrator.is_some() {
                100.0
            } else {
                40.0
            },
        );
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .max_height((ui.available_height() - footer).max(0.0))
            .show(ui, |ui| {
                let mut current = None;
                for (i, item) in items.iter().enumerate() {
                    let g = usize::from(i >= NOW);
                    if !searching && current != Some(g) {
                        current = Some(g);
                        group_label(ui, ["// now", "// later"][g]);
                    }
                    let selected = cursor == Some(&bar::Entry::Session(item.key.clone()));
                    let response = row(ui, selected, p().signal, true, |ui| {
                        let row = egui::vec2(ui.available_width(), 18.0);
                        ui.allocate_ui_with_layout(
                            row,
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                ui.label(label(&item.ago));
                                if item.laya {
                                    laya_chip(
                                        ui,
                                        "Laya read the last message: whether it waits on you",
                                    );
                                }
                                if !item.why.is_empty() {
                                    ui.label(label(&item.why).color(p().text_3));
                                }
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    state_dot(ui, &item.label, status_color(&item.label));
                                    ui.add(agent_icon(ui, app.claude.agent_of(&item.key)));
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&item.title).color(p().text),
                                        )
                                        .truncate(),
                                    );
                                });
                            },
                        );
                        let detail = [item.place.as_str(), item.line.as_str()]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .copied()
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if !detail.is_empty() {
                            let color = if item.label == "failed" {
                                p().danger
                            } else {
                                p().text_2
                            };
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.add(egui::Label::new(label(&detail).color(color)).truncate());
                            });
                        }
                    });
                    if selected {
                        bar::follow(ui, &response, &item.key);
                    }
                    if response.clicked() {
                        actions.push(Action::OpenSession(item.key.clone()));
                    }
                }
                if items.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        let empty = if searching {
                            "No session matches."
                        } else {
                            "No local sessions yet."
                        };
                        ui.label(RichText::new(empty).color(p().text_2));
                    });
                }
            });
        // The operator, then the board run and its autopilot, last:
        // controls, not sessions.
        let top = ui.cursor().top();
        hairline(ui);
        let rect = pad(ui, |ui| operator(app, ui, actions));
        mark(app, ui, "operator", rect);
        if crate::profile::get().orchestrator.is_some() {
            hairline(ui);
            let rect = pad(ui, |ui| orchestrator(app, ui, actions));
            mark(app, ui, "orchestrate", rect);
        }
        let drawn = ui.cursor().top() - top + 8.0;
        if (drawn - footer).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(footer_id, drawn));
            ui.ctx().request_repaint();
        }
    });
    mark(app, ui, "list", panel.response.rect);
}

/// The operator (skills/operate): one Claude run that works Orbit through
/// `orbit ctl`, its actions shown as grey agent toasts. While it runs, its
/// state instead; click it to watch.
fn operator(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let key = crate::claude::OPERATOR;
    let running = app.claude.busy(key);
    ui.horizontal(|ui| {
        if !running
            && hover_hand(&ui.button("✳ Run the operator"))
                .on_hover_text(
                    "One Claude run works Orbit for you through orbit ctl: alerts, \
                     notifications, For you, your PRs, finished sessions. You watch it as grey \
                     agent toasts. It never approves, merges or changes what others see.",
                )
                .clicked()
        {
            actions.push(Action::Operate);
        }
        let status = match app.claude.live_of(key).map(|l| l.label()) {
            _ if app.claude.starting.contains(key) => "starting".to_owned(),
            Some(label) => label.to_owned(),
            None if app.claude.saved.contains_key(key) => "finished".to_owned(),
            None => "not run yet".to_owned(),
        };
        let color = if running { p().signal } else { p().text_3 };
        if link(
            ui,
            label(&format!("✳ operator: {status} · open")).color(color),
        )
        .on_hover_text("Show the run and its report")
        .clicked()
        {
            actions.push(Action::OpenSession(key.to_owned()));
        }
    });
}

/// One click hands the profile's board to an unattended orchestrator run
/// (the profile's orchestrate skill). While it runs, its state instead;
/// click it to watch.
fn orchestrator(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(team) = crate::profile::get()
        .orchestrator
        .as_ref()
        .map(|o| o.team.clone())
    else {
        return;
    };
    let key = crate::claude::ORCHESTRATOR;
    let live = app.claude.live_of(key).map(|l| l.label());
    let starting = app.claude.starting.contains(key);
    let running = app.claude.busy(key);
    ui.horizontal(|ui| {
        if !running {
            let button = hover_hand(&ui.button("✳ Run the board")).on_hover_text(format!(
                "One unattended Claude run operates the {team} board: lands approved PRs, \
                 fixes red ones, splits Blocked tickets, refills Todo, grooms Triage. It never \
                 approves a PR. The report lists what is ready for you."
            ));
            if button.clicked() {
                actions.push(Action::Orchestrate);
            }
        }
        let status = match (starting, live) {
            (true, _) => Some("starting".to_owned()),
            (false, Some(label)) => Some(label.to_owned()),
            (false, None) if app.claude.saved.contains_key(key) => Some("finished".to_owned()),
            _ => None,
        };
        let status = status.unwrap_or_else(|| "not run today".to_owned());
        let color = if running { p().signal } else { p().text_3 };
        let text = label(&format!("✳ board run: {status} · open")).color(color);
        if link(ui, text)
            .on_hover_text("Show the run and its report")
            .clicked()
        {
            actions.push(Action::OpenSession(key.to_owned()));
        }
        let rect = autopilot(app, ui, actions);
        mark(app, ui, "autopilot", rect);
    });
}

/// The autopilot's switch: Run the board every hour, eight times, then off.
fn autopilot(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) -> egui::Rect {
    let state = &app.autopilot;
    let mut on = state.on;
    let text = match (state.on, state.runs) {
        (true, runs) => format!("autopilot · {runs} of {}", crate::autopilot::RUNS),
        (false, runs) if runs >= crate::autopilot::RUNS => "autopilot · done for today".into(),
        (false, _) => "autopilot".into(),
    };
    let response = ui.checkbox(&mut on, label(&text)).on_hover_text(format!(
        "Runs the board every hour while on, the first run at once. Switches itself off \
         after {} runs, so turn it on in the morning.",
        crate::autopilot::RUNS
    ));
    if response.changed() {
        actions.push(Action::Autopilot(on));
    }
    response.rect
}

#[cfg(test)]
mod tests {
    use super::*;

    /// herdr's order in the Sessions view: needs approval, your turn,
    /// working, idle, stopped, with nothing else to tell them apart.
    #[test]
    fn sessions_rank_needs_approval_then_your_turn_then_working_then_idle() {
        let score = |label| relevance(label, None, None, None, None).0;
        let order = ["needs approval", "your turn", "working", "idle", "stopped"];
        for pair in order.windows(2) {
            assert!(
                score(pair[0]) > score(pair[1]),
                "{} above {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn a_done_ticket_sinks_and_one_that_asks_the_user_rises() {
        use crate::story::tests::issue;
        let mut done = crate::story::card(issue("Done", "completed", vec![]), &Default::default());
        done.stage = Stage::Done;
        let mut asking = done.clone();
        asking.stage = Stage::Review;
        asking.ask = Some(crate::story::Ask::Review);
        let sunk = relevance("idle", Some(1.0), Some(&done), None, None);
        let lifted = relevance("idle", Some(1.0), Some(&asking), None, None);
        let bare = relevance("idle", Some(1.0), None, None, None);
        assert!(sunk.0 < bare.0 && bare.0 < lifted.0);
        assert!(sunk.1.contains("ticket done"));
        assert_eq!(lifted.1, crate::story::Ask::Review.label().to_lowercase());
        assert!(!sunk.2 && !lifted.2, "no Laya read, no Laya chip");
    }

    #[test]
    fn a_session_is_found_by_every_word_of_the_query() {
        let text = "abc-9  untitled\n\nship the workers to prod with the new deploy config\n";
        assert!(matches_all(text, "Workers prod"));
        assert!(matches_all(text, "  deploy  "));
        assert!(!matches_all(text, "workers staging"));
    }

    #[test]
    fn a_question_for_you_ranks_above_a_done_report_and_a_done_ticket_sinks() {
        let asking = relevance("your turn", Some(1.0), None, None, Some((true, 0.9)));
        let done = relevance("your turn", Some(1.0), None, None, Some((false, 0.9)));
        let working = relevance("working", Some(0.1), None, None, None);
        assert!(asking.0 > done.0 && asking.2 && done.2);
        assert!(asking.1.contains("asks you something"));
        // Not sure: Laya changes nothing and says nothing.
        let unsure = relevance("your turn", Some(1.0), None, None, Some((true, 0.5)));
        assert!(!unsure.2 && unsure.0 < asking.0);
        let approval = relevance("needs approval", Some(30.0), None, None, None);
        assert!(approval.0 > working.0);
        let planned = relevance("idle", Some(5.0), None, Some(0), None);
        let unplanned = relevance("idle", Some(5.0), None, None, None);
        assert!(planned.0 > unplanned.0 && planned.1.contains("plan #1"));
    }
}
