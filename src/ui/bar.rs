//! The frame around the panes, multiplexer style: the windows sidebar on
//! the left, the bottom bar with the views, and the sheet that rises from it.
//!
//! - The sidebar: one row per window (`App::windows`), its number, name,
//!   what it holds and its session's state from the hooks; a live incident
//!   pinned on top. Cmd+1 to 9 switch, Cmd+N opens a new one, « collapses.
//! - The bar: `i inbox`, the views with their counts (0 to 7), and on
//!   the right what the sessions are doing and when Linear last answered.
//! - The sheet: `i` raises the current view over the lower 60% of the
//!   window, the panes above dimmed. j/k/h/l move its cursor, Enter opens
//!   what is under it as a new window, Cmd+D as a split, Esc closes.
//!
//! What the sheet lists is `entries`: the same list the keys move through
//! and the views highlight, so the cursor and the drawing never disagree.

use std::collections::HashMap;

use super::*;
use crate::app::panes::Surface;
use egui::{Id, Order, Pos2, Rect, UiBuilder};

pub(super) const BOTTOM: f32 = 34.0;
/// Below this width the view tabs show only their key and count.
const NARROW: f32 = 1200.0;
/// The sheet's share of the window's height.
const SHEET: f32 = 0.6;

/// One thing the sheet's cursor can be on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    /// A ticket, by issue id.
    Ticket(String),
    /// A Claude session, by key (`Action::OpenSession`).
    Session(String),
    Customer(String),
    /// An Inbox row, by `inbox::Item::key`.
    Note(String),
    /// A repository, `owner/repo` (the Repos view).
    Repo(String),
    /// A PR on a repo page, by URL.
    Pr(String),
}

/// The sheet's tabs: the key, the name, the view.
fn views() -> [(&'static str, String, View); 8] {
    let ktlo = crate::profile::get().ktlo.label.to_lowercase();
    [
        ("0", "notifications".into(), View::Inbox),
        ("1", "for you".into(), View::ForYou),
        ("4", "customers".into(), View::Customers),
        ("5", "sessions".into(), View::Sessions),
        ("6", "intake".into(), View::Intake),
        ("7", ktlo, View::Ktlo),
        ("8", "alerts".into(), View::Alerts),
        ("9", "repos".into(), View::Repos),
    ]
}

/// The tab a view belongs to: a story under stories, a customer under
/// customers, a repo page under repos.
fn tab_of(view: &View) -> View {
    match view {
        View::Customer(_) => View::Customers,
        View::Repo(_) => View::Repos,
        other => other.clone(),
    }
}

/// The For you columns: what the ask is, then what is assigned to you.
pub(super) const FOR_YOU: [(Option<Ask>, &str); 4] = [
    (Some(Ask::Review), "review the PR"),
    (Some(Ask::Decide), "make a call"),
    (Some(Ask::Verify), "verify"),
    (None, "assigned to you"),
];

/// What the sheet lists now, as columns of entries.
pub(super) fn entries(app: &App) -> Vec<Vec<Entry>> {
    let ticket = |c: &&Card| Entry::Ticket(c.issue.id.clone());
    match &app.view {
        View::ForYou => {
            let cards = app.visible();
            FOR_YOU
                .iter()
                .map(|(ask, _)| cards.iter().filter(|c| c.ask == *ask).map(ticket).collect())
                .collect()
        }
        View::Sessions => vec![
            sessions::items(app)
                .into_iter()
                .filter(|i| matches_all(&i.text, &app.sheet_query))
                .map(|i| Entry::Session(i.key))
                .collect(),
        ],
        View::Customers => vec![
            app.customer_list()
                .into_iter()
                .map(|(name, ..)| Entry::Customer(name))
                .collect(),
        ],
        View::Inbox => vec![
            app.inbox()
                .0
                .into_iter()
                .map(|i| Entry::Note(i.key))
                .collect(),
        ],
        View::Repos => vec![
            app.repos()
                .into_iter()
                .filter(|slug| matches_all(slug, &app.sheet_query))
                .map(Entry::Repo)
                .collect(),
        ],
        View::Repo(slug) => vec![
            app.repo_prs
                .get(slug)
                .and_then(|(_, prs)| prs.as_ref().ok())
                .map(|prs| list::repo_order(prs, &app.my_prs, &app.sheet_query))
                .unwrap_or_default()
                .into_iter()
                .map(|pr| Entry::Pr(pr.url.clone()))
                .collect(),
        ],
        View::Settings | View::Alerts | View::Stats => vec![Vec::new()],
        _ => vec![app.visible().iter().map(ticket).collect()],
    }
}

/// The cursor, kept inside what is listed now.
fn clamp(at: (usize, usize), entries: &[Vec<Entry>]) -> (usize, usize) {
    let mut column = at.0.min(entries.len().saturating_sub(1));
    // An empty column holds no cursor: land on the first one with rows.
    if entries.get(column).is_some_and(Vec::is_empty)
        && let Some(first) = entries.iter().position(|c| !c.is_empty())
    {
        column = first;
    }
    let rows = entries.get(column).map_or(0, Vec::len);
    (column, at.1.min(rows.saturating_sub(1)))
}

/// What the sheet's cursor is on, if anything.
pub(super) fn cursor(app: &App) -> Option<Entry> {
    let entries = entries(app);
    let (column, row) = clamp(app.sheet_at, &entries);
    entries.get(column)?.get(row).cloned()
}

/// The sheet's keys, while it is open: they come before the panes' own,
/// so Cmd+D splits with the ticket under the cursor.
pub(super) fn keys(app: &App, ui: &Ui, actions: &mut Vec<Action>) {
    use egui::{Key, Modifiers};
    let entries = entries(app);
    let (column, row) = clamp(app.sheet_at, &entries);
    let under = entries.get(column).and_then(|c| c.get(row)).cloned();
    let open = |split: bool| match under.clone() {
        Some(Entry::Ticket(id)) => Some(Action::OpenTicket { id, split }),
        Some(Entry::Session(key)) if !split => Some(Action::OpenSession(key)),
        Some(Entry::Customer(name)) if !split => Some(Action::Show(View::Customer(name))),
        Some(Entry::Note(key)) if !split => Some(Action::InboxOpen(key)),
        Some(Entry::Repo(slug)) if !split => Some(Action::Show(View::Repo(slug))),
        // A PR row opens as a PR card, like a Notifications row does.
        Some(Entry::Pr(url)) if !split => app
            .repo_prs
            .values()
            .filter_map(|(_, prs)| prs.as_ref().ok())
            .flatten()
            .find(|pr| pr.url == url)
            .map(|pr| Action::OpenPr {
                url: pr.url.clone(),
                title: pr.title.clone(),
                at: pr.at.clone(),
            }),
        _ => None,
    };
    ui.ctx().input_mut(|i| {
        if i.consume_key(Modifiers::COMMAND, Key::D) {
            actions.extend(open(true));
        }
    });
    // Only the sheet's search field keeps its keys; a terminal under the
    // sheet gives them up.
    let field = egui::Id::unique(SEARCH);
    match ui.ctx().memory(|m| m.focused()) {
        Some(id) if id == field => return,
        Some(_) => ui.ctx().memory_mut(|m| m.stop_text_input()),
        None => {}
    }
    let sessions = app.view == View::Sessions;
    // `/` puts the keys in the view's search, where it has one.
    if matches!(app.view, View::Sessions | View::Repos | View::Repo(_))
        && ui
            .ctx()
            .input_mut(|i| i.consume_key(Modifiers::NONE, Key::Slash))
    {
        ui.ctx().memory_mut(|m| m.request_focus(Id::unique(SEARCH)));
        return;
    }
    ui.ctx().input_mut(|i| {
        let key = |i: &mut egui::InputState, key| i.consume_key(Modifiers::NONE, key);
        if key(i, Key::Escape) || key(i, Key::I) {
            actions.push(Action::Sheet(false));
        }
        for (name, _, view) in views() {
            if let Some(k) = Key::from_name(&name.to_uppercase())
                && key(i, k)
            {
                actions.push(Action::Show(view));
            }
        }
        let moved = if key(i, Key::J) || key(i, Key::ArrowDown) {
            Some((column, row + 1))
        } else if key(i, Key::K) || key(i, Key::ArrowUp) {
            Some((column, row.saturating_sub(1)))
        } else if key(i, Key::L) || key(i, Key::ArrowRight) {
            (column + 1..entries.len())
                .find(|&c| !entries[c].is_empty())
                .map(|c| (c, row))
        } else if key(i, Key::H) || key(i, Key::ArrowLeft) {
            (0..column)
                .rev()
                .find(|&c| !entries[c].is_empty())
                .map(|c| (c, row))
        } else {
            None
        };
        if let Some(at) = moved {
            let (c, r) = clamp(at, &entries);
            actions.push(Action::SheetAt(c, r));
        }
        if key(i, Key::Enter) {
            actions.extend(open(false));
        }
        if let Some(Entry::Note(k)) = &under
            && key(i, Key::X)
        {
            actions.push(Action::InboxDone(k.clone()));
        }
        if app.view == View::ForYou
            && let Some(Entry::Ticket(id)) = &under
            && key(i, Key::X)
        {
            actions.push(Action::MarkRead(id.clone()));
        }
        if sessions && let Some(Entry::Session(k)) = &under {
            // Attach and resume both open it: a stopped one resumes.
            if key(i, Key::A) || i.consume_key(Modifiers::SHIFT, Key::R) {
                actions.push(Action::OpenSession(k.clone()));
            }
            if i.consume_key(Modifiers::SHIFT, Key::X) {
                actions.push(Action::CloseSession(k.clone()));
            }
            if key(i, Key::X) {
                actions.push(Action::StopSession(k.clone()));
            }
        }
    });
}

/// Scrolls the cursor's row into view once, when the cursor moves to it.
pub(super) fn follow(ui: &Ui, row: &egui::Response, what: &str) {
    let seen = Id::unique("sheet-cursor-seen");
    if ui.data(|d| d.get_temp::<String>(seen)).as_deref() != Some(what) {
        ui.data_mut(|d| d.insert_temp(seen, what.to_owned()));
        row.scroll_to_me(None);
    }
}

/// 0 to 7 and p with the sheet down: raise it on that view.
pub(super) fn view_keys(ui: &Ui, actions: &mut Vec<Action>) {
    ui.ctx().input_mut(|i| {
        for (name, _, view) in views() {
            if let Some(k) = egui::Key::from_name(&name.to_uppercase())
                && i.consume_key(egui::Modifiers::NONE, k)
            {
                actions.push(Action::Show(view));
            }
        }
    });
}

/// The key hints in the sheet's header, per view.
pub(super) fn hints(view: &View) -> &'static str {
    match view {
        View::ForYou => "j k move · h l column · ⏎ window · ⌘D split · esc close",
        View::Sessions => "/ search · j k move · enter open · x stop · ⇧x close · esc",
        View::Customers => "j k move · ⏎ open · esc close",
        View::Inbox => "j k move · ⏎ open, marks read · x mark read · esc close",
        View::Alerts => "click to open · esc close",
        View::Repos => "/ search · j k move · ⏎ open the repo · esc close",
        View::Repo(_) => "/ search · j k move · ⏎ open the PR here · esc close",
        _ => "j k move · ⏎ window · ⌘D split · esc close",
    }
}

