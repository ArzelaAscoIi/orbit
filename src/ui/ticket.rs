//! The right column: one ticket, read top to bottom the way a reviewer
//! needs it. Where it is on the bot's rail, what it needs from you, where it
//! came from, what was planned, and what happened, with the raw ticket last.

use super::*;

/// A ticket pane: draws `view` into `ui`, which `panes` sizes. `busy` is
/// the pane that already draws this view's session, if another does.
pub(super) fn show(
    app: &mut App,
    pane: u64,
    view: &TicketView,
    busy: Option<u64>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    // A copy, so the composer below can borrow the draft mutably.
    let card = view
        .selected
        .as_deref()
        .and_then(|id| app.card(id))
        .cloned();
    ui.scope(|ui| {
        // The triage session: the tab of its own window.
        let triage = view.tab == Tab::Triage;
        tabs(app, pane, view, card.as_ref(), triage, ui, actions);
        Frame::new()
            .inner_margin(Margin::symmetric(28, 20))
            .show(ui, |ui| {
                if triage && view.tab == Tab::Triage {
                    sessions::pane(app, crate::claude::TRIAGE, busy, ui, actions);
                    return;
                }
                let session = view.session.clone().filter(|_| view.tab == Tab::Claude);
                if let Some(key) = session {
                    sessions::pane(app, &key, busy, ui, actions);
                    return;
                }
                let Some(card) = card else {
                    // An empty tab: what to put in it.
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.35);
                        ui.label(RichText::new("An empty tab").color(p().text_2));
                        ui.add_space(10.0);
                        let rect = ui
                            .horizontal(|ui| {
                                let width = 390.0;
                                ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                                if primary(ui, "Claude")
                                    .on_hover_text("A Claude session with no ticket, in this tab")
                                    .clicked()
                                {
                                    actions.push(Action::NewSession {
                                        name: "Session".into(),
                                        folder: None,
                                    });
                                }
                                if ui
                                    .button("Shell")
                                    .on_hover_text("Your login shell, in this tab")
                                    .clicked()
                                {
                                    actions.push(Action::ShellWindow);
                                }
                                if ui
                                    .button("Editor")
                                    .on_hover_text("A plain text editor, in this tab")
                                    .clicked()
                                {
                                    actions.push(Action::EditorWindow);
                                }
                                if crate::browser::available()
                                    && ui
                                        .button("Browser")
                                        .on_hover_text("A web page, in this tab: test a UI here")
                                        .clicked()
                                {
                                    actions.push(Action::BrowserWindow);
                                }
                                if ui
                                    .button("New ticket")
                                    .on_hover_text("File one (⌘T)")
                                    .clicked()
                                {
                                    actions.push(Action::NewTicket);
                                }
                            })
                            .response
                            .rect;
                        mark(app, ui, "empty-tab", rect);
                        ui.add_space(8.0);
                        ui.label(label(
                            "⏎ Claude · T shell · E editor · B browser · ⌘T new ticket · ⌥↓ the next ticket that needs you",
                        ));
                    });
                    return;
                };
                if view.tab == Tab::Diff
                    && let Some(pr) = card.pr(view.diff_pr.as_deref())
                {
                    diff::show(app, &card, pr, ui, actions);
                    return;
                }
                if view.tab == Tab::Testing && card.has_testing() {
                    testing::show(app, &card, ui, actions);
                    return;
                }
                // The full pane: the scroll bar sits on its edge, not
                // beside a narrower column.
                let mut area = egui::ScrollArea::vertical().auto_shrink(false);
                // Screenshots only (app/review.rs): start scrolled down.
                if let Some(y) = crate::app::review::scroll() {
                    area = area.vertical_scroll_offset(y);
                }
                area.show(ui, |ui| {
                    title_block(app, &card, ui, actions);
                    mission(app, &card, ui, actions);
                    ui.add_space(14.0);
                    let rect = ui.scope(|ui| rail(ui, card.stage)).response.rect;
                    mark(app, ui, "rail", rect);
                    ui.add_space(16.0);
                    let rect = ui
                        .scope(|ui| your_move(app, &card, ui, actions))
                        .response
                        .rect;
                    mark(app, ui, "your-move", rect);
                    quick_actions(app, &card, ui, actions);
                    // Its parent and sub-tickets first: before a long description.
                    if card.issue.parent.is_some() || !card.issue.children.nodes.is_empty() {
                        let rect = ui.scope(|ui| family(&card, ui, actions)).response.rect;
                        mark(app, ui, "sub-tickets", rect);
                    }
                    // What the ticket is about, as filed, images and all; a
                    // PR with no ticket shows its own description, folded.
                    let pr = card.pr_only();
                    if let Some(description) = card
                        .issue
                        .description
                        .as_deref()
                        .filter(|d| !d.trim().is_empty())
                    {
                        section(ui, if pr { "The PR" } else { "The ticket" });
                        let head = if pr {
                            "Description"
                        } else {
                            "Original description"
                        };
                        let rect =
                            egui::CollapsingHeader::new(RichText::new(head).color(p().text_2))
                                .id_salt(("description", &card.issue.id))
                                .default_open(!pr)
                                .show(ui, |ui| markdown_images(app, ui, description, actions))
                                .header_response
                                .rect;
                        mark(app, ui, "description", rect);
                    }
                    // A PR with no ticket has no Linear side to show.
                    if pr {
                        ui.add_space(24.0);
                        return;
                    }
                    let rect = ui.scope(|ui| origin(&card, ui, actions)).response.rect;
                    mark(app, ui, "origin", rect);
                    if let Some(ask) = card.plan.as_ref().and_then(|p| story::the_ask(&p.body)) {
                        let rect = ui.scope(|ui| the_ask(&ask, ui, actions)).response.rect;
                        mark(app, ui, "the-ask", rect);
                    }
                    plan(&card, ui);
                    timeline(&card, &app.viewer, ui);
                    let rect = ui
                        .scope(|ui| composer(&mut app.draft, ui, actions))
                        .response
                        .rect;
                    mark(app, ui, "composer", rect);
                    ui.add_space(24.0);
                });
            });
    });
}

/// A pane's header, one slim row: the ticket's id, its tabs (the triage
/// session's first while it is open, then walk, diff, claude, test), and on
/// the right its stage and its PR with CI.
fn tabs(
    app: &mut App,
    pane: u64,
    view: &TicketView,
    card: Option<&Card>,
    triage: bool,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let session_dot = |app: &App, key: &str| {
        app.claude
            .live_of(key)
            .map(|_| p().success)
            .or_else(|| app.claude.saved.contains_key(key).then_some(p().text_3))
    };
    let mut entries: Vec<(Tab, &str, Option<Color32>)> = Vec::new();
    if triage {
        entries.push((
            Tab::Triage,
            "✳ triage",
            session_dot(app, crate::claude::TRIAGE),
        ));
    }
    if let Some(card) = card {
        entries.push((Tab::Walkthrough, "walk", None));
        if !card.prs.is_empty() {
            entries.push((Tab::Diff, "diff", None));
        }
        let agent = app.claude.agent_of(&card.issue.id).name();
        entries.push((Tab::Claude, agent, session_dot(app, &card.issue.id)));
        if card.has_testing() {
            entries.push((Tab::Testing, "test", None));
        }
    }
    // Text-only tabs; the open one is underlined.
    let mut underline = None;
    let bar = header_bar(ui, PANE_BAR, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(12.0);
        if let Some(card) = card {
            ui.label(label(&card.issue.identifier).color(p().text));
            ui.add_space(6.0);
        }
        for (tab, name, dot_color) in entries {
            let selected = view.tab == tab
                && (tab != Tab::Claude
                    || card
                        .is_some_and(|c| view.session.as_deref().is_none_or(|k| k == c.issue.id)));
            let response = clickable(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(9, 0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let color = if selected { p().text } else { p().text_2 };
                            ui.label(RichText::new(name).family(medium()).color(color));
                            if let Some(color) = dot_color {
                                dot(ui, color);
                            }
                        });
                    });
            });
            if selected {
                underline = Some(response.rect.x_range());
            }
            if tab == Tab::Testing {
                mark(app, ui, "testing-tab", response.rect);
            }
            if hover_hand(&response).clicked() {
                actions.push(Action::Tab(tab));
            }
        }
        // A session with no ticket names itself where the tabs would be.
        if card.is_none()
            && let Some(key) = view.session.as_deref().filter(|_| view.tab == Tab::Claude)
        {
            let name = app
                .claude
                .saved
                .get(key)
                .map(|s| format!("{}  {}", s.identifier, s.title))
                .unwrap_or_else(|| key.to_owned());
            ui.add(egui::Label::new(RichText::new(name).family(medium())).truncate());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            clear_of_clone(ui);
            ui.add_space(10.0);
            // Beside other panes, a way to close this one.
            if !app.panes.single() {
                if link(ui, label("×").size(14.0))
                    .on_hover_text("Close the pane (Cmd+W)")
                    .clicked()
                {
                    actions.push(Action::ClosePane(Some(pane)));
                }
                ui.add_space(10.0);
            }
            super::panes::screen_keys(app.full_screen, &app.panes, pane, ui, actions);
            let Some(card) = card else {
                return;
            };
            // The same ticket again beside this one: Claude here, the diff
            // or walkthrough there (Cmd+D).
            let split = link(ui, label("split")).on_hover_text(
                "This ticket again beside this pane, e.g. Claude next to the diff (Cmd+D)",
            );
            mark(app, ui, "split-ticket", split.rect);
            if split.clicked() {
                actions.push(Action::FocusPane(pane));
                actions.push(Action::Split { across: true });
            }
            ui.add_space(10.0);
            if let Some(pr) = card.pr(view.diff_pr.as_deref()) {
                match card.ci {
                    Ci::Green => {
                        ui.label(label("ci green").color(p().success));
                    }
                    Ci::Red => {
                        ui.label(label("ci red").color(p().danger));
                    }
                    Ci::Unknown => {}
                }
                ui.add_space(8.0);
                // Mergeable or not, in its colour; else GitHub's state.
                let verdict = app
                    .gates
                    .get(&pr.url)
                    .and_then(|g| g.as_ref().ok())
                    .and_then(crate::github::verdict);
                match verdict {
                    Some(((t, _), _)) => {
                        let word = if t == crate::github::Tone::Good {
                            "mergeable"
                        } else {
                            "not mergeable"
                        };
                        ui.label(label(word).color(tone(t)));
                        ui.add_space(6.0);
                        ui.label(label(&format!("{}#{}", pr.repo, pr.number)));
                    }
                    None => {
                        let state = app.pr_state(pr).label();
                        ui.label(label(&format!("{}#{} {state}", pr.repo, pr.number)));
                    }
                }
                // The others are on the ticket's walkthrough and the Diff tab.
                if card.prs.len() > 1 {
                    ui.add_space(6.0);
                    ui.label(label(&format!("+{} more", card.prs.len() - 1)))
                        .on_hover_text("This ticket has several PRs: each is on the walkthrough");
                }
                ui.add_space(8.0);
            }
            let color = if card.stage == Stage::Blocked {
                p().danger
            } else {
                p().text_2
            };
            ui.label(label(&card.stage.label().to_lowercase()).color(color));
        });
    });
    mark(app, ui, "tabs", bar);
    // Over the bar's hairline, which sits just under the bar.
    if let Some(x) = underline {
        let rect = egui::Rect::from_x_y_ranges(x, bar.bottom() - 1.0..=bar.bottom() + 1.0);
        ui.painter().rect_filled(rect, 0.0, p().text);
    }
}

