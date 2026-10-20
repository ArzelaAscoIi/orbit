//! The small pieces every column shares. The instrument look lives here:
//! hairlines instead of cards, a 2px marker on what is selected, square
//! marks, metadata in mono.

use super::*;

/// Page and card padding.
pub(super) const PAD: i8 = 16;
/// The sheet's title bars are this tall, with a hairline under.
pub(super) const BAR: f32 = 52.0;
/// A pane's header: its ticket, its tabs, its stage and PR.
pub(super) const PANE_BAR: f32 = 32.0;

/// A framed box on an empty background: the login and picker screens,
/// and the "start a session" prompts.
pub(super) fn card_frame() -> Frame {
    Frame::new()
        .fill(p().bg)
        .stroke(Stroke::new(1.0, p().line_2))
        .inner_margin(24)
}

/// Padding for anything that is not a full-width row. Returns its rect.
/// The sheet's search field (`App::sheet_query`): one per view, the same
/// widget id everywhere, so the sheet's keys know a field has them.
pub(super) const SEARCH: &str = "sheet-search";

pub(super) fn sheet_search(
    app: &App,
    ui: &mut Ui,
    hint: &str,
    actions: &mut Vec<Action>,
) -> egui::Rect {
    let mut query = app.sheet_query.clone();
    let rect = pad(ui, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .id(egui::Id::new(SEARCH))
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        );
    });
    if query != app.sheet_query {
        actions.push(Action::SheetQuery(query));
    }
    hairline(ui);
    rect
}

/// Every word of the query is somewhere in `text`, any case.
pub(super) fn matches_all(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| text.contains(word))
}

pub(super) fn pad(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) -> egui::Rect {
    Frame::new()
        .inner_margin(Margin::symmetric(PAD, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        })
        .response
        .rect
}

/// A header bar, exactly `height` tall, with a hairline under it, so the
/// panes' headers line up. Returns the bar's rect.
pub(super) fn header_bar(
    ui: &mut Ui,
    height: f32,
    add_contents: impl FnOnce(&mut Ui),
) -> egui::Rect {
    let size = egui::vec2(ui.available_width(), height);
    let rect = ui
        .allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_size(size);
            add_contents(ui);
        })
        .response
        .rect;
    hairline(ui);
    rect
}

/// A sheet view's title bar: the title over one mono line, in a `BAR`, and
/// its keys on the right.
pub(super) fn title_bar(ui: &mut Ui, title: &str, subtitle: &str, keys: &str) {
    header_bar(ui, BAR, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(PAD as f32);
            ui.label(label(keys));
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                title_text(ui, title, subtitle);
            });
        });
    });
}

fn title_text(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.add_space(PAD as f32);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        // A vertical block in a row starts at the top; centre it by hand.
        ui.add_space(if subtitle.is_empty() { 14.0 } else { 7.0 });
        ui.add(egui::Label::new(RichText::new(title).heading()).truncate());
        if !subtitle.is_empty() {
            let text = RichText::new(subtitle).monospace().size(11.0);
            ui.add(egui::Label::new(text.color(p().text_2)).truncate());
        }
    });
}

/// `// group  n` over a hairline, between list rows.
pub(super) fn group_label(ui: &mut Ui, text: &str) {
    group_label_in(ui, label(text));
}

/// A group heading in its own colour (Intake's new tickets in red).
pub(super) fn group_label_in(ui: &mut Ui, text: RichText) {
    Frame::new()
        .inner_margin(Margin {
            left: PAD,
            right: PAD,
            top: 14,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // One label: a truncated label inside a row grows the panel.
            ui.add(egui::Label::new(text).truncate());
        });
    hairline(ui);
}

/// A hairline across the full width of `ui`.
pub(super) fn hairline(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p().line);
}

/// A full-width clickable row: `sel` when selected or hovered, a 2px
/// `marker` on its left edge when selected, and a hairline under it when
/// `rule` is set (list rows; nav rows go without).
pub(super) fn row(
    ui: &mut Ui,
    selected: bool,
    marker: Color32,
    rule: bool,
    add_contents: impl FnOnce(&mut Ui),
) -> egui::Response {
    let response = clickable(ui, |ui| {
        let mut prepared = Frame::new()
            .inner_margin(Margin::symmetric(PAD, if rule { 12 } else { 7 }))
            .begin(ui);
        prepared
            .content_ui
            .set_width(prepared.content_ui.available_width());
        add_contents(&mut prepared.content_ui);
        let rect = prepared.content_ui.min_rect() + prepared.frame.inner_margin;
        if selected || ui.rect_contains_pointer(rect) {
            prepared.frame.fill = p().sel;
        }
        prepared.end(ui);
    });

    let painter = ui.painter();
    if selected {
        let mut bar = response.rect;
        bar.set_width(2.0);
        painter.rect_filled(bar, 0.0, marker);
    }
    if rule {
        let r = response.rect;
        painter.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p().line));
    }
    hover_hand(&response)
}

/// Makes everything `add_contents` draws one click target. The click is
/// registered before the contents, so buttons and fields inside keep their
/// own clicks. Never `.interact(Sense::click())` a response after its
/// contents instead: egui gives a click to the widget registered last, and
/// that swallows every button inside (the test below and
/// `no_click_added_after_contents` hold this).
pub(super) fn clickable(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) -> egui::Response {
    clickable_with(ui, Sense::click(), add_contents)
}

