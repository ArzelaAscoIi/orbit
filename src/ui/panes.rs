//! Draws the current window's panes (`app/panes.rs`): ticket panes, each a
//! full ticket view of its own, and shells. The active ticket pane draws
//! from `App`'s view; the others from the view they kept. A session is
//! drawn by one pane at a time, the focused one first; any other pane on
//! it points there.

use super::*;
use crate::app::panes::Surface;
use egui::{CursorIcon, Id, UiBuilder};

pub(super) fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(p().bg))
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            mark(app, ui, "panes", area);
            let layout = app.panes.layout(area);
            let many = layout.panes.len() > 1;
            // A pane that just got focus hands it to its terminal.
            let seen = Id::new("pane-focus-seen");
            let changed = ui.data(|d| d.get_temp::<u64>(seen)) != Some(app.panes.focused);
            ui.data_mut(|d| d.insert_temp(seen, app.panes.focused));
            let pressed = ui.input(|i| {
                i.pointer
                    .any_pressed()
                    .then(|| i.pointer.interact_pos())
                    .flatten()
            });
            let views: Vec<(u64, TicketView)> = layout
                .panes
                .iter()
                .filter_map(|(id, surface, _)| match surface {
                    Surface::Ticket(_) if *id == app.panes.active => Some((*id, app.ticket_view())),
                    Surface::Ticket(view) => Some((*id, view.clone())),
                    Surface::Shell | Surface::Editor | Surface::Browser => None,
                })
                .collect();
            let owners = session_owners(&views, app.panes.focused);

            for (id, surface, rect) in layout.panes {
                if pressed.is_some_and(|at| rect.contains(at)) && app.panes.focused != id {
                    actions.push(Action::FocusPane(id));
                }
                if changed && id == app.panes.focused {
                    ui.data_mut(|d| d.insert_temp(Id::new("terminal-grab"), rect));
                }
                let mut pane = ui.new_child(
                    UiBuilder::new()
                        .max_rect(rect)
                        .id_salt(("pane", id))
                        .layout(Layout::top_down(Align::Min)),
                );
                pane.set_clip_rect(rect.intersect(ui.clip_rect()));
                match surface {
                    Surface::Ticket(_) => {
                        let view = views
                            .iter()
                            .find(|(v, _)| *v == id)
                            .map(|(_, view)| view.clone())
                            .expect("a view per ticket pane");
                        let busy = session_of(&view)
                            .and_then(|key| owners.iter().find(|(k, _)| k == key))
                            .map(|(_, owner)| *owner)
                            .filter(|owner| *owner != id);
                        ticket::show(app, id, &view, busy, &mut pane, actions);
                    }
                    Surface::Shell => shell(app, id, &mut pane, actions),
                    Surface::Editor => editor(app, id, &mut pane, actions),
                    Surface::Browser => browser(app, id, &mut pane, actions),
                }
                if many && id == app.panes.focused {
                    ui.painter().rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(1.0, p().signal),
                        egui::StrokeKind::Inside,
                    );
                }
            }

            for (split, across, whole, divider) in layout.dividers {
                ui.painter().rect_filled(divider, 0.0, p().line_2);
                let handle = ui.interact(
                    divider.expand(3.0),
                    Id::new(("divider", split)),
                    Sense::drag(),
                );
                let cursor = if across {
                    CursorIcon::ResizeHorizontal
                } else {
                    CursorIcon::ResizeVertical
                };
                let handle = handle.on_hover_cursor(cursor);
                if handle.dragged()
                    && let Some(at) = handle.interact_pointer_pos()
                {
                    let ratio = if across {
                        (at.x - whole.left()) / whole.width()
                    } else {
                        (at.y - whole.top()) / whole.height()
                    };
                    actions.push(Action::PaneRatio(split, ratio));
                }
            }
        });
}

/// The session a view shows on its Claude tab, if any.
fn session_of(view: &TicketView) -> Option<&str> {
    (view.tab == Tab::Claude)
        .then_some(view.session.as_deref())
        .flatten()
}

