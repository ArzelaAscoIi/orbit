//! Demo mode (`ORBIT_DEMO=1`): Acme, a made-up analytics company, instead
//! of the user's Linear and GitHub, for the README's GIF and screenshots
//! (scripts/demo-gif.sh). Nothing leaves the app: the backend answers every
//! command from the fixtures in tests/demo/ (`answer`), sessions on this
//! Mac are not listed, no notification is shown, and it is read-only.
//!
//! The board (tests/demo/board.json) is Linear-shaped; a time `@-90` is
//! 90 minutes before now, so "waiting 1h" stays true on any day.

use crate::backend::{Command, Event};
use crate::linear::{Home, Issue, Team};

const BOARD: &str = include_str!("../tests/demo/board.json");
const PROFILE: &str = include_str!("../tests/profiles/acme/profile.json");
const DIFF: &str = include_str!("../tests/demo/web-318.diff");
const GUIDE: &str = include_str!("../tests/demo/web-318.guide");
const TEST_PLAN: &str = include_str!("../tests/demo/api-91.plan");
const CLAUDE_BUILD: &str = include_str!("../tests/demo/claude-build.sh");
const CLONE_CHAT: &str = include_str!("../tests/demo/clone-chat.sh");

/// The two sessions the demo shows: WEB-160 built in its own Claude
/// session, and the clone's chat that drives the saved views initiative.
/// (key, session id, title, identifier, script, working)
/// (key, session id, title, identifier, folder under work/, script, working)
const SESSIONS: [(&str, &str, &str, &str, &str, &str, bool); 3] = [
    (
        "id-web-160",
        "d3m0b1d0-0000-4000-8000-000000000160",
        "Pin a dashboard to the sidebar",
        "WEB-160",
        "WEB-160",
        "claude-build.sh",
        true,
    ),
    (
        "chat:1",
        "d3m0c1a7-0000-4000-8000-000000000001",
        "Saved views on dashboards",
        "Chat",
        "Chat",
        "clone-chat.sh",
        false,
    ),
    // A free session that works in WEB-160's folder: it opens as WEB-160.
    (
        "free:1",
        "d3m0f4ee-0000-4000-8000-000000000001",
        "Unit tests for pinned dashboards",
        "Session",
        "WEB-160",
        "claude-build.sh",
        false,
    ),
];

/// The PR the demo reviews: its diff and walkthrough are in the fixtures.
const REVIEWED: &str = "https://github.com/acme/web/pull/318";

pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ORBIT_DEMO").is_some_and(|v| !v.is_empty() && v != "0"))
}

/// Acme's profile: Robo the bot and its commands, maintenance rules.
pub fn profile() -> crate::profile::Profile {
    let mut profile: crate::profile::Profile = serde_json::from_str(PROFILE).expect("acme profile");
    // The plain word, as a new user sees it.
    profile.ktlo.label = "Maintenance".into();
    // Sam runs the Web board, where Robo builds.
    profile.orchestrator =
        serde_json::from_value(serde_json::json!({"team": "WEB"})).expect("orchestrator");
    profile
}

pub fn home() -> Home {
    let mut board: serde_json::Value = serde_json::from_str(BOARD).expect("demo board");
    relative_times(&mut board, jiff::Timestamp::now());
    let issues: Vec<Issue> = serde_json::from_value(board["issues"].take()).expect("demo issues");
    let teams: Vec<Team> = serde_json::from_value(board["teams"].take()).expect("demo teams");
    Home {
        viewer: "Sam Lee".into(),
        viewer_email: "sam@acme.example".into(),
        org: "Acme".into(),
        my_teams: teams
            .iter()
            .map(|t| (t.key.clone(), t.name.clone()))
            .collect(),
        issues,
        teams,
    }
}

