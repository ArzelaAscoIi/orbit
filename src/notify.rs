//! Mac notifications when something needs the user: a board ticket that
//! newly asks for them (review, decide, verify) and a new Notifications
//! row that asks them to act, a product ticket newly in Triage, an
//! incident.io alert that starts firing, a release run that failed, and a
//! Claude session that waits on them (your turn, needs approval), as
//! herdr says when an agent finishes or blocks (`actionable`: never
//! activity on their own PRs or a GitHub mention). One short line each,
//! only while Orbit is not the focused window.
//!
//! A line is said only once it was open on two checks in a row (`stable`):
//! a state that flickers for a moment (a turn that ends and a new prompt
//! that follows at once) never reaches the Mac, as herdr holds a toast and
//! drops it when the state moved on.
//!
//! `notified.json` keeps what was already said, shared by every copy of the
//! app (read-modify-write), so a restart or a second copy does not say it
//! again. The first time there is no file, everything open counts as said.

use std::collections::BTreeMap;

use crate::story::{Ask, Card, Lane};

const FILE: &str = "notified.json";

/// What needs the user now, as `key -> line`: a key changes when the ask
/// does, so a ticket that moves from decide to review is said again.
pub fn open(
    cards: &[&Card],
    inbox: &[crate::inbox::Item],
    alerts: &[crate::incidents::Alert],
    releases: &[(&str, &crate::releases::Run)],
    sessions: &[(String, String, String)],
    short: impl Fn(&Card) -> String,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    // A session waiting on the user: a turn they have not looked at, or a
    // permission prompt. Looking at it (the pane in front) takes it back.
    for (key, name, state) in sessions {
        let verb = match state.as_str() {
            // A terminal session Orbit did not start: the user is in it.
            "your turn" if key.starts_with("sid:") => continue,
            "your turn" => "Your turn",
            "needs approval" => "Needs approval",
            _ => continue,
        };
        out.insert(
            format!("session:{key}:{state}"),
            line(&format!("{verb}: {name}")),
        );
    }
    // A failed release run, once per run.
    for (name, run) in releases {
        out.insert(
            format!("release:{}", run.id),
            line(&match run.by() {
                Some(by) => format!("Deploy failed: {name} · {by} · {}", run.title),
                None => format!("Deploy failed: {name} · {}", run.title),
            }),
        );
    }
    // A firing alert, once: said again only if it resolves and fires anew.
    for alert in alerts.iter().filter(|a| a.firing()) {
        out.insert(
            format!("alert:{}", alert.id),
            line(&format!("Alert: {}", alert.title)),
        );
    }
    for card in cards.iter().filter(|c| c.lane == Lane::Product) {
        let Some(ask) = card.ask else {
            continue;
        };
        let verb = match ask {
            Ask::Review => "Review",
            Ask::Decide => "Decide",
            Ask::Verify => "Verify",
        };
        out.insert(
            format!("card:{}:{verb}", card.issue.id),
            line(&format!(
                "{verb} {}: {}",
                card.issue.identifier,
                short(card)
            )),
        );
    }
    // A product ticket newly filed in Triage: nobody has looked at it yet.
    for card in cards
        .iter()
        .filter(|c| c.lane == Lane::Product && c.untriaged())
    {
        out.insert(
            format!("triage:{}", card.issue.id),
            line(&format!(
                "New in Triage: {} {}",
                card.issue.identifier,
                short(card)
            )),
        );
    }
    // KTLO nobody handles, urgent or high: the one KTLO line on the Mac.
    let now = jiff::Timestamp::now();
    for card in cards
        .iter()
        .filter(|c| crate::story::unhandled(c, now) && crate::story::pressing(c))
    {
        out.insert(
            format!("ktlo:{}", card.issue.id),
            line(&format!(
                "Nobody on it: {} {}",
                card.issue.identifier,
                short(card)
            )),
        );
    }
    // A row about a ticket the board already asks about is said once, there.
    let asked = |id: &String| cards.iter().any(|c| &c.issue.id == id && c.ask.is_some());
    let rows = inbox
        .iter()
        .filter(|i| actionable(i) && !i.issue.as_ref().is_some_and(asked));
    for item in rows {
        let verb = item.ask.verb();
        let mut verb = verb.chars();
        let verb: String = verb
            .next()
            .map(|c| c.to_uppercase().chain(verb).collect())
            .unwrap_or_default();
        out.insert(
            format!("inbox:{}", item.key),
            line(&format!("{verb}: {}", item.title)),
        );
    }
    out
}