fn title_block(app: &App, card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    // Buttons first, right to left, so the truncated story name only gets
    // the width that is left over.
    let row = egui::vec2(ui.available_width(), 30.0);
    ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
        if card.pr_only() {
            if ui
                .button("GitHub ↗")
                .on_hover_text("Open the PR on GitHub")
                .clicked()
            {
                actions.push(Action::Open(card.issue.url.clone()));
            }
            copy_link(ui, &card.issue.url);
        } else {
            if linear_button(ui, "Linear ↗")
                .on_hover_text("Open the ticket in Linear")
                .clicked()
            {
                actions.push(Action::Open(card.issue.url.clone()));
            }
            let rect = copy_link(ui, &card.issue.url).rect;
            mark(app, ui, "copy-link", rect);
            // A ComboBox reserves only `interact_size.y`, centred, and grows
            // down from there; at a button's height it centres like the
            // button beside it.
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = button_height(ui);
                state_picker(app, card, ui, actions);
            });
            yolo(app, card, ui, actions);
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(label(&card.issue.identifier));
            // A PR with no ticket was opened by its author; a ticket by its creator.
            let gate = app.gates.get(&card.issue.url).and_then(|g| g.as_ref().ok());
            let by = match (card.pr_only(), &card.issue.creator) {
                (true, _) => gate.filter(|g| !g.author.is_empty()).map(|g| {
                    (
                        story::who(&g.author, g.own, g.author_bot),
                        g.author.as_str(),
                    )
                }),
                (false, Some(c)) => Some((
                    story::who(&c.name, c.name == app.viewer, false),
                    c.name.as_str(),
                )),
                (false, None) => None,
            };
            if let Some((who, name)) = by {
                let rect = opened_by(ui, who, name).rect;
                mark(app, ui, "opened-by", rect);
            }
            if card.story_key != "loose" {
                ui.label(label("·"));
                ui.add(egui::Label::new(RichText::new(&card.story).color(p().text_2)).truncate());
            }
        });
    });
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            RichText::new(&card.issue.title)
                .size(22.0)
                .family(semibold()),
        )
        .wrap(),
    );
}

/// Yolo: the ticket goes straight to the bot's team in Todo, the bot's go,
/// from any team and state. Not on one the bot already has, or one done.
fn yolo(app: &App, card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(bot) = crate::profile::get()
        .orchestrator
        .as_ref()
        .map(|o| o.team.clone())
    else {
        return;
    };
    if !yolo_fits(card, &bot) {
        return;
    }
    if app.dispatching.contains(&card.issue.identifier) {
        ui.label(label(&format!("yolo → {bot} Todo")));
        return;
    }
    let button = ui.button("Yolo").on_hover_text(format!(
        "Straight to {bot} Todo: the bot plans and builds it, no triage first"
    ));
    mark(app, ui, "yolo", button.rect);
    if button.clicked() {
        actions.push(Action::Yolo(card.issue.id.clone()));
    }
}

/// A ticket Yolo can hand over: a real ticket, not done, not already
/// with the bot (its team, in Todo or later).
fn yolo_fits(card: &Card, bot_team: &str) -> bool {
    let kind = card.issue.state.kind.as_str();
    let team = card.issue.identifier.split('-').next().unwrap_or_default();
    !card.pr_only()
        && !matches!(kind, "completed" | "canceled")
        && !(team == bot_team && matches!(kind, "unstarted" | "started"))
}

fn state_picker(app: &App, card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    let states = app
        .states
        .get(&card.issue.team.id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let picker = egui::ComboBox::from_id_salt(("state", &card.issue.id))
        .selected_text(&card.issue.state.name)
        .show_ui(ui, |ui| {
            for state in states {
                if ui
                    .selectable_label(state.id == card.issue.state.id, &state.name)
                    .clicked()
                {
                    actions.push(Action::SetState {
                        issue_id: card.issue.id.clone(),
                        state_id: state.id.clone(),
                    });
                }
            }
        });
    mark(app, ui, "state", picker.response.rect);
    // Another team: ENG to WEB, API, and so on, in the same kind of state.
    let current = app
        .all_teams
        .iter()
        .find(|(id, ..)| *id == card.issue.team.id)
        .map_or("team", |(_, key, _)| key.as_str());
    let teams = egui::ComboBox::from_id_salt(("team", &card.issue.id))
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (id, key, name) in &app.all_teams {
                let here = *id == card.issue.team.id;
                if ui
                    .selectable_label(here, format!("{key}  {name}"))
                    .clicked()
                    && !here
                {
                    actions.push(Action::MoveTeam {
                        issue_id: card.issue.id.clone(),
                        team_id: id.clone(),
                    });
                }
            }
        });
    let teams = teams
        .response
        .on_hover_text("Move the ticket to another team; it keeps its kind of state");
    mark(app, ui, "move-team", teams.rect);
}

/// Triage → Plan → Build → Review → Test → Done: six columns, each a 3px
/// bar over `01 triage`. Done steps dim, the current one in signal.
fn rail(ui: &mut Ui, stage: Stage) {
    let width = ui.available_width().min(860.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), Sense::hover());
    let painter = ui.painter();
    let current = stage.step();
    let gap = 3.0;
    let col = (rect.width() - gap * (STEPS.len() - 1) as f32) / STEPS.len() as f32;
    for (i, name) in STEPS.iter().enumerate() {
        let left = rect.left() + i as f32 * (col + gap);
        let name = if i == current && stage == Stage::Blocked {
            "Blocked"
        } else {
            name
        };
        let (bar, text) = if i < current {
            (p().text_2, p().text_2)
        } else if i == current {
            let bar = if stage == Stage::Blocked {
                p().danger
            } else {
                p().signal
            };
            (bar, p().text)
        } else {
            (p().control, p().text_3)
        };
        let top = egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(col, 3.0));
        painter.rect_filled(top, 0.0, bar);
        // A narrow pane has room for the numbers only, and the current name.
        let full = format!("{:02} {}", i + 1, name.to_lowercase());
        let fits = full.len() as f32 * 7.0 <= col;
        let shown = if fits || i == current {
            full
        } else {
            format!("{:02}", i + 1)
        };
        painter
            .with_clip_rect(egui::Rect::from_min_size(
                egui::pos2(left, rect.top()),
                egui::vec2(if i == current { col * 2.0 } else { col }, rect.height()),
            ))
            .text(
                egui::pos2(left, rect.top() + 10.0),
                Align2::LEFT_TOP,
                shown,
                FontId::monospace(11.0),
                text,
            );
    }
}

fn your_move(app: &App, card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    let (stroke, strip, title) = match card.ask {
        Some(ask) => (
            p().signal,
            p().signal_line,
            ask_title(ask, &card.prs).to_owned(),
        ),
        None if app.is_mine(card) => (
            p().line_2,
            p().line,
            format!("Your ticket · {}", card.stage.label()),
        ),
        None => (
            p().line_2,
            p().line,
            format!("Nothing for you · {}", card.stage.label()),
        ),
    };
    Frame::new()
        .stroke(Stroke::new(1.0, stroke))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The strip: what this box is, and how long it has waited.
            let strip_rect = Frame::new()
                .inner_margin(Margin::symmetric(PAD, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let kicker = match card.ask {
                            Some(_) => label("▲ your move").color(p().signal),
                            None => label("// status"),
                        };
                        ui.label(kicker);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let since = story::ago(&card.issue.updated_at);
                            let word = if card.ask.is_some() {
                                "waiting"
                            } else {
                                "updated"
                            };
                            ui.label(label(&format!("{word} {since}")));
                        });
                    });
                })
                .response
                .rect;
            ui.painter().hline(
                strip_rect.x_range(),
                strip_rect.bottom(),
                Stroke::new(1.0, strip),
            );
            Frame::new()
                .inner_margin(Margin::symmetric(PAD, 14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(title).size(17.0).family(semibold()));
                    if !card.why.is_empty() {
                        let color = if card.ci == Ci::Red {
                            p().danger
                        } else {
                            p().text_2
                        };
                        ui.add(egui::Label::new(RichText::new(&card.why).color(color)).wrap());
                    }
                    ui.add_space(8.0);
                    // Several PRs get a row each; one sits with the buttons.
                    // Each says first whether it can be merged.
                    let many = card.prs.len() > 1;
                    if many {
                        for pr in &card.prs {
                            // An open one: its state, name and facts, then its
                            // buttons under them; a merged or closed one: a row.
                            if pr.open && !pr.merged {
                                ui.horizontal(|ui| {
                                    state_column(app, pr, ui);
                                    pr_verdict(app, pr, true, true, ui);
                                });
                                ui.horizontal_wrapped(|ui| {
                                    ui.add_space(STATE_COLUMN + ui.spacing().item_spacing.x);
                                    pr_buttons(app, card, pr, false, ui, actions);
                                });
                                ui.add_space(6.0);
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    pr_buttons(app, card, pr, true, ui, actions)
                                });
                            }
                        }
                        ui.add_space(4.0);
                    } else if let Some(pr) = card.prs.first() {
                        pr_verdict(app, pr, false, true, ui);
                        ui.add_space(4.0);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if !many && let Some(pr) = card.prs.first() {
                            pr_buttons(app, card, pr, true, ui, actions);
                        }
                        if closes_here(card, app.is_mine(card)) {
                            mark_done(app, card, "Mark done", ui, actions);
                        }
                        // The bot's step: here in this ticket's Claude session, or,
                        // where the profile names the bot's command for it, remotely.
                        // In review, Fix is the step: among the PR's buttons.
                        let step = skill_step(card).filter(|_| card.stage != Stage::Review);
                        if let Some((label, hint, command, submit)) = step {
                            let local = Action::RunSkill {
                                issue_id: card.issue.id.clone(),
                                command,
                                submit,
                            };
                            step_button(
                                app,
                                card,
                                label,
                                &hint,
                                local,
                                remote_step(card),
                                None,
                                Vec::new(),
                                ui,
                                actions,
                            );
                        }
                        // No plan yet: Plan is always there, next to Triage or
                        // whatever the stage offers.
                        if let Some((label, hint, command, submit)) = plan_too(card) {
                            let local = Action::RunSkill {
                                issue_id: card.issue.id.clone(),
                                command,
                                submit,
                            };
                            step_button(
                                app,
                                card,
                                label,
                                &hint,
                                local,
                                Some(story::BotCommand::Plan),
                                None,
                                Vec::new(),
                                ui,
                                actions,
                            );
                        }
                        // A sweep's ticket the bot can investigate (profile ktlo.sweeps).
                        if let Some((hint, local)) = investigate(card) {
                            let rect = step_button(
                                app,
                                card,
                                "✳ Investigate",
                                &hint,
                                local,
                                Some(story::BotCommand::Investigate),
                                None,
                                Vec::new(),
                                ui,
                                actions,
                            );
                            mark(app, ui, "investigate", rect);
                        }
                        let agent = app.claude.agent_of(&card.issue.id);
                        let started = app.claude.saved.contains_key(&card.issue.id);
                        let text = if started {
                            format!("Resume {}", agent.short())
                        } else {
                            format!("Work on it with {}", agent.short())
                        };
                        // A session that ended (killed, failed, stopped): Resume,
                        // or start over with the agent picked.
                        let ended = app
                            .claude
                            .row_of(&card.issue.id)
                            .is_some_and(|row| !row.running());
                        if started {
                            if session_button(ui, agent, &text).clicked() {
                                actions.push(Action::Claude(card.issue.id.clone()));
                            }
                            if ended {
                                let default = app.claude.default_agent;
                                let fresh = |_| Action::FreshSession(card.issue.id.clone());
                                actions.extend(agent_choice(
                                    ui,
                                    default,
                                    default,
                                    "Start fresh",
                                    false,
                                    fresh,
                                ));
                            }
                        } else {
                            let default = app.claude.default_agent;
                            let start = |_| Action::Claude(card.issue.id.clone());
                            actions.extend(agent_choice(ui, agent, default, &text, false, start));
                        }
                        if card.prs.is_empty()
                            && card.ask.is_some()
                            && linear_button(ui, "Open in Linear ↗").clicked()
                        {
                            actions.push(Action::Open(card.issue.url.clone()));
                        }
                    });
                    let id = &card.issue.id;
                    // A command left for the bot, when there is one.
                    let remote = app.asked_bot.get(id).map(|(command, at)| {
                        let note = crate::profile::get().bot.poller_note.clone();
                        label(&asked_line(*command, &story::ago(at), &note))
                    });
                    if let Some(remote) = remote {
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(remote).wrap());
                    }
                    // Why the tab says failed: the session crashed or was
                    // killed, and how. One that finished its work (done)
                    // says nothing: Resume Claude is there.
                    let ended = app
                        .claude
                        .row_of(id)
                        .filter(|row| !row.running())
                        .filter(|row| {
                            row.failure.is_some()
                                || matches!(row.state.as_deref(), Some("failed" | "crashed"))
                        })
                        .map(|row| {
                            let how = row
                                .failure
                                .clone()
                                .unwrap_or_else(|| row.label().to_owned());
                            let agent = app.claude.agent_of(id).name();
                            format!("✳ the {agent} session ended: {how} · resume it or start fresh")
                        });
                    if let Some(ended) = ended {
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(label(&ended).color(p().danger)).wrap());
                    }
                    let live = app.claude.live_of(id).map(|l| l.label());
                    if let Some(note) = background_note(
                        app.claude.agent_of(id).name(),
                        app.claude.starting.contains(id),
                        app.claude.pending(id),
                        app.claude.sent(id),
                        live,
                    ) {
                        ui.add_space(6.0);
                        ui.label(super::widgets::label(&note));
                    }
                });
        });
}

