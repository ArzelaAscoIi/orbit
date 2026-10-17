//! Initiatives: a goal the clone pursues over days, in the user's words: a
//! new feature, a fix someone reported in Slack, an existing parent ticket.
//! A mission session files the tickets (skills/mission); then Orbit drives
//! the parent's sub-tickets through the team's bot. Every two minutes the
//! parent is read again (`Command::Lookup`): `next` says which sub-tickets
//! go to the bot's team in its go state (e.g. Todo), at most
//! `parallel` in flight, each once every sibling that blocks it is merged;
//! `changes` says what moved since the last look: every move goes in the
//! log (`orbit ctl initiatives`), and the user hears once of what is theirs
//! (a PR to review, the bot's block, a merge). A sub-ticket with the bot
//! and no move for `STALL` is said once too, with the bot's review cap
//! when that is what holds it.
//! Driving counts as the user's click for the bot's commands on its own
//! sub-tickets (`kick`): one quiet for `KICK_MS` gets the profile's exact
//! Plan or Build comment, once per state, and the user hears it.
//! Waiting costs nothing: no session sits there. A sub-ticket with its PR
//! open (In Review) counts as implemented: the review is the user's.
//! initiatives.json is read-modify-write (several copies may run).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const FILE: &str = "initiatives.json";
/// Log lines kept per initiative.
const LOG: usize = 80;
/// With the bot this long without a move: the user hears it.
pub const STALL_MS: u64 = 60 * 60 * 1000;
/// Quiet this long where the bot waits for a word: the drive kicks it.
pub const KICK_MS: u64 = 30 * 60 * 1000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Initiative {
    /// `i<ms>`.
    pub id: String,
    /// What the user wants, in their words.
    pub goal: String,
    /// The parent ticket's identifier, `ABC-769`; empty while it is filed.
    pub parent: String,
    /// Sub-tickets the bot may work on at once.
    pub parallel: usize,
    /// The mission session that files its tickets, if one does.
    pub session: String,
    pub created_ms: u64,
    /// Each sub-ticket's state at the last look.
    pub seen: BTreeMap<String, String>,
    /// When each sub-ticket last moved (unix millis).
    pub moved: BTreeMap<String, u64>,
    /// Sub-tickets already said to be stalled, until they move again.
    pub stalled: std::collections::BTreeSet<String>,
    /// Sub-tickets the drive kicked, with the state it kicked them in
    /// (`Look::key`): once per state.
    pub kicked: BTreeMap<String, String>,
    /// Sub-tickets the bot never answered a kick on, now built in an Orbit
    /// session (`fall_back`): once each.
    pub fallen_back: std::collections::BTreeSet<String>,
    /// What happened, oldest first: (unix millis, line).
    pub log: Vec<(u64, String)>,
    /// Every sub-ticket implemented (or stopped by the user).
    pub done: bool,
    /// Built here in Orbit sessions the clone runs, not by the bot: the
    /// drive hands nothing over and kicks nothing; it only follows.
    pub local: bool,
}

/// A sub-ticket as the driver needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kid {
    pub identifier: String,
    pub team: String,
    pub state: String,
    /// Linear's state type: triage, backlog, unstarted, started, completed, canceled.
    pub kind: String,
    /// Siblings that block it (Linear's `blocks` relations).
    pub blocked_by: Vec<String>,
}

impl Kid {
    pub fn merged(&self) -> bool {
        matches!(self.kind.as_str(), "completed" | "canceled") || self.state == "Testing"
    }
    /// Its PR is open or in: the user's review from here.
    pub fn implemented(&self) -> bool {
        self.merged() || self.state == "In Review"
    }
    pub fn in_flight(&self, bot: &str) -> bool {
        self.team == bot
            && !self.implemented()
            && matches!(self.kind.as_str(), "unstarted" | "started")
    }
    fn waiting(&self, bot: &str) -> bool {
        !self.implemented() && !self.in_flight(bot)
    }
}

/// The sub-tickets to hand the bot now, in the parent's order.
pub fn next(kids: &[Kid], bot: &str, parallel: usize) -> Vec<String> {
    let flying = kids.iter().filter(|k| k.in_flight(bot)).count();
    let merged = |id: &String| kids.iter().any(|k| &k.identifier == id && k.merged());
    kids.iter()
        .filter(|k| k.waiting(bot) && k.blocked_by.iter().all(merged))
        .take(parallel.max(1).saturating_sub(flying))
        .map(|k| k.identifier.clone())
        .collect()
}

