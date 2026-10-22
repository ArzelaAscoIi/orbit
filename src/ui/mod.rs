//! The window, multiplexer style: the windows in a sidebar on the left (as
//! herdr and xirp keep their sessions), the panes of the current one beside
//! it, and a bar of views at the bottom whose sheet rises over the panes
//! (`i`). The sidebar collapses to numbers and states for the full width.
//!
//! Every view draws from `&App` and pushes `Action`s; nothing here mutates
//! state except text fields, which borrow their `String` directly.
//!
//! - `bar`: the windows sidebar, the bottom bar, the sheet and its keys.
//! - `capture`: the New ticket window (Cmd+T).
//! - `palette`: the command palette (Cmd+K).
//! - `login`: the key screen and the team picker.
//! - `list`: what the sheet shows: tickets, customers, the day's plan.
//! - `panes`: the current window's split panes; the ticket pane is one.
//! - `ticket`: the walkthrough of one ticket.
//! - `diff`: the PR diff with Claude's walkthrough of it.
//! - `sessions`: Claude Code sessions, attached or listed.
//! - `testing`: the test plan of a ticket in Testing.
//! - `settings`: the theme picker.
//! - `tutorial`: the guided tour over all of it.
//! - `widgets`: the small pieces every view shares.

mod bar;
mod capture;
mod diff;
mod find;
mod list;
mod login;
mod palette;
mod panes;
pub(crate) mod sessions;
mod settings;
mod testing;
mod ticket;
pub mod tutorial;
mod widgets;

use egui::{Align, Align2, Color32, FontId, Frame, Layout, Margin, RichText, Sense, Stroke, Ui};

use crate::app::panes::TicketView;
use crate::app::{Action, App, Tab, View};
use crate::story::{self, Ask, Card, Ci, STEPS, Stage};
use crate::theme::{self, medium, p, semibold, wash};
use tutorial::mark;
use widgets::*;

/// Every keyboard shortcut, as the palette lists them (`Keyboard
/// shortcuts`, or type a key or what it does): the one place that names
/// them, so the list and `keys()` cannot drift apart (the test below holds
/// every key `keys()` consumes to a line here).
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("⌘K", "Command palette"),
    ("⌘T", "New ticket"),
    ("⌘J", "Your clone's chat, on the right"),
    ("⌘V", "Paste a GitHub PR or Linear link: opens it here"),
    ("⌘N", "New tab (Claude, a shell or a ticket)"),
    (
        "⏎ / T / E / B",
        "In an empty tab: Claude / a shell / an editor / a browser",
    ),
    (
        "Tab / ⇧Tab",
        "In the editor: move the selected lines right / left",
    ),
    ("⌘W", "Close the pane"),
    ("⌘D", "Split the pane to the right"),
    ("⌘⇧D", "Split the pane below"),
    ("⌘⇧⏎", "Zoom the pane"),
    ("⌘⏎", "Full screen: the focused session alone, Esc to leave"),
    (
        "⌘Z",
        "Reopen the last closed window, again for the one before",
    ),
    ("⌘]  ⌘[", "Next or previous pane"),
    ("⌘⇧]  ⌘⇧[", "Next or previous window"),
    ("⌘1 … ⌘9", "Go to window 1 to 9"),
    (
        "⌘⇧↑  ⌘⇧↓  ⌘⇧←  ⌘⇧→",
        "Move the window up or down the sidebar",
    ),
    ("⌘F", "Find on the screen; in a browser, in its page"),
    ("⌘⇧F", "Feedback: improve Orbit"),
    ("⌘⇧T", "Next theme"),
    ("⌘,", "Settings"),
    ("⌥↓", "Next ticket that needs you"),
    ("i", "Raise the sheet"),
    (
        "0 … 9",
        "Raise the sheet on a view (0 notifications, 1 for you, 5 sessions, 9 repos)",
    ),
    ("o", "Open the ticket's PR or the ticket"),
    (
        "esc",
        "Close the sheet, Settings or an overlay; leave full screen",
    ),
    ("j k h l", "Move in the sheet"),
    ("⏎", "Open the sheet's line in a window"),
    (
        "x",
        "Stop the session or mark the row read (For you: until it changes)",
    ),
    (
        "⇧X",
        "Close the session: stopped and off the list (Sessions)",
    ),
    ("r", "Plan again (Plan)"),
    ("⌘C", "Copy the selected terminal text"),
    (
        "⌃B then c v z n p 1…9 …",
        "herdr's keys, with your herdr config in use (Settings, herdr)",
    ),
    ("⌘ click", "Open a link in a session"),
];