/// A row that asks the user to act: Linear's review requests, mentions,
/// assignments and SLA alerts; from GitHub only an open review request or
/// an assignment. Activity on their own PRs and GitHub mentions stay in the
/// list, never on the Mac.
fn actionable(item: &crate::inbox::Item) -> bool {
    use crate::inbox::{Ask, Source};
    item.notes.iter().any(|n| match n.source {
        Source::Linear => n.ask != Ask::YourPr,
        Source::GitHub => matches!(n.ask, Ask::Review | Ask::Assigned),
    })
}

/// One line, short: a notification is read at a glance.
fn line(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(72) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

/// The lines not said yet, and what to keep as said: exactly what is open,
/// so an ask that goes away and comes back is said again. `None` said
/// means the first run: nothing is new.
pub fn fresh(
    open: &BTreeMap<String, String>,
    said: Option<Vec<String>>,
) -> (Vec<(String, String)>, Vec<String>) {
    let keep = open.keys().cloned().collect();
    let Some(said) = said else {
        return (Vec::new(), keep);
    };
    let new = open
        .iter()
        .filter(|(k, _)| !said.contains(k))
        .map(|(k, line)| (k.clone(), line.clone()))
        .collect();
    (new, keep)
}

/// Which sound a check's new lines call for: needs approval always (a
/// prompt holds the work), a turn that ended only while Orbit is not in
/// front (in front, the user sees it). `None`: nothing to sound.
pub fn sound(new_keys: &[&str], front: bool) -> Option<Sound> {
    let session = |state: &str| {
        new_keys
            .iter()
            .any(|k| k.starts_with("session:") && k.ends_with(state))
    };
    if session(":needs approval") {
        Some(Sound::Request)
    } else if session(":your turn") && !front {
        Some(Sound::Done)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Done,
    Request,
}

/// Plays the sound, off the UI thread: the user's file from settings (as
/// herdr's config had it), else one of macOS's own.
pub fn play(sound: Sound, settings: &crate::store::Settings) {
    let (file, default) = match sound {
        Sound::Done => (&settings.done_sound, "/System/Library/Sounds/Glass.aiff"),
        Sound::Request => (&settings.request_sound, "/System/Library/Sounds/Ping.aiff"),
    };
    let path = if file.trim().is_empty() {
        default.to_owned()
    } else {
        file.clone()
    };
    std::thread::spawn(move || {
        let _ = std::process::Command::new("afplay").arg(path).output();
    });
}

/// Where a click on a notification's line goes, from its key: the ticket,
/// the Notifications row, the session, the Alerts view.
pub fn action_for(key: &str) -> Option<crate::app::Action> {
    use crate::app::{Action, View};
    let (kind, rest) = key.split_once(':')?;
    Some(match kind {
        "card" | "triage" | "ktlo" => {
            Action::Focus(rest.rsplit_once(':').map_or(rest, |(id, _)| id).to_owned())
        }
        "inbox" => Action::InboxOpen(rest.to_owned()),
        "session" => {
            let key = rest.rsplit_once(':').map_or(rest, |(k, _)| k);
            match key.strip_prefix("herdr:") {
                Some(target) => Action::HerdrFocus(target.to_owned()),
                None => Action::OpenSession(key.to_owned()),
            }
        }
        "alert" | "release" => Action::Show(View::Alerts),
        // An initiative's sub-ticket: opened here, fetched when off the board.
        "linear" if !rest.is_empty() => Action::Link(rest.to_owned()),
        _ => return None,
    })
}

/// What was open on this check and the one before: a line is said only
/// once it held for two checks. What is open only now waits for the next.
pub fn stable(
    open: &BTreeMap<String, String>,
    previous: Option<&[String]>,
) -> BTreeMap<String, String> {
    let Some(previous) = previous else {
        return BTreeMap::new();
    };
    open.iter()
        .filter(|(k, _)| previous.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Re-reads what was said (another copy may have said it), writes what is
/// open now, and returns the lines to say, with their keys.
pub fn take(open: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let path = crate::store::data_dir().join(FILE);
    let said = path
        .exists()
        .then(|| crate::store::load_at::<Vec<String>>(&path));
    let (new, keep) = fresh(open, said);
    let _ = crate::store::save_at(&path, &keep);
    new
}

/// Shows one Mac notification, off the UI thread: Orbit's own when it runs
/// from an app bundle (`make run` and `make bundle` both do), so a click
/// brings Orbit forward; else through osascript, whose click opens Script
/// Editor. The line goes in as an argument, never into the script, so no
/// text can break out of it.
pub fn send(text: String) {
    if crate::demo::on() {
        return;
    }
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        if native::send(crate::NAME, &text) {
            return;
        }
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                &format!(
                    "display notification (item 1 of argv) with title \"{}\"",
                    crate::NAME
                ),
                "-e",
                "end run",
                &text,
            ])
            .output();
    });
}