/// `clickable` that can also be dragged (the strip's tabs): the same
/// registration before the contents, so buttons inside keep their clicks.
pub(super) fn clickable_with(
    ui: &mut Ui,
    sense: Sense,
    add_contents: impl FnOnce(&mut Ui),
) -> egui::Response {
    ui.scope_builder(egui::UiBuilder::new().sense(sense), |ui| {
        // A selectable label senses clicks too and would sit on top: a
        // click on the text would select it instead of clicking the row.
        ui.style_mut().interaction.selectable_labels = false;
        add_contents(ui)
    })
    .response
}

/// A clickable line of small text.
pub(super) fn link(ui: &mut Ui, text: RichText) -> egui::Response {
    let response = ui.add(egui::Label::new(text).sense(Sense::click()));
    hover_hand(&response)
}

/// A one-line password field with a hint; `true` when Enter was pressed.
pub(super) fn secret_field(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> bool {
    let field = ui.add(
        egui::TextEdit::singleline(value)
            .password(true)
            .hint_text(hint)
            .desired_width(width),
    );
    field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

pub(super) fn progress(ui: &mut Ui, finished: usize, total: usize, height: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, p().line_2);
    if total > 0 {
        let mut done = rect;
        done.set_width(rect.width() * finished as f32 / total as f32);
        painter.rect_filled(done, 0.0, p().success);
    }
}

/// Enough markdown for tickets and the bot's comments: headings, bullets
/// and numbered steps, quotes, code blocks, rules, and inline bold,
/// italics, `code` and links. Lines stay short enough to read.
pub(super) fn markdown(ui: &mut Ui, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(ui.available_width().min(760.0));
        ui.spacing_mut().item_spacing.y = 5.0;
        let body = ui.style().text_styles[&egui::TextStyle::Body].size;
        let mut code: Option<Vec<&str>> = None;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                match code.take() {
                    Some(lines) => code_block(ui, &lines),
                    None => code = Some(Vec::new()),
                }
                continue;
            }
            if let Some(lines) = &mut code {
                lines.push(line);
                continue;
            }
            if let Some(summary) = trimmed.strip_prefix("<summary>") {
                let summary = summary.trim_end_matches("</summary>");
                md_label(ui, summary, body, p().text, true);
                continue;
            }
            if trimmed.starts_with('<') && trimmed.ends_with('>') {
                continue;
            }
            let indent = (line.len() - line.trim_start().len()) as f32 * 4.0;
            if trimmed.is_empty() {
                ui.add_space(3.0);
            } else if trimmed == "---" || trimmed == "***" {
                ui.add_space(4.0);
                let x = ui.max_rect().x_range();
                let y = ui.cursor().top();
                ui.painter().hline(x, y, Stroke::new(1.0, p().line));
                ui.add_space(6.0);
            } else if trimmed.starts_with('#') {
                let level = trimmed.chars().take_while(|&c| c == '#').count();
                let size = if level <= 2 { body + 3.5 } else { body + 1.5 };
                ui.add_space(8.0);
                let heading = trimmed.trim_start_matches('#').trim();
                md_label(ui, heading, size, p().text, true);
            } else if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                hanging(ui, indent, "•", item, body);
            } else if let Some((number, item)) = trimmed
                .split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            {
                hanging(ui, indent, &format!("{number}."), item, body);
            } else if let Some(quote) = trimmed.strip_prefix('>') {
                let quote = quote.trim_start_matches('>').trim();
                ui.horizontal_top(|ui| {
                    let top = ui.cursor().top();
                    ui.add_space(12.0);
                    let rect = md_label(ui, quote, body, p().text_2, false).rect;
                    let x = rect.left() - 8.0;
                    ui.painter()
                        .vline(x, top..=rect.bottom(), Stroke::new(2.0, p().line_2));
                });
            } else {
                md_label(ui, trimmed, body, p().text, false);
            }
        }
        if let Some(lines) = code {
            code_block(ui, &lines);
        }
    });
}

/// A list item: its marker in a fixed column, the text wrapping beside it.
fn hanging(ui: &mut Ui, indent: f32, marker: &str, text: &str, size: f32) {
    ui.horizontal_top(|ui| {
        ui.add_space(indent);
        let column = egui::vec2(18.0, 0.0);
        ui.allocate_ui_with_layout(column, Layout::left_to_right(Align::Min), |ui| {
            ui.set_min_width(column.x);
            ui.label(RichText::new(marker).size(size).color(p().text_3));
        });
        md_label(ui, text, size, p().text, false);
    });
}

/// Where a clicked markdown link waits for `ui::show`, which turns it into
/// `Action::Link`: a Linear or GitHub link opens here, the rest in the
/// browser. (Markdown draws from views that take no actions.)
pub(super) const MARKDOWN_LINK: &str = "markdown-link";

/// One markdown line as wrapped text; its links clickable, with the hand.
fn md_label(ui: &mut Ui, text: &str, size: f32, color: Color32, strong: bool) -> egui::Response {
    let (job, links) = inline_links(text, size, color, strong);
    if links.is_empty() {
        return ui.add(egui::Label::new(job).wrap());
    }
    let (pos, galley, response) = egui::Label::new(job)
        .wrap()
        .sense(Sense::click())
        .layout_in_ui(ui);
    ui.painter().galley(pos, galley.clone(), color);
    let at = |point: egui::Pos2| {
        let index = galley.cursor_from_pos(point - pos).index.0;
        links
            .iter()
            .find(|(range, _)| range.contains(&index))
            .map(|(_, url)| url.clone())
    };
    if let Some(url) = response.hover_pos().and_then(at) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if response.clicked() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new(MARKDOWN_LINK), url));
        }
    }
    response
}

