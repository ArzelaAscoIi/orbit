//! The PR's diff: each file with its +/- counts, then its lines, colored,
//! with Claude's walkthrough of it on request. A ticket with several PRs
//! picks the one to show in the header.

use super::*;
use crate::anthropic::{Span, Step, Think};
use crate::github::{Draft, File, Line};
use crate::story::Pr;

pub(super) fn show(app: &mut App, card: &Card, pr: &Pr, ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        if card.prs.len() > 1 {
            // The shown PR underlined, the others links to their diffs.
            for other in &card.prs {
                let name = RichText::new(other.name()).monospace();
                if other.url == pr.url {
                    ui.label(name.color(p().text).underline());
                } else if link(ui, name.color(p().text_3))
                    .on_hover_text("Show this PR's diff")
                    .clicked()
                {
                    actions.push(Action::DiffPr(other.url.clone()));
                }
            }
        } else {
            ui.label(RichText::new(pr.name()).monospace());
        }
        if ui.small_button("⟳").on_hover_text("Load again").clicked() {
            actions.push(Action::ReloadDiff);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let github = egui_phosphor::regular::ARROW_SQUARE_OUT;
            if action(ui, github, "GitHub", Intent::Plain, false)
                .on_hover_text("The PR's files on GitHub")
                .clicked()
            {
                actions.push(Action::Open(format!("{}/files", pr.url)));
            }
            // Review done here, approve and merge here: same gates and the
            // same filled lead as the ticket's row, but not on a diff new
            // commits left behind. Right to left: Approve reads first.
            if app.moved(&pr.url).is_none() {
                let lead = super::ticket::lead_of(app, pr);
                super::ticket::update_branch(app, pr, ui, actions);
                super::ticket::merge(app, pr, lead, ui, actions);
                review_menu(app, pr, ui);
                super::ticket::approve(app, pr, lead, ui, actions);
            }
        });
    });
    review_composer(app, pr, ui, actions);
    ui.add_space(8.0);
    super::ticket::pr_verdict(app, pr, false, false, ui);
    ui.add_space(6.0);
    if let Some((seen, now)) = app.changed(&pr.url) {
        let short = |h: &str| h[..7.min(h.len())].to_owned();
        let what = match app.moved(&pr.url) {
            Some(_) => "new commits since you loaded this diff",
            None => "new commits since your review",
        };
        let frame = Frame::new()
            .stroke(Stroke::new(1.0, p().signal))
            .inner_margin(Margin::symmetric(12, 8));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(label(&format!(
                    "{what} · {} → {}",
                    short(&seen),
                    short(&now)
                )));
                let again = primary(ui, "Review the changes").on_hover_text(
                    "Loads the diff again and walks you through what changed since, \
                     and whether your comments were handled. Unposted comments are \
                     dropped: they point at the old lines.",
                );
                mark(app, ui, "review-again", again.rect);
                if again.clicked() {
                    actions.push(Action::ReviewAgain(pr.url.clone()));
                }
            });
        });
        ui.add_space(8.0);
    }
    let files = match app.diffs.get(&pr.url) {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Loading the diff…").color(p().text_2));
            });
            return;
        }
        Some(Err(error)) => {
            ui.add(egui::Label::new(RichText::new(error).color(p().danger)).wrap());
            ui.add_space(8.0);
            let hint = RichText::new(
                "Connect GitHub: run `gh auth login`, or add a token in Settings, connections.",
            )
            .color(p().text_2)
            .underline();
            if link(ui, hint).clicked() {
                actions.push(Action::Show(View::Settings));
            }
            return;
        }
        Some(Ok(files)) => files.clone(),
    };
    let all = files.clone();
    let (files, step) = guide(app, card, &pr.url, &files, ui, actions);
    let rect = ui.scope(|ui| conversation(app, &pr.url, ui)).response.rect;
    mark(app, ui, "line-comments", rect);
    // Every suggestion of the walkthrough sits at its line, whichever step
    // is open; the open step's lines are the ones marked.
    let suggestions: Vec<crate::anthropic::Think> = match app.guides.get(&pr.url) {
        Some(Ok(guide)) => guide.steps.iter().flat_map(|s| s.think.clone()).collect(),
        _ => Vec::new(),
    };
    let threads: &[crate::github::Thread] = match app.talk.get(&pr.url) {
        Some(Ok(talk)) => &talk.threads,
        _ => &[],
    };
    let compose = egui::Id::new(("compose", &pr.url));
    let folders: Vec<String> = app
        .folded
        .iter()
        .filter(|(url, _)| *url == pr.url)
        .map(|(_, folder)| folder.clone())
        .collect();
    let paths: Vec<&str> = all.iter().map(|f| f.path.as_str()).collect();
    let rows = tree(&paths);
    let viewed_paths: Vec<String> = all
        .iter()
        .filter(|f| app.viewed.contains(&viewed_key(&pr.url, f)))
        .map(|f| f.path.clone())
        .collect();
    let files: Vec<File> = files
        .into_iter()
        .filter(|f| !hidden(&f.path, &folders))
        .collect();
    let (added, removed) = files
        .iter()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    ui.horizontal(|ui| {
        ui.label(label(&format!("{} files", files.len())));
        if !viewed_paths.is_empty() {
            ui.label(
                label(&format!("· {}/{} viewed", viewed_paths.len(), all.len())).color(p().text_3),
            );
        }
        if files.len() < all.len() {
            ui.label(label(&format!("· {} folded", all.len() - files.len())).color(p().text_3));
        }
        ui.label(label(&format!("+{added}")).color(p().success));
        ui.label(label(&format!("−{removed}")).color(p().danger));
        ui.add_space(12.0);
        // Side by side by default; one column on a click.
        let unified = app.unified_diff;
        if ui.selectable_label(!unified, label("split")).clicked() {
            actions.push(Action::UnifiedDiff(false));
        }
        if ui.selectable_label(unified, label("unified")).clicked() {
            actions.push(Action::UnifiedDiff(true));
        }
    });
    ui.add_space(6.0);
    const MAX_LINES: usize = 4000;
    let mut shown = 0;
    // The folders on the side, GitHub style, once there is a tree to show.
    let side = rows.iter().any(|r| r.folder) && ui.available_width() > 700.0;
    ui.horizontal_top(|ui| {
        if side {
            let tree_rect = ui
                .vertical(|ui| {
                    ui.set_width(220.0);
                    egui::ScrollArea::vertical()
                        .id_salt("file-tree")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            file_tree(&rows, &folders, &viewed_paths, &pr.url, ui, actions)
                        });
                })
                .response
                .rect;
            mark(app, ui, "diff-tree", tree_rect);
            ui.add_space(12.0);
        }
        ui.vertical(|ui| {
            egui::ScrollArea::both()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for file in &files {
                        ui.add_space(8.0);
                        // Viewed, GitHub style: folded until the file changes.
                        // A click on the path folds or opens it either way.
                        let key = viewed_key(&pr.url, file);
                        let viewed = app.viewed.contains(&key);
                        let toggled = app.folded_files.contains(&(pr.url.clone(), file.path.clone()));
                        let folded = viewed != toggled;
                        ui.horizontal(|ui| {
                            let arrow = if folded { "▸" } else { "▾" };
                            let path = RichText::new(format!("{arrow} {}", file.path))
                                .monospace()
                                .color(if viewed { p().text_3 } else { p().text });
                            if link(ui, path).on_hover_text("Fold or open this file").clicked() {
                                actions.push(Action::FoldFile(pr.url.clone(), file.path.clone()));
                            }
                            ui.label(label(&format!("+{}", file.added)).color(p().success));
                            ui.label(label(&format!("−{}", file.removed)).color(p().danger));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let mut ticked = viewed;
                                let check = ui
                                    .checkbox(&mut ticked, label("viewed"))
                                    .on_hover_text("Mark it reviewed: it folds, and opens again when the file changes");
                                mark(app, ui, "diff-viewed", check.rect);
                                if check.clicked() {
                                    actions.push(Action::ViewedFile(key.clone()));
                                }
                            });
                        });
                        if folded {
                            continue;
                        }
                        let half = (ui.available_width() - 8.0) / 2.0;
                        let rows = if app.unified_diff || one_sided(file) {
                            (1..=file.lines.len()).map(Row::One).collect()
                        } else {
                            pair(&file.lines)
                        };
                        for row in rows {
                            if shown >= MAX_LINES {
                                break;
                            }
                            let cx = Cx {
                                app,
                                url: &pr.url,
                                all: &all,
                                file,
                                step: step.as_ref(),
                                suggestions: &suggestions,
                                threads,
                                compose,
                            };
                            match row {
                                Row::One(n) => {
                                    shown += 1;
                                    cell(&cx, n, None, ui);
                                    annotate(&cx, n, ui, actions);
                                }
                                Row::Two(left, right) => {
                                    shown += 1;
                                    // What is said about a line whose other side is empty sits
                                    // there, beside the line, so the code column stays in one piece.
                                    let margin = lone(left, right).filter(|n| annotated(&cx, *n, ui));
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        for (side, old) in [(left, true), (right, false)] {
                                            match (side, margin) {
                                                (Some(n), _) => {
                                                    cell(&cx, n, Some((half, old)), ui);
                                                }
                                                (None, Some(n)) => {
                                                    ui.vertical(|ui| {
                                                        ui.set_width(half);
                                                        annotate(&cx, n, ui, actions);
                                                    });
                                                }
                                                (None, None) => {
                                                    ui.allocate_exact_size(
                                                        egui::vec2(half, 1.0),
                                                        Sense::hover(),
                                                    );
                                                }
                                            }
                                        }
                                    });
                                    if margin.is_some() {
                                        continue;
                                    }
                                    // Under the pair, what is said about either side, under its own column.
                                    if let Some(n) = left {
                                        beside(&cx, n, half, ui, actions);
                                    }
                                    if let Some(n) = right.filter(|r| Some(*r) != left) {
                                        beside(&cx, n, half, ui, actions);
                                    }
                                }
                            }
                        }
                    }
                    if shown >= MAX_LINES {
                        ui.add_space(8.0);
                        ui.label(RichText::new("The rest is on GitHub.").color(p().text_3));
                    }
                });
        });
    });
}