/// The ask as the box's title; several PRs to review say so, each has
/// its row below.
fn ask_title(ask: Ask, prs: &[story::Pr]) -> &'static str {
    match ask {
        Ask::Review if prs.iter().filter(|pr| pr.waiting()).count() > 1 => "Review the PRs",
        ask => ask.label(),
    }
}

/// What the ticket's session is doing out of sight, for under the
/// buttons: starting, about to get a command, or working (on the command
/// a button just sent, when there is one).
fn background_note(
    agent: &str,
    starting: bool,
    pending: Option<&str>,
    sent: Option<&str>,
    live: Option<&str>,
) -> Option<String> {
    let word = |line: &str| line.split_whitespace().next().unwrap_or(line).to_owned();
    match (starting, pending.map(word), live) {
        (true, Some(command), _) => Some(format!(
            "✳ starting {agent} for {command} · {agent} tab to watch"
        )),
        (true, None, _) => Some(format!("✳ starting {agent} in the background")),
        (false, Some(command), _) => Some(format!("✳ sending {command} · {agent} tab to watch")),
        (false, None, Some("working")) => Some(match sent.map(word) {
            Some(command) => format!("✳ {agent} is working on {command} · {agent} tab to watch"),
            None => format!("✳ {agent} is working · {agent} tab to watch"),
        }),
        (false, None, Some(state @ ("needs approval" | "your turn"))) => {
            Some(format!("✳ {agent}: {state} · open the {agent} tab"))
        }
        (false, None, _) if sent.is_some() => Some(format!(
            "✳ sent {} · {agent} tab to watch",
            sent.map(word).unwrap_or_default()
        )),
        _ => None,
    }
}

/// One PR's row: its status in a fixed column, so the buttons of several
/// line up, then open it, then merge it when GitHub allows. A lone PR
/// waiting for your review is the filled button; several PRs share one
/// look, since a screen has one filled button.
/// Width of the PR state column; a PR's buttons line up after it.
const STATE_COLUMN: f32 = 76.0;

/// "■ draft" in the PR's colour, as tall as a button so it sits on the
/// middle of the row.
fn state_column(app: &App, pr: &story::Pr, ui: &mut Ui) {
    let state = app.pr_state(pr);
    let column = egui::vec2(STATE_COLUMN, button_height(ui));
    ui.allocate_ui_with_layout(column, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(column);
        let text = label(&format!("■ {}", state.label())).color(crate::theme::pr(state));
        ui.label(text);
    });
}

/// `with_state`: the state column first (one PR, or a merged one); an open
/// PR among several has it on its facts row above instead.
fn pr_buttons(
    app: &App,
    card: &Card,
    pr: &story::Pr,
    with_state: bool,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    use egui_phosphor::regular as icon;
    let state = app.pr_state(pr);
    if with_state {
        state_column(app, pr, ui);
    }
    // A PR closed without merging is history: a link, never a button.
    if state == story::PrState::Closed {
        if ui.link(format!("{} ↗", pr.name())).clicked() {
            actions.push(Action::Open(pr.url.clone()));
        }
        return;
    }
    // What the PR needs next is its row's filled button, first in the row;
    // with several PRs, each row has its own (`lead_of`), and Fix covers
    // only that PR.
    let lead = lead_of(app, pr);
    if lead == Some(Lead::Fix) {
        fix_button(app, card, Some(pr), true, ui, actions);
    }
    ready_pr(app, pr, lead, ui, actions);
    approve(app, pr, lead, ui, actions);
    merge(app, pr, lead, ui, actions);
    if lead == Some(Lead::RequestReview) && !review_open(&pr.url, ui) {
        let button = action(ui, icon::USER_PLUS, "Request review", Intent::Go, true)
            .on_hover_text("Your PR has no approval yet: ask someone on GitHub to review it");
        if button.clicked() {
            open_review(pr, ui, actions);
        }
    }
    if lead != Some(Lead::Fix) {
        fix_button(app, card, Some(pr), false, ui, actions);
    }
    update_branch(app, pr, ui, actions);
    land_it(app, pr, ui, actions);
    request_reporter(app, card, pr, ui, actions);
    preview(app, pr, ui, actions);
    review_form(app, pr, ui, actions);
    // The PR itself, quieter: open it, copy its link, ask GitHub again.
    let lead_pr = card.pr(None).is_some_and(|lead| lead.url == pr.url);
    let hint = if lead_pr {
        "Open the PR on GitHub (key: o)"
    } else {
        "Open the PR on GitHub"
    };
    if action(ui, icon::ARROW_SQUARE_OUT, &pr.name(), Intent::Plain, false)
        .on_hover_text(hint)
        .clicked()
    {
        actions.push(Action::Open(pr.url.clone()));
    }
    copy_icon(ui, &pr.url);
    let refresh = action(ui, icon::ARROWS_CLOCKWISE, "", Intent::Plain, false)
        .on_hover_text("Ask GitHub again now: merged, queued, checks, reviews");
    mark(app, ui, "refresh-gate", refresh.rect);
    if refresh.clicked() {
        actions.push(Action::RefreshGate(pr.url.clone()));
    }
    more(app, card, pr, lead, ui, actions);
}

/// What a PR's row fills: the one step it needs from the user next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Lead {
    MarkReady,
    Merge,
    Approve,
    Fix,
    AiReview,
    RequestReview,
}

/// A draft can only become ready; then what ends it (Merge), what the
/// user is asked for (Approve), what blocks it (Fix), and on the user's
/// own PR without approval, asking for a review (`ai`: the profile asks
/// for an AI review, not yet at this head).
fn lead(gate: &crate::github::Gate, ai: bool) -> Option<Lead> {
    if gate.state != "OPEN" || gate.queued {
        return None;
    }
    if gate.draft {
        return Some(Lead::MarkReady);
    }
    if (gate.green || gate.can_auto_merge) && !gate.auto_merge {
        return Some(Lead::Merge);
    }
    if gate.can_approve {
        return Some(Lead::Approve);
    }
    if crate::github::needs_fix(gate) || !gate.queue_failed.is_empty() {
        return Some(Lead::Fix);
    }
    if gate.own && gate.review != "APPROVED" {
        return Some(if ai {
            Lead::AiReview
        } else {
            Lead::RequestReview
        });
    }
    None
}

/// `lead` for a PR, from what GitHub said last.
pub(super) fn lead_of(app: &App, pr: &story::Pr) -> Option<Lead> {
    let Some(Ok(gate)) = app.gates.get(&pr.url) else {
        return None;
    };
    if !pr.open {
        return None;
    }
    let ai = crate::profile::get().github.ai_review.how().is_some()
        && app.ai_reviews.get(&pr.url) != Some(&gate.head);
    lead(gate, ai)
}

/// More ▾: what a PR needs now and then: someone's review, the review
/// router, and closing it (last, in red).
fn more(
    app: &App,
    card: &Card,
    pr: &story::Pr,
    lead: Option<Lead>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    use egui_phosphor::regular as icon;
    // What GitHub says now, not what the card was built with: a merged or
    // closed PR has nothing to close, nor a review to ask for.
    let state = app.pr_state(pr);
    let open = matches!(
        state,
        story::PrState::Open | story::PrState::Approved | story::PrState::Draft
    );
    if !open {
        return;
    }
    let can_ask = matches!(state, story::PrState::Open | story::PrState::Draft)
        && lead != Some(Lead::RequestReview)
        && !review_open(&pr.url, ui);
    let roulette = crate::profile::get().github.review_roulette.clone();
    let rolled = app.labeled.contains(&(pr.url.clone(), roulette.clone()));
    let canceled = (!card.pr_only())
        .then(|| app.states.get(&card.issue.team.id))
        .flatten()
        .and_then(|states| story::canceled_state(states))
        .filter(|s| s.id != card.issue.state.id);
    let menu = action_menu(
        ui,
        icon::DOTS_THREE,
        "More",
        "Request a review, close the PR",
        |ui| {
            if can_ask
                && ui
                    .button(format!("{}  Request review…", icon::USER_PLUS))
                    .on_hover_text(
                        "Ask someone on GitHub to review it: a field opens beside the buttons",
                    )
                    .clicked()
            {
                open_review(pr, ui, actions);
                ui.close();
            }
            if can_ask && !roulette.is_empty() && !rolled {
                let hint = format!("Adds the {roulette} label: the review router picks a reviewer");
                if ui
                    .button(format!("{}  Review roulette", icon::SHUFFLE))
                    .on_hover_text(hint)
                    .clicked()
                {
                    actions.push(Action::AddLabel {
                        url: pr.url.clone(),
                        label: roulette.clone(),
                    });
                    ui.close();
                }
            }
            if can_ask {
                ui.separator();
            }
            let close = |cancel| Action::ClosePr {
                url: pr.url.clone(),
                cancel,
            };

            if action(ui, icon::X_CIRCLE, "Close the PR", Intent::Danger, false)
                .on_hover_text("Closes it on GitHub without merging; it can be reopened there")
                .clicked()
            {
                actions.push(close(None));
                ui.close();
            }
            if let Some(state) = &canceled {
                let text = format!("Close the PR and cancel {}", card.issue.identifier);
                let hover = format!("Also moves the ticket to {} in Linear", state.name);
                if action(ui, icon::X_CIRCLE, &text, Intent::Danger, false)
                    .on_hover_text(hover)
                    .clicked()
                {
                    actions.push(close(Some((card.issue.id.clone(), state.id.clone()))));
                    ui.close();
                }
            }
        },
    );
    mark(app, ui, "close-pr", menu.rect);
    if can_ask {
        mark(app, ui, "request-review", menu.rect);
    }
}

/// Request review from the person who filed the ticket: they know best
/// whether it is fixed. A plain rule (a person other than the user filed
/// it, the PR is open in review); Laya could not tell (laya.rs QUICK).
fn request_reporter(
    app: &App,
    card: &Card,
    pr: &story::Pr,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let Some(name) = crate::app::reporter(card, &app.viewer) else {
        return;
    };
    let Some(Some(login)) = app.logins.get(name) else {
        return;
    };
    let Some(Ok(gate)) = app.gates.get(&pr.url) else {
        return;
    };
    // Not the one who opened the PR, and not when it is closed or merged.
    if gate.state != "OPEN" || gate.author == *login {
        return;
    }
    let first = name.split_whitespace().next().unwrap_or(name);
    if app
        .review_requested
        .contains(&(pr.url.clone(), login.clone()))
    {
        ui.label(label(&format!("review requested from {first}")));
        return;
    }
    let text = format!("Request review from {first}");
    let button = action(ui, egui_phosphor::regular::USER_PLUS, &text, Intent::Plain, false).on_hover_text(
        format!("{name} filed the ticket and can tell best whether it is fixed: asks @{login} to review the PR on GitHub"),
    );
    mark(app, ui, "request-reporter", button.rect);
    if button.clicked() {
        actions.push(Action::RequestReview {
            url: pr.url.clone(),
            login: login.clone(),
        });
    }
}

/// Land it: a reviewed PR that is not mergeable yet (a flaky or red check,
/// the branch behind) gets a landing session of its own, as KTLO's Land
/// approved does: it re-runs, fixes and updates until the PR is merged,
/// merging through GitHub (`gh pr merge --auto`) once it is CLEAN.
fn land_it(app: &App, pr: &story::Pr, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(Ok(gate)) = app.gates.get(&pr.url) else {
        return;
    };
    if !crate::github::landable(gate) {
        return;
    }
    let land = egui_phosphor::regular::ROCKET_LAUNCH;
    let button = action(ui, land, "Land it", Intent::Good, false).on_hover_text(
        "Starts a landing session that keeps at it until the PR is merged: it re-runs flaky \
         checks, fixes real failures, updates the branch, and merges through GitHub once the \
         PR is approved and green. It never approves or force-merges.",
    );
    mark(app, ui, "land-it", button.rect);
    if button.clicked() {
        actions.push(Action::LandBatch(vec![(pr.url.clone(), gate.head.clone())]));
    }
}

