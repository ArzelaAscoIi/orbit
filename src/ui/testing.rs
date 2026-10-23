//! The Testing tab: how to check a ticket in Testing, one step at a time.
//! UI steps say where to click; terminal steps give a command to copy.
//! Tick the steps as you go; Looks good moves the ticket to done.
//!
//! The plan comes from the ticket's test plan comment, or Claude writes
//! one and posts it (src/testplan.rs). Opening the tab is enough.

use super::*;
use crate::testplan::Kind;

pub(super) fn show(app: &App, card: &Card, ui: &mut Ui, actions: &mut Vec<Action>) {
    let id = &card.issue.id;
    let pending = app.test_plan_pending.contains(id);
    let plan = match app.test_plans.get(id) {
        Some(Ok(plan)) => plan,
        Some(Err(error)) => {
            section(ui, "test plan");
            ui.add(egui::Label::new(RichText::new(error).color(p().danger)).wrap());
            ui.add_space(8.0);
            if app.has_anthropic_key() && ui.button("Try again").clicked() {
                actions.push(Action::TestPlan(id.clone()));
            }
            return;
        }
        None => {
            if !pending {
                actions.push(Action::TestPlan(id.clone()));
            }
            section(ui, "test plan");
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Claude is reading the ticket and the diff for a test plan…")
                        .color(p().text_2),
                );
            });
            ui.label(label(
                "it is posted on the ticket once, then read from there",
            ));
            return;
        }
    };

    if plan.kind == Kind::Terminal {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                steps(app, card, plan, ui, actions);
            });
        return;
    }
    // A UI plan: the steps on the left, the browser to click through them
    // on the right.
    let left = (ui.available_width() * 0.38).clamp(300.0, 420.0);
    ui.horizontal_top(|ui| {
        let size = egui::vec2(left, ui.available_height());
        ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("test-steps")
                .show(ui, |ui| steps(app, card, plan, ui, actions));
        });
        ui.add_space(16.0);
        ui.vertical(|ui| browser_pane(app, card, plan, ui, actions));
    });
}

/// The plan's header, its steps to tick, and Mark done once all are.
fn steps(
    app: &App,
    card: &Card,
    plan: &crate::testplan::TestPlan,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let id = &card.issue.id;
    ui.horizontal(|ui| {
        ui.label(label("// test plan"));
        let place = match plan.kind {
            Kind::Ui => "■ in the ui",
            Kind::Terminal => "■ in a terminal",
        };
        ui.label(label(place).color(p().signal));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = label(&format!("on the ticket as {}", crate::testplan::marker(id)));
            ui.add(egui::Label::new(text).truncate());
        });
    });
    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(&plan.summary).size(17.0).family(semibold())).wrap());
    ui.add_space(8.0);
    // Seen enough? Done in one click; the steps are there when it is not
    // obvious.
    ticket::mark_done(app, card, "Looks good", ui, actions);
    ui.add_space(12.0);

    let mut checked = 0;
    hairline(ui);
    for (i, step) in plan.steps.iter().enumerate() {
        let key = egui::Id::unique(("test-step", id, i));
        let on = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
        checked += usize::from(on);
        ui.horizontal_top(|ui| {
            let tick = clickable(ui, |ui| {
                ui.add_space(2.0);
                check(ui, on);
            });
            if hover_hand(&tick)
                .on_hover_text("Tick when it checks out")
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(key, !on));
            }
            ui.label(label(&format!("{:02}", i + 1)));
            ui.vertical(|ui| {
                let title = RichText::new(&step.title).family(medium());
                let title = if on {
                    title.color(p().text_2).strikethrough()
                } else {
                    title
                };
                ui.label(title);
                ui.add(egui::Label::new(RichText::new(&step.how).color(p().text_2)).wrap());
                if let Some(command) = &step.command {
                    command_box(ui, command);
                }
                ui.horizontal_top(|ui| {
                    ui.label(label("expect"));
                    ui.add(egui::Label::new(&step.expect).wrap());
                });
            });
        });
        ui.add_space(8.0);
        hairline(ui);
        ui.add_space(8.0);
    }

    ui.horizontal(|ui| {
        let total = plan.steps.len();
        ui.label(label(&format!("{checked} / {total} checked")));
    });
    ui.add_space(24.0);
}

/// The ticket's open PR in a repo that deploys previews, and the
/// preview's address once its comment says it is up.
fn preview_of<'a>(app: &App, card: &'a Card) -> Option<(&'a story::Pr, Option<String>)> {
    let preview = &crate::profile::get().github.preview;
    if preview.ready.is_empty() {
        return None;
    }
    let pr = card.prs.iter().find(|pr| {
        matches!(
            app.pr_state(pr),
            story::PrState::Open | story::PrState::Draft
        ) && !preview.labels_for(&pr.repo).is_empty()
    })?;
    let url = match app.gates.get(&pr.url) {
        Some(Ok(gate)) => preview.url_in(&gate.comments),
        _ => None,
    };
    Some((pr, url))
}