/// Every sub-ticket implemented: the mission is done.
pub fn finished(kids: &[Kid]) -> bool {
    !kids.is_empty() && kids.iter().all(Kid::implemented)
}

/// The parent's sub-tickets as kids in the order they were filed (the
/// mission files them in build order), blockers read from the siblings.
pub fn kids(children: &[crate::linear::Child]) -> Vec<Kid> {
    let mut filed: Vec<&crate::linear::Child> = children.iter().collect();
    filed.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    filed
        .into_iter()
        .map(|c| Kid {
            identifier: c.identifier.clone(),
            team: c.team.as_ref().map(|t| t.key.clone()).unwrap_or_default(),
            state: c.state.name.clone(),
            kind: c.state.kind.clone(),
            blocked_by: children
                .iter()
                .filter(|o| {
                    o.relations
                        .nodes
                        .iter()
                        .any(|r| r.kind == "blocks" && r.related_issue.identifier == c.identifier)
                })
                .map(|o| o.identifier.clone())
                .collect(),
        })
        .collect()
}

/// What changed for one sub-ticket since the last look, said once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Its PR is open: the user's review.
    Review,
    /// The bot blocked it: the user's call.
    Blocked,
    /// Merged: the user checks it works.
    Merged,
    /// Any other move, from this state: logged, not said.
    Moved(String),
}

/// The changes since `seen`, in the parent's order; `seen` updated.
pub fn changes(kids: &[Kid], seen: &mut BTreeMap<String, String>) -> Vec<(String, Change)> {
    let mut out = Vec::new();
    for kid in kids {
        let before = seen.insert(kid.identifier.clone(), kid.state.clone());
        let Some(before) = before.filter(|b| b != &kid.state) else {
            continue;
        };
        let change = match kid.state.as_str() {
            "In Review" => Change::Review,
            "Blocked" => Change::Blocked,
            "Testing" | "Done" if kid.kind != "canceled" => Change::Merged,
            _ => Change::Moved(before),
        };
        out.push((kid.identifier.clone(), change));
    }
    out
}

/// Why a sub-ticket that is not implemented is where it is, for the log
/// and `orbit ctl initiatives`.
pub fn why(kid: &Kid, kids: &[Kid], bot: &str, parallel: usize) -> String {
    let open: Vec<&str> = kid
        .blocked_by
        .iter()
        .filter(|b| !kids.iter().any(|k| &&k.identifier == b && k.merged()))
        .map(String::as_str)
        .collect();
    if kid.implemented() {
        "implemented".into()
    } else if kid.in_flight(bot) {
        format!("with the bot ({})", kid.state)
    } else if kid.state == "Blocked" {
        "blocked by the bot: your call".into()
    } else if !open.is_empty() {
        format!("waits for {} to merge", open.join(", "))
    } else if kids.iter().filter(|k| k.in_flight(bot)).count() >= parallel.max(1) {
        format!("next: {parallel} already with the bot")
    } else {
        "next".into()
    }
}

/// What Orbit knows of a sub-ticket's work beyond Linear: its session,
/// the PR it opened, that PR's gate. For `phase`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Its session is working (or starting) now.
    pub working: bool,
    /// It has a session at all.
    pub session: bool,
    pub pr: bool,
    /// Unresolved review threads on the PR.
    pub threads: usize,
    /// The PR's checks are red.
    pub red: bool,
    /// The PR is in the merge queue.
    pub queued: bool,
    /// With the bot and nothing on it for `KICK_MS`: no work shows.
    pub quiet: bool,
    /// The bot holds new builds at its review cap: reviews free it.
    pub capped: bool,
}

/// The sub-ticket's step in one or two words, for the initiative's list:
/// done, merging, fixing comments, your review, building, waiting on ...
pub fn phase(kid: &Kid, kids: &[Kid], bot: &str, parallel: usize, work: &Work) -> String {
    let fixing = |what: &str| {
        if work.working {
            format!("fixing {what}")
        } else {
            format!("{what} open")
        }
    };
    if kid.merged() {
        "done".into()
    } else if work.queued {
        "merging".into()
    } else if work.pr && work.threads > 0 {
        fixing("comments")
    } else if work.pr && work.red {
        fixing("CI")
    } else if work.working {
        "building".into()
    } else if work.pr || kid.state == "In Review" {
        "your review".into()
    } else if kid.in_flight(bot) && kid.kind == "unstarted" && work.capped {
        "held: review cap".into()
    } else if kid.in_flight(bot) && work.quiet {
        "bot quiet".into()
    } else if kid.in_flight(bot) {
        "with the bot".into()
    } else if work.session
        && kid
            .blocked_by
            .iter()
            .all(|b| kids.iter().any(|k| &k.identifier == b && k.merged()))
    {
        "stopped".into()
    } else {
        let why = why(kid, kids, bot, parallel);
        why.strip_prefix("waits for ").map_or(why.clone(), |rest| {
            format!("waiting on {}", rest.trim_end_matches(" to merge"))
        })
    }
}