/// A file that only adds lines (a new file) or only removes them: side by
/// side would leave one half empty, so it shows in one column.
fn one_sided(file: &File) -> bool {
    let has = |f: fn(&Line) -> bool| file.lines.iter().any(f);
    !has(|l| matches!(l, Line::Added(_))) || !has(|l| matches!(l, Line::Removed(_)))
}

/// A file marked viewed, keyed by PR, path and content: a new commit that
/// changes the file unmarks it. ponytail: hashes the file every frame,
/// cache per diff if big PRs get slow.
pub(crate) fn viewed_key(url: &str, file: &File) -> String {
    let hash = crate::store::text_hash(&format!("{:?}", file.lines));
    format!("{url}\t{}\t{hash:x}", file.path)
}

/// A row of the diff's file tree: a folder or a file, at its depth.
#[derive(Debug, PartialEq)]
struct TreeRow {
    depth: usize,
    name: String,
    path: String,
    folder: bool,
}

/// The folders and files of `paths`, each folder before what is in it.
fn tree(paths: &[&str]) -> Vec<TreeRow> {
    let mut sorted = paths.to_vec();
    // Sorted, everything under a folder sits together after it.
    sorted.sort_unstable();
    let mut rows: Vec<TreeRow> = Vec::new();
    for path in sorted {
        let parts: Vec<&str> = path.split('/').collect();
        for depth in 0..parts.len() - 1 {
            let folder = parts[..=depth].join("/");
            if !rows.iter().any(|r| r.folder && r.path == folder) {
                rows.push(TreeRow {
                    depth,
                    name: parts[depth].into(),
                    path: folder,
                    folder: true,
                });
            }
        }
        rows.push(TreeRow {
            depth: parts.len() - 1,
            name: parts[parts.len() - 1].into(),
            path: path.into(),
            folder: false,
        });
    }
    rows
}