/// Dev or the PR's preview, while there is one to pick; then back, reload,
/// the address, and the page itself.
fn browser_pane(
    app: &App,
    card: &Card,
    plan: &crate::testplan::TestPlan,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let dev = app.settings.test_app_url.trim();
    let preview = preview_of(app, card);
    // No test app named (settings.json testAppUrl) and no preview: nothing
    // to browse.
    if dev.is_empty() && preview.is_none() {
        section(ui, "browser");
        let hint =
            label("set a test app URL in Settings (bot & PRs) to click UI plans through here");
        if link(ui, hint.underline()).clicked() {
            actions.push(Action::Show(crate::app::View::Settings));
        }
        return;
    }
    // The preview once it is up, dev until then; the user's pick wins.
    let key = egui::Id::unique(("test-on-preview", &card.issue.id));
    let ready = preview.as_ref().is_some_and(|(_, url)| url.is_some());
    let on_preview = preview.is_some()
        && (dev.is_empty() || ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(ready));
    if let Some((pr, url)) = &preview {
        ui.horizontal(|ui| {
            ui.label(label("// test on"));
            if !dev.is_empty() && ui.selectable_label(!on_preview, "dev").clicked() {
                ui.data_mut(|d| d.insert_temp(key, false));
            }
            if ui.selectable_label(on_preview, "PR preview").clicked() {
                ui.data_mut(|d| d.insert_temp(key, true));
            }
        });
        if on_preview && url.is_none() {
            let asked = crate::profile::get()
                .github
                .preview
                .labels_for(&pr.repo)
                .iter()
                .any(|kind| {
                    app.labeled.contains(&(pr.url.clone(), kind.label.clone()))
                        || matches!(app.gates.get(&pr.url), Some(Ok(g)) if g.labels.contains(&kind.label))
                });
            ui.add_space(6.0);
            if asked {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("The preview is deploying, not ready yet.").color(p().text_2),
                    );
                });
                ui.label(label(
                    "it opens here once the PR says where it runs; pick dev to test there meanwhile",
                ));
            } else {
                ui.label(RichText::new("This PR has no preview yet.").color(p().text_2));
                ui.horizontal_wrapped(|ui| ticket::preview(app, pr, ui, actions));
            }
            return;
        }
        ui.add_space(4.0);
    }
    let base = match &preview {
        Some((_, Some(url))) if on_preview => url.as_str(),
        _ => dev,
    };
    if !crate::browser::available() {
        section(ui, "browser");
        match &plan.url {
            Some(url) => {
                if link(
                    ui,
                    label(&format!("open {url} in your browser")).underline(),
                )
                .clicked()
                {
                    actions.push(Action::Open(url.clone()));
                }
            }
            None => {
                ui.label(label("open the page to test in your browser"));
            }
        }
        return;
    }
    ui.horizontal(|ui| {
        if ui.small_button("←").on_hover_text("Back").clicked() {
            actions.push(Action::BrowserBack(crate::browser::TESTING));
        }
        if ui.small_button("⟳").on_hover_text("Reload").clicked() {
            actions.push(Action::BrowserReload(crate::browser::TESTING));
        }
        let open = ui.small_button("↗").on_hover_text("Open in your browser");
        if open.clicked() {
            actions.push(Action::Open(app.browser.current().to_owned()));
        }
        // The address shows where the page is, until you type in it.
        let field_id = egui::Id::unique("browser-address-field");
        let text_id = egui::Id::unique("browser-address");
        let mut text = if ui.memory(|m| m.has_focus(field_id)) {
            ui.data(|d| d.get_temp::<String>(text_id))
                .unwrap_or_default()
        } else {
            app.browser.current().to_owned()
        };
        let field = ui.add(
            egui::TextEdit::singleline(&mut text)
                .id(field_id)
                .hint_text("address of the page to test")
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            actions.push(Action::BrowserGo(crate::browser::TESTING, text.clone()));
        }
        ui.data_mut(|d| d.insert_temp(text_id, text));
    });
    super::panes::find_bar(&app.browser, crate::browser::TESTING, ui, actions);
    if let Some(error) = app.browser.error() {
        ui.label(label(error).color(p().danger));
    }
    if let Some(url) = &plan.url
        && !crate::browser::loadable(&crate::testplan::page(base, Some(url)))
    {
        ui.label(label(&format!("// the plan says: {url}")));
    }
    ui.add_space(6.0);
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    ui.painter().rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, p().line_2),
        egui::StrokeKind::Inside,
    );
    // The plan's page on dev or the preview; one with placeholders in it
    // cannot be opened, so the app itself, with the plan's words above.
    let page = crate::testplan::page(base, plan.url.as_deref());
    let home = if crate::browser::loadable(&page) {
        page
    } else {
        base.to_owned()
    };
    crate::browser::place(ui.ctx(), crate::browser::TESTING, rect.shrink(1.0), &home);
}

/// A command in a sunken box, with a copy link.
fn command_box(ui: &mut Ui, command: &str) {
    Frame::new()
        .fill(p().side)
        .stroke(Stroke::new(1.0, p().line))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if link(ui, label("copy")).clicked() {
                        ui.ctx().copy_text(command.to_owned());
                    }
                    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        let text = RichText::new(command).monospace().color(p().text);
                        ui.add(egui::Label::new(text).wrap());
                    });
                });
            });
        });
}