/// Preview ▾: the profile's preview labels on an open PR in a repo that
/// deploys previews, an item each (e.g. Preview UI, Preview full).
pub(super) fn preview(app: &App, pr: &story::Pr, ui: &mut Ui, actions: &mut Vec<Action>) {
    use egui_phosphor::regular as icon;
    if !matches!(
        app.pr_state(pr),
        story::PrState::Open | story::PrState::Draft
    ) {
        return;
    }
    let labels = crate::profile::get().github.preview.labels_for(&pr.repo);
    if labels.is_empty() {
        return;
    }
    let menu = action_menu(
        ui,
        icon::EYE,
        "Preview",
        "Deploy a preview of this PR",
        |ui| {
            for kind in &labels {
                if app.labeled.contains(&(pr.url.clone(), kind.label.clone())) {
                    ui.label(label(&format!("{}  {} asked", icon::CHECK, kind.button)));
                    continue;
                }
                if ui
                    .button(&kind.button)
                    .on_hover_text(format!("Adds the {} label: {}", kind.label, kind.hint))
                    .clicked()
                {
                    actions.push(Action::AddLabel {
                        url: pr.url.clone(),
                        label: kind.label.clone(),
                    });
                    ui.close();
                }
            }
        },
    );
    mark(app, ui, "preview", menu.rect);
}

/// Whether the request-review field is open for `url` (one id for the
/// whole window: More's popup and the row share it).
fn review_open(url: &str, ui: &Ui) -> bool {
    ui.data(|d| d.get_temp(egui::Id::new(("review-open", url))))
        .unwrap_or(false)
}

/// Opens the request-review field beside the buttons (from More, or the
/// lead on the user's own PR) and loads who may review on the repo.
fn open_review(pr: &story::Pr, ui: &Ui, actions: &mut Vec<Action>) {
    ui.data_mut(|d| d.insert_temp(egui::Id::new(("review-open", &pr.url)), true));
    actions.push(Action::LoadReviewers(format!("{}/{}", pr.owner, pr.repo)));
}

/// The request-review field, while open: someone by their GitHub login, or
/// the review router's label (Roulette); who was asked, after.
fn review_form(app: &App, pr: &story::Pr, ui: &mut Ui, actions: &mut Vec<Action>) {
    if !matches!(
        app.pr_state(pr),
        story::PrState::Open | story::PrState::Draft
    ) {
        return;
    }
    let roulette = crate::profile::get().github.review_roulette.clone();
    let rolled = app.labeled.contains(&(pr.url.clone(), roulette.clone()));
    let asked: Vec<&str> = app
        .review_requested
        .iter()
        .filter(|(url, _)| url == &pr.url)
        .map(|(_, login)| login.as_str())
        .collect();
    // Inline, not in a popup menu: a text field in a popup lost the keys.
    let open_id = egui::Id::new(("review-open", &pr.url));
    let login_id = egui::Id::new(("review-login", &pr.url));
    if review_open(&pr.url, ui) {
        let mut login: String = ui.data(|d| d.get_temp(login_id)).unwrap_or_default();
        let field = ui.add(
            egui::TextEdit::singleline(&mut login)
                .hint_text("GitHub login")
                .desired_width(160.0),
        );
        mark(app, ui, "request-review", field.rect);
        // Typing goes straight in: the field takes the keys when it opens.
        if !field.has_focus() && login.is_empty() && !field.lost_focus() {
            field.request_focus();
        }
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        // The repo's reviewers whose name or login holds what is typed;
        // a pick sends their login, so a name never reaches GitHub.
        let slug = format!("{}/{}", pr.owner, pr.repo);
        let people = app.reviewers.get(&slug);
        let mut picked = None;
        match people {
            Some(Ok(people)) => {
                for (who, name) in crate::github::matching(people, &login).into_iter().take(5) {
                    let text = if name.is_empty() {
                        format!("@{who}")
                    } else {
                        format!("{name} @{who}")
                    };
                    if ui.small_button(text).clicked() {
                        picked = Some(who.clone());
                    }
                }
            }
            Some(Err(_)) => {}
            None => {
                ui.label(label("loading reviewers"));
            }
        }
        let typed = login.trim().trim_start_matches('@').to_owned();
        // A login GitHub will take: one of the repo's, or anything when
        // the list could not be read.
        let known = match people {
            Some(Ok(people)) => people.iter().any(|(l, _)| l.eq_ignore_ascii_case(&typed)),
            _ => !typed.is_empty(),
        };
        let request = ui
            .add_enabled(known, egui::Button::new("Request"))
            .on_disabled_hover_text("Pick one of the names: GitHub asks only people with access");
        let who = picked.or_else(|| ((request.clicked() || enter) && known).then_some(typed));
        if let Some(who) = who {
            actions.push(Action::RequestReview {
                url: pr.url.clone(),
                login: who,
            });
            login.clear();
            ui.data_mut(|d| d.insert_temp(open_id, false));
        }
        if !roulette.is_empty() && !rolled {
            let hint = format!("Adds the {roulette} label: the review router picks a reviewer");
            if ui.button("Roulette").on_hover_text(hint).clicked() {
                actions.push(Action::AddLabel {
                    url: pr.url.clone(),
                    label: roulette.clone(),
                });
                ui.data_mut(|d| d.insert_temp(open_id, false));
            }
        }
        if link(ui, label("cancel")).clicked()
            || field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            login.clear();
            ui.data_mut(|d| d.insert_temp(open_id, false));
        }
        ui.data_mut(|d| d.insert_temp(login_id, login));
    }
    if rolled {
        ui.label(label("roulette asked"));
    } else if !asked.is_empty() {
        ui.label(label(&format!("asked {}", asked.join(", "))));
    }
}

/// A draft PR: Ready for review, the step GitHub offers before merging.
fn ready_pr(app: &App, pr: &story::Pr, lead: Option<Lead>, ui: &mut Ui, actions: &mut Vec<Action>) {
    use egui_phosphor::regular as icon;
    if app.pr_state(pr) != story::PrState::Draft {
        return;
    }
    if app.readying.contains(&pr.url) {
        pending(ui, "Marking ready");
        return;
    }
    let filled = lead == Some(Lead::MarkReady);
    if action(ui, icon::GIT_PULL_REQUEST, "Mark ready", Intent::Go, filled)
        .on_hover_text("Marks the draft ready for review on GitHub")
        .clicked()
    {
        actions.push(Action::ReadyPr(pr.url.clone()));
    }
}

/// GitHub's Update branch: merges the base into a branch that is behind,
/// bound to the head the user saw. Updating until a new head shows.
pub(super) fn update_branch(app: &App, pr: &story::Pr, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(Ok(gate)) = app.gates.get(&pr.url) else {
        return;
    };
    if app.updating.get(&pr.url) == Some(&gate.head) {
        pending(ui, "Updating branch");
        return;
    }
    if !pr.open || gate.branch != "BEHIND" {
        return;
    }
    let update = egui_phosphor::regular::GIT_BRANCH;
    let button = action(ui, update, "Update branch", Intent::Plain, false)
        .on_hover_text("Merges the latest base branch into this PR's branch, on GitHub.");
    mark(app, ui, "update-branch", button.rect);
    if button.clicked() {
        actions.push(Action::UpdateBranch {
            url: pr.url.clone(),
            head: gate.head.clone(),
        });
    }
}

/// The colour of a verdict or one of its facts.
pub(super) fn tone(tone: crate::github::Tone) -> Color32 {
    match tone {
        crate::github::Tone::Good => p().success,
        crate::github::Tone::Waiting => p().signal,
        crate::github::Tone::Blocked => p().danger,
    }
}

/// Whether the PR can be merged, as one quiet line: who opened it, then
/// the facts in their colours ("ready to merge" first when it is; GitHub's
/// reason only where the facts do not say it). `named`: several PRs, so
/// the line says which; `draft_shown`: a state label beside it says draft.
pub(super) fn pr_verdict(app: &App, pr: &story::Pr, named: bool, draft_shown: bool, ui: &mut Ui) {
    let Some(Ok(gate)) = app.gates.get(&pr.url) else {
        // Never a blank: an open PR says it is being asked about.
        if pr.open && !app.gates.contains_key(&pr.url) && !app.merged.contains_key(&pr.url) {
            ui.label(label("checking with GitHub whether it can be merged…"));
        }
        return;
    };
    let facts = crate::github::merge_facts(gate, draft_shown);
    // The branch always shows, merged or not; a click copies its name.
    if facts.is_empty() && gate.head_branch.is_empty() {
        return;
    }
    let rect = ui
        .horizontal_wrapped(|ui| {
            if named {
                ui.label(RichText::new(pr.name()).monospace().color(p().text));
            }
            if !gate.author.is_empty() {
                opened_by(
                    ui,
                    story::who(&gate.author, gate.own, gate.author_bot),
                    &gate.author,
                );
            }
            for (i, (t, fact)) in facts.iter().enumerate() {
                if i > 0 {
                    ui.label(label("·"));
                }
                // Ready to merge reads in the Merge button's colour: the
                // fact and the click it allows look like one thing.
                let color = if fact == "ready to merge" {
                    p().btn
                } else {
                    tone(*t)
                };
                ui.label(label(fact).color(color));
            }
            if !gate.head_branch.is_empty() {
                if !facts.is_empty() {
                    ui.label(label("·"));
                }
                let text = match gate.base_branch.as_str() {
                    "" => gate.head_branch.clone(),
                    base => format!("{} → {base}", gate.head_branch),
                };
                let branch = link(ui, label(&text).color(p().text_2))
                    .on_hover_text("The PR's branch, into its base; click to copy the branch name");
                if branch.clicked() {
                    ui.ctx().copy_text(gate.head_branch.clone());
                }
            }
            // Who approved it stands out, merged or not; then who else is
            // on it: requested, reviewed, commented.
            let (approved, rest) = approvals(&gate.on_it);
            if !approved.is_empty() {
                ui.label(label("·"));
                ui.label(
                    label(&format!(
                        "{} approved by {}",
                        egui_phosphor::regular::CHECK,
                        approved.join(", ")
                    ))
                    .color(p().success),
                );
            }
            if !rest.is_empty() {
                ui.label(label("·"));
                ui.label(label(&format!("on it: {}", rest.join(", "))).color(p().text));
            }
        })
        .response
        .rect;
    mark(app, ui, "verdict", rect);
}

/// `Gate::on_it` split: who approved (`@ana`), and everyone else's line.
fn approvals(on_it: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut approved = Vec::new();
    let mut rest = Vec::new();
    for line in on_it {
        match line.strip_suffix(" approved") {
            Some(who) => approved.push(who),
            None => rest.push(line.as_str()),
        }
    }
    (approved, rest)
}

/// Approve, only while GitHub says the user's review is what is missing
/// and the PR is not theirs; bound to the head they saw, checked again
/// right before. One click: an approval can be dismissed, a merge not.
pub(super) fn approve(
    app: &App,
    pr: &story::Pr,
    lead: Option<Lead>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let Some(Ok(gate)) = app.gates.get(&pr.url) else {
        return;
    };
    if pr.open && gate.own {
        ai_review(app, pr, gate, lead, ui, actions);
        return;
    }
    if !pr.open || !gate.can_approve {
        return;
    }
    if app.approving.contains(&pr.url) {
        pending(ui, "Approving");
        return;
    }
    let short = &gate.head[..7.min(gate.head.len())];
    // The note from the Diff tab, when the user asked about the PR and kept one.
    let note = match app.approve_notes.get(&pr.url) {
        Some(Some(Ok(note))) if !note.trim().is_empty() => Some(note.trim().to_owned()),
        _ => None,
    };
    let (text, hover) = match &note {
        Some(note) => (
            "Approve with note",
            format!("Approves the PR at {short} as you, on GitHub, commenting:\n{note}"),
        ),
        None => (
            "Approve",
            format!("Approves the PR at {short} as you, on GitHub."),
        ),
    };
    let check = egui_phosphor::regular::CHECK;
    let button =
        action(ui, check, text, Intent::Good, lead == Some(Lead::Approve)).on_hover_text(hover);
    mark(app, ui, "approve", button.rect);
    if button.clicked() {
        actions.push(Action::Approve {
            url: pr.url.clone(),
            head: gate.head.clone(),
            body: note,
        });
    }
}