/// A sub-ticket built here gets its PR's review threads fixed now: open
/// threads, its session idle, not sent already at this head (`mark`).
pub fn fix_now(work: &Work, sent: Option<&String>, mark: &str) -> bool {
    work.pr
        && work.threads > 0
        && work.session
        && !work.working
        && sent.map(String::as_str) != Some(mark)
}

/// A driven sub-ticket as its board card shows it, for `kick`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub stage: crate::story::Stage,
    /// The bot's plan is on the ticket.
    pub planned: bool,
    /// Its PR's CI is red.
    pub red: bool,
    /// Since the last comment or state move.
    pub quiet_ms: u64,
}

impl Look {
    /// What a kick is once for: a new plan, a red run, a new stage asks again.
    pub fn key(&self) -> String {
        format!("{:?}|{}|{}", self.stage, self.planned, self.red)
    }
}

/// What to ask the bot for a sub-ticket gone quiet where it waits for a
/// word: Build once the plan is there (and on a red PR: fix mode), Plan
/// when none came. Never while it builds: a kick then resets its run.
pub fn kick(look: &Look) -> Option<crate::story::BotCommand> {
    use crate::story::{BotCommand, Stage};
    if look.quiet_ms < KICK_MS {
        return None;
    }
    match look.stage {
        Stage::Planning if look.planned => Some(BotCommand::Build),
        Stage::Planning => Some(BotCommand::Plan),
        Stage::Review if look.red => Some(BotCommand::Build),
        _ => None,
    }
}

/// Kicked in this very state and still quiet `KICK_MS` after the kick
/// (the kick is a comment, so quiet counts from it): the bot is not
/// answering, down or skipping its runs, and the ticket is built
/// in an Orbit session instead.
// ponytail: silence is the signal; reading the bot's workflow runs (a
// skipped agent job) would say it sooner, and needs the profile's bot repo.
pub fn fall_back(look: &Look, kicked: Option<&String>) -> bool {
    kicked == Some(&look.key()) && look.quiet_ms >= KICK_MS
}

/// Sub-tickets with the bot and no move for `STALL_MS`, not yet said.
pub fn stalls(kids: &[Kid], it: &Initiative, bot: &str, now_ms: u64) -> Vec<String> {
    kids.iter()
        .filter(|k| k.in_flight(bot) && !it.stalled.contains(&k.identifier))
        .filter(|k| {
            it.moved
                .get(&k.identifier)
                .is_some_and(|at| now_ms.saturating_sub(*at) >= STALL_MS)
        })
        .map(|k| k.identifier.clone())
        .collect()
}

pub fn load() -> Vec<Initiative> {
    let all: Vec<Initiative> = crate::store::load(FILE);
    if !all.is_empty() {
        return all;
    }
    // Missions from before initiatives: each parent becomes one.
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Old {
        parent: String,
        parallel: usize,
    }
    let old: Vec<Old> = crate::store::load("missions.json");
    old.into_iter()
        .map(|m| Initiative {
            id: format!("i{}", m.parent),
            goal: format!("Drive {}", m.parent),
            parent: m.parent,
            parallel: m.parallel.max(1),
            ..Default::default()
        })
        .collect()
}

/// Changes one initiative (by id) on the file as it is now, and saves.
pub fn update(id: &str, f: impl FnOnce(&mut Initiative)) -> std::io::Result<Vec<Initiative>> {
    let mut all = load();
    if let Some(it) = all.iter_mut().find(|i| i.id == id) {
        f(it);
        let over = it.log.len().saturating_sub(LOG);
        it.log.drain(..over);
    }
    crate::store::save(FILE, &all)?;
    Ok(all)
}