pub fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if !app.signed_in() {
        login::sign_in(app, ui, actions);
        return;
    }
    if let Some(picked) = &app.picking {
        login::team_picker(&app.my_teams, picked, !app.teams.is_empty(), ui, actions);
        return;
    }
    keys(app, ui, actions);
    find::jump(app, ui);
    // The bar, then the windows sidebar: panels take their room before the
    // panes, and the bar runs the full width under the sidebar.
    if !app.full_screen {
        bar::bottom(app, ui, actions);
        bar::sidebar(app, ui, actions);
        clone_chat(app, ui, actions);
    }
    if app.view == View::Settings {
        settings::show(app, ui, actions);
    } else {
        panes::show(app, ui, actions);
    }
    bar::sheet(app, ui, actions);
    toasts(app, ui, actions);
    if !app.full_screen {
        clone_switch(app, ui, actions);
    }
    capture::show(app, ui, actions);
    palette::show(app, ui, actions);
    // Last but the tutorial: it searches what everything else drew.
    find::show(app, ui);
    // A link clicked in a ticket's markdown, kept for this frame's actions.
    let clicked = egui::Id::new(widgets::MARKDOWN_LINK);
    if let Some(url) = ui.ctx().data_mut(|d| d.remove_temp::<String>(clicked)) {
        actions.push(Action::Link(url));
    }
    tutorial::overlay(app, ui, actions);
}