/// The windows sidebar, on the left like herdr's and xirp's: one row per
/// window (its name, what it holds, its session's state), the current one
/// marked; a live incident pinned on top, the notifications above, a new
/// window at the top. Drag a row to reorder, right-click to rename, × to
/// close. `needs you first` sorts by what waits on the user (herdr's
/// priority order) instead of the user's order. Collapsed (« / »), only
/// the numbers and their states remain.
pub(super) fn sidebar(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let collapsed_id = Id::unique("sidebar-collapsed");
    let collapsed: bool = ui.data(|d| d.get_temp(collapsed_id)).unwrap_or(false);
    let panel = if collapsed {
        egui::Panel::left("sidebar-narrow").exact_size(SIDEBAR_NARROW)
    } else {
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(SIDEBAR)
            .size_range(180.0..=440.0)
    };
    let panel = panel
        .show_separator_line(false)
        .frame(Frame::new().fill(p().side))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().vline(
                full.right() - 0.5,
                full.y_range(),
                Stroke::new(1.0, p().line),
            );
            if collapsed {
                narrow(app, ui, collapsed_id, actions);
            } else {
                wide(app, ui, collapsed_id, actions);
            }
        });
    // The sheet rises over the panes only, beside the sidebar.
    let rect = panel.response.rect;
    ui.ctx()
        .data_mut(|d| d.insert_temp(Id::unique(SIDEBAR_RECT), rect));
    mark(app, ui, "strip", rect);
}

/// The sidebar's width when collapsed.
const SIDEBAR_NARROW: f32 = 46.0;
pub(super) const SIDEBAR: f32 = 248.0;
pub(super) const SIDEBAR_RECT: &str = "sidebar-rect";
/// Where the clone's chat panel sits (ui/mod.rs `clone_chat`).
pub(super) const CHAT_RECT: &str = "chat-rect";

/// How the sidebar orders the windows (`sidebar-sort` in egui's memory):
/// the user's order, what needs them first, or grouped by repository.
pub(super) const SORT_OWN: u8 = 0;
pub(super) const SORT_NEEDS: u8 = 1;
pub(super) const SORT_REPO: u8 = 2;

/// The order the rows are drawn in: the user's, what needs them first
/// (needs approval, your turn, failed, PR opened, working, the rest), or by
/// repository (herdr groups worktrees under their repo), the user's order
/// among equals.
pub(super) fn sidebar_order(states: &[Option<&str>], repos: &[String], sort: u8) -> Vec<usize> {
    let mut order: Vec<usize> = (0..states.len()).collect();
    match sort {
        SORT_NEEDS => order.sort_by_key(|&i| states[i].map_or(9, urgency)),
        SORT_REPO => order.sort_by(|&a, &b| {
            let none = |r: &String| r.is_empty();
            (none(&repos[a]), &repos[a]).cmp(&(none(&repos[b]), &repos[b]))
        }),
        _ => {}
    }
    order
}

/// The repository a window works in: its tickets' first PR's, else none.
fn window_repo(app: &App, i: usize) -> String {
    let panes = if i == app.window {
        &app.panes
    } else {
        &app.windows[i]
    };
    panes
        .ids()
        .into_iter()
        .filter_map(|id| match panes.surface(id)? {
            Surface::Ticket(_) if i == app.window && id == panes.active => {
                app.ticket_view().selected
            }
            Surface::Ticket(view) => view.selected.clone().or(view.session.clone()),
            _ => None,
        })
        .filter_map(|id| app.card(&id))
        .find_map(|card| card.prs.first().map(|pr| pr.repo.clone()))
        .unwrap_or_default()
}

/// The sidebar's sort: what the user picked, else herdr's
/// `ui.agent_panel_sort` while herdr's config is in use.
fn sort_of(app: &App, ui: &Ui) -> u8 {
    ui.data(|d| d.get_temp(Id::unique("sidebar-sort")))
        .unwrap_or_else(|| {
            let herdr = app
                .herdr_config
                .as_ref()
                .filter(|_| app.settings.herdr != Some(false));
            if herdr.is_some_and(|c| c.needs_first) {
                SORT_NEEDS
            } else {
                SORT_OWN
            }
        })
}

