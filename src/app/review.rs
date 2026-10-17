//! Hooks for reviewing the interface without a person at the screen. All
//! of them are off unless their `ORBIT_*` variable is set:
//!
//! - `ORBIT_VIEW=foryou|stories|intake|sessions|customers|work|ktlo|plan`
//!   starts with the sheet open on that view (`settings`: Settings,
//!   `panes`: the sheet closed); `ORBIT_TAB=claude` opens the first ticket's
//!   Claude session once the board has loaded; `ORBIT_TAB=testing`
//!   opens the first ticket in Testing on its Testing tab; `diff` the
//!   first ticket with a PR on its Diff tab.
//! - `ORBIT_TUTORIAL=<step>` opens the tutorial at that step.
//! - `ORBIT_SPLIT=ticket|shell` splits the right column once.
//! - `ORBIT_PALETTE=<query>` opens the command palette with a query.
//! - `ORBIT_CHAT=1` opens the clone panel (Cmd+J) once the board loaded.
//! - `ORBIT_SCROLL=<px>` starts the ticket page scrolled that far down.
//! - `ORBIT_LINK=<url>` opens a ticket or PR link once loaded, as a click on it.
//! - `ORBIT_FIND=<query>` opens find on the screen with a query.
//! - `ORBIT_RELEASE=fix` runs Fix it on the first red release.
//! - `ORBIT_DIFF_ALL=1` turns KTLO's Diff all on once its gates are in.
//! - `ORBIT_ALERT=open|debug|investigate` opens the first alert's
//!   details on Alerts, or starts its Debug or Investigate session.
//! - `ORBIT_DUMP=out.tsv` writes For you, Notifications and KTLO as text once loaded.
//! - `ORBIT_SIMULATE=steps.json` (with `ORBIT_READ_ONLY=1`) plays a user's
//!   steps (`simulate`: show, select, tab, palette, key, rename, move,
//!   window, search, do, shot, wait) and saves a frame per `shot`; scripts/simulate.sh wraps this.
//! - `ORBIT_SIZE=1100x700` starts the window at that size (main.rs).
//! - `ORBIT_SCREENSHOT=out.ppm` saves the frame drawn
//!   `ORBIT_SCREENSHOT_DELAY` seconds after start, then quits.
//!   `scripts/shot.sh` wraps this.

use std::sync::atomic::{AtomicBool, Ordering};

use super::{Action, App, Tab, View};

/// The view to start in, and whether the sheet starts open on it.
pub(super) fn start_view() -> (View, bool) {
    let named = std::env::var("ORBIT_VIEW");
    let view = match named.as_deref() {
        Ok("intake") => View::Intake,
        Ok("alerts") => View::Alerts,
        Ok("sessions") => View::Sessions,
        Ok("customers") => View::Customers,
        Ok("settings") => View::Settings,
        Ok("ktlo") => View::Ktlo,
        Ok("notifications") => View::Inbox,
        _ => View::ForYou,
    };
    let sheet = named.is_ok() && view != View::Settings && named.as_deref() != Ok("panes");
    (view, sheet)
}

