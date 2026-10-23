//! The command palette window (Cmd+K, app/palette.rs): type, move with the
//! arrows, Enter runs the picked line, Esc closes. What it shows is built
//! fresh from the app every frame, so it is never stale.

use super::*;
use crate::app::palette::{self, Item};

const SHOWN: usize = 14;
const PER_GROUP: usize = 4;

pub(super) fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(query) = app.palette.as_ref().map(|p| p.query.clone()) else {
        return;
    };
    // The board's matches, then Linear's (tickets not on the board), then
    // the lines that turn the query into something new: none crowds out
    // the others.
    // What the user ran lately and often, and what waits on them, first.
    let now = crate::activity::now_ms() as u64;
    let picks = &app.palette_picks;
    let asks = |id: &str| app.card(id).is_some_and(|c| c.ask.is_some());
    let recent = palette::recent(picks);
    let ranked = palette::rank_by(
        &query,
        palette::items(app),
        |item| palette::lift(picks, now, asks, item),
        &recent,
    );
    let (linear, board): (Vec<Item>, Vec<Item>) = ranked
        .into_iter()
        .partition(|item| item.section == "linear");
    // At most a few per group, so one kind (sessions, say) never fills
    // the list: tickets, PRs and actions keep their place.
    let mut per: std::collections::HashMap<&str, usize> = Default::default();
    let mut results: Vec<Item> = board
        .into_iter()
        .filter(|item| {
            let n = per.entry(item.section).or_default();
            *n += 1;
            query.trim().is_empty() || *n <= PER_GROUP
        })
        .take(SHOWN)
        .collect();
    // Grouped as ranked: each group where its best line ranks.
    let order: Vec<&str> = results.iter().fold(Vec::new(), |mut seen, item| {
        if !seen.contains(&item.section) {
            seen.push(item.section);
        }
        seen
    });
    results.sort_by_key(|item| order.iter().position(|s| *s == item.section));
    // Whatever was typed can always become a ticket or a session: these
    // come before Linear's results, so they are never pushed off the end.
    let scoped = app.palette.as_ref().and_then(|p| p.repo.clone());
    let tab_repo = palette::repo_for(&query, &app.my_prs);
    let typed = query.trim();
    // A pasted PR or ticket link: opening it is the first line.
    if let Some(url) = crate::story::openable(typed) {
        let name = crate::story::pr_from_url(&url, "")
            .map(|p| format!("{}#{}", p.repo, p.number))
            .or_else(|| crate::story::issue_from_url(&url))
            .unwrap_or_else(|| url.clone());
        results.insert(
            0,
            Item {
                section: "open",
                title: format!("Open {name}"),
                detail: "Here in Orbit: the PR with its Linear ticket when it has one".into(),
                keys: "⏎",
                also: "",
                text: String::new(),
                actions: vec![Action::Link(url)],
            },
        );
    }
    if !typed.is_empty() && scoped.is_none() {
        results.push(Item {
            section: "create",
            title: format!("Create ticket: {typed}"),
            detail: "New ticket, with this as the note".into(),
            keys: "⌘T",
            also: "",
            text: String::new(),
            actions: vec![Action::NewTicketWith(typed.to_owned())],
        });
        let goal = typed
            .strip_prefix("Mission:")
            .unwrap_or(typed)
            .trim()
            .to_owned();
        results.push(Item {
            section: "create",
            title: format!("Mission: {goal}"),
            detail: "Your clone researches it, files a parent with sub-tickets, drives them through the bot, tells you what is yours".into(),
            keys: "",
            also: "",
            text: String::new(),
            actions: vec![Action::Instruct(goal)],
        });
        let (name, folder) = palette::new_session_parts(typed);
        results.push(Item {
            section: "create",
            title: format!("New session: {name}"),
            detail: match &folder {
                Some(folder) => format!("A Claude session with no ticket, in {folder}"),
                None => "A Claude session with no ticket, in its own folder (add \"in ~/path\" to pick one)".into(),
            },
            keys: "",
            also: "",
            text: String::new(),
            actions: vec![Action::NewSession { name, folder }],
        });
        // herdr's rename key opens Cmd+K on "Rename this tab: ": the name is
        // what follows it.
        let new_name = typed
            .strip_prefix("Rename this tab:")
            .unwrap_or(typed)
            .trim()
            .to_owned();
        results.push(Item {
            section: "create",
            title: format!("Rename this tab: {new_name}"),
            detail: "Names the current window in the strip".into(),
            keys: "",
            also: "",
            text: String::new(),
            actions: vec![Action::RenameWindow(app.window, new_name)],
        });
    }

    if scoped.is_none() {
        results.extend(linear.into_iter().take(4));
    }
    let ctx = ui.ctx().clone();
    // Each session line's agent, read before the palette's state is borrowed.
    let agents: Vec<crate::claude::Agent> = results
        .iter()
        .map(|item| match palette::kind(item) {
            palette::Kind::Session(key) => app.claude.agent_of(&key),
            _ => app.claude.default_agent,
        })
        .collect();
    let Some(state) = app.palette.as_mut() else {
        return;
    };
    // The arrows move the pick, not the cursor; Enter runs it, Esc closes.
    let (down, up, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    // Tab on a repo's name lists only your PRs in it; Backspace on an
    // empty field goes back to everything.
    let (tab, back) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
            state.query.is_empty() && i.key_pressed(egui::Key::Backspace),
        )
    });
    if tab && state.repo.is_none() && tab_repo.is_some() {
        state.repo = tab_repo;
        state.query.clear();
        state.selected = 0;
    }
    if back && state.repo.is_some() {
        state.repo = None;
        state.selected = 0;
    }
    let last = results.len().saturating_sub(1);
    if down {
        state.selected = (state.selected + 1).min(last);
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    state.selected = state.selected.min(last);
    let mut run = None;

    egui::Area::new(egui::Id::unique("palette"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
        .show(&ctx, |ui| {
            Frame::new()
                .fill(p().bg)
                .stroke(Stroke::new(1.0, p().signal))
                .show(ui, |ui| {
                    ui.set_width(640.0);
                    Frame::new()
                        .inner_margin(Margin::symmetric(PAD, 12))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let hint = match &state.repo {
                                    Some(repo) => {
                                        ui.label(
                                            RichText::new(format!("{repo} ›"))
                                                .family(egui::FontFamily::Monospace)
                                                .color(p().signal),
                                        );
                                        "your open PRs here · backspace: everything".to_owned()
                                    }
                                    None => "Search tickets, sessions, stories, or type a command \
                                             · repo + tab: your PRs"
                                        .to_owned(),
                                };
                                let field = ui.add(
                                    egui::TextEdit::singleline(&mut state.query)
                                        .hint_text(hint)
                                        .font(egui::FontId::proportional(17.0))
                                        .frame(Frame::NONE)
                                        .desired_width(f32::INFINITY),
                                );
                                field.request_focus();
                            });
                        });
                    hairline(ui);
                    egui::ScrollArea::vertical()
                        .max_height(470.0)
                        .show(ui, |ui| {
                            let mut section = "";
                            for (i, item) in results.iter().enumerate() {
                                if item.section != section {
                                    // Each group apart: a hairline, then its name.
                                    if !section.is_empty() {
                                        ui.add_space(4.0);
                                        hairline(ui);
                                    }
                                    section = item.section;
                                    Frame::new()
                                        .inner_margin(Margin {
                                            left: PAD,
                                            right: PAD,
                                            top: 8,
                                            bottom: 2,
                                        })
                                        .show(ui, |ui| {
                                            ui.label(label(&format!(
                                                "// {}",
                                                palette::section_name(section)
                                            )));
                                        });
                                }
                                let kind = palette::kind(item);
                                let picked = i == state.selected;
                                let response = row(ui, picked, p().signal, false, |ui| {
                                    let line = egui::vec2(ui.available_width(), 18.0);
                                    ui.allocate_ui_with_layout(
                                        line,
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            if !item.keys.is_empty() {
                                                ui.label(label(item.keys));
                                            }
                                            // An action says it runs, a view that it goes.
                                            match kind {
                                                palette::Kind::Action => {
                                                    ui.label(label("run"));
                                                }
                                                palette::Kind::Go => {
                                                    ui.label(label("go"));
                                                }
                                                _ => {}
                                            }
                                            ui.with_layout(
                                                Layout::left_to_right(Align::Center),
                                                |ui| {
                                                    ui.spacing_mut().item_spacing.x = 8.0;
                                                    kind_mark(ui, &kind, agents[i]);
                                                    let title =
                                                        RichText::new(&item.title).family(medium());
                                                    ui.add(egui::Label::new(title).truncate());
                                                },
                                            );
                                        },
                                    );
                                    if !item.detail.is_empty() {
                                        ui.add(egui::Label::new(label(&item.detail)).truncate());
                                    }
                                });
                                if picked && (down || up) {
                                    response.scroll_to_me(None);
                                }
                                if response.clicked() {
                                    run = Some(i);
                                }
                            }
                            if results.is_empty() {
                                pad(ui, |ui| {
                                    ui.label(label("nothing matches"));
                                });
                            }
                        });
                    hairline(ui);
                    Frame::new()
                        .inner_margin(Margin::symmetric(PAD, 6))
                        .show(ui, |ui| {
                            ui.label(label("↑↓ move · enter run · esc close · ⌘K anywhere"));
                        });
                });
        });

    if enter && !results.is_empty() {
        run = Some(state.selected);
    }
    if let Some(i) = run {
        actions.push(Action::TogglePalette);
        let item = results.swap_remove(i);
        actions.push(Action::PalettePicked(item.title));
        actions.extend(item.actions);
    } else if escape {
        actions.push(Action::TogglePalette);
    }
}

/// The mark before a palette line: Linear's or GitHub's logo, the
/// session's agent, an arrow for a view, a key for a shortcut, and
/// Orbit's ▸ for something it does.
fn kind_mark(ui: &mut Ui, kind: &palette::Kind, agent: crate::claude::Agent) {
    let glyph = |ui: &mut Ui, text: &str, color| {
        ui.add_sized(
            egui::vec2(13.0, 13.0),
            egui::Label::new(RichText::new(text).monospace().color(color)),
        );
    };
    match kind {
        palette::Kind::Linear => {
            ui.add(linear_mark(ui));
        }
        palette::Kind::GitHub => {
            ui.add(github_mark(ui));
        }
        palette::Kind::Session(_) => {
            ui.add(agent_icon(ui, agent));
        }
        palette::Kind::Go => glyph(ui, "→", p().text_3),
        palette::Kind::Key => glyph(ui, "⌘", p().text_3),
        palette::Kind::Action => glyph(ui, "▸", p().text_2),
    }
}