/// A fenced block: mono inside a hairline frame, never wrapped.
fn code_block(ui: &mut Ui, lines: &[&str]) {
    Frame::new()
        .stroke(Stroke::new(1.0, p().line))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for line in lines {
                ui.label(
                    RichText::new(*line)
                        .monospace()
                        .size(12.0)
                        .color(p().text_2),
                );
            }
        });
}

/// Where each link is in a laid-out line: its char range and its URL.
type Links = Vec<(std::ops::Range<usize>, String)>;

/// One line's inline markdown as styled text: `**bold**`, `*italics*`,
/// `` `code` ``, `[text](url)` (the text, underlined) and `\` escapes,
/// with where its links are. `strong`: the whole line in semibold.
fn inline_links(
    text: &str,
    size: f32,
    color: Color32,
    strong: bool,
) -> (egui::text::LayoutJob, Links) {
    let mut job = egui::text::LayoutJob::default();
    let mut links: Links = Vec::new();
    let (mut bold, mut italic, mut code) = (false, false, false);
    let mut chars = text.char_indices().peekable();
    let mut run = String::new();
    let flush = |job: &mut egui::text::LayoutJob,
                 run: &mut String,
                 (bold, italic, code, link): (bool, bool, bool, bool)| {
        if run.is_empty() {
            return;
        }
        let family = if bold || strong {
            semibold()
        } else {
            egui::FontFamily::Proportional
        };
        let mut format = egui::TextFormat {
            font_id: egui::FontId::new(size, family),
            color,
            italics: italic,
            ..Default::default()
        };
        if code {
            format.font_id = egui::FontId::monospace(size - 1.5);
            format.color = p().text_2;
            format.background = p().sel;
        }
        if link {
            format.underline = Stroke::new(1.0, p().text_3);
        }
        job.append(run, 0.0, format);
        run.clear();
    };
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if !code => {
                if let Some((_, next)) = chars.next() {
                    run.push(next);
                }
            }
            '`' => {
                flush(&mut job, &mut run, (bold, italic, code, false));
                code = !code;
            }
            '*' if !code && chars.peek().is_some_and(|(_, n)| *n == '*') => {
                chars.next();
                flush(&mut job, &mut run, (bold, italic, code, false));
                bold = !bold;
            }
            // An asterisk opens italics before a word and closes after one.
            '*' if !code && (italic || chars.peek().is_some_and(|(_, n)| !n.is_whitespace())) => {
                flush(&mut job, &mut run, (bold, italic, code, false));
                italic = !italic;
            }
            '[' if !code => {
                let rest = &text[i..];
                match (rest.find("]("), rest.find(')')) {
                    (Some(mid), Some(end)) if mid < end => {
                        flush(&mut job, &mut run, (bold, italic, code, false));
                        run.push_str(&rest[1..mid]);
                        let from = job.text.chars().count();
                        let to = from + run.chars().count();
                        links.push((from..to, rest[mid + 2..end].to_owned()));
                        flush(&mut job, &mut run, (bold, italic, code, true));
                        while chars.peek().is_some_and(|(j, _)| *j <= i + end) {
                            chars.next();
                        }
                    }
                    _ => run.push(c),
                }
            }
            c => run.push(c),
        }
    }
    flush(&mut job, &mut run, (bold, italic, code, false));
    (job, links)
}

/// The images in a markdown line, `![alt](url)`: each one's byte range
/// in the line and its URL.
fn image_links(line: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(start) = line[from..].find("![").map(|i| from + i) {
        let Some(mid) = line[start..].find("](").map(|i| start + i + 2) else {
            break;
        };
        let Some(end) = line[mid..].find(')').map(|i| mid + i) else {
            break;
        };
        // `![alt](url "title")`: the URL is up to the first space.
        let url = line[mid..end].split_whitespace().next().unwrap_or_default();
        if url.starts_with("https://") {
            found.push((start..end + 1, url));
        }
        from = end + 1;
    }
    found
}

/// `markdown` with its images drawn, fetched once (`Action::LoadImage`);
/// a click opens the full one in the browser.
// ponytail: chunks between image lines render on their own, so an image
// inside a code block breaks the block; tickets put images on their own lines.
pub(super) fn markdown_images(app: &App, ui: &mut Ui, text: &str, actions: &mut Vec<Action>) {
    let mut chunk = String::new();
    for line in text.lines() {
        let links = image_links(line);
        if links.is_empty() {
            chunk.push_str(line);
            chunk.push('\n');
            continue;
        }
        markdown(ui, &chunk);
        chunk.clear();
        let mut rest = String::new();
        let mut at = 0;
        for (range, _) in &links {
            rest.push_str(&line[at..range.start]);
            at = range.end;
        }
        rest.push_str(&line[at..]);
        if !rest.trim().is_empty() {
            markdown(ui, &rest);
        }
        for (_, url) in links {
            image(app, ui, url, actions);
        }
    }
    markdown(ui, &chunk);
}