/// Which pane draws each session: the focused pane first, then reading
/// order.
fn session_owners(views: &[(u64, TicketView)], focused: u64) -> Vec<(String, u64)> {
    let mut ordered: Vec<&(u64, TicketView)> = views.iter().collect();
    ordered.sort_by_key(|(id, _)| *id != focused);
    let mut owners: Vec<(String, u64)> = Vec::new();
    for (id, view) in ordered {
        if let Some(key) = session_of(view)
            && !owners.iter().any(|(k, _)| k == key)
        {
            owners.push((key.to_owned(), *id));
        }
    }
    owners
}

/// Small hints for the pane keys at the right of a pane's bar, clickable
/// too: full screen (Cmd+Enter, Esc leaves) and, beside other panes, zoom
/// (Cmd+Shift+Enter). Right to left: drawn after the close ×.
pub(super) fn screen_keys(
    full_screen: bool,
    panes: &crate::app::panes::Panes,
    pane: u64,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let full = if full_screen {
        "esc leave full screen"
    } else {
        "⌘⏎ full screen"
    };
    if link(ui, label(full)).clicked() {
        actions.push(Action::FocusPane(pane));
        actions.push(Action::FullScreen);
    }
    ui.add_space(10.0);
    if !full_screen && (!panes.single() || panes.zoomed) {
        let zoom = if panes.zoomed {
            "⌘⇧⏎ unzoom"
        } else {
            "⌘⇧⏎ zoom"
        };
        if link(ui, label(zoom)).clicked() {
            actions.push(Action::FocusPane(pane));
            actions.push(Action::ZoomPane);
        }
        ui.add_space(10.0);
    }
}

fn shell(app: &mut App, id: u64, ui: &mut Ui, actions: &mut Vec<Action>) {
    let size = egui::vec2(ui.available_width(), PANE_BAR);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_size(size);
        clear_of_clone(ui);
        ui.add_space(10.0);
        if link(ui, label("×").size(14.0))
            .on_hover_text("Close the shell (Cmd+W)")
            .clicked()
        {
            actions.push(Action::ClosePane(Some(id)));
        }
        ui.add_space(10.0);
        screen_keys(app.full_screen, &app.panes, id, ui, actions);
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(12.0);
            let title = app.shell_titles.get(&id).map_or("shell", String::as_str);
            ui.label(RichText::new(title).family(medium()));
        });
    });
    hairline(ui);
    let alive = app.shells.get_mut(&id).is_some_and(|t| t.alive());
    Frame::new()
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            match app.shells.get_mut(&id) {
                Some(terminal) if alive => {
                    actions.extend(crate::claude::show(ui, terminal).map(Action::Link));
                }
                // The shell exited (exit, Ctrl+D): its pane goes, and its
                // window with it when it was the only pane.
                _ => actions.push(Action::ClosePane(Some(id))),
            }
        });
}