/// Writes what the demo's sessions and initiative need into the data
/// folder before the app reads it: the saved sessions, their hook reports,
/// the replay scripts, the initiative. Nothing outside the data folder.
pub fn seed() {
    if !on() {
        return;
    }
    let dir = crate::store::data_dir();
    let now = jiff::Timestamp::now();
    let secs = now.as_second();
    let ms = now.as_millisecond() as u64;
    let _ = std::fs::create_dir_all(dir.join("status"));
    let _ = std::fs::create_dir_all(dir.join("demo"));
    let mut saved = serde_json::Map::new();
    for (i, (key, id, title, identifier, folder, script, working)) in
        SESSIONS.into_iter().enumerate()
    {
        let text = if script == "claude-build.sh" {
            CLAUDE_BUILD
        } else {
            CLONE_CHAT
        };
        let _ = std::fs::write(dir.join("demo").join(script), text);
        saved.insert(
            key.into(),
            serde_json::json!({
                "agent": "claude",
                "identifier": identifier,
                "title": title,
                "session_id": id,
                "cwd": format!("~/orbit/work/{folder}"),
                "started_at": (now - jiff::SignedDuration::from_mins(20 + i as i64 * 50)).to_string(),
            }),
        );
        let report = if working {
            format!(
                "{}\tworking\t\n{}\tnote\tPinned dashboards work, running the tests\n",
                secs - 40,
                secs - 30
            )
        } else {
            format!("{}\tturn\t\n", secs - 60)
        };
        let _ = std::fs::write(crate::claude::status::path(key), report);
    }
    let _ = std::fs::write(
        dir.join("sessions.json"),
        serde_json::Value::Object(saved).to_string(),
    );
    let mins = |m: u64| ms - m * 60_000;
    let initiative = serde_json::json!([{
        "id": "i1",
        "goal": "Ship saved views for dashboards: per-user saved filters, shareable links, default view. Robo builds, I review.",
        "parent": "WEB-170",
        "parallel": 2,
        "session": "chat:1",
        "created_ms": mins(72),
        "log": [
            [mins(70), "Filed WEB-170 with 4 sub-tickets"],
            [mins(66), "WEB-171 to Robo"],
            [mins(41), "WEB-172 to Robo"],
            [mins(25), "WEB-171: PR web#322 is up for your review"],
            [mins(12), "WEB-172: Robo is building"],
        ],
    }]);
    let _ = std::fs::write(dir.join("missions.json"), initiative.to_string());
}

/// The demo sessions as `claude agents` would list them: running, WEB-160
/// at work, the clone's chat waiting on the user.
pub fn rows() -> Vec<crate::claude::Live> {
    SESSIONS
        .iter()
        .filter_map(|(_, id, title, _, folder, _, working)| {
            serde_json::from_value(serde_json::json!({
                "sessionId": id,
                "id": &id[..8],
                "name": title,
                "cwd": format!("~/orbit/work/{folder}"),
                "kind": "background",
                "status": if *working { "busy" } else { "idle" },
                "state": if *working { "working" } else { "idle" },
            }))
            .ok()
        })
        .collect()
}

/// The script a demo session plays in place of `claude attach`.
pub fn replay(key: &str) -> Option<std::path::PathBuf> {
    let (.., script, _) = SESSIONS.iter().find(|s| s.0 == key).filter(|_| on())?;
    // Absolute: the terminal runs it from its own folder.
    std::fs::canonicalize(crate::store::data_dir().join("demo").join(script)).ok()
}

/// `@-90` becomes the timestamp 90 minutes before `now`.
fn relative_times(value: &mut serde_json::Value, now: jiff::Timestamp) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(minutes) = s.strip_prefix("@-").and_then(|m| m.parse::<i64>().ok()) {
                *s = (now - jiff::SignedDuration::from_mins(minutes)).to_string();
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| relative_times(v, now)),
        serde_json::Value::Object(map) => map.values_mut().for_each(|v| relative_times(v, now)),
        _ => {}
    }
}