/// The user's own PR with no approval yet: ask for an AI review the way
/// the profile says, again once new commits landed. Without a profile
/// setting, why there is no Approve.
fn ai_review(
    app: &App,
    pr: &story::Pr,
    gate: &crate::github::Gate,
    lead: Option<Lead>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    if app.ai_asking.contains(&pr.url) {
        pending(ui, "Requesting AI review");
        return;
    }
    let how = crate::profile::get().github.ai_review.how();
    let approved = app.pr_state(pr) == story::PrState::Approved;
    let Some(how) = how.filter(|_| !approved) else {
        ui.label(label("your PR: no self-approval")).on_hover_text(
            "It was opened from your GitHub account; GitHub needs someone else to approve it.",
        );
        return;
    };
    let short = &gate.head[..7.min(gate.head.len())];
    if app.ai_asking.contains(&pr.url) {
        ui.label(label("asking for an AI review…"));
        return;
    }
    let asked = app.ai_reviews.get(&pr.url);
    if asked == Some(&gate.head) {
        ui.label(label(&format!("AI review asked at {short}")));
        return;
    }
    let text = if asked.is_some() {
        "Re-request AI review"
    } else {
        "Request AI review"
    };
    let sparkle = egui_phosphor::regular::SPARKLE;
    let filled = lead == Some(Lead::AiReview);
    let button = action(ui, sparkle, text, Intent::Go, filled).on_hover_text(format!(
        "Your PR has no approval yet. One click {how}, at {short}."
    ));
    mark(app, ui, "ai-review", button.rect);
    if button.clicked() {
        actions.push(Action::AiReview {
            url: pr.url.clone(),
            head: gate.head.clone(),
        });
    }
}

/// What Laya proposes after reading the ticket (laya.rs `QUICK`), before
/// any PR: a click is logged against the proposal, then runs the action.
fn quick_actions(app: &App, card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    if !crate::app::quick_fits(card) {
        return;
    }
    let proposed = app.proposed(&card.issue.id);
    if proposed.is_empty() {
        return;
    }
    let id = &card.issue.identifier;
    ui.add_space(8.0);
    // Centered: the small chip and label sit on the buttons' midline. The
    // row is a button high (with_layout would hand it all the height left,
    // and the chip would stretch down the pane); wrapped rows add their own.
    let row = Layout::left_to_right(Align::Center).with_main_wrap(true);
    let height = ui.spacing().interact_size.y.max(30.0);
    let rect = ui
        .allocate_ui_with_layout(egui::vec2(ui.available_width(), height), row, |ui| {
            laya_chip(
                ui,
                "Laya, the model inside the app, read the ticket and suggests these; \
                 Settings shows how often it was right",
            );
            ui.label(label("suggests:"));
            for question in proposed {
                let (text, hint, command, submit) = match question {
                    "ready" => {
                        let plan = crate::profile::get().skill("plan");
                        (
                            "✳ Plan it".to_owned(),
                            format!("It says how to reproduce it and what is expected: runs /{plan} here"),
                            format!("/{plan} {id}"),
                            true,
                        )
                    }
                    "repro" => (
                        "Ask for repro steps".to_owned(),
                        "It reads like a bug without repro steps: types a request for a draft \
                         comment into the Claude session, for you to send"
                            .to_owned(),
                        format!(
                            "Draft a short comment for {id} asking the reporter how to reproduce \
                             the problem and what they expected instead. Show it to me before \
                             posting it."
                        ),
                        false,
                    ),
                    _ => continue,
                };
                // Planning runs in the ticket's session: its agent's mark.
                let button = if text.starts_with("✳ ") {
                    session_button(ui, app.claude.agent_of(&card.issue.id), &text)
                } else {
                    ui.button(text)
                };
                if button.on_hover_text(hint).clicked() {
                    actions.push(Action::Quick {
                        ticket: id.clone(),
                        question: question.into(),
                        then: Box::new(Action::RunSkill {
                            issue_id: card.issue.id.clone(),
                            command,
                            submit,
                        }),
                    });
                }
            }
        })
        .response
        .rect;
    mark(app, ui, "laya-quick", rect);
    ui.add_space(12.0);
}

/// What Fix covers on a card's open PRs (`fix_parts`), each its own
/// command: review threads, red checks, changes asked or conflicts, a
/// merge-queue drop.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FixPart {
    /// Open review threads: the fix-comments skill on the PR.
    Comments { url: String, n: usize },
    /// Failing or erroring checks: only their root cause.
    Ci,
    /// Changes asked for, or conflicts: the build skill's fix mode.
    Changes,
    /// The merge queue dropped the PR: why, from the queue's run.
    Queue {
        url: String,
        reason: String,
        base: String,
    },
}

impl FixPart {
    fn what(&self) -> String {
        match self {
            FixPart::Comments { n, .. } => {
                format!("{n} comment{}", if *n == 1 { "" } else { "s" })
            }
            FixPart::Ci => "CI".into(),
            FixPart::Changes => "changes".into(),
            FixPart::Queue { .. } => "merge queue".into(),
        }
    }
}

/// Every reason to fix the open PRs (`(url, gate)`), once each.
fn fix_parts(prs: &[(&str, &crate::github::Gate)]) -> Vec<FixPart> {
    let mut parts = Vec::new();
    for (url, gate) in prs {
        if gate.open_threads > 0 {
            parts.push(FixPart::Comments {
                url: (*url).to_owned(),
                n: gate.open_threads,
            });
        }
    }
    if prs
        .iter()
        .any(|(_, g)| matches!(g.checks.as_str(), "FAILURE" | "ERROR"))
    {
        parts.push(FixPart::Ci);
    }
    if prs
        .iter()
        .any(|(_, g)| g.review == "CHANGES_REQUESTED" || g.branch == "DIRTY")
    {
        parts.push(FixPart::Changes);
    }
    for (url, gate) in prs {
        if !gate.queue_failed.is_empty() {
            parts.push(FixPart::Queue {
                url: (*url).to_owned(),
                reason: gate.queue_failed.clone(),
                base: gate.base_branch.clone(),
            });
        }
    }
    parts
}

/// One button for all of it: "Fix · 3 comments · CI".
fn fix_label(parts: &[FixPart]) -> String {
    let mut comments = 0;
    let mut words = Vec::new();
    for part in parts {
        match part {
            FixPart::Comments { n, .. } => comments += n,
            other => words.push(other.what()),
        }
    }
    if comments > 0 {
        let what = FixPart::Comments {
            url: String::new(),
            n: comments,
        }
        .what();
        words.insert(0, what);
    }
    format!("✳ Fix · {}", words.join(" · "))
}

/// What Fix runs, and its hint: one part alone runs just that part; more
/// run the build skill's fix mode, which takes comments, checks, changes
/// and conflicts together.
fn fix_run(parts: &[FixPart], build: &str, fix_comments: &str, id: &str) -> (String, String) {
    match parts {
        [FixPart::Comments { url, n }] => (
            format!("/{fix_comments} {url}"),
            format!(
                "Runs /{fix_comments} on the PR in the ticket's Claude session: fixes each of the \
                 {n} threads, pushes, replies and resolves it. Never merges."
            ),
        ),
        [FixPart::Ci] => (
            {
                let ci = "only the failing CI checks: fix their root cause, change nothing else";
                format!("/{build} {id} {ci}")
            },
            "Runs the build skill's fix mode on the failing checks only: their root cause, \
             nothing else"
                .into(),
        ),
        [FixPart::Queue { url, reason, base }] => (
            fix_queue_brief(url, reason, base),
            format!(
                "The merge queue dropped this PR ({}). Finds why from the queue's last run and \
                 fixes it on the PR's branch, in the ticket's Claude session. Never queues or \
                 merges.",
                reason.replace('_', " ")
            ),
        ),
        _ => (
            format!("/{build} {id}"),
            format!(
                "Runs /{build} in fix mode in the ticket's Claude session: review comments, \
                 failing checks, changes asked for and conflicts. Never merges."
            ),
        ),
    }
}

/// Fix: everything that blocks the card's open PRs (or `only` one of them,
/// on its own row), as one button; its ▾ runs one part alone when there
/// are several. `lead`: the row's filled button.
fn fix_button(
    app: &App,
    card: &Card,
    only: Option<&story::Pr>,
    lead: bool,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let prs: Vec<(&str, &crate::github::Gate)> = card
        .prs
        .iter()
        .filter(|pr| pr.open && only.is_none_or(|o| o.url == pr.url))
        .filter_map(|pr| match app.gates.get(&pr.url) {
            Some(Ok(gate)) => Some((pr.url.as_str(), gate)),
            _ => None,
        })
        .collect();
    let parts = fix_parts(&prs);
    if parts.is_empty() {
        return;
    }
    let profile = crate::profile::get();
    let (build, fix_comments) = (profile.skill("build"), profile.skill("fixComments"));
    let id = &card.issue.identifier;
    let run = |command: String| Action::RunSkill {
        issue_id: card.issue.id.clone(),
        command,
        submit: true,
    };
    let (command, hint) = fix_run(&parts, &build, &fix_comments, id);
    let extra = if parts.len() > 1 {
        parts
            .iter()
            .map(|part| {
                let (command, hint) =
                    fix_run(std::slice::from_ref(part), &build, &fix_comments, id);
                (format!("Only the {}", part.what()), hint, run(command))
            })
            .collect()
    } else {
        Vec::new()
    };
    let rect = step_button(
        app,
        card,
        &fix_label(&parts),
        &hint,
        run(command),
        remote_step(card),
        lead.then_some(Intent::Fix),
        extra,
        ui,
        actions,
    );
    if parts.iter().any(|p| matches!(p, FixPart::Comments { .. })) {
        mark(app, ui, "fix-comments", rect);
    }
    if parts.iter().any(|p| matches!(p, FixPart::Queue { .. })) {
        mark(app, ui, "fix-queue", rect);
    }
}

/// What the session is told when the merge queue dropped `url`.
fn fix_queue_brief(url: &str, reason: &str, base: &str) -> String {
    let number = url.rsplit('/').next().unwrap_or_default();
    format!(
        "The merge queue removed {url} from {base}'s queue ({reason}). Find why: list the \
         queue's runs with `gh run list -R <owner/repo> --event merge_group -L 50 --json \
         headBranch,conclusion,url,databaseId` and take the latest one whose headBranch \
         has `pr-{number}-` (failed, cancelled or still running when it was removed), then read its failed jobs (`gh run view <id> --log-failed`). \
         Find the root cause and fix it on this PR's branch, run what failed locally where \
         you can, and push. When it is a flake or a failure unrelated to this PR, change \
         nothing and say so. Never add it to the queue again, merge or approve: that stays \
         mine. Tell me in a few lines what failed and what you changed."
    )
}