/// A path inside one of the folded folders.
fn hidden(path: &str, folded: &[String]) -> bool {
    folded.iter().any(|f| {
        path.strip_prefix(f.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The tree on the side: a click on a folder folds it, and its files leave
/// the diff.
fn file_tree(
    rows: &[TreeRow],
    folded: &[String],
    viewed: &[String],
    url: &str,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    for row in rows.iter().filter(|r| !hidden(&r.path, folded)) {
        ui.horizontal(|ui| {
            ui.add_space(row.depth as f32 * 12.0);
            if row.folder {
                let open = !folded.contains(&row.path);
                let text = format!("{} {}", if open { "▾" } else { "▸" }, row.name);
                let hint = if open {
                    "Fold: its files leave the diff"
                } else {
                    "Unfold"
                };
                if link(ui, label(&text).color(p().text_2))
                    .on_hover_text(hint)
                    .clicked()
                {
                    actions.push(Action::FoldFolder(url.to_owned(), row.path.clone()));
                }
            } else if viewed.contains(&row.path) {
                ui.add(
                    egui::Label::new(label(&format!("✓ {}", row.name)).color(p().text_3))
                        .truncate(),
                );
            } else {
                ui.add(egui::Label::new(label(&row.name).color(p().text)).truncate());
            }
        });
    }
}

/// A row of the diff: one line across (unified, or a hunk header), or the
/// old line beside the new one (side by side), by diff line number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    One(usize),
    Two(Option<usize>, Option<usize>),
}

/// Side by side: unchanged lines on both sides, a run of removed lines
/// beside the run of added lines that follows it, a hunk header across.
fn pair(lines: &[Line]) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut removed, mut added): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let flush = |rows: &mut Vec<Row>, removed: &mut Vec<usize>, added: &mut Vec<usize>| {
        for i in 0..removed.len().max(added.len()) {
            rows.push(Row::Two(removed.get(i).copied(), added.get(i).copied()));
        }
        removed.clear();
        added.clear();
    };
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        match line {
            Line::Removed(_) if added.is_empty() => removed.push(n),
            Line::Removed(_) => {
                flush(&mut rows, &mut removed, &mut added);
                removed.push(n);
            }
            Line::Added(_) => added.push(n),
            Line::Context(_) => {
                flush(&mut rows, &mut removed, &mut added);
                rows.push(Row::Two(Some(n), Some(n)));
            }
            Line::Hunk(_) => {
                flush(&mut rows, &mut removed, &mut added);
                rows.push(Row::One(n));
            }
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    rows
}

/// What every row of a file's diff needs to draw itself.
struct Cx<'a> {
    app: &'a App,
    url: &'a str,
    all: &'a [File],
    file: &'a File,
    step: Option<&'a Step>,
    suggestions: &'a [crate::anthropic::Think],
    threads: &'a [crate::github::Thread],
    compose: egui::Id,
}

/// One diff line's cell: its line number, sign and code, marked when the
/// open step covers it and dimmed when not. A click opens a comment on its
/// line. `width`: one side of a split, else the whole row.
/// One diff line; side by side it is `(width, old side)`, and an unchanged
/// line shows its old number on the left, its new one on the right.
fn cell(cx: &Cx, n: usize, width: Option<(f32, bool)>, ui: &mut Ui) {
    let Some(line) = cx.file.lines.get(n - 1) else {
        return;
    };
    let (sign, text, color, fill) = match line {
        Line::Hunk(t) => (" ", t, p().text_3, Color32::TRANSPARENT),
        Line::Added(t) => ("+", t, p().diff_text, p().diff_bg),
        Line::Removed(t) => ("−", t, p().text, wash(p().danger)),
        Line::Context(t) => (" ", t, p().text_2, Color32::TRANSPARENT),
    };
    // A step marks its lines and dims the rest of the file.
    let focus = cx.step.map(|s| s.spans.iter().any(|x| at(x, cx.file, n)));
    let (color, fill) = match focus {
        Some(false) => (p().text_3, fill.gamma_multiply(0.4)),
        _ => (color, fill),
    };
    let snippet = cx
        .suggestions
        .iter()
        .filter(|t| same_path(&cx.file.path, &t.at.path) && line_of(cx.file, t) == n)
        .map(|t| t.snippet.as_str())
        .find(|s| !s.is_empty() && text.contains(*s));
    // Where GitHub knows this row: a comment goes there.
    let place = crate::github::line_at(cx.file, n);
    let number = match (width, crate::github::numbers_at(cx.file, n)) {
        (Some((_, true)), Some((Some(old), _))) => old.to_string(),
        (Some((_, false)), Some((_, Some(new)))) => new.to_string(),
        _ => place.map_or(String::new(), |(_, line)| line.to_string()),
    };
    let row = clickable(ui, |ui| {
        Frame::new().fill(fill).show(ui, |ui| {
            match width {
                Some((w, _)) => ui.set_width(w),
                None => ui.set_min_width(ui.available_width()),
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if !matches!(line, Line::Hunk(_)) {
                    let gutter = RichText::new(format!("{number:>4}"))
                        .monospace()
                        .color(p().text_3);
                    ui.label(gutter);
                }
                let job = code(&format!("{sign} {text}"), color, snippet);
                ui.add(egui::Label::new(job).wrap());
            });
        });
    });
    if let Some((side, line)) = place
        && row
            .clone()
            .on_hover_text("Click to comment on this line")
            .clicked()
    {
        let at = (cx.file.path.clone(), side.to_owned(), line);
        ui.data_mut(|d| d.insert_temp(cx.compose, (at, String::new())));
    }
    if focus == Some(true) {
        let r = row.rect;
        ui.painter()
            .vline(r.left(), r.y_range(), Stroke::new(2.0, p().signal));
        if first_focus(ui, cx.step, cx.file, n) {
            row.scroll_to_me(Some(Align::Center));
        }
    }
}

/// What is said under a diff line: the walkthrough's suggestions for it,
/// review threads on it, the user's drafts, and the comment box when open.
/// Side by side: line `n`'s threads, drafts and comment box under its own
/// column, the old side's on the left, the new side's on the right.
fn beside(cx: &Cx, n: usize, half: f32, ui: &mut Ui, actions: &mut Vec<Action>) {
    if !annotated(cx, n, ui) {
        return;
    }
    let side = crate::github::line_at(cx.file, n).map(|(side, _)| side);
    ui.horizontal_top(|ui| {
        ui.add_space(column_offset(side, half));
        ui.vertical(|ui| {
            ui.set_width(half);
            annotate(cx, n, ui, actions);
        });
    });
}

/// The one line of a side-by-side row whose other side is empty: an added
/// or removed line on its own. A pair or a context line has none.
fn lone(left: Option<usize>, right: Option<usize>) -> Option<usize> {
    match (left, right) {
        (Some(n), None) | (None, Some(n)) => Some(n),
        _ => None,
    }
}

/// Where a side's column starts in a side-by-side row: the old side at
/// the left edge, the new one (and a suggestion with no side) after the gap.
fn column_offset(side: Option<&str>, half: f32) -> f32 {
    if side == Some("LEFT") {
        0.0
    } else {
        half + 8.0
    }
}

/// Anything `annotate` would draw under line `n`.
fn annotated(cx: &Cx, n: usize, ui: &Ui) -> bool {
    if cx
        .suggestions
        .iter()
        .any(|t| same_path(&cx.file.path, &t.at.path) && line_of(cx.file, t) == n)
    {
        return true;
    }
    let Some((side, line)) = crate::github::line_at(cx.file, n) else {
        return false;
    };
    let at =
        |path: &str, s: &str, l: usize| same_path(&cx.file.path, path) && s == side && l == line;
    cx.threads.iter().any(|t| at(&t.path, &t.side, t.line))
        || cx
            .app
            .drafts
            .get(cx.url)
            .is_some_and(|ds| ds.iter().any(|d| at(&d.path, &d.side, d.line)))
        || ui
            .data(|d| d.get_temp::<Open>(cx.compose))
            .is_some_and(|((path, s, l), _)| at(&path, &s, l))
}