pub(super) fn tick(app: &mut App, ctx: &egui::Context) {
    if std::env::var("ORBIT_TAB").as_deref() == Ok("claude")
        && app.tab != Tab::Claude
        && let Some(id) = app.selected.clone().filter(|_| app.loaded_at.is_some())
    {
        app.apply(Action::Claude(id), ctx);
    }
    // `ORBIT_TAB=testing` opens the first ticket in Testing on its
    // Testing tab, once the board has loaded.
    static TESTING: AtomicBool = AtomicBool::new(false);
    if std::env::var("ORBIT_TAB").as_deref() == Ok("testing")
        && app.loaded_at.is_some()
        && let Some(id) = app
            .visible()
            .iter()
            .find(|c| c.stage == crate::story::Stage::Testing)
            .map(|c| c.issue.id.clone())
        && !TESTING.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::Select(id), ctx);
        app.apply(Action::Tab(Tab::Testing), ctx);
    }
    // `ORBIT_SPLIT=ticket|shell` splits the right column once.
    static SPLIT: AtomicBool = AtomicBool::new(false);
    if let Ok(what) = std::env::var("ORBIT_SPLIT")
        && app.loaded_at.is_some()
        && !SPLIT.swap(true, Ordering::Relaxed)
    {
        app.apply(
            if what == "shell" {
                Action::NewShell
            } else {
                Action::Split { across: true }
            },
            ctx,
        );
    }
    // `ORBIT_TAB=diff` opens the first ticket with a PR on its Diff
    // tab, once the board has loaded.
    static DIFF: AtomicBool = AtomicBool::new(false);
    if std::env::var("ORBIT_TAB").as_deref() == Ok("diff")
        && app.loaded_at.is_some()
        && let Some(id) = app
            .visible()
            .iter()
            .find(|c| !c.prs.is_empty())
            .map(|c| c.issue.id.clone())
        && !DIFF.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::Select(id), ctx);
        app.apply(Action::Tab(Tab::Diff), ctx);
    }
    // `ORBIT_DIFF_ALL=1`: Diff all, once some KTLO PR waits on approval.
    static DIFF_ALL: AtomicBool = AtomicBool::new(false);
    if std::env::var("ORBIT_DIFF_ALL").is_ok()
        && app
            .gates
            .values()
            .any(|g| matches!(g, Ok(g) if g.can_approve))
        && !DIFF_ALL.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::DiffAll(true), ctx);
    }
    // `ORBIT_RELEASE=fix`: Fix it on the first red release, once read.
    static RELEASE: AtomicBool = AtomicBool::new(false);
    if std::env::var("ORBIT_RELEASE").as_deref() == Ok("fix")
        && let Some(id) = app.releases.iter().find_map(|(_, run)| match run {
            Ok(Some(run)) if run.failed() => Some(run.id),
            _ => None,
        })
        && !RELEASE.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::FixRelease(id), ctx);
    }
    // `ORBIT_ALERT=open|debug|investigate` on the first alert, once
    // the alerts are in.
    static ALERT: AtomicBool = AtomicBool::new(false);
    if let Ok(what) = std::env::var("ORBIT_ALERT")
        && let Some(first) = app.alerts.first().map(|a| a.id.clone())
        && !ALERT.swap(true, Ordering::Relaxed)
    {
        match what.as_str() {
            "open" => {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("alert-open"), first));
            }
            what => app.apply(
                Action::InvestigateAlert {
                    id: first,
                    debug: what == "debug",
                },
                ctx,
            ),
        }
    }
    // `ORBIT_FIND=<query>` opens find on the screen with that query.
    static FIND: AtomicBool = AtomicBool::new(false);
    if let Ok(query) = std::env::var("ORBIT_FIND")
        && app.loaded_at.is_some()
        && !FIND.swap(true, Ordering::Relaxed)
    {
        app.find = Some(super::Find {
            query,
            ..Default::default()
        });
    }
    // `ORBIT_LINK=<url>` opens it as a click on the link does.
    static LINK: AtomicBool = AtomicBool::new(false);
    if let Ok(url) = std::env::var("ORBIT_LINK")
        && app.loaded_at.is_some()
        && !LINK.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::Link(url), ctx);
    }
    // `ORBIT_CHAT=1` opens the clone panel.
    static CHAT: AtomicBool = AtomicBool::new(false);
    if std::env::var_os("ORBIT_CHAT").is_some()
        && app.loaded_at.is_some()
        && !CHAT.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::ToggleChat, ctx);
    }
    // `ORBIT_PALETTE=<query>` opens the palette with that query.
    static PALETTE: AtomicBool = AtomicBool::new(false);
    if let Ok(query) = std::env::var("ORBIT_PALETTE")
        && app.loaded_at.is_some()
        && !PALETTE.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::TogglePalette, ctx);
        if let Some(palette) = app.palette.as_mut() {
            palette.query = query;
        }
    }
    // `ORBIT_TUTORIAL=<step>` opens the tutorial there, once the
    // board has loaded.
    static TOURED: AtomicBool = AtomicBool::new(false);
    if let Ok(step) = std::env::var("ORBIT_TUTORIAL")
        && let Ok(step) = step.parse()
        && app.loaded_at.is_some()
        && !TOURED.swap(true, Ordering::Relaxed)
    {
        app.apply(Action::Tutorial(Some(step)), ctx);
    }
    dump(app);
    screenshot(app, ctx);
}