/// A new initiative from the user's words; its id.
pub fn begin(goal: &str, session: &str, now_ms: u64) -> std::io::Result<(String, Vec<Initiative>)> {
    let mut all = load();
    let id = format!("i{now_ms}");
    all.push(Initiative {
        id: id.clone(),
        goal: goal.trim().to_owned(),
        parallel: 2,
        session: session.to_owned(),
        created_ms: now_ms,
        log: vec![(now_ms, "started: researching and filing the tickets".into())],
        ..Default::default()
    });
    crate::store::save(FILE, &all)?;
    Ok((id, all))
}

/// Drives `parent`: the initiative `id` when given (its tickets are filed),
/// else the one already on this parent, else a new one named after it.
pub fn drive(
    parent: &str,
    parallel: usize,
    local: bool,
    id: Option<&str>,
    title: &str,
    session: &str,
    now_ms: u64,
) -> std::io::Result<Vec<Initiative>> {
    let mut all = load();
    let found = all
        .iter()
        .position(|i| id.is_some_and(|id| i.id == id))
        .or_else(|| all.iter().position(|i| i.parent == parent && !i.done));
    let line = if local {
        format!("following {parent}: built here in Orbit sessions")
    } else {
        format!(
            "driving {parent}: {} at a time through the bot",
            parallel.max(1)
        )
    };
    match found {
        Some(at) => {
            let it = &mut all[at];
            it.parent = parent.to_owned();
            it.parallel = parallel.max(1);
            it.local = local;
            it.done = false;
            // The session that drives it owns it: the user steers it there.
            if !session.is_empty() {
                it.session = session.to_owned();
            }
            it.log.push((now_ms, line));
        }
        None => all.push(Initiative {
            id: format!("i{now_ms}"),
            goal: if title.is_empty() {
                format!("Drive {parent}")
            } else {
                title.to_owned()
            },
            parent: parent.to_owned(),
            parallel: parallel.max(1),
            local,
            session: session.to_owned(),
            created_ms: now_ms,
            log: vec![(now_ms, line)],
            ..Default::default()
        }),
    }
    crate::store::save(FILE, &all)?;
    Ok(all)
}

/// The session the user steers this initiative from, opened for it
/// because none drove it (`Action::Steer`).
pub fn own(id: &str, session: &str, now_ms: u64) -> std::io::Result<Vec<Initiative>> {
    let mut all = load();
    for it in all.iter_mut().filter(|i| i.id == id) {
        it.session = session.to_owned();
        it.log.push((now_ms, "a session to steer it".into()));
    }
    crate::store::save(FILE, &all)?;
    Ok(all)
}

/// A step where work happens right now (the initiative's spinner), not
/// one that waits on a sibling, the user or a free slot.
pub fn moving(phase: &str) -> bool {
    matches!(phase, "building" | "merging")
        || phase.starts_with("fixing ")
        || phase == "with the bot"
}