fn annotate(cx: &Cx, n: usize, ui: &mut Ui, actions: &mut Vec<Action>) {
    for t in cx
        .suggestions
        .iter()
        .filter(|t| same_path(&cx.file.path, &t.at.path) && line_of(cx.file, t) == n)
    {
        inline_think(cx.app, cx.url, cx.all, t, ui, actions);
    }
    let Some((side, line)) = crate::github::line_at(cx.file, n) else {
        return;
    };
    for thread in cx
        .threads
        .iter()
        .filter(|t| same_path(&cx.file.path, &t.path) && t.side == side && t.line == line)
    {
        thread_box(thread, ui);
    }
    drafts_at(cx.app, cx.url, &cx.file.path, side, line, ui, actions);
    composer(
        cx.app,
        cx.url,
        cx.compose,
        &cx.file.path,
        side,
        line,
        ui,
        actions,
    );
}

/// Claude's walkthrough of the diff, asked for with a click: a summary,
/// then one step at a time. Returns the files the current step covers,
/// and the step, whose lines the diff marks.
fn guide(
    app: &App,
    card: &Card,
    url: &str,
    files: &[File],
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) -> (Vec<File>, Option<Step>) {
    let all = files.to_vec();
    let frame = Frame::new()
        .stroke(Stroke::new(1.0, p().line_2))
        .inner_margin(Margin::symmetric(12, 10));
    if app.guide_pending.contains(url) {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Claude is reading the diff…").color(p().text_2));
            });
        });
        ui.add_space(8.0);
        return (all, None);
    }
    let guide = match app.guides.get(url) {
        Some(Ok(guide)) => guide,
        failed => {
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(Err(error)) = failed {
                    ui.add(egui::Label::new(RichText::new(error).small().color(p().danger)).wrap());
                }
                if app.has_anthropic_key() {
                    if primary(ui, "✳ Walk me through")
                        .on_hover_text("Claude reads the diff once and shows it step by step")
                        .clicked()
                    {
                        actions.push(Action::Guide);
                    }
                } else {
                    ui.label(
                        RichText::new(
                            "Add an Anthropic key in Settings, connections, for a walkthrough.",
                        )
                        .small()
                        .color(p().text_3),
                    );
                }
                // Asking needs no walkthrough first.
                let rect = ask(app, url, ui, actions);
                super::tutorial::mark(app, ui, "ask-pr", rect);
            });
            ui.add_space(8.0);
            return (all, None);
        }
    };
    let steps = guide.steps.len();
    // A step picked on another PR is not this one's.
    let current = app
        .guide_step
        .as_ref()
        .filter(|(of, i)| of == url && *i < steps)
        .map(|(_, i)| *i);
    let go = |i: usize| Action::GuideStep(Some((url.to_owned(), i)));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        if !guide.summary.trim().is_empty() {
            let summary = RichText::new(format!("✳ {}", guide.summary)).color(p().text);
            ui.add(egui::Label::new(summary).wrap());
        }
        // Inside a step the step says it; the why is for the overview.
        if current.is_none() && !guide.why.is_empty() {
            ui.add(egui::Label::new(RichText::new(&guide.why).color(p().text_2)).wrap());
        }
        // What it could break, then whether the ask was right: what a
        // reviewer reads instead of every line.
        if current.is_none()
            && let Some((level, what)) = &guide.risk
        {
            ui.add_space(4.0);
            let color = match level.as_str() {
                "low" => p().success,
                "medium" => p().signal,
                _ => p().danger,
            };
            let rect = ui
                .horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("risk {level}"))
                            .monospace()
                            .color(color),
                    );
                    ui.add(egui::Label::new(RichText::new(what).color(p().text_2)).wrap());
                })
                .response
                .rect;
            mark(app, ui, "guide-risk", rect);
        }
        if current.is_none()
            && let Some(challenge) = &guide.challenge
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("the ask").monospace().color(p().signal));
                ui.add(egui::Label::new(RichText::new(challenge).color(p().text_2)).wrap());
            });
        }
        if !guide.resolved.is_empty() {
            ui.add_space(6.0);
            ui.label(label("since your review"));
            for r in &guide.resolved {
                let (mark, color) = match r.verdict.as_str() {
                    "yes" => ("✓", p().success),
                    "partly" => ("~", p().signal),
                    _ => ("✗", p().danger),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(mark).monospace().color(color));
                    ui.label(RichText::new(&r.comment).color(p().text));
                    ui.label(RichText::new(&r.how).color(p().text_2));
                });
            }
        }
        ui.add_space(6.0);
        match current {
            Some(i) => {
                let step = &guide.steps[i];
                ui.label(
                    RichText::new(format!("Step {} of {steps} · {}", i + 1, step.title))
                        .family(medium()),
                );
                ui.add(egui::Label::new(RichText::new(&step.check).color(p().text)).wrap());
                for t in &step.think {
                    think(app, url, files, t, ui, actions);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if i > 0 && ui.button("← Prev").clicked() {
                        actions.push(go(i - 1));
                    }
                    if i + 1 < steps && primary(ui, "Next →").clicked() {
                        actions.push(go(i + 1));
                    }
                    if ui.button("All files").clicked() {
                        actions.push(Action::GuideStep(None));
                    }
                });
            }
            None => {
                ui.horizontal_wrapped(|ui| {
                    for (i, step) in guide.steps.iter().enumerate() {
                        if ui
                            .small_button(format!("{} {}", i + 1, step.title))
                            .clicked()
                        {
                            actions.push(go(i));
                        }
                    }
                });
            }
        }
        let rect = ask(app, url, ui, actions);
        super::tutorial::mark(app, ui, "ask-pr", rect);
        wrap_up(app, card, url, ui, actions);
    });
    ui.add_space(8.0);
    let Some(i) = current else {
        return (all, None);
    };
    let step = &guide.steps[i];
    let picked: Vec<File> = files
        .iter()
        .filter(|f| step.files.iter().any(|w| same_path(&f.path, w)))
        .cloned()
        .collect();
    if picked.is_empty() {
        (all, None)
    } else {
        (picked, Some(step.clone()))
    }
}

/// What people said on the PR outside its lines: comments and review
/// bodies, oldest first, folded when there are many.
fn conversation(app: &App, url: &str, ui: &mut Ui) {
    let Some(Ok(talk)) = app.talk.get(url) else {
        return;
    };
    if talk.conversation.is_empty() {
        return;
    }
    let n = talk.conversation.len();
    egui::CollapsingHeader::new(label(&format!("// conversation  {n}")))
        .id_salt(("conversation", url))
        .default_open(true)
        .show(ui, |ui| {
            // One line each, the first words; a click opens the whole.
            for (i, said) in talk.conversation.iter().enumerate() {
                let first = first_words(&said.body);
                let head = format!("{} · {} · {first}", said.author, story::ago(&said.at));
                let id = ui.make_persistent_id(("said", url, i));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    ui.add(egui::Label::new(label(&head).color(p().text_2)).truncate());
                })
                .body(|ui| markdown(ui, &said.body));
            }
        });
    ui.add_space(6.0);
}

