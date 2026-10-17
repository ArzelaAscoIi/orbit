//! The menu bar icon (macOS): an orbit ring with the count of what waits
//! on the user, and a menu of the running sessions and what For you lists.
//! A line clicked comes back as its id (`clicked`) and the app opens it.
//! settings.json `menuBar: false` takes the icon away.
//!
//! The menu is rebuilt only when its lines change; it lives on the main
//! thread, as AppKit wants, and is touched only from `App::logic`.

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// One menu line: an id to open (empty: a header, greyed) and its text;
/// `-` as the text is a separator.
pub type Line = (String, String);

pub struct Tray {
    icon: TrayIcon,
    shown: Vec<Line>,
    title: String,
}

impl Tray {
    /// The icon in the menu bar; a click on a line repaints `ctx` so the
    /// app takes it at once, even behind other windows.
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        // A handler takes every event from muda's own receiver, so it keeps
        // the ids itself. muda keeps only the first handler ever set.
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            CLICKED.lock().unwrap().push(e.id.0);
            ctx.request_repaint();
        }));
        let icon = TrayIconBuilder::new()
            .with_icon_templated(ring())
            .with_tooltip(crate::NAME)
            .build()
            .ok()?;
        Some(Self {
            icon,
            shown: Vec::new(),
            title: String::new(),
        })
    }

    /// Puts these lines in the menu and `title` beside the icon.
    pub fn show(&mut self, title: String, lines: Vec<Line>) {
        if title != self.title {
            self.icon.set_title((!title.is_empty()).then_some(&title));
            self.title = title;
        }
        if lines == self.shown {
            return;
        }
        let menu = Menu::new();
        for (id, text) in &lines {
            let _ = if text == "-" {
                menu.append(&PredefinedMenuItem::separator())
            } else {
                menu.append(&MenuItem::with_id(id.as_str(), text, !id.is_empty(), None))
            };
        }
        self.icon.set_menu(Some(Box::new(menu)));
        self.shown = lines;
    }
}

static CLICKED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The ids of the lines clicked since the last call.
pub fn clicked() -> Vec<String> {
    std::mem::take(&mut *CLICKED.lock().unwrap())
}

/// A ring with a moon on it, black on clear: macOS tints a template icon
/// for the menu bar's light or dark look.
fn ring() -> Icon {
    const N: u32 = 36;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    let c = N as f32 / 2.0;
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let r = (dx * dx + dy * dy).sqrt();
            let (mx, my) = (dx - 9.2, dy + 9.2);
            let moon = (mx * mx + my * my).sqrt() < 4.5;
            let on = moon || ((r - 11.0).abs() < 1.8 && (mx * mx + my * my).sqrt() > 6.5);
            rgba.extend_from_slice(&[0, 0, 0, if on { 255 } else { 0 }]);
        }
    }
    Icon::from_rgba(rgba, N, N).expect("36x36 RGBA")
}

/// The menu's lines, ids as `notify::action_for` reads them: what needs
/// the user (notify's open lines), every running session with its state,
/// the rest of For you (`mine`: id and text), and Open Orbit.
pub fn lines(
    needs: &std::collections::BTreeMap<String, String>,
    sessions: &[(String, String, String)],
    mine: &[(String, String)],
) -> Vec<Line> {
    let mut out = Vec::new();
    let mut section = |title: &str, rows: Vec<Line>| {
        if rows.is_empty() {
            return;
        }
        if !out.is_empty() {
            out.push((String::new(), "-".into()));
        }
        out.push((String::new(), title.into()));
        out.extend(rows);
    };
    section(
        "Needs you",
        needs.iter().map(|(k, l)| (k.clone(), l.clone())).collect(),
    );
    section(
        "Sessions",
        sessions
            .iter()
            .map(|(key, name, state)| {
                (
                    format!("session:{key}:{state}"),
                    format!("{name} · {state}"),
                )
            })
            .collect(),
    );
    section(
        "For you",
        mine.iter()
            .map(|(id, text)| (format!("card:{id}:mine"), text.clone()))
            .collect(),
    );
    out.push((String::new(), "-".into()));
    out.push(("show".into(), format!("Open {}", crate::NAME)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_opens_what_it_names() {
        let needs = [("card:abc:Review".to_owned(), "Review ENG-1: x".to_owned())].into();
        let sessions = [(
            "ENG-2".to_owned(),
            "fix it".to_owned(),
            "working".to_owned(),
        )];
        let mine = [("def".to_owned(), "ENG-3 mine".to_owned())];
        let lines = lines(&needs, &sessions, &mine);
        let ids: Vec<&str> = lines
            .iter()
            .map(|(id, _)| id.as_str())
            .filter(|i| !i.is_empty())
            .collect();
        assert_eq!(
            ids,
            [
                "card:abc:Review",
                "session:ENG-2:working",
                "card:def:mine",
                "show"
            ]
        );
        for id in &ids[..3] {
            assert!(crate::notify::action_for(id).is_some(), "{id}");
        }
        // Empty sections leave no header behind.
        let lone = super::lines(&Default::default(), &[], &[]);
        assert_eq!(lone.len(), 2);
    }
}