/// UserNotifications, which needs a bundle id: false outside an app bundle.
#[cfg(target_os = "macos")]
mod native {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use objc2_foundation::{NSBundle, NSString};

    #[link(name = "UserNotifications", kind = "framework")]
    unsafe extern "C" {}

    pub fn send(title: &str, body: &str) -> bool {
        // Without a bundle id the center raises instead of answering.
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return false;
        }
        unsafe {
            let center: Retained<AnyObject> =
                msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
            // Asks once (alert, sound); macOS remembers the answer.
            static ASK: std::sync::Once = std::sync::Once::new();
            ASK.call_once(|| {
                let done = RcBlock::new(|_: Bool, _: *mut AnyObject| {});
                let _: () = msg_send![&center, requestAuthorizationWithOptions: 6usize, completionHandler: &*done];
            });
            let content: Retained<AnyObject> = msg_send![class!(UNMutableNotificationContent), new];
            let _: () = msg_send![&content, setTitle: &*NSString::from_str(title)];
            let _: () = msg_send![&content, setBody: &*NSString::from_str(body)];
            let id = NSString::from_str(&format!("{:x}", crate::store::text_hash(body)));
            let request: Retained<AnyObject> = msg_send![
                class!(UNNotificationRequest),
                requestWithIdentifier: &*id,
                content: &*content,
                trigger: std::ptr::null::<AnyObject>()
            ];
            let _: () = msg_send![
                &center,
                addNotificationRequest: &*request,
                withCompletionHandler: std::ptr::null::<AnyObject>()
            ];
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_deploy_is_said_with_who_broke_it() {
        let run = crate::releases::Run {
            id: 9,
            title: "Deploy api".into(),
            status: "completed".into(),
            conclusion: "failure".into(),
            responsible: Some("ana".into()),
            pr: Some(7),
            ..Default::default()
        };
        let open = open(&[], &[], &[], &[("api prod", &run)], &[], |_| String::new());
        assert_eq!(open.keys().collect::<Vec<_>>(), ["release:9"]);
        assert_eq!(
            open.values().next().map(String::as_str),
            Some("Deploy failed: api prod · @ana (#7) · Deploy api")
        );
    }

    #[test]
    fn ktlo_nobody_on_is_said_only_when_urgent_or_high() {
        use crate::story::tests::issue;
        // KTLO in Triage, no owner, filed days ago: high says it, medium not.
        let ktlo = |priority: f64| {
            let mut issue = issue("Triage", "triage", vec![]);
            issue.created_at = "2026-09-24T03:08:46Z".into();
            issue.priority = priority;
            let mut card = crate::story::card(issue, &Default::default());
            card.lane = Lane::Alerts;
            card
        };
        let (high, medium) = (ktlo(2.0), ktlo(3.0));
        let said = open(&[&high], &[], &[], &[], &[], |_| "x".into());
        assert_eq!(said.values().collect::<Vec<_>>(), ["Nobody on it: DEV-1 x"]);
        assert!(open(&[&medium], &[], &[], &[], &[], |_| "x".into()).is_empty());
    }

    #[test]
    fn a_ticket_that_asks_for_review_is_one_short_line_and_ktlo_says_nothing() {
        use crate::story::tests::issue;
        let mut card =
            crate::story::card(issue("In Review", "started", vec![]), &Default::default());
        card.lane = Lane::Product;
        card.ask = Some(Ask::Review);
        let said = open(&[&card], &[], &[], &[], &[], |_| "fix login".into());
        assert_eq!(
            said.get("card:i:Review").map(String::as_str),
            Some("Review DEV-1: fix login")
        );
        // One very short line, however long the title.
        let long = open(&[&card], &[], &[], &[], &[], |_| "word ".repeat(40));
        assert!(long["card:i:Review"].chars().count() <= 73);
        // KTLO that is not pressing and unhandled says nothing.
        let mut ktlo = card.clone();
        ktlo.lane = Lane::Security;
        assert!(open(&[&ktlo], &[], &[], &[], &[], |_| "x".into()).is_empty());
    }

    #[test]
    fn only_rows_that_ask_to_act_notify() {
        use crate::inbox::{Ask, Item, Note, Source};
        let row = |source, ask| Item {
            key: "k".into(),
            ask,
            title: "t".into(),
            why: String::new(),
            url: String::new(),
            pr: None,
            issue: None,
            at: String::new(),
            notes: vec![Note {
                source,
                id: "n".into(),
                ask,
                title: "t".into(),
                why: String::new(),
                url: String::new(),
                pr: None,
                issue: None,
                at: String::new(),
            }],
        };
        assert!(actionable(&row(Source::GitHub, Ask::Review)));
        assert!(actionable(&row(Source::GitHub, Ask::Assigned)));
        assert!(!actionable(&row(Source::GitHub, Ask::YourPr)));
        assert!(!actionable(&row(Source::GitHub, Ask::Reply)));
        assert!(actionable(&row(Source::Linear, Ask::Reply)));
        assert!(actionable(&row(Source::Linear, Ask::Sla)));
        let open = open(
            &[],
            &[row(Source::GitHub, Ask::YourPr)],
            &[],
            &[],
            &[],
            |_| String::new(),
        );
        assert!(open.is_empty());
    }

    #[test]
    fn a_ticket_new_in_triage_is_said() {
        use crate::story::tests::issue;
        let card = crate::story::card(issue("Triage", "triage", vec![]), &Default::default());
        let open = open(&[&card], &[], &[], &[], &[], |_| "x".into());
        assert_eq!(
            open.values().next().map(String::as_str),
            Some("New in Triage: DEV-1 x")
        );
    }

    #[test]
    fn a_firing_alert_is_said_a_resolved_one_not() {
        use crate::incidents::Alert;
        let alert = |id: &str, status: &str| Alert {
            id: id.into(),
            title: format!("{id} down"),
            status: status.into(),
            ..Default::default()
        };
        let open = open(
            &[],
            &[],
            &[alert("a", "firing"), alert("b", "resolved")],
            &[],
            &[],
            |_| String::new(),
        );
        assert_eq!(open.values().collect::<Vec<_>>(), ["Alert: a down"]);
    }

    #[test]
    fn a_session_waiting_on_the_user_is_said_a_working_one_not() {
        let sessions = [
            (
                "DEV-1".to_owned(),
                "DEV-1 fix login".to_owned(),
                "your turn".to_owned(),
            ),
            (
                "DEV-2".to_owned(),
                "DEV-2 tests".to_owned(),
                "needs approval".to_owned(),
            ),
            (
                "DEV-3".to_owned(),
                "DEV-3 docs".to_owned(),
                "working".to_owned(),
            ),
            (
                "DEV-4".to_owned(),
                "DEV-4 seen".to_owned(),
                "idle".to_owned(),
            ),
        ];
        let said = open(&[], &[], &[], &[], &sessions, |_| String::new());
        assert_eq!(
            said.values().collect::<Vec<_>>(),
            ["Your turn: DEV-1 fix login", "Needs approval: DEV-2 tests"]
        );
        assert!(said.contains_key("session:DEV-1:your turn"));
        // A turn ended in a terminal Orbit did not start is not said; a prompt is.
        let outside = [
            ("sid:a".to_owned(), "a".to_owned(), "your turn".to_owned()),
            (
                "sid:b".to_owned(),
                "b".to_owned(),
                "needs approval".to_owned(),
            ),
        ];
        let said = open(&[], &[], &[], &[], &outside, |_| String::new());
        assert_eq!(said.values().collect::<Vec<_>>(), ["Needs approval: b"]);
    }

    #[test]
    fn a_prompt_always_sounds_and_a_finished_turn_only_out_of_sight() {
        assert_eq!(
            sound(&["session:DEV-1:needs approval"], true),
            Some(Sound::Request)
        );
        assert_eq!(
            sound(&["session:DEV-1:your turn"], false),
            Some(Sound::Done)
        );
        assert_eq!(sound(&["session:DEV-1:your turn"], true), None);
        assert_eq!(
            sound(&["card:x:Review"], false),
            None,
            "only sessions sound"
        );
        assert!(matches!(
            action_for("session:herdr:reviewer:needs approval"),
            Some(crate::app::Action::HerdrFocus(t)) if t == "reviewer"
        ));
    }

    #[test]
    fn a_toast_click_goes_where_its_key_points() {
        use crate::app::{Action, View};
        assert!(matches!(action_for("card:abc:Review"), Some(Action::Focus(id)) if id == "abc"));
        assert!(matches!(action_for("triage:abc"), Some(Action::Focus(id)) if id == "abc"));
        assert!(matches!(action_for("inbox:k:1"), Some(Action::InboxOpen(k)) if k == "k:1"));
        assert!(
            matches!(action_for("session:sid:x:your turn"), Some(Action::OpenSession(k)) if k == "sid:x")
        );
        assert!(matches!(
            action_for("alert:7"),
            Some(Action::Show(View::Alerts))
        ));
        assert!(action_for("nonsense").is_none());
    }

    #[test]
    fn a_line_is_said_only_once_it_held_for_two_checks() {
        let open = |keys: &[&str]| -> BTreeMap<String, String> {
            keys.iter()
                .map(|k| (k.to_string(), format!("line {k}")))
                .collect()
        };
        // The first check: nothing is stable yet.
        assert!(stable(&open(&["a"]), None).is_empty());
        let previous = vec!["a".to_owned()];
        // Held: said. New just now: waits. Gone: never said.
        let held = stable(&open(&["a", "b"]), Some(&previous));
        assert_eq!(held.keys().collect::<Vec<_>>(), ["a"]);
        assert!(stable(&open(&["c"]), Some(&previous)).is_empty());
    }

    #[test]
    fn only_what_is_new_is_said_and_the_first_run_says_nothing() {
        let open = |keys: &[&str]| -> BTreeMap<String, String> {
            keys.iter()
                .map(|k| (k.to_string(), format!("line {k}")))
                .collect()
        };
        let (new, keep) = fresh(&open(&["a", "b"]), None);
        assert!(new.is_empty());
        assert_eq!(keep, ["a", "b"]);
        let (new, keep) = fresh(&open(&["b", "c"]), Some(keep));
        assert_eq!(new, [("c".to_owned(), "line c".to_owned())]);
        assert_eq!(keep, ["b", "c"]);
        // Gone and back: said again.
        let (new, _) = fresh(&open(&["a", "b", "c"]), Some(keep));
        assert_eq!(new, [("a".to_owned(), "line a".to_owned())]);
        assert_eq!(line(&"x ".repeat(100)).chars().count(), 73);
    }
}