/// What the backend answers in demo mode: a pure function of the command,
/// never the network. `None`: nothing comes back, as for a write.
pub fn answer(command: Command) -> Option<Event> {
    Some(match command {
        Command::Refresh { request, .. } => Event::Loaded {
            request,
            result: Ok(home()),
        },
        Command::Brief { fingerprint, .. } => Event::Brief {
            fingerprint,
            result: Ok("WEB-142: review web#318, the CSV export fix".into()),
        },
        Command::Shorten { items, .. } => Event::Shortened {
            result: Ok(items
                .iter()
                .map(|(k, text)| (k.clone(), text.clone()))
                .collect()),
            items,
        },
        Command::Customers { items, .. } => Event::Customers {
            result: Ok(Vec::new()),
            items,
        },
        Command::Digests { items, .. } => Event::Digests {
            result: Ok(Vec::new()),
            items,
        },
        Command::Diff { url, .. } => Event::Diff {
            result: Ok(if url == REVIEWED {
                crate::github::parse(DIFF)
            } else {
                Vec::new()
            }),
            url,
        },
        Command::Gate { url } => Event::Gate {
            result: Ok(gate(&url)),
            url,
        },
        Command::Talk { url } => Event::Talk {
            url,
            result: Ok(Default::default()),
        },
        Command::Guide { url, .. } if url == REVIEWED => Event::Guided {
            url,
            result: Ok(crate::anthropic::parse_guide(GUIDE)),
        },
        Command::TestPlan { issue_id, .. } => Event::TestPlan {
            result: crate::testplan::from_reply(TEST_PLAN)
                .map(|plan| (plan, false))
                .ok_or_else(|| "no plan".to_owned()),
            issue_id,
        },
        Command::Inbox => Event::Inbox {
            linear: Ok((Vec::new(), 0)),
            github: Ok((Vec::new(), 0)),
        },
        Command::MyPrs => Event::MyPrs(Ok(Vec::new())),
        Command::DependabotPrs(_) => Event::DependabotPrs(Ok(Vec::new())),
        Command::Herdr => Event::Herdr(Ok(Vec::new())),
        Command::Releases(_) => Event::Releases(Vec::new()),
        Command::Incidents { .. } => Event::Incidents(Ok(Vec::new())),
        Command::Alerts { .. } => Event::Alerts(Ok(Vec::new())),
        Command::CheckUpdate => Event::UpdateFound(Ok(None)),
        // The initiative's parent, with its sub-tickets.
        Command::Lookup { id } => Event::Looked {
            result: home()
                .issues
                .into_iter()
                .find(|i| i.identifier == id || i.id == id)
                .map(Box::new)
                .ok_or_else(|| format!("no {id} in the demo")),
            id,
        },
        Command::Search { term } => Event::Found {
            term,
            result: Ok(Vec::new()),
        },
        _ => return None,
    })
}

