//! The right column as split panes, the way a terminal splits its window:
//! Cmd+D splits the focused pane to the right, Cmd+Shift+D below, a shell
//! opens a shell beside it.
//!
//! Every pane but a shell is a full ticket view: walkthrough, diff,
//! testing and Claude tabs, for its own ticket. One ticket pane is
//! active: its view lives in `App::selected`, `tab` and `session`, so the
//! list, j/k and every ticket action work on it unchanged. The other
//! ticket panes keep their view here until they are focused, when they
//! swap with the active one (`App::focus_pane`). Shells are for k9s, logs
//! and the like. Any pane closes while another stays; a window's last pane
//! closes the window (`Action::ClosePane`).
//!
//! Each window in the strip (`App::windows`) is one of these trees. Pane
//! ids come from one counter for all of them, so a shell's terminal and a
//! pane's egui ids never meet another window's.
//!
//! Plain data and tree operations here; drawing is in `ui/panes.rs`.

use std::sync::atomic::{AtomicU64, Ordering};

use egui::Rect;

use serde::{Deserialize, Serialize};

use super::Tab;

/// What a ticket pane shows while it is not the active one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketView {
    pub selected: Option<String>,
    pub tab: Tab,
    pub session: Option<String>,
    /// The PR its Diff tab shows (`App::diff_pr`).
    pub diff_pr: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface {
    /// The active pane's entry is stale; its view is in `App`.
    Ticket(TicketView),
    /// A login shell; its terminal lives in `App::shells` under the pane id.
    Shell,
    /// A plain text editor; its text lives in `App::editors` under the pane id.
    Editor,
    /// A web page (browser.rs); its view lives in `App::browsers` under the pane id.
    Browser,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Node {
    Leaf {
        id: u64,
        surface: Surface,
    },
    Split {
        id: u64,
        /// Side by side when true, stacked when false.
        across: bool,
        /// The first child's share of the space.
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Panes {
    root: Node,
    /// The pane with the keyboard and the highlight.
    pub focused: u64,
    /// The ticket pane whose view is `App`'s.
    pub active: u64,
    /// Only the focused pane, over the whole column.
    pub zoomed: bool,
    /// The window's name in the strip when it has a fixed one (`debug
    /// prod`); otherwise its panes name it.
    #[serde(default)]
    pub title: Option<String>,
}

/// The next pane or split id, unique across every window. Pane 0 is the
/// first window's first pane.
static NEXT: AtomicU64 = AtomicU64::new(1);

fn fresh() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// What `layout` gives the drawing code: panes and the dividers between
/// them, each with its rect.
pub struct Layout {
    pub panes: Vec<(u64, Surface, Rect)>,
    /// `(split id, across, the split's rect, the divider's rect)`.
    pub dividers: Vec<(u64, bool, Rect, Rect)>,
}

const DIVIDER: f32 = 1.0;

impl Default for Panes {
    fn default() -> Self {
        Self {
            root: Node::Leaf {
                id: 0,
                surface: Surface::Ticket(TicketView {
                    selected: None,
                    tab: Tab::Walkthrough,
                    session: None,
                    diff_pr: None,
                }),
            },
            focused: 0,
            active: 0,
            zoomed: false,
            title: None,
        }
    }
}

impl Panes {
    /// A new window with one pane showing `surface`.
    pub fn new(surface: Surface) -> Self {
        let id = fresh();
        Self {
            root: Node::Leaf { id, surface },
            focused: id,
            active: id,
            zoomed: false,
            title: None,
        }
    }

    /// Keeps ids handed out from now on clear of this tree's, for a
    /// window read back from disk.
    pub fn reserve(&self) {
        NEXT.fetch_max(max_id(&self.root) + 1, Ordering::Relaxed);
    }

    /// The active pane shows a ticket (a window of shells has none).
    pub fn ticket_active(&self) -> bool {
        matches!(self.surface(self.active), Some(Surface::Ticket(_)))
    }

    pub fn single(&self) -> bool {
        matches!(self.root, Node::Leaf { .. })
    }

    /// Splits the focused pane; the new half shows `surface`. Focus stays;
    /// the caller moves it. Returns the new pane's id.
    pub fn split(&mut self, across: bool, surface: Surface) -> u64 {
        let (split, pane) = (fresh(), fresh());
        let focused = self.focused;
        let mut surface = Some(surface);
        replace(&mut self.root, focused, &mut |leaf| Node::Split {
            id: split,
            across,
            ratio: 0.5,
            first: Box::new(leaf),
            second: Box::new(Node::Leaf {
                id: pane,
                surface: surface.take().expect("one leaf"),
            }),
        });
        self.zoomed = false;
        pane
    }

    /// Closes a pane; its sibling takes its space. A window's last pane
    /// stays (false): the caller closes the window instead. A ticket pane
    /// beside a shell closes like any other, so a window can end up with
    /// shells only, as a shell window starts. The caller re-homes focus and
    /// the active view.
    pub fn close(&mut self, id: u64) -> bool {
        let closed = remove(&mut self.root, id);
        if closed && self.focused == id {
            self.zoomed = false;
        }
        closed
    }

    pub fn set(&mut self, id: u64, surface: Surface) {
        set(&mut self.root, id, surface);
    }

    pub fn set_ratio(&mut self, split: u64, ratio: f32) {
        set_ratio(&mut self.root, split, ratio.clamp(0.15, 0.85));
    }

    /// The pane after (+1) or before (-1) the focused one, in reading order.
    pub fn neighbour(&self, step: i32) -> u64 {
        let ids = self.ids();
        let at = ids.iter().position(|&i| i == self.focused).unwrap_or(0) as i32;
        let n = ids.len() as i32;
        ids[((at + step).rem_euclid(n)) as usize]
    }

    pub fn ids(&self) -> Vec<u64> {
        let mut out = Vec::new();
        leaves(&self.root, &mut |id, _| out.push(id));
        out
    }

    /// Ticket panes, in reading order.
    pub fn tickets(&self) -> Vec<u64> {
        let mut out = Vec::new();
        leaves(&self.root, &mut |id, surface| {
            if matches!(surface, Surface::Ticket(_)) {
                out.push(id);
            }
        });
        out
    }

    pub fn surface(&self, id: u64) -> Option<&Surface> {
        let mut found = None;
        find(&self.root, id, &mut found);
        found
    }

    /// The ticket pane that keeps this ticket (issue id), if one does. The
    /// active pane's kept view is stale: `App` checks that one itself.
    pub fn showing(&self, ticket: &str) -> Option<u64> {
        self.others()
            .into_iter()
            .find(|(_, v)| v.selected.as_deref() == Some(ticket))
            .map(|(id, _)| id)
    }

    /// Any ticket pane that shows `ticket`, the active one too: for a
    /// window not in front, whose active pane holds its view (stashed).
    pub fn showing_stored(&self, ticket: &str) -> Option<u64> {
        self.ids().into_iter().find(|&id| {
            matches!(self.surface(id), Some(Surface::Ticket(v)) if v.selected.as_deref() == Some(ticket))
        })
    }

    /// The inactive ticket pane that shows session `key`, if one does.
    pub fn showing_session(&self, key: &str) -> Option<u64> {
        self.others()
            .into_iter()
            .find(|(_, v)| v.session.as_deref() == Some(key))
            .map(|(id, _)| id)
    }

    /// What each ticket pane is about: its ticket, else its session.
    pub fn keys(&self) -> Vec<String> {
        self.ids()
            .into_iter()
            .filter_map(|id| match self.surface(id)? {
                Surface::Ticket(v) => v.selected.clone().or(v.session.clone()),
                Surface::Shell | Surface::Editor | Surface::Browser => None,
            })
            .collect()
    }

    /// The ticket each inactive ticket pane shows.
    pub fn others(&self) -> Vec<(u64, &TicketView)> {
        let mut out = Vec::new();
        collect(&self.root, self.active, &mut out);
        out
    }

    /// Where every pane and divider goes inside `rect`.
    pub fn layout(&self, rect: Rect) -> Layout {
        let mut out = Layout {
            panes: Vec::new(),
            dividers: Vec::new(),
        };
        if self.zoomed
            && let Some(surface) = self.surface(self.focused)
        {
            out.panes.push((self.focused, surface.clone(), rect));
            return out;
        }
        place(&self.root, rect, &mut out);
        out
    }
}

fn collect<'a>(node: &'a Node, active: u64, out: &mut Vec<(u64, &'a TicketView)>) {
    match node {
        Node::Leaf {
            id,
            surface: Surface::Ticket(view),
        } if *id != active => out.push((*id, view)),
        Node::Leaf { .. } => {}
        Node::Split { first, second, .. } => {
            collect(first, active, out);
            collect(second, active, out);
        }
    }
}

fn place(node: &Node, rect: Rect, out: &mut Layout) {
    match node {
        Node::Leaf { id, surface } => out.panes.push((*id, surface.clone(), rect)),
        Node::Split {
            id,
            across,
            ratio,
            first,
            second,
        } => {
            let (a, divider, b) = if *across {
                let x = rect.left() + (rect.width() - DIVIDER) * ratio;
                (
                    Rect::from_min_max(rect.min, egui::pos2(x, rect.bottom())),
                    Rect::from_min_max(
                        egui::pos2(x, rect.top()),
                        egui::pos2(x + DIVIDER, rect.bottom()),
                    ),
                    Rect::from_min_max(egui::pos2(x + DIVIDER, rect.top()), rect.max),
                )
            } else {
                let y = rect.top() + (rect.height() - DIVIDER) * ratio;
                (
                    Rect::from_min_max(rect.min, egui::pos2(rect.right(), y)),
                    Rect::from_min_max(
                        egui::pos2(rect.left(), y),
                        egui::pos2(rect.right(), y + DIVIDER),
                    ),
                    Rect::from_min_max(egui::pos2(rect.left(), y + DIVIDER), rect.max),
                )
            };
            out.dividers.push((*id, *across, rect, divider));
            place(first, a, out);
            place(second, b, out);
        }
    }
}

fn max_id(node: &Node) -> u64 {
    match node {
        Node::Leaf { id, .. } => *id,
        Node::Split {
            id, first, second, ..
        } => (*id).max(max_id(first)).max(max_id(second)),
    }
}

fn leaves(node: &Node, visit: &mut dyn FnMut(u64, &Surface)) {
    match node {
        Node::Leaf { id, surface } => visit(*id, surface),
        Node::Split { first, second, .. } => {
            leaves(first, visit);
            leaves(second, visit);
        }
    }
}

fn find<'a>(node: &'a Node, id: u64, found: &mut Option<&'a Surface>) {
    match node {
        Node::Leaf { id: i, surface } if *i == id => *found = Some(surface),
        Node::Leaf { .. } => {}
        Node::Split { first, second, .. } => {
            find(first, id, found);
            find(second, id, found);
        }
    }
}

/// Replaces the leaf `id` with what `with` makes of it.
fn replace(node: &mut Node, id: u64, with: &mut dyn FnMut(Node) -> Node) {
    match node {
        Node::Leaf { id: i, .. } if *i == id => {
            let leaf = std::mem::replace(
                node,
                Node::Leaf {
                    id: u64::MAX,
                    surface: Surface::Shell,
                },
            );
            *node = with(leaf);
        }
        Node::Leaf { .. } => {}
        Node::Split { first, second, .. } => {
            replace(first, id, with);
            replace(second, id, with);
        }
    }
}

/// Removes the leaf `id`: its parent split becomes the sibling.
fn remove(node: &mut Node, id: u64) -> bool {
    let Node::Split { first, second, .. } = node else {
        return false;
    };
    let is = |n: &Node| matches!(n, Node::Leaf { id: i, .. } if *i == id);
    let keep = if is(first) {
        Some((**second).clone())
    } else if is(second) {
        Some((**first).clone())
    } else {
        None
    };
    match keep {
        Some(sibling) => {
            *node = sibling;
            true
        }
        None => remove(first, id) || remove(second, id),
    }
}

fn set(node: &mut Node, id: u64, to: Surface) {
    match node {
        Node::Leaf { id: i, surface } if *i == id => *surface = to,
        Node::Leaf { .. } => {}
        Node::Split { first, second, .. } => {
            set(first, id, to.clone());
            set(second, id, to);
        }
    }
}

fn set_ratio(node: &mut Node, split: u64, to: f32) {
    if let Node::Split {
        id,
        ratio,
        first,
        second,
        ..
    } = node
    {
        if *id == split {
            *ratio = to;
        } else {
            set_ratio(first, split, to);
            set_ratio(second, split, to);
        }
    }
}

/// Drops a one-pane window whose ticket or session an earlier window
/// already shows: the sidebar lists each once. Windows of several panes
/// stay as the user split them.
pub fn dedupe(windows: &mut Vec<Panes>) {
    let mut seen = std::collections::HashSet::new();
    windows.retain(|w| {
        let keys = w.keys();
        let repeat =
            w.ids().len() == 1 && keys.iter().all(|k| seen.contains(k)) && !keys.is_empty();
        seen.extend(keys);
        !repeat
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_window_in_the_back_with_one_ticket_is_found() {
        let one = Panes::new(Surface::Ticket(TicketView {
            selected: Some("XYZ-794".into()),
            tab: crate::app::Tab::Walkthrough,
            session: None,
            diff_pr: None,
        }));
        assert_eq!(
            one.showing("XYZ-794"),
            None,
            "its only pane is the active one"
        );
        assert_eq!(one.showing_stored("XYZ-794"), Some(one.active));
        assert_eq!(one.showing_stored("XYZ-795"), None);
    }

    #[test]
    fn a_ticket_saved_in_two_windows_comes_back_in_one() {
        let ticket = |id: &str| {
            Panes::new(Surface::Ticket(TicketView {
                selected: Some(id.into()),
                tab: crate::app::Tab::Walkthrough,
                session: None,
                diff_pr: None,
            }))
        };
        let mut windows = vec![
            ticket("a"),
            ticket("b"),
            ticket("a"),
            Panes::new(Surface::Shell),
        ];
        dedupe(&mut windows);
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[2].keys(), Vec::<String>::new(), "a shell stays");
    }

    use super::*;

    #[test]
    fn a_debug_window_keeps_only_its_tools() {
        // Debug opens a shell window, splits in a pane per tool, then drops
        // the window's empty starting pane.
        let mut window = Panes::new(Surface::Shell);
        let first = window.focused;
        let k9s = window.split(true, Surface::Shell);
        window.focused = k9s;
        let claude = window.split(true, Surface::Shell);
        assert!(window.close(first));
        assert_eq!(window.ids(), [k9s, claude]);
    }

    #[test]
    fn a_shell_window_has_no_ticket_pane() {
        assert!(!Panes::new(Surface::Shell).ticket_active());
        let view = TicketView {
            selected: None,
            tab: Tab::Walkthrough,
            session: None,
            diff_pr: None,
        };
        assert!(Panes::new(Surface::Ticket(view)).ticket_active());
    }

    #[test]
    fn a_pane_is_found_by_the_session_it_shows() {
        let view = |session: &str| TicketView {
            selected: None,
            tab: Tab::Claude,
            session: Some(session.into()),
            diff_pr: None,
        };
        let mut panes = Panes::new(Surface::Ticket(view("free:1")));
        let second = panes.split(true, Surface::Ticket(view("free:2")));
        assert_eq!(panes.showing_session("free:2"), Some(second));
        // The active pane's entry is stale (its view is App's): not searched.
        assert_eq!(panes.showing_session("free:1"), None);
        assert_eq!(panes.showing_session("free:3"), None);
    }

    fn ticket(id: &str) -> Surface {
        Surface::Ticket(TicketView {
            selected: Some(id.into()),
            tab: Tab::Walkthrough,
            session: None,
            diff_pr: None,
        })
    }

    #[test]
    fn any_pane_closes_beside_another_and_the_last_one_stays() {
        let mut panes = Panes::default();
        let right = panes.split(true, ticket("ABC-2"));
        panes.focused = right;
        let shell = panes.split(false, Surface::Shell);
        assert_eq!(panes.ids(), vec![0, right, shell]);
        assert_eq!(panes.tickets(), vec![0, right]);
        // Pane 0 is active, so only the other ticket pane is listed.
        assert_eq!(panes.others().len(), 1);
        assert!(panes.close(shell));
        assert!(panes.close(0));
        // One pane left: it stays, the window closes instead.
        assert!(!panes.close(right));
        assert!(panes.single());
        // A session pane beside a shell (the only ticket pane) closes.
        let shell = panes.split(true, Surface::Shell);
        assert!(panes.close(right));
        assert_eq!(panes.ids(), vec![shell]);
        assert!(panes.tickets().is_empty());
    }

    #[test]
    fn a_new_window_has_fresh_ids_and_finds_its_tickets() {
        let first = Panes::default();
        let mut second = Panes::new(ticket("ABC-1"));
        assert_ne!(second.focused, 0);
        let right = second.split(true, ticket("ABC-2"));
        assert!(!first.ids().contains(&right));
        // The active pane is App's to check; the other one is found.
        assert_eq!(second.showing("ABC-2"), Some(right));
        assert_eq!(second.showing("ABC-1"), None);
    }

    #[test]
    fn neighbours_wrap_and_layout_fills_the_rect() {
        let mut panes = Panes::default();
        let right = panes.split(true, ticket("ABC-2"));
        assert_eq!(panes.neighbour(1), right);
        assert_eq!(panes.neighbour(-1), right);

        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1001.0, 800.0));
        let layout = panes.layout(rect);
        let widths: Vec<f32> = layout.panes.iter().map(|(_, _, r)| r.width()).collect();
        assert_eq!(widths, vec![500.0, 500.0]);
        assert_eq!(layout.dividers.len(), 1);

        panes.zoomed = true;
        let zoomed = panes.layout(rect);
        assert_eq!(zoomed.panes.len(), 1);
        assert_eq!(zoomed.panes[0].2, rect);
    }
}