/// The clone's chat on the right (Cmd+J), like a coding agent's panel in
/// an editor: each chat a full Claude Code session that steers Orbit
/// through its CLI. Above it every chat and clone run (passes, missions),
/// newest first; a click shows one, + starts a new chat.
fn clone_chat(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if !app.chat_open {
        return;
    }
    let mut runs: Vec<(String, String, String)> = app
        .claude
        .saved
        .iter()
        .filter(|(k, _)| app.is_clone_session(k))
        .map(|(k, s)| (k.clone(), app.session_name(k, s), s.started_at.clone()))
        .collect();
    runs.sort_by(|a, b| b.2.cmp(&a.2));
    // Each mission: its goal, how far, what is the user's, the last word.
    struct Row {
        id: String,
        goal: String,
        progress: String,
        yours: String,
        last: String,
        session: String,
        linear: Option<String>,
        /// Each sub-ticket: its step, identifier, its session's note (else
        /// what was done last), URL, its id before the bot's team took it.
        steps: Vec<(String, String, String, String, String)>,
    }
    let initiatives: Vec<Row> = app
        .missions
        .iter()
        .filter(|i| !i.done)
        .map(|i| {
            let parent = app.looked_up.get(&i.parent).and_then(|p| p.as_ref().ok());
            let kids = parent.map_or_else(Vec::new, |p| crate::mission::kids(&p.children.nodes));
            let done = kids.iter().filter(|k| k.implemented()).count();
            let review = kids.iter().filter(|k| k.state == "In Review").count();
            let blocked = kids.iter().filter(|k| k.state == "Blocked").count();
            let progress = match (i.parent.as_str(), kids.len()) {
                ("", _) => "filing the tickets".to_owned(),
                (p, 0) => format!("{p} · reading"),
                (p, n) => format!("{p} · {done} of {n} implemented"),
            };
            let yours = [(review, "to review"), (blocked, "blocked")]
                .iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, what)| format!("{n} {what}"))
                .collect::<Vec<_>>()
                .join(" · ");
            let bot = crate::profile::get()
                .orchestrator
                .as_ref()
                .map(|o| o.team.clone())
                .unwrap_or_default();
            let capped = app.bot_review_cap(&bot).is_some();
            let now = crate::activity::now_ms() as u64;
            // In the order they were filed (the build order), as `kids`.
            let steps = kids
                .iter()
                .filter_map(|kid| {
                    let c = parent?
                        .children
                        .nodes
                        .iter()
                        .find(|c| c.identifier == kid.identifier)?;
                    let (mut work, note, _) = app.work_of_kid(&kid.identifier);
                    let card = app
                        .cards
                        .iter()
                        .find(|card| card.issue.identifier == kid.identifier);
                    // Nothing on it for as long as the drive waits before a
                    // kick: no spinner says it works when it does not.
                    work.quiet = card.is_some_and(|card| {
                        let moved = i.moved.get(&kid.identifier).copied();
                        crate::app::quiet_ms(card, moved, now) >= crate::mission::KICK_MS
                    });
                    work.capped = capped;
                    let phase = crate::mission::phase(kid, &kids, &bot, i.parallel, &work);
                    // No session here reports on it: what the bot did last.
                    let note =
                        note.or_else(|| card.and_then(|c| crate::story::last_step(&c.timeline)));
                    Some((
                        phase,
                        kid.identifier.clone(),
                        note.unwrap_or_default(),
                        c.url.clone(),
                        c.previous_identifiers.last().cloned().unwrap_or_default(),
                    ))
                })
                .collect();
            Row {
                steps,
                id: i.id.clone(),
                goal: i.goal.clone(),
                progress,
                yours,
                last: i.log.last().map(|(_, l)| l.clone()).unwrap_or_default(),
                session: i.session.clone(),
                linear: parent.map(|p| p.url.clone()),
            }
        })
        .collect();
    let states: Vec<String> = runs
        .iter()
        .map(|(k, ..)| match app.claude.live_of(k) {
            _ if app.claude.starting.contains(k) => "starting".to_owned(),
            Some(live) => live.label().to_owned(),
            None => "stopped".to_owned(),
        })
        .collect();
    let panel = egui::Panel::right("clone-chat")
        .default_size(520.0)
        // Never dragged down to a sliver that reads as closed.
        .size_range(320.0..=1400.0)
        .resizable(true)
        .frame(
            Frame::new()
                .fill(p().bg)
                .inner_margin(Margin::symmetric(10, 8)),
        )
        .show(ui, |ui| {
            // Under the clone switch, top right: its row stays free.
            ui.add_space(30.0);
            // Headings lead; their actions are small links beside them.
            ui.horizontal(|ui| {
                ui.label(label("// your clone"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if link(ui, label("close")).clicked() {
                        actions.push(Action::ToggleChat);
                    }
                });
            });
            ui.add_space(6.0);
            // The missions first: the goals; the hourly pass is plumbing.
            let mut shown = true;
            ui.horizontal(|ui| {
                shown = fold(ui, "missions", true);
                if !shown && !initiatives.is_empty() {
                    ui.label(label(&initiatives.len().to_string()).color(p().text_3));
                }
                if link(ui, label("new mission"))
                    .on_hover_text(
                        "A goal in your words: a feature, a fix someone reported, a ticket to land",
                    )
                    .clicked()
                {
                    actions.push(Action::PaletteQuery("Mission: ".into()));
                }
            });
            let rect = ui
                .scope(|ui| {
                    if !shown {
                        return;
                    }
                    if app.missions.is_empty() {
                        ui.label(label("none yet"));
                    }
                    for row in &initiatives {
                        let current = app.chat.as_deref() == Some(row.session.as_str());
                        let response = widgets::row(ui, current, p().signal, false, |ui| {
                            // Right first, so the goal truncates before the
                            // links; the goal itself reads from the left.
                            let line = egui::vec2(ui.available_width(), 18.0);
                            ui.allocate_ui_with_layout(
                                line,
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    if link(ui, label("×"))
                                        .on_hover_text("Close this mission")
                                        .clicked()
                                    {
                                        actions.push(Action::StopDrive(row.id.clone()));
                                    }
                                    if app.claude.saved.contains_key(&row.session) {
                                        window_link(ui, &row.session, actions);
                                    }
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        // Something runs right now: it spins.
                                        if row.steps.iter().any(|s| crate::mission::moving(&s.0)) {
                                            ui.add(
                                                egui::Spinner::new().size(10.0).color(p().signal),
                                            );
                                        }
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&row.goal)
                                                    .family(medium())
                                                    .color(p().text),
                                            )
                                            .truncate(),
                                        );
                                    });
                                },
                            );
                            ui.horizontal(|ui| {
                                ui.label(label(&row.progress));
                                if !row.yours.is_empty() {
                                    ui.label(label(&row.yours).color(p().signal));
                                }
                            });
                            // Each sub-ticket's step, in filed order: the
                            // overview; the chat below stays the session.
                            for (phase, ident, note, url, was) in &row.steps {
                                ui.horizontal(|ui| {
                                    let tone = step_tone(phase);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(136.0, 16.0),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            ui.set_min_width(136.0);
                                            if crate::mission::moving(phase) {
                                                widgets::pulse(ui, tone);
                                            }
                                            ui.add(
                                                egui::Label::new(label(phase).color(tone))
                                                    .truncate(),
                                            );
                                        },
                                    );
                                    if link(ui, label(ident).color(p().text))
                                        .on_hover_text("Open the ticket")
                                        .clicked()
                                    {
                                        actions.push(Action::Link(url.clone()));
                                    }
                                    // Findable by the id the user filed it as.
                                    if !was.is_empty() {
                                        ui.label(label(&format!("was {was}")).color(p().text_3));
                                    }
                                    // A done one's last note is history.
                                    if phase != "done" {
                                        ui.add(egui::Label::new(label(note)).truncate());
                                    }
                                });
                            }
                            if !row.last.is_empty() {
                                ui.add(egui::Label::new(label(&row.last)).truncate());
                            }
                        });
                        if response
                            .clone()
                            .on_hover_text("Talk to the session that drives it: ask, steer")
                            .clicked()
                        {
                            actions.push(Action::Steer(row.id.clone()));
                        }
                        response.context_menu(|ui| {
                            if app.claude.saved.contains_key(&row.session)
                                && ui.button("Open in a window").clicked()
                            {
                                actions.push(Action::CloneWindow(row.session.clone()));
                            }
                            if let Some(url) = &row.linear
                                && ui.button("Open the parent in Linear").clicked()
                            {
                                actions.push(Action::Link(url.clone()));
                            }
                            if ui.button("Close this mission").clicked() {
                                actions.push(Action::StopDrive(row.id.clone()));
                            }
                        });
                    }
                    finished_missions(app, ui, actions);
                })
                .response
                .rect;
            mark(app, ui, "missions", rect);
            hairline(ui);
            let rect = ui.scope(|ui| hourly_pass(app, ui, actions)).response.rect;
            mark(app, ui, "clone", rect);
            hairline(ui);
            ui.horizontal(|ui| {
                ui.label(label("// chats and runs"));
                if link(ui, label("+ new chat")).clicked() {
                    actions.push(Action::NewChat);
                }
            });
            ui.add_space(4.0);
            for ((key, name, _), state) in runs.iter().zip(&states).take(6) {
                let current = app.chat.as_deref() == Some(key.as_str());
                let response = row(ui, current, p().signal, false, |ui| {
                    let line = egui::vec2(ui.available_width(), 18.0);
                    ui.allocate_ui_with_layout(line, Layout::right_to_left(Align::Center), |ui| {
                        // Close: stopped and off the list; Claude Code keeps it.
                        if link(ui, label("×"))
                            .on_hover_text("Close: stops it and takes it off the list")
                            .clicked()
                        {
                            actions.push(Action::CloseSession(key.clone()));
                        }
                        window_link(ui, key, actions);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            widgets::state(ui, state, status_color(state), p().text_2);
                            ui.add(
                                egui::Label::new(RichText::new(name).color(p().text)).truncate(),
                            );
                        });
                    });
                });
                if response.clicked() {
                    actions.push(Action::ChatShow(key.clone()));
                }
                response.context_menu(|ui| {
                    if ui.button("Open in a window").clicked() {
                        actions.push(Action::CloneWindow(key.clone()));
                        ui.close();
                    }
                    if ui.button("Close").clicked() {
                        actions.push(Action::CloseSession(key.clone()));
                        ui.close();
                    }
                });
            }
            hairline(ui);
            ui.add_space(4.0);
            let Some(key) = app.chat.clone() else {
                ui.label(label("+ new chat to talk to your clone"));
                return;
            };
            if let Some(window) = app.session_in_window(&key) {
                // One terminal, one size: shown in a window, it stays there.
                ui.horizontal(|ui| {
                    ui.label(label(&format!("open in window {}", window + 1)));
                    if link(ui, label("go there")).clicked() {
                        actions.push(Action::GoWindow(window));
                    }
                });
            } else if let Some(terminal) = app.claude.attached.get_mut(&key) {
                actions.extend(crate::claude::show(ui, terminal).map(Action::Link));
            } else if app.claude.starting.contains(&key) {
                ui.label(label("starting your clone…"));
            } else if ui.button("Resume this chat").clicked() {
                actions.push(Action::ChatShow(key));
            }
        });
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(bar::CHAT_RECT), panel.response.rect));
    mark(app, ui, "clone-chat", panel.response.rect);
}