/// `ORBIT_DUMP=out.tsv`: once the board and the notifications have
/// loaded, writes For you and Notifications one row per line, to compare
/// what the views list without a screenshot per scroll.
fn dump(app: &App) {
    static DUMPED: AtomicBool = AtomicBool::new(false);
    let Ok(path) = std::env::var("ORBIT_DUMP") else {
        return;
    };
    if app.loaded_at.is_none() || !app.inbox_answered || DUMPED.swap(true, Ordering::Relaxed) {
        return;
    }
    let name =
        |u: &Option<crate::linear::User>| u.as_ref().map_or(String::new(), |u| u.name.clone());
    let mut out = format!("viewer\t{}\n", app.viewer);
    for card in app.for_you() {
        out.push_str(&format!(
            "foryou\t{}\t{:?}\t{:?}\tmine={}\tbot={}\tassignee={}\tcreator={}\t{}\t{}\n",
            card.issue.identifier,
            card.stage,
            card.ask,
            app.is_mine(card),
            card.bot_works,
            name(&card.issue.assignee),
            name(&card.issue.creator),
            card.issue.title,
            card.why,
        ));
    }
    let now = jiff::Timestamp::now();
    for card in app.ktlo() {
        out.push_str(&format!(
            "ktlo\t{}\t{}\t{:?}\tunhandled={}\tassignee={}\t{}\t{}\n",
            card.lane.label(),
            card.issue.identifier,
            card.stage,
            crate::story::unhandled(card, now),
            name(&card.issue.assignee),
            card.issue.title,
            card.why,
        ));
    }
    let (items, _) = app.inbox();
    for item in items {
        out.push_str(&format!(
            "inbox\t{:?}\t{}\t{}\t{}\tissue={}\n",
            item.ask,
            item.title,
            item.why,
            item.url,
            item.issue
                .as_deref()
                .and_then(|id| app.card(id))
                .map_or("", |c| c.issue.identifier.as_str()),
        ));
    }
    if let Err(error) = std::fs::write(&path, out) {
        eprintln!("could not write {path}: {error}");
    }
}