fn wide(app: &App, ui: &mut Ui, collapsed_id: Id, actions: &mut Vec<Action>) {
    let sort_id = Id::unique("sidebar-sort");
    let sort = sort_of(app, ui);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(RichText::new("orbit").size(13.0).family(semibold()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if link(ui, label("«"))
                .on_hover_text("Collapse the sidebar")
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(collapsed_id, true));
            }
        });
    });
    ui.add_space(6.0);
    // What asks for you, where the eye lands: the Notifications count, in
    // the signal colour while one asks something; a click opens the view.
    let (inbox, _) = app.inbox();
    let asked = inbox.iter().any(|i| i.ask <= crate::inbox::Ask::Reply);
    let (text, color) = match inbox.len() {
        0 => ("notifications".to_owned(), p().text_3),
        n if asked => (format!("● {n} notifications"), p().signal),
        n => (format!("{n} notifications"), p().text_2),
    };
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let badge = link(ui, label(&text).color(color)).on_hover_text(
            "Linear's and GitHub's notifications, only what asks something of you (0)",
        );
        mark(app, ui, "notifications-badge", badge.rect);
        if badge.clicked() {
            actions.push(Action::Show(View::Inbox));
        }
    });
    ui.add_space(8.0);
    hairline(ui);
    // The windows' header: how they are ordered, and a new one.
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(label("// windows"));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if link(ui, label("+ new"))
                .on_hover_text("A new empty window (⌘N)")
                .clicked()
            {
                actions.push(Action::NewTab);
            }
            ui.add_space(10.0);
            let (text, hint) = match sort {
                SORT_NEEDS => ("needs you first", "Next: grouped by repository"),
                SORT_REPO => ("by repo", "Next: your own order"),
                _ => ("your order", "Next: what waits on you first"),
            };
            let color = if sort == SORT_OWN {
                p().text_3
            } else {
                p().signal
            };
            if link(ui, label(text).color(color))
                .on_hover_text(hint)
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(sort_id, (sort + 1) % 3));
            }
        });
    });
    ui.add_space(4.0);
    let footer = 30.0;
    egui::ScrollArea::vertical()
        .id_salt("sidebar-windows")
        .auto_shrink(false)
        .max_height((ui.available_height() - footer).max(0.0))
        .show(ui, |ui| {
            sidebar_rows(app, ui, sort, actions);
            herdr_agents(app, ui, actions);
        });
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            // herdr's prefix waiting for its key: its mode, as herdr shows it.
            if ui.data(|d| d.get_temp::<f64>(Id::unique(PREFIX))).is_some() {
                ui.label(label("PREFIX · c new · v split · z zoom · n/p · esc").color(p().signal));
            } else {
                ui.label(label("⌘1–9 · ⌘⇧[ ] · drag to reorder"));
            }
        });
    });
}

/// herdr's agents under Orbit's windows (herdr.rs): herdr's state in
/// Orbit's words, the agent and its herdr workspace; a click brings it
/// forward in herdr and its terminal to the front.
fn herdr_agents(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if app.herdr_agents.is_empty() {
        return;
    }
    let rect = ui
        .scope(|ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(label("// in herdr"));
            });
            ui.add_space(4.0);
            let mut agents: Vec<&crate::herdr::Agent> = app.herdr_agents.iter().collect();
            agents.sort_by_key(|a| urgency(a.label()));
            for agent in agents {
                let status = agent.label();
                let response = sidebar_row(ui, false, false, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.add(
                            egui::Label::new(
                                RichText::new(agent.target())
                                    .family(medium())
                                    .color(p().text_2),
                            )
                            .truncate(),
                        );
                        ui.label(label(&agent.kind));
                    });
                    let place = [agent.workspace.as_str(), agent.title.as_str()]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .copied()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    if !place.is_empty() {
                        ui.add(egui::Label::new(label(&place)).truncate());
                    }
                    if status != "unknown" {
                        ui.horizontal(|ui| {
                            state(ui, status, window_color(status), window_color(status));
                            if status == "working" {
                                ui.add_space(4.0);
                                ui.label(label(status).color(p().success));
                            }
                        });
                    }
                });
                if response
                    .on_hover_text("Bring it forward in herdr")
                    .clicked()
                {
                    actions.push(Action::HerdrFocus(agent.target().to_owned()));
                }
            }
        })
        .response
        .rect;
    mark(app, ui, "herdr", rect);
}

/// egui's memory key for herdr's prefix: when it was pressed.
pub(super) const PREFIX: &str = "herdr-prefix";