/// A comment's first words for its one line: markup and a bot's hidden
/// `<!-- … -->` markers left out.
fn first_words(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("<!--"))
        .map(|l| l.trim_start_matches(['#', '>', '-', '*', ' ']))
        .find(|l| !l.is_empty())
        .map(story::plain)
        .unwrap_or_default()
}

/// A review thread under its line, GitHub style: each comment's author
/// and age over its body (markdown, a bot's hidden markers left out). A
/// resolved one is a single dimmed line that opens on a click.
fn thread_box(thread: &crate::github::Thread, ui: &mut Ui) {
    let id = ui.make_persistent_id(("thread", &thread.path, &thread.side, thread.line));
    let row = Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 8,
            top: 6,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let comments = |ui: &mut Ui| {
                for (i, said) in thread.comments.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(6.0);
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&said.author).family(medium()).color(p().text));
                        ui.label(label(&story::ago(&said.at)));
                    });
                    markdown(ui, &said.body);
                }
            };
            if !thread.resolved {
                comments(ui);
                return;
            }
            let first = thread.comments.first();
            let head = format!(
                "✓ resolved · {}: {}",
                first.map_or("", |s| s.author.as_str()),
                first.map(|s| first_words(&s.body)).unwrap_or_default()
            );
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    ui.add(egui::Label::new(label(&head)).truncate());
                })
                .body(comments);
        });
    let r = row.response.rect;
    let color = if thread.resolved {
        p().line_2
    } else {
        p().text_2
    };
    ui.painter()
        .vline(r.left(), r.y_range(), Stroke::new(2.0, color));
}

/// The user's drafted comments on this line, each with × to take it back.
fn drafts_at(
    app: &App,
    url: &str,
    path: &str,
    side: &str,
    line: usize,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let Some(drafts) = app.drafts.get(url) else {
        return;
    };
    for draft in drafts
        .iter()
        .filter(|d| d.path == path && d.side == side && d.line == line)
    {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(14.0);
            ui.label(label("your draft").color(p().signal));
            ui.add(egui::Label::new(RichText::new(&draft.body).color(p().text)).wrap());
            if ui.small_button("×").on_hover_text("Take it back").clicked() {
                actions.push(Action::Draft {
                    url: url.to_owned(),
                    draft: draft.clone(),
                });
            }
        });
    }
}

/// The comment box on the line the user clicked: Add to review drafts it
/// (posted with the review), Esc or Cancel drops it.
/// The open comment box: (path, side, line) and the text typed so far.
type Open = ((String, String, usize), String);

#[allow(clippy::too_many_arguments)]
fn composer(
    app: &App,
    url: &str,
    id: egui::Id,
    path: &str,
    side: &'static str,
    line: usize,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let Some(((at_path, at_side, at_line), mut text)) = ui.data(|d| d.get_temp::<Open>(id)) else {
        return;
    };
    if at_path != path || at_side != side || at_line != line {
        return;
    }
    let mut close = false;
    Frame::new()
        .inner_margin(Margin::symmetric(14, 6))
        .show(ui, |ui| {
            let field = ui.add(
                egui::TextEdit::multiline(&mut text)
                    .hint_text(format!("comment on line {line}"))
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            if !field.has_focus() && text.is_empty() {
                field.request_focus();
            }
            ui.horizontal(|ui| {
                let add = primary(ui, "Add to review").on_hover_text(
                    "Drafts it on this line; Comment below posts your drafts as one review",
                );
                if add.clicked() && !text.trim().is_empty() {
                    actions.push(Action::Draft {
                        url: url.to_owned(),
                        draft: Draft {
                            path: path.to_owned(),
                            line,
                            side: side.into(),
                            body: text.trim().to_owned(),
                        },
                    });
                    close = true;
                }
                // Or post it right away, with any other drafts, as one review.
                let posting = app.reviewing.contains(url);
                match post_head(app, url) {
                    Ok(head) if !posting => {
                        let now = ui.button("Comment").on_hover_text(
                            "Posts it now on GitHub, with your other drafts, as one review",
                        );
                        if now.clicked() && !text.trim().is_empty() {
                            actions.push(Action::Draft {
                                url: url.to_owned(),
                                draft: Draft {
                                    path: path.to_owned(),
                                    line,
                                    side: side.into(),
                                    body: text.trim().to_owned(),
                                },
                            });
                            actions.push(Action::Review {
                                url: url.to_owned(),
                                head,
                                fix: None,
                                changes: false,
                            });
                            close = true;
                        }
                    }
                    result => {
                        let why = result.err().unwrap_or("posting your review…");
                        ui.add_enabled(false, egui::Button::new("Comment"))
                            .on_disabled_hover_text(why);
                    }
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
    ui.data_mut(|d| {
        if close {
            d.remove::<Open>(id);
        } else {
            d.insert_temp(id, ((at_path, at_side, at_line), text));
        }
    });
}

/// A suggestion of the walkthrough under its line: what to think about, the
/// comment Claude would post, and its options, one click to draft each.
fn inline_think(
    app: &App,
    url: &str,
    files: &[File],
    t: &crate::anthropic::Think,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let row = Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 8,
            top: 4,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(label("claude").color(p().signal));
                ui.add(egui::Label::new(RichText::new(&t.note).color(p().text)).wrap());
            });
            if let Some((path, side, line)) = target(files, t) {
                offers(app, url, (path, side, line), t, ui, actions);
            }
        });
    let r = row.response.rect;
    ui.painter()
        .vline(r.left(), r.y_range(), Stroke::new(2.0, p().signal));
}

/// Your own questions about the PR and Claude's short answers, then the
/// field to ask the next one (Enter). Nothing here is posted.
fn ask(app: &App, url: &str, ui: &mut Ui, actions: &mut Vec<Action>) -> egui::Rect {
    ui.scope(|ui| {
        ui.add_space(6.0);
        for (question, answer) in app.pr_questions.get(url).into_iter().flatten() {
            ui.add(egui::Label::new(RichText::new(question).family(medium())).wrap());
            match answer {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Claude is reading the diff…").color(p().text_2));
                    });
                }
                Some(Ok(answer)) => {
                    ui.add(egui::Label::new(RichText::new(answer).color(p().text_2)).wrap());
                }
                Some(Err(error)) => {
                    ui.add(egui::Label::new(RichText::new(error).small().color(p().danger)).wrap());
                }
            }
            ui.add_space(4.0);
        }
        if !app.has_anthropic_key() {
            return;
        }
        let answered = app
            .pr_questions
            .get(url)
            .is_some_and(|a| a.iter().any(|(_, a)| matches!(a, Some(Ok(_)))));
        if answered {
            let rect = approval_note(app, url, ui, actions);
            super::tutorial::mark(app, ui, "approval-note", rect);
        }
        let id = egui::Id::new(("ask-pr", url));
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
        let field = ui.add(
            egui::TextEdit::singleline(&mut text)
                .hint_text(
                    "ask Claude about this PR (Enter): is this specific, or for all of them?",
                )
                .desired_width(f32::INFINITY),
        );
        if field.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !text.trim().is_empty()
        {
            actions.push(Action::AskPr(std::mem::take(&mut text)));
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    })
    .response
    .rect
}