/// Saves one frame as PPM (no image crate needed; `sips` converts it) and
/// closes the window, so the interface can be checked without
/// screen-recording rights.
fn screenshot(app: &App, ctx: &egui::Context) {
    static REQUESTED: AtomicBool = AtomicBool::new(false);
    let Ok(path) = std::env::var("ORBIT_SCREENSHOT") else {
        return;
    };
    let delay: f64 = std::env::var("ORBIT_SCREENSHOT_DELAY")
        .ok()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);
    let image = ctx.input(|i| {
        i.raw.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(image) = image {
        let [w, h] = image.size;
        let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
        ppm.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
        let _ = std::fs::write(path, ppm);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else if (app.loaded_at.is_some() || !app.checking())
        && ctx.input(|i| i.time) > delay
        && !REQUESTED.swap(true, Ordering::Relaxed)
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }
    ctx.request_repaint();
}

/// Simulated users (scripts/simulate.sh): `ORBIT_SIMULATE=<steps.json>`
/// plays steps on the loaded board and saves a frame at each `shot` into
/// `ORBIT_SIM_OUT`, then quits. Only with `ORBIT_READ_ONLY=1`: a simulated
/// user must never change anything in Linear or GitHub (main.rs read_only).
///
/// Steps, one JSON string each: `show <view>`, `select <n>` (the n-th ticket
/// of the list), `tab <walkthrough|diff|claude|testing>`, `palette <query>`,
/// `note <text>` (New ticket with it), `ask <text>` (a question about the
/// shown PR), `do <Action>` for the ones without arguments (ReadNote,
/// FileAsIs, CloseCapture, SpotWork, Refresh, TogglePalette, Triage,
/// NewTicket, Orchestrate, Guide, BrowserWindow, ToggleChat), `browse <url>` (the focused
/// browser pane goes there), `wait <s>`, `shot <name>`.
pub(super) fn simulate(app: &mut App, ctx: &egui::Context) {
    use std::sync::Mutex;
    struct Run {
        steps: Vec<String>,
        next: usize,
        until: f64,
        shooting: Option<String>,
        log: Vec<String>,
    }
    static RUN: Mutex<Option<Run>> = Mutex::new(None);
    let Ok(script) = std::env::var("ORBIT_SIMULATE") else {
        return;
    };
    if !crate::read_only() {
        eprintln!("ORBIT_SIMULATE needs ORBIT_READ_ONLY=1: a simulated user changes nothing");
        std::process::exit(2);
    }
    let out =
        std::path::PathBuf::from(std::env::var("ORBIT_SIM_OUT").unwrap_or("target/sim".into()));
    let mut guard = RUN.lock().unwrap();
    let run = guard.get_or_insert_with(|| Run {
        steps: std::fs::read_to_string(&script)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default(),
        next: 0,
        until: 0.0,
        shooting: None,
        log: Vec::new(),
    });
    ctx.request_repaint();
    // The board first, like a person waits for it.
    if app.loaded_at.is_none() && app.checking() {
        return;
    }
    let now = ctx.input(|i| i.time);
    if let Some(name) = run.shooting.clone() {
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
            ppm.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
            let _ = std::fs::create_dir_all(&out);
            let _ = std::fs::write(out.join(format!("{name}.ppm")), ppm);
            run.shooting = None;
        }
        return;
    }
    if now < run.until {
        return;
    }
    let Some(step) = run.steps.get(run.next).cloned() else {
        // Done: what happened, for whoever reads the frames.
        let _ = std::fs::write(out.join("steps.log"), run.log.join("\n"));
        let _ = std::fs::write(out.join("done"), "");
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    };
    run.next += 1;
    let (verb, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
    let note = match play(app, ctx, verb, rest.trim()) {
        Ok(Some(wait)) => {
            run.until = now + wait;
            "ok".to_owned()
        }
        Ok(None) if verb == "shot" => {
            run.shooting = Some(rest.trim().to_owned());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            "ok".to_owned()
        }
        Ok(None) => {
            // A beat after every step, so the frame shows its effect.
            run.until = now + 0.8;
            "ok".to_owned()
        }
        Err(why) => why,
    };
    let error = app.error.clone().unwrap_or_default();
    run.log.push(format!(
        "{step} -> {note}{}",
        if error.is_empty() {
            String::new()
        } else {
            format!(" (app error: {error})")
        }
    ));
}

/// One step. `Ok(Some(s))`: wait `s` seconds; `Err`: why it could not run.
fn play(app: &mut App, ctx: &egui::Context, verb: &str, rest: &str) -> Result<Option<f64>, String> {
    let view = |name: &str| match name {
        "foryou" => Some(View::ForYou),
        "intake" => Some(View::Intake),
        "sessions" => Some(View::Sessions),
        "customers" => Some(View::Customers),
        "ktlo" => Some(View::Ktlo),
        "notifications" => Some(View::Inbox),
        "settings" => Some(View::Settings),
        "repos" => Some(View::Repos),
        "stats" => Some(View::Stats),
        other => other
            .strip_prefix("repo ")
            .map(|slug| View::Repo(slug.to_owned())),
    };
    let action = match verb {
        "wait" => {
            return rest
                .parse()
                .map(Some)
                .map_err(|_| "wait needs seconds".into());
        }
        "shot" => return Ok(None),
        "show" => Action::Show(view(rest).ok_or(format!("no view {rest}"))?),
        "select" => {
            let n: usize = rest.parse().map_err(|_| "select needs a number")?;
            let id = app
                .visible()
                .get(n.saturating_sub(1))
                .map(|c| c.issue.id.clone())
                .ok_or(format!("no ticket {n} in this list"))?;
            Action::Select(id)
        }
        "focus" => Action::Focus(rest.to_owned()),
        "pr" => Action::OpenPr {
            url: rest.to_owned(),
            title: "PR".into(),
            at: jiff::Timestamp::now().to_string(),
        },
        "tab" => Action::Tab(match rest {
            "walkthrough" => Tab::Walkthrough,
            "diff" => Tab::Diff,
            "claude" => Tab::Claude,
            "testing" => Tab::Testing,
            other => return Err(format!("no tab {other}")),
        }),
        "palette" => {
            if app.palette.is_none() {
                app.apply(Action::TogglePalette, ctx);
            }
            if let Some(palette) = app.palette.as_mut() {
                palette.query = rest.to_owned();
            }
            return Ok(None);
        }
        "note" => Action::NewTicketWith(rest.to_owned()),
        // The strip's windows: rename the current one (empty: automatic),
        // move one, go to one, as the tab menu and the keys do.
        "rename" => Action::RenameWindow(app.window, rest.to_owned()),
        "move" => {
            let (from, to) = rest.split_once(' ').ok_or("move needs <from> <to>")?;
            Action::MoveWindow {
                from: from.parse().map_err(|_| "move needs numbers")?,
                to: to.parse().map_err(|_| "move needs numbers")?,
            }
        }
        "window" => Action::GoWindow(rest.parse().map_err(|_| "window needs a number")?),
        "search" => Action::SheetQuery(rest.to_owned()),
        // A key press as the window gets it (enter, escape, up, down), so
        // the palette's Enter and the like run their real path.
        "key" => {
            let key = match rest {
                "enter" => egui::Key::Enter,
                "escape" => egui::Key::Escape,
                "up" => egui::Key::ArrowUp,
                "down" => egui::Key::ArrowDown,
                other => return Err(format!("no key {other}")),
            };
            ctx.input_mut(|i| {
                i.events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
                if key == egui::Key::Enter {
                    i.keys_down.insert(key);
                }
            });
            return Ok(None);
        }
        "ask" => Action::AskPr(rest.to_owned()),
        // The focused browser pane goes to an address.
        "browse" => Action::BrowserGo(app.panes.focused, rest.to_owned()),
        "do" => match rest {
            "Guide" => Action::Guide,
            "Split" => Action::Split { across: true },
            "ShellWindow" => Action::ShellWindow,
            "NewTab" => Action::NewTab,
            "ReadNote" => Action::ReadNote,
            "CloseCapture" => Action::CloseCapture,
            "SpotWork" => Action::SpotWork(None),
            "Refresh" => Action::Refresh,
            "TogglePalette" => Action::TogglePalette,
            "Triage" => Action::Triage,
            "NewTicket" => Action::NewTicket,
            "Sheet" => Action::Sheet(!app.sheet),
            "ToggleChat" => Action::ToggleChat,
            "Orchestrate" => Action::Orchestrate,
            "FileAsIs" => Action::FileAsIs,
            "BrowserWindow" => Action::BrowserWindow,
            other => return Err(format!("no action {other}")),
        },
        other => return Err(format!("no step {other}")),
    };
    app.apply(action, ctx);
    Ok(None)
}

/// `ORBIT_SCROLL=<px>`: how far down the ticket page starts.
pub fn scroll() -> Option<f32> {
    std::env::var("ORBIT_SCROLL").ok()?.parse().ok()
}