/// herdr's keys (herdr.rs `Keys`), while the user's herdr config is in use:
/// the prefix (ctrl+b by default), then one key for one action, as in
/// herdr; direct chords too. Esc, an unbound key or two seconds drop the
/// prefix. Runs before the window's own keys, so a terminal never sees it.
pub(super) fn herdr_keys(app: &App, ui: &Ui, actions: &mut Vec<Action>) {
    let Some(config) = app
        .herdr_config
        .as_ref()
        .filter(|_| app.settings.herdr != Some(false))
    else {
        return;
    };
    let keys = &config.keys;
    let id = Id::unique(PREFIX);
    let now = ui.input(|i| i.time);
    let pending = ui
        .data(|d| d.get_temp::<f64>(id))
        .filter(|at| now - at < 2.0);
    let mut action: Option<(String, Option<usize>)> = None;
    let mut prefix_now = pending.is_some();
    let mut drop_prefix = false;
    ui.ctx().input_mut(|i| {
        let mut taken = None;
        for (n, event) in i.events.iter().enumerate() {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let Some(name) = key_name(*key) else {
                continue;
            };
            let chord = crate::herdr::Chord {
                prefix: prefix_now,
                key: name.to_owned(),
                ctrl: modifiers.ctrl,
                alt: modifiers.alt,
                // `?` comes with shift on most layouts; herdr writes it bare.
                shift: modifiers.shift && name != "?",
                cmd: modifiers.mac_cmd,
            };
            let is_prefix = !prefix_now
                && keys.prefix.iter().any(|p| {
                    p.key == chord.key
                        && (p.ctrl, p.alt, p.shift, p.cmd)
                            == (chord.ctrl, chord.alt, chord.shift, chord.cmd)
                });
            if is_prefix {
                prefix_now = true;
                taken = Some(n);
                break;
            }
            if let Some((name, index)) = keys.action(&chord) {
                action = Some((name.to_owned(), index));
                taken = Some(n);
                break;
            }
            if prefix_now {
                // Esc or a key with no action ends the prefix, as in herdr.
                drop_prefix = true;
                taken = Some(n);
                break;
            }
        }
        if let Some(n) = taken {
            i.events.remove(n);
        }
    });
    if action.is_some() || drop_prefix {
        ui.data_mut(|d| d.remove::<f64>(id));
    } else if prefix_now && pending.is_none() {
        ui.data_mut(|d| d.insert_temp(id, now));
    } else if !prefix_now {
        ui.data_mut(|d| d.remove::<f64>(id));
    }
    if prefix_now {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    let Some((name, index)) = action else {
        return;
    };
    let windows = app.windows.len();
    let step = |by: i64| ((app.window as i64 + by).rem_euclid(windows.max(1) as i64)) as usize;
    let sidebar = Id::unique("sidebar-collapsed");
    match name.as_str() {
        "help" => actions.push(Action::PaletteQuery("shortcut ".into())),
        "settings" => actions.push(Action::Show(View::Settings)),
        "new_tab" | "new_workspace" => actions.push(Action::NewTab),
        "workspace_picker" => actions.push(Action::Show(View::Sessions)),
        "goto" => actions.push(Action::TogglePalette),
        "open_notification_target" => {
            if let Some(action) = app
                .toasts
                .last()
                .and_then(|t| crate::notify::action_for(&t.key))
            {
                actions.push(action);
            }
        }
        "rename_tab" => actions.push(Action::PaletteQuery("Rename this tab: ".into())),
        "previous_tab" => actions.push(Action::GoWindow(step(-1))),
        "next_tab" => actions.push(Action::GoWindow(step(1))),
        "switch_tab" => actions.extend(index.map(Action::GoWindow)),
        "close_tab" => actions.push(Action::CloseWindow(app.window)),
        "focus_pane_left" | "focus_pane_up" | "cycle_pane_previous" => {
            actions.push(Action::CyclePane(-1))
        }
        "focus_pane_right" | "focus_pane_down" | "cycle_pane_next" => {
            actions.push(Action::CyclePane(1))
        }
        "split_vertical" => actions.push(Action::Split { across: true }),
        "split_horizontal" => actions.push(Action::Split { across: false }),
        "close_pane" => actions.push(Action::ClosePane(None)),
        "zoom" => actions.push(Action::ZoomPane),
        "toggle_sidebar" => {
            let collapsed: bool = ui.data(|d| d.get_temp(sidebar)).unwrap_or(false);
            ui.data_mut(|d| d.insert_temp(sidebar, !collapsed));
        }
        _ => {}
    }
}

/// herdr's name for a key (`c`, `1`, `minus`, `tab`, `?`).
fn key_name(key: egui::Key) -> Option<&'static str> {
    use egui::Key as K;
    Some(match key {
        K::Minus => "minus",
        K::Tab => "tab",
        K::Questionmark | K::Slash => "?",
        K::OpenBracket => "[",
        K::CloseBracket => "]",
        K::Enter => "enter",
        K::Escape => "esc",
        K::Space => "space",
        K::ArrowUp => "up",
        K::ArrowDown => "down",
        K::ArrowLeft => "left",
        K::ArrowRight => "right",
        K::A => "a",
        K::B => "b",
        K::C => "c",
        K::D => "d",
        K::E => "e",
        K::F => "f",
        K::G => "g",
        K::H => "h",
        K::I => "i",
        K::J => "j",
        K::K => "k",
        K::L => "l",
        K::M => "m",
        K::N => "n",
        K::O => "o",
        K::P => "p",
        K::Q => "q",
        K::R => "r",
        K::S => "s",
        K::T => "t",
        K::U => "u",
        K::V => "v",
        K::W => "w",
        K::X => "x",
        K::Y => "y",
        K::Z => "z",
        K::Num0 => "0",
        K::Num1 => "1",
        K::Num2 => "2",
        K::Num3 => "3",
        K::Num4 => "4",
        K::Num5 => "5",
        K::Num6 => "6",
        K::Num7 => "7",
        K::Num8 => "8",
        K::Num9 => "9",
        _ => return None,
    })
}

fn sidebar_rows(app: &App, ui: &mut Ui, sort: u8, actions: &mut Vec<Action>) {
    let reorder = sort == SORT_OWN;
    // A live incident is pinned on top, in red.
    if let Some(incident) = app.incidents.first() {
        let response = sidebar_row(ui, false, false, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("■ {}", incident.reference))
                        .family(medium())
                        .color(p().danger),
                )
                .truncate(),
            );
            ui.add(egui::Label::new(label(&incident.name).color(p().danger)).truncate());
        });
        let hint = format!("Live incident: open {}", crate::profile::get().ktlo.label);
        if response.on_hover_text(hint).clicked() {
            actions.push(Action::Show(View::Ktlo));
        }
    }
    let windows: Vec<_> = (0..app.windows.len())
        .map(|i| {
            let (name, status, next) = window(app, i);
            (name, status, window_parts(app, i), next)
        })
        .collect();
    let states: Vec<Option<&str>> = windows.iter().map(|(_, s, ..)| *s).collect();
    let repos: Vec<String> = (0..app.windows.len())
        .map(|i| window_repo(app, i))
        .collect();
    let order = sidebar_order(&states, &repos, sort);
    let mut group: Option<&str> = None;
    // The current window came into view once, when it became so.
    let seen = Id::unique("strip-window-seen");
    let changed = ui.data(|d| d.get_temp::<usize>(seen)) != Some(app.window);
    // A row dragged along the list: which one, and every row's place, for
    // where it would land. Only in the user's own order.
    let dragging_id = Id::unique("strip-dragging");
    let dragging: Option<usize> = ui.data(|d| d.get_temp(dragging_id));
    let mut rects: Vec<Rect> = Vec::new();
    for &i in &order {
        let (name, status, parts, next) = &windows[i];
        // By repo: a heading where the repository changes.
        if sort == SORT_REPO && group != Some(repos[i].as_str()) {
            group = Some(repos[i].as_str());
            let heading = if repos[i].is_empty() {
                "no repo"
            } else {
                repos[i].as_str()
            };
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(label(&format!("// {heading}")));
            });
        }
        let current = i == app.window && app.view != View::Settings;
        let response = sidebar_row(ui, current, reorder, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(label(&(i + 1).to_string()));
                // An alert investigation's window reads as one, in red; the clone's in green.
                let marked = ["ALERT ·", "INCIDENT ·", "RELEASE ·"];
                let color = if marked.iter().any(|m| name.starts_with(m)) {
                    p().danger
                } else if name.starts_with("CLONE ·") {
                    // The clone's own run, in a window: good news, not alarm.
                    p().success
                } else if current {
                    p().text
                } else {
                    p().text_2
                };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Every ticket in it is done: suggest closing it.
                    let done = window_done(app, i);
                    let hovered = ui.rect_contains_pointer(ui.max_rect());
                    if done || hovered || current {
                        let (close, hint) = if done {
                            (
                                label("done ×").color(p().success),
                                "Its ticket is done: close this window; its sessions keep running",
                            )
                        } else {
                            (label("×"), "Close this window; its sessions keep running")
                        };
                        if link(ui, close).on_hover_text(hint).clicked() {
                            actions.push(Action::CloseWindow(i));
                        }
                    }
                    // The next move, where done would be: a finished session
                    // says what is left on its ticket.
                    if let Some(next) = next.filter(|_| !done) {
                        ui.label(label(next).color(p().signal))
                            .on_hover_text("Your next move on this ticket");
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(name).family(medium()).color(color))
                                .truncate(),
                        );
                    });
                });
            });
            if let Some(parts) = parts {
                ui.add(egui::Label::new(label(parts)).truncate());
            }
            if let Some(status) = status {
                ui.horizontal(|ui| {
                    state(ui, status, window_color(status), window_color(status));
                    if matches!(*status, "working" | "starting") {
                        ui.add_space(4.0);
                        ui.label(label(status).color(p().success));
                    }
                });
            }
        });
        if i == app.window && changed {
            response.scroll_to_me(Some(Align::Center));
            if ui.clip_rect().contains_rect(response.rect) {
                ui.data_mut(|d| d.insert_temp(seen, i));
            }
        }
        rects.push(response.rect);
        if response.drag_started() && reorder {
            ui.data_mut(|d| d.insert_temp(dragging_id, i));
        }
        if response.middle_clicked() {
            actions.push(Action::CloseWindow(i));
        }
        // Right-click: rename it, or move it up or down.
        let open = response
            .context_menu(|ui| tab_menu(app, i, name, ui, actions))
            .is_some();
        // The menu closed however (Enter, Escape, a click elsewhere): a
        // name typed in it is saved, never lost.
        let typed = Id::unique(("rename-window", i));
        if !open
            && let Some(text) = ui.data_mut(|d| d.remove_temp::<String>(typed))
            && text.trim() != name.trim()
        {
            actions.push(Action::RenameWindow(i, text.trim().to_owned()));
        }
        let hint = if i < 9 {
            format!("Window {} (Cmd+{})", i + 1, i + 1)
        } else {
            format!("Window {}", i + 1)
        };
        if response.on_hover_text(hint).clicked() {
            actions.push(Action::GoWindow(i));
            if app.view == View::Settings {
                actions.push(Action::Sheet(false));
            }
        }
    }
    // While a row is dragged: a 2px marker where it would land, the list
    // scrolls at its edges, and on release it moves there.
    if let Some(from) = dragging.filter(|&f| f < rects.len() && reorder)
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
    {
        let centers: Vec<f32> = rects.iter().map(|r| r.center().y).collect();
        let slot = centers.iter().filter(|&&c| c < pointer.y).count();
        let y = match slot {
            0 => rects[0].top(),
            n => rects[n - 1].bottom(),
        };
        ui.painter()
            .hline(rects[0].x_range(), y, Stroke::new(2.0, p().signal));
        let clip = ui.clip_rect();
        if pointer.y < clip.top() + 30.0 {
            ui.scroll_with_delta(egui::vec2(0.0, 12.0));
        } else if pointer.y > clip.bottom() - 30.0 {
            ui.scroll_with_delta(egui::vec2(0.0, -12.0));
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if ui.input(|i| i.pointer.any_released()) {
            ui.data_mut(|d| d.remove::<usize>(dragging_id));
            let to = drop_index(slot, from);
            if to != from {
                actions.push(Action::MoveWindow { from, to });
            }
        }
    } else if dragging.is_some() {
        ui.data_mut(|d| d.remove::<usize>(dragging_id));
    }
    if app.view == View::Settings {
        let response = sidebar_row(ui, true, false, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("settings").family(medium()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(label("×"));
                });
            });
        });
        if response.on_hover_text("Close Settings (Esc)").clicked() {
            actions.push(Action::Sheet(false));
        }
    }
}

