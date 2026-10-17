//! What the backend answers, matched against what the interface shows now.

use std::collections::HashSet;

use super::{Action, App, Tab};
use crate::backend::{Command, Event};
use crate::secrets::{self, Secret};
use crate::story;

/// An image file (png, jpeg, gif, webp) as egui draws it.
fn decode(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|e| format!("not an image: {e}"))?
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
}

impl App {
    pub fn handle(&mut self, event: Event, ctx: &egui::Context) {
        match event {
            Event::Loaded { request, result } => self.on_loaded(request, result),
            // Yours on Linear now: the next look shows it.
            Event::TakenFromBot { result } => match result {
                Ok(()) => self.apply(Action::Refresh, ctx),
                Err(error) => self.error = Some(error),
            },
            Event::StateSet {
                issue_id,
                state_id,
                result,
            } => {
                let latest = self
                    .shown
                    .get(&issue_id)
                    .is_some_and(|s| s.state_id == state_id && s.confirmed_by.is_none());
                match result {
                    Ok(()) if latest => {
                        // The refresh sent next is the first to include it.
                        self.shown.get_mut(&issue_id).unwrap().confirmed_by =
                            Some(self.last_request + 1);
                        self.apply(Action::Refresh, ctx);
                    }
                    Ok(()) => {}
                    Err(error) => {
                        self.error = Some(error);
                        if latest {
                            self.shown.remove(&issue_id);
                            self.rebuild();
                            self.apply(Action::Refresh, ctx);
                        }
                    }
                }
            }
            Event::CommentAdded { temp_id, result } => {
                let Some(index) = self.locals.iter().position(|l| l.comment.id == temp_id) else {
                    return;
                };
                match result {
                    Ok(comment) => {
                        let local = &mut self.locals[index];
                        local.comment = comment;
                        local.confirmed_by = Some(self.last_request + 1);
                    }
                    Err(error) => {
                        let lost = self.locals.remove(index);
                        // Hand the text back rather than lose it.
                        if self.draft.is_empty() {
                            self.draft = lost.comment.body;
                        }
                        self.error = Some(error);
                    }
                }
                self.rebuild();
            }
            Event::NotesSaved(result) => {
                if let Err(error) = result {
                    self.error = Some(format!("Linear note: {error}"));
                }
            }
            Event::Gate { url, result } => {
                if !self.merging.contains(&url) {
                    // An older answer that still asks for the review keeps
                    // the approval shown; an error or a done review ends it.
                    if !result.as_ref().is_ok_and(|g| g.can_approve) {
                        self.approving.remove(&url);
                    }
                    // The first answer after a diff arrived says what it shows.
                    if let (Ok(gate), Some(Ok(_))) = (&result, self.diffs.get(&url)) {
                        self.diff_heads
                            .entry(url.clone())
                            .or_insert_with(|| gate.head.clone());
                    }
                    self.guard_auto_merge(&url, &result);
                    if let Ok(gate) = &result {
                        self.pr_known(&url, gate);
                    }
                    let next = match (&result, self.landing.get(&url)) {
                        (Ok(gate), Some(head)) => {
                            Some((super::actions::landing_next(gate, head), head.clone()))
                        }
                        _ => None,
                    };
                    self.gates.insert(url.clone(), result);
                    self.rebuild();
                    // The user's Land click still covers it at the head they saw.
                    use super::actions::Landing;
                    match next {
                        Some((Landing::Merge, head)) => {
                            self.apply(Action::Merge { url, head }, ctx)
                        }
                        Some((Landing::WhenReady, head)) => {
                            self.apply(Action::AutoMerge { url, head }, ctx)
                        }
                        Some((Landing::Done, _)) => {
                            self.landing.remove(&url);
                        }
                        Some((Landing::Moved, _)) => {
                            self.landing.remove(&url);
                            let name = crate::story::pr_from_url(&url, "")
                                .map_or(url.clone(), |p| format!("{}#{}", p.repo, p.number));
                            self.toasts.push(super::Toast {
                                key: format!("land:{url}"),
                                line: format!("{name} got new commits: look, then Land it again"),
                                at: std::time::Instant::now(),
                                agent: false,
                            });
                        }
                        Some((Landing::Wait, _)) | None => {}
                    }
                }
            }
            Event::AutoMerged { url, result } => {
                self.merging.remove(&url);
                if let Err(error) = &result {
                    self.auto_merge_heads.remove(&url);
                    self.error = Some(format!("Merge when ready: {error}"));
                    self.send(Command::Gate { url });
                } else {
                    self.guard_auto_merge(&url, &result);
                    self.gates.insert(url, result);
                }
            }
            Event::Merged { url, result } => {
                self.merging.remove(&url);
                match result {
                    Ok(crate::github::Merged::Commit(sha)) => {
                        self.merged.insert(url, sha);
                        self.apply(Action::Refresh, ctx);
                    }
                    // In the queue: its gate says so, and the row reads merging.
                    Ok(crate::github::Merged::Queued(gate)) => {
                        self.gates.insert(url, Ok(*gate));
                        self.apply(Action::Refresh, ctx);
                    }
                    Err(error) => {
                        self.error = Some(format!("Merge: {error}"));
                        self.send(Command::Gate { url });
                    }
                }
            }
            Event::Quick {
                issue_id,
                hash,
                result,
            } => {
                self.quick_pending.remove(&issue_id);
                if let Ok(answers) = &result {
                    let shown = if self.settings.laya.on() == Some(true) {
                        crate::laya::shown(answers)
                    } else {
                        Vec::new()
                    };
                    let ticket = self
                        .card(&issue_id)
                        .map(|c| c.issue.identifier.clone())
                        .unwrap_or_default();
                    let path = crate::store::data_dir().join("laya-quick.jsonl");
                    for a in answers {
                        let _ = crate::laya::log(
                            &path,
                            &crate::laya::QuickLine {
                                ticket: ticket.clone(),
                                question: a.id.into(),
                                yes: a.yes,
                                confidence: a.confidence,
                                shown: shown.contains(&a.id),
                                clicked: false,
                            },
                        );
                    }
                }
                self.quick.insert(issue_id, (hash, result));
            }
            Event::Reviewed { url, fix, result } => {
                self.reviewing.remove(&url);
                match result {
                    // Kept on failure, so nothing the user wrote is lost.
                    Err(error) => self.error = Some(format!("Comment: {error}")),
                    Ok(()) => {
                        let comments = self
                            .drafts
                            .remove(&url)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|d| format!("{}:{}: {}", d.path, d.line, d.body))
                            .collect();
                        // The head the review was bound to: the diff's.
                        if let Some(head) = self.diff_heads.get(&url).cloned() {
                            self.remember_review(&url, head, comments);
                        }
                        // The posted comments show as threads on their lines.
                        self.send(Command::Talk { url: url.clone() });
                        if let Some(issue_id) = fix {
                            let skill = crate::profile::get().skill("fixComments");
                            self.apply(
                                Action::RunSkill {
                                    issue_id,
                                    command: format!("/{skill} {url}"),
                                    submit: true,
                                },
                                ctx,
                            );
                        }
                    }
                }
                self.send(Command::Gate { url });
            }
            Event::AiReviewAsked { url, head, result } => {
                self.ai_asking.remove(&url);
                match result {
                    Ok(()) => {
                        self.ai_reviews.insert(url, head);
                    }
                    Err(error) => self.error = Some(format!("AI review: {error}")),
                }
            }
            Event::Approved { url, result } => {
                match result {
                    Err(error) => {
                        self.approving.remove(&url);
                        self.error = Some(format!("Approve: {error}"));
                    }
                    // Approved at the head the gate showed: nothing open since.
                    Ok(()) => {
                        let head = self.gates.get(&url).and_then(|g| g.as_ref().ok());
                        if let Some(head) = head.map(|g| g.head.clone()) {
                            self.remember_review(&url, head, Vec::new());
                        }
                    }
                }
                // Held until GitHub's own word on it arrives: no flicker back.
                self.send(Command::Gate { url });
            }
            Event::LayaAsks { asked, result } => {
                self.asks_pending = false;
                match result {
                    Ok(answers) => {
                        for (key, yes, confidence) in answers {
                            if let Some((_, hash)) = asked.iter().find(|(k, _)| *k == key) {
                                self.session_asks.insert(key, (*hash, yes, confidence));
                            }
                        }
                    }
                    // Not asked again for these texts: a broken Laya stays quiet.
                    Err(_) => {
                        for (key, hash) in asked {
                            self.session_asks.insert(key, (hash, false, 0.0));
                        }
                    }
                }
            }
            Event::LoginFound { name, result } => {
                self.logins_pending.remove(&name);
                self.logins.insert(name, result.ok().flatten());
            }
            Event::Labeled { url, label, result } => {
                if let Err(error) = result {
                    self.take_back(&url);
                    self.labeled.remove(&(url, label));
                    self.error = Some(format!("Label: {error}"));
                }
            }
            Event::ReviewRequested { url, login, result } => {
                if let Err(error) = result {
                    self.take_back(&url);
                    self.review_requested.remove(&(url.clone(), login));
                    self.error = Some(format!("Request review: {error}"));
                }
                self.send(Command::Gate { url });
            }
            Event::Talk { url, result } => {
                self.talk.insert(url, result);
            }
            Event::PrReady { url, result } => {
                if let Err(error) = result {
                    self.error = Some(format!("Ready for review: {error}"));
                }
                self.send(Command::Gate { url });
            }
            Event::PrClosed { url, result } => {
                if let Err(error) = result {
                    self.closed.remove(&url);
                    self.error = Some(format!("Close PR: {error}"));
                }
                self.send(Command::Gate { url });
            }
            Event::BranchUpdated { url, result } => {
                if let Err(error) = result {
                    self.updating.remove(&url);
                    self.error = Some(format!("Update branch: {error}"));
                }
                self.send(Command::Gate { url });
            }
            Event::Diff { url, result } => {
                self.diff_pending.remove(&url);
                let loaded = result.is_ok();
                self.diffs.insert(url.clone(), result);
                if loaded && self.guide_after.remove(&url) {
                    self.apply(Action::Guide, ctx);
                }
                self.send(Command::Gate { url });
            }
            Event::ProfileImported { source, result } => {
                self.profile_importing = false;
                match result {
                    Ok(profile) => self.use_profile(source, *profile, ctx),
                    Err(error) => self.profile_error = Some(error),
                }
            }
            Event::LayaProgress(done, total) => self.laya_download = Some(Ok((done, total))),
            Event::UpdateFound(result) => match result {
                Ok(tag) => self.update = tag,
                Err(error) => self.update_error = Some(error),
            },
            // The new app is in place: it opens, this copy closes. Sessions
            // run on their own and the new copy attaches to them.
            Event::Updated(result) => {
                self.installing = false;
                match result {
                    Ok(app) => match std::process::Command::new("open")
                        .arg("-n")
                        .arg(&app)
                        .spawn()
                    {
                        Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(error) => {
                            self.update_error = Some(format!("Updated, reopen it: {error}"))
                        }
                    },
                    Err(error) => self.update_error = Some(error),
                }
            }
            Event::LayaDownloaded(result) => {
                self.laya_download = result.err().map(Err);
                self.laya_ready = crate::laya::downloaded(&crate::laya::dir());
            }
            Event::File { url, result } => {
                let image = result.and_then(|bytes| decode(&bytes)).map(|image| {
                    ctx.load_texture(url.clone(), image, egui::TextureOptions::LINEAR)
                });
                self.images.insert(url, image);
            }
            Event::Releases(releases) => self.releases = releases,
            Event::Alerts(result) => match result {
                Ok(alerts) => {
                    self.alerts = alerts;
                    self.alerts_error = None;
                }
                Err(error) => self.alerts_error = Some(error),
            },
            Event::Incidents(result) => match result {
                Ok(incidents) => {
                    self.incidents = incidents;
                    self.incidents_error = None;
                }
                Err(error) => self.incidents_error = Some(error),
            },
            Event::Inbox { linear, github } => {
                // A side that failed keeps what it last sent, so its rows
                // never flicker away and come back.
                let mut errors = Vec::new();
                let mut skipped = 0;
                for (source, result) in [
                    (crate::inbox::Source::Linear, linear),
                    (crate::inbox::Source::GitHub, github),
                ] {
                    match result {
                        Ok((notes, left_out)) => {
                            self.inbox_notes.retain(|n| n.source != source);
                            self.inbox_notes.extend(notes);
                            skipped += left_out;
                            if source == crate::inbox::Source::GitHub {
                                self.inbox_github = true;
                            }
                        }
                        Err(error) => errors.push(format!("{source:?}: {error}")),
                    }
                }
                self.inbox_skipped = skipped;
                self.inbox_answered = true;
                self.inbox_error = (!errors.is_empty()).then(|| errors.join(" · "));
                // Held until neither side sends them any more.
                let ids: HashSet<&str> = self.inbox_notes.iter().map(|n| n.id.as_str()).collect();
                self.inbox_done.retain(|id| ids.contains(id.as_str()));
            }
            Event::InboxRead(result) => {
                if let Err((ids, error)) = result {
                    for id in ids {
                        self.inbox_done.remove(&id);
                    }
                    self.inbox_error = Some(format!("could not mark read: {error}"));
                }
            }
            Event::Dispatched { identifier, result } => match result {
                Ok(()) => {
                    let owner = self
                        .missions
                        .iter()
                        .find(|m| {
                            !m.done
                                && self.looked_up.get(&m.parent).is_some_and(|p| {
                                    p.as_ref().is_ok_and(|p| {
                                        p.children.nodes.iter().any(|c| c.identifier == identifier)
                                    })
                                })
                        })
                        .map(|m| (m.id.clone(), m.parent.clone()));
                    let (id, parent) = owner.unwrap_or_default();
                    let line = match parent.as_str() {
                        "" => format!("{identifier} → Todo for the bot"),
                        parent => format!("{identifier} → Todo for the bot ({parent})"),
                    };
                    self.agent_says(&line);
                    // The bot's loop starts now, not at its next poll.
                    self.kick_bot();
                    let now = crate::activity::now_ms() as u64;
                    if let Ok(all) = crate::mission::update(&id, |i| i.log.push((now, line))) {
                        self.missions = all;
                    }
                    self.apply(Action::Refresh, ctx);
                }
                Err(error) => {
                    self.dispatching.remove(&identifier);
                    self.agent_says(&format!("could not hand {identifier} to the bot: {error}"));
                }
            },
            Event::Kicked(result) => {
                let bot = crate::profile::get().bot.name.clone();
                let bot = if bot.is_empty() {
                    "the bot".to_owned()
                } else {
                    bot
                };
                match result {
                    Ok(()) => self.agent_says(&format!("asked {bot} to look now")),
                    Err(error) => self.agent_says(&format!("could not wake {bot}: {error}")),
                }
            }
            Event::Looked { id, result } => {
                self.looked_up.insert(id, result.map(|i| *i));
            }
            // keep_fresh loads the board again on the next frame.
            Event::Wrote(true) => self.refreshed = None,
            Event::Wrote(false) => {}
            Event::Issue(result) => match result {
                Ok(issue) => {
                    let id = issue.id.clone();
                    self.fetched.retain(|i| i.id != id);
                    self.fetched.push(*issue);
                    self.rebuild();
                    self.apply(crate::app::Action::OpenTicket { id, split: false }, ctx);
                }
                Err(error) => self.error = Some(format!("could not open the ticket: {error}")),
            },
            Event::PrTicket { url, result } => match result {
                Ok(Some(issue)) => {
                    let id = issue.id.clone();
                    let pr_only = self
                        .pr_cards
                        .iter()
                        .find(|c| c.prs.iter().any(|p| p.url == url))
                        .map(|c| c.issue.id.clone());
                    self.pr_cards
                        .retain(|c| !c.prs.iter().any(|p| p.url == url));
                    self.fetched.retain(|i| i.id != id);
                    self.fetched.push(*issue);
                    self.rebuild();
                    // Still looking at the PR: the ticket opens in its place.
                    if pr_only.is_some() && self.selected == pr_only {
                        self.apply(crate::app::Action::OpenTicket { id, split: false }, ctx);
                    }
                }
                Ok(None) => {}
                Err(error) => self.error = Some(format!("could not find the PR's ticket: {error}")),
            },
            // Quiet on failure: gh not signed in leaves the lane empty.
            Event::DependabotPrs(Ok(prs)) => {
                story::take_dependencies(&mut self.pr_cards, &prs, self.selected.as_deref())
            }
            Event::DependabotPrs(Err(_)) => {}
            Event::Herdr(result) => {
                // herdr not running is no error: its agents are just gone.
                self.herdr_agents = result.unwrap_or_default();
            }
            Event::HerdrDone(result) => match result {
                Ok(note) if note.is_empty() => {}
                Ok(note) => self.herdr_note = Some(note),
                Err(error) => self.herdr_note = Some(error),
            },
            Event::TeamStats(result) => {
                self.team_stats = Some((std::time::Instant::now(), result));
            }
            Event::Reviewers { slug, result } => {
                self.reviewers.insert(slug, result);
            }
            Event::RepoPrs { slug, result } => {
                self.repo_prs
                    .insert(slug, (std::time::Instant::now(), result));
            }
            Event::MyPrs(result) => {
                // `orbit ctl prs` waits for this answer, empty or not.
                self.lookups_asked.insert("my-prs:in".into());
                match result {
                    Ok(prs) => self.my_prs = prs,
                    Err(error) => self.error = Some(format!("could not list your PRs: {error}")),
                }
            }
            Event::Found { term, result } => {
                // An answer for a query typed over since is dropped.
                let current = self.palette.as_ref().map(|p| p.query.trim().to_owned());
                if current.as_deref() == Some(term.as_str()) {
                    let error = result.as_ref().err().cloned();
                    self.palette_found = result.unwrap_or_default();
                    self.palette_answered = (term, error);
                }
            }
            Event::NoteRead(result) => {
                let mut create = false;
                if let Some(capture) = self.capture.as_mut() {
                    capture.busy = false;
                    match result {
                        Ok((intake, duplicates)) => {
                            // Something that just makes sense is created at once,
                            // unless it may be filed already.
                            create = intake.obvious && duplicates.is_empty();
                            capture.answers = vec![String::new(); intake.questions.len()];
                            capture.intake = Some(intake);
                            capture.duplicates = duplicates;
                        }
                        Err(error) => capture.error = Some(error),
                    }
                }
                if create {
                    self.apply(Action::CreateTicket, ctx);
                }
            }
            Event::ReportAdded(result) => {
                if let Some(capture) = self.capture.as_mut() {
                    capture.busy = false;
                    match result {
                        Ok(added) => {
                            capture.created = Some(added);
                            capture.added = true;
                        }
                        Err(error) => capture.error = Some(error),
                    }
                }
                self.spot_filed();
                self.apply(Action::Refresh, ctx);
            }
            Event::TicketCreated(result) => {
                if let Some(capture) = self.capture.as_mut() {
                    capture.busy = false;
                    match result {
                        Ok(created) => capture.created = Some(created),
                        Err(error) => capture.error = Some(error),
                    }
                }
                self.spot_filed();
                self.apply(Action::Refresh, ctx);
            }
            Event::TestPlan { issue_id, result } => {
                self.test_plan_pending.remove(&issue_id);
                let posted = matches!(result, Ok((_, true)));
                self.test_plans
                    .insert(issue_id, result.map(|(plan, _)| plan));
                // Show the new comment on the ticket's timeline.
                if posted {
                    self.apply(Action::Refresh, ctx);
                }
            }
            Event::Guided { url, result } => {
                self.guide_pending.remove(&url);
                self.guides.insert(url, result);
            }
            Event::ApproveNote { url, result } => {
                // Dropped while it was being written: stays dropped.
                if let Some(slot) = self.approve_notes.get_mut(&url) {
                    *slot = Some(result);
                }
            }
            Event::AnsweredPr { url, result } => {
                super::answer(self.pr_questions.entry(url).or_default(), result);
            }
            Event::Brief {
                fingerprint,
                result,
            } => self.on_brief(fingerprint, result),
            Event::Shortened { items, result } => self.on_shortened(items, result),
            Event::Customers { items, result } => self.on_customers(items, result),
            Event::Digests { items, result } => self.on_digests(items, result),
        }
    }

    fn on_loaded(&mut self, request: u64, result: Result<crate::linear::Home, String>) {
        if request < self.applied_request {
            return;
        }
        self.applied_request = request;
        self.loading = request < self.last_request;
        if !self.verified {
            match &result {
                Ok(_) => {
                    self.verified = true;
                    if let Some(key) = self.pending_key.take()
                        && let Err(error) = secrets::set(Secret::Linear, &key)
                    {
                        self.error = Some(format!(
                            "signed in, but the key could not be saved: {error}"
                        ));
                    }
                }
                Err(error) => {
                    // Back to the login screen, saying why.
                    self.backend = None;
                    self.pending_key = None;
                    self.loading = false;
                    self.sign_in_error = Some(format!("Linear did not accept this key: {error}"));
                    return;
                }
            }
        }
        let home = match result {
            Ok(home) => home,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.viewer = home.viewer;
        crate::story::set_org(&home.org);
        // Their own section of the profile, once we know who they are.
        // A simulated user (review.rs) may be someone else, read-only.
        let email = match std::env::var("ORBIT_SIM_EMAIL") {
            Ok(email) if crate::read_only() => email,
            _ => home.viewer_email.clone(),
        };
        if crate::profile::set_viewer(&email) {
            match crate::profile::load(&self.settings.profile) {
                Ok(profile) => {
                    crate::profile::set(profile);
                    self.settings = crate::store::load_settings();
                }
                Err(error) => self.profile_error = Some(format!("{error:#}")),
            }
        }
        self.my_teams = home.my_teams;
        // First run: choose teams before anything else.
        if self.teams.is_empty() && self.picking.is_none() {
            self.picking = Some(Vec::new());
        }
        if let Some(bot) = story::find_bot(&home.issues) {
            story::set_bot(&bot);
        }
        let mut teams: Vec<(String, String, String)> = home
            .teams
            .iter()
            .map(|t| (t.id.clone(), t.key.clone(), t.name.clone()))
            .collect();
        teams.sort_by(|a, b| a.1.cmp(&b.1));
        self.all_teams = teams;
        self.states = home
            .teams
            .into_iter()
            .map(|team| {
                let mut states = team.states.nodes;
                states.sort_by(|a, b| a.position.total_cmp(&b.position));
                (team.id, states)
            })
            .collect();
        self.server = home.issues;
        self.read_notes();
        self.settle(request);
        self.rebuild();
        self.loaded_at = Some(jiff::Timestamp::now().to_string());
        self.run_helpers();
        self.error = None;
        // The triage window stays without a ticket, a session's window (an
        // alert's, a free one) shows its session, and a shell window has no
        // ticket pane to show one in.
        if self.selected.is_none()
            && self.session.is_none()
            && self.tab != Tab::Triage
            && self.panes.ticket_active()
        {
            let first = self.for_you().first().map(|c| c.issue.id.clone());
            self.select_ticket(first);
        }
    }

    /// Merge when ready is bound to the head the user saw; GitHub keeps
    /// it on through new commits, so this copy turns it off when a gate
    /// shows another head.
    fn guard_auto_merge(&mut self, url: &str, result: &Result<crate::github::Gate, String>) {
        let (Ok(gate), Some(head)) = (result, self.auto_merge_heads.get(url)) else {
            return;
        };
        if crate::github::moved_past(head, gate) {
            self.auto_merge_heads.remove(url);
            self.error = Some("New commits after Merge when ready: turned it off".into());
            if self.merging.insert(url.to_owned()) {
                self.send(Command::AutoMerge {
                    url: url.to_owned(),
                    head: None,
                });
            }
        } else if !gate.auto_merge && !self.merging.contains(url) {
            // Merged, queued, or turned off on GitHub: nothing to watch.
            self.auto_merge_heads.remove(url);
        }
    }
}