fn image(app: &App, ui: &mut Ui, url: &str, actions: &mut Vec<Action>) {
    match app.images.get(url) {
        Some(Ok(texture)) => {
            let width = ui.available_width().min(720.0);
            let image = egui::Image::new(egui::load::SizedTexture::from_handle(texture))
                .max_width(width)
                .sense(Sense::click());
            if ui.add(image).on_hover_text("Open the full image").clicked() {
                actions.push(Action::Open(url.to_owned()));
            }
        }
        Some(Err(error)) => {
            if link(ui, label("image: could not load it ↗").underline())
                .on_hover_text(error)
                .clicked()
            {
                actions.push(Action::Open(url.to_owned()));
            }
        }
        None => {
            actions.push(Action::LoadImage(url.to_owned()));
            ui.label(label("loading image…"));
        }
    }
}

/// `// title`: a section label in lowercase mono.
pub(super) fn section(ui: &mut Ui, title: &str) {
    ui.add_space(18.0);
    ui.label(label(&format!("// {}", title.to_lowercase())));
    ui.add_space(4.0);
}

/// Metadata text: mono, 11px, tertiary.
pub(super) fn label(text: &str) -> RichText {
    RichText::new(text).monospace().size(11.0).color(p().text_3)
}

/// A 6px square; the default fonts have no bullet glyph.
/// A session's state: a spinner while it works (no word), else its dot and
/// its word in `text`.
pub(super) fn state(ui: &mut Ui, status: &str, color: Color32, text: Color32) {
    if matches!(status, "working" | "starting") {
        ui.add(egui::Spinner::new().size(11.0).color(p().success))
            .on_hover_text(status);
        return;
    }
    dot(ui, color);
    ui.add_space(4.0);
    ui.label(label(status).color(text));
}

/// A session's dot, or a spinner while it works.
pub(super) fn state_dot(ui: &mut Ui, status: &str, color: Color32) {
    if matches!(status, "working" | "starting") {
        ui.add(egui::Spinner::new().size(11.0).color(p().success))
            .on_hover_text(status);
    } else {
        dot(ui, color);
    }
}

pub(super) fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 16.0), Sense::hover());
    let square = egui::Rect::from_center_size(rect.center(), egui::vec2(6.0, 6.0));
    ui.painter().rect_filled(square, 0.0, color);
}

/// `■ in review`: the stage in its colour, no pill.
pub(super) fn status(ui: &mut Ui, stage: Stage) {
    let text = format!("■ {}", stage.label().to_lowercase());
    ui.label(
        RichText::new(text)
            .monospace()
            .size(11.0)
            .color(theme::stage(stage)),
    );
}

/// A tag: lowercase mono in a hairline box. A truncated one shows its
/// whole text on hover, as a truncated label does.
pub(super) fn chip(ui: &mut Ui, text: &str) -> egui::Response {
    // Allocated whole, so a wrapped row moves a chip that does not fit to
    // the next line. A Frame around a truncated label shrank the label to
    // what was left on the line and then stuck out of the row: the
    // sheet's card ran under the next column. Only a chip wider than the
    // whole row truncates, and it never gets wider than the row.
    // The old Frame's 6x2 margin plus its 1px stroke on each side.
    let margin = egui::vec2(7.0, 3.0);
    let width = ui.max_rect().width().max(0.0);
    let room = (width - 2.0 * margin.x).max(0.0);
    let full = text.to_lowercase();
    let text = RichText::new(&full)
        .monospace()
        .size(10.0)
        .color(p().text_2);
    let lay = |mode| {
        egui::WidgetText::from(text.clone()).into_galley(
            ui,
            Some(mode),
            room,
            egui::TextStyle::Monospace,
        )
    };
    let mut galley = lay(egui::TextWrapMode::Extend);
    if galley.size().x > room {
        galley = lay(egui::TextWrapMode::Truncate);
    }
    let size = (galley.size() + 2.0 * margin).min(egui::vec2(width, f32::INFINITY));
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let said = galley.text().to_owned();
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &said));
    let elided = galley.elided;
    if ui.is_rect_visible(rect) {
        // Clipped to the chip: in a row narrower than its margins the
        // ellipsis is cut, never drawn past the row.
        let painter = ui.painter_at(rect);
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, p().line_2),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + margin, galley, p().text_2);
    }
    if elided {
        response.on_hover_text(full)
    } else {
        response
    }
}

/// Who opened a ticket or PR, at a glance: you in the accent colour, a
/// bot marked as one, anyone else by name.
pub(super) fn opened_by(ui: &mut Ui, who: crate::story::Who, name: &str) -> egui::Response {
    use crate::story::Who;
    let (text, color) = match who {
        Who::You => ("opened by you".to_owned(), p().signal),
        Who::Bot => (
            format!("opened by {} · bot", name.trim_end_matches("[bot]")),
            p().text_2,
        ),
        Who::Other => (format!("opened by {name}"), p().text),
    };
    Frame::new()
        .stroke(Stroke::new(1.0, color))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(10.0).color(color));
        })
        .response
}