/// Merge, only while GitHub says the PR is clean at the head it was asked
/// about: open, not a draft, not already in the merge queue, every check
/// green. Into a merge queue it is one click: the queue can still drop it.
/// A squash-merge takes two, since it cannot be taken back. Otherwise, why
/// not, in mono, unless the status already says it (draft, merging,
/// merged, closed).
pub(super) fn merge(
    app: &App,
    pr: &story::Pr,
    lead: Option<Lead>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let git_merge = egui_phosphor::regular::GIT_MERGE;
    let filled = lead == Some(Lead::Merge);
    if let Some(sha) = app.merged.get(&pr.url) {
        ui.label(label(&format!("merged {}", &sha[..7.min(sha.len())])));
        return;
    }
    if app.merging.contains(&pr.url) {
        pending(ui, "Merging");
        return;
    }
    let gate = match app.gates.get(&pr.url) {
        Some(Ok(gate)) if pr.open => gate,
        Some(Err(error)) if pr.open => {
            ui.label(label("merge: can't check")).on_hover_text(error);
            return;
        }
        _ => return,
    };
    let short = &gate.head[..7.min(gate.head.len())];
    if gate.auto_merge {
        let rect = ui
            .label(
                label(&format!(
                    "{} merges when ready",
                    egui_phosphor::regular::CHECK
                ))
                .color(p().success),
            )
            .rect;
        mark(app, ui, "merge-when-ready", rect);
        if ui
            .button("Turn off")
            .on_hover_text("Stops GitHub from merging it by itself")
            .clicked()
        {
            actions.push(Action::CancelAutoMerge(pr.url.clone()));
        }
        return;
    }
    let armed_id = ui.make_persistent_id(("merge", &pr.url, &gate.head));
    // A queue takes it back out on request: no confirm click before one.
    let armed = gate.merge_queue || ui.data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
    // Only the checks missing: GitHub's Merge when ready, as two clicks.
    if gate.can_auto_merge {
        let then = if gate.merge_queue {
            "adds it to the merge queue"
        } else {
            "squash-merges it"
        };
        if !armed {
            let button = action(ui, git_merge, "Merge when ready", Intent::Good, filled)
                .on_hover_text(format!(
                    "Approved, checks still running at {short}: GitHub {then} once they pass. \
                 New commits turn it off."
                ));
            mark(app, ui, "merge-when-ready", button.rect);
            if button.clicked() {
                ui.data_mut(|d| d.insert_temp(armed_id, true));
            }
            return;
        }
        let text = format!("Merge when ready at {short}");
        let button = action(ui, git_merge, &text, Intent::Good, true).on_hover_text(format!(
            "Checks still running: GitHub {then} once they pass."
        ));
        if gate.merge_queue {
            mark(app, ui, "merge-when-ready", button.rect);
        }
        if button.clicked() {
            ui.data_mut(|d| d.remove::<bool>(armed_id));
            actions.push(Action::AutoMerge {
                url: pr.url.clone(),
                head: gate.head.clone(),
            });
        }
        if !gate.merge_queue && ui.button("Cancel").clicked() {
            ui.data_mut(|d| d.remove::<bool>(armed_id));
        }
        return;
    }
    // Why not is the verdict's headline, over the buttons.
    if !gate.green {
        return;
    }
    if !armed {
        let what = if gate.merge_queue {
            "Adds the PR to the base branch's merge queue."
        } else {
            "Squash-merges the PR."
        };
        let button = action(ui, git_merge, "Merge", Intent::Good, filled)
            .on_hover_text(format!("Checks green at {short}. {what}"));
        mark(app, ui, "merge", button.rect);
        if button.clicked() {
            ui.data_mut(|d| d.insert_temp(armed_id, true));
        }
        return;
    }
    let confirm = if gate.merge_queue {
        format!("Add to merge queue at {short}")
    } else {
        format!("Squash-merge at {short}")
    };
    let button = action(ui, git_merge, &confirm, Intent::Good, true);
    if gate.merge_queue {
        mark(app, ui, "merge", button.rect);
    }
    if button
        .on_hover_text(format!("Checks green at {short}."))
        .clicked()
    {
        ui.data_mut(|d| d.remove::<bool>(armed_id));
        actions.push(Action::Merge {
            url: pr.url.clone(),
            head: gate.head.clone(),
        });
    }
    if !gate.merge_queue && ui.button("Cancel").clicked() {
        ui.data_mut(|d| d.remove::<bool>(armed_id));
    }
}

/// A primary button, `label`, that moves the ticket to its team's
/// completed state.
/// Mark done shows when the ticket asks you to verify it, and on your own
/// ticket in Testing, which has no ask (a person took it: you).
fn closes_here(card: &Card, mine: bool) -> bool {
    card.ask == Some(Ask::Verify) || (card.stage == Stage::Testing && mine)
}

pub(super) fn mark_done(
    app: &App,
    card: &Card,
    label: &str,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let done = app
        .states
        .get(&card.issue.team.id)
        .and_then(|states| states.iter().find(|s| s.kind == "completed"));
    if let Some(done) = done
        && primary(ui, label).clicked()
    {
        actions.push(Action::SetState {
            issue_id: card.issue.id.clone(),
            state_id: done.id.clone(),
        });
    }
}

/// The bot's step that fits where the ticket is, as a local skill command
/// (the profile names the skills): `(label, hint, command, submit)`.
fn skill_step(card: &Card) -> Option<(&'static str, String, String, bool)> {
    let id = &card.issue.identifier;
    let profile = crate::profile::get();
    let (triage, plan, build) = (
        profile.skill("triage"),
        profile.skill("plan"),
        profile.skill("build"),
    );
    match card.stage {
        Stage::Intake => Some((
            "✳ Triage",
            format!("Runs /{triage} in this ticket's Claude session"),
            format!("/{triage} {id}"),
            true,
        )),
        Stage::Planning if card.plan.is_some() => Some((
            "✳ Build",
            format!("Runs /{build} locally: implements the plan and opens the PR"),
            format!("/{build} {id}"),
            true,
        )),
        Stage::Planning => Some(plan_step(card)),
        Stage::Review => Some((
            "✳ Fix",
            format!("Runs /{build} in fix mode: review comments and failing checks"),
            format!("/{build} {id}"),
            true,
        )),
        Stage::Blocked => Some((
            "✳ Re-plan…",
            format!("Types /{plan} … iterate: into the session; add your answer and press Enter"),
            format!("/{plan} {id} iterate: "),
            false,
        )),
        _ => None,
    }
}

/// Plan beside the stage's own step on a ticket with no plan and no PR yet;
/// Planning already offers it, Blocked has Re-plan.
fn plan_too(card: &Card) -> Option<(&'static str, String, String, bool)> {
    let shown = matches!(card.stage, Stage::Planning | Stage::Blocked | Stage::Done);
    if card.plan.is_some() || !card.prs.is_empty() || shown {
        return None;
    }
    Some(plan_step(card))
}

fn plan_step(card: &Card) -> (&'static str, String, String, bool) {
    let plan = crate::profile::get().skill("plan");
    (
        "✳ Plan",
        format!("Runs /{plan} locally and posts the plan after you approve it"),
        format!("/{plan} {}", card.issue.identifier),
        true,
    )
}

/// A step's button: it runs in this ticket's session; the chevron picks the
/// agent (before a session exists) or, when the profile names the bot's
/// command for it, hands the step to the bot. The bot's commands are Linear
/// comments: none without a ticket.
#[allow(clippy::too_many_arguments)]
fn step_button(
    app: &App,
    card: &Card,
    label: &str,
    hint: &str,
    local: Action,
    remote: Option<story::BotCommand>,
    lead: Option<Intent>,
    extra: Vec<(String, String, Action)>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) -> egui::Rect {
    let remote = remote.filter(|_| !card.pr_only());
    let unset = remote_unset(remote, &crate::profile::get());
    let remote = remote.and_then(|command| Some((command, command.comment()?)));
    let agent = app.claude.agent_of(&card.issue.id);
    let started = app.claude.saved.contains_key(&card.issue.id);
    let hint = hint.replace("Claude session", &format!("{} session", agent.short()));
    // Button and chevron touch, in this row (a nested row sits lower than
    // its neighbours).
    // A click always shows it was taken: "Fixing…" until the session
    // works, then where it works (crate::app::started_state).
    let working = app
        .claude
        .live_of(&card.issue.id)
        .is_some_and(|l| matches!(l.label(), "working" | "starting"));
    let age = app.started.get(&card.issue.id).map(|at| at.elapsed());
    let state = crate::app::started_state(age, working);
    if state == crate::app::Started::Waiting {
        let rect = pending(ui, &pending_verb(label)).rect;
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
        return rect;
    }
    let spacing = std::mem::replace(&mut ui.spacing_mut().item_spacing.x, 0.0);
    let button = match lead {
        Some(intent) => session_lead(ui, agent, label, intent),
        None => session_button(ui, agent, label),
    }
    .on_hover_text(hint);
    let rect = button.rect;
    if button.clicked() {
        actions.push(local.clone());
    }
    if !started || remote.is_some() || unset || !extra.is_empty() {
        chevron_menu(ui, "Another way to do it", |ui| {
            for (text, hint, action) in extra {
                if hover_hand(&ui.button(text)).on_hover_text(hint).clicked() {
                    actions.push(action);
                    ui.close();
                }
            }
            if !started {
                for choice in crate::claude::Agent::offered() {
                    let text = format!("Here, with {}", choice.label());
                    let button = egui::Button::image_and_text(agent_icon(ui, choice), text);
                    if hover_hand(&ui.add(button)).clicked() {
                        actions.push(Action::WithAgent(choice, Box::new(local.clone())));
                        ui.close();
                    }
                }
            }
            if let Some((command, comment)) = remote {
                let text = format!("Remotely, {}: {comment}", story::bot());
                let note = crate::profile::get().bot.poller_note.clone();
                let hint = if note.is_empty() {
                    "Comments the command on the ticket for the bot".to_owned()
                } else {
                    format!("Comments the command on the ticket; {note}")
                };
                let button = egui::Button::new(text);
                if hover_hand(&ui.add(button)).on_hover_text(hint).clicked() {
                    actions.push(Action::AskBot {
                        issue_id: card.issue.id.clone(),
                        command,
                    });
                    ui.close();
                }
            }
            if unset {
                let text = RichText::new(REMOTE_UNSET).color(p().text_2);
                let button = egui::Button::new(text).frame(false);
                let hint = "Settings, bot & PRs: your bot's plan and build commands";
                if hover_hand(&ui.add(button)).on_hover_text(hint).clicked() {
                    actions.push(Action::Show(View::Settings));
                    ui.close();
                }
            }
        });
    }
    ui.spacing_mut().item_spacing.x = spacing;
    ui.add_space(spacing);
    // One short link to the session: where it works, or that it did not answer.
    let note = match state {
        crate::app::Started::Running => Some(("working · open", p().success)),
        crate::app::Started::NoAnswer => Some(("no answer yet · open", p().signal)),
        _ => None,
    };
    if let Some((note, color)) = note
        && link(ui, super::widgets::label(note).color(color).underline())
            .on_hover_text(format!("Opens the ticket's {} session", agent.short()))
            .clicked()
    {
        actions.push(Action::Claude(card.issue.id.clone()));
    }
    rect
}

/// What a session step's button says while it is on its way: "Fix · 3
/// comments" → "Fixing", "Plan" → "Planning".
fn pending_verb(label: &str) -> String {
    let word = label
        .trim_start_matches(|c: char| !c.is_alphabetic())
        .split([' ', '·'])
        .next()
        .unwrap_or_default();
    match word {
        "Fix" => "Fixing",
        "Plan" => "Planning",
        "Re-plan" => "Re-planning",
        "Build" => "Building",
        "Triage" => "Triaging",
        "Investigate" => "Investigating",
        "Resume" => "Resuming",
        _ => "Starting",
    }
    .to_owned()
}

/// Under a step's ▼ when the profile has no command for the bot
/// (`step_button`: only on a step the bot could run).
const REMOTE_UNSET: &str = "Remotely, with your team's bot: set its commands in Settings";

/// A step the bot could run, but the profile has no command for it yet:
/// its ▼ says where to set one.
fn remote_unset(step: Option<story::BotCommand>, profile: &crate::profile::Profile) -> bool {
    step.is_some_and(|command| command.comment_in(profile).is_none())
}

/// The line under a ticket's buttons once the bot was asked.
fn asked_line(command: story::BotCommand, ago: &str, note: &str) -> String {
    let mut line = format!("✳ asked {} to {}", story::bot(), command.what());
    if !ago.is_empty() {
        line.push_str(&format!(" · {ago} ago"));
    }
    if !note.trim().is_empty() {
        line.push_str(&format!(" · {}", note.trim()));
    }
    line
}

/// Investigate, for a ticket one of the bot's sweeps filed where the
/// profile says the sweep investigates (`ktlo.sweeps[].investigate`): here,
/// the ticket's session checks the finding again and finds its cause; the
/// bot's own run sits behind the arrow (`bot.investigateCommand`).
fn investigate(card: &Card) -> Option<(String, Action)> {
    if card.stage == Stage::Done || card.pr_only() {
        return None;
    }
    let profile = crate::profile::get();
    let title = card.issue.title.to_lowercase();
    profile
        .ktlo
        .sweeps
        .iter()
        .find(|s| s.investigate && title.starts_with(&s.prefix))?;
    let id = &card.issue.identifier;
    let command = format!(
        "Investigate {id}: check again, with the data the ticket names, whether what it \
         describes still happens and for whom; find the root cause in the code; then tell me \
         in a few lines what you found and what the fix would be. Post nothing and change \
         nothing without asking me."
    );
    Some((
        "Checks the finding again and finds its cause here, in the ticket's Claude session"
            .to_owned(),
        Action::RunSkill {
            issue_id: card.issue.id.clone(),
            command,
            submit: true,
        },
    ))
}