/// After questions: one short line in the user's voice for their approval
/// ("Makes sense, I also checked…"), editable, × drops it; Approve posts it
/// as the review's comment. Without questions an approval has none.
fn approval_note(app: &App, url: &str, ui: &mut Ui, actions: &mut Vec<Action>) -> egui::Rect {
    ui.horizontal(|ui| match app.approve_notes.get(url) {
        None => {
            let button = hover_hand(&ui.button("✳ Note for the approval")).on_hover_text(
                "One short line from what you asked, posted as the comment when you approve",
            );
            if button.clicked() {
                actions.push(Action::NoteForApproval(url.to_owned()));
            }
        }
        Some(None) => {
            ui.spinner();
            ui.label(RichText::new("writing the note…").color(p().text_2));
        }
        Some(Some(Ok(note))) => {
            ui.label(label("approval note"));
            let mut text = note.clone();
            let field = ui.add(
                egui::TextEdit::singleline(&mut text).desired_width(ui.available_width() - 40.0),
            );
            if field.changed() {
                actions.push(Action::SetApprovalNote {
                    url: url.to_owned(),
                    note: Some(text),
                });
            }
            if ui
                .small_button("×")
                .on_hover_text("Approve without a note")
                .clicked()
            {
                actions.push(Action::SetApprovalNote {
                    url: url.to_owned(),
                    note: None,
                });
            }
        }
        Some(Some(Err(error))) => {
            ui.label(RichText::new(error).small().color(p().danger));
            if ui.small_button("try again").clicked() {
                actions.push(Action::NoteForApproval(url.to_owned()));
            }
        }
    })
    .response
    .rect
}

/// A thing to think about, one line; open it for the comment Claude would
/// post and its options as suggestions. A click drafts the comment on its
/// line; another click takes it back.
fn think(
    app: &App,
    url: &str,
    files: &[File],
    t: &crate::anthropic::Think,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let text = RichText::new(format!("? {}  {}", t.note, where_(&t.at))).color(p().text_2);
    let Some((path, side, line)) = target(files, t) else {
        ui.add(egui::Label::new(text).wrap());
        return;
    };
    // One line, cut with an ellipsis: a header cannot wrap, and a long one
    // would widen the whole walkthrough. Open, it says it in full.
    let id = ui.make_persistent_id(("think", url, &t.note));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            ui.add(egui::Label::new(text).truncate());
        })
        .body(|ui| {
            let full = RichText::new(format!("? {}", t.note)).color(p().text_2);
            ui.add(egui::Label::new(full).wrap());
            offers(app, url, (path, side, line), t, ui, actions);
        });
}

/// The comment Claude would post on the line, and its options as
/// suggestions: a click drafts one there, another takes it back.
fn offers(
    app: &App,
    url: &str,
    (path, side, line): (String, &'static str, usize),
    t: &crate::anthropic::Think,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let drafted = app.drafts.get(url);
    let mut offer = |ui: &mut Ui, label: String, body: String| {
        let draft = Draft {
            path: path.clone(),
            line,
            side: side.into(),
            body,
        };
        let queued = drafted.is_some_and(|d| d.contains(&draft));
        let label = if queued {
            format!("✓ {label}")
        } else {
            label
        };
        let hover = if queued {
            "Drafted; click to take it back".to_owned()
        } else {
            format!("Adds to your review on line {line}:\n{}", draft.body)
        };
        // Framed, so it reads as a button and not as more text.
        if ui
            .add(egui::Button::new(label).selected(queued))
            .on_hover_text(hover)
            .clicked()
        {
            actions.push(Action::Draft {
                url: url.to_owned(),
                draft,
            });
        }
    };
    ui.add_space(2.0);
    let quote = RichText::new(format!("“{}”", t.comment)).color(p().text_2);
    ui.add(egui::Label::new(quote).wrap());
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        offer(ui, "Ask the author".to_owned(), t.comment.clone());
        if !t.options.is_empty() {
            ui.label(label("or suggest"));
        }
        for option in &t.options {
            let body = format!("{}\n\nOne option: {option}", t.comment);
            offer(ui, option.clone(), body);
        }
    });
}

/// The review so far: each drafted comment (× takes it back), then
/// Comment posts them as one review, Comment and fix also has the
/// ticket's session fix them. Bound to the head the gate last saw.
fn wrap_up(app: &App, card: &Card, url: &str, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(drafts) = app.drafts.get(url).filter(|d| !d.is_empty()) else {
        return;
    };
    ui.add_space(8.0);
    let n = drafts.len();
    ui.label(label(&format!(
        "your review · {n} comment{}",
        if n == 1 { "" } else { "s" }
    )));
    for d in drafts {
        ui.horizontal(|ui| {
            if ui.small_button("×").on_hover_text("Take it back").clicked() {
                actions.push(Action::Draft {
                    url: url.to_owned(),
                    draft: d.clone(),
                });
            }
            let file = d.path.rsplit('/').next().unwrap_or(&d.path);
            ui.label(label(&format!("{file}:{}", d.line)));
            let first = d.body.lines().next().unwrap_or_default();
            ui.add(egui::Label::new(RichText::new(first).color(p().text_2)).truncate())
                .on_hover_text(&d.body);
        });
    }
    ui.add_space(4.0);
    if app.reviewing.contains(url) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Posting your review…").color(p().text_2));
        });
        return;
    }
    let head = match post_head(app, url) {
        Ok(head) => head,
        Err(why) => {
            ui.label(label(why));
            return;
        }
    };
    let short = &head[..7.min(head.len())];
    let review = |fix: Option<String>, changes: bool| Action::Review {
        url: url.to_owned(),
        head: head.clone(),
        fix,
        changes,
    };
    ui.horizontal(|ui| {
        let comment = primary(ui, "Comment").on_hover_text(format!(
            "Posts these as one review at {short}, as you, on GitHub."
        ));
        mark(app, ui, "review-comments", comment.rect);
        if comment.clicked() {
            actions.push(review(None, false));
        }
        if ui
            .button("Request changes")
            .on_hover_text(format!(
                "Posts these as one review at {short} that requests changes, as you, on GitHub."
            ))
            .clicked()
        {
            actions.push(review(None, true));
        }
        let skill = crate::profile::get().skill("fixComments");
        if ui
            .button("✳ Comment and fix")
            .on_hover_text(format!(
                "Posts the review, then runs /{skill} on this PR in the ticket's Claude \
                 session: it fixes each comment, pushes and replies. Never merges."
            ))
            .clicked()
        {
            actions.push(review(Some(card.issue.id.clone()), false));
        }
        if ui.button("Discard").clicked() {
            actions.push(Action::DiscardDrafts(url.to_owned()));
        }
    });
}