/// The clone's chat button, top right over everything (Cmd+J does the
/// same); its hourly pass lives in the chat panel.
fn clone_switch(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let looping = app.claude.live_of(crate::claude::OPERATOR).is_some();
    let area = egui::Area::new(egui::Id::new(widgets::CLONE_SWITCH))
        .order(egui::Order::Foreground)
        .anchor(Align2::RIGHT_TOP, egui::vec2(-12.0, 8.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(p().bg)
                .stroke(Stroke::new(1.0, p().line_2))
                .inner_margin(Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    // One line however narrow the panel under it.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    let failed = pass_failed(app).is_some();
                    let working = clones_working(app);
                    let hourly = match (looping, failed) {
                        (_, true) => " · hourly failed",
                        (true, false) => " · hourly on",
                        (false, false) => "",
                    };
                    // Missions on the go: seen with the panel closed too.
                    let drives = app
                        .missions
                        .iter()
                        .filter(|m| !m.done && !m.parent.is_empty())
                        .count();
                    let text = label(&clone_switch_text(working, drives, hourly));
                    let text = match (failed, working + drives) {
                        (true, _) => text.color(p().danger),
                        (false, 0) => text,
                        (false, _) => text.color(p().signal),
                    };
                    if link(ui, text)
                        .on_hover_text("Your clone: its chats, missions and hourly pass")
                        .clicked()
                    {
                        actions.push(Action::ToggleChat);
                    }
                });
        });
    mark(app, ui, "clone-button", area.response.rect);
}

/// A step's colour: the user's turn stands out, work in progress is the
/// signal, done fades.
fn step_tone(phase: &str) -> egui::Color32 {
    match phase {
        "done" => p().text_3,
        moving if crate::mission::moving(moving) => p().signal,
        "your review" | "comments open" | "CI open" | "stopped" | "held: review cap" => p().danger,
        "building" | "merging" | "fixing comments" | "fixing CI" => p().signal,
        _ => p().text_2,
    }
}

/// Clone sessions (chats, missions, the hourly pass) working right now.
fn clones_working(app: &App) -> usize {
    app.claude
        .saved
        .keys()
        .filter(|k| app.is_clone_session(k))
        .filter(|k| {
            app.claude.starting.contains(*k)
                || app
                    .claude
                    .live_of(k)
                    .is_some_and(|l| l.label() == "working")
        })
        .count()
}

/// The clone switch's line: how many clones work now, and the hourly pass.
fn clone_switch_text(working: usize, drives: usize, hourly: &str) -> String {
    let working = match working {
        0 => String::new(),
        n => format!(" · {n} working"),
    };
    let drives = match drives {
        0 => String::new(),
        1 => " · 1 mission".to_owned(),
        n => format!(" · {n} missions"),
    };
    format!("clone{working}{drives}{hourly} ⌘J")
}

/// The hourly pass's last run failed and none got through since: how
/// long ago, and why.
fn pass_failed(app: &App) -> Option<(String, String)> {
    let reports = app.claude.status.get(crate::claude::OPERATOR)?;
    let failed = crate::claude::status::failed(reports).filter(|r| r.recent())?;
    let minutes = failed.age() / 60;
    let ago = if minutes < 60 {
        format!("{minutes}m ago")
    } else {
        format!("{}h ago", minutes / 60)
    };
    Some((ago, failed.text.clone()))
}