/// Collapsed: each window's number and state, the current one marked.
fn narrow(app: &App, ui: &mut Ui, collapsed_id: Id, actions: &mut Vec<Action>) {
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        if link(ui, label("»"))
            .on_hover_text("Show the sidebar")
            .clicked()
        {
            ui.data_mut(|d| d.insert_temp(collapsed_id, false));
        }
        ui.add_space(8.0);
        let (inbox, _) = app.inbox();
        if !inbox.is_empty() {
            let badge = link(ui, label(&format!("● {}", inbox.len())).color(p().signal))
                .on_hover_text("Notifications (0)");
            if badge.clicked() {
                actions.push(Action::Show(View::Inbox));
            }
            ui.add_space(8.0);
        }
        if link(ui, label("+"))
            .on_hover_text("New window (⌘N)")
            .clicked()
        {
            actions.push(Action::NewTab);
        }
        ui.add_space(6.0);
    });
    for i in 0..app.windows.len() {
        let (name, status, _) = window(app, i);
        let current = i == app.window && app.view != View::Settings;
        let response = sidebar_row(ui, current, false, |ui| {
            ui.vertical_centered(|ui| {
                let color = if current { p().text } else { p().text_2 };
                ui.label(
                    RichText::new((i + 1).to_string())
                        .family(medium())
                        .color(color),
                );
                if let Some(status) = status {
                    state_dot(ui, status, window_color(status));
                }
            });
        });
        if response.on_hover_text(name).clicked() {
            actions.push(Action::GoWindow(i));
        }
    }
}

/// What a window holds, when its name does not say it already: its panes'
/// names in order ("ABC-12 · shell").
fn window_parts(app: &App, i: usize) -> Option<String> {
    let panes = if i == app.window {
        &app.panes
    } else {
        &app.windows[i]
    };
    app.window_title(i)?;
    let names: Vec<String> = panes
        .ids()
        .into_iter()
        .map(|id| pane_name(app, panes, i == app.window, id).0)
        .collect();
    Some(names.join(" · "))
}

/// Where a row dragged from `from` lands when dropped at gap `slot` (0 is
/// before the first row, `n` after the last): its own gaps keep it put.
fn drop_index(slot: usize, from: usize) -> usize {
    if slot > from { slot - 1 } else { slot }
}

/// A window's right-click menu: a name of the user's own (empty: the
/// automatic one again), and moving it one place up or down.
fn tab_menu(app: &App, i: usize, name: &str, ui: &mut Ui, actions: &mut Vec<Action>) {
    let id = Id::unique(("rename-window", i));
    let mut text: String = ui
        .data(|d| d.get_temp(id))
        .unwrap_or_else(|| name.to_owned());
    ui.label(label("rename (empty: automatic)"));
    let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    if !field.lost_focus() {
        field.request_focus();
    }
    if enter {
        // Saved as the menu closes (the sidebar row does it).
        ui.data_mut(|d| d.insert_temp(id, text));
        ui.close();
        return;
    }
    ui.data_mut(|d| d.insert_temp(id, text));
    ui.separator();
    if i > 0 && ui.button("Move up (⌘⇧↑)").clicked() {
        actions.push(Action::MoveWindow { from: i, to: i - 1 });
        ui.close();
    }
    if i + 1 < app.windows.len() && ui.button("Move down (⌘⇧↓)").clicked() {
        actions.push(Action::MoveWindow { from: i, to: i + 1 });
        ui.close();
    }
}

/// One row of the sidebar: hairline under it, the selection fill and a
/// 2px marker on the left for the current one.
fn sidebar_row(
    ui: &mut Ui,
    current: bool,
    drag: bool,
    add: impl FnOnce(&mut Ui),
) -> egui::Response {
    let sense = if drag {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let response = clickable_with(ui, sense, |ui| {
        let mut frame = Frame::new().inner_margin(Margin {
            left: 14,
            right: 10,
            top: 7,
            bottom: 7,
        });
        if current {
            frame = frame.fill(p().sel);
        }
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            add(ui);
        });
    });
    let r = response.rect;
    if current {
        let marker = Rect::from_x_y_ranges(r.left()..=r.left() + 2.0, r.y_range());
        ui.painter().rect_filled(marker, 0.0, p().signal);
    }
    ui.painter()
        .hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p().line));
    hover_hand(&response)
}

/// A window's name and its session's state: every pane's ticket, session
/// or shell, in order ("shell | ABC-12"), and the state of the one that
/// needs the user most.
fn window(app: &App, i: usize) -> (String, Option<&'static str>, Option<&'static str>) {
    let panes = if i == app.window {
        &app.panes
    } else {
        &app.windows[i]
    };
    let named: Vec<(String, Option<&'static str>, Option<&'static str>)> = panes
        .ids()
        .into_iter()
        .map(|id| pane_name(app, panes, i == app.window, id))
        .collect();
    let status = named
        .iter()
        .filter_map(|(_, s, _)| *s)
        .min_by_key(|s| urgency(s));
    // The first pane with a move for the user names it for the window.
    let next = named.iter().find_map(|(_, _, n)| *n);
    // A fixed title or Haiku's name for the whole window, else its panes'.
    if let Some(title) = app.window_title(i) {
        return (title, status, next);
    }
    let names: Vec<&str> = named.iter().map(|(n, ..)| n.as_str()).collect();
    (strip_name(&names), status, next)
}

