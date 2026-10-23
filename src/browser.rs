//! The embedded browser for testing UI changes (the Testing tab of a UI
//! test plan): a native WebKit view laid over the egui window.
//!
//! Views never own it. While drawing, a view that wants it calls `place`
//! with the rect to fill and the page it starts on; after the frame,
//! `App` calls `follow`, which shows the view there, or hides it when no
//! one asked this frame. A native view sits above everything egui draws,
//! so it is also hidden while the tutorial dims the window.
//!
//! The web view keeps WebKit's default, persistent data store, so a login
//! in it survives restarts. Only macOS has it; elsewhere `available()` is
//! false and the tab offers the system browser instead.
//!
//! There is one per slot: the Testing tab's (`TESTING`) and one per
//! browser pane (Cmd+N, then Browser), keyed by the pane's id.
//!
//! A focused web view takes every key, so the app's shortcuts would die
//! in it. A script in each page sends Cmd/Ctrl keys back over IPC (all
//! but the page's own copy, paste, cut, select all and undo), and
//! `take_keys` hands them to egui as if pressed on the window. Cmd+F
//! opens the find bar (`finding`), which searches the page with
//! `window.find`.

use std::sync::{Arc, Mutex};

use egui::{Id, Rect};

/// The Testing tab's browser; a pane's uses the pane's id.
pub const TESTING: u64 = u64::MAX;

/// Sends the page's shortcuts to the app: `find`, or `key:<code>:<mods>`.
const KEYS: &str = r#"document.addEventListener('keydown', function (e) {
  if (!(e.metaKey || e.ctrlKey)) return;
  var k = (e.key || '').toLowerCase();
  if (k === 'f' && !e.shiftKey && !e.altKey) {
    e.preventDefault();
    window.ipc.postMessage('find');
    return;
  }
  if (!e.shiftKey && !e.altKey && ['c', 'v', 'x', 'a', 'z'].indexOf(k) >= 0) return;
  if (e.shiftKey && k === 'z') return;
  e.preventDefault();
  window.ipc.postMessage('key:' + e.code + ':' + (e.metaKey ? 'm' : '') + (e.ctrlKey ? 'c' : '')
    + (e.shiftKey ? 's' : '') + (e.altKey ? 'a' : ''));
}, true);"#;

/// Asks for the browser in `slot` this frame, filling `rect`, starting at `home`.
pub fn place(ctx: &egui::Context, slot: u64, rect: Rect, home: &str) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(Id::unique(("browser", slot)), (rect, home.to_owned(), pass)));
}

/// This frame's request for `slot`, if a view made one.
fn placed(ctx: &egui::Context, slot: u64) -> Option<(Rect, String)> {
    let (rect, home, pass) =
        ctx.data(|d| d.get_temp::<(Rect, String, u64)>(Id::unique(("browser", slot))))?;
    (pass == ctx.cumulative_pass_nr() && rect.is_positive()).then_some((rect, home))
}

/// A message the page sent: open the find bar, or a key for the app.
#[derive(Clone, Debug, PartialEq)]
enum Message {
    Find,
    Key(egui::Key, egui::Modifiers),
}

fn message(text: &str) -> Option<Message> {
    if text == "find" {
        return Some(Message::Find);
    }
    let (code, mods) = text.strip_prefix("key:")?.rsplit_once(':')?;
    let key = egui::Key::from_name(code)?;
    let modifiers = egui::Modifiers {
        alt: mods.contains('a'),
        ctrl: mods.contains('c'),
        shift: mods.contains('s'),
        mac_cmd: mods.contains('m'),
        command: mods.contains('m') || (!cfg!(target_os = "macos") && mods.contains('c')),
    };
    Some(Message::Key(key, modifiers))
}

pub fn available() -> bool {
    cfg!(target_os = "macos")
}

#[derive(Default)]
pub struct Browser {
    /// Which `place` request it follows.
    slot: u64,
    #[cfg(target_os = "macos")]
    view: Option<wry::WebView>,
    /// What the page sent over IPC since the last frame.
    inbox: Arc<Mutex<Vec<String>>>,
    /// The find bar is open (Cmd+F in the page or on its pane).
    pub finding: bool,
    /// Counts each opening of the find bar, so its field takes the focus
    /// once per opening (the UI remembers the last count it saw).
    pub find_opened: u64,
    /// The page the current request started on; a new one navigates.
    home: String,
    /// Where the view is now, for the address bar.
    current: String,
    error: Option<String>,
}