/// A section heading that folds: `// title −` open, `+` folded; a click
/// flips it. Returns whether it is open.
/// Finished missions under the running ones: the last two days, then a
/// folded "+ N earlier". A click opens its session as a running one's does;
/// × takes it off the list.
fn finished_missions(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let now = crate::activity::now_ms() as u64;
    let (recent, earlier) = crate::mission::finished_lists(&app.missions, now);
    let row = |ui: &mut Ui, it: &crate::mission::Initiative, actions: &mut Vec<Action>| {
        let line = egui::vec2(ui.available_width(), 18.0);
        ui.allocate_ui_with_layout(line, Layout::right_to_left(Align::Center), |ui| {
            if link(ui, label("×"))
                .on_hover_text("Take it off this list")
                .clicked()
            {
                actions.push(Action::DismissMission(it.id.clone()));
            }
            ui.label(label(&crate::mission::outcome(it)).color(p().text_3));
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if !it.parent.is_empty() {
                    ui.label(label(&it.parent).color(p().text_3));
                }
                if link(ui, label(&it.goal).color(p().text_2))
                    .on_hover_text("Open the session that drove it")
                    .clicked()
                {
                    actions.push(Action::Steer(it.id.clone()));
                }
            });
        });
    };
    if !recent.is_empty() {
        ui.add_space(4.0);
        ui.label(label("finished").color(p().text_3));
        for it in recent {
            row(ui, it, actions);
        }
    }
    if !earlier.is_empty() {
        let id = egui::Id::new("missions-earlier");
        let open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
        let text = if open {
            "− earlier".to_owned()
        } else {
            format!("+ {} earlier", earlier.len())
        };
        if link(ui, label(&text).color(p().text_3)).clicked() {
            ui.data_mut(|d| d.insert_temp(id, !open));
        }
        if open {
            for it in earlier {
                row(ui, it, actions);
            }
        }
    }
}

fn fold(ui: &mut Ui, title: &str, open_at_first: bool) -> bool {
    let id = egui::Id::new(("fold", title));
    let open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(open_at_first);
    let sign = if open { "−" } else { "+" };
    if link(ui, label(&format!("// {title} {sign}")))
        .on_hover_text(if open { "Fold" } else { "Unfold" })
        .clicked()
    {
        ui.data_mut(|d| d.insert_temp(id, !open));
    }
    open
}

/// A clone session's `window` link: the session in a window of its own,
/// marked in the sidebar.
fn window_link(ui: &mut Ui, key: &str, actions: &mut Vec<Action>) {
    if link(ui, label("window"))
        .on_hover_text("Open in a window of its own, marked CLONE in the sidebar")
        .clicked()
    {
        actions.push(Action::CloneWindow(key.to_owned()));
    }
}

/// The hourly pass in the clone panel: what it does, the jobs to give it,
/// and Start / Stop for its one /loop session. Folded by default: the
/// heading and its state stay, `+` shows the jobs.
fn hourly_pass(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let key = crate::claude::OPERATOR;
    let live = app.claude.live_of(key).map(|l| l.label().to_owned());
    let tasks = app.settings.clone.tasks.as_deref();
    let paused = tasks.is_some_and(<[String]>::is_empty);
    let failed = pass_failed(app);
    let mut open = false;
    ui.horizontal(|ui| {
        open = fold(ui, "hourly pass", false);
        // A run that failed (an API error) says so here, not only in its log.
        if let Some((ago, why)) = &failed {
            ui.label(label(&format!("last run failed {ago}")).color(p().danger))
                .on_hover_text(why);
        }
        match &live {
            Some(state) => {
                ui.label(label(&format!("running · {state}")).color(p().signal));
                if link(ui, label("watch")).clicked() {
                    actions.push(Action::ChatShow(key.to_owned()));
                }
                if link(ui, label("stop")).clicked() {
                    actions.push(Action::Clone(false));
                }
            }
            None if paused => {
                ui.label(label("paused").color(p().text_3));
            }
            None => {
                if link(ui, label("start").color(p().text))
                    .on_hover_text(
                        "One Claude Code session runs /loop 1h /operate on the jobs below",
                    )
                    .clicked()
                {
                    actions.push(Action::Clone(true));
                }
            }
        }
    });
    if !open {
        return;
    }
    // One line; the rules are on hover.
    ui.add(
        egui::Label::new(
            RichText::new("Each hour one Claude session works these jobs, then says what it did.")
                .color(p().text_2),
        )
        .wrap(),
    )
    .on_hover_text(
        "It never approves, merges or changes what others see, and starts nothing new while \
         five things wait on you. The loop ends after seven days. Untick every job to pause it.",
    );
    for (name, short, what) in crate::claude::PASS_TASKS {
        let mut on = tasks.is_none_or(|t| t.iter().any(|t| t == name));
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut on, RichText::new(short).color(p().text))
                .changed()
            {
                actions.push(Action::PassTask(name.to_owned(), on));
            }
            ui.add(egui::Label::new(label(what)).truncate());
        });
    }
    if paused {
        ui.label(label("paused: tick a job, then start"));
    } else if live.is_some() {
        ui.label(label("changes apply on the next start"));
    }
}