/// Every pane in window `i` shows a ticket that is done (a shell or an
/// empty pane is not).
fn window_done(app: &App, i: usize) -> bool {
    let panes = if i == app.window {
        &app.panes
    } else {
        &app.windows[i]
    };
    let stages: Vec<Option<Stage>> = panes
        .ids()
        .into_iter()
        .map(|id| match panes.surface(id) {
            Some(Surface::Ticket(_)) if i == app.window && id == panes.active => {
                app.ticket_view().selected
            }
            Some(Surface::Ticket(view)) => view.selected.clone(),
            _ => None,
        })
        .map(|id| id.as_deref().and_then(|id| app.card(id)).map(|c| c.stage))
        .collect();
    all_done(&stages)
}

fn all_done(stages: &[Option<Stage>]) -> bool {
    !stages.is_empty() && stages.iter().all(|s| *s == Some(Stage::Done))
}

/// The panes' names in one tab: three at most, then how many more.
fn strip_name(names: &[&str]) -> String {
    let mut name = names[..names.len().min(3)].join(" | ");
    if names.len() > 3 {
        name.push_str(&format!(" +{}", names.len() - 3));
    }
    name
}

/// Which state the strip shows for a window of several panes: what waits on
/// the user first.
fn urgency(status: &str) -> u8 {
    match status {
        "needs approval" => 0,
        "your turn" => 1,
        "failed" => 2,
        "PR opened" => 3,
        "working" => 4,
        "merging" => 5,
        _ => 6,
    }
}

/// One pane's name and its session's state.
fn pane_name(
    app: &App,
    panes: &crate::app::panes::Panes,
    current: bool,
    id: u64,
) -> (String, Option<&'static str>, Option<&'static str>) {
    let view = match panes.surface(id) {
        Some(Surface::Ticket(_)) if current && id == panes.active => app.ticket_view(),
        Some(Surface::Ticket(view)) => view.clone(),
        Some(Surface::Editor) => return ("editor".into(), None, None),
        // A browser by the site it shows.
        Some(Surface::Browser) => {
            let url = app.browsers.get(&id).map_or("", |b| b.current());
            return (crate::browser::site(url), None, None);
        }
        _ => {
            let title = app.shell_titles.get(&id);
            let state = app.shell_states.get(&id).copied();
            return (
                title.map_or("shell", String::as_str).to_owned(),
                state,
                None,
            );
        }
    };
    let card = view.selected.as_deref().and_then(|id| app.card(id));
    let key = view.session.as_deref().or(view.selected.as_deref());
    let name = match (
        card,
        view.session.as_deref().filter(|_| view.tab == Tab::Claude),
    ) {
        (None, Some(key)) => app
            .claude
            .saved
            .get(key)
            .map_or_else(|| key.to_owned(), |s| s.identifier.clone()),
        // Haiku's tab name beside the id, when the window has room for it.
        (Some(card), _) => tab_name(&card.issue.identifier, app.tab_title(card), panes.single()),
        (None, None) if view.tab == Tab::Triage => "triage".into(),
        (None, None) => "new".into(),
    };
    // Its PR in the merge queue: the window says merging, as the ticket does.
    let merging = card.is_some_and(|c| {
        c.prs
            .iter()
            .any(|pr| pr.open && matches!(app.gates.get(&pr.url), Some(Ok(g)) if g.queued))
    });
    let status = pane_status(key.and_then(|k| session_state(app, k)), merging);
    // What the user does next on it, unless its session is still at it.
    let next = card
        .filter(|_| !matches!(status, Some("working" | "starting" | "merging")))
        .and_then(|c| {
            let gate = c
                .pr(None)
                .and_then(|pr| app.gates.get(&pr.url)?.as_ref().ok());
            next_action(c, gate)
        });
    (name, status, next)
}

/// The user's next move on a ticket, in two words, the way they work it
/// (activity.jsonl): comments get fixed, a red PR fixed, an approved
/// clean one merged, one waiting on them approved, a merged one verified.
/// Nothing while it waits on someone else (checks running, the bot).
pub(super) fn next_action(
    card: &crate::story::Card,
    gate: Option<&crate::github::Gate>,
) -> Option<&'static str> {
    if let Some(g) = gate.filter(|g| g.state == "OPEN" && !g.queued) {
        let red = matches!(g.checks.as_str(), "FAILURE" | "ERROR");
        return match () {
            _ if g.open_threads > 0 => Some("fix comments"),
            _ if g.review == "CHANGES_REQUESTED" => Some("fix changes"),
            _ if red => Some("fix CI"),
            _ if g.branch == "DIRTY" => Some("fix conflicts"),
            _ if g.green => Some("merge"),
            _ if g.can_auto_merge => Some("merge when ready"),
            _ if g.can_approve => Some("approve"),
            _ if g.branch == "BEHIND" => Some("update branch"),
            _ if g.own && g.review == "REVIEW_REQUIRED" => Some("ask for review"),
            _ => None,
        };
    }
    match card.ask {
        Some(crate::story::Ask::Verify) => Some("verify"),
        Some(crate::story::Ask::Decide) => Some("decide"),
        Some(crate::story::Ask::Review) => Some("review"),
        None => None,
    }
}

/// What a pane's row says: what waits on the user or runs comes first,
/// then a PR in the merge queue, then the session's quieter states.
fn pane_status(session: Option<&'static str>, merging: bool) -> Option<&'static str> {
    match session {
        Some(s) if urgency(s) <= urgency("working") => Some(s),
        _ if merging => Some("merging"),
        other => other,
    }
}

/// A ticket pane's name: its id, with Haiku's short name beside it when
/// the pane has the window to itself.
fn tab_name(identifier: &str, short: Option<&str>, single: bool) -> String {
    match short.filter(|_| single) {
        Some(short) => format!("{identifier} {short}"),
        None => identifier.to_owned(),
    }
}

/// A session's state as the strip says it: the hooks' report first (a PR
/// opened, a failure), then what Claude Code says.
fn session_state(app: &App, key: &str) -> Option<&'static str> {
    use crate::claude::status::State;
    if app.claude.starting.contains(key) {
        return Some("starting");
    }
    let row = app.claude.row_of(key)?;
    if row.failure.is_some() {
        return Some("failed");
    }
    if !row.running() {
        return None;
    }
    let reported = row.report.as_ref().filter(|r| r.recent()).map(|r| r.state);
    match (reported, row.label()) {
        (Some(State::Failed), _) => Some("failed"),
        (_, "working") => Some("working"),
        (_, "needs approval") => Some("needs approval"),
        (_, "your turn") => Some("your turn"),
        (Some(State::Pr), _) => Some("PR opened"),
        _ => None,
    }
}

fn window_color(status: &str) -> Color32 {
    match status {
        "working" | "starting" => p().success,
        "PR opened" => p().text,
        "merging" => p().signal,
        other => status_color(other),
    }
}