/// Which review the composer under the header writes, per PR: Some(true)
/// requests changes, Some(false) comments. egui memory, not App.
fn composing(ui: &Ui, url: &str) -> Option<bool> {
    ui.data(|d| d.get_temp::<bool>(egui::Id::new(("review-composer", url))))
}

/// Review ▾ beside Approve: Comment, or Request changes (not on the user's
/// own PR: GitHub refuses it). Opens the composer below the header.
fn review_menu(app: &App, pr: &story::Pr, ui: &mut Ui) {
    let Some(Ok(gate)) = app.gates.get(&pr.url) else {
        return;
    };
    if !pr.open || composing(ui, &pr.url).is_some() {
        return;
    }
    let mut pick = None;
    let review = egui_phosphor::regular::CHAT_TEXT;
    let rect = action_menu(ui, review, "Review", "Comment, or request changes", |ui| {
        if ui
            .button("Comment…")
            .on_hover_text("A review comment, without approving")
            .clicked()
        {
            pick = Some(false);
            ui.close();
        }
        if review_choices(gate.own).1
            && ui
                .button("Request changes…")
                .on_hover_text("Asks the author to change something before it merges")
                .clicked()
        {
            pick = Some(true);
            ui.close();
        }
    })
    .rect;
    mark(app, ui, "review-menu", rect);
    if let Some(changes) = pick {
        ui.data_mut(|d| {
            d.insert_temp(egui::Id::new(("review-composer", pr.url.as_str())), changes)
        });
    }
}

/// (Comment, Request changes) offered on a PR: GitHub refuses requesting
/// changes on your own PR.
fn review_choices(own: bool) -> (bool, bool) {
    (true, !own)
}

/// The composer the Review menu opened: what you think, then Send posts it
/// with any drafted line comments as one review at the head the diff shows.
fn review_composer(app: &App, pr: &story::Pr, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(changes) = composing(ui, &pr.url) else {
        return;
    };
    let key = egui::Id::new(("review-composer", pr.url.as_str()));
    let text_id = egui::Id::new(("review-text", pr.url.as_str()));
    let mut text = ui
        .data(|d| d.get_temp::<String>(text_id))
        .unwrap_or_default();
    ui.add_space(6.0);
    let what = if changes {
        "request changes"
    } else {
        "comment"
    };
    ui.label(label(&format!("// {what}")));
    if app.reviewing.contains(&pr.url) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Posting your review…").color(p().text_2));
        });
        return;
    }
    ui.add(
        egui::TextEdit::multiline(&mut text)
            .hint_text(if changes {
                "what should change, and why"
            } else {
                "what you think of it"
            })
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    let drafts = app.drafts.get(&pr.url).map_or(0, Vec::len);
    let mut close = false;
    ui.horizontal(|ui| {
        let head = post_head(app, &pr.url);
        let ready = !text.trim().is_empty() || (!changes && drafts > 0);
        match head {
            Ok(head) => {
                let short = &head[..7.min(head.len())];
                let also = match drafts {
                    0 => String::new(),
                    n => format!(" with your {n} line comment(s)"),
                };
                let send = ui
                    .add_enabled(ready, egui::Button::new("Send"))
                    .on_hover_text(format!("Posts it{also} at {short}, as you, on GitHub."));
                if send.clicked() {
                    actions.push(Action::ReviewSummary {
                        url: pr.url.clone(),
                        head,
                        body: text.clone(),
                        changes,
                    });
                    close = true;
                }
            }
            Err(why) => {
                ui.label(label(why));
            }
        }
        if ui.button("Cancel").clicked() {
            close = true;
        }
    });
    ui.data_mut(|d| {
        if close {
            d.remove::<bool>(key);
            d.remove::<String>(text_id);
        } else {
            d.insert_temp(text_id, text);
        }
    });
}

/// The head a review posts at, bound to the commit the diff shows (new
/// commits make GitHub refuse), or why it cannot post now.
fn post_head(app: &App, url: &str) -> Result<String, &'static str> {
    match (app.gates.get(url), app.diff_heads.get(url)) {
        (Some(Ok(gate)), _) if gate.state != "OPEN" => Err("the PR is not open"),
        (Some(Ok(_)), Some(head)) if app.moved(url).is_none() => Ok(head.clone()),
        (Some(Ok(_)), Some(_)) => Err("new commits: review again to post"),
        _ => Err("checking the PR…"),
    }
}

/// Where GitHub takes a comment on these lines: the first one of the span
/// the PR's diff has (a hunk header has none).
fn target(files: &[File], t: &Think) -> Option<(String, &'static str, usize)> {
    let file = files.iter().find(|f| same_path(&f.path, &t.at.path))?;
    let from = line_of(file, t);
    (from..=t.at.to.max(from).min(file.lines.len()))
        .find_map(|n| crate::github::line_at(file, n))
        .map(|(side, line)| (file.path.clone(), side, line))
}

/// The diff line a note is about: Claude's number, or the line within
/// three of it that holds the snippet, since Claude miscounts by a few.
fn line_of(file: &File, t: &Think) -> usize {
    let text = |n: usize| match file.lines.get(n.wrapping_sub(1)) {
        Some(Line::Added(s) | Line::Removed(s) | Line::Context(s)) => s.as_str(),
        _ => "",
    };
    let mut near = [0, 1, 2, 3]
        .into_iter()
        .flat_map(|d| [t.at.from + d, t.at.from.saturating_sub(d)]);
    let snippet = t.snippet.trim();
    near.find(|&n| !snippet.is_empty() && text(n).contains(snippet))
        .unwrap_or(t.at.from)
}

/// Claude names paths as the diff has them; tolerate a leading folder
/// either way.
fn same_path(path: &str, named: &str) -> bool {
    path == named || path.ends_with(named) || named.ends_with(path)
}

fn at(span: &Span, file: &File, n: usize) -> bool {
    (span.from..=span.to).contains(&n) && same_path(&file.path, &span.path)
}

fn where_(span: &Span) -> String {
    let file = span.path.rsplit('/').next().unwrap_or(&span.path);
    match span.to {
        usize::MAX => file.to_owned(),
        to if to == span.from => format!("{file}:{to}"),
        to => format!("{file}:{}-{to}", span.from),
    }
}