/// Stops the initiative with this id or parent: done, said in its log.
pub fn stop(key: &str, now_ms: u64) -> std::io::Result<Vec<Initiative>> {
    let mut all = load();
    for it in all
        .iter_mut()
        .filter(|i| i.id == key || (i.parent == key && !i.done))
    {
        it.done = true;
        it.log.push((now_ms, "stopped by you".into()));
    }
    crate::store::save(FILE, &all)?;
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kid(id: &str, team: &str, state: &str, kind: &str, blocked_by: &[&str]) -> Kid {
        Kid {
            identifier: id.into(),
            team: team.into(),
            state: state.into(),
            kind: kind.into(),
            blocked_by: blocked_by.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn the_clone_hands_the_bot_what_is_unblocked_up_to_the_limit() {
        // ABC-769 as it stands: 206 blocks 208.
        let kids = vec![
            kid("DEV-207", "DEV", "Todo", "unstarted", &[]),
            kid("ABC-773", "ABC", "Backlog", "backlog", &[]),
            kid("DEV-208", "DEV", "Backlog", "backlog", &["DEV-206"]),
            kid("DEV-206", "DEV", "Todo", "unstarted", &[]),
            kid("ABC-770", "ABC", "Canceled", "canceled", &[]),
        ];
        // Two already in flight: nothing more at 2.
        assert!(next(&kids, "DEV", 2).is_empty());
        // At 3, the next unblocked one, not 208 (206 is not merged).
        assert_eq!(next(&kids, "DEV", 3), ["ABC-773"]);
        // 206 merged, 207's PR open: 773 and 208 both go.
        let mut later = kids.clone();
        later[3] = kid("DEV-206", "DEV", "Testing", "started", &[]);
        later[0] = kid("DEV-207", "DEV", "In Review", "started", &[]);
        assert_eq!(next(&later, "DEV", 2), ["ABC-773", "DEV-208"]);
        assert!(!finished(&later));
        let done: Vec<Kid> = later
            .iter()
            .map(|k| kid(&k.identifier, "DEV", "In Review", "started", &[]))
            .collect();
        assert!(finished(&done));
    }

    #[test]
    fn the_user_hears_once_when_a_sub_ticket_needs_them() {
        let mut seen = BTreeMap::new();
        let at = |state: &str, kind: &str| vec![kid("DEV-206", "DEV", state, kind, &[])];
        // The first look only learns where things stand.
        assert!(changes(&at("In Progress", "started"), &mut seen).is_empty());
        assert_eq!(
            changes(&at("In Review", "started"), &mut seen),
            [("DEV-206".to_owned(), Change::Review)]
        );
        assert!(
            changes(&at("In Review", "started"), &mut seen).is_empty(),
            "once"
        );
        assert_eq!(
            changes(&at("Blocked", "started"), &mut seen)[0].1,
            Change::Blocked
        );
        assert_eq!(
            changes(&at("Testing", "started"), &mut seen)[0].1,
            Change::Merged
        );
        // Any other move is logged, not said.
        assert_eq!(
            changes(&at("Canceled", "canceled"), &mut seen)[0].1,
            Change::Moved("Testing".into())
        );
    }

    #[test]
    fn a_sub_ticket_stuck_with_the_bot_is_said_once_with_why() {
        let kids = vec![
            kid("DEV-231", "DEV", "In Progress", "started", &[]),
            kid("DEV-232", "DEV", "In Progress", "started", &[]),
            kid("DEV-228", "DEV", "Backlog", "backlog", &[]),
            kid("DEV-229", "DEV", "Backlog", "backlog", &["DEV-230"]),
        ];
        let mut it = Initiative::default();
        it.moved.insert("DEV-231".into(), 0);
        it.moved.insert("DEV-232".into(), STALL_MS);
        assert_eq!(stalls(&kids, &it, "DEV", STALL_MS), ["DEV-231"]);
        it.stalled.insert("DEV-231".into());
        assert!(stalls(&kids, &it, "DEV", STALL_MS).is_empty(), "once");
        assert_eq!(why(&kids[0], &kids, "DEV", 2), "with the bot (In Progress)");
        assert_eq!(
            why(&kids[2], &kids, "DEV", 2),
            "next: 2 already with the bot"
        );
        assert_eq!(why(&kids[3], &kids, "DEV", 2), "waits for DEV-230 to merge");
    }

    #[test]
    fn a_quiet_sub_ticket_gets_the_bots_next_step_and_a_busy_one_nothing() {
        use crate::story::{BotCommand, Stage};
        let look = |stage, planned, red, quiet_ms| Look {
            stage,
            planned,
            red,
            quiet_ms,
        };
        // A plan done at night, nothing since.
        assert_eq!(
            kick(&look(Stage::Planning, true, false, KICK_MS)),
            Some(BotCommand::Build)
        );
        assert_eq!(
            kick(&look(Stage::Planning, false, false, KICK_MS)),
            Some(BotCommand::Plan)
        );
        assert_eq!(
            kick(&look(Stage::Review, true, true, KICK_MS)),
            Some(BotCommand::Build)
        );
        // Not quiet long enough, building, or a green PR: left alone.
        assert_eq!(kick(&look(Stage::Planning, true, false, KICK_MS - 1)), None);
        assert_eq!(kick(&look(Stage::Building, true, false, KICK_MS * 9)), None);
        assert_eq!(kick(&look(Stage::Review, true, false, KICK_MS * 9)), None);
        // A plan arriving is a new state: one more kick.
        assert_ne!(
            look(Stage::Planning, false, false, 0).key(),
            look(Stage::Planning, true, false, 0).key()
        );
    }

    #[test]
    fn each_sub_ticket_says_its_step_in_a_word() {
        let kids = vec![
            kid("ABC-793", "ABC", "Done", "completed", &[]),
            kid("ABC-794", "ABC", "In Progress", "started", &["ABC-793"]),
            kid("ABC-795", "ABC", "Todo", "unstarted", &["ABC-794"]),
        ];
        let at = |i: usize, work: Work| phase(&kids[i], &kids, "DEV", 1, &work);
        assert_eq!(at(0, Work::default()), "done");
        let working = Work {
            working: true,
            session: true,
            ..Work::default()
        };
        assert_eq!(at(1, working.clone()), "building");
        let comments = Work {
            pr: true,
            threads: 2,
            ..working.clone()
        };
        assert_eq!(at(1, comments.clone()), "fixing comments");
        assert_eq!(
            at(
                1,
                Work {
                    working: false,
                    ..comments
                }
            ),
            "comments open"
        );
        let clean = Work {
            pr: true,
            session: true,
            ..Work::default()
        };
        assert_eq!(at(1, clean.clone()), "your review");
        assert_eq!(
            at(
                1,
                Work {
                    queued: true,
                    ..clean
                }
            ),
            "merging"
        );
        assert_eq!(
            at(
                1,
                Work {
                    session: true,
                    ..Work::default()
                }
            ),
            "stopped"
        );
        assert_eq!(at(2, Work::default()), "waiting on ABC-794");
    }

    #[test]
    fn with_the_bot_only_reads_as_work_while_something_happens() {
        let kids = vec![
            kid("DEV-270", "DEV", "In Progress", "started", &[]),
            kid("DEV-271", "DEV", "Todo", "unstarted", &[]),
        ];
        let at = |i: usize, work: Work| phase(&kids[i], &kids, "DEV", 2, &work);
        assert_eq!(at(0, Work::default()), "with the bot");
        assert!(moving(&at(0, Work::default())));
        let quiet = Work {
            quiet: true,
            ..Work::default()
        };
        assert_eq!(at(0, quiet.clone()), "bot quiet");
        assert!(!moving("bot quiet"), "a stalled bot does not spin");
        let capped = Work {
            capped: true,
            ..Work::default()
        };
        // The cap holds new builds: one already in progress goes on.
        assert_eq!(at(1, capped.clone()), "held: review cap");
        assert_eq!(at(0, capped), "with the bot");
        assert!(!moving("held: review cap"));
    }

    #[test]
    fn open_threads_on_a_pr_built_here_get_fixed_once_per_head() {
        let open = Work {
            session: true,
            pr: true,
            threads: 1,
            ..Work::default()
        };
        assert!(fix_now(&open, None, "fix|abc"));
        assert!(
            !fix_now(&open, Some(&"fix|abc".to_owned()), "fix|abc"),
            "sent at this head"
        );
        assert!(
            fix_now(&open, Some(&"fix|abc".to_owned()), "fix|def"),
            "a new push"
        );
        assert!(
            !fix_now(
                &Work {
                    working: true,
                    ..open.clone()
                },
                None,
                "fix|abc"
            ),
            "busy"
        );
        assert!(!fix_now(&Work { threads: 0, ..open }, None, "fix|abc"));
    }

    #[test]
    fn a_kick_the_bot_never_answers_falls_back_to_an_orbit_session() {
        let look = |quiet_ms| Look {
            stage: crate::story::Stage::Planning,
            planned: false,
            red: false,
            quiet_ms,
        };
        let key = look(0).key();
        assert!(
            !fall_back(&look(KICK_MS), None),
            "not kicked yet: kick first"
        );
        assert!(
            !fall_back(&look(KICK_MS - 1), Some(&key)),
            "the bot has its time"
        );
        assert!(fall_back(&look(KICK_MS), Some(&key)));
        let planned = Look {
            planned: true,
            ..look(KICK_MS)
        };
        assert!(!fall_back(&planned, Some(&key)), "it answered: a new state");
    }

    #[test]
    fn only_a_step_where_work_happens_spins() {
        for phase in [
            "building",
            "merging",
            "fixing comments",
            "fixing CI",
            "with the bot",
        ] {
            assert!(moving(phase), "{phase}");
        }
        for phase in [
            "done",
            "your review",
            "comments open",
            "waiting on DEV-1",
            "next",
            "stopped",
        ] {
            assert!(!moving(phase), "{phase}");
        }
    }

    #[test]
    fn sub_tickets_go_in_the_order_they_were_filed() {
        let child = |id: &str, at: &str| -> crate::linear::Child {
            serde_json::from_value(serde_json::json!({
                "identifier": id, "title": "t", "url": "u", "createdAt": at,
                "state": {"name": "Backlog", "type": "backlog"}, "assignee": null,
                "team": {"key": "DEV"},
            }))
            .unwrap()
        };
        // Linear lists the newest first; the mission filed 228 first.
        let kids = kids(&[
            child("DEV-232", "2026-10-06T21:28:05Z"),
            child("DEV-228", "2026-10-06T21:28:01Z"),
        ]);
        assert_eq!(next(&kids, "DEV", 1), ["DEV-228"]);
    }
}