/// The bottom bar: the sheet's toggle and tabs, and the board's pulse.
pub(super) fn bottom(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let narrow = ui.ctx().content_rect().width() < NARROW;
    let panel = egui::Panel::bottom("bar")
        .exact_size(BOTTOM)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(p().side))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter()
                .hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, p().line));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(6.0);
                let open = app.sheet;
                let inbox = bar_tab(ui, open, |ui| {
                    ui.label(label("i").color(p().signal));
                    ui.add_space(6.0);
                    ui.label(RichText::new("inbox").color(if open {
                        p().text
                    } else {
                        p().text_2
                    }));
                });
                mark(app, ui, "inbox", inbox.rect);
                if inbox
                    .on_hover_text("Raise or lower the sheet (i)")
                    .clicked()
                {
                    actions.push(Action::Sheet(!open));
                }
                ui.add_space(8.0);
                let counts = counts(app);
                let tabs = ui
                    .scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (key, name, view) in views() {
                            let (n, loud, red) = counts.get(key).copied().unwrap_or_default();
                            let current = open && tab_of(&app.view) == view;
                            let response = bar_tab(ui, current, |ui| {
                                ui.label(label(key));
                                if !narrow {
                                    ui.add_space(6.0);
                                    let color = if current { p().text } else { p().text_2 };
                                    ui.label(RichText::new(&name).color(color));
                                }
                                if n > 0 {
                                    ui.add_space(6.0);
                                    let color = if loud { p().signal } else { p().text_3 };
                                    ui.label(label(&n.to_string()).color(color));
                                }
                                if red {
                                    ui.add_space(5.0);
                                    dot(ui, p().danger);
                                }
                            });
                            if response.on_hover_text(format!("{name} ({key})")).clicked() {
                                actions.push(if current {
                                    Action::Sheet(false)
                                } else {
                                    Action::Show(view)
                                });
                            }
                        }
                    })
                    .response
                    .rect;
                mark(app, ui, "views", tabs);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(12.0);
                    let settings = link(ui, label("⚙ settings")).on_hover_text("Settings (⌘,)");
                    mark(app, ui, "settings", settings.rect);
                    if settings.clicked() {
                        actions.push(settings_click(&app.view));
                    }
                    ui.add_space(14.0);
                    if link(ui, label("⌘K"))
                        .on_hover_text("Search and every action")
                        .clicked()
                    {
                        actions.push(Action::TogglePalette);
                    }
                    ui.add_space(14.0);
                    let feedback = link(ui, label("feedback"))
                        .on_hover_text("Tell Claude what to improve; it opens a PR (⌘⇧F)");
                    mark(app, ui, "feedback", feedback.rect);
                    if feedback.clicked() {
                        actions.push(Action::Improve);
                    }
                    ui.add_space(14.0);
                    let synced = if app.loading && app.loaded_at.is_none() {
                        "syncing".to_owned()
                    } else {
                        app.loaded_at
                            .as_deref()
                            .map_or(String::new(), |at| format!("synced {}", story::ago(at)))
                    };
                    if link(ui, label(&synced))
                        .on_hover_text("Linear is asked every 20 seconds. Click to ask now.")
                        .clicked()
                    {
                        actions.push(Action::Refresh);
                    }
                    let (working, turn) = pulse(app);
                    ui.add_space(14.0);
                    // Each count opens the Sessions view searched for that
                    // state, so the list behind the number is one click.
                    let color = if turn > 0 { p().signal } else { p().text_3 };
                    if link(ui, label(&format!("{turn} your turn")).color(color))
                        .on_hover_text("The sessions waiting on you, in Sessions")
                        .clicked()
                    {
                        actions.push(Action::SessionsFiltered("your turn".into()));
                    }
                    ui.add_space(10.0);
                    let color = if working > 0 { p().success } else { p().text_3 };
                    // A spinner while any works, beside how many.
                    if working > 0 {
                        ui.add(egui::Spinner::new().size(11.0).color(p().success));
                        ui.add_space(4.0);
                    }
                    let count = link(ui, label(&format!("{working}")).color(color))
                        .on_hover_text("The sessions at work, in Sessions");
                    if count.clicked() {
                        actions.push(Action::SessionsFiltered("working".into()));
                    }
                    mark(app, ui, "working", count.rect);
                    // What went wrong last, where the eye goes after a click.
                    if let Some(error) = app.error.as_ref().or(app.claude.error.as_ref()) {
                        ui.add_space(14.0);
                        ui.add(egui::Label::new(label(error).color(p().danger)).truncate())
                            .on_hover_text(error);
                    }
                });
            });
        });
    mark(app, ui, "bar", panel.response.rect);
}

/// The bar's settings link: opens Settings, or closes it when it is open.
fn settings_click(view: &View) -> Action {
    if *view == View::Settings {
        Action::Sheet(false)
    } else {
        Action::Show(View::Settings)
    }
}

/// One tab of the bottom bar.
fn bar_tab(ui: &mut Ui, current: bool, add: impl FnOnce(&mut Ui)) -> egui::Response {
    let response = clickable(ui, |ui| {
        let mut frame = Frame::new().inner_margin(Margin::symmetric(9, 6));
        if current {
            frame = frame.fill(p().sel);
        }
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                add(ui);
            });
        });
    });
    if current {
        let r = response.rect;
        let marker = Rect::from_x_y_ranges(r.x_range(), r.top()..=r.top() + 2.0);
        ui.painter().rect_filled(marker, 0.0, p().signal);
    }
    hover_hand(&response)
}

/// Sessions working, and sessions waiting on you.
fn pulse(app: &App) -> (usize, usize) {
    let rows = app.claude.rows.values().filter(|r| r.running());
    rows.fold((0, 0), |(w, t), r| match r.label() {
        "working" => (w + 1, t),
        "your turn" | "needs approval" => (w, t + 1),
        _ => (w, t),
    })
}

/// Per tab key: its count, whether that count is waiting on you (the
/// signal colour), and whether it carries a red dot (a live incident).
fn counts(app: &App) -> HashMap<&'static str, (usize, bool, bool)> {
    let product: Vec<&Card> = app
        .cards
        .iter()
        .filter(|c| c.lane == story::Lane::Product)
        .collect();
    let count = |f: &dyn Fn(&Card) -> bool| product.iter().filter(|c| f(c)).count();
    let ktlo = app.ktlo().count();
    let now = jiff::Timestamp::now();
    let stuck = app.cards.iter().any(|c| story::unhandled(c, now));
    let running = app.claude.rows.values().filter(|r| r.running()).count();
    let approvals = app
        .claude
        .rows
        .values()
        .filter(|r| r.label() == "needs approval")
        .count();
    let mine = count(&|c| app.for_you_shows(c));
    let asks = count(&|c| c.ask.is_some());
    let (inbox, _) = app.inbox();
    let asked = inbox.iter().any(|i| i.ask <= crate::inbox::Ask::Reply);
    let firing = app.alerts.iter().filter(|a| a.firing()).count();
    let red = app
        .releases
        .iter()
        .filter(|(_, run)| matches!(run, Ok(Some(run)) if run.failed()))
        .count();
    HashMap::from([
        ("0", (inbox.len(), asked, false)),
        ("1", (mine, asks > 0, false)),
        ("4", (app.customer_list().len(), false, false)),
        ("5", (running, approvals > 0, false)),
        ("6", (count(&|c| c.untriaged()), false, false)),
        (
            "7",
            (
                ktlo + app.incidents.len(),
                !app.incidents.is_empty() || stuck,
                !app.incidents.is_empty() || stuck,
            ),
        ),
        // Firing alerts and red releases: red while any is.
        ("8", (firing + red, firing + red > 0, firing + red > 0)),
    ])
}