/// The bot's command for the step, when it can run it remotely: Plan, and
/// Build (which is fix mode on a ticket with a PR). Triage and Re-plan run
/// here only.
fn remote_step(card: &Card) -> Option<story::BotCommand> {
    match card.stage {
        Stage::Planning if card.plan.is_none() => Some(story::BotCommand::Plan),
        Stage::Planning | Stage::Review => Some(story::BotCommand::Build),
        _ => None,
    }
}

/// A parent with sub-tickets can run as one mission: Orbit drives them all
/// through the team's bot (mission.rs). Its own row with Orbit's mark,
/// right under the title, so it never reads as this ticket's next step.
fn mission(app: &App, card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    let kids = &card.issue.children.nodes;
    let bot = crate::profile::get()
        .orchestrator
        .as_ref()
        .map(|o| o.team.clone());
    let Some(bot) = bot.filter(|_| !kids.is_empty() && !card.pr_only()) else {
        return;
    };
    let parent = &card.issue.identifier;
    let running = app.missions.iter().find(|m| &m.parent == parent && !m.done);
    let done = kids
        .iter()
        .filter(|c| matches!(c.state.kind.as_str(), "completed" | "canceled"))
        .count();
    let (title, detail) = match running {
        Some(m) => (
            "Mission running",
            format!(
                "{done} of {} sub-tickets done · {} at a time · click to steer it",
                kids.len(),
                m.parallel
            ),
        ),
        None => (
            "Start mission",
            format!(
                "Orbit drives all {} sub-tickets through {bot}: 2 at a time, blockers first, \
                 until each has a PR",
                kids.len()
            ),
        ),
    };
    ui.add_space(12.0);
    let response = row(ui, false, p().signal, false, |ui| {
        ui.horizontal(|ui| {
            orbit_mark(ui, 26.0);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title)
                        .size(16.0)
                        .family(semibold())
                        .color(p().signal),
                );
                ui.add(egui::Label::new(label(&detail)).wrap());
            });
            if running.is_some() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if link(ui, label("stop"))
                        .on_hover_text("Stop the mission: nothing more goes to the bot")
                        .clicked()
                    {
                        actions.push(Action::StopDrive(parent.clone()));
                    }
                });
            }
        });
    });
    ui.painter().rect_stroke(
        response.rect,
        0.0,
        egui::Stroke::new(1.0, p().signal),
        egui::StrokeKind::Inside,
    );
    if response.clicked() {
        actions.push(match running {
            Some(m) => Action::Steer(m.id.clone()),
            None => Action::Drive {
                parent: parent.clone(),
                parallel: 2,
                initiative: None,
                local: false,
                session: None,
            },
        });
    }
    mark(app, ui, "mission", response.rect);
}

/// The ticket's parent and its sub-tickets, each with its state; a click
/// opens it here (fetched from Linear when it is not on the board).
fn family(card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    let children = &card.issue.children.nodes;
    let done = children
        .iter()
        .filter(|c| matches!(c.state.kind.as_str(), "completed" | "canceled"))
        .count();
    let title = if children.is_empty() {
        "Part of".to_owned()
    } else {
        format!("Sub-tickets · {done} of {} done", children.len())
    };
    section(ui, &title);
    if let Some(parent) = &card.issue.parent {
        ui.horizontal_wrapped(|ui| {
            ui.label(label("part of"));
            let text = format!("{} {}", parent.identifier, parent.title);
            if link(ui, RichText::new(text).color(p().text)).clicked() {
                actions.push(match parent.url.is_empty() {
                    true => Action::Focus(parent.id.clone()),
                    false => Action::Link(parent.url.clone()),
                });
            }
        });
    }
    for child in children {
        ui.horizontal(|ui| {
            let color = match child.state.kind.as_str() {
                "completed" => p().success,
                "canceled" => p().text_3,
                "started" => p().signal,
                _ => p().text_2,
            };
            ui.label(
                RichText::new(&child.state.name)
                    .monospace()
                    .small()
                    .color(color),
            );
            ui.label(label(&child.identifier));
            if link(ui, RichText::new(&child.title).color(p().text))
                .on_hover_text("Open it here")
                .clicked()
            {
                actions.push(Action::Link(child.url.clone()));
            }
            if let Some(who) = &child.assignee {
                ui.label(label(&who.name));
            }
        });
    }
}

fn origin(card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "Where it came from");
    let o = &card.origin;
    // A key column and a value column, split by hairlines.
    let pair = |ui: &mut Ui, k: &str, value: &mut dyn FnMut(&mut Ui)| {
        ui.horizontal_top(|ui| {
            let key = egui::vec2(110.0, 18.0);
            ui.allocate_ui_with_layout(key, Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(key.x);
                ui.label(RichText::new(k).color(p().text_3));
            });
            value(ui);
        });
        ui.add_space(2.0);
        hairline(ui);
        ui.add_space(2.0);
    };
    hairline(ui);
    ui.add_space(2.0);
    pair(ui, "Source", &mut |ui| match &o.source_url {
        Some(url) => {
            let text = RichText::new(format!("{} ↗", o.source))
                .underline()
                .color(p().text);
            if link(ui, text).clicked() {
                actions.push(Action::Open(url.clone()));
            }
        }
        None => {
            ui.label(&o.source);
        }
    });
    let rows = [
        ("Came in as", o.came_in_as.clone()),
        ("Type", o.kind.clone()),
        ("Area", o.area.clone()),
        ("Customers", o.customers.clone()),
        (
            "Seen again",
            (o.seen_again > 0).then(|| format!("{} times after filing", o.seen_again)),
        ),
        (
            "Assignee",
            card.issue.assignee.as_ref().map(|a| a.name.clone()),
        ),
    ];
    for (k, v) in rows {
        if let Some(v) = v {
            pair(ui, k, &mut |ui| {
                ui.add(egui::Label::new(v.as_str()).wrap());
            });
        }
    }
}

/// The bot's case on whether to build at all, before its plan: who asked,
/// the need behind it, the evidence, a smaller answer, its verdict. Not a
/// Build: the reply it suggests, one click into the comment box.
fn the_ask(ask: &story::TheAsk, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "The ask");
    let color = if ask.build() { p().success } else { p().signal };
    ui.label(RichText::new(&ask.verdict).monospace().color(color));
    for (key, value) in [
        ("asked by", &ask.asked_by),
        ("need", &ask.need),
        ("evidence", &ask.evidence),
        ("smaller answer", &ask.smaller),
    ] {
        if value.is_empty() {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(label(key));
            ui.add(egui::Label::new(RichText::new(value).color(p().text_2)).wrap());
        });
    }
    if !ask.build() && !ask.reply.is_empty() {
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(RichText::new(format!("\"{}\"", ask.reply)).color(p().text)).wrap(),
        );
        ui.horizontal(|ui| {
            if ui
                .button("Use this reply")
                .on_hover_text("Puts it in the comment box below, to edit and send")
                .clicked()
            {
                actions.push(Action::DraftComment(ask.reply.clone()));
            }
            ui.label(label("to build it anyway: the Build step's ▼"));
        });
    }
}

fn plan(card: &Card, ui: &mut Ui) {
    let Some(plan) = &card.plan else {
        return;
    };
    section(ui, "The plan");
    if !plan.tldr.is_empty() {
        ui.add(egui::Label::new(RichText::new(&plan.tldr).family(medium())).wrap());
    }
    egui::CollapsingHeader::new(RichText::new("Full plan").color(p().text_2))
        .id_salt(("plan", &card.issue.id))
        .show(ui, |ui| markdown(ui, &plan.body));
}

/// What happened, by day: the steps of the work (planned, building, a PR)
/// lead with a dot on the rail, who did it and the specifics under it; the
/// messages around them (questions, answers, comments) sit quiet and
/// indented, one line each. The same step said twice shows once. Click a
/// row for the full comment.
fn timeline(card: &Card, viewer: &str, ui: &mut Ui) {
    let events = story::condense(&card.timeline);
    if events.is_empty() {
        return;
    }
    section(ui, "What happened");
    // Lines stay readable on a wide pane.
    let width = ui.available_width().min(760.0);
    let rail = 18.0;
    let mut day = String::new();
    let folded = story::folded(&events, 2);
    let open_runs = ui.make_persistent_id(("event-runs", &card.issue.id));
    let opened = ui.data(|d| d.get_temp::<bool>(open_runs)).unwrap_or(false);
    for (i, event) in events.iter().enumerate() {
        let this_day = story::day(&event.at);
        if this_day != day {
            if !day.is_empty() {
                ui.add_space(10.0);
            }
            ui.label(label(&this_day).color(p().text_2));
            ui.add_space(6.0);
            day = this_day;
        }
        // Long runs of messages fold to their last two, behind one line.
        if folded[i] && !opened {
            if i == 0 || !folded[i - 1] {
                let n = folded[i..].iter().take_while(|h| **h).count();
                ui.horizontal(|ui| {
                    ui.add_space(rail);
                    if link(ui, label(&format!("+ {n} earlier messages"))).clicked() {
                        ui.data_mut(|d| d.insert_temp(open_runs, true));
                    }
                });
                ui.add_space(4.0);
            }
            continue;
        }
        let step = story::is_step(event.verb);
        let id = ui.make_persistent_id(("event", &card.issue.id, i));
        let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let expandable = event.body.trim().len() > event.summary.len() + 40;
        let who = if event.who == viewer {
            "You"
        } else {
            event.who.as_str()
        };
        let row = clickable(ui, |ui| {
            ui.set_max_width(width);
            ui.horizontal_top(|ui| {
                ui.add_space(rail);
                ui.vertical(|ui| {
                    if step {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(who).family(medium()).color(p().text));
                            ui.label(RichText::new(event.verb).family(medium()).color(p().text));
                            ui.label(label(&story::clock(&event.at)));
                        });
                        if !event.summary.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(&event.summary).color(p().text_2))
                                    .wrap(),
                            );
                        }
                    } else {
                        // A message: who and its first line, quiet.
                        ui.horizontal(|ui| {
                            ui.label(label(who));
                            let line = if event.summary.is_empty() {
                                event.verb.to_owned()
                            } else {
                                event.summary.clone()
                            };
                            let text = RichText::new(line).color(p().text_2);
                            if open {
                                ui.add(egui::Label::new(text).wrap());
                            } else {
                                ui.add(egui::Label::new(text).truncate());
                            }
                        });
                    }
                    if open && expandable {
                        ui.add_space(4.0);
                        Frame::new()
                            .stroke(Stroke::new(1.0, p().line))
                            .inner_margin(Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                markdown(ui, &event.body);
                            });
                    }
                });
            });
        });
        let rect = row.rect;
        let x = rect.left() + rail / 2.0 - 2.0;
        let painter = ui.painter();
        if step {
            let mark =
                egui::Rect::from_center_size(egui::pos2(x, rect.top() + 9.0), egui::vec2(7.0, 7.0));
            painter.rect_filled(mark, 0.0, theme::tone(event.tone));
        } else {
            painter.line_segment(
                [
                    egui::pos2(x, rect.top() + 2.0),
                    egui::pos2(x, rect.bottom() - 2.0),
                ],
                Stroke::new(1.0, p().line_2),
            );
        }
        if (expandable || !step) && hover_hand(&row).clicked() {
            ui.data_mut(|d| d.insert_temp(id, !open));
        }
        ui.add_space(if step { 10.0 } else { 4.0 });
    }
}