/// GitHub's answer for the demo's PRs: Robo's export fix waits on the
/// user's review with green checks; the rest are merged or someone else's.
fn gate(url: &str) -> crate::github::Gate {
    let number = url.rsplit('/').next().unwrap_or_default();
    let mut gate = crate::github::Gate {
        head: format!("8b9d0e7{number}"),
        state: "OPEN".into(),
        review: "REVIEW_REQUIRED".into(),
        checks: "SUCCESS".into(),
        branch: "CLEAN".into(),
        why: "a review is missing".into(),
        base_branch: "main".into(),
        ..Default::default()
    };
    match url {
        u if u == REVIEWED || u.ends_with("/322") => {
            gate.can_approve = true;
            gate.author = "acme-bot[bot]".into();
            gate.author_bot = true;
            gate.head_branch = format!("robo/{number}");
            // The second bot PR also has review threads and a red check:
            // Approve leads, Fix beside it.
            if u.ends_with("/322") {
                gate.open_threads = 2;
                gate.checks = "FAILURE".into();
                gate.why = "a check failed".into();
            }
        }
        // A blocked ticket with two of the user's PRs: one approved with a
        // red check (Fix fills its row, Land it beside it), one waiting for
        // a review (Request AI review / Request review).
        u if u.ends_with("/330") => {
            gate.author = "sam-lee".into();
            gate.own = true;
            gate.review = "APPROVED".into();
            gate.checks = "FAILURE".into();
            gate.why = "a check failed".into();
            gate.head_branch = "sam/widget-pin".into();
        }
        u if u.ends_with("/331") => {
            gate.author = "sam-lee".into();
            gate.own = true;
            gate.head_branch = "sam/widget-dependency".into();
        }
        u if u.ends_with("/211") => {
            // The user's own draft: Mark ready leads.
            gate.author = "sam-lee".into();
            gate.own = true;
            gate.draft = true;
            gate.head_branch = "sam/reports-index".into();
            gate.on_it = vec!["@jo requested".into()];
        }
        _ => {
            gate.state = "MERGED".into();
            gate.review = "APPROVED".into();
            gate.branch = "CLEAN".into();
            gate.author = "acme-bot[bot]".into();
            gate.author_bot = true;
            gate.on_it = vec!["@alex-kim approved".into()];
        }
    }
    gate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::{Ask, Stage};
    use std::collections::HashSet;

    #[test]
    fn the_demo_board_tells_the_story_it_should() {
        crate::story::set_bot("Robo");
        let cards: Vec<_> = home()
            .issues
            .into_iter()
            .map(|i| crate::story::card(i, &HashSet::new()))
            .collect();
        let of = |id: &str| cards.iter().find(|c| c.issue.identifier == id).unwrap();
        assert_eq!(of("WEB-142").stage, Stage::Review);
        assert_eq!(of("WEB-142").ask, Some(Ask::Review));
        assert_eq!(of("API-88").ask, Some(Ask::Decide));
        assert_eq!(of("API-91").ask, Some(Ask::Verify));
        assert_eq!(of("WEB-150").stage, Stage::Building);
        assert_eq!(of("API-95").stage, Stage::Planning);
        assert!(of("API-95").plan.is_some());
        assert_eq!(of("WEB-158").stage, Stage::Intake);
        assert_eq!(of("WEB-140").stage, Stage::Done);
        // Times are relative to now: nothing reads as weeks old.
        assert!(of("WEB-142").issue.updated_at.starts_with("20"));
    }

    #[test]
    fn demo_mode_answers_from_fixtures_never_the_network() {
        // A pure function: these come back at once, without a runtime or a key.
        let Some(Event::Diff {
            result: Ok(files), ..
        }) = answer(Command::Diff {
            url: REVIEWED.into(),
            owner: "acme".into(),
            repo: "web".into(),
            number: 318,
            token: None,
        })
        else {
            panic!("the demo PR has a diff");
        };
        assert_eq!(files.len(), 3);
        let Some(Event::Guided {
            result: Ok(guide), ..
        }) = answer(Command::Guide {
            url: REVIEWED.into(),
            key: String::new(),
            context: String::new(),
            diff: String::new(),
            since: None,
        })
        else {
            panic!("the demo PR has a walkthrough");
        };
        assert_eq!(guide.steps.len(), 3);
        let Some(Event::Gate {
            result: Ok(gate), ..
        }) = answer(Command::Gate {
            url: REVIEWED.into(),
        })
        else {
            panic!("the demo PR has a gate");
        };
        assert!(gate.can_approve && !gate.green);
        // A write gets no answer: nothing to send it to.
        assert!(
            answer(Command::SetState {
                issue_id: "x".into(),
                state_id: "y".into(),
            })
            .is_none()
        );
        assert_eq!(profile().bot.plan_command, "@Robo Plan!");
        // The initiative's parent comes back with its four sub-tickets.
        let Some(Event::Looked {
            result: Ok(parent), ..
        }) = answer(Command::Lookup {
            id: "WEB-170".into(),
        })
        else {
            panic!("the initiative's parent is on the demo board");
        };
        let kids = crate::mission::kids(&parent.children.nodes);
        assert_eq!(kids.len(), 4);
        assert_eq!(kids[2].blocked_by, ["WEB-171"]);
    }

    #[test]
    fn the_demo_sessions_run_as_claude_agents_would_list_them() {
        let rows = rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].label(), "working");
        assert!(
            rows.iter()
                .all(|r| r.running() && r.cwd.starts_with("~/orbit/work/"))
        );
        // Outside demo mode nothing is replayed: `claude attach` as always.
        assert_eq!(replay("id-web-160"), None);
    }
}