/// The sheet, over the lower part of the window while it is open; the
/// panes above it are dimmed, and a click there lowers it.
pub(super) fn sheet(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if !app.sheet || app.view == View::Settings {
        return;
    }
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    // Beside the sidebar: the windows stay in view while the sheet is up.
    let left = ctx
        .data(|d| d.get_temp::<Rect>(Id::unique(SIDEBAR_RECT)))
        .map_or(screen.left(), |r| r.right());
    // And beside the clone's chat on the right, while it is open.
    let right = ctx
        .data(|d| d.get_temp::<Rect>(Id::unique(CHAT_RECT)))
        .filter(|_| app.chat_open)
        .map_or(screen.right(), |r| r.left());
    let bottom = screen.bottom() - BOTTOM;
    let top = (bottom - screen.height() * SHEET).max(screen.top()).round();
    let above = Rect::from_min_max(Pos2::new(left, screen.top()), Pos2::new(right, top));
    let rect = Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom));

    egui::Area::new(Id::unique("sheet-scrim"))
        .order(Order::Middle)
        .fixed_pos(above.min)
        .show(&ctx, |ui| {
            let response = ui.allocate_rect(above, Sense::click());
            ui.painter().rect_filled(above, 0.0, theme::scrim());
            if response.clicked() {
                actions.push(Action::Sheet(false));
            }
        });
    egui::Area::new(Id::unique("sheet"))
        .order(Order::Middle)
        .fixed_pos(rect.min)
        .show(&ctx, |ui| {
            ui.painter().rect_filled(rect, 0.0, p().bg);
            ui.painter().hline(
                rect.x_range(),
                rect.top() + 0.5,
                Stroke::new(1.0, p().line_2),
            );
            let mut inner = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            inner.set_clip_rect(rect);
            inner.set_min_size(rect.size());
            list::show(app, &mut inner, actions);
            // The area is as big as what its own ui holds, and egui sends
            // the wheel to the layer under the pointer: without this the
            // sheet is 0x0 to it and the panes below scroll instead.
            ui.expand_to_include_rect(rect);
            mark(app, ui, "sheet", rect);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_ticket_tab_carries_haikus_name() {
        assert_eq!(
            tab_name("ABC-1", Some("Login loop"), true),
            "ABC-1 Login loop"
        );
        assert_eq!(tab_name("ABC-1", Some("Login loop"), false), "ABC-1");
        assert_eq!(tab_name("ABC-1", None, true), "ABC-1");
    }

    #[test]
    fn a_window_suggests_closing_only_when_every_ticket_in_it_is_done() {
        use crate::story::Stage;
        assert!(all_done(&[Some(Stage::Done)]));
        assert!(all_done(&[Some(Stage::Done), Some(Stage::Done)]));
        assert!(!all_done(&[Some(Stage::Done), None]), "a shell beside it");
        assert!(!all_done(&[Some(Stage::Done), Some(Stage::Review)]));
        assert!(!all_done(&[]));
    }

    #[test]
    fn the_cursor_stays_on_what_is_listed() {
        let t = |id: &str| Entry::Ticket(id.into());
        let entries = vec![vec![t("a"), t("b"), t("c")], vec![], vec![t("d")]];
        // Past the end of a column: its last row.
        assert_eq!(clamp((0, 9), &entries), (0, 2));
        // Into an empty column: the first column with rows.
        assert_eq!(clamp((1, 2), &entries), (0, 2));
        // Past the last column: the last one.
        assert_eq!(clamp((7, 0), &entries), (2, 0));
        assert_eq!(clamp((0, 0), &[]), (0, 0));
        // The first column empty (For you, nothing to review): the next one.
        let entries = vec![vec![], vec![t("a"), t("b")]];
        assert_eq!(clamp((0, 0), &entries), (1, 0));
    }

    #[test]
    fn a_split_window_names_every_pane_and_what_waits_on_you() {
        assert_eq!(strip_name(&["shell"]), "shell");
        assert_eq!(strip_name(&["shell", "ABC-12"]), "shell | ABC-12");
        assert_eq!(strip_name(&["a", "b", "c", "d", "e"]), "a | b | c +2");
        // Your turn wins over the merge queue; the queue over an idle session.
        assert_eq!(
            super::pane_status(Some("your turn"), true),
            Some("your turn")
        );
        assert_eq!(super::pane_status(Some("idle"), true), Some("merging"));
        assert_eq!(super::pane_status(None, true), Some("merging"));
        assert_eq!(super::pane_status(Some("idle"), false), Some("idle"));
        let mut states = ["working", "your turn", "needs approval"];
        states.sort_by_key(|s| urgency(s));
        assert_eq!(states[0], "needs approval");
    }

    #[test]
    fn needs_you_first_puts_what_waits_on_the_user_on_top_and_keeps_order_among_equals() {
        let states = [
            Some("working"),
            None,
            Some("your turn"),
            Some("needs approval"),
            Some("your turn"),
        ];
        let repos = vec![String::new(); 5];
        let order = |sort| super::sidebar_order(&states, &repos, sort);
        assert_eq!(order(super::SORT_OWN), [0, 1, 2, 3, 4]);
        assert_eq!(order(super::SORT_NEEDS), [3, 2, 4, 0, 1]);
        // By repo: grouped by name, windows without one last, user order within.
        let repos: Vec<String> = ["web", "", "api", "web", "api"].map(String::from).to_vec();
        assert_eq!(
            super::sidebar_order(&states, &repos, super::SORT_REPO),
            [2, 4, 0, 3, 1]
        );
    }

    #[test]
    fn the_bars_settings_link_opens_settings_and_closes_it_again() {
        assert!(matches!(
            super::settings_click(&View::ForYou),
            Action::Show(View::Settings)
        ));
        assert!(matches!(
            super::settings_click(&View::Settings),
            Action::Sheet(false)
        ));
    }

    #[test]
    fn a_dragged_tab_lands_in_the_gap_it_is_dropped_on() {
        // Four tabs, the second (1) dragged.
        assert_eq!(super::drop_index(0, 1), 0, "before the first");
        assert_eq!(super::drop_index(1, 1), 1, "its own left gap: stays");
        assert_eq!(super::drop_index(2, 1), 1, "its own right gap: stays");
        assert_eq!(super::drop_index(3, 1), 2);
        assert_eq!(super::drop_index(4, 1), 3, "after the last");
    }

    #[test]
    fn every_view_tab_has_its_own_key() {
        let keys: Vec<&str> = views().iter().map(|(k, ..)| *k).collect();
        // The bar holds what asks for action; Stats opens from Cmd+K and Settings.
        // 2 was Plan my day, removed; the others keep their keys.
        assert_eq!(keys, ["0", "1", "4", "5", "6", "7", "8", "9"]);
        assert!(views().iter().all(|(_, _, v)| *v != View::Stats));
        for (key, ..) in views() {
            assert!(egui::Key::from_name(&key.to_uppercase()).is_some(), "{key}");
        }
    }

    #[test]
    fn a_window_says_the_next_move_the_way_the_user_works_a_ticket() {
        use crate::github::Gate;
        let card = crate::story::card(
            crate::story::tests::issue("In Review", "started", vec![]),
            &Default::default(),
        );
        let gate = |f: fn(&mut Gate)| {
            let mut g = Gate {
                state: "OPEN".into(),
                checks: "SUCCESS".into(),
                review: "REVIEW_REQUIRED".into(),
                branch: "BLOCKED".into(),
                ..Default::default()
            };
            f(&mut g);
            g
        };
        let next = |g: &Gate| super::next_action(&card, Some(g));
        assert_eq!(next(&gate(|g| g.open_threads = 2)), Some("fix comments"));
        assert_eq!(next(&gate(|g| g.checks = "FAILURE".into())), Some("fix CI"));
        assert_eq!(
            next(&gate(|g| g.branch = "DIRTY".into())),
            Some("fix conflicts")
        );
        assert_eq!(next(&gate(|g| g.green = true)), Some("merge"));
        assert_eq!(
            next(&gate(|g| g.can_auto_merge = true)),
            Some("merge when ready")
        );
        assert_eq!(next(&gate(|g| g.can_approve = true)), Some("approve"));
        // Waiting on others, or already on its way in: nothing to do.
        assert_eq!(next(&gate(|g| g.checks = "PENDING".into())), None);
        assert_eq!(
            next(&gate(|g| {
                g.green = true;
                g.queued = true
            })),
            None
        );
    }
}