/// What newly asks for you, inside the app (`App::toasts`): a few lines
/// bottom right above the bar, each a click away from what it names, × to
/// close one; they leave on their own after ten seconds. The same lines
/// the Mac gets while Orbit is not in front, so nothing is missed either way.
fn toasts(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if app.toasts.is_empty() {
        return;
    }
    // At most two lines; several sessions on the user's turn are one line
    // that opens Sessions on them, so the toasts never cover the work.
    struct Shown {
        keys: Vec<String>,
        line: String,
        agent: bool,
        open: Option<Action>,
    }
    let turns: Vec<&crate::app::Toast> = app
        .toasts
        .iter()
        .filter(|t| t.line.starts_with("Your turn:"))
        .collect();
    let mut items: Vec<Shown> = Vec::new();
    for toast in &app.toasts {
        if turns.len() > 1 && toast.line.starts_with("Your turn:") {
            continue;
        }
        items.push(Shown {
            keys: vec![toast.key.clone()],
            line: toast.line.clone(),
            agent: toast.agent,
            open: crate::notify::action_for(&toast.key),
        });
    }
    if turns.len() > 1 {
        items.push(Shown {
            keys: turns.iter().map(|t| t.key.clone()).collect(),
            line: format!("{} sessions on your turn", turns.len()),
            agent: false,
            open: Some(Action::SessionsFiltered("your turn".into())),
        });
    }
    let shown: Vec<&Shown> = items.iter().rev().take(2).collect();
    let area = egui::Area::new(egui::Id::new("toasts"))
        .order(egui::Order::Foreground)
        .anchor(
            Align2::RIGHT_BOTTOM,
            egui::vec2(-16.0, -(bar::BOTTOM + 12.0)),
        )
        .interactable(true)
        .show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            for toast in shown.iter().rev() {
                let response = clickable(ui, |ui| {
                    Frame::new()
                        .fill(p().side)
                        // What an agent did (orbit ctl) is quieter than
                        // what needs the user: there only to follow along.
                        .stroke(Stroke::new(
                            1.0,
                            if toast.agent { p().line_2 } else { p().signal },
                        ))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let text = RichText::new(&toast.line);
                                let text = if toast.agent {
                                    text.monospace().small().color(p().text_2)
                                } else {
                                    text.color(p().text)
                                };
                                ui.add(egui::Label::new(text).truncate());
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if link(ui, label("×")).on_hover_text("Close").clicked() {
                                        actions.extend(
                                            toast.keys.iter().cloned().map(Action::DismissToast),
                                        );
                                    }
                                });
                            });
                        });
                });
                if response.clicked()
                    && let Some(action) = toast.open.clone()
                {
                    actions.push(action);
                    actions.extend(toast.keys.iter().cloned().map(Action::DismissToast));
                }
                ui.add_space(6.0);
            }
        });
    mark(app, ui, "toasts", area.response.rect);
}