/// Marks what Laya decided or proposed, so it never passes for Haiku's
/// or the user's own: a chip in the accent colour, `why` on hover.
/// Copy link: puts `url` on the clipboard and says so for two seconds.
pub(super) fn copy_link(ui: &mut Ui, url: &str) -> egui::Response {
    let id = egui::Id::new(("copied", url));
    let now = ui.input(|i| i.time);
    let copied = ui
        .data(|d| d.get_temp::<f64>(id))
        .is_some_and(|at| now - at < 2.0);
    let text = if copied { "✓ Copied" } else { "Copy link" };
    let button = hover_hand(&ui.button(text)).on_hover_text(format!("Copies {url}"));
    if button.clicked() {
        ui.ctx().copy_text(url.to_owned());
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    if copied {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    button
}

/// A dot that breathes: work runs right now. Drawn at about 8 frames a
/// second, not every frame as egui's spinner asks, so an open panel does
/// not keep the whole window redrawing.
pub(super) fn pulse(ui: &mut Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let t = ui.input(|i| i.time);
    let alpha = 0.3 + 0.7 * (t * 3.0).sin().abs() as f32;
    ui.painter()
        .circle_filled(rect.center(), 3.0, color.gamma_multiply(alpha));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(120));
}

pub(super) fn laya_chip(ui: &mut Ui, why: &str) {
    Frame::new()
        .stroke(Stroke::new(1.0, p().signal))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            let text = RichText::new("✳ laya").monospace().size(10.0);
            ui.add(egui::Label::new(text.color(p().signal)));
        })
        .response
        .on_hover_text(why);
}

/// The one filled button on a screen.
pub(super) fn primary(ui: &mut Ui, label: &str) -> egui::Response {
    let text = RichText::new(label).family(medium()).color(p().on_btn);
    let button = egui::Button::new(text).fill(p().btn).stroke(Stroke::NONE);
    hover_hand(&ui.add(button))
}

/// What a PR action does, for how it looks: the one step that moves the
/// PR on is filled in its colour, the rest are outlined with the icon in
/// that colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Intent {
    /// Moves the PR on (Mark ready, ask for a review): the primary colour.
    Go,
    /// Approve, merge.
    Good,
    /// Fix what blocks it.
    Fix,
    /// Ends something (Close PR): red.
    Danger,
    /// Everything else.
    Plain,
}

impl Intent {
    /// The fill of the lead button, and its text.
    fn fill(self) -> (Color32, Color32) {
        match self {
            Intent::Good => (p().success, crate::theme::on_fill(p().success)),
            Intent::Fix => (p().signal, crate::theme::on_fill(p().signal)),
            _ => (p().btn, p().on_btn),
        }
    }

    /// The icon of an outlined button.
    fn tint(self) -> Color32 {
        match self {
            Intent::Go => p().text,
            Intent::Good => p().success,
            Intent::Fix => p().signal,
            Intent::Danger => p().danger,
            Intent::Plain => p().text_2,
        }
    }
}

/// A PR action: a Phosphor `icon` (`egui_phosphor::regular`), then `text`
/// (none: an icon button). `lead`: the one filled button of the row.
pub(super) fn action(
    ui: &mut Ui,
    icon: &str,
    text: &str,
    intent: Intent,
    lead: bool,
) -> egui::Response {
    let button = if lead {
        let (fill, color) = intent.fill();
        let label = format!("{icon}  {text}");
        egui::Button::new(
            RichText::new(label.trim_end())
                .family(medium())
                .color(color),
        )
        .fill(fill)
        .stroke(Stroke::NONE)
    } else {
        let text_color = match intent {
            Intent::Danger => p().danger,
            _ => p().text,
        };
        let mut job = egui::text::LayoutJob::default();
        let (style, font) = (ui.style().clone(), egui::FontSelection::Default);
        RichText::new(icon).color(intent.tint()).append_to(
            &mut job,
            &style,
            font.clone(),
            Align::Center,
        );
        if !text.is_empty() {
            RichText::new(format!("  {text}"))
                .color(text_color)
                .append_to(&mut job, &style, font, Align::Center);
        }
        egui::Button::new(job)
    };
    hover_hand(&ui.add(button))
}

/// An action with a menu under it (More, Preview): the button opens it.
pub(super) fn action_menu(
    ui: &mut Ui,
    icon: &str,
    text: &str,
    hint: &str,
    menu: impl FnOnce(&mut Ui),
) -> egui::Response {
    let response = action(ui, icon, text, Intent::Plain, false).on_hover_text(hint);
    egui::Popup::menu(&response).show(menu);
    response
}

/// The ticket session's button filled as the row's lead (Fix): the
/// agent's mark in the fill's text colour.
pub(super) fn session_lead(
    ui: &mut Ui,
    agent: crate::claude::Agent,
    text: &str,
    intent: Intent,
) -> egui::Response {
    let text = text.strip_prefix("✳ ").unwrap_or(text);
    let (fill, color) = intent.fill();
    let icon = agent_icon(ui, agent).tint(color);
    let text = RichText::new(text).family(medium()).color(color);
    let button = egui::Button::image_and_text(icon, text)
        .fill(fill)
        .stroke(Stroke::NONE);
    hover_hand(&ui.add(button))
}