/// A plain text editor pane (editor.rs): where to save it, save (Cmd+S),
/// copy all, and the text. Tab and Shift+Tab move the selected lines right
/// and left; copy, paste, select all and undo are the text field's own.
fn editor(app: &mut App, id: u64, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(editor) = app.editors.get_mut(&id) else {
        actions.push(Action::ClosePane(Some(id)));
        return;
    };
    let field = Id::new(("editor", id));
    let focused = ui.memory(|m| m.has_focus(field));
    let size = egui::vec2(ui.available_width(), PANE_BAR);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_size(size);
        clear_of_clone(ui);
        ui.add_space(10.0);
        if link(ui, label("×").size(14.0))
            .on_hover_text("Close the editor (Cmd+W)")
            .clicked()
        {
            actions.push(Action::ClosePane(Some(id)));
        }
        ui.add_space(10.0);
        screen_keys(app.full_screen, &app.panes, id, ui, actions);
        if link(ui, label("copy all")).clicked() {
            ui.ctx().copy_text(editor.text.clone());
            editor.said = "copied".into();
        }
        ui.add_space(8.0);
        let save_key = focused && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));
        if link(ui, label("save ⌘S")).clicked() || save_key {
            actions.push(Action::SaveEditor(id));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("editor").family(medium()));
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut editor.path)
                    .hint_text("save to: ~/path/to/file.py")
                    .font(egui::TextStyle::Monospace)
                    .desired_width((ui.available_width() - 12.0).max(80.0)),
            );
        });
    });
    hairline(ui);
    if !editor.said.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(label(&editor.said));
        });
    }
    // Before the field sees it: Tab moves the selected lines, never focus.
    if focused {
        let (left, right) = ui.input_mut(|i| {
            let left = i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
            (
                left,
                !left && i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
            )
        });
        if (left || right)
            && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field)
        {
            use egui::text::{CCursor, CCursorRange};
            let (a, b) = state.cursor.char_range().map_or((0, 0), |r| {
                (r.primary.index.into(), r.secondary.index.into())
            });
            let before = editor.text.chars().count();
            let (text, start, end) = crate::editor::shift(&editor.text, a, b, left);
            let grown = text.chars().count() as isize - before as isize;
            editor.text = text;
            // No selection: the cursor moves with its line, nothing selected.
            let range = if a == b {
                let at = (a as isize + grown).max(start as isize) as usize;
                CCursorRange::one(CCursor::new(at))
            } else {
                CCursorRange::two(CCursor::new(start), CCursor::new(end))
            };
            state.cursor.set_char_range(Some(range));
            state.store(ui.ctx(), field);
        }
    }
    // Code reads in lines: numbers in a muted gutter, no soft wrap (a long
    // line scrolls sideways), the same padding as a terminal pane.
    let lines = editor.text.split('\n').count();
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let job =
            egui::text::LayoutJob::simple(buf.as_str().to_owned(), font, p().text, f32::INFINITY);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    ui.add_space(8.0);
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(
                    egui::Label::new(RichText::new(gutter(lines)).monospace().color(p().text_3))
                        .selectable(false),
                );
            });
            ui.add_space(12.0);
            let size = ui.available_size();
            ui.add_sized(
                size,
                egui::TextEdit::multiline(&mut editor.text)
                    .id(field)
                    .code_editor()
                    .lock_focus(true)
                    .hint_text("paste or type")
                    .layouter(&mut layouter)
                    .desired_width(f32::INFINITY)
                    .margin(Margin::ZERO)
                    .frame(egui::Frame::NONE),
            );
        });
    });
}

/// The editor's line numbers, right-aligned, one per line of text.
fn gutter(lines: usize) -> String {
    let width = lines.to_string().len().max(2);
    (1..=lines.max(1))
        .map(|n| format!("  {n:>width$}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A browser pane (browser.rs): back, reload, open outside, the address,
/// close; the find bar while open (Cmd+F); the page below.
fn browser(app: &App, id: u64, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(browser) = app.browsers.get(&id) else {
        actions.push(Action::ClosePane(Some(id)));
        return;
    };
    let size = egui::vec2(ui.available_width(), PANE_BAR);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_size(size);
        clear_of_clone(ui);
        ui.add_space(10.0);
        if link(ui, label("×").size(14.0))
            .on_hover_text("Close the browser (Cmd+W)")
            .clicked()
        {
            actions.push(Action::ClosePane(Some(id)));
        }
        ui.add_space(10.0);
        screen_keys(app.full_screen, &app.panes, id, ui, actions);
        if link(ui, label("find ⌘F")).clicked() {
            actions.push(Action::BrowserFindOpen(id));
        }
        ui.add_space(8.0);
        if link(ui, label("↗"))
            .on_hover_text("Open in your browser")
            .clicked()
        {
            actions.push(Action::Open(browser.current().to_owned()));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(12.0);
            if link(ui, label("←")).on_hover_text("Back").clicked() {
                actions.push(Action::BrowserBack(id));
            }
            ui.add_space(6.0);
            if link(ui, label("⟳")).on_hover_text("Reload").clicked() {
                actions.push(Action::BrowserReload(id));
            }
            ui.add_space(8.0);
            address(
                browser.current(),
                id,
                "an address: localhost:3000, app.example.com",
                ui,
                actions,
            );
        });
    });
    hairline(ui);
    find_bar(browser, id, ui, actions);
    if let Some(error) = browser.error() {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(label(error).color(p().danger));
        });
    }
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    mark(app, ui, "browser-pane", rect);
    crate::browser::place(ui.ctx(), id, rect, "about:blank");
}