/// The window's keys. With the sheet open its own come first (`bar`);
/// otherwise `i` or a view's key (0 to 7) raises it and `o` opens the PR
/// or the ticket. Cmd+Shift+T
/// cycles the theme.
fn keys(app: &App, ui: &Ui, actions: &mut Vec<Action>) {
    use egui::{Key, Modifiers};
    // herdr's prefix keys first, while the user's herdr config is in use.
    bar::herdr_keys(app, ui, actions);
    let overlay = app.palette.is_some() || app.capture.is_some() || app.tutorial.is_some();
    if app.sheet && !overlay {
        bar::keys(app, ui, actions);
    }
    // Cmd+Shift+T and the pane keys work everywhere, even in a terminal.
    // The shift variants go first: a plain Cmd shortcut also matches
    // Cmd+Shift.
    let shift = Modifiers::COMMAND | Modifiers::SHIFT;
    // A text field in focus (not a terminal): Cmd+Z is its own undo.
    let typing = ui
        .ctx()
        .memory(|m| m.focused())
        .is_some_and(|id| egui::TextEdit::load_state(ui.ctx(), id).is_some());
    let empty_tab = !overlay
        && !app.sheet
        && ui.ctx().memory(|m| m.focused()).is_none()
        && matches!(
            app.panes.surface(app.panes.active),
            Some(crate::app::panes::Surface::Ticket(_))
        )
        && app.selected.is_none()
        && app.tab != Tab::Triage
        && !(app.tab == Tab::Claude && app.session.is_some());
    let mut new_tab = false;
    ui.ctx().input_mut(|i| {
        if i.consume_key(shift, Key::T) {
            actions.push(Action::SetTheme(app.theme.next()));
        }
        if i.consume_key(shift, Key::D) {
            actions.push(Action::Split { across: false });
        } else if i.consume_key(Modifiers::COMMAND, Key::D) {
            actions.push(Action::Split { across: true });
        }
        if i.consume_key(shift, Key::Enter) {
            actions.push(Action::ZoomPane);
        }
        if i.consume_key(Modifiers::COMMAND, Key::Enter) {
            actions.push(Action::FullScreen);
        }
        // Esc leaves full screen before the terminal sees it.
        if app.full_screen && !overlay && i.consume_key(Modifiers::NONE, Key::Escape) {
            actions.push(Action::FullScreen);
        }
        if i.consume_key(shift, Key::F) {
            actions.push(Action::Improve);
        }
        // Over a browser pane, Cmd+F finds in its page.
        if i.consume_key(Modifiers::COMMAND, Key::F) {
            let focused = app.panes.focused;
            if app.panes.surface(focused) == Some(&crate::app::panes::Surface::Browser) {
                actions.push(Action::BrowserFindOpen(focused));
            } else {
                actions.push(Action::Find);
            }
        }
        if i.consume_key(Modifiers::COMMAND, Key::K) {
            actions.push(Action::TogglePalette);
        }
        if !typing && i.consume_key(Modifiers::COMMAND, Key::Z) {
            actions.push(Action::ReopenWindow);
        }
        if i.consume_key(Modifiers::COMMAND, Key::J) {
            actions.push(Action::ToggleChat);
        }
        // Cmd+N a new empty tab, Cmd+T a new ticket.
        if i.consume_key(Modifiers::COMMAND, Key::N) {
            actions.push(Action::NewTab);
            new_tab = true;
        }
        // An empty tab in front, nothing else taking keys: Enter starts
        // Claude in it, T a shell.
        if empty_tab && i.consume_key(Modifiers::NONE, Key::Enter) {
            actions.push(Action::NewSession {
                name: "Session".into(),
                folder: None,
            });
        } else if empty_tab && i.consume_key(Modifiers::NONE, Key::T) {
            actions.push(Action::ShellWindow);
        } else if empty_tab && i.consume_key(Modifiers::NONE, Key::E) {
            actions.push(Action::EditorWindow);
        } else if empty_tab && i.consume_key(Modifiers::NONE, Key::B) {
            actions.push(Action::BrowserWindow);
        }
        // The Mac's shortcut for an app's settings.
        if i.consume_key(Modifiers::COMMAND, Key::Comma) {
            actions.push(Action::Show(View::Settings));
        }
        if i.consume_key(Modifiers::COMMAND, Key::T) {
            actions.push(Action::NewTicket);
        }
        if i.consume_key(Modifiers::COMMAND, Key::W) {
            actions.push(Action::ClosePane(None));
        }
        // Cmd+Shift+] and [ step through the strip's windows, wrapping.
        // With Shift the key may arrive as } and { instead.
        if i.consume_key(shift, Key::CloseBracket) || i.consume_key(shift, Key::CloseCurlyBracket) {
            actions.push(Action::GoWindow(step(app.window, app.windows.len(), 1)));
        }
        if i.consume_key(shift, Key::OpenBracket) || i.consume_key(shift, Key::OpenCurlyBracket) {
            actions.push(Action::GoWindow(step(app.window, app.windows.len(), -1)));
        }
        if i.consume_key(Modifiers::COMMAND, Key::CloseBracket) {
            actions.push(Action::CyclePane(1));
        }
        if i.consume_key(Modifiers::COMMAND, Key::OpenBracket) {
            actions.push(Action::CyclePane(-1));
        }
        let windows = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for (n, key) in windows.into_iter().enumerate() {
            if i.consume_key(Modifiers::COMMAND, key) {
                actions.push(Action::GoWindow(n));
            }
        }
        // The next thing that needs you, without the sheet.
        if i.consume_key(Modifiers::ALT, Key::ArrowDown) {
            actions.push(Action::NextForYou);
        }
    });
    // The new tab takes the keys: Enter or T picks what goes in it, not
    // the terminal that had focus.
    if new_tab && let Some(id) = ui.ctx().memory(|m| m.focused()) {
        ui.ctx().memory_mut(|m| m.surrender_focus(id));
    }
    // Anything focused (a text field, the Claude terminal) owns the keys,
    // and so does the tutorial while it runs.
    // A GitHub PR or Linear ticket link pasted with no field focused opens
    // it here: the PR with its ticket when it has one.
    if !overlay && ui.ctx().memory(|m| m.focused().is_none()) {
        ui.ctx().input_mut(|i| {
            if let Some(at) = i.events.iter().position(
                |e| matches!(e, egui::Event::Paste(text) if crate::story::openable(text).is_some()),
            ) && let egui::Event::Paste(text) = i.events.remove(at)
                && let Some(url) = crate::story::openable(&text)
            {
                actions.push(Action::Link(url));
            }
        });
    }
    if app.sheet || overlay || ui.ctx().memory(|m| m.focused().is_some()) {
        return;
    }
    bar::view_keys(ui, actions);
    ui.ctx().input_mut(|i| {
        // Cmd+Shift+Left/Right move the current window along the strip;
        // only here, where no field or terminal wants them for selecting.
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let last = app.windows.len().saturating_sub(1);
        // Up and down too: the windows are a column now.
        if i.consume_key(shift, Key::ArrowUp) && app.window > 0 {
            actions.push(Action::MoveWindow {
                from: app.window,
                to: app.window - 1,
            });
        }
        if i.consume_key(shift, Key::ArrowDown) && app.window < last {
            actions.push(Action::MoveWindow {
                from: app.window,
                to: app.window + 1,
            });
        }
        if i.consume_key(shift, Key::ArrowLeft) && app.window > 0 {
            actions.push(Action::MoveWindow {
                from: app.window,
                to: app.window - 1,
            });
        }
        if i.consume_key(shift, Key::ArrowRight) && app.window < last {
            actions.push(Action::MoveWindow {
                from: app.window,
                to: app.window + 1,
            });
        }
        if i.consume_key(Modifiers::NONE, Key::I) {
            actions.push(Action::Sheet(true));
        }
        if app.view == View::Settings && i.consume_key(Modifiers::NONE, Key::Escape) {
            actions.push(Action::Sheet(false));
        }
        if i.consume_key(Modifiers::NONE, Key::O)
            && let Some(card) = app.selected.as_deref().and_then(|id| app.card(id))
        {
            let url = card.pr(None).map_or(&card.issue.url, |p| &p.url);
            actions.push(Action::Open(url.clone()));
        }
    });
}

