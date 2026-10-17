//! Find on the screen (Cmd+F), like a browser's: every text Orbit drew this
//! frame is searched, whatever view drew it (a ticket, the diff, a session,
//! the sheet). Matches are marked, the current one stronger; Enter and
//! Shift+Enter step through them and scroll the current one into view, Esc
//! closes. Only what is on screen is searched: egui skips drawing text a
//! scroll area holds out of view, and a terminal draws only its screen.
//! ponytail: scroll to find more; search the views' own text if that is
//! not enough.

use super::*;
use egui::epaint::text::cursor::CCursor;
use egui::{Rect, Shape};

/// Scrolls to the match picked last frame, before the views draw: their
/// scroll areas take the target as they lay out.
pub(super) fn jump(app: &mut App, ui: &Ui) {
    if let Some(rect) = app.find.as_mut().and_then(|f| f.jump.take()) {
        ui.scroll_to_rect(rect, Some(Align::Center));
    }
}

pub(super) fn show(app: &mut App, ui: &mut Ui) {
    let Some(find) = app.find.as_mut() else {
        return;
    };
    let ctx = ui.ctx().clone();
    let (mut enter, mut back, mut escape) = (false, false, false);
    let bar = egui::Area::new(egui::Id::new("find"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 48.0))
        .show(&ctx, |ui| {
            Frame::new()
                .fill(p().side)
                .stroke(Stroke::new(1.0, p().signal))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut find.query)
                                .hint_text("find on screen")
                                .desired_width(220.0),
                        );
                        // Enter ends the edit in egui: read it, then keep the focus.
                        if field.lost_focus() {
                            ui.input(|i| {
                                let shift = i.modifiers.shift;
                                if i.key_pressed(egui::Key::Enter) {
                                    (enter, back) = (!shift, shift);
                                }
                                escape = i.key_pressed(egui::Key::Escape);
                            });
                        }
                        if find.focus || enter || back {
                            field.request_focus();
                            find.focus = false;
                        }
                        if field.changed() {
                            find.current = 0;
                        }
                        // Last frame's count: the bar's own layer is never searched.
                        let count = match find.matches {
                            0 if find.query.trim().is_empty() => String::new(),
                            0 => "no matches".to_owned(),
                            n => format!("{} of {n}", find.current.min(n - 1) + 1),
                        };
                        ui.label(label(&count).color(p().text_2));
                    })
                    .response
                })
                .inner
        });
    if escape {
        app.find = None;
        return;
    }
    let marks = all_matches(&ctx, &find.query, bar.response.layer_id);
    if marks.is_empty() {
        find.current = 0;
    } else {
        let n = marks.len();
        if enter {
            find.current = (find.current + 1) % n;
        }
        if back {
            find.current = (find.current + n - 1) % n;
        }
        find.current = find.current.min(n - 1);
        if enter || back {
            find.jump = Some(marks[find.current].0);
            ctx.request_repaint();
        }
    }
    find.matches = marks.len();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("find-marks"),
    ));
    for (i, (rect, clip)) in marks.iter().enumerate() {
        let color = if i == find.current {
            p().signal.gamma_multiply(0.55)
        } else {
            p().signal.gamma_multiply(0.25)
        };
        painter.with_clip_rect(*clip).rect_filled(*rect, 0.0, color);
    }
}

/// Every match of `query` in the text drawn this frame, outside `skip`
/// (the find bar itself): its rect and the clip it was drawn in, in
/// reading order.
fn all_matches(ctx: &egui::Context, query: &str, skip: egui::LayerId) -> Vec<(Rect, Rect)> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut ids: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
    if !ids.contains(&egui::LayerId::background()) {
        ids.push(egui::LayerId::background());
    }
    ctx.graphics(|layers| {
        for id in ids.into_iter().filter(|id| *id != skip) {
            for clipped in layers.get(id).into_iter().flat_map(|l| l.all_entries()) {
                collect(&clipped.shape, clipped.clip_rect, query, &mut out);
            }
        }
    });
    out.sort_by(|a, b| {
        (a.0.top(), a.0.left())
            .partial_cmp(&(b.0.top(), b.0.left()))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn collect(shape: &Shape, clip: Rect, query: &str, out: &mut Vec<(Rect, Rect)>) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, clip, query, out)),
        Shape::Text(text) => {
            let galley = &text.galley;
            for (start, end) in spans(galley.text(), query) {
                let a = galley.pos_from_cursor(CCursor::new(start));
                let b = galley.pos_from_cursor(CCursor::new(end));
                // A match the wrap split: marked to the end of its first row.
                let right = if (a.top() - b.top()).abs() < 1.0 {
                    b.right()
                } else {
                    galley.rect.right()
                };
                let rect = Rect::from_min_max(a.min, egui::pos2(right, a.bottom()));
                out.push((rect.translate(text.pos.to_vec2()), clip));
            }
        }
        _ => {}
    }
}

/// Where `query` occurs in `text`, ignoring case, as char ranges.
fn spans(text: &str, query: &str) -> Vec<(usize, usize)> {
    let fold = |s: &str| -> Vec<char> {
        s.chars()
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect()
    };
    let (text, query) = (fold(text), fold(query.trim()));
    if query.is_empty() || query.len() > text.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + query.len() <= text.len() {
        if text[i..i + query.len()] == query[..] {
            out.push((i, i + query.len()));
            i += query.len();
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_match_ignoring_case_by_char() {
        assert_eq!(spans("Ärger ärger", "ÄR"), [(0, 2), (6, 8)]);
        assert_eq!(spans("aaaa", "aa"), [(0, 2), (2, 4)]);
        assert!(spans("abc", " ").is_empty());
        assert!(spans("ab", "abc").is_empty());
    }
}
