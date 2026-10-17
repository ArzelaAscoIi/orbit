//! The running app's side of `orbit ctl` (src/control.rs): each request is
//! answered on the UI thread between frames. An action goes through the
//! same `apply` a click does, so the window shows it, and is logged with
//! `via: "cli"`. A `Yours` action is refused: approving, merging and
//! answering a permission prompt stay the user's click (CLAUDE.md Safety).

use super::{Action, App};
use crate::activity::{Gate, name};
use crate::story::Card;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

impl App {
    /// Answers every request waiting on the socket.
    pub(super) fn follow_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.control else {
            return;
        };
        let requests: Vec<crate::control::Request> = rx.try_iter().collect();
        for request in requests {
            if request.body["cmd"] == "write" {
                self.linear_write(request, ctx);
                continue;
            }
            let out = self.control_answer(&request.body, ctx);
            let _ = request.reply.send(out);
        }
    }

    /// `orbit ctl comment / move / parent / block`: Orbit's own Linear key
    /// does it, and the backend answers the socket once Linear said yes.
    /// Gate queue: what other people see, pressed only when the user asked.
    fn linear_write(&mut self, request: crate::control::Request, ctx: &egui::Context) {
        let write = serde_json::from_value::<crate::linear::Write>(request.body["write"].clone())
            .map_err(|e| format!("not a write ({e}); see orbit ctl help"))
            .and_then(|w| w.check().map(|()| w))
            .and_then(|w| match self.backend {
                Some(_) => Ok(w),
                None => Err("not signed in to Linear".to_owned()),
            });
        let write = match write {
            Ok(write) => write,
            Err(error) => {
                let _ = request.reply.send(fail(error));
                return;
            }
        };
        let (what, on) = match &write {
            crate::linear::Write::Comment { issue, .. } => ("comment", issue),
            crate::linear::Write::Move { issue, .. } => ("move", issue),
            crate::linear::Write::Parent { issue, .. } => ("parent", issue),
            crate::linear::Write::Block { blocker, .. } => ("block", blocker),
        };
        let line = format!("agent · {what} {on}");
        self.log(&format!("Linear{}", name(&write)), "", "cli", None);
        self.agent_toast(line);
        self.send(crate::backend::Command::Write {
            write,
            reply: request.reply,
        });
        ctx.request_repaint();
    }

    fn agent_toast(&mut self, line: String) {
        self.toasts.push(super::Toast {
            key: format!("agent:{}", crate::activity::now_ms()),
            line,
            at: std::time::Instant::now(),
            agent: true,
        });
    }

    pub(super) fn control_answer(&mut self, body: &Value, ctx: &egui::Context) -> Value {
        let id = body["id"].as_str().unwrap_or_default();
        match body["cmd"].as_str().unwrap_or_default() {
            "do" => self.press(body["action"].clone(), ctx),
            "open" => match self.find(id) {
                Some(card) => {
                    let id = card.issue.id.clone();
                    // As the palette opens one: the right window, the Ticket tab.
                    self.press(json!({ "Focus": id }), ctx)
                }
                None => fail(format!("no ticket {id} on the board")),
            },
            "state" => self.control_state(),
            "tickets" => {
                let cards: Vec<&Card> = if id == "all" {
                    self.cards.iter().chain(&self.pr_cards).collect()
                } else {
                    self.for_you()
                };
                json!({"ok": true, "tickets": cards.into_iter().map(|c| self.card_json(c, false)).collect::<Vec<_>>()})
            }
            "ticket" => match self.find(id) {
                Some(card) => {
                    json!({"ok": true, "ticket": self.card_json(card, true), "session": self.session_json(&card.issue.id)})
                }
                None if self.claude.saved.contains_key(id)
                    || self.claude.starting.contains(id)
                    || self.claude.notes.contains_key(id) =>
                {
                    json!({"ok": true, "session": self.session_json(id)})
                }
                None => fail(format!("no ticket or session {id}")),
            },
            "sessions" => {
                let keys: Vec<String> = self.claude.saved.keys().cloned().collect();
                let rows: Vec<Value> = keys
                    .iter()
                    .map(|k| {
                        let mut row = self.session_json(k);
                        row["work"] = self.work_of(k);
                        row
                    })
                    .collect();
                json!({"ok": true, "sessions": rows})
            }
            "close" => {
                let key = self
                    .find(id)
                    .map(|c| c.issue.id.clone())
                    .filter(|k| self.claude.saved.contains_key(k))
                    .or_else(|| self.session_key(id));
                match key {
                    Some(key) => self.press(json!({ "CloseSession": key }), ctx),
                    None => fail(format!("no session {id}")),
                }
            }
            "inbox" => {
                let (items, _) = self.inbox();
                let rows: Vec<Value> = items
                    .iter()
                    .map(|i| {
                        json!({
                            "key": i.key, "ask": name(&i.ask), "title": i.title, "why": i.why,
                            "url": i.url, "pr": i.pr, "ticket": i.issue, "at": i.at,
                        })
                    })
                    .collect();
                json!({"ok": true, "inbox": rows})
            }
            "actions" => {
                let rows: Vec<Value> = crate::control::actions()
                    .into_iter()
                    .filter(|(n, ..)| id.is_empty() || n.eq_ignore_ascii_case(id))
                    .map(|(n, payload, doc)| {
                        let gate = crate::activity::gate(&n).map(|g| name(&g).to_lowercase());
                        let example = example(&n, &payload);
                        json!({"name": n, "gate": gate, "payload": payload, "example": example, "doc": doc})
                    })
                    .collect();
                json!({"ok": true, "actions": rows})
            }
            "say" => {
                let text: String = body["text"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect();
                if text.trim().is_empty() {
                    return fail("say needs a line".into());
                }
                // A toast, and the Mac's notification while Orbit is behind.
                self.agent_says(&text);
                self.log("AgentSaid", "", "cli", None);
                ctx.request_repaint();
                json!({"ok": true})
            }
            "alerts" => {
                let alerts: Vec<Value> = self
                    .alerts
                    .iter()
                    .map(|a| json!({"id": a.id, "title": a.title, "status": a.status, "url": a.source_url, "createdAt": a.created_at, "resolvedAt": a.resolved_at}))
                    .collect();
                let incidents: Vec<Value> = self
                    .incidents
                    .iter()
                    .map(|i| json!({"id": i.id, "reference": i.reference, "name": i.name, "severity": i.severity, "status": i.status, "summary": i.summary, "url": i.permalink}))
                    .collect();
                let releases: Vec<Value> = self
                    .releases
                    .iter()
                    .map(|(r, run)| match run {
                        Ok(Some(run)) => json!({"name": r.name, "repo": r.repo, "runId": run.id, "status": run.status, "conclusion": run.conclusion, "url": run.url, "title": run.title}),
                        Ok(None) => json!({"name": r.name, "repo": r.repo, "run": null}),
                        Err(e) => json!({"name": r.name, "repo": r.repo, "error": e}),
                    })
                    .collect();
                let sessions = |prefix: &str| -> Vec<String> {
                    self.claude
                        .saved
                        .keys()
                        .filter(|k| k.starts_with(prefix))
                        .cloned()
                        .collect()
                };
                json!({
                    "ok": true, "alerts": alerts, "incidents": incidents, "releases": releases,
                    "alertsError": self.alerts_error, "incidentsError": self.incidents_error,
                    "alertSessions": sessions(crate::claude::ALERT),
                    "incidentSessions": sessions(crate::claude::INCIDENT),
                })
            }
            "prs" => {
                // Loaded on the first ask (the palette loads them too), then
                // each PR's gate once: `pending` until both are in.
                if self.lookups_asked.insert("my-prs".into()) {
                    self.send(crate::backend::Command::MyPrs);
                    return json!({"ok": true, "pending": true, "prs": []});
                }
                if !self.lookups_asked.contains("my-prs:in") {
                    return json!({"ok": true, "pending": true, "prs": []});
                }
                let urls: Vec<String> = self.my_prs.iter().map(|p| p.url.clone()).collect();
                for url in &urls {
                    if !self.gates.contains_key(url) && self.lookups_asked.insert(url.clone()) {
                        self.send(crate::backend::Command::Gate { url: url.clone() });
                    }
                }
                let prs: Vec<Value> = self
                    .my_prs
                    .iter()
                    .map(|p| {
                        let gate = match self.gates.get(&p.url) {
                            Some(Ok(g)) => json!({"head": g.head, "green": g.green, "review": g.review, "checks": g.checks, "branch": g.branch, "openThreads": g.open_threads, "why": g.why}),
                            Some(Err(e)) => json!({"error": e}),
                            None => json!({"pending": true}),
                        };
                        json!({"url": p.url, "repo": p.repo, "number": p.number, "title": p.title, "draft": p.draft, "updatedAt": p.at, "gate": gate})
                    })
                    .collect();
                json!({"ok": true, "prs": prs})
            }
            "missions" => {
                let rows: Vec<Value> = self
                    .missions
                    .iter()
                    .map(|m| {
                        let kids = match self.looked_up.get(&m.parent) {
                            Some(Ok(p)) => crate::mission::kids(&p.children.nodes),
                            _ => Vec::new(),
                        };
                        let bot = crate::profile::get()
                            .orchestrator
                            .as_ref()
                            .map(|o| o.team.clone())
                            .unwrap_or_default();
                        json!({
                            "id": m.id,
                            "goal": m.goal,
                            "done": m.done,
                            // Oldest first, each line with its time.
                            "log": m.log.iter().map(|(at, l)| json!({"at": at, "line": l})).collect::<Vec<_>>(),
                            "parent": m.parent,
                            // The session that steers it, empty until one does.
                            "session": m.session,
                            "parallel": m.parallel,
                            "subTickets": kids.iter().map(|k| json!({
                                "identifier": k.identifier, "team": k.team, "state": k.state,
                                "implemented": k.implemented(), "blockedBy": k.blocked_by,
                                "why": crate::mission::why(k, &kids, &bot, m.parallel),
                                "movedAt": m.moved.get(&k.identifier),
                                "url": match self.looked_up.get(&m.parent) {
                                    Some(Ok(p)) => p.children.nodes.iter()
                                        .find(|c| c.identifier == k.identifier)
                                        .map(|c| c.url.clone()),
                                    _ => None,
                                },
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                json!({"ok": true, "missions": rows})
            }
            "" => fail(body["error"].as_str().unwrap_or("no cmd").to_owned()),
            cmd => fail(format!("unknown cmd {cmd}; see orbit ctl help")),
        }
    }

    /// One button, pressed the way a click presses it.
    fn press(&mut self, action: Value, ctx: &egui::Context) -> Value {
        let action: Action = match serde_json::from_value(action) {
            Ok(action) => action,
            Err(e) => return fail(format!("not an action ({e}); see orbit ctl actions")),
        };
        let event = name(&action);
        if yours(&action) {
            return fail(format!(
                "{event} is the user's alone (gate yours): only their click in Orbit does it"
            ));
        }
        if app_only(&action) {
            return fail(format!(
                "{event} changes Orbit itself (gate app: keys, settings, profile, updates): \
                 only the user does it, in Orbit"
            ));
        }
        if let Some(why) = foreign_folder(&action, &crate::store::data_dir().join("work")) {
            return fail(why);
        }
        if let Some(why) = self.unknown(&action) {
            return fail(why);
        }
        let gate = crate::activity::gate(&event).map(|g| name(&g).to_lowercase());
        // A start that fails later lands in the session's notes (ticket,
        // sessions); the error line is the user's, never cleared from here.
        let before = self.error.clone();
        self.log_action(&action, "cli");
        // On screen too, quieter than what needs the user: to follow along.
        let line = self.agent_line(&action);
        self.agent_toast(line);
        self.apply(action, ctx);
        ctx.request_repaint();
        let error = self.error.clone().filter(|e| Some(e) != before.as_ref());
        json!({"ok": error.is_none(), "action": event, "gate": gate, "error": error})
    }

    fn control_state(&self) -> Value {
        let view = self.ticket_view();
        json!({
            "ok": true,
            "view": name(&self.view),
            "viewDetail": format!("{:?}", self.view),
            "tab": name(&self.tab),
            "sheet": self.sheet,
            "clonePanel": self.chat_open,
            "cloneChat": self.chat,
            "window": self.window,
            "windows": self.windows.len(),
            "ticket": view.selected.as_deref().and_then(|id| self.card(id)).map(|c| self.card_json(c, false)),
            "session": view.session,
            "signedIn": !self.viewer.is_empty(),
            "loadedAt": self.loaded_at,
            "error": self.error,
            "forYou": self.for_you().len(),
            "board": self.cards.len(),
            "sessions": self.claude.saved.len(),
            "inbox": self.inbox().0.len(),
        })
    }

    /// A board card or PR card by Linear id, identifier or PR URL.
    fn find(&self, id: &str) -> Option<&Card> {
        self.cards.iter().chain(&self.pr_cards).find(|c| {
            c.issue.id == id
                || c.issue.identifier.eq_ignore_ascii_case(id)
                || c.prs.iter().any(|p| p.url == id)
        })
    }

    fn card_json(&self, card: &Card, full: bool) -> Value {
        let prs: Vec<Value> = card
            .prs
            .iter()
            .map(|p| {
                let mut pr = json!({
                    "url": p.url, "name": p.name(), "open": p.open, "merged": p.merged,
                    "draft": p.draft, "approved": p.approved, "conflicts": p.conflicts,
                });
                if full && let Some(Ok(g)) = self.gates.get(&p.url) {
                    pr["gate"] = json!({
                        "head": g.head, "green": g.green, "why": g.why, "state": g.state,
                        "draft": g.draft, "queued": g.queued, "review": g.review,
                        "checks": g.checks, "branch": g.branch, "own": g.own,
                        "author": g.author, "authorBot": g.author_bot,
                        "canApprove": g.can_approve, "openThreads": g.open_threads,
                        "autoMerge": g.auto_merge, "mergeQueue": g.merge_queue,
                    });
                }
                pr
            })
            .collect();
        let mut out = json!({
            "id": card.issue.id,
            "identifier": card.issue.identifier,
            "title": card.issue.title,
            "url": card.issue.url,
            "state": card.issue.state.name,
            "lane": name(&card.lane),
            "stage": name(&card.stage),
            "ask": card.ask.map(|a| name(&a)),
            "why": card.why,
            "ci": name(&card.ci),
            "mine": self.is_mine(card),
            "botWorks": card.bot_works,
            "prs": prs,
            "session": self.claude.row_of(&card.issue.id).map(|r| r.label().to_owned()),
        });
        if full {
            out["description"] = json!(card.issue.description);
            out["customers"] = json!(card.customers);
            out["parent"] = json!(card.issue.parent.as_ref().map(|p| &p.identifier));
            out["subTickets"] = json!(card
                .issue
                .children
                .nodes
                .iter()
                .map(|c| json!({"identifier": c.identifier, "title": c.title, "state": c.state.name}))
                .collect::<Vec<_>>());
        }
        out
    }

    /// A session by key, short id or its ticket's identifier.
    fn session_key(&self, id: &str) -> Option<String> {
        self.claude
            .saved
            .iter()
            .find(|(k, s)| *k == id || s.short_id() == id || s.identifier.eq_ignore_ascii_case(id))
            .map(|(k, _)| k.clone())
    }

    /// What the session worked on and where that stands: its ticket's
    /// Linear state, its PRs' (open, merged, closed). Off the board it is
    /// looked up once (`pending` until the answer is in).
    fn work_of(&mut self, key: &str) -> Value {
        let pr_state = |p: &crate::story::Pr| {
            if p.merged {
                "merged"
            } else if p.open {
                "open"
            } else {
                "closed"
            }
        };
        if let Some(card) = self.find(key) {
            let prs: Vec<Value> = card
                .prs
                .iter()
                .map(|p| json!({"url": p.url, "state": pr_state(p)}))
                .collect();
            let finished = matches!(card.issue.state.kind.as_str(), "completed" | "canceled");
            return json!({"ticket": card.issue.identifier, "state": card.issue.state.name, "finished": finished, "prs": prs});
        }
        if let Some(url) = crate::story::pr_url_of(key) {
            return match self.gates.get(&url) {
                Some(Ok(g)) => {
                    let state = g.state.to_lowercase();
                    json!({"pr": url, "state": state, "finished": state != "open"})
                }
                Some(Err(e)) => json!({"pr": url, "error": e}),
                None => {
                    if self.lookups_asked.insert(url.clone()) {
                        self.send(crate::backend::Command::Gate { url: url.clone() });
                    }
                    json!({"pr": url, "pending": true})
                }
            };
        }
        let identifier = self
            .claude
            .saved
            .get(key)
            .map(|s| s.identifier.clone())
            .unwrap_or_default();
        if !is_identifier(&identifier) {
            return Value::Null;
        }
        match self.looked_up.get(&identifier) {
            Some(Ok(issue)) => {
                let finished = matches!(issue.state.kind.as_str(), "completed" | "canceled");
                json!({"ticket": identifier, "state": issue.state.name, "finished": finished})
            }
            Some(Err(e)) => json!({"ticket": identifier, "error": e}),
            None => {
                if self.lookups_asked.insert(identifier.clone()) {
                    self.send(crate::backend::Command::Lookup {
                        id: identifier.clone(),
                    });
                }
                json!({"ticket": identifier, "pending": true})
            }
        }
    }

    /// The toast line for an action a program pressed: what, and on what.
    fn agent_line(&self, action: &Action) -> String {
        let what = match super::target(action) {
            Some(super::Target::Issue(id)) => self.card(&id).map(|c| c.issue.identifier.clone()),
            Some(super::Target::Pr(url)) => self
                .cards
                .iter()
                .chain(&self.pr_cards)
                .flat_map(|c| &c.prs)
                .find(|p| p.url == url)
                .map(|p| p.name())
                .or(Some(url)),
            Some(super::Target::Session(key)) => Some(
                self.claude
                    .saved
                    .get(&key)
                    .map_or(key.clone(), |s| self.session_name(&key, s)),
            ),
            Some(super::Target::Inbox(key)) => Some(key),
            None => match action {
                Action::Show(view) => Some(name(view)),
                Action::Tab(tab) => Some(name(tab)),
                _ => None,
            },
        };
        let event = name(action);
        match what {
            Some(what) => format!("agent · {event} {what}"),
            None => format!("agent · {event}"),
        }
    }

    fn session_json(&self, key: &str) -> Value {
        let Some(saved) = self.claude.saved.get(key) else {
            return match self.claude.notes.get(key) {
                _ if self.claude.starting.contains(key) => json!({"key": key, "state": "starting"}),
                Some(error) => json!({"key": key, "state": "failed", "error": error}),
                None => json!({"key": key, "state": "none"}),
            };
        };
        let row = self.claude.row_of(key);
        json!({
            "key": key,
            "link": format!("orbit://session/{key}"),
            "name": self.session_name(key, saved),
            "identifier": saved.identifier,
            "agent": name(&self.claude.agent_of(key)),
            "state": row.map_or("stopped", |r| r.label()),
            "report": row.and_then(|r| r.report.as_ref()).map(|r| name(&r.state)),
            "shortId": saved.short_id(),
            "cwd": saved.cwd,
            "startedAt": saved.started_at,
            "run": run_of(self.claude.status.get(key).map_or(&[][..], Vec::as_slice)),
        })
    }
}

/// What a run is doing, from its status log, so a driving agent reads it
/// without opening the session: its own last note, the question or
/// approval it waits on, its PR, why it failed.
fn run_of(reports: &[crate::claude::status::Report]) -> Value {
    use crate::claude::status::{self, State};
    let text = |r: Option<&status::Report>| r.map(|r| r.text.clone());
    let asks = status::current(reports)
        .filter(|r| matches!(r.state, State::NeedsDecision | State::NeedsApproval));
    json!({
        "note": text(status::note(reports)),
        "asks": text(asks),
        "pr": text(status::pr(reports)),
        "failed": text(status::failed(reports)),
    })
}

impl App {
    /// Why a pressed action names something the app does not know, from
    /// what it already holds: the board, looked-up tickets, team states,
    /// PRs it has seen. Nothing is asked.
    fn unknown(&self, action: &Action) -> Option<String> {
        let cards = || self.cards.iter().chain(&self.pr_cards);
        let tickets: HashMap<&str, &str> = cards()
            .map(|c| &c.issue)
            .chain(self.looked_up.values().filter_map(|r| r.as_ref().ok()))
            // Focus (and Linear) take the identifier too: ABC-1.
            .flat_map(|i| {
                [
                    (i.id.as_str(), i.team.id.as_str()),
                    (i.identifier.as_str(), i.team.id.as_str()),
                ]
            })
            .collect();
        let prs: HashSet<&str> = cards()
            .flat_map(|c| &c.prs)
            .map(|p| p.url.as_str())
            .chain(self.gates.keys().map(String::as_str))
            .chain(self.my_prs.iter().map(|p| p.url.as_str()))
            .chain(
                self.repo_prs
                    .values()
                    .filter_map(|(_, r)| r.as_ref().ok())
                    .flatten()
                    .map(|p| p.url.as_str()),
            )
            .collect();
        let teams: Vec<&str> = self.all_teams.iter().map(|t| t.0.as_str()).collect();
        unknown_in(action, &tickets, &self.states, &teams, &prs)
    }
}

/// `App::unknown` on what it knows: tickets (id → team id), each team's
/// states, team ids, PR URLs.
fn unknown_in(
    action: &Action,
    tickets: &HashMap<&str, &str>,
    states: &HashMap<String, Vec<crate::linear::State>>,
    teams: &[&str],
    prs: &HashSet<&str>,
) -> Option<String> {
    if let Action::WithAgent(_, inner) = action {
        return unknown_in(inner, tickets, states, teams, prs);
    }
    match super::target(action) {
        Some(super::Target::Issue(id)) if !tickets.contains_key(id.as_str()) => {
            return Some(format!(
                "unknown ticket id {id} (use the `id` field from `orbit ctl ticket`)"
            ));
        }
        // Opening a PR by its URL is how one off the board is shown.
        Some(super::Target::Pr(url))
            if crate::github::parts(&url).is_none()
                || (!prs.contains(url.as_str())
                    && !matches!(action, Action::OpenPr { .. } | Action::DiffPr(_))) =>
        {
            return Some(format!(
                "unknown PR {url}: give the full https://github.com/<owner>/<repo>/pull/<n> URL \
                 of a PR on the board, in `orbit ctl prs` or a ticket's `prs`"
            ));
        }
        _ => {}
    }
    match action {
        Action::SetState { issue_id, state_id } => {
            let team = tickets.get(issue_id.as_str()).and_then(|t| states.get(*t));
            let known = match team {
                Some(team) => team.iter().any(|s| s.id == *state_id),
                None => states.values().flatten().any(|s| s.id == *state_id),
            };
            (!known).then(|| {
                let names: Vec<String> = team
                    .into_iter()
                    .flatten()
                    .map(|s| format!("{} {}", s.id, s.name))
                    .collect();
                format!(
                    "unknown state id {state_id} for this ticket's team ({}); or `orbit ctl move <ABC-1> <state name>`",
                    names.join(", ")
                )
            })
        }
        Action::MoveTeam { team_id, .. } if !teams.contains(&team_id.as_str()) => {
            Some(format!("unknown team id {team_id}"))
        }
        _ => None,
    }
}

/// A payload to copy for `orbit ctl actions`: the action's shape with each
/// part filled in by what it is (a PR's URL, a ticket's id, a state's).
fn example(name: &str, shape: &str) -> Value {
    let pr = "https://github.com/OWNER/REPO/pull/123";
    let field = |field: &str, kind: &str| -> Value {
        match (field, kind) {
            (_, "bool") => json!(false),
            (_, k) if k.starts_with("usize") || k.starts_with("u64") || k.starts_with("u32") => {
                json!(0)
            }
            (_, k) if k.starts_with("Option") => Value::Null,
            ("url", _) => json!(pr),
            ("head", _) => json!("<the PR's gate.head from orbit ctl ticket>"),
            ("issue_id" | "id", _) => json!("<the ticket's id from orbit ctl ticket>"),
            ("state_id", _) => json!("<a state id of its team>"),
            ("team_id", _) => json!("<a team id>"),
            ("parent", _) => json!("ABC-1"),
            ("key", _) => json!("<a session key from orbit ctl sessions>"),
            (f, _) => json!(format!("<{f}>")),
        }
    };
    let shape = shape.trim();
    if shape.is_empty() {
        return json!(name);
    }
    if let Some(fields) = shape.strip_prefix('{') {
        let mut out = serde_json::Map::new();
        for part in fields.trim_end_matches('}').split(',') {
            if let Some((f, kind)) = part.split_once(':') {
                let f = f.trim().trim_start_matches("#[serde(default)]").trim();
                out.insert(f.to_owned(), field(f, kind.trim()));
            }
        }
        return json!({ name: out });
    }
    // A tuple variant: a String by what the action does with it.
    let inner = shape.trim_start_matches('(').trim_end_matches(')');
    let value = if inner == "String" {
        let probe = |v: &str| serde_json::from_value::<Action>(json!({ name: v })).ok();
        match probe("x").as_ref().and_then(super::target) {
            Some(super::Target::Pr(_)) => json!(pr),
            Some(super::Target::Issue(_)) => json!("<the ticket's id from orbit ctl ticket>"),
            Some(super::Target::Session(_)) => json!("<a session key from orbit ctl sessions>"),
            Some(super::Target::Inbox(_)) => json!("<a key from orbit ctl inbox>"),
            None => json!("<text>"),
        }
    } else if inner == "bool" {
        json!(true)
    } else {
        json!(format!("<{inner}>"))
    };
    json!({ name: value })
}

/// Only the user's click: the action, or one it wraps.
fn yours(action: &Action) -> bool {
    let gate = crate::activity::gate(&name(action));
    match action {
        Action::WithAgent(_, inner) => gate == Some(Gate::Yours) || yours(inner),
        _ => gate == Some(Gate::Yours),
    }
}

/// What changes the app itself (keys, settings, the profile, updates,
/// the activity log): never a program's, however it is wrapped.
fn app_only(action: &Action) -> bool {
    let gate = crate::activity::gate(&name(action));
    match action {
        Action::WithAgent(_, inner) => gate == Some(Gate::App) || app_only(inner),
        _ => gate == Some(Gate::App),
    }
}

/// A session in a folder Orbit did not make: only the user picks one.
/// Orbit marks a session's folder trusted for Claude Code, so a program
/// starting one in any folder would switch that folder's trust dialog off.
fn foreign_folder(action: &Action, work: &std::path::Path) -> Option<String> {
    let Action::NewSession {
        folder: Some(folder),
        ..
    } = action
    else {
        return None;
    };
    (!std::path::Path::new(folder).starts_with(work) || folder.contains("..")).then(|| {
        format!(
            "a session in {folder}: only the user starts one in a folder of their own; \
             leave folder out for one in Orbit's work folder"
        )
    })
}

/// `ABC-12`: a Linear identifier, not `Triage` or a PR's name.
fn is_identifier(s: &str) -> bool {
    s.split_once('-').is_some_and(|(team, n)| {
        !team.is_empty() && team.chars().all(|c| c.is_ascii_uppercase()) && n.parse::<u32>().is_ok()
    })
}

fn fail(error: String) -> Value {
    json!({"ok": false, "error": error})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_cannot_change_orbit_itself_or_pick_a_sessions_folder() {
        assert!(app_only(&Action::SetActivityLog(false)));
        assert!(app_only(&Action::SignOut));
        assert!(app_only(&Action::WithAgent(
            crate::claude::Agent::Claude,
            Box::new(Action::SignOut)
        )));
        assert!(!app_only(&Action::Refresh));
        let at = |folder: Option<&str>| Action::NewSession {
            name: "x".into(),
            folder: folder.map(String::from),
        };
        let work = std::path::Path::new("/d/work");
        assert!(foreign_folder(&at(None), work).is_none());
        assert!(foreign_folder(&at(Some("/Users/k")), work).is_some());
        assert!(foreign_folder(&at(Some("/d/work/x")), work).is_none());
        assert!(foreign_folder(&at(Some("/d/work/../..")), work).is_some());
    }

    #[test]
    fn a_session_row_says_what_its_run_waits_on() {
        use crate::claude::status::{Report, State};
        let at = |state, text: &str| Report {
            at: std::time::SystemTime::now(),
            state,
            text: text.into(),
        };
        let run = run_of(&[
            at(State::Working, "/plan XYZ-1"),
            at(State::Note, "Planning XYZ-1"),
            at(State::Pr, "https://github.com/o/r/pull/1"),
            at(State::NeedsDecision, "Post this plan?"),
        ]);
        assert_eq!(run["note"], "Planning XYZ-1");
        assert_eq!(run["asks"], "Post this plan?");
        assert_eq!(run["pr"], "https://github.com/o/r/pull/1");
        assert_eq!(run["failed"], Value::Null);
        let run = run_of(&[
            at(State::NeedsDecision, "q"),
            at(State::Failed, "Workspace not trusted"),
        ]);
        assert_eq!(
            run["asks"],
            Value::Null,
            "answered or gone: not asking any more"
        );
        assert_eq!(run["failed"], "Workspace not trusted");
    }

    #[test]
    fn approve_and_merge_are_refused_from_the_cli() {
        let parse = |v: Value| serde_json::from_value::<Action>(v).unwrap();
        assert!(yours(&parse(json!({"Approve": {"url": "u", "head": "h"}}))));
        assert!(yours(&parse(json!({"Merge": {"url": "u", "head": "h"}}))));
        assert!(yours(&parse(
            json!({"WithAgent": ["codex", {"Merge": {"url": "u", "head": "h"}}]})
        )));
        assert!(!yours(&parse(json!({"Show": "Sessions"}))));
        assert!(!yours(&parse(json!("Refresh"))));
        assert!(!yours(&parse(json!({"CloseSession": "k"}))));
    }

    #[test]
    fn a_made_up_ticket_state_or_pr_is_refused_with_why() {
        let parse = |v: Value| serde_json::from_value::<Action>(v).unwrap();
        let tickets = HashMap::from([("t1", "team1")]);
        let state = |id: &str, name: &str| crate::linear::State {
            id: id.into(),
            name: name.into(),
            color: String::new(),
            position: 0.0,
            kind: "started".into(),
        };
        let states = HashMap::from([
            ("team1".to_owned(), vec![state("s1", "Todo")]),
            ("team2".to_owned(), vec![state("s9", "Done")]),
        ]);
        let pr = "https://github.com/o/r/pull/1";
        let prs = HashSet::from([pr]);
        let check = |v: Value| unknown_in(&parse(v), &tickets, &states, &["team1"], &prs);
        assert_eq!(
            check(json!({"SetState": {"issue_id": "t1", "state_id": "s1"}})),
            None
        );
        let why = check(json!({"SetState": {"issue_id": "fake", "state_id": "s1"}})).unwrap();
        assert!(
            why.contains("unknown ticket id fake") && why.contains("orbit ctl ticket"),
            "{why}"
        );
        // Another team's state is as unknown as a made-up one.
        let why = check(json!({"SetState": {"issue_id": "t1", "state_id": "s9"}})).unwrap();
        assert!(
            why.contains("unknown state id s9") && why.contains("s1 Todo"),
            "{why}"
        );
        assert_eq!(check(json!({"ReadyPr": pr})), None);
        assert!(
            check(json!({"ReadyPr": "123"}))
                .unwrap()
                .contains("unknown PR 123")
        );
        assert!(check(json!({"ReadyPr": "https://github.com/o/r/pull/2"})).is_some());
        // Opening a PR by URL is how one off the board is shown.
        assert_eq!(
            check(json!({"DiffPr": "https://github.com/o/r/pull/2"})),
            None
        );
        assert!(check(json!({"MoveTeam": {"issue_id": "t1", "team_id": "nope"}})).is_some());
        assert!(check(json!({"WithAgent": ["codex", {"Select": "fake"}]})).is_some());
        assert_eq!(check(json!({"Show": "Sessions"})), None);
    }

    #[test]
    fn every_action_has_an_example_and_a_pr_one_takes_a_url() {
        let examples: HashMap<String, Value> = crate::control::actions()
            .into_iter()
            .map(|(n, shape, _)| {
                let e = example(&n, &shape);
                (n, e)
            })
            .collect();
        assert_eq!(
            examples["ReadyPr"],
            json!({"ReadyPr": "https://github.com/OWNER/REPO/pull/123"})
        );
        assert_eq!(examples["Refresh"], json!("Refresh"));
        assert!(examples["SetState"]["SetState"]["state_id"].is_string());
        assert_eq!(
            examples["Merge"]["Merge"]["url"],
            "https://github.com/OWNER/REPO/pull/123"
        );
        // The ones that are only strings and numbers parse as they are.
        for name in [
            "ReadyPr",
            "Refresh",
            "SetState",
            "Merge",
            "Drive",
            "ClosePr",
            "CloseSession",
        ] {
            serde_json::from_value::<Action>(examples[name].clone())
                .unwrap_or_else(|e| panic!("{name}: {e} {}", examples[name]));
        }
    }

    #[test]
    fn only_linear_identifiers_are_looked_up() {
        assert!(is_identifier("ABC-155"));
        assert!(is_identifier("QA-898"));
        assert!(!is_identifier("Triage"));
        assert!(!is_identifier("Land 13 KTLO PRs"));
        assert!(!is_identifier("api#1249"));
    }
}