/// Copy as an icon: the URL to the clipboard, a check for two seconds.
pub(super) fn copy_icon(ui: &mut Ui, url: &str) -> egui::Response {
    use egui_phosphor::regular::{CHECK, COPY};
    let id = egui::Id::new(("copied", url));
    let now = ui.input(|i| i.time);
    let copied = ui
        .data(|d| d.get_temp::<f64>(id))
        .is_some_and(|at| now - at < 2.0);
    let icon = if copied { CHECK } else { COPY };
    let button = action(ui, icon, "", Intent::Plain, false).on_hover_text(format!("Copies {url}"));
    if button.clicked() {
        ui.ctx().copy_text(url.to_owned());
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    if copied {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    button
}

/// A primary button that starts a session with `agent`, its mark first.
pub(super) fn agent_primary(
    ui: &mut Ui,
    agent: crate::claude::Agent,
    text: &str,
) -> egui::Response {
    let icon = agent_icon(ui, agent).tint(p().on_btn);
    let text = RichText::new(text).family(medium()).color(p().on_btn);
    let button = egui::Button::image_and_text(icon, text)
        .fill(p().btn)
        .stroke(Stroke::NONE);
    hover_hand(&ui.add(button))
}

/// A row of buttons centred in the space: laid out after last frame's
/// width, since egui places a row before it knows how wide it is.
pub(super) fn centered_row(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui)) {
    let id = ui.id().with(("centered-row", salt));
    let width: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        let row = ui.horizontal(add).response.rect.width();
        if (row - width).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(id, row));
            ui.ctx().request_repaint();
        }
    });
}

/// A thin chevron button right after the button it belongs to; its menu
/// holds the other ways to do the same thing.
pub(super) fn chevron_menu(ui: &mut Ui, hint: &str, menu: impl FnOnce(&mut Ui)) {
    let height = button_height(ui);
    let button = egui::Button::new("").min_size(egui::vec2(height * 0.9, height));
    let response = hover_hand(&ui.add(button)).on_hover_text(hint);
    let (c, s) = (response.rect.center(), 3.5);
    let stroke = Stroke::new(1.3, p().text_2);
    let points = [
        c + egui::vec2(-s, -s * 0.45),
        c + egui::vec2(0.0, s * 0.55),
        c + egui::vec2(s, -s * 0.45),
    ];
    ui.painter().line_segment([points[0], points[1]], stroke);
    ui.painter().line_segment([points[1], points[2]], stroke);
    egui::Popup::menu(&response).show(menu);
}

/// A start with the agent picked: the button starts with `agent` (the
/// default, or the one that should go first), the chevron lists every
/// agent. `start` builds the action for one agent.
pub(super) fn agent_choice(
    ui: &mut Ui,
    agent: crate::claude::Agent,
    default: crate::claude::Agent,
    text: &str,
    first: bool,
    start: impl Fn(crate::claude::Agent) -> Action,
) -> Option<Action> {
    let mut picked = None;
    // Button and chevron touch, in the row they are in (a nested row sits
    // lower than its neighbours).
    let spacing = std::mem::replace(&mut ui.spacing_mut().item_spacing.x, 0.0);
    {
        let main = if first {
            agent_primary(ui, agent, text)
        } else {
            hover_hand(&ui.add(egui::Button::image_and_text(agent_icon(ui, agent), text)))
        };
        if main
            .on_hover_text(format!("With {}", agent.label()))
            .clicked()
        {
            picked = Some(agent);
        }
        chevron_menu(ui, "With another agent", |ui| {
            for choice in crate::claude::Agent::offered() {
                let mut name = choice.label().to_owned();
                if choice == default {
                    name.push_str("  · default");
                }
                let button = egui::Button::image_and_text(agent_icon(ui, choice), name);
                if hover_hand(&ui.add(button)).clicked() {
                    picked = Some(choice);
                    ui.close();
                }
            }
        });
    }
    ui.spacing_mut().item_spacing.x = spacing;
    ui.add_space(spacing);
    // Always named: after Start fresh the key remembers its old agent,
    // which is not what the button says.
    picked.map(|choice| Action::WithAgent(choice, Box::new(start(choice))))
}

/// A button that runs in the ticket's session: the agent's mark in place
/// of the ✳ its label starts with.
pub(super) fn session_button(
    ui: &mut Ui,
    agent: crate::claude::Agent,
    text: &str,
) -> egui::Response {
    let text = text.strip_prefix("✳ ").unwrap_or(text);
    hover_hand(&ui.add(egui::Button::image_and_text(agent_icon(ui, agent), text)))
}

/// A square check box: filled with `signal` when on.
pub(super) fn check(ui: &mut Ui, on: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), Sense::hover());
    let square = egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0));
    let painter = ui.painter();
    painter.rect_stroke(
        square,
        0.0,
        Stroke::new(1.0, p().control),
        egui::StrokeKind::Inside,
    );
    if on {
        painter.rect_filled(square.shrink(2.0), 0.0, p().signal);
    }
}

pub(super) fn hover_hand(response: &egui::Response) -> egui::Response {
    response
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A button's height in the current style: its text line and padding, at
/// least the interact size.
pub(super) fn button_height(ui: &Ui) -> f32 {
    let spacing = ui.spacing();
    let text = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * spacing.button_padding.y;
    spacing.interact_size.y.max(text)
}

/// A button opening Linear: its logomark, then `text`.
pub(super) fn linear_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::image_and_text(linear_logo(ui), text))
}

/// Linear's logomark in the theme's text colour. The alpha of the brand
/// kit's `logo-light.svg` at 64 px (`rsvg-convert -w 64 -h 64`, then
/// `magick … -alpha extract -depth 8 gray:`), so no image crate is needed.
fn linear_logo(ui: &Ui) -> egui::Image<'static> {
    static ALPHA: &Mask = include_bytes!("../../assets/linear-logo-64.alpha");
    mask(ui, "linear-logo", ALPHA)
}