fn composer(draft: &mut String, ui: &mut Ui, actions: &mut Vec<Action>) {
    section(ui, "Reply on the ticket");
    let field = ui.add(
        egui::TextEdit::multiline(draft)
            .hint_text("Write a comment on the ticket.")
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    let send_key =
        field.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        // Secondary: the step's own action above is the one filled button.
        if hover_hand(&ui.button("Send")).clicked() || send_key {
            actions.push(Action::SendComment);
        }
        ui.label(label("⌘ enter"));
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_session_step_says_what_it_does_while_on_its_way() {
        assert_eq!(super::pending_verb("✳ Fix · 2 comments · CI"), "Fixing");
        assert_eq!(super::pending_verb("Plan"), "Planning");
        assert_eq!(super::pending_verb("✳ Investigate"), "Investigating");
        assert_eq!(super::pending_verb("Work on it with Claude"), "Starting");
    }

    #[test]
    fn the_remote_hint_shows_only_where_no_command_is_set() {
        use crate::story::BotCommand;
        let plain = crate::profile::Profile::default();
        assert!(super::remote_unset(Some(BotCommand::Plan), &plain));
        assert!(
            !super::remote_unset(None, &plain),
            "a step the bot cannot run"
        );
        let acme = crate::profile::acme();
        assert!(!super::remote_unset(Some(BotCommand::Plan), &acme));
        assert!(!super::remote_unset(Some(BotCommand::Build), &acme));
    }

    #[test]
    fn asking_the_bot_says_when_and_when_it_picks_up() {
        let line = super::asked_line(crate::story::BotCommand::Plan, "2m", "within 5 minutes");
        assert!(
            line.ends_with("to plan · 2m ago · within 5 minutes"),
            "{line}"
        );
        let bare = super::asked_line(crate::story::BotCommand::Plan, "", " ");
        assert!(bare.ends_with("to plan"), "{bare}");
    }

    use super::*;

    #[test]
    fn fix_ci_passes_only_the_failing_checks_as_the_intent() {
        let (command, hint) = fix_run(&[FixPart::Ci], "build", "fix-pr-comments", "ABC-1");
        assert!(command.starts_with("/build ABC-1 only the failing CI checks"));
        assert!(command.contains("change nothing else"));
        assert!(hint.contains("failing checks only"));
    }

    fn gate(f: impl FnOnce(&mut crate::github::Gate)) -> crate::github::Gate {
        let mut gate = crate::github::Gate {
            state: "OPEN".into(),
            checks: "SUCCESS".into(),
            branch: "CLEAN".into(),
            ..Default::default()
        };
        f(&mut gate);
        gate
    }

    #[test]
    fn the_row_fills_the_one_step_the_pr_needs_next() {
        // A draft can only become ready, even with red checks.
        let draft = gate(|g| {
            g.draft = true;
            g.checks = "FAILURE".into();
        });
        assert_eq!(lead(&draft, true), Some(Lead::MarkReady));
        // Clean: merge it; checks still running and approved: merge when ready.
        assert_eq!(lead(&gate(|g| g.green = true), false), Some(Lead::Merge));
        assert_eq!(
            lead(&gate(|g| g.can_auto_merge = true), false),
            Some(Lead::Merge)
        );
        // Someone else's PR waiting for the user's review: Approve, before Fix.
        let review = gate(|g| {
            g.can_approve = true;
            g.open_threads = 2;
        });
        assert_eq!(lead(&review, false), Some(Lead::Approve));
        // Comments, red CI or a queue drop: Fix.
        assert_eq!(lead(&gate(|g| g.open_threads = 1), false), Some(Lead::Fix));
        assert_eq!(
            lead(&gate(|g| g.checks = "ERROR".into()), false),
            Some(Lead::Fix)
        );
        assert_eq!(
            lead(&gate(|g| g.queue_failed = "checks_failed".into()), false),
            Some(Lead::Fix)
        );
        // The user's own approved PR with red checks, on any card, even a
        // blocked one with several PRs: Fix fills its row (Land it next to it).
        let approved_red = gate(|g| {
            g.own = true;
            g.review = "APPROVED".into();
            g.checks = "FAILURE".into();
        });
        assert_eq!(lead(&approved_red, false), Some(Lead::Fix));
        // The user's own PR without approval: the AI review, else a person.
        let own = gate(|g| {
            g.own = true;
            g.review = "REVIEW_REQUIRED".into();
        });
        assert_eq!(lead(&own, true), Some(Lead::AiReview));
        assert_eq!(lead(&own, false), Some(Lead::RequestReview));
        // Approved, merged, queued: nothing to fill.
        let approved = gate(|g| {
            g.own = true;
            g.review = "APPROVED".into();
        });
        assert_eq!(lead(&approved, true), None);
        assert_eq!(lead(&gate(|g| g.state = "MERGED".into()), true), None);
        assert_eq!(lead(&gate(|g| g.queued = true), true), None);
    }

    #[test]
    fn every_pr_action_is_still_reachable_from_the_row() {
        // Regrouped (lead, outlined, Preview ▾, More ▾), never removed.
        let source = include_str!("ticket.rs");
        let source = source.split("#[cfg(test)]").next().unwrap();
        for action in [
            "Action::ReadyPr(",
            "Action::Approve {",
            "Action::AiReview {",
            "Action::Merge {",
            "Action::AutoMerge {",
            "Action::CancelAutoMerge(",
            "Action::UpdateBranch {",
            "Action::LandBatch(",
            "Action::RequestReview {",
            "Action::LoadReviewers(",
            "Action::AddLabel {",
            "Action::ClosePr {",
            "Action::RefreshGate(",
            "Action::RunSkill {",
        ] {
            assert!(source.contains(action), "{action} is gone from the PR row");
        }
    }

    #[test]
    fn one_fix_button_takes_comments_ci_and_the_queue_together() {
        let threads = gate(|g| g.open_threads = 3);
        let red = gate(|g| {
            g.checks = "FAILURE".into();
            g.open_threads = 1;
            g.queue_failed = "checks_failed".into();
            g.base_branch = "main".into();
        });
        let parts = fix_parts(&[("u/1", &threads), ("u/2", &red)]);
        assert_eq!(parts.len(), 4, "{parts:?}");
        assert_eq!(fix_label(&parts), "✳ Fix · 4 comments · CI · merge queue");
        // Several parts: the build skill's fix mode takes them together.
        let (command, _) = fix_run(&parts, "build", "fix-pr-comments", "ABC-1");
        assert_eq!(command, "/build ABC-1");
        // One part alone runs just that part.
        let only = fix_parts(&[("u/1", &threads)]);
        assert_eq!(fix_label(&only), "✳ Fix · 3 comments");
        let (command, _) = fix_run(&only, "build", "fix-pr-comments", "ABC-1");
        assert_eq!(command, "/fix-pr-comments u/1");
        let queue = fix_parts(&[("u/2", &gate(|g| g.queue_failed = "x".into()))]);
        let (command, _) = fix_run(&queue, "build", "fix-pr-comments", "ABC-1");
        assert!(command.contains("merge queue removed u/2"), "{command}");
        // Changes asked for or conflicts go to the fix mode too.
        let asked = fix_parts(&[("u/3", &gate(|g| g.branch = "DIRTY".into()))]);
        assert_eq!(asked, vec![FixPart::Changes]);
        // A clean PR has nothing to fix: no button.
        assert!(fix_parts(&[("u/4", &gate(|_| {}))]).is_empty());
    }

    #[test]
    fn several_prs_to_review_are_one_ask_in_the_plural() {
        let pr =
            |n| crate::story::pr_from_url(&format!("https://github.com/o/r/pull/{n}"), "").unwrap();
        assert_eq!(ask_title(Ask::Review, &[pr(1)]), "Review the PR");
        assert_eq!(ask_title(Ask::Review, &[pr(1), pr(2)]), "Review the PRs");
        assert_eq!(ask_title(Ask::Verify, &[pr(1), pr(2)]), Ask::Verify.label());
    }

    #[test]
    fn your_own_ticket_in_testing_can_be_marked_done() {
        use crate::story::tests::issue;
        let mut card = crate::story::card(issue("s", "started", vec![]), &Default::default());
        card.stage = Stage::Testing;
        card.ask = None;
        assert!(closes_here(&card, true));
        assert!(!closes_here(&card, false));
        card.stage = Stage::Review;
        assert!(!closes_here(&card, true));
    }

    #[test]
    fn plan_and_build_can_run_remotely_triage_and_re_plan_cannot() {
        use crate::story::tests::issue;
        let mut card = crate::story::card(issue("s", "unstarted", vec![]), &Default::default());
        card.stage = Stage::Planning;
        assert_eq!(remote_step(&card), Some(story::BotCommand::Plan));
        card.plan = Some(crate::story::Plan {
            tldr: String::new(),
            body: String::new(),
        });
        assert_eq!(remote_step(&card), Some(story::BotCommand::Build));
        card.stage = Stage::Review;
        assert_eq!(remote_step(&card), Some(story::BotCommand::Build));
        for stage in [Stage::Intake, Stage::Blocked] {
            card.stage = stage;
            assert_eq!(remote_step(&card), None);
        }
    }

    #[test]
    fn a_ticket_without_a_plan_always_offers_plan() {
        use crate::story::tests::issue;
        let mut card = crate::story::card(issue("s", "triage", vec![]), &Default::default());
        card.stage = Stage::Intake;
        assert_eq!(plan_too(&card).map(|s| s.0), Some("✳ Plan"));
        card.stage = Stage::Building;
        assert_eq!(plan_too(&card).map(|s| s.0), Some("✳ Plan"));
        // Planning shows it as its own step.
        card.stage = Stage::Planning;
        assert_eq!(plan_too(&card), None);
        card.stage = Stage::Intake;
        card.plan = Some(crate::story::Plan {
            tldr: String::new(),
            body: String::new(),
        });
        assert_eq!(plan_too(&card), None);
    }

    #[test]
    fn a_background_session_says_what_it_is_doing() {
        assert_eq!(
            background_note("claude", true, Some("/build ABC-1"), None, None).as_deref(),
            Some("✳ starting claude for /build · claude tab to watch")
        );
        assert_eq!(
            background_note("claude", false, None, None, Some("needs approval")).as_deref(),
            Some("✳ claude: needs approval · open the claude tab")
        );
        assert_eq!(
            background_note("claude", false, None, None, Some("idle")),
            None
        );
        assert_eq!(
            background_note("codex", true, None, None, None).as_deref(),
            Some("✳ starting codex in the background")
        );
        // Fix on a session that was already working: the note names the command.
        assert_eq!(
            background_note("claude", false, None, Some("/build ABC-1"), Some("working"))
                .as_deref(),
            Some("✳ claude is working on /build · claude tab to watch")
        );
        assert_eq!(
            background_note("claude", false, None, Some("/build ABC-1"), Some("idle")).as_deref(),
            Some("✳ sent /build · claude tab to watch")
        );
    }

    #[test]
    fn a_dropped_pr_is_fixed_from_its_own_queue_run_and_never_requeued() {
        let brief =
            super::fix_queue_brief("https://github.com/o/r/pull/384", "failed_checks", "main");
        assert!(brief.contains("pr-384-"));
        assert!(brief.contains("--event merge_group"));
        assert!(brief.contains("failed_checks"));
        assert!(brief.contains("Never add it to the queue again, merge or approve"));
    }

    #[test]
    fn yolo_hands_over_any_open_ticket_the_bot_does_not_have_yet() {
        use crate::story::tests::issue;
        let card = |id: &str, state: &str, kind: &str| {
            let mut i = issue(state, kind, vec![]);
            i.identifier = id.into();
            crate::story::card(i, &Default::default())
        };
        assert!(super::yolo_fits(&card("XYZ-1", "Triage", "triage"), "ABC"));
        assert!(super::yolo_fits(
            &card("XYZ-1", "In Progress", "started"),
            "ABC"
        ));
        assert!(super::yolo_fits(&card("ABC-1", "Triage", "triage"), "ABC"));
        assert!(!super::yolo_fits(
            &card("ABC-1", "Todo", "unstarted"),
            "ABC"
        ));
        assert!(!super::yolo_fits(
            &card("XYZ-1", "Done", "completed"),
            "ABC"
        ));
    }

    #[test]
    fn who_approved_a_pr_is_said_apart_from_who_else_is_on_it() {
        let on_it = vec!["@alex-k approved".to_owned(), "@ana commented".to_owned()];
        let (approved, rest) = super::approvals(&on_it);
        assert_eq!(approved, ["@alex-k"]);
        assert_eq!(rest, ["@ana commented"]);
    }
}