/// Whether line `n` of `file` is the step's first marked line, the one
/// the diff scrolls to once when the step opens.
fn first_focus(ui: &Ui, step: Option<&Step>, file: &File, n: usize) -> bool {
    let Some(first) = step.and_then(|s| s.spans.first()) else {
        return false;
    };
    let here = at(first, file, n) && (n == first.from || first.to == usize::MAX && n == 1);
    let id = egui::Id::new("guide-scrolled");
    let key = (step.map(|s| s.title.clone()), file.path.clone(), n);
    let key = egui::Id::new(key);
    if !here || ui.ctx().data(|d| d.get_temp::<egui::Id>(id)) == Some(key) {
        return false;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, key));
    true
}

/// A diff line in mono, with the characters a note is about washed.
fn code(text: &str, color: Color32, snippet: Option<&str>) -> egui::text::LayoutJob {
    let font = egui::FontId::monospace(12.0);
    let plain = egui::TextFormat::simple(font.clone(), color);
    let mut job = egui::text::LayoutJob::default();
    match snippet.and_then(|s| text.find(s).map(|at| (at, at + s.len()))) {
        Some((from, to)) => {
            job.append(&text[..from], 0.0, plain.clone());
            let mark = egui::TextFormat {
                background: wash(p().signal).gamma_multiply(2.5),
                color: p().text,
                underline: Stroke::new(1.0, p().signal),
                ..plain.clone()
            };
            job.append(&text[from..to], 0.0, mark);
            job.append(&text[to..], 0.0, plain);
        }
        None => job.append(text, 0.0, plain),
    }
    job
}

#[cfg(test)]
mod tests {
    #[test]
    fn request_changes_is_not_offered_on_your_own_pr() {
        assert_eq!(super::review_choices(true), (true, false));
        assert_eq!(super::review_choices(false), (true, true));
    }

    use super::*;

    #[test]
    fn a_new_or_deleted_file_shows_in_one_column_even_side_by_side() {
        let file = |lines: Vec<Line>| File {
            path: "a.py".into(),
            added: 0,
            removed: 0,
            lines,
        };
        assert!(one_sided(&file(vec![
            Line::Hunk("@@".into()),
            Line::Added("x".into())
        ])));
        assert!(one_sided(&file(vec![Line::Removed("x".into())])));
        assert!(!one_sided(&file(vec![
            Line::Removed("x".into()),
            Line::Added("y".into())
        ])));
    }

    #[test]
    fn a_changed_file_is_no_longer_viewed() {
        let file = |line: &str| File {
            path: "a.rs".into(),
            added: 1,
            removed: 0,
            lines: vec![Line::Added(line.into())],
        };
        let url = "https://github.com/o/r/pull/1";
        assert_eq!(viewed_key(url, &file("x")), viewed_key(url, &file("x")));
        assert_ne!(viewed_key(url, &file("x")), viewed_key(url, &file("y")));
    }

    #[test]
    fn a_note_on_a_lone_line_goes_beside_it_in_the_empty_column() {
        assert_eq!(lone(None, Some(7)), Some(7));
        assert_eq!(lone(Some(3), None), Some(3));
        assert_eq!(lone(Some(3), Some(4)), None);
        assert_eq!(lone(Some(3), Some(3)), None);
    }

    #[test]
    fn a_comment_sits_under_its_own_column() {
        assert_eq!(column_offset(Some("LEFT"), 100.0), 0.0);
        assert_eq!(column_offset(Some("RIGHT"), 100.0), 108.0);
    }

    #[test]
    fn a_folded_folder_hides_what_is_under_it_only() {
        let rows = tree(&["gen/a.py", "gen/sub/b.py", "src/c.rs", "top.md"]);
        let names: Vec<(usize, &str, bool)> = rows
            .iter()
            .map(|r| (r.depth, r.name.as_str(), r.folder))
            .collect();
        assert_eq!(
            names,
            [
                (0, "gen", true),
                (1, "a.py", false),
                (1, "sub", true),
                (2, "b.py", false),
                (0, "src", true),
                (1, "c.rs", false),
                (0, "top.md", false),
            ]
        );
        let folded = vec!["gen".to_owned()];
        assert!(hidden("gen/sub/b.py", &folded));
        assert!(!hidden("gen", &folded), "the folder itself stays");
        assert!(!hidden("generated/x", &folded));
    }

    #[test]
    fn side_by_side_pairs_removed_with_added_and_keeps_context_on_both() {
        let lines = [
            Line::Hunk("@@ -1,4 +1,5 @@".into()),
            Line::Context("a".into()),
            Line::Removed("old 1".into()),
            Line::Removed("old 2".into()),
            Line::Added("new 1".into()),
            Line::Context("b".into()),
            Line::Added("only new".into()),
        ];
        assert_eq!(
            pair(&lines),
            [
                Row::One(1),
                Row::Two(Some(2), Some(2)),
                Row::Two(Some(3), Some(5)),
                Row::Two(Some(4), None),
                Row::Two(Some(6), Some(6)),
                Row::Two(None, Some(7)),
            ]
        );
    }

    #[test]
    fn a_comments_line_starts_with_what_it_says() {
        assert_eq!(
            first_words("<!-- linear-linkback -->\n\n**ABC-172** fixed"),
            "ABC-172 fixed"
        );
        assert_eq!(
            first_words("### Cypress Component Tests\nall green"),
            "Cypress Component Tests"
        );
        assert_eq!(first_words("  \n"), "");
    }

    #[test]
    fn a_span_finds_its_lines_under_either_path_spelling() {
        let file = File {
            path: "pkg/utils/parser.py".into(),
            added: 0,
            removed: 0,
            lines: Vec::new(),
        };
        let span = Span {
            path: "utils/parser.py".into(),
            from: 4,
            to: 6,
        };
        assert!(at(&span, &file, 5));
        assert!(!at(&span, &file, 7));
        assert_eq!(where_(&span), "parser.py:4-6");
    }

    #[test]
    fn a_note_snaps_to_the_nearby_line_with_its_snippet() {
        let file = File {
            path: "t.py".into(),
            added: 3,
            removed: 0,
            lines: vec![
                Line::Hunk("@@ -1,0 +1,3 @@".into()),
                Line::Added("@patch(\"x\")".into()),
                Line::Added("def f(m):".into()),
                Line::Added("    m.return_value = 1".into()),
            ],
        };
        let think = |from: usize, snippet: &str| Think {
            at: Span {
                path: "t.py".into(),
                from,
                to: from,
            },
            snippet: snippet.into(),
            note: String::new(),
            comment: String::new(),
            options: Vec::new(),
        };
        assert_eq!(line_of(&file, &think(2, "m.return_value")), 4);
        assert_eq!(line_of(&file, &think(2, "not there")), 2);
    }
}