/// A browser's address field: shows where the page is until you type in
/// it; Enter goes there.
pub(super) fn address(
    current: &str,
    slot: u64,
    hint: &str,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let field_id = Id::new(("browser-address-field", slot));
    let text_id = Id::new(("browser-address", slot));
    let mut text = if ui.memory(|m| m.has_focus(field_id)) {
        ui.data(|d| d.get_temp::<String>(text_id))
            .unwrap_or_default()
    } else {
        current.to_owned()
    };
    let field = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(field_id)
            .hint_text(hint)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY),
    );
    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        actions.push(Action::BrowserGo(slot, text.clone()));
    }
    ui.data_mut(|d| d.insert_temp(text_id, text));
}

/// The find bar over a page, while open: Enter the next match, Shift+Enter
/// the one before, Esc closes it.
pub(super) fn find_bar(
    browser: &crate::browser::Browser,
    slot: u64,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    if !browser.finding {
        return;
    }
    let field_id = Id::new(("browser-find-field", slot));
    let text_id = Id::new(("browser-find", slot));
    let seen_id = Id::new(("browser-find-seen", slot));
    let mut text = ui
        .data(|d| d.get_temp::<String>(text_id))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(label("find"));
        let field = ui.add(
            egui::TextEdit::singleline(&mut text)
                .id(field_id)
                .hint_text("text on the page")
                .font(egui::TextStyle::Monospace)
                .desired_width(260.0),
        );
        // Each opening (Cmd+F) puts the cursor in the field once.
        if ui.data(|d| d.get_temp::<u64>(seen_id)) != Some(browser.find_opened) {
            ui.data_mut(|d| d.insert_temp(seen_id, browser.find_opened));
            field.request_focus();
        }
        let (enter, shift, escape) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.modifiers.shift,
                i.key_pressed(egui::Key::Escape),
            )
        });
        if field.lost_focus() && enter {
            actions.push(Action::BrowserFind {
                slot,
                text: text.clone(),
                backwards: shift,
            });
            field.request_focus();
        }
        if link(ui, label("↑"))
            .on_hover_text("Previous (⇧⏎)")
            .clicked()
        {
            actions.push(Action::BrowserFind {
                slot,
                text: text.clone(),
                backwards: true,
            });
        }
        if link(ui, label("↓")).on_hover_text("Next (⏎)").clicked() {
            actions.push(Action::BrowserFind {
                slot,
                text: text.clone(),
                backwards: false,
            });
        }
        if link(ui, label("×")).on_hover_text("Close (esc)").clicked()
            || (escape && (field.has_focus() || field.lost_focus()))
        {
            actions.push(Action::BrowserFindClose(slot));
        }
    });
    ui.data_mut(|d| d.insert_temp(text_id, text));
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_editor_numbers_every_line() {
        assert_eq!(super::gutter(1), "   1");
        assert_eq!(super::gutter(3), "   1\n   2\n   3");
        assert_eq!(super::gutter(12).lines().last(), Some("  12"));
    }

    use super::*;

    fn on_claude(id: u64, session: &str) -> (u64, TicketView) {
        (
            id,
            TicketView {
                selected: Some(session.into()),
                tab: Tab::Claude,
                session: Some(session.into()),
                diff_pr: None,
            },
        )
    }

    #[test]
    fn a_session_is_drawn_by_one_pane_the_focused_one_first() {
        let views = vec![
            on_claude(0, "ABC-1"),
            on_claude(2, "ABC-1"),
            on_claude(4, "ABC-2"),
        ];
        let owners = session_owners(&views, 2);
        assert_eq!(owners, vec![("ABC-1".into(), 2), ("ABC-2".into(), 4)]);
    }
}