/// The window `by` steps from `at` in a strip of `len`, wrapping around.
fn step(at: usize, len: usize, by: i32) -> usize {
    (at as i64 + by as i64).rem_euclid(len.max(1) as i64) as usize
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_clone_switch_counts_the_clones_at_work() {
        assert_eq!(super::clone_switch_text(0, 0, ""), "clone ⌘J");
        assert_eq!(super::clone_switch_text(2, 0, ""), "clone · 2 working ⌘J");
        assert_eq!(
            super::clone_switch_text(1, 0, " · hourly on"),
            "clone · 1 working · hourly on ⌘J"
        );
        assert_eq!(super::clone_switch_text(0, 1, ""), "clone · 1 mission ⌘J");
        assert_eq!(
            super::clone_switch_text(1, 2, ""),
            "clone · 1 working · 2 missions ⌘J"
        );
    }

    /// Every key `keys()` consumes is named in `SHORTCUTS`: the palette's
    /// list is read from the same table, so a new shortcut without a line
    /// here fails this test instead of staying unknown to the user.
    #[test]
    fn every_key_the_window_takes_is_listed_as_a_shortcut() {
        let source = include_str!("mod.rs");
        let keys = source.split("#[cfg(test)]").next().unwrap();
        let listed = super::SHORTCUTS
            .iter()
            .map(|(k, _)| k.replace(' ', ""))
            .collect::<Vec<_>>()
            .join(" ");
        fn shown(key: &str) -> &str {
            match key {
                "Enter" => "⏎",
                "Escape" => "esc",
                "Comma" => ",",
                "CloseBracket" | "CloseCurlyBracket" => "]",
                "OpenBracket" | "OpenCurlyBracket" => "[",
                "ArrowDown" => "↓",
                "ArrowUp" => "↑",
                "ArrowLeft" => "←",
                "ArrowRight" => "→",
                "Num1" => "1",
                "Num2" | "Num3" | "Num4" | "Num5" | "Num6" | "Num7" | "Num8" => "…",
                "Num9" => "9",
                other => other,
            }
        }
        let mut missing = Vec::new();
        for (at, _) in keys.match_indices("Key::") {
            let name: String = keys[at + 5..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let symbol = shown(&name);
            let lower = symbol.to_lowercase();
            if !listed.contains(symbol) && !listed.contains(&lower) {
                missing.push(name);
            }
        }
        assert!(
            missing.is_empty(),
            "keys without a shortcut line: {missing:?}"
        );
    }

    /// `src/ui/` draws and emits Actions; it never writes into `App`
    /// (CLAUDE.md). The two exceptions are a frame-local flag and the
    /// attach-once set, named here; anything else fails (herdr keeps its
    /// render pure with a scan like this one).
    #[test]
    fn the_ui_never_writes_into_the_app() {
        let sources = [
            ("bar.rs", include_str!("bar.rs")),
            ("capture.rs", include_str!("capture.rs")),
            ("diff.rs", include_str!("diff.rs")),
            ("find.rs", include_str!("find.rs")),
            ("list.rs", include_str!("list.rs")),
            ("login.rs", include_str!("login.rs")),
            ("palette.rs", include_str!("palette.rs")),
            ("panes.rs", include_str!("panes.rs")),
            ("sessions.rs", include_str!("sessions.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("testing.rs", include_str!("testing.rs")),
            ("ticket.rs", include_str!("ticket.rs")),
            ("tutorial.rs", include_str!("tutorial.rs")),
        ];
        let allowed = [
            "find.rs: app.find =",
            "sessions.rs: app.claude.auto_attached.insert(",
        ];
        let mut writes = Vec::new();
        for (file, source) in sources {
            let code = source.split("#[cfg(test)]").next().unwrap();
            for (at, _) in code.match_indices("app.") {
                let rest = &code[at..];
                let path: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
                    .collect();
                let after = rest[path.len()..].trim_start();
                let assigns = after.starts_with("= ") || after.starts_with("=\n");
                let mutates = path.rsplit('.').next().is_some_and(|m| {
                    matches!(
                        m,
                        "push" | "insert" | "remove" | "clear" | "retain" | "extend"
                    )
                }) && after.starts_with('(');
                if assigns || mutates {
                    writes.push(format!(
                        "{file}: {path}{}",
                        if mutates { "(" } else { " =" }
                    ));
                }
            }
        }
        writes.retain(|w| !allowed.contains(&w.as_str()));
        assert!(writes.is_empty(), "ui writes into the app: {writes:?}");
    }

    #[test]
    fn step_wraps_around_the_strip() {
        assert_eq!(super::step(0, 3, 1), 1);
        assert_eq!(super::step(2, 3, 1), 0);
        assert_eq!(super::step(0, 3, -1), 2);
        assert_eq!(super::step(0, 0, 1), 0);
    }
}