/// Linear's logomark alone: a palette line that is a Linear ticket.
pub(super) fn linear_mark(ui: &Ui) -> egui::Image<'static> {
    linear_logo(ui)
}

/// GitHub's mark, no word: a palette line that is a PR or a repo. Simple
/// Icons' `github.svg` at 64 px (Quick Look, `sips`), alpha from its ink.
pub(super) fn github_mark(ui: &Ui) -> egui::Image<'static> {
    static ALPHA: &Mask = include_bytes!("../../assets/github-logo-64.alpha");
    mask(ui, "github-logo", ALPHA)
}

/// The agent a session runs with, as its mark: Claude's, or OpenAI's for
/// Codex. Made like Linear's, from the Simple Icons `claude.svg` and
/// `openai.svg`.
pub(super) fn agent_icon(ui: &Ui, agent: crate::claude::Agent) -> egui::Image<'static> {
    static CLAUDE: &Mask = include_bytes!("../../assets/claude-logo-64.alpha");
    static CODEX: &Mask = include_bytes!("../../assets/codex-logo-64.alpha");
    // A plain π drawn for this app, not pi's own logo.
    static PI: &Mask = include_bytes!("../../assets/pi-logo-64.alpha");
    match agent {
        crate::claude::Agent::Claude => mask(ui, "claude-logo", CLAUDE),
        crate::claude::Agent::Codex => mask(ui, "codex-logo", CODEX),
        crate::claude::Agent::Pi => mask(ui, "pi-logo", PI),
    }
}

/// Orbit's own mark, painted: a planet, its orbit, and a moon on it, in
/// the signal colour. Marks what Orbit itself runs (a mission), so it never
/// reads as Linear's, GitHub's or one ticket's step.
pub(super) fn orbit_mark(ui: &mut Ui, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let ring = size * 0.42;
    painter.circle_filled(c, size * 0.2, p().signal);
    painter.circle_stroke(c, ring, Stroke::new(1.5, p().signal));
    let at = c + egui::vec2(1.0, -1.0).normalized() * ring;
    painter.circle_filled(at, size * 0.11, p().signal);
    response
}

const MASK: usize = 64;
type Mask = [u8; MASK * MASK];

/// A 64 px alpha mask as a 13 px image in the theme's text colour, its
/// texture made once per name.
fn mask(ui: &Ui, name: &'static str, alpha: &Mask) -> egui::Image<'static> {
    let id = egui::Id::new(name);
    let texture = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id));
    let texture = texture.unwrap_or_else(|| {
        // Premultiplied white; the tint gives it the theme's colour.
        let rgba: Vec<u8> = alpha.iter().flat_map(|&a| [a, a, a, a]).collect();
        let image = egui::ColorImage::from_rgba_premultiplied([MASK, MASK], &rgba);
        let options = egui::TextureOptions {
            mipmap_mode: Some(egui::TextureFilter::Linear),
            ..egui::TextureOptions::LINEAR
        };
        let texture = ui.ctx().load_texture(name, image, options);
        ui.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
        texture
    });
    egui::Image::new((texture.id(), egui::vec2(13.0, 13.0))).tint(p().text)
}

/// The colour of a session's state label.
pub(super) fn status_color(status: &str) -> Color32 {
    let p = p();
    match status {
        "working" => p.text_2,
        "needs approval" | "your turn" => p.signal,
        "idle" => p.text_3,
        "failed" | "crashed" => p.danger,
        _ => p.text_3,
    }
}

/// In a right-to-left row under the clone's switch (ui/mod.rs, top right):
/// leaves its room empty, so the switch never covers what the row says.
pub(super) fn clear_of_clone(ui: &mut Ui) {
    let Some(switch) = ui
        .ctx()
        .memory(|m| m.area_rect(egui::Id::new(CLONE_SWITCH)))
    else {
        return;
    };
    let here = ui.max_rect();
    if here.top() < switch.bottom() && here.bottom() > switch.top() && here.right() > switch.left()
    {
        ui.add_space(here.right() - switch.left() + 8.0);
    }
}