impl Browser {
    pub fn new(slot: u64) -> Self {
        Self {
            slot,
            ..Default::default()
        }
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Shows the view where this frame asked for it, or hides it.
    pub fn follow(&mut self, ctx: &egui::Context, frame: &eframe::Frame, allowed: bool) {
        let request = placed(ctx, self.slot).filter(|_| allowed);
        #[cfg(target_os = "macos")]
        self.follow_native(ctx, frame, request);
        #[cfg(not(target_os = "macos"))]
        let _ = (frame, request);
    }

    pub fn go(&mut self, url: &str) {
        let url = with_scheme(url);
        if !loadable(&url) {
            self.error = Some(format!("Not an address a browser can open: {url}"));
            return;
        }
        #[cfg(target_os = "macos")]
        if let Some(view) = &self.view {
            self.error = view.load_url(&url).err().map(|e| e.to_string());
        }
        self.current = url;
    }

    pub fn back(&self) {
        #[cfg(target_os = "macos")]
        if let Some(view) = &self.view {
            let _ = view.evaluate_script("history.back()");
        }
    }

    pub fn reload(&self) {
        #[cfg(target_os = "macos")]
        if let Some(view) = &self.view {
            let _ = view.reload();
        }
    }

    pub fn open_find(&mut self) {
        self.finding = true;
        self.find_opened += 1;
    }

    /// Finds `text` in the page, the next match (or the one before),
    /// wrapping; WebKit selects it and scrolls to it.
    pub fn find(&self, text: &str, backwards: bool) {
        #[cfg(target_os = "macos")]
        if let Some(view) = &self.view
            && !text.is_empty()
        {
            let _ = view.evaluate_script(&find_script(text, backwards));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (text, backwards);
    }

    /// The keys the page passed on since the last frame, for egui; a Cmd+F
    /// in the page opens the find bar instead.
    pub fn take_keys(&mut self) -> Vec<(egui::Key, egui::Modifiers)> {
        let messages = std::mem::take(&mut *self.inbox.lock().unwrap());
        let mut keys = Vec::new();
        for message in messages.iter().filter_map(|m| message(m)) {
            match message {
                Message::Find => self.open_find(),
                Message::Key(key, modifiers) => keys.push((key, modifiers)),
            }
        }
        keys
    }

    #[cfg(target_os = "macos")]
    fn follow_native(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        request: Option<(Rect, String)>,
    ) {
        use wry::dpi::{PhysicalPosition, PhysicalSize};
        let Some((rect, home)) = request else {
            if let Some(view) = &self.view {
                let _ = view.set_visible(false);
            }
            return;
        };
        // Physical pixels, so egui zoom and the display's scale both hold.
        let ppp = ctx.pixels_per_point();
        let bounds = wry::Rect {
            position: PhysicalPosition::new(rect.min.x * ppp, rect.min.y * ppp).into(),
            size: PhysicalSize::new(rect.width() * ppp, rect.height() * ppp).into(),
        };
        // Never hand wry an address WebKit rejects: it panics on those.
        let home = Some(with_scheme(&home))
            .filter(|h| loadable(h))
            .unwrap_or_else(|| "about:blank".into());
        match &self.view {
            None => {
                // An address asked for before the view existed goes first.
                let start = Some(self.current.clone())
                    .filter(|c| loadable(c) && c != "about:blank")
                    .unwrap_or_else(|| home.clone());
                let inbox = self.inbox.clone();
                let built = wry::WebViewBuilder::new()
                    .with_initialization_script(KEYS)
                    .with_ipc_handler(move |request| {
                        inbox.lock().unwrap().push(request.body().clone());
                    })
                    .with_url(&start)
                    .with_bounds(bounds)
                    .build_as_child(frame);
                match built {
                    Ok(view) => {
                        self.view = Some(view);
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
                self.home = home;
                self.current = start;
            }
            Some(view) => {
                let _ = view.set_bounds(bounds);
                let _ = view.set_visible(true);
                if home != self.home {
                    self.home = home.clone();
                    self.go(&home);
                }
                if let Some(view) = &self.view
                    && let Ok(url) = view.url()
                {
                    self.current = url;
                }
            }
        }
    }
}

/// Whether WebKit will take the address. wry unwraps the `NSURL` it makes
/// from it, so one WebKit rejects (spaces, `<WORKSPACE>` placeholders,
/// non-ASCII) would crash the app.
pub fn loadable(url: &str) -> bool {
    let clean = url
        .chars()
        .all(|c| c.is_ascii_graphic() && !"<>\"{}|\\^`".contains(c));
    if url.starts_with("about:") || url.starts_with("data:") {
        return clean;
    }
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    // host[:port], then the path: a real host, and a port only of digits.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = authority.split_once(':').unwrap_or((authority, ""));
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let port_ok =
        port.chars().all(|c| c.is_ascii_digit()) && (port.is_empty() == !authority.contains(':'));
    clean && host_ok && port_ok
}

/// A browser's name in the windows list: the page's host (with its
/// port), or `browser` on a blank page.
pub fn site(url: &str) -> String {
    let rest = url.split_once("://").map_or("", |(_, rest)| rest);
    match rest.split(['/', '?', '#']).next().unwrap_or_default() {
        "" => "browser".into(),
        host => host.trim_start_matches("www.").into(),
    }
}

/// `window.find` for `text`, case-insensitive, wrapping; the text as a JSON
/// string, so quotes in it cannot break out of the script.
fn find_script(text: &str, backwards: bool) -> String {
    let text = serde_json::to_string(text).unwrap_or_default();
    format!("window.find({text}, false, {backwards}, true)")
}

/// `app.example.com/x` → `https://app.example.com/x`; a blank page stays one.
fn with_scheme(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        "about:blank".into()
    } else if url.contains("://") || url.starts_with("about:") {
        url.into()
    } else {
        format!("https://{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_shortcuts_reach_the_app_and_find_opens_the_bar() {
        assert_eq!(message("find"), Some(Message::Find));
        let Some(Message::Key(key, mods)) = message("key:KeyN:m") else {
            panic!("Cmd+N is a key");
        };
        assert_eq!(key, egui::Key::N);
        assert!(mods.command && mods.mac_cmd && !mods.shift);
        let Some(Message::Key(key, mods)) = message("key:BracketRight:ms") else {
            panic!("Cmd+Shift+] is a key");
        };
        assert_eq!(key, egui::Key::CloseBracket);
        assert!(mods.command && mods.shift);
        assert_eq!(message("key:Nonsense:m"), None);
        assert_eq!(message("hello"), None);
        let mut browser = Browser::new(7);
        browser
            .inbox
            .lock()
            .unwrap()
            .extend(["find".into(), "key:Digit1:m".into()]);
        assert_eq!(browser.take_keys().len(), 1);
        assert!(browser.finding && browser.find_opened == 1);
        // The page's own editing keys stay in the page.
        assert!(KEYS.contains("['c', 'v', 'x', 'a', 'z']"));
    }

    #[test]
    fn a_browser_is_named_by_its_site() {
        assert_eq!(site("about:blank"), "browser");
        assert_eq!(site(""), "browser");
        assert_eq!(site("http://localhost:3000/x?y"), "localhost:3000");
        assert_eq!(site("https://www.example.com/a"), "example.com");
    }

    #[test]
    fn find_text_cannot_break_out_of_the_script() {
        assert_eq!(
            find_script(r#"a"); alert(1); ("#, true),
            r#"window.find("a\"); alert(1); (", false, true, true)"#
        );
    }

    #[test]
    fn addresses_get_a_scheme() {
        assert_eq!(
            with_scheme("app.example.com/reports"),
            "https://app.example.com/reports"
        );
        assert_eq!(
            with_scheme("http://localhost:3000"),
            "http://localhost:3000"
        );
        assert_eq!(with_scheme("  "), "about:blank");
    }

    #[test]
    fn addresses_webkit_rejects_are_never_loaded() {
        assert!(loadable("https://app.example.com/reports?x=1#top"));
        assert!(loadable("data:text/html;base64,PGh0bWw+"));
        assert!(loadable("http://localhost:3000/reports"));
        for bad in [
            "https://app.example.com/<WORKSPACE>/reports",
            "https://Reports page",
            "https://example.com/ü",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://localhost:/x",
        ] {
            assert!(!loadable(&with_scheme(bad)), "{bad}");
        }
    }
}