/// The clone switch's egui area id.
pub(super) const CLONE_SWITCH: &str = "clone-switch";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_markdown_link_keeps_where_it_is_and_its_url() {
        let (job, links) = inline_links(
            "See [ABC-1](https://linear.app/x/issue/ABC-1/y) now",
            13.0,
            Color32::WHITE,
            false,
        );
        assert_eq!(job.text, "See ABC-1 now");
        assert_eq!(
            links,
            [(4..9, "https://linear.app/x/issue/ABC-1/y".to_owned())]
        );
    }

    #[test]
    fn inline_markdown_becomes_styled_text_without_its_marks() {
        let (job, _) = inline_links(
            r"\[Acme\] **bold** and *it* with `code` and [a link](https://x.io) *",
            13.5,
            Color32::WHITE,
            false,
        );
        assert_eq!(job.text, "[Acme] bold and it with code and a link *");
        let styled = |needle: &str| {
            let at = job.text.find(needle).unwrap();
            let section = job
                .sections
                .iter()
                .find(|s| s.byte_range.contains(&egui::epaint::text::ByteIndex(at)))
                .unwrap();
            section.format.clone()
        };
        assert_eq!(styled("bold").font_id.family, semibold());
        assert!(styled("it ").italics);
        assert_eq!(styled("code").font_id.family, egui::FontFamily::Monospace);
        assert!(styled("a link").underline.width > 0.0);
        assert!(!styled("[Acme]").italics);
    }

    #[test]
    fn images_are_found_in_a_line_with_their_place() {
        let line = "see ![shot.png](https://uploads.linear.app/a/b.png) and ![x](https://i.io/c.gif \"t\") end";
        let found = image_links(line);
        let urls: Vec<&str> = found.iter().map(|(_, u)| *u).collect();
        assert_eq!(
            urls,
            ["https://uploads.linear.app/a/b.png", "https://i.io/c.gif"]
        );
        assert_eq!(
            &line[found[0].0.clone()],
            "![shot.png](https://uploads.linear.app/a/b.png)"
        );
        assert!(image_links("a [link](https://x.io) and ![local](file.png)").is_empty());
    }

    enum At {
        Button,
        Label,
        Beside,
    }

    /// Clicks on the button inside a row, on its label, or beside them, and
    /// says whether the button and the row saw the click.
    fn click_in_row(at: At) -> (bool, bool) {
        let ctx = egui::Context::default();
        let (mut button, mut whole) = (false, false);
        let mut frame = |events: Vec<egui::Event>| {
            let mut target = (egui::Rect::NOTHING, egui::Rect::NOTHING);
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                let r = row(ui, false, Color32::RED, true, |ui| {
                    target.0 = ui.label("text").rect;
                    let b = ui.button("Save");
                    target.1 = b.rect;
                    button |= b.clicked();
                });
                whole |= r.clicked();
            })
            .textures_delta
            .clear();
            target
        };
        let (text, save) = frame(vec![]);
        let at = match at {
            At::Button => save.center(),
            At::Label => text.center(),
            At::Beside => save.right_center() + egui::vec2(100.0, 0.0),
        };
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(at)]);
        frame(vec![press(true)]);
        frame(vec![press(false)]);
        (button, whole)
    }

    #[test]
    fn chips_wrap_inside_their_row_instead_of_sticking_out() {
        let ctx = egui::Context::default();
        let words = [
            "✳ claude",
            "◧ in a pane",
            "filed by andreas krivas",
            "sre continuous improvement",
            "a story name far wider than the row it sits in, which truncates",
        ];
        let mut row = egui::Rect::NOTHING;
        let mut chips = Vec::new();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_max_width(160.0);
            row = ui.max_rect();
            ui.horizontal_wrapped(|ui| {
                for word in words {
                    let whole = ui
                        .painter()
                        .layout_no_wrap(
                            word.to_lowercase(),
                            egui::FontId::monospace(10.0),
                            p().text_2,
                        )
                        .size()
                        .x
                        + 14.0;
                    chips.push((chip(ui, word).rect, whole));
                }
            });
        })
        .textures_delta
        .clear();
        for (rect, _) in &chips {
            assert!(
                rect.right() <= row.right(),
                "{rect:?} sticks out of {row:?}"
            );
        }
        // A chip keeps its whole text, or, only when it is wider than the
        // whole row, fills the row: never cut to what a part-filled line
        // had left.
        for ((rect, whole), word) in chips.iter().zip(words) {
            let kept = (rect.width() - whole).abs() < 0.5;
            // Truncation ends on a whole glyph, so up to one mono glyph short.
            let fills = *whole > row.width() && rect.width() > row.width() - 7.0;
            assert!(kept || fills, "{word} was cut at {rect:?} in {row:?}");
        }
        for pair in chips.windows(2) {
            let (before, after) = (pair[0].0, pair[1].0);
            assert!(
                after.left() >= before.right() || after.top() >= before.bottom(),
                "{after:?} overlaps {before:?}"
            );
        }
        let (last, whole) = chips[4];
        assert!(last.width() < whole, "the widest chip should truncate");
        assert!(
            chips
                .iter()
                .map(|(r, _)| r.top())
                .collect::<Vec<_>>()
                .windows(2)
                .any(|t| t[1] > t[0])
        );

        let mut narrow = (egui::Rect::NOTHING, egui::Rect::NOTHING);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_max_width(8.0);
            let right = ui.max_rect();
            narrow = (
                right,
                ui.horizontal_wrapped(|ui| chip(ui, "claude")).inner.rect,
            );
        })
        .textures_delta
        .clear();
        assert!(
            narrow.1.right() <= narrow.0.right(),
            "a narrow row's chip sticks out"
        );
    }

    #[test]
    fn a_button_inside_a_row_gets_its_click() {
        assert_eq!(
            click_in_row(At::Button),
            (true, false),
            "the row took the button's click"
        );
        assert_eq!(
            click_in_row(At::Beside),
            (false, true),
            "the row lost its own click"
        );
        assert_eq!(
            click_in_row(At::Label),
            (false, true),
            "a click on the row's text selected it instead"
        );
    }

    #[test]
    fn no_click_added_after_contents() {
        // Every file in src/ui, so a new view is held too; this one says
        // the pattern in its own text, so it is left out.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
        let mut ui = String::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().unwrap() != "widgets.rs" {
                ui += &std::fs::read_to_string(path).unwrap();
            }
        }
        let flat: String = ui.split_whitespace().collect();
        assert!(
            !flat.contains("interact(Sense::click())") && !flat.contains("Sense::click(),)"),
            "a click added to a response after its contents swallows the buttons inside; \
             use widgets::clickable"
        );
    }
}
