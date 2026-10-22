//! What the user does. Every view emits `Action`s; this applies them after
//! the frame is drawn.

use super::board::{Local, Shown};
use super::{Action, App, Tab, View, panes};
use crate::backend::{Backend, Command};
use crate::claude;
use crate::linear::{Comment, User};
use crate::secrets::{self, Secret};
use crate::store::{self, Settings};

/// How long a sub-ticket has had nothing: no state move (`moved`, unknown
/// counts as now) and no comment on its card.
pub(crate) fn quiet_ms(card: &crate::story::Card, moved: Option<u64>, now: u64) -> u64 {
    let last_word = card
        .issue
        .comments
        .nodes
        .iter()
        .filter_map(|c| c.created_at.parse::<jiff::Timestamp>().ok())
        .map(|t| t.as_millisecond().max(0) as u64)
        .max()
        .unwrap_or(0);
    now.saturating_sub(moved.unwrap_or(now).max(last_word))
}

impl App {
    /// The browser in `slot`: the Testing tab's, or a browser pane's.
    fn browser_in(&mut self, slot: u64) -> Option<&mut crate::browser::Browser> {
        if slot == crate::browser::TESTING {
            Some(&mut self.browser)
        } else {
            self.browsers.get_mut(&slot)
        }
    }
}

impl App {
    pub fn apply(&mut self, action: Action, ctx: &egui::Context) {
        // What opens in a pane lowers the sheet, so it is seen.
        if matches!(action, Action::OpenSession(_) | Action::Claude(_)) {
            self.sheet = false;
        }
        // A session opens in a window of its own, like a ticket: the one
        // showing it, else a new one. Triage finds its own.
        // The clone's sessions open in its panel, and a session in a
        // ticket's folder with that ticket: neither gets a window here.
        if let Action::OpenSession(key) = &action
            && key != claude::TRIAGE
            && !self.is_clone_session(key)
            && self.ticket_of_session(key).is_none()
        {
            self.session_window(key);
        }
        match action {
            Action::SignIn => {
                let key = self.key_input.trim().to_owned();
                if key.is_empty() {
                    return;
                }
                self.sign_in_error = None;
                self.verified = false;
                self.backend = Some(Backend::spawn(&key, ctx.clone()));
                // The old backend's answers never arrive; ask again.
                self.diff_pending.clear();
                self.pending_key = Some(key);
                self.key_input.clear();
                self.apply(Action::Refresh, ctx);
            }
            Action::SignInWithLinear => {
                self.sign_in_error = None;
                self.oauth = Some(crate::oauth::sign_in(ctx.clone()));
            }
            Action::SignOut => {
                if let Err(error) = secrets::clear(Secret::Linear) {
                    self.error = Some(format!("could not remove the Linear key: {error}"));
                }
                self.backend = None;
                self.verified = false;
                self.pending_key = None;
                self.server.clear();
                self.cards.clear();
                self.viewer.clear();
                self.select_ticket(None);
                self.loaded_at = None;
                self.error = None;
            }
            Action::SaveKey(secret) => {
                // The one place keys are typed: Settings, connections.
                let key = self
                    .key_drafts
                    .remove(secrets::name(secret))
                    .unwrap_or_default();
                if key.trim().is_empty() {
                    return;
                }
                if let Err(error) = self.set_key(secret, key.trim().to_owned()) {
                    self.settings_error = Some(error);
                }
                // settings.json shows it as SAVED now.
                let settings = store::load_settings();
                self.show_settings(settings);
            }
            Action::PickTeams => self.picking = Some(self.teams.clone()),
            Action::ToggleTeam(key) => {
                if let Some(picked) = &mut self.picking {
                    match picked.iter().position(|k| *k == key) {
                        Some(i) => {
                            picked.remove(i);
                        }
                        None => picked.push(key),
                    }
                }
            }
            Action::SaveTeams => {
                let Some(picked) = self.picking.take() else {
                    return;
                };
                let first = self.teams.is_empty();
                self.teams = picked;
                let teams = self.teams.clone();
                self.save_settings(|s| s.teams = teams);
                self.apply(Action::Refresh, ctx);
                if first {
                    self.apply(Action::SetUpProfile, ctx);
                }
            }
            Action::Tutorial(Some(i)) => {
                let Some(step) = crate::ui::tutorial::STEPS.get(i) else {
                    return self.apply(Action::Tutorial(None), ctx);
                };
                if self.tutorial.is_none() {
                    self.tutorial_return = Some((self.view.clone(), self.sheet));
                }
                self.tutorial = Some(i);
                // A step with a view shows it in the sheet (Settings in the
                // pane area); one without shows the panes.
                match step.view.clone() {
                    Some(view) => self.apply(Action::Show(view), ctx),
                    None => self.apply(Action::Sheet(false), ctx),
                }
                // The ticket steps need a ticket on screen.
                if self.selected.is_none() {
                    let first = self.for_you().first().map(|c| c.issue.id.clone());
                    self.select_ticket(first);
                }
                // The ticket steps point into the walkthrough.
                if matches!(self.tab, Tab::Claude | Tab::Diff) {
                    self.tab = Tab::Walkthrough;
                }
            }
            Action::Tutorial(None) => {
                self.tutorial = None;
                if let Some((view, sheet)) = self.tutorial_return.take() {
                    self.view = view;
                    self.sheet = sheet;
                }
            }
            Action::SetTheme(choice) => {
                self.theme = choice;
                crate::theme::follow(ctx, choice);
                self.save_settings(|s| s.theme = choice);
            }
            Action::SetLocalAgent(agent) => self.save_settings(|s| s.local_agent = agent),
            Action::WithAgent(agent, action) => {
                self.claude.launch = Some(agent);
                self.apply(*action, ctx);
                self.claude.launch = None;
            }
            Action::Refresh => {
                self.load_gate();
                self.last_request += 1;
                self.loading = true;
                self.refreshed = Some(std::time::Instant::now());
                self.send(Command::Refresh {
                    teams: self.teams.clone(),
                    request: self.last_request,
                });
            }
            Action::Show(view) => {
                // An old error stays with the view it happened in.
                self.error = None;
                self.claude.error = None;
                if view != self.view {
                    self.sheet_at = (0, 0);
                    self.sheet_query.clear();
                }
                // A repo page asks GitHub for its PRs when shown, again
                // after a minute; the Repos view needs the user's PRs.
                if let View::Repo(slug) = &view
                    && self
                        .repo_prs
                        .get(slug)
                        .is_none_or(|(at, _)| at.elapsed() > std::time::Duration::from_secs(60))
                {
                    self.send(Command::RepoPrs(slug.clone()));
                }
                // Stats asks SigNoz when shown, again after ten minutes.
                let stats = &crate::profile::get().stats;
                if view == View::Stats
                    && !stats.mcp.is_empty()
                    && self
                        .team_stats
                        .as_ref()
                        .is_none_or(|(at, _)| at.elapsed() > std::time::Duration::from_secs(600))
                {
                    self.send(Command::TeamStats {
                        mcp: stats.mcp.clone(),
                        repos: stats.repos.clone(),
                    });
                }
                if matches!(view, View::Repos | View::Repo(_)) && self.my_prs.is_empty() {
                    self.send(Command::MyPrs);
                }
                // The sheet only browses: what the panes show stays.
                self.sheet = view != View::Settings;
                self.view = view;
            }
            Action::Sheet(open) => {
                self.sheet = open;
                // Closing leaves Settings too: back to the panes.
                if self.view == View::Settings {
                    self.view = View::ForYou;
                    self.sheet_at = (0, 0);
                }
                if open {
                    // The keys go to the sheet, not a terminal under it.
                    ctx.memory_mut(|m| m.stop_text_input());
                }
            }
            Action::SheetAt(column, row) => self.sheet_at = (column, row),
            Action::OpenTicket { id, split } => {
                self.sheet = false;
                // A window that shows it already comes to the front instead:
                // a ticket is in the sidebar once, a split never repeats it.
                if let Some((window, pane)) = self.window_showing(&id) {
                    self.apply(Action::GoWindow(window), ctx);
                    self.focus_pane(pane);
                    return;
                }
                if split {
                    self.apply(Action::Split { across: true }, ctx);
                } else {
                    self.stash_window();
                    self.windows.push(panes::Panes::new(panes::Surface::Ticket(
                        self.ticket_view(),
                    )));
                    self.enter_window(self.windows.len() - 1);
                }
                self.tab = Tab::Walkthrough;
                self.session = None;
                self.apply(Action::Select(id), ctx);
            }
            Action::NextForYou => {
                let ids: Vec<String> = self.for_you().iter().map(|c| c.issue.id.clone()).collect();
                let at = self
                    .selected
                    .as_ref()
                    .and_then(|s| ids.iter().position(|id| id == s));
                let next = at.map_or(0, |i| i + 1);
                if let Some(id) = ids.get(next.min(ids.len().saturating_sub(1))).cloned() {
                    self.apply(Action::Focus(id), ctx);
                }
            }
            Action::GoWindow(i) => {
                if i != self.window && i < self.windows.len() {
                    self.stash_window();
                    self.enter_window(i);
                }
            }
            Action::Select(issue_id) => {
                // Shown in another pane or window: that one comes forward,
                // so no ticket is ever open twice.
                if let Some((window, pane)) = self
                    .window_showing(&issue_id)
                    .filter(|&(w, p)| (w, p) != (self.window, self.panes.active))
                {
                    self.apply(Action::GoWindow(window), ctx);
                    self.focus_pane(pane);
                    return;
                }
                if self.selected.as_ref() != Some(&issue_id) {
                    self.draft.clear();
                }
                self.select_ticket(Some(issue_id.clone()));
                self.session = own_session(self.session.take(), &issue_id);
                self.load_gate();
                match self.tab {
                    // Picking a ticket while triaging shows the ticket; the
                    // triage session keeps running behind its tab.
                    Tab::Triage => self.tab = Tab::Walkthrough,
                    Tab::Diff => self.guide_step = None,
                    // Coming back to a ticket whose session is running
                    // reattaches at once; a stopped one waits for a click.
                    Tab::Claude => {
                        self.session = Some(issue_id.clone());
                        if self.claude.live_of(&issue_id).is_some()
                            && let Some(card) = self.card(&issue_id)
                        {
                            let spec = claude::ticket(card, &self.viewer, self.skill_files(), None);
                            self.claude.open(spec, ctx);
                        }
                    }
                    // Every ticket has one; a PR alone has no ticket to hold the plan.
                    Tab::Testing => {
                        if !self
                            .card(&issue_id)
                            .is_some_and(crate::story::Card::has_testing)
                        {
                            self.tab = Tab::Walkthrough;
                        }
                    }
                    Tab::Walkthrough => {}
                }
            }
            Action::LoadImage(url) => {
                if self.images_asked.insert(url.clone()) {
                    self.send(Command::File { url });
                }
            }
            Action::BrowserGo(slot, url) => {
                if let Some(browser) = self.browser_in(slot) {
                    browser.go(&url);
                }
            }
            Action::BrowserBack(slot) => {
                if let Some(browser) = self.browser_in(slot) {
                    browser.back();
                }
            }
            Action::BrowserReload(slot) => {
                if let Some(browser) = self.browser_in(slot) {
                    browser.reload();
                }
            }
            Action::BrowserFindOpen(slot) => {
                if let Some(browser) = self.browser_in(slot) {
                    browser.open_find();
                }
            }
            Action::BrowserFindClose(slot) => {
                if let Some(browser) = self.browser_in(slot) {
                    browser.finding = false;
                }
            }
            Action::BrowserFind {
                slot,
                text,
                backwards,
            } => {
                if let Some(browser) = self.browser_in(slot) {
                    browser.find(&text, backwards);
                }
            }
            Action::TestPlan(issue_id) => {
                if self.test_plan_pending.contains(&issue_id)
                    || matches!(self.test_plans.get(&issue_id), Some(Ok(_)))
                {
                    return;
                }
                let Some(card) = self.card(&issue_id).cloned() else {
                    return;
                };
                // A plan already on the ticket is read, not written again.
                let marker = crate::testplan::marker(&issue_id);
                let on_ticket = card
                    .issue
                    .comments
                    .nodes
                    .iter()
                    .filter(|c| c.body.contains(&marker))
                    .find_map(|c| crate::testplan::from_comment(&c.body));
                if let Some(plan) = on_ticket {
                    self.test_plans.insert(issue_id, Ok(plan));
                    return;
                }
                let Some(key) = self.anthropic_key.clone() else {
                    let why = "Add an Anthropic key in Settings, connections, and Claude writes the test plan.";
                    self.test_plans.insert(issue_id, Err(why.into()));
                    return;
                };
                let context = format!(
                    "Ticket {}: {}\n\nDescription:\n{}\n\nPlan:\n{}",
                    card.issue.identifier,
                    card.issue.title,
                    crate::testplan::clip(
                        card.issue.description.as_deref().unwrap_or("none"),
                        6000
                    ),
                    card.plan
                        .as_ref()
                        .map_or("none".to_owned(), |p| crate::testplan::clip(&p.body, 6000)),
                );
                // Every PR that is or was going in, for its diff.
                let prs = card
                    .prs
                    .into_iter()
                    .filter(|p| p.open || p.merged)
                    .map(|p| (p.owner, p.repo, p.number))
                    .collect();
                self.test_plans.remove(&issue_id);
                self.test_plan_pending.insert(issue_id.clone());
                self.send(Command::TestPlan {
                    issue_id,
                    key,
                    context,
                    prs,
                    token: self.github_token.clone(),
                });
            }
            Action::Focus(identifier) => {
                // By identifier (the palette, the brief) or by id (Alt+Down).
                let Some(id) = self
                    .cards
                    .iter()
                    .find(|c| c.issue.identifier == identifier || c.issue.id == identifier)
                    .map(|c| c.issue.id.clone())
                else {
                    return;
                };
                // Only an empty ticket pane takes it: a pane showing a ticket
                // or a session (an incident's, a landing), a named window and
                // a window of shells keep theirs, and the ticket goes to the
                // window showing it or a new one.
                let ticket_pane = matches!(
                    self.panes.surface(self.panes.active),
                    Some(panes::Surface::Ticket(_))
                );
                if !ticket_pane
                    || self.panes.title.is_some()
                    || !takes_a_ticket(&self.ticket_view())
                {
                    return self.apply(Action::OpenTicket { id, split: false }, ctx);
                }
                self.sheet = false;
                self.tab = Tab::Walkthrough;
                self.apply(Action::Select(id), ctx);
            }
            Action::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            // A session link a session printed (orbit ctl gives them).
            Action::Link(url) if url.starts_with("orbit://session/") => {
                let key = url.trim_start_matches("orbit://session/").to_owned();
                self.apply(Action::OpenSession(key), ctx);
            }
            Action::Link(url) => {
                if let Some(pr) = crate::story::pr_from_url(&url, "") {
                    let url = format!(
                        "https://github.com/{}/{}/pull/{}",
                        pr.owner, pr.repo, pr.number
                    );
                    let owner = self.cards.iter().find(|c| {
                        c.prs.iter().any(|p| {
                            (&p.owner, &p.repo, p.number) == (&pr.owner, &pr.repo, pr.number)
                        })
                    });
                    match owner.map(|c| c.issue.id.clone()) {
                        Some(id) => self.apply(Action::OpenTicket { id, split: false }, ctx),
                        None => {
                            let title = format!("{}#{}", pr.repo, pr.number);
                            let at = jiff::Timestamp::now().to_string();
                            self.apply(
                                Action::OpenPr {
                                    url: url.clone(),
                                    title,
                                    at,
                                },
                                ctx,
                            );
                        }
                    }
                    self.tab = Tab::Diff;
                    self.apply(Action::DiffPr(url), ctx);
                    return;
                }
                let ident = crate::story::issue_from_url(&url);
                let ticket = ident.as_ref().and_then(|ident| {
                    self.cards
                        .iter()
                        .find(|c| {
                            &c.issue.identifier == ident
                                || c.issue.previous_identifiers.contains(ident)
                        })
                        .map(|c| c.issue.id.clone())
                });
                match (ticket, ident) {
                    (Some(id), _) => self.apply(Action::OpenTicket { id, split: false }, ctx),
                    // Not on the board: fetched from Linear, then opened here.
                    (None, Some(ident)) => self.send(Command::Issue { id: ident }),
                    (None, None) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                }
            }
            Action::InboxOpen(key) => {
                if let Some(item) = self.read_inbox(&key) {
                    let ticket = item
                        .issue
                        .clone()
                        .filter(|id| self.cards.iter().any(|c| &c.issue.id == id));
                    match (ticket, item.pr) {
                        (Some(id), _) => self.apply(Action::OpenTicket { id, split: false }, ctx),
                        // A ticket not on the board: fetched, then opened here.
                        (None, _) if item.issue.is_some() => self.send(Command::Issue {
                            id: item.issue.unwrap_or_default(),
                        }),
                        (None, Some(url)) => {
                            let (title, at) = (item.title, item.at);
                            self.apply(Action::OpenPr { url, title, at }, ctx);
                        }
                        (None, None) => ctx.open_url(egui::OpenUrl::new_tab(item.url)),
                    }
                }
            }
            Action::OpenPr { url, title, at } => match crate::story::pr_card(&url, &title, &at) {
                Some(card) => {
                    let id = card.issue.id.clone();
                    if self.card(&id).is_none() {
                        self.pr_cards.push(card);
                        // Its ticket, when Linear has one off the board,
                        // replaces it (`Event::PrTicket`).
                        self.send(Command::PrTicket { url });
                    }
                    self.apply(Action::OpenTicket { id, split: false }, ctx);
                }
                None => ctx.open_url(egui::OpenUrl::new_tab(url)),
            },
            Action::InboxDone(key) => {
                self.read_inbox(&key);
            }
            Action::MarkRead(id) => {
                if let Some(card) = self.cards.iter().find(|c| c.issue.id == id) {
                    // Another copy may have marked some since: theirs stay.
                    let mut read: std::collections::HashMap<String, u64> = store::load("read.json");
                    read.insert(id, crate::story::read_key(card));
                    let _ = store::save("read.json", &read);
                    self.read = read;
                }
            }
            Action::Tab(Tab::Claude) => {
                if let Some(id) = self.selected.clone() {
                    self.apply(Action::Claude(id), ctx);
                }
            }
            Action::Tab(Tab::Triage) => {
                self.tab = Tab::Triage;
                // A running triage session reattaches at once.
                if self.claude.live_of(claude::TRIAGE).is_some() {
                    self.show_triage(ctx);
                }
            }
            Action::Tab(tab) => self.tab = tab,
            Action::ReloadDiff => self.reload_diff(),
            Action::ReviewAgain(url) => {
                let Some(pr) = self
                    .selected
                    .as_deref()
                    .and_then(|id| self.card(id))
                    .and_then(|c| c.prs.iter().find(|p| p.url == url))
                    .cloned()
                else {
                    return;
                };
                // Drafts point at lines of the old diff; they go with it.
                self.drafts.remove(&url);
                self.guides.remove(&url);
                self.guide_step = None;
                self.diff_pr = Some(url.clone());
                self.tab = Tab::Diff;
                if self.has_anthropic_key() {
                    self.guide_after.insert(url);
                }
                self.fetch_diff(pr);
            }
            Action::DiffPr(url) => self.diff_pr = Some(url),
            Action::AutoMerge { url, head } => {
                if self.merging.insert(url.clone()) {
                    self.auto_merge_heads.insert(url.clone(), head.clone());
                    self.send(Command::AutoMerge {
                        url,
                        head: Some(head),
                    });
                }
            }
            Action::CancelAutoMerge(url) => {
                if self.merging.insert(url.clone()) {
                    self.auto_merge_heads.remove(&url);
                    self.send(Command::AutoMerge { url, head: None });
                }
            }
            Action::Merge { url, head } => {
                // The button hides at once; no second merge while one runs.
                if self.merging.insert(url.clone()) {
                    self.gates.remove(&url);
                    self.send(Command::Merge { url, head });
                }
            }
            Action::Quick {
                ticket,
                question,
                then,
            } => {
                let _ = crate::laya::log(
                    &store::data_dir().join("laya-quick.jsonl"),
                    &crate::laya::QuickLine {
                        ticket,
                        question,
                        clicked: true,
                        ..Default::default()
                    },
                );
                self.apply(*then, ctx);
            }
            Action::AiReview { url, head } => {
                let how = crate::profile::get().github.ai_review.clone();
                if self.ai_asking.insert(url.clone()) {
                    self.send(Command::AiReview { url, head, how });
                }
            }
            Action::Approve { url, head, body } => {
                // Shown as approved at once; GitHub's answer follows.
                if self.approving.insert(url.clone()) {
                    self.approve_notes.remove(&url);
                    self.send(Command::Approve { url, head, body });
                }
            }
            Action::NoteForApproval(url) => {
                let Some(key) = self.anthropic_key.clone() else {
                    return;
                };
                let asked: Vec<(String, String)> = self
                    .pr_questions
                    .get(&url)
                    .into_iter()
                    .flatten()
                    .filter_map(|(q, a)| match a {
                        Some(Ok(a)) => Some((q.clone(), a.clone())),
                        _ => None,
                    })
                    .collect();
                if asked.is_empty() || matches!(self.approve_notes.get(&url), Some(None)) {
                    return;
                }
                self.approve_notes.insert(url.clone(), None);
                self.send(Command::ApproveNote { url, key, asked });
            }
            Action::SetApprovalNote { url, note } => match note {
                Some(note) => {
                    self.approve_notes.insert(url, Some(Ok(note)));
                }
                None => {
                    self.approve_notes.remove(&url);
                }
            },
            Action::ClosePr { url, cancel } => {
                // Closed at once; an error takes it back.
                if self.closed.insert(url.clone()) {
                    self.send(Command::ClosePr { url });
                }
                if let Some((issue_id, state_id)) = cancel {
                    self.apply(Action::SetState { issue_id, state_id }, ctx);
                }
            }
            Action::UpdateBranch { url, head } => {
                // Shown as updating at once, until GitHub shows a new head.
                if self.updating.get(&url) != Some(&head) {
                    self.updating.insert(url.clone(), head.clone());
                    self.send(Command::UpdateBranch { url, head });
                }
            }
            Action::GuideStep(step) => self.guide_step = step,
            Action::Draft { url, draft } => toggle(self.drafts.entry(url).or_default(), draft),
            Action::DiscardDrafts(url) => {
                self.drafts.remove(&url);
            }
            Action::Review {
                url,
                head,
                fix,
                changes,
            } => {
                let drafts = self.drafts.get(&url).cloned().unwrap_or_default();
                if !drafts.is_empty() && self.reviewing.insert(url.clone()) {
                    self.send(Command::Review {
                        url,
                        head,
                        drafts,
                        body: String::new(),
                        fix,
                        changes,
                    });
                }
            }
            Action::ReviewSummary {
                url,
                head,
                body,
                changes,
            } => {
                let drafts = self.drafts.get(&url).cloned().unwrap_or_default();
                let empty = body.trim().is_empty();
                if (changes && empty) || (empty && drafts.is_empty()) {
                    return;
                }
                if self.reviewing.insert(url.clone()) {
                    self.send(Command::Review {
                        url,
                        head,
                        drafts,
                        body,
                        fix: None,
                        changes,
                    });
                }
            }
            Action::ReadyPr(url) => {
                if self.readying.insert(url.clone()) {
                    self.send(Command::ReadyPr { url });
                }
            }
            Action::NewTab => {
                self.stash_window();
                self.windows
                    .push(panes::Panes::new(panes::Surface::Ticket(empty_view())));
                self.enter_window(self.windows.len() - 1);
                self.sheet = false;
            }
            Action::RenameWindow(i, name) if i < self.windows.len() => {
                let title = Some(name).filter(|n| !n.is_empty());
                if i == self.window {
                    self.panes.title = title.clone();
                }
                self.windows[i].title = title;
            }
            Action::RenameWindow(..) => {}
            Action::MoveWindow { from, to }
                if from < self.windows.len() && to < self.windows.len() =>
            {
                self.stash_window();
                let current = self.window;
                let window = self.windows.remove(from);
                self.windows.insert(to, window);
                self.window = moved_index(current, from, to);
                self.panes = self.windows[self.window].clone();
            }
            Action::MoveWindow { .. } => {}
            Action::Guide => {
                let Some(card) = self.selected.as_deref().and_then(|id| self.card(id)) else {
                    return;
                };
                let (Some(pr), Some(key)) = (
                    card.pr(self.diff_pr.as_deref()).cloned(),
                    self.anthropic_key.clone(),
                ) else {
                    return;
                };
                let Some(Ok(files)) = self.diffs.get(&pr.url) else {
                    return;
                };
                let context = pr_context(card);
                let diff = crate::github::numbered(files, 60_000);
                let since = self
                    .since_review(&pr.url)
                    .map(|(r, now)| crate::backend::Since {
                        owner: pr.owner.clone(),
                        repo: pr.repo.clone(),
                        number: pr.number,
                        token: self.github_token.clone(),
                        base: r.head,
                        head: now.to_owned(),
                        comments: r.comments,
                    });
                self.guides.remove(&pr.url);
                self.guide_pending.insert(pr.url.clone());
                self.guide_step = Some((pr.url.clone(), 0));
                self.send(Command::Guide {
                    url: pr.url,
                    key,
                    context,
                    diff,
                    since,
                });
            }
            Action::AskPr(question) => {
                let question = question.trim().to_owned();
                let Some(card) = self.selected.as_deref().and_then(|id| self.card(id)) else {
                    return;
                };
                let (Some(pr), Some(key)) = (
                    card.pr(self.diff_pr.as_deref()).cloned(),
                    self.anthropic_key.clone(),
                ) else {
                    return;
                };
                let Some(Ok(files)) = self.diffs.get(&pr.url) else {
                    return;
                };
                let asked = self.pr_questions.get(&pr.url);
                // One at a time: an answer lands on the question waiting for it.
                if question.is_empty() || asked.is_some_and(|a| a.iter().any(|(_, a)| a.is_none()))
                {
                    return;
                }
                let mut context = pr_context(card);
                // Where they are in the walkthrough, and what they asked before.
                let step = self
                    .guide_step
                    .as_ref()
                    .filter(|(of, _)| of == &pr.url)
                    .and_then(|(_, i)| match self.guides.get(&pr.url) {
                        Some(Ok(guide)) => guide.steps.get(*i),
                        _ => None,
                    });
                if let Some(step) = step {
                    context.push_str(&format!(
                        "\n\nThe step they are on: {}. {}",
                        step.title, step.check
                    ));
                }
                for (q, a) in asked.into_iter().flatten() {
                    if let Some(Ok(a)) = a {
                        context.push_str(&format!("\n\nThey asked: {q}\nYou answered: {a}"));
                    }
                }
                let diff = crate::github::numbered(files, 60_000);
                self.pr_questions
                    .entry(pr.url.clone())
                    .or_default()
                    .push((question.clone(), None));
                self.send(Command::AskPr {
                    url: pr.url,
                    key,
                    context,
                    diff,
                    question,
                });
            }
            Action::Claude(issue_id) => {
                self.select_ticket(Some(issue_id.clone()));
                self.session = Some(issue_id.clone());
                self.tab = Tab::Claude;
                self.apply(Action::Attach(issue_id), ctx);
            }
            Action::Triage => {
                // Continue means run the skill again: an existing
                // conversation gets it typed in (a resume sends no prompt).
                let spec = claude::triage(&self.teams, self.skill_files());
                if (self.claude.live_of(claude::TRIAGE).is_some()
                    || self.claude.resumable(claude::TRIAGE))
                    && let Some(prompt) = spec.prompt
                {
                    self.claude.type_line(claude::TRIAGE, prompt, true);
                }
                self.show_triage(ctx);
            }
            Action::AskBot { issue_id, command } => {
                // Only the profile's exact command, nothing else (story.rs).
                let Some(comment) = command.comment() else {
                    return;
                };
                self.asked_bot.insert(
                    issue_id.clone(),
                    (command, jiff::Timestamp::now().to_string()),
                );
                self.post_comment(issue_id, comment);
                // The poller reads it now, not on its next round.
                self.kick_bot();
            }
            Action::Yolo(issue_id) => {
                let Some(card) = self.card(&issue_id) else {
                    return;
                };
                let Some(bot) = crate::profile::get()
                    .orchestrator
                    .as_ref()
                    .map(|o| o.team.clone())
                else {
                    return;
                };
                let identifier = card.issue.identifier.clone();
                // Shown in the bot's Todo at once when its states are known;
                // Linear's answer confirms it (board.rs).
                let todo = self
                    .all_teams
                    .iter()
                    .find(|(_, key, _)| *key == bot)
                    .and_then(|(id, ..)| {
                        let states = self.states.get(id)?;
                        let todo = states
                            .iter()
                            .find(|s| s.name == "Todo")
                            .or_else(|| crate::story::state_like(states, "unstarted"))?;
                        Some((id.clone(), todo.id.clone()))
                    });
                if let Some((team_id, state_id)) = todo {
                    self.shown.insert(
                        issue_id.clone(),
                        Shown {
                            state_id,
                            team_id: Some(team_id),
                            confirmed_by: None,
                        },
                    );
                    self.rebuild();
                }
                if self.dispatching.insert(identifier.clone()) {
                    self.send(Command::Dispatch {
                        identifier,
                        team: bot,
                        state: "Todo".into(),
                    });
                }
            }
            Action::RefreshGate(url) => self.send(Command::Gate { url }),
            Action::WakeBot => {
                // The user asked: no two-minute rest.
                self.kicked_at = None;
                self.kick_bot();
                if self.kicked_at.is_some() {
                    let bot = crate::profile::get().bot_name("the bot").to_owned();
                    self.agent_says(&format!("{bot}: break is over, WORK!"));
                }
            }
            Action::NewTicket => {
                self.sheet = false;
                if self.capture.is_none() {
                    self.capture = Some(Default::default());
                }
            }
            Action::NewTicketWith(note) => {
                self.capture = Some(crate::app::Capture {
                    note,
                    ..Default::default()
                });
            }
            Action::Find => self.find.get_or_insert_default().focus = true,
            Action::TogglePalette => {
                self.palette = match self.palette {
                    Some(_) => None,
                    None => Some(Default::default()),
                };
                self.palette_found.clear();
                if self.palette.is_some() {
                    self.send(Command::MyPrs);
                    // A PR session titled only by its number: its gate
                    // brings the PR's title (App::session_name).
                    let bare: Vec<String> = self
                        .claude
                        .saved
                        .iter()
                        .filter(|(_, s)| s.title.trim() == s.identifier)
                        .filter_map(|(key, _)| crate::story::pr_url_of(key))
                        .filter(|url| !self.gates.contains_key(url))
                        .collect();
                    for url in bare {
                        self.send(Command::Gate { url });
                    }
                }
            }
            Action::DismissToast(key) => self.toasts.retain(|t| t.key != key),
            Action::HerdrFocus(target) => self.send(Command::HerdrFocus(target)),
            Action::HerdrStart(id) => {
                let Some(spec) = self.spec_for(&id) else {
                    self.herdr_note = Some("No session to start for that ticket.".into());
                    return;
                };
                match self.claude.herdr_launch(&spec) {
                    Ok((cwd, args)) => self.send(Command::HerdrStart {
                        name: crate::herdr::agent_name(&spec.identifier),
                        label: spec.identifier.clone(),
                        cwd,
                        args,
                    }),
                    Err(error) => self.herdr_note = Some(format!("{error:#}")),
                }
            }
            Action::UseHerdrConfig => {
                self.herdr_config = crate::herdr::config();
                let Some(config) = self.herdr_config.clone() else {
                    self.herdr_note = Some("No herdr config found.".into());
                    return;
                };
                let theme = crate::herdr::theme(&config.theme);
                self.theme = theme;
                crate::theme::follow(ctx, theme);
                self.save_settings(|s| {
                    s.herdr = Some(true);
                    s.theme = theme;
                    s.notifications = Some(config.toasts != "off");
                    s.sound = Some(config.sound);
                    s.done_sound = config
                        .done_sound
                        .as_ref()
                        .map_or(String::new(), |p| p.display().to_string());
                    s.request_sound = config
                        .request_sound
                        .as_ref()
                        .map_or(String::new(), |p| p.display().to_string());
                });
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("sidebar-sort"), u8::from(config.needs_first));
                    d.insert_temp(egui::Id::new("sidebar-collapsed"), config.start_collapsed);
                });
                self.herdr_note = Some(format!(
                    "Took herdr's theme ({}), notifications, sounds, sidebar and keys from {}",
                    config.theme,
                    config.path.display()
                ));
            }
            Action::SetHerdrKeys(on) => self.save_settings(|s| s.herdr = Some(on)),
            Action::InstallHerdrPlugin => self.send(Command::HerdrPlugin),
            Action::SetSound(on) => self.save_settings(|s| s.sound = Some(on)),
            Action::SessionsFiltered(query) => {
                self.apply(Action::Show(View::Sessions), ctx);
                self.sheet_query = query;
            }
            Action::SheetQuery(query) => {
                self.sheet_query = query;
                self.sheet_at = (0, 0);
            }
            Action::RefreshRepo(slug) => {
                self.repo_prs.remove(&slug);
                self.send(Command::RepoPrs(slug));
            }
            Action::PaletteQuery(query) => {
                if self.palette.is_none() {
                    self.apply(Action::TogglePalette, ctx);
                }
                if let Some(palette) = self.palette.as_mut() {
                    palette.query = query;
                    palette.selected = 0;
                }
            }
            Action::CloseCapture => self.capture = None,
            Action::ReadNote => {
                let key = self.anthropic_key.clone();
                let Some(capture) = self.capture.as_mut().filter(|c| !c.busy) else {
                    return;
                };
                let Some(key) = key else {
                    capture.error =
                        Some("Add an Anthropic key in Settings, connections, first.".into());
                    return;
                };
                if capture.note.trim().is_empty() {
                    return;
                }
                capture.busy = true;
                capture.error = None;
                let note = capture.note.clone();
                // A Work spot is read already: only the duplicate check.
                let known = capture.intake.clone().filter(|_| capture.spot.is_some());
                let laya = self
                    .settings
                    .laya
                    .on()
                    .map(|use_it| (use_it, self.settings.laya.min_confidence));
                let mut watched: Vec<(String, String)> = self
                    .my_teams
                    .iter()
                    .filter(|(key, _)| self.teams.contains(key))
                    .cloned()
                    .collect();
                if watched.is_empty() {
                    watched = self.teams.iter().map(|k| (k.clone(), k.clone())).collect();
                }
                let teams = crate::capture::teams_text(&watched);
                self.send(Command::ReadNote {
                    key,
                    note,
                    laya,
                    teams,
                    known,
                });
            }
            Action::AddToDuplicate(i) => {
                let reporter = self.viewer.clone();
                let Some(capture) = self.capture.as_mut().filter(|c| !c.busy) else {
                    return;
                };
                let (Some(duplicate), Some(intake)) =
                    (capture.duplicates.get(i).cloned(), capture.intake.as_ref())
                else {
                    return;
                };
                let answers: Vec<(String, String)> = intake
                    .questions
                    .iter()
                    .cloned()
                    .zip(capture.answers.iter().cloned())
                    .collect();
                let body = crate::capture::report_comment(&reporter, &capture.note, &answers);
                capture.busy = true;
                capture.error = None;
                self.send(Command::AddReport {
                    issue_id: duplicate.id,
                    identifier: duplicate.identifier,
                    url: duplicate.url,
                    body,
                });
            }
            Action::CreateTicket => {
                let key = self.anthropic_key.clone();
                let Some(capture) = self.capture.as_mut().filter(|c| !c.busy) else {
                    return;
                };
                let Some(intake) = capture.intake.clone() else {
                    return;
                };
                capture.busy = true;
                capture.error = None;
                let answers = intake
                    .questions
                    .iter()
                    .cloned()
                    .zip(capture.answers.iter().cloned())
                    .collect();
                let note = capture.note.clone();
                let description = capture.description.clone();
                self.send(Command::CreateTicket {
                    key,
                    note,
                    intake,
                    answers,
                    description,
                });
            }
            Action::FindGoals => {
                let spec = claude::goals(&self.priorities_hint(), self.skill_files());
                self.run_fresh(spec, ctx);
            }
            Action::Orchestrate => match claude::orchestrator(self.skill_files(), true) {
                // Each run is fresh: the board is the memory, not the last
                // conversation.
                Some(spec) => self.run_fresh(spec, ctx),
                None => {
                    self.error = Some(
                        "Run the board needs a profile with an orchestrator and its skill.".into(),
                    )
                }
            },
            // The hourly pass: one operator session whose /loop runs the
            // operate skill every hour on the jobs picked; off stops it.
            Action::Clone(on) => {
                let key = claude::OPERATOR;
                if on {
                    let Some(prompt) = claude::pass_prompt(self.settings.clone.tasks.as_deref())
                    else {
                        return;
                    };
                    let exe = std::env::current_exe().unwrap_or_default();
                    let spec =
                        claude::operator(self.skill_files(), &exe.to_string_lossy(), &prompt);
                    self.run_fresh(spec, ctx);
                    self.chat_open = true;
                    self.chat = Some(key.to_owned());
                } else if self.claude.live_of(key).is_some() {
                    self.claude.stop(key);
                }
            }
            Action::PassTask(name, on) => {
                let all: Vec<String> = claude::PASS_TASKS
                    .iter()
                    .map(|(n, ..)| n.to_string())
                    .collect();
                self.save_settings(|s| {
                    // Unset means all: the first untick starts from all.
                    let tasks = s.clone.tasks.get_or_insert_with(|| all.clone());
                    tasks.retain(|t| *t != name);
                    if on {
                        tasks.push(name.clone());
                    }
                    tasks.sort_by_key(|t| all.iter().position(|a| a == t));
                });
                // The last job unticked pauses the pass: its loop stops.
                if self
                    .settings
                    .clone
                    .tasks
                    .as_ref()
                    .is_some_and(Vec::is_empty)
                    && self.claude.live_of(claude::OPERATOR).is_some()
                {
                    self.claude.stop(claude::OPERATOR);
                }
            }
            Action::ToggleChat => {
                self.chat_open = !self.chat_open;
                // Open with nothing to show: the latest chat, else a new one.
                if self.chat_open && self.chat.is_none() {
                    let latest = self
                        .claude
                        .saved
                        .iter()
                        .filter(|(k, _)| k.starts_with(claude::CHAT))
                        .max_by(|a, b| a.1.started_at.cmp(&b.1.started_at))
                        .map(|(k, _)| k.clone());
                    match latest {
                        Some(key) => self.apply(Action::ChatShow(key), ctx),
                        None => self.apply(Action::NewChat, ctx),
                    }
                }
            }
            Action::NewChat => {
                let exe = std::env::current_exe().unwrap_or_default();
                let key = format!("{}{}", claude::CHAT, crate::activity::now_ms());
                let number = self
                    .claude
                    .saved
                    .keys()
                    .filter(|k| k.starts_with(claude::CHAT))
                    .count()
                    + 1;
                let spec = claude::chat(
                    &key,
                    number,
                    self.skill_files(),
                    &exe.to_string_lossy(),
                    &self.settings.clone,
                );
                self.chat_open = true;
                self.chat = Some(key);
                self.claude.open(spec, ctx);
            }
            Action::ChatShow(key) => {
                self.release_session(&key, ctx);
                self.chat_open = true;
                // A chat that runs comes up at once; a stopped one resumes.
                if !self.claude.is_attached(&key) {
                    self.apply(Action::Attach(key.clone()), ctx);
                }
                self.chat = Some(key);
            }
            Action::Steer(id) => {
                let Some(it) = self.missions.iter().find(|i| i.id == id).cloned() else {
                    return;
                };
                // The session that drives it, when there still is one (or
                // is starting: a second click opens no second session).
                if self.claude.saved.contains_key(&it.session)
                    || self.claude.starting.contains(&it.session)
                {
                    self.apply(Action::ChatShow(it.session), ctx);
                    return;
                }
                let exe = std::env::current_exe().unwrap_or_default();
                let key = format!("{}{}", claude::CHAT, crate::activity::now_ms());
                let spec = claude::steer(
                    &key,
                    &it,
                    self.skill_files(),
                    &exe.to_string_lossy(),
                    &self.settings.clone,
                );
                match crate::mission::own(&id, &key, crate::activity::now_ms() as u64) {
                    Ok(all) => self.missions = all,
                    Err(error) => {
                        self.error = Some(format!("could not save the initiative: {error}"));
                        return;
                    }
                }
                self.chat_open = true;
                self.chat = Some(key);
                self.claude.open(spec, ctx);
            }
            Action::CloneWindow(key) => {
                let name = self
                    .claude
                    .saved
                    .get(&key)
                    .map(|s| self.session_name(&key, s))
                    .unwrap_or_default();
                // One terminal, one place: the panel lets go of it.
                if self.chat.as_deref() == Some(key.as_str()) {
                    self.chat = None;
                }
                self.sheet = false;
                let before = self.windows.len();
                self.session_window(&key);
                // A window of its own, named so it reads as the clone's.
                if self.windows.len() > before {
                    self.panes.title = Some(marked_window("CLONE", &name));
                }
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                if !self.claude.is_attached(&key) {
                    self.apply(Action::Attach(key), ctx);
                }
            }
            Action::Drive {
                parent,
                parallel,
                initiative,
                local,
                session,
            } => {
                let title = self
                    .cards
                    .iter()
                    .find(|c| c.issue.identifier == parent)
                    .map(|c| c.issue.title.clone())
                    .unwrap_or_default();
                let now = crate::activity::now_ms() as u64;
                // The first session that drives it keeps it: a later drive
                // (the hourly pass, another chat) does not take it over.
                let owned = self.missions.iter().any(|m| {
                    !m.done && m.parent == parent && self.claude.saved.contains_key(&m.session)
                });
                let session = session.filter(|_| !owned).unwrap_or_default();
                match crate::mission::drive(
                    &parent,
                    parallel,
                    local,
                    initiative.as_deref(),
                    &title,
                    &session,
                    now,
                ) {
                    Ok(all) => {
                        self.missions = all;
                        self.missions_at = None;
                        if local {
                            self.agent_says(&format!(
                                "follows {parent}: built here, each step listed"
                            ));
                        } else {
                            self.agent_says(&format!(
                                "drives {parent}: {parallel} at a time through the bot"
                            ));
                            // What is already in the bot's Todo starts now too.
                            self.kick_bot();
                        }
                    }
                    Err(error) => {
                        self.error = Some(format!("could not save the initiative: {error}"))
                    }
                }
            }
            Action::DismissMission(id) => match crate::mission::update(&id, |i| i.dismissed = true)
            {
                Ok(all) => self.missions = all,
                Err(error) => self.error = Some(format!("could not save the initiatives: {error}")),
            },
            Action::StopDrive(key) => {
                match crate::mission::stop(&key, crate::activity::now_ms() as u64) {
                    Ok(all) => self.missions = all,
                    Err(error) => {
                        self.error = Some(format!("could not save the initiatives: {error}"))
                    }
                }
            }
            Action::Instruct(text) => {
                let exe = std::env::current_exe().unwrap_or_default();
                let key = claude::free_key();
                let now = crate::activity::now_ms() as u64;
                let id = match crate::mission::begin(&text, &key, now) {
                    Ok((id, all)) => {
                        self.missions = all;
                        id
                    }
                    Err(error) => {
                        self.error = Some(format!("could not save the initiative: {error}"));
                        return;
                    }
                };
                let spec =
                    claude::mission(&key, &text, &id, self.skill_files(), &exe.to_string_lossy());
                self.sheet = false;
                self.session_window(&key);
                self.session = Some(key);
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
            }
            Action::Operate => {
                let exe = std::env::current_exe().unwrap_or_default();
                let spec = claude::operator(self.skill_files(), &exe.to_string_lossy(), "/operate");
                self.run_fresh(spec, ctx);
            }
            Action::Autopilot(on) => match crate::autopilot::switch(on) {
                Ok(state) => {
                    self.autopilot = state;
                    // The first run goes at the next check, not in 30s.
                    self.autopilot_at = None;
                }
                Err(error) => self.error = Some(format!("could not save the autopilot: {error}")),
            },
            Action::SpotWork(mode) => {
                // The plan needs the week's goals: the user's own, or this
                // week's from the goals run. Without either, that run goes
                // first and the plan follows it (follow_goals).
                let date = crate::today::date();
                let stale = !self
                    .found_goals()
                    .is_some_and(|g| crate::goals::current(g, &date));
                let tried = self.goals_tried.as_deref() == Some(date.as_str());
                if self.settings.goals.trim().is_empty() && stale && !tried {
                    self.goals_tried = Some(date);
                    self.plan_after_goals = Some(mode);
                    self.apply(Action::FindGoals, ctx);
                    return;
                }
                // Fresh every time: what is true now is in Linear and Slack,
                // not in the last run.
                let tuning = [
                    self.settings.work.tuning(mode.as_deref()),
                    self.plan_context(),
                ]
                .into_iter()
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
                let spec = claude::spotter(&tuning, self.skill_files());
                self.run_fresh(spec, ctx);
            }
            Action::FileAsIs => {
                // No AI: the first line as the title, the note as the text,
                // the team picked (else the default one), into its Backlog.
                let default = self.default_team();
                let Some(capture) = self.capture.as_mut().filter(|c| !c.busy) else {
                    return;
                };
                let team = filing_team(&capture.team, &default);
                let note = capture.note.trim().to_owned();
                if note.is_empty() || team.is_empty() {
                    capture.error = Some("Pick the teams to watch first.".into());
                    return;
                }
                let title: String = note
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(80)
                    .collect();
                capture.busy = true;
                capture.error = None;
                self.send(Command::CreateTicket {
                    key: None,
                    note: note.clone(),
                    intake: crate::capture::Intake {
                        title,
                        team,
                        priority: "P3".into(),
                        ..Default::default()
                    },
                    answers: Vec::new(),
                    description: Some(note),
                });
            }
            Action::FixNote => {
                let Some(capture) = self.capture.take() else {
                    return;
                };
                // The note, its answers and any reading, as a job of its own.
                let intake = capture.intake.clone().unwrap_or_default();
                let mut description = capture.note.clone();
                for (q, a) in intake.questions.iter().zip(&capture.answers) {
                    if !a.trim().is_empty() {
                        description.push_str(&format!("\n\nQ: {q}\nA: {}", a.trim()));
                    }
                }
                let title = match intake.title.trim() {
                    "" => capture
                        .note
                        .lines()
                        .next()
                        .unwrap_or("a fix")
                        .chars()
                        .take(60)
                        .collect(),
                    title => title.to_owned(),
                };
                let spot = crate::work::Spot {
                    id: format!("note-{}", claude::free_key()),
                    title,
                    kind: intake.kind.clone(),
                    route: "now".into(),
                    description,
                    ..Default::default()
                };
                self.do_now(spot, ctx);
            }
            Action::Improve => {
                let key = claude::free_key();
                let view = crate::activity::name(&self.view);
                let spec = claude::improve(&key, &view, self.skill_files());
                // A window of its own: never over what the pane shows now.
                self.session_window(&key);
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
            }
            Action::NewSession { name, folder } => {
                let key = claude::free_key();
                let folder = folder
                    .map(|f| f.trim().to_owned())
                    .filter(|f| !f.is_empty())
                    .map(|f| match f.strip_prefix("~/") {
                        Some(rest) => directories::BaseDirs::new()
                            .map_or_else(|| f.clone().into(), |d| d.home_dir().join(rest)),
                        None => std::path::PathBuf::from(&f),
                    });
                let spec = claude::free(&key, &name, folder.as_deref(), self.skill_files());
                // An empty tab takes it (its Claude button); else its own window.
                let empty = self.selected.is_none()
                    && self.session.is_none()
                    && matches!(
                        self.panes.surface(self.panes.active),
                        Some(panes::Surface::Ticket(_))
                    );
                if !empty {
                    self.session_window(&key);
                }
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
            }
            Action::Attach(key) if key.starts_with("sid:") => {
                self.claude.attach_external(&key, ctx)
            }
            Action::Attach(key) if self.card(&key).is_none() => match self.spec_for(&key) {
                Some(spec) => self.claude.open(spec, ctx),
                None => {
                    self.error = Some(
                        "That ticket is not on the board any more (done over 14 days ago).".into(),
                    )
                }
            },
            Action::Attach(key) => match self.card(&key) {
                Some(card) => {
                    let first = claude::summary(card);
                    let spec = claude::ticket(card, &self.viewer, self.skill_files(), Some(first));
                    self.claude.open(spec, ctx);
                }
                None => {
                    self.error = Some(
                        "That ticket is not on the board any more (done over 14 days ago).".into(),
                    )
                }
            },
            // A shell splits into another shell, as in a terminal.
            Action::Split { across }
                if matches!(
                    self.panes.surface(self.panes.focused),
                    Some(panes::Surface::Shell)
                ) =>
            {
                // In the folder the shell beside it is in now.
                let dir = self.shells.get(&self.panes.focused).and_then(|t| t.cwd());
                match claude::Terminal::shell_in(dir.as_deref(), ctx) {
                    Ok(terminal) => {
                        let pane = self.panes.split(across, panes::Surface::Shell);
                        self.shells.insert(pane, terminal);
                        self.focus_pane(pane);
                    }
                    Err(error) => self.error = Some(format!("Could not start a shell: {error:#}")),
                }
            }
            Action::Split { across } => {
                let mut view = self.ticket_view();
                // One terminal per session: the copy opens on the diff, or
                // the walkthrough without a PR, so the two sit side by side.
                if view.tab == Tab::Claude {
                    let has_pr = view
                        .selected
                        .as_deref()
                        .and_then(|id| self.card(id))
                        .is_some_and(|c| !c.prs.is_empty());
                    view.tab = beside_claude(has_pr);
                    view.session = None;
                }
                let pane = self.panes.split(across, panes::Surface::Ticket(view));
                self.focus_pane(pane);
            }
            Action::SplitClaude if self.tab == Tab::Claude && self.session.is_some() => {
                let (walk, claude) = claude_aside(self.ticket_view());
                self.show_ticket_view(walk);
                let pane = self.panes.split(true, panes::Surface::Ticket(claude));
                self.focus_pane(pane);
            }
            Action::SplitClaude => {}
            Action::SaveSettings => self.save_settings_text(ctx),
            Action::SetLayaMode(mode) => {
                self.save_settings(|s| s.laya.mode = mode.clone());
                if mode != "off" && !self.laya_ready {
                    self.apply(Action::DownloadLaya, ctx);
                }
            }
            Action::ImportProfile => {
                let source = self.profile_input.trim().to_owned();
                if source.is_empty() || self.profile_importing {
                    return;
                }
                if source.starts_with("https://") {
                    if self.backend.is_none() {
                        self.profile_error =
                            Some("Sign in first to import from a URL, or use a file".into());
                        return;
                    }
                    self.profile_importing = true;
                    self.profile_error = None;
                    self.send(Command::ImportProfile { source });
                } else if crate::profile::is_bundle(&source) {
                    // A local bundle needs no network: unpacked right away.
                    let unpacked = std::fs::read_to_string(crate::profile::expand(&source))
                        .map_err(anyhow::Error::from)
                        .and_then(|b| crate::profile::unpack(&b, &crate::profile::unpacked()));
                    match unpacked {
                        Ok(profile) => self.use_profile(source, profile, ctx),
                        Err(error) => self.profile_error = Some(format!("{error:#}")),
                    }
                } else {
                    // A folder is used where it is.
                    match crate::profile::load(&source) {
                        Ok(profile) => self.use_profile(source, profile, ctx),
                        Err(error) => self.profile_error = Some(format!("{error:#}")),
                    }
                }
            }
            Action::SaveProfile => self.save_profile_text(),
            Action::SaveBotSetup => self.save_bot_setup(),
            Action::RevertBotSetup => {
                self.bot_drafts.clear();
                self.bot_save_error = None;
            }
            Action::RevertProfile => {
                self.profile_seen.reset();
                self.profile_save_error = None;
                self.follow_profile();
            }
            Action::SetUpProfile => {
                if crate::read_only() {
                    self.error = Some("read-only: no profile session".into());
                    return;
                }
                // No profile yet: one of its own in the data folder, used in place.
                let folder = crate::profile::folder(&self.settings.profile)
                    .unwrap_or_else(|| store::data_dir().join("my-profile"));
                if let Err(error) = std::fs::create_dir_all(&folder) {
                    self.error = Some(format!("cannot create {}: {error}", folder.display()));
                    return;
                }
                // Absolute, or the session's folder lands under the work dir.
                let folder = std::fs::canonicalize(&folder).unwrap_or(folder);
                if crate::profile::effective(&self.settings.profile).is_empty() {
                    let path = folder.to_string_lossy().into_owned();
                    self.save_settings(|s| s.profile = path);
                }
                let spec = claude::profile_setup(&folder, self.skill_files(), &self.new_skills());
                self.session_window(claude::PROFILE);
                self.session = Some(claude::PROFILE.into());
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
                self.start_onboarding();
            }
            Action::Reveal(path) => {
                let _ = std::process::Command::new("open")
                    .arg("-R")
                    .arg(path)
                    .spawn();
            }
            Action::SetActivityLog(on) => self.save_settings(|s| s.activity_log = Some(on)),
            Action::ClearProfile => self.use_profile(
                crate::profile::GENERIC.into(),
                crate::profile::Profile::default(),
                ctx,
            ),
            Action::UseProfile(name) => match crate::profile::load(&name) {
                Ok(profile) => self.use_profile(name, profile, ctx),
                Err(error) => self.profile_error = Some(format!("{error:#}")),
            },
            Action::DownloadLaya => {
                if self.laya_download.is_none() {
                    self.laya_download = Some(Ok((0, 0)));
                    self.send(Command::DownloadLaya);
                }
            }
            Action::Update => {
                if let (Some(tag), false) = (self.update.clone(), self.installing) {
                    self.installing = true;
                    self.update_error = None;
                    self.send(Command::Update { tag });
                }
            }
            Action::RevertSettings => self.revert_settings(),
            Action::Investigate(incident_id) => {
                let Some(incident) = self.incidents.iter().find(|i| i.id == incident_id).cloned()
                else {
                    return;
                };
                let key = format!("{}{}", claude::INCIDENT, incident.id);
                let spec = claude::investigation(&key, &incident, self.skill_files());
                // A window of its own like Debug: the session, then the panes.
                self.sheet = false;
                let before = self.windows.len();
                self.session_window(&key);
                if self.windows.len() > before {
                    let name = format!("{} {}", incident.reference, incident.name);
                    self.panes.title = Some(marked_window("INCIDENT", &name));
                }
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
                // The cluster next to it (settings.json `investigate`).
                let env = self.settings.investigate.clone();
                self.debug_panes(&env, false, ctx);
            }
            Action::LandBatch(prs) => {
                let mut rest = Vec::new();
                for (url, head) in prs {
                    let Some(Ok(gate)) = self.gates.get(&url) else {
                        continue;
                    };
                    // Followed until it lands: queued again at this head.
                    if land_step(gate, &head) != Land::Skip {
                        self.landing.insert(url.clone(), head.clone());
                    }
                    match land_step(gate, &head) {
                        Land::Merge => self.apply(Action::Merge { url, head }, ctx),
                        // Auto-merge on, and a session watching: a check
                        // that fails after this still gets re-run or fixed.
                        Land::WhenReady => {
                            self.apply(
                                Action::AutoMerge {
                                    url: url.clone(),
                                    head,
                                },
                                ctx,
                            );
                            rest.push(url);
                        }
                        Land::Goal => rest.push(url),
                        Land::Skip => {}
                    }
                }
                // The rest: a landing session per goal (one, unless the
                // list outgrows /goal's limit), merging each once CLEAN. Several sit
                // side by side in one window.
                let goals = crate::github::land_goals(&rest);
                let stem = claude::free_key();
                for (i, goal) in goals.iter().enumerate() {
                    let key = format!("{stem}-{i}");
                    let n = goal.matches("\n- ").count();
                    let name = match rest.as_slice() {
                        [one] => format!("Land {}", pr_ref(one)),
                        _ => format!("Land {n} {} PRs", crate::profile::get().ktlo.label),
                    };
                    let mut spec = claude::free(&key, &name, None, self.skill_files());
                    // Named so Sessions, Cmd+K and KTLO's batch find it again.
                    spec.identifier = claude::LANDING.into();
                    spec.prompt = Some(format!("/goal {goal}"));
                    // The user's Land click covers merging these: queue or
                    // auto-merge only, never --admin (claude::LANDING_ALLOW).
                    spec.allow = claude::LANDING_ALLOW.map(String::from).to_vec();
                    spec.deny = claude::LANDING_DENY.map(String::from).to_vec();
                    if i == 0 {
                        self.sheet = false;
                        let before = self.windows.len();
                        self.session_window(&key);
                        if self.windows.len() > before {
                            let what = match rest.as_slice() {
                                [one] => pr_ref(one),
                                _ => format!(
                                    "{} {} PRs",
                                    rest.len(),
                                    crate::profile::get().ktlo.label
                                ),
                            };
                            self.panes.title = Some(marked_window("LAND", &what));
                        }
                        self.session = Some(key.clone());
                        self.tab = Tab::Claude;
                    } else {
                        let view = panes::TicketView {
                            tab: Tab::Claude,
                            session: Some(key.clone()),
                            ..empty_view()
                        };
                        self.panes.split(true, panes::Surface::Ticket(view));
                    }
                    self.claude.open(spec, ctx);
                }
            }
            Action::FixRelease(id) => {
                let Some((release, run)) = self.releases.iter().find_map(|(r, run)| {
                    let run = run.as_ref().ok()?.as_ref()?;
                    (run.id == id).then(|| (r.clone(), run.clone()))
                }) else {
                    return;
                };
                let key = format!("{}{}", claude::RELEASE, run.id);
                let spec = claude::release_fix(&key, &release, &run, self.skill_files());
                self.sheet = false;
                let before = self.windows.len();
                self.session_window(&key);
                if self.windows.len() > before {
                    self.panes.title = Some(marked_window("RELEASE", release.label()));
                }
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
            }
            Action::InvestigateAlert { id, debug } => {
                let Some(alert) = self.alerts.iter().find(|a| a.id == id).cloned() else {
                    return;
                };
                let key = format!("{}{}", claude::ALERT, alert.id);
                // The profile's alert skill first (`skills.debugAlert`).
                let profile = crate::profile::get();
                let skill = profile.skills.get("debugAlert").filter(|_| debug);
                let spec = claude::alert_investigation(
                    &key,
                    &alert,
                    skill.map(String::as_str),
                    self.skill_files(),
                );
                self.sheet = false;
                let before = self.windows.len();
                self.session_window(&key);
                // A window of its own, named so it reads as an alert's.
                if self.windows.len() > before {
                    self.panes.title = Some(marked_window("ALERT", &alert.title));
                }
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
                if !debug {
                    let env = self.settings.investigate.clone();
                    self.debug_panes(&env, false, ctx);
                }
            }
            Action::NewWindow => {
                let (program, args) = launcher(&std::env::current_exe().unwrap_or_default());
                if let Err(error) = std::process::Command::new(&program).args(&args).spawn() {
                    self.error = Some(format!("Could not open a new window: {error}"));
                }
            }
            Action::Debug(name) => {
                let Some(env) = self.settings.debug.iter().find(|e| e.name == name).cloned() else {
                    return;
                };
                // A window of its own, as a new tab is.
                self.stash_window();
                let mut window = panes::Panes::new(panes::Surface::Shell);
                window.title = Some(format!("debug {}", env.name));
                self.windows.push(window);
                self.enter_window(self.windows.len() - 1);
                self.sheet = false;
                let first = self.panes.focused;
                self.debug_panes(&env, true, ctx);
                // The window's starting pane held nothing: the tools replace it.
                if !self.shells.contains_key(&first) && self.panes.close(first) {
                    let next = self.panes.ids()[0];
                    if self.panes.focused == first {
                        self.panes.focused = next;
                    }
                    if self.panes.active == first {
                        self.panes.active = next;
                    }
                }
            }
            Action::NewShell => match claude::Terminal::shell(ctx) {
                Ok(terminal) => {
                    let pane = self.panes.split(true, panes::Surface::Shell);
                    self.shells.insert(pane, terminal);
                    self.focus_pane(pane);
                }
                Err(error) => self.error = Some(format!("Could not start a shell: {error:#}")),
            },
            Action::EditorWindow => {
                // An empty pane (Cmd+N, Cmd+D) becomes the editor itself.
                if self.focused_is_empty() {
                    let pane = self.panes.focused;
                    self.panes.set(pane, panes::Surface::Editor);
                    self.editors.insert(pane, Default::default());
                    if pane == self.panes.active
                        && let Some(other) = self.panes.tickets().first().copied()
                    {
                        if let Some(panes::Surface::Ticket(view)) =
                            self.panes.surface(other).cloned()
                        {
                            self.show_ticket_view(view);
                        }
                        self.panes.active = other;
                    }
                    self.panes.focused = pane;
                } else {
                    self.stash_window();
                    let window = panes::Panes::new(panes::Surface::Editor);
                    self.editors.insert(window.focused, Default::default());
                    self.windows.push(window);
                    self.enter_window(self.windows.len() - 1);
                }
                self.sheet = false;
            }
            Action::BrowserWindow => {
                // Like the editor: an empty pane becomes the browser itself.
                if self.focused_is_empty() {
                    let pane = self.panes.focused;
                    self.panes.set(pane, panes::Surface::Browser);
                    self.browsers
                        .insert(pane, crate::browser::Browser::new(pane));
                    if pane == self.panes.active
                        && let Some(other) = self.panes.tickets().first().copied()
                    {
                        if let Some(panes::Surface::Ticket(view)) =
                            self.panes.surface(other).cloned()
                        {
                            self.show_ticket_view(view);
                        }
                        self.panes.active = other;
                    }
                    self.panes.focused = pane;
                } else {
                    self.stash_window();
                    let window = panes::Panes::new(panes::Surface::Browser);
                    let pane = window.focused;
                    self.browsers
                        .insert(pane, crate::browser::Browser::new(pane));
                    self.windows.push(window);
                    self.enter_window(self.windows.len() - 1);
                }
                self.sheet = false;
            }
            Action::SaveEditor(pane) => {
                if let Some(editor) = self.editors.get_mut(&pane) {
                    editor.said = match crate::editor::save(&editor.path, &editor.text) {
                        Ok(path) => format!(
                            "saved {} · {}",
                            path.display(),
                            jiff::Zoned::now().strftime("%H:%M")
                        ),
                        Err(error) => format!("not saved: {error}"),
                    };
                }
            }
            Action::ShellWindow => match claude::Terminal::shell(ctx) {
                // An empty pane (Cmd+D gave one) becomes the shell itself.
                Ok(terminal) if self.focused_is_empty() => {
                    let pane = self.panes.focused;
                    self.panes.set(pane, panes::Surface::Shell);
                    self.shells.insert(pane, terminal);
                    // The list and ticket actions go to a ticket pane left, if any.
                    if pane == self.panes.active
                        && let Some(other) = self.panes.tickets().first().copied()
                    {
                        if let Some(panes::Surface::Ticket(view)) =
                            self.panes.surface(other).cloned()
                        {
                            self.show_ticket_view(view);
                        }
                        self.panes.active = other;
                    }
                    self.panes.focused = pane;
                    self.sheet = false;
                }
                Ok(terminal) => {
                    self.stash_window();
                    let window = panes::Panes::new(panes::Surface::Shell);
                    self.shells.insert(window.focused, terminal);
                    self.windows.push(window);
                    self.enter_window(self.windows.len() - 1);
                    self.sheet = false;
                }
                Err(error) => self.error = Some(format!("Could not start a shell: {error:#}")),
            },
            Action::ClosePane(id) => {
                let id = id.unwrap_or(self.panes.focused);
                if !self.panes.close(id) {
                    // A window's last pane: the window goes, unless it is the last
                    // and a ticket (a shell's last window becomes an empty ticket).
                    let shell = self.shells.contains_key(&id) || self.editors.contains_key(&id);
                    if closes_window(&self.panes.ids(), id, self.windows.len(), shell) {
                        self.apply(Action::CloseWindow(self.window), ctx);
                    }
                    return;
                }
                // Closing a shell's pane ends the shell; a session keeps running.
                self.shells.remove(&id);
                self.editors.remove(&id);
                self.browsers.remove(&id);
                self.shell_titles.remove(&id);
                if id == self.panes.active {
                    let next = self.panes.tickets().first().copied();
                    let next = next.unwrap_or(self.panes.ids()[0]);
                    match self.panes.surface(next).cloned() {
                        Some(panes::Surface::Ticket(view)) => self.show_ticket_view(view),
                        _ => self.show_ticket_view(empty_view()),
                    }
                    self.panes.active = next;
                }
                if id == self.panes.focused {
                    self.panes.focused = self.panes.active;
                }
            }
            Action::CloseWindow(i) if i < self.windows.len() => {
                self.stash_window();
                // Into the history first: Cmd+Z brings it back, a shell's
                // claude resumed.
                let claude = self.windows[i]
                    .ids()
                    .into_iter()
                    .filter_map(|id| {
                        let pid = self.shells.get(&id)?.pid()?;
                        Some((id, self.claude.resumable_in_shell(pid)?))
                    })
                    .collect();
                remember(
                    &mut self.closed_windows,
                    (i, self.windows[i].clone(), claude),
                );
                for pane in self.windows[i].ids() {
                    self.shells.remove(&pane);
                    self.shell_titles.remove(&pane);
                }
                if self.windows.len() == 1 {
                    self.windows[0] = panes::Panes::new(panes::Surface::Ticket(empty_view()));
                    return self.enter_window(0);
                }
                self.windows.remove(i);
                self.enter_window(after_close(self.window, i, self.windows.len()));
            }
            Action::CloseWindow(_) => {}
            Action::ReopenWindow => {
                let Some((i, window, claude)) = self.closed_windows.pop() else {
                    return;
                };
                self.stash_window();
                for id in window.ids() {
                    if window.surface(id) == Some(&panes::Surface::Shell)
                        && let Some(terminal) = resumed_shell(claude.get(&id), ctx)
                    {
                        self.shells.insert(id, terminal);
                    }
                }
                let at = reopen_at(i, self.windows.len());
                self.windows.insert(at, window);
                self.enter_window(at);
                self.sheet = false;
            }
            Action::FocusPane(id) => self.focus_pane(id),
            Action::CyclePane(step) => self.focus_pane(self.panes.neighbour(step)),
            Action::ZoomPane => self.panes.zoomed = !self.panes.zoomed && !self.panes.single(),
            Action::FullScreen => {
                self.full_screen = !self.full_screen;
                // The pane alone: zoomed while it is on, as it was after.
                self.panes.zoomed = self.full_screen && !self.panes.single();
                // Leaving brings back only the chrome: the window stays as it is.
                if self.full_screen {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                }
            }
            Action::PaneRatio(split, ratio) => self.panes.set_ratio(split, ratio),
            Action::OpenSession(key) if key == claude::TRIAGE => self.show_triage(ctx),
            // The clone's own sessions (its chats, the hourly pass, a
            // mission) open in the clone panel, wherever they are clicked.
            Action::OpenSession(key) if self.is_clone_session(&key) => {
                self.apply(Action::ChatShow(key), ctx)
            }
            // A session from elsewhere working in a ticket's folder opens as
            // that ticket, the session on its Claude tab: never bare.
            Action::OpenSession(key) if let Some(id) = self.ticket_of_session(&key) => {
                self.apply(Action::OpenTicket { id, split: false }, ctx);
                self.tab = Tab::Claude;
                self.session = Some(key.clone());
                self.apply(Action::Attach(key), ctx);
            }
            // A PR's session opens with the PR's tabs (walk, diff, Claude),
            // its card made again when the board does not hold it.
            Action::OpenSession(key) if self.card(&key).is_none() && self.pr_card_for(&key) => {
                self.apply(Action::Claude(key), ctx)
            }
            // A ticket's session shows with its ticket; any other session
            // (free, incident, orchestrator, spotter, profile) on its own.
            Action::OpenSession(key)
                if self.card(&key).is_none()
                    && !key.starts_with("sid:")
                    && !key.starts_with("cid:") =>
            {
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.apply(Action::Attach(key), ctx);
            }
            // A terminal elsewhere cannot be attached: its app comes to the
            // front instead of a tab that only says so.
            Action::OpenSession(key)
                if key.strip_prefix("sid:").is_some_and(|sid| {
                    self.claude
                        .rows
                        .get(sid)
                        .is_some_and(|row| row.kind == "interactive" && claude::show_host(row))
                }) => {}
            Action::OpenSession(key) if key.starts_with("sid:") => {
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.claude.attach_external(&key, ctx);
            }
            Action::OpenSession(key) if key.starts_with("cid:") => {
                self.session = Some(key.clone());
                self.tab = Tab::Claude;
                self.claude.attach_codex_external(&key);
            }
            Action::OpenSession(key) => self.apply(Action::Claude(key), ctx),
            Action::StopSession(key) => self.claude.stop(&key),
            Action::CloseSession(key) => {
                if self.claude.live_of(&key).is_some() {
                    self.claude.stop(&key);
                }
                self.claude.forget(&key);
                // The chat panel no longer shows a chat that is gone.
                if self.chat.as_deref() == Some(key.as_str()) {
                    self.chat = None;
                }
            }
            Action::CodexPrompt { key, text } => {
                if !text.trim().is_empty() {
                    self.claude.codex_prompt(&key, text);
                    if let Some(draft) = self.claude.codex_draft(&key) {
                        draft.clear();
                    }
                }
            }
            Action::CodexAnswer { key, response } => self.claude.codex_answer(&key, response),
            // Plan my day and the goals run start over through their own
            // action, which builds the day's brief (date, teams, goals).
            Action::FreshSession(key) if key == claude::SPOTTER => {
                self.apply(Action::SpotWork(None), ctx)
            }
            Action::FreshSession(key) if key == claude::GOALS => self.apply(Action::FindGoals, ctx),
            Action::FreshSession(key) => {
                // The brief first: a session without a ticket keeps its
                // name and folder only in the saved entry fresh() drops.
                let spec = self.spec_for(&key);
                self.claude.fresh(&key);
                match spec {
                    Some(spec) => self.claude.open(spec, ctx),
                    None => self.apply(Action::Attach(key), ctx),
                }
            }
            Action::RunSkill {
                issue_id,
                command,
                submit,
            } => {
                // By identifier too; never a silent no-op: `orbit ctl` says
                // ok only when something happened (stuck-session-send-build.md).
                let Some(card) = self.card_named(&issue_id).cloned() else {
                    self.error = Some(format!(
                        "{issue_id} is not on the board: {command} not sent"
                    ));
                    return;
                };
                let issue_id = card.issue.id.clone();
                // A second click while the first is on its way does nothing.
                let working = self
                    .claude
                    .live_of(&issue_id)
                    .is_some_and(|l| matches!(l.label(), "working" | "starting"));
                let age = self.started.get(&issue_id).map(|at| at.elapsed());
                if super::started_state(age, working) == super::Started::Waiting {
                    return;
                }
                // Its own window when no pane shows it (an empty pane takes
                // it, as a click does): dispatching N tickets gives N windows,
                // never one that each replaces in the focused pane.
                if self.selected.as_deref() != Some(issue_id.as_str()) {
                    self.apply(Action::Focus(issue_id.clone()), ctx);
                }
                // Claude waiting on a permission prompt would take the command
                // (and its Enter) as the answer: show the prompt instead.
                let asking = self
                    .claude
                    .live_of(&issue_id)
                    .is_some_and(|l| l.label() == "needs approval");
                if asking {
                    self.session = Some(issue_id);
                    self.tab = Tab::Claude;
                    return;
                }
                // A command that runs by itself runs in the background: the
                // session starts or wakes, the view stays, and the Claude tab
                // shows it when asked. One left for the user to finish opens
                // the tab, since they have to type there.
                if !submit {
                    self.session = Some(issue_id.clone());
                    self.tab = Tab::Claude;
                }
                // A build here takes the ticket from the bot: its poller
                // would build it a second time.
                let build = builds_here(&command, &crate::profile::get().skill("build"));
                if card.bot_works && build {
                    self.send(Command::TakeFromBot {
                        issue_id: issue_id.clone(),
                    });
                }
                // A sub-ticket built here: its parent is followed, so the
                // clone panel lists each sub-ticket's step without anyone
                // having to ask for it.
                if build
                    && let Some(parent) = &card.issue.parent
                    && !self
                        .missions
                        .iter()
                        .any(|m| !m.done && m.parent == parent.identifier)
                    && let Ok(all) = crate::mission::drive(
                        &parent.identifier,
                        1,
                        true,
                        None,
                        &parent.title,
                        "",
                        crate::activity::now_ms() as u64,
                    )
                {
                    self.missions = all;
                    self.missions_at = None;
                }
                self.run_in_ticket(&card, command, submit, ctx);
                self.started
                    .retain(|_, at| at.elapsed() < std::time::Duration::from_secs(300));
                self.started.insert(issue_id, std::time::Instant::now());
            }
            Action::SetState { issue_id, state_id } => {
                self.shown.insert(
                    issue_id.clone(),
                    Shown {
                        state_id: state_id.clone(),
                        team_id: None,
                        confirmed_by: None,
                    },
                );
                self.rebuild();
                self.send(Command::SetState { issue_id, state_id });
            }
            Action::UnifiedDiff(on) => self.unified_diff = on,
            Action::DiffAll(on) => {
                self.diff_all = on;
                // The diffs of every PR waiting on the user's approval, once.
                let missing: Vec<crate::story::Pr> = self
                    .ktlo()
                    .flat_map(|c| c.prs.iter())
                    .filter(|p| p.open)
                    .filter(|p| matches!(self.gates.get(&p.url), Some(Ok(g)) if g.can_approve))
                    .filter(|p| {
                        !self.diffs.contains_key(&p.url) && !self.diff_pending.contains(&p.url)
                    })
                    .cloned()
                    .collect();
                if on {
                    for pr in missing {
                        self.fetch_diff(pr);
                    }
                }
            }
            Action::ViewedFile(key) => {
                // Another copy may have marked files since: re-read first.
                let mut all: std::collections::HashSet<String> =
                    store::load::<Vec<String>>("viewed.json")
                        .into_iter()
                        .collect();
                if !all.remove(&key) {
                    all.insert(key);
                }
                let _ = store::save("viewed.json", &all.iter().collect::<Vec<_>>());
                self.viewed = all;
            }
            Action::FoldFile(url, path) => {
                let key = (url, path);
                if !self.folded_files.remove(&key) {
                    self.folded_files.insert(key);
                }
            }
            Action::FoldFolder(url, folder) => {
                let key = (url, folder);
                if !self.folded.remove(&key) {
                    self.folded.insert(key);
                }
            }
            Action::AddLabel { url, label } => {
                // The review roulette takes the review off the user's plate.
                if label == crate::profile::get().github.review_roulette {
                    self.hand_off(&url, "review roulette");
                }
                if self.labeled.insert((url.clone(), label.clone())) {
                    self.send(Command::AddLabel { url, label });
                }
            }
            Action::RequestReview { url, login } => {
                self.hand_off(&url, &format!("@{login}"));
                // Shown as asked at once; an error takes it back.
                if self.review_requested.insert((url.clone(), login.clone())) {
                    self.send(Command::RequestReview { url, login });
                }
            }
            Action::HandleInbox => {
                let (items, _) = self.inbox();
                if items.is_empty() {
                    return;
                }
                let key = claude::free_key();
                let mut spec =
                    claude::free(&key, "Handle my notifications", None, self.skill_files());
                spec.prompt = Some(crate::inbox::brief(&items));
                self.sheet = false;
                self.session_window(&key);
                self.session = Some(key);
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
            }
            Action::FindWork => {
                let key = claude::free_key();
                let mut spec = claude::free(&key, "Find work", None, self.skill_files());
                let skill = crate::profile::get().skill("find-work");
                spec.prompt = Some(format!("/{skill} {}", self.plan_context()));
                self.sheet = false;
                self.session_window(&key);
                self.session = Some(key);
                self.tab = Tab::Claude;
                self.claude.open(spec, ctx);
            }
            Action::ClearKtlo => {
                let cards: Vec<&crate::story::Card> = self.ktlo().collect();
                let label = crate::profile::get().ktlo.label.clone();
                let what = format!("open {label} tickets and Dependabot PRs");
                let prompt = ship_prompt(cards, &what);
                self.ship(prompt, &format!("Clear {label}"), ctx);
            }
            Action::ShipReviews => {
                let cards: Vec<&crate::story::Card> = self
                    .for_you()
                    .into_iter()
                    .filter(|c| c.ask == Some(crate::story::Ask::Review))
                    .collect();
                let prompt = ship_prompt(cards, "of the bot's PRs waiting on your review");
                self.ship(prompt, "Ship the bot's PRs", ctx);
            }
            Action::MoveTeam { issue_id, team_id } => {
                // The ticket keeps its kind of state (Todo stays Todo) in the
                // new team, so Linear never has to guess one.
                let Some(card) = self.card(&issue_id) else {
                    return;
                };
                let kind = card.issue.state.kind.clone();
                let Some(state_id) = self
                    .states
                    .get(&team_id)
                    .and_then(|states| crate::story::state_like(states, &kind))
                    .map(|s| s.id.clone())
                else {
                    self.error = Some("That team's states are not loaded yet.".into());
                    return;
                };
                self.shown.insert(
                    issue_id.clone(),
                    Shown {
                        state_id: state_id.clone(),
                        team_id: Some(team_id.clone()),
                        confirmed_by: None,
                    },
                );
                self.rebuild();
                self.send(Command::MoveTeam {
                    issue_id,
                    team_id,
                    state_id,
                });
            }
            Action::DraftComment(text) => self.draft = text,
            Action::PalettePicked(title) => {
                self.palette_picks =
                    super::palette::record_pick(&title, crate::activity::now_ms() as u64);
            }
            Action::LoadReviewers(slug) => {
                if self.reviewers_asked.insert(slug.clone()) {
                    self.send(Command::Reviewers(slug));
                }
            }
            Action::SendComment => {
                let body = self.draft.trim().to_owned();
                let Some(issue_id) = self.selected.clone() else {
                    return;
                };
                if body.is_empty() {
                    return;
                }
                self.draft.clear();
                // The user's words, and a last line saying where they came from.
                let body = crate::profile::get().attribution.signed(&body);
                self.post_comment(issue_id, body);
            }
        }
    }

    /// Posts a comment as the user, shown at once and confirmed by Linear.
    fn post_comment(&mut self, issue_id: String, body: String) {
        self.next_temp += 1;
        let temp_id = format!("pending-{}", self.next_temp);
        self.locals.push(Local {
            issue_id: issue_id.clone(),
            comment: Comment {
                id: temp_id.clone(),
                body: body.clone(),
                created_at: jiff::Timestamp::now().to_string(),
                user: Some(User {
                    name: self.viewer.clone(),
                }),
            },
            confirmed_by: None,
        });
        self.rebuild();
        self.send(Command::AddComment {
            issue_id,
            temp_id,
            body,
        });
    }

    /// How to start or resume the session under `key`, whatever it is: a
    /// ticket's, a special run's, or a saved one with no ticket left (a
    /// free session, an incident that is over).
    fn spec_for(&self, key: &str) -> Option<claude::Spec> {
        let skills = self.skill_files();
        if let Some(card) = self.card(key) {
            let first = claude::summary(card);
            return Some(claude::ticket(card, &self.viewer, skills, Some(first)));
        }
        if let Some(id) = key.strip_prefix(claude::RELEASE)
            && let Some((release, run)) = self.releases.iter().find_map(|(r, run)| {
                let run = run.as_ref().ok()?.as_ref()?;
                (run.id.to_string() == id).then_some((r, run))
            })
        {
            return Some(claude::release_fix(key, release, run, skills));
        }
        if let Some(id) = key.strip_prefix(claude::ALERT)
            && let Some(alert) = self.alerts.iter().find(|a| a.id == id)
        {
            return Some(claude::alert_investigation(key, alert, None, skills));
        }
        if let Some(id) = key.strip_prefix(claude::INCIDENT)
            && let Some(incident) = self.incidents.iter().find(|i| i.id == id)
        {
            return Some(claude::investigation(key, incident, skills));
        }
        if key.starts_with(claude::CHAT) {
            let exe = std::env::current_exe().unwrap_or_default();
            let title = self
                .claude
                .saved
                .get(key)
                .map(|s| s.title.clone())
                .unwrap_or_default();
            let number = title.trim_start_matches("Chat ").parse().unwrap_or(1);
            return Some(claude::chat(
                key,
                number,
                skills,
                &exe.to_string_lossy(),
                &self.settings.clone,
            ));
        }
        match key {
            claude::TRIAGE => Some(claude::triage(&self.teams, skills)),
            claude::SPOTTER => Some(claude::spotter("", skills)),
            claude::ORCHESTRATOR => claude::orchestrator(skills, true),
            claude::OPERATOR => {
                let exe = std::env::current_exe().unwrap_or_default();
                Some(claude::operator(skills, &exe.to_string_lossy(), "/operate"))
            }
            claude::PROFILE => crate::profile::folder(&self.settings.profile)
                .map(|folder| claude::profile_setup(&folder, skills, &self.new_skills())),
            _ => {
                let saved = self.claude.saved.get(key)?;
                // A mission resumes as one: its brief, its CLI permission
                // and its place among the clone's runs.
                if let Some(it) = self.missions.iter().find(|m| m.session == key) {
                    let exe = std::env::current_exe().unwrap_or_default();
                    return Some(claude::mission(
                        key,
                        &it.goal,
                        &it.id,
                        skills,
                        &exe.to_string_lossy(),
                    ));
                }
                Some(claude::free(key, &saved.title, Some(&saved.cwd), skills))
            }
        }
        .or_else(|| {
            key.starts_with(claude::FREE)
                .then(|| claude::free(key, "", None, self.skill_files()))
        })
    }

    /// A run that starts over each time (Run the board, Plan my day): not
    /// while one is busy; a finished one still open is stopped first.
    /// One ship session (skills/ship) with this prompt: ships what only
    /// needs a merge, moves the rest to its team.
    fn ship(&mut self, prompt: Option<String>, name: &str, ctx: &egui::Context) {
        let Some(prompt) = prompt else {
            return;
        };
        let key = claude::free_key();
        let mut spec = claude::free(&key, name, None, self.skill_files());
        spec.prompt = Some(prompt);
        self.sheet = false;
        self.session_window(&key);
        self.session = Some(key);
        self.tab = Tab::Claude;
        self.claude.open(spec, ctx);
    }

    pub(super) fn run_fresh(&mut self, spec: claude::Spec, ctx: &egui::Context) {
        let key = spec.key.clone();
        if self.claude.busy(&key) {
            return;
        }
        if self.claude.live_of(&key).is_some() {
            self.claude.stop(&key);
        }
        self.claude.forget(&key);
        self.claude.open(spec, ctx);
    }

    /// A Work spot that went through New ticket and got a ticket: started,
    /// with the ticket to show for it.
    pub(super) fn spot_filed(&mut self) {
        let Some(capture) = self.capture.as_ref() else {
            return;
        };
        let (Some(id), Some((identifier, _))) = (capture.spot.clone(), capture.created.clone())
        else {
            return;
        };
        if let Some(mut spot) = self.spot(&id) {
            spot.ticket = Some(identifier);
            self.remember_spot(&spot, "");
        }
    }

    /// The team a ticket goes to when nothing says otherwise: the profile's
    /// default, else the first watched team.
    pub(crate) fn default_team(&self) -> String {
        let named = crate::profile::get().intake.default_team.trim().to_owned();
        if named.is_empty() {
            self.teams.first().cloned().unwrap_or_default()
        } else {
            named
        }
    }

    /// A local session does it now, with the task as its brief.
    fn do_now(&mut self, spot: crate::work::Spot, ctx: &egui::Context) {
        let key = claude::free_key();
        if !spot.id.starts_with("note-") {
            self.remember_spot(&spot, &key);
        }
        let spec = claude::job(&key, &spot, self.skill_files());
        self.session_window(&key);
        self.session = Some(key);
        self.tab = Tab::Claude;
        self.claude.open(spec, ctx);
    }

    /// Starts a spot: a ticket's quick action in that ticket's own session,
    /// anything else as a job of its own. `shown` (the user's click) opens
    /// the job's session; the plan's own start leaves the view alone and
    /// skips a ticket whose session is busy (someone is in it).
    pub(super) fn start_spot(&mut self, spot: crate::work::Spot, shown: bool, ctx: &egui::Context) {
        let card = spot
            .ticket
            .as_deref()
            .and_then(|t| self.cards.iter().find(|c| c.issue.identifier == t))
            .cloned();
        if let (Some(command), Some(card)) = (spot.quick(), card) {
            if !shown && self.claude.busy(&card.issue.id) {
                return;
            }
            self.remember_spot(&spot, &card.issue.id);
            if shown {
                self.select_ticket(Some(card.issue.id.clone()));
            }
            return self.run_in_ticket(&card, command, true, ctx);
        }
        if shown {
            return self.do_now(spot, ctx);
        }
        let key = claude::free_key();
        self.remember_spot(&spot, &key);
        self.claude
            .open(claude::job(&key, &spot, self.skill_files()), ctx);
    }

    /// Shows the triage session in its own window, triage only: the one
    /// showing it, else a new one. The ticket it was started from keeps
    /// its window.
    fn show_triage(&mut self, ctx: &egui::Context) {
        self.sheet = false;
        if !triaging(&self.ticket_view()) {
            self.stash_window();
            match triage_window(&self.windows) {
                Some(i) => self.enter_window(i),
                None => {
                    self.windows.push(panes::Panes::new(panes::Surface::Ticket(
                        panes::TicketView {
                            tab: Tab::Triage,
                            ..empty_view()
                        },
                    )));
                    self.enter_window(self.windows.len() - 1);
                }
            }
        }
        self.apply(Action::Attach(claude::TRIAGE.into()), ctx);
    }

    /// Runs a skill command in the ticket's session: an existing
    /// conversation gets it typed in once it is up (the terminal runs
    /// attached but unseen); a new one starts with it as its first message.
    fn run_in_ticket(
        &mut self,
        card: &crate::story::Card,
        command: String,
        submit: bool,
        ctx: &egui::Context,
    ) {
        let issue_id = &card.issue.id;
        let existing = self.claude.live_of(issue_id).is_some() || self.claude.resumable(issue_id);
        let spec = if existing || !submit {
            self.claude.type_line(issue_id, command, submit);
            claude::ticket(card, &self.viewer, self.skill_files(), None)
        } else {
            claude::ticket(card, &self.viewer, self.skill_files(), Some(command))
        };
        self.claude.open(spec, ctx);
    }

    /// Where the weekly priority documents are, so a run reads them first.
    fn priorities_hint(&self) -> String {
        format!(
            "Weekly priorities: in Linear, teams {} -> the cycle that covers the week of {} \
             -> its documents whose title contains \"{}\". Read these first.",
            self.cycle_teams().join(", "),
            crate::goals::week_of(&crate::today::date()),
            crate::profile::get().linear.priorities.title(),
        )
    }

    /// What the spotter plans the day by: the date, the user's teams (their
    /// tickets can be done first, another team's only asked for), where the
    /// weekly priorities are, and the week's goals.
    fn plan_context(&self) -> String {
        // Their teams are the goals' teams: membership says little (one can
        // be in a dozen). Without goals, the ones they are in.
        let mut teams = self
            .found_goals()
            .map(crate::goals::teams)
            .unwrap_or_default();
        if teams.is_empty() {
            teams = self.my_teams.iter().map(|(key, _)| key.clone()).collect();
        }
        let goals = crate::goals::render(&self.settings.goals, self.found_goals());
        let goals = match goals.trim() {
            "" => "None written down: plan from the weekly priorities.",
            goals => goals,
        };
        format!(
            "## Plan the day\nToday is {}. The user is {}; their teams: {}.\n{}\n\n\
             ## This week's goals\n{goals}",
            crate::today::date(),
            self.viewer,
            teams.join(", "),
            self.priorities_hint(),
        )
    }

    fn spot(&self, id: &str) -> Option<crate::work::Spot> {
        let spots = self.spots.as_ref()?.as_ref().ok()?;
        spots.spots.iter().find(|s| s.id == id).cloned()
    }

    /// Notes a spot as started, so the spotter skips it next time.
    fn remember_spot(&mut self, spot: &crate::work::Spot, session: &str) {
        let folder = self.claude.folder(claude::SPOTTER);
        if let Err(error) = crate::work::remember(&folder, spot, session) {
            self.error = Some(format!("could not note the spot as started: {error}"));
        }
        self.spots_started = crate::work::started(&folder);
    }

    /// Changes one thing in `settings.json`, keeping the rest. Reads the
    /// file rather than `self`, so teams from ORBIT_TEAMS never land
    /// in it by accident.
    pub(super) fn save_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        let mut settings = store::load_settings();
        change(&mut settings);
        if let Err(error) = store::save_settings(&settings) {
            self.error = Some(format!("could not save settings: {error}"));
        }
        // The editor picks the change up from the file.
        self.settings = settings;
    }

    /// One shell pane per tool of `env`, each typing its setup first.
    fn debug_panes(&mut self, env: &store::DebugEnv, focus: bool, ctx: &egui::Context) {
        for tool in &env.panes {
            if tool.split_whitespace().next() == Some("k9s")
                && let Err(error) = crate::k9s::install(&crate::profile::get())
            {
                self.error = Some(format!("Could not set up k9s: {error:#}"));
            }
            match claude::Terminal::shell(ctx) {
                Ok(mut terminal) => {
                    terminal.type_line(&env.command(tool));
                    let pane = self.panes.split(true, panes::Surface::Shell);
                    self.shells.insert(pane, terminal);
                    self.shell_titles
                        .insert(pane, format!("{tool} · {}", env.name));
                    if focus {
                        self.focus_pane(pane);
                    }
                }
                Err(error) => {
                    self.error = Some(format!("Could not start a shell: {error:#}"));
                    return;
                }
            }
        }
    }

    /// The user handed this PR's review to `who`: it asks them nothing more.
    /// handed.json is re-read first (another copy may have written it).
    pub(super) fn hand_off(&mut self, url: &str, who: &str) {
        self.handed_set(url, Some(who));
    }

    /// The review request failed: the PR asks the user again.
    pub(super) fn take_back(&mut self, url: &str) {
        self.handed_set(url, None);
    }

    fn handed_set(&mut self, url: &str, who: Option<&str>) {
        let mut handed: std::collections::HashMap<String, String> = store::load("handed.json");
        match who {
            Some(who) => handed.insert(url.to_owned(), who.to_owned()),
            None => handed.remove(url),
        };
        let _ = store::save("handed.json", &handed);
        self.handed = handed;
        self.rebuild();
    }

    /// The window (index) that shows session `key` in one of its panes, if
    /// one does: the chat panel then points there instead of drawing the
    /// same terminal twice (two sizes for one PTY clip both).
    /// One terminal, one place: a session the clone panel takes is let go
    /// by the window showing it. A window that only showed it closes; a
    /// pane in a bigger one goes empty.
    fn release_session(&mut self, key: &str, ctx: &egui::Context) {
        let Some(w) = self.session_in_window(key) else {
            return;
        };
        let current = w == self.window;
        let only = if current {
            &self.panes
        } else {
            &self.windows[w]
        }
        .ids()
        .len()
            == 1;
        if only && self.windows.len() > 1 {
            return self.apply(Action::CloseWindow(w), ctx);
        }
        if current && self.session.as_deref() == Some(key) {
            self.show_ticket_view(empty_view());
            return;
        }
        let window = if current {
            &mut self.panes
        } else {
            &mut self.windows[w]
        };
        for id in window.ids() {
            if matches!(window.surface(id), Some(panes::Surface::Ticket(v)) if v.session.as_deref() == Some(key))
            {
                window.set(id, panes::Surface::Ticket(empty_view()));
            }
        }
    }

    pub(crate) fn session_in_window(&self, key: &str) -> Option<usize> {
        if self.session.as_deref() == Some(key) || self.panes.showing_session(key).is_some() {
            return Some(self.window);
        }
        (0..self.windows.len()).filter(|&i| i != self.window).find(|&i| {
            let w = &self.windows[i];
            w.showing_session(key).is_some()
                || matches!(w.surface(w.active), Some(panes::Surface::Ticket(v)) if v.session.as_deref() == Some(key))
        })
    }

    /// Brings up the window whose pane shows session `key`, or opens a new
    /// window for it; the caller then points the active view at it.
    fn session_window(&mut self, key: &str) {
        if self.session.as_deref() == Some(key) {
            return;
        }
        if let Some(pane) = self.panes.showing_session(key) {
            return self.focus_pane(pane);
        }
        // A ticket's own session (keyed by its issue id) goes to the window
        // showing that ticket, on any tab: a ticket is in the sidebar once.
        if let Some((i, pane)) = self.window_showing(key) {
            if i != self.window {
                self.stash_window();
                self.enter_window(i);
            }
            return self.focus_pane(pane);
        }
        self.stash_window();
        let found = (0..self.windows.len()).find_map(|i| {
            let w = &self.windows[i];
            match w.surface(w.active) {
                Some(panes::Surface::Ticket(v)) if v.session.as_deref() == Some(key) => {
                    Some((i, w.active))
                }
                _ => w.showing_session(key).map(|pane| (i, pane)),
            }
        });
        match found {
            Some((i, pane)) => {
                self.enter_window(i);
                self.focus_pane(pane);
            }
            None => {
                self.windows
                    .push(panes::Panes::new(panes::Surface::Ticket(empty_view())));
                self.enter_window(self.windows.len() - 1);
            }
        }
    }

    /// Gives a pane the keyboard. A ticket pane also becomes the active
    /// one: the view it kept swaps with the one in `App`.
    /// The focused pane is a ticket pane showing nothing yet: no ticket, no
    /// session, not triage.
    fn focused_is_empty(&self) -> bool {
        let focused = self.panes.focused;
        let view = match self.panes.surface(focused) {
            Some(panes::Surface::Ticket(_)) if focused == self.panes.active => self.ticket_view(),
            Some(panes::Surface::Ticket(view)) => view.clone(),
            _ => return false,
        };
        view.selected.is_none() && view.session.is_none() && view.tab != Tab::Triage
    }

    /// One pass over the missions: parents read again, and the sub-tickets
    /// free to go handed to the bot (mission.rs).
    pub(super) fn drive_missions(&mut self, ctx: &egui::Context) {
        let Some(bot) = crate::profile::get()
            .orchestrator
            .as_ref()
            .map(|o| o.team.clone())
        else {
            return;
        };
        let now = crate::activity::now_ms() as u64;
        // A mission session that ended before it named a parent filed
        // nothing: its initiative stops instead of filing forever.
        for it in self.missions.clone().iter().filter(|i| {
            !i.done
                && i.parent.is_empty()
                && !i.session.is_empty()
                && crate::claude::status::current(&crate::claude::status::reports(
                    &crate::claude::status::path(&i.session),
                ))
                .is_some_and(|r| r.state == crate::claude::status::State::Ended)
        }) {
            if let Ok(all) = crate::mission::update(&it.id, |i| {
                i.done = true;
                i.done_ms = now;
                i.log.push((
                    now,
                    "the mission session ended without a parent ticket".into(),
                ));
            }) {
                self.missions = all;
            }
        }
        for it in self
            .missions
            .clone()
            .into_iter()
            .filter(|i| !i.done && !i.parent.is_empty())
        {
            self.send(Command::Lookup {
                id: it.parent.clone(),
            });
            let Some(Ok(parent)) = self.looked_up.get(&it.parent).cloned() else {
                continue;
            };
            let children = &parent.children.nodes;
            let kids = crate::mission::kids(children);
            // On its way until Linear shows it with the bot: a stale look
            // must not hand it over again (that resets the bot's run).
            self.dispatching.retain(|id| {
                !kids.iter().any(|k| {
                    &k.identifier == id && (k.implemented() || k.team == bot && k.kind != "backlog")
                })
            });
            // What moved since the last look: every move logged, what is
            // the user's said once.
            let mut seen = it.seen.clone();
            let mut moved = it.moved.clone();
            let mut stalled = it.stalled.clone();
            let mut lines: Vec<String> = Vec::new();
            for kid in &kids {
                moved.entry(kid.identifier.clone()).or_insert(now);
            }
            for (id, change) in crate::mission::changes(&kids, &mut seen) {
                moved.insert(id.clone(), now);
                stalled.remove(&id);
                let url = children
                    .iter()
                    .find(|c| c.identifier == id)
                    .map(|c| c.url.clone())
                    .unwrap_or_default();
                let line = match change {
                    crate::mission::Change::Review => format!("{id} has its PR: review it"),
                    crate::mission::Change::Blocked => format!("{id} is blocked: your call"),
                    crate::mission::Change::Merged => format!("{id} merged: check it works"),
                    crate::mission::Change::Moved(from) => {
                        let to = &seen[&id];
                        lines.push(format!("{id}: {from} → {to}"));
                        continue;
                    }
                };
                self.agent_tells(&line, &format!("linear:{url}"));
                lines.push(line);
            }
            // With the bot and not moving: say so once, with the bot's
            // review cap when that is what holds it.
            let it_now = crate::mission::Initiative {
                moved: moved.clone(),
                stalled: stalled.clone(),
                ..it.clone()
            };
            for id in crate::mission::stalls(&kids, &it_now, &bot, now) {
                let state = &seen[&id];
                let line = match self.bot_review_cap(&bot) {
                    Some((n, cap)) => format!(
                        "{id} sits {state} with the bot: it holds new builds while {n} of its tickets are In Review (cap {cap}); reviews free it"
                    ),
                    None => format!("{id} sits {state} with the bot for over an hour, no PR yet"),
                };
                let url = children
                    .iter()
                    .find(|c| c.identifier == id)
                    .map(|c| c.url.clone())
                    .unwrap_or_default();
                self.agent_tells(&line, &format!("linear:{url}"));
                lines.push(line);
                stalled.insert(id);
            }
            // Gone quiet where the bot waits for a word: the drive asks it,
            // once per state (driving is the user's click for these).
            let mut kicked = it.kicked.clone();
            let mut fallen_back = it.fallen_back.clone();
            // Only what the drive handed over: one it holds back stays held.
            // Built here: nothing was handed to the bot, nothing is kicked.
            for kid in kids
                .iter()
                .filter(|_| !it.local)
                .filter(|k| k.in_flight(&bot) || k.state == "In Review")
            {
                let Some(card) = self
                    .cards
                    .iter()
                    .find(|c| c.issue.identifier == kid.identifier)
                else {
                    continue;
                };
                let look = crate::mission::Look {
                    stage: card.stage,
                    planned: card.plan.is_some(),
                    red: card.ci == crate::story::Ci::Red,
                    quiet_ms: quiet_ms(card, moved.get(&kid.identifier).copied(), now),
                };
                // Kicked and still quiet: the bot is not answering. Built
                // here instead, said once (a ticket with a session is left).
                if crate::mission::fall_back(&look, kicked.get(&kid.identifier))
                    && !self.claude.saved.contains_key(&card.issue.id)
                    && fallen_back.insert(kid.identifier.clone())
                {
                    let id = &kid.identifier;
                    let line = format!(
                        "{id}: {} did not answer its kick in {} min, building it in an Orbit session",
                        crate::story::bot(),
                        look.quiet_ms / 60_000
                    );
                    let (issue_id, url) = (card.issue.id.clone(), card.issue.url.clone());
                    let build = crate::profile::get().skill("build");
                    self.agent_tells(&line, &format!("linear:{url}"));
                    lines.push(line);
                    self.apply(
                        Action::RunSkill {
                            issue_id,
                            command: format!("/{build} {id}"),
                            submit: true,
                        },
                        ctx,
                    );
                    continue;
                }
                if kicked.get(&kid.identifier) == Some(&look.key()) {
                    continue;
                }
                let Some(command) = crate::mission::kick(&look) else {
                    continue;
                };
                let Some(comment) = command.comment() else {
                    continue;
                };
                let id = &kid.identifier;
                let mins = look.quiet_ms / 60_000;
                let line = format!(
                    "{id} was quiet {mins} min: asked {} to {}",
                    crate::story::bot(),
                    command.what()
                );
                let (issue_id, url) = (card.issue.id.clone(), card.issue.url.clone());
                self.asked_bot.insert(
                    issue_id.clone(),
                    (command, jiff::Timestamp::now().to_string()),
                );
                self.post_comment(issue_id, comment);
                self.kick_bot();
                self.agent_tells(&line, &format!("linear:{url}"));
                lines.push(line);
                kicked.insert(id.clone(), look.key());
            }
            let finished = crate::mission::finished(&kids);
            if finished {
                let line = format!(
                    "{}: every sub-ticket is implemented, the reviews are yours",
                    it.parent
                );
                self.agent_says(&line);
                lines.push(line);
            }
            let changed = seen != it.seen
                || moved != it.moved
                || stalled != it.stalled
                || kicked != it.kicked
                || fallen_back != it.fallen_back
                || !lines.is_empty();
            if changed
                && let Ok(all) = crate::mission::update(&it.id, |i| {
                    i.seen = seen;
                    i.moved = moved;
                    i.stalled = stalled;
                    i.kicked = kicked;
                    i.fallen_back = fallen_back;
                    if finished && !i.done {
                        i.done_ms = now;
                    }
                    i.done |= finished;
                    i.log.extend(lines.into_iter().map(|l| (now, l)));
                })
            {
                self.missions = all;
            }
            if finished {
                continue;
            }
            // Built here: the steps list reads each PR's gate, asked now,
            // and a PR with open review threads gets its fix at once, sent to
            // its idle session once per head (the Fix comments button), so it
            // never waits for the user to ask (stuck-session-send-build.md).
            if it.local {
                // A sub-ticket off the board becomes a card (looked up, as a
                // link opens one), so its session's commands reach it.
                let mut added = false;
                for kid in kids.iter().filter(|k| !k.merged()) {
                    if self.card_named(&kid.identifier).is_some() {
                        continue;
                    }
                    match self.looked_up.get(&kid.identifier) {
                        Some(Ok(issue)) => {
                            let issue = issue.clone();
                            self.fetched.retain(|i| i.id != issue.id);
                            self.fetched.push(issue);
                            added = true;
                        }
                        _ => self.send(Command::Lookup {
                            id: kid.identifier.clone(),
                        }),
                    }
                }
                if added {
                    self.rebuild();
                }
                let mut fixed = it.kicked.clone();
                let mut said: Vec<String> = Vec::new();
                for kid in kids.iter().filter(|k| !k.merged()) {
                    let (work, _, pr) = self.work_of_kid(&kid.identifier);
                    let Some(url) = pr else { continue };
                    self.send(Command::Gate { url: url.clone() });
                    let head = self
                        .gates
                        .get(&url)
                        .and_then(|g| g.as_ref().ok())
                        .map(|g| g.head.clone())
                        .unwrap_or_default();
                    // Its head first: a fix is sent once per pushed head.
                    if head.is_empty() {
                        continue;
                    }
                    let mark = format!("fix|{head}");
                    if !crate::mission::fix_now(&work, fixed.get(&kid.identifier), &mark) {
                        continue;
                    }
                    let Some(card) = self
                        .session_for(&kid.identifier)
                        .and_then(|key| self.card_named(&key).cloned())
                    else {
                        continue;
                    };
                    let skill = crate::profile::get().skill("fixComments");
                    let line = format!(
                        "{}: {} open review thread(s) on its PR, sent /{skill}",
                        kid.identifier, work.threads
                    );
                    let ticket_url = children
                        .iter()
                        .find(|c| c.identifier == kid.identifier)
                        .map(|c| c.url.clone())
                        .unwrap_or_default();
                    self.agent_tells(&line, &format!("linear:{ticket_url}"));
                    said.push(line);
                    fixed.insert(kid.identifier.clone(), mark);
                    // In the background, as RunSkill does, but no window
                    // comes forward: nobody clicked.
                    self.run_in_ticket(&card, format!("/{skill} {url}"), true, ctx);
                }
                if fixed != it.kicked
                    && let Ok(all) = crate::mission::update(&it.id, |i| {
                        i.kicked = fixed;
                        i.log.extend(said.into_iter().map(|l| (now, l)));
                    })
                {
                    self.missions = all;
                }
                continue;
            }
            for id in crate::mission::next(&kids, &bot, it.parallel) {
                if self.dispatching.insert(id.clone()) {
                    self.send(Command::Dispatch {
                        identifier: id,
                        team: bot.clone(),
                        state: "Todo".into(),
                    });
                }
            }
        }
    }

    /// The session working on a sub-ticket: the Orbit session started for
    /// that ticket (its identifier), if there is one.
    pub(crate) fn session_for(&self, identifier: &str) -> Option<String> {
        self.claude
            .saved
            .iter()
            .find(|(_, s)| s.identifier == identifier)
            .map(|(k, _)| k.clone())
    }

    /// What Orbit knows of a sub-ticket's work: its session, its PR (the
    /// card's, else the one its session opened) and that PR's gate.
    pub(crate) fn work_of_kid(
        &self,
        identifier: &str,
    ) -> (crate::mission::Work, Option<String>, Option<String>) {
        let key = self.session_for(identifier);
        let reports = key.as_ref().and_then(|k| self.claude.status.get(k));
        let note = reports
            .and_then(|r| claude::status::note(r))
            .map(|r| r.text.clone());
        let card = self
            .cards
            .iter()
            .chain(&self.pr_cards)
            .find(|c| c.issue.identifier == identifier);
        let pr = card
            .and_then(|c| c.prs.iter().find(|p| p.open).map(|p| p.url.clone()))
            .or_else(|| {
                reports
                    .and_then(|r| claude::status::pr(r))
                    .map(|r| r.text.clone())
            });
        let gate = pr
            .as_ref()
            .and_then(|u| self.gates.get(u))
            .and_then(|g| g.as_ref().ok());
        let work = crate::mission::Work {
            working: key.as_ref().is_some_and(|k| {
                self.claude.starting.contains(k)
                    || self
                        .claude
                        .live_of(k)
                        .is_some_and(|l| l.label() == "working")
            }),
            session: key.is_some(),
            pr: pr.is_some(),
            threads: gate.map_or(0, |g| g.open_threads),
            red: card.is_some_and(|c| c.ci == crate::story::Ci::Red),
            queued: gate.is_some_and(|g| g.queued),
            ..Default::default()
        };
        (work, note, pr)
    }

    /// The bot's tickets In Review on the board and its cap, when the
    /// profile names one and it is reached: the bot builds nothing new.
    pub(crate) fn bot_review_cap(&self, bot: &str) -> Option<(usize, usize)> {
        let cap = crate::profile::get().bot.review_cap;
        let prefix = format!("{bot}-");
        let n = self
            .cards
            .iter()
            .filter(|c| {
                c.issue.identifier.starts_with(&prefix) && c.issue.state.name == "In Review"
            })
            .count();
        (cap > 0 && n >= cap).then_some((n, cap))
    }

    /// Has the bot look now (the profile's poller workflow), at most every
    /// two minutes: what an initiative handed it starts at once.
    pub(super) fn kick_bot(&mut self) {
        let target = crate::profile::get().bot.poller_workflow.clone();
        if target.is_empty()
            || self
                .kicked_at
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(120))
        {
            return;
        }
        self.kicked_at = Some(std::time::Instant::now());
        self.send(Command::KickBot(target));
    }

    /// A line from the clone about something to open: its toast opens it.
    pub(super) fn agent_tells(&mut self, line: &str, key: &str) {
        let line = format!("agent · {line}");
        self.toasts.push(super::Toast {
            key: key.to_owned(),
            line: line.clone(),
            at: std::time::Instant::now(),
            agent: true,
        });
        if !self.front {
            crate::notify::send(line);
        }
    }

    /// A line from the clone: a grey agent toast, and a Mac notification
    /// while Orbit is not in front.
    pub(super) fn agent_says(&mut self, line: &str) {
        // An hourly pass that finds the same as last hour says it once.
        if !repeats(&self.last_said, line) {
            self.last_said = line.to_owned();
        } else {
            return;
        }
        let line = format!("agent · {line}");
        self.toasts.push(super::Toast {
            key: format!("agent:{}", crate::activity::now_ms()),
            line: line.clone(),
            at: std::time::Instant::now(),
            agent: true,
        });
        if !self.front {
            crate::notify::send(line);
        }
    }

    /// A session of the clone's: a chat, the hourly pass, or a mission
    /// (also one an initiative names that resumed before missions kept
    /// their identifier).
    pub(crate) fn is_clone_session(&self, key: &str) -> bool {
        let identifier = self.claude.saved.get(key).map(|s| s.identifier.as_str());
        clone_session(key, identifier, &self.missions)
    }

    /// The ticket a session not keyed by one works on (a free session, one
    /// Orbit did not start): see `ticket_of`. A ticket's or PR's own session
    /// keeps its own path.
    pub(super) fn ticket_of_session(&self, key: &str) -> Option<String> {
        if self.card(key).is_some() || key.starts_with("pr:") {
            return None;
        }
        let cwd = match key.strip_prefix("sid:") {
            Some(sid) => self.claude.rows.get(sid).map(|r| r.cwd.clone()),
            None => self
                .claude
                .saved
                .get(key)
                .map(|s| s.cwd.to_string_lossy().into_owned()),
        };
        let reports = self.claude.status.get(key).map_or(&[][..], Vec::as_slice);
        ticket_of(cwd.as_deref(), reports, &self.cards)
    }

    /// The window and pane that show this ticket, the active one first.
    pub(super) fn window_showing(&self, id: &str) -> Option<(usize, u64)> {
        if self.selected.as_deref() == Some(id) {
            return Some((self.window, self.panes.active));
        }
        // A window in the back keeps its active pane's view stashed, so
        // every pane counts there; in front, the active one is `selected`.
        (0..self.windows.len()).find_map(|i| {
            let pane = if i == self.window {
                self.panes.showing(id)
            } else {
                self.windows[i].showing_stored(id)
            };
            pane.map(|pane| (i, pane))
        })
    }

    pub(super) fn focus_pane(&mut self, id: u64) {
        self.panes.focused = id;
        if id == self.panes.active
            || !matches!(self.panes.surface(id), Some(panes::Surface::Ticket(_)))
        {
            return;
        }
        self.stash_view();
        if let Some(panes::Surface::Ticket(view)) = self.panes.surface(id).cloned() {
            self.show_ticket_view(view);
        }
        self.panes.active = id;
        self.guide_step = None;
    }
}

impl App {
    /// The teams whose cycles hold the weekly priorities: the profile's,
    /// else the watched ones.
    fn cycle_teams(&self) -> Vec<String> {
        let named = &crate::profile::get().linear.priorities.teams;
        if named.is_empty() {
            self.teams.clone()
        } else {
            named.clone()
        }
    }
}

/// The team File as-is files into: the one picked, else the default.
fn filing_team(picked: &str, default: &str) -> String {
    match picked.trim() {
        "" => default.to_owned(),
        team => team.to_owned(),
    }
}

/// What a ticket pane shows before a ticket is picked.
/// The triage window's view: the triage session and no ticket.
/// The ship session's prompt over these tickets, most pressing first;
/// none, no session.
fn ship_prompt(mut cards: Vec<&crate::story::Card>, what: &str) -> Option<String> {
    if cards.is_empty() {
        return None;
    }
    crate::story::sort(&mut cards);
    let profile = crate::profile::get();
    Some(crate::story::ship_brief(
        &profile.skill("ship"),
        what,
        &cards,
        &profile.intake.routing,
    ))
}

/// The session a view keeps when it picks ticket `id`: only that ticket's.
/// Another one kept would make the view count as showing it, and opening
/// that session would land under this ticket instead of a window of its own.
fn own_session(session: Option<String>, id: &str) -> Option<String> {
    session.filter(|s| s == id)
}

fn triaging(view: &panes::TicketView) -> bool {
    view.tab == Tab::Triage && view.selected.is_none()
}

/// The window whose active pane shows triage, if one does.
fn triage_window(windows: &[panes::Panes]) -> Option<usize> {
    windows
        .iter()
        .position(|w| matches!(w.surface(w.active), Some(panes::Surface::Ticket(v)) if triaging(v)))
}

/// The window shown after window `closed` went, of `left` remaining: the
/// current one stays; a closed current one gives way to its right
/// neighbour, like a browser tab, or the left one at the end.
/// A Claude tab split in two: the ticket's walkthrough for the left pane,
/// the session as it was for the right.
fn claude_aside(view: panes::TicketView) -> (panes::TicketView, panes::TicketView) {
    let walk = panes::TicketView {
        tab: Tab::Walkthrough,
        session: None,
        ..view.clone()
    };
    (walk, view)
}

/// A ticket pane showing nothing yet (no ticket, no session, not triage):
/// the only one a click in the sheet fills instead of opening a window.
fn takes_a_ticket(view: &panes::TicketView) -> bool {
    view.selected.is_none() && view.session.is_none() && !triaging(view)
}

/// Where the window at `current` sits after one moved from `from` to `to`.
fn moved_index(current: usize, from: usize, to: usize) -> usize {
    match current {
        c if c == from => to,
        c if from < c && c <= to => c - 1,
        c if to <= c && c < from => c + 1,
        c => c,
    }
}

/// The tab a pane split off a Claude tab opens on.
fn beside_claude(has_pr: bool) -> Tab {
    if has_pr { Tab::Diff } else { Tab::Walkthrough }
}

/// Whether closing pane `id`, which its tree would not let go, closes
/// its window instead.
fn closes_window(ids: &[u64], id: u64, windows: usize, shell: bool) -> bool {
    ids == [id] && (windows > 1 || shell)
}

fn after_close(current: usize, closed: usize, left: usize) -> usize {
    match current {
        w if w > closed => w - 1,
        w => w.min(left - 1),
    }
}

fn empty_view() -> panes::TicketView {
    panes::TicketView {
        selected: None,
        tab: Tab::Walkthrough,
        session: None,
        diff_pr: None,
    }
}

impl App {
    /// Writes `App`'s view back into the active pane, when it is a ticket
    /// pane (a window of shells has none).
    fn stash_view(&mut self) {
        if matches!(
            self.panes.surface(self.panes.active),
            Some(panes::Surface::Ticket(_))
        ) {
            let current = self.ticket_view();
            self.panes
                .set(self.panes.active, panes::Surface::Ticket(current));
        }
    }

    /// Puts the current window's tree back into `windows` before another
    /// window is shown.
    fn stash_window(&mut self) {
        self.stash_view();
        self.windows[self.window] = self.panes.clone();
    }

    /// Opens the windows the last run left open (`windows.json`), unless
    /// the copy that wrote them is still running: a second copy starts
    /// empty instead of holding the same tickets twice. A shell comes back
    /// as a fresh one.
    pub(super) fn restore_windows(&mut self, ctx: &egui::Context) {
        let mut saved: OpenWindows = store::load(WINDOWS);
        if saved.windows.is_empty() || running_elsewhere(saved.pid) {
            return;
        }
        for window in &mut saved.windows {
            window.reserve();
            for id in window.ids() {
                if let Some(panes::Surface::Ticket(view)) = window.surface(id) {
                    let view = with_pr(view.clone());
                    window.set(id, panes::Surface::Ticket(view));
                }
            }
            for id in window.ids() {
                // An editor comes back empty: its text was never saved here.
                if window.surface(id) == Some(&panes::Surface::Editor) {
                    self.editors.insert(id, Default::default());
                }
                // A browser comes back on a blank page.
                if window.surface(id) == Some(&panes::Surface::Browser) {
                    self.browsers.insert(id, crate::browser::Browser::new(id));
                }
                if window.surface(id) != Some(&panes::Surface::Shell) {
                    continue;
                }
                if let Some(terminal) = resumed_shell(saved.claude.get(&id), ctx) {
                    self.shells.insert(id, terminal);
                }
            }
        }
        // A restored PR pane needs its card back for its tabs.
        let keys: Vec<String> = saved
            .windows
            .iter()
            .flat_map(|w| w.ids().into_iter().filter_map(|id| w.surface(id).cloned()))
            .filter_map(|surface| match surface {
                panes::Surface::Ticket(view) => view.selected.or(view.session),
                panes::Surface::Shell | panes::Surface::Editor | panes::Surface::Browser => None,
            })
            .collect();
        for key in keys {
            self.pr_card_for(&key);
        }
        // A ticket or session saved in two windows comes back in one.
        panes::dedupe(&mut saved.windows);
        self.windows = saved.windows;
        self.enter_window(saved.window.min(self.windows.len() - 1));
    }

    /// Puts the card of a `pr:` session key back among the PR cards, named
    /// by its saved session, and asks for its ticket. False when `key` is
    /// not a PR's.
    pub(super) fn pr_card_for(&mut self, key: &str) -> bool {
        if self.card(key).is_some() {
            return true;
        }
        let Some(url) = crate::story::pr_url_of(key) else {
            return false;
        };
        let (title, at) = self
            .claude
            .saved
            .get(key)
            .map(|s| (s.title.clone(), s.started_at.clone()))
            .unwrap_or_default();
        let Some(card) = crate::story::pr_card(&url, &title, &at) else {
            return false;
        };
        self.pr_cards.push(card);
        self.send(Command::PrTicket { url });
        true
    }

    /// Writes the open windows when they changed, while this copy owns the
    /// file (it wrote it last, or its writer is gone).
    pub(super) fn save_windows(&mut self) {
        self.stash_window();
        let claude = self
            .shells
            .iter()
            .filter_map(|(id, t)| Some((*id, self.claude.resumable_in_shell(t.pid()?)?)))
            .collect();
        let saved = OpenWindows {
            windows: self.windows.clone(),
            window: self.window,
            pid: std::process::id(),
            claude,
        };
        let Ok(text) = serde_json::to_string(&saved) else {
            return;
        };
        if text == self.windows_saved {
            return;
        }
        let owner = store::load::<OpenWindows>(WINDOWS).pid;
        if owner == saved.pid || !running_elsewhere(owner) {
            let _ = store::save(WINDOWS, &saved);
        }
        self.windows_saved = text;
    }

    /// Shows window `i`: its tree in `panes`, its active view in `App`.
    fn enter_window(&mut self, i: usize) {
        self.window = i;
        self.panes = self.windows[i].clone();
        match self.panes.surface(self.panes.active).cloned() {
            Some(panes::Surface::Ticket(view)) => self.show_ticket_view(view),
            _ => self.show_ticket_view(empty_view()),
        }
        self.guide_step = None;
    }
}

const WINDOWS: &str = "windows.json";

/// What Land approved does with one approved PR.
#[derive(Debug, PartialEq)]
enum Land {
    /// Green at the head shown: merged now (checked again first).
    Merge,
    /// Only checks missing at the head shown: Merge when ready.
    WhenReady,
    /// Held back by CI or its branch: a goal in the landing session.
    Goal,
    /// New commits since it was shown, or nothing to do.
    Skip,
}

/// A PR the user's Land click covers, as its gate changed: queue it again
/// at the head they saw, keep waiting, or let it go.
pub(super) fn landing_next(gate: &crate::github::Gate, head: &str) -> Landing {
    match () {
        _ if gate.state != "OPEN" => Landing::Done,
        _ if gate.head != head => Landing::Moved,
        _ if gate.queued || gate.auto_merge => Landing::Wait,
        _ if gate.green => Landing::Merge,
        _ if gate.can_auto_merge => Landing::WhenReady,
        _ => Landing::Wait,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Landing {
    Merge,
    WhenReady,
    Wait,
    /// New commits since the click: the user sees them before it lands.
    Moved,
    Done,
}

fn land_step(gate: &crate::github::Gate, head: &str) -> Land {
    match () {
        _ if gate.head != head => Land::Skip,
        _ if gate.green => Land::Merge,
        _ if gate.can_auto_merge => Land::WhenReady,
        _ if crate::github::landable(gate) => Land::Goal,
        _ => Land::Skip,
    }
}

/// `repo#7` for a PR link, the link itself when it is not one.
fn pr_ref(url: &str) -> String {
    match url.rsplit_once("/pull/") {
        Some((repo, n)) => format!("{}#{n}", repo.rsplit('/').next().unwrap_or(repo)),
        None => url.to_owned(),
    }
}

/// An investigation's window name in the strip, marked with what it is
/// (`ALERT`, `INCIDENT`) so it never passes for a ticket's.
/// The same line as the last one, but for case and spacing.
fn repeats(last: &str, line: &str) -> bool {
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    !last.is_empty() && norm(last) == norm(line)
}

fn marked_window(mark: &str, title: &str) -> String {
    let short: String = title.chars().take(40).collect();
    let more = if title.chars().count() > 40 {
        "…"
    } else {
        ""
    };
    format!("{mark} · {short}{more}")
}

/// A pane on a PR's session shows the PR too, so it has its tabs.
fn with_pr(mut view: panes::TicketView) -> panes::TicketView {
    if view.selected.is_none()
        && let Some(key) = view.session.as_deref()
        && crate::story::pr_url_of(key).is_some()
    {
        view.selected = Some(key.to_owned());
    }
    view
}

/// How many closed windows Cmd+Z can bring back.
const CLOSED_KEPT: usize = 20;

/// A close into the history; the oldest goes past `CLOSED_KEPT`.
fn remember<T>(closed: &mut Vec<T>, entry: T) {
    closed.push(entry);
    if closed.len() > CLOSED_KEPT {
        closed.remove(0);
    }
}

/// Where a reopened window goes: its old place, or last when fewer are left.
fn reopen_at(was: usize, open: usize) -> usize {
    was.min(open)
}

/// A shell coming back (a restart, Cmd+Z): one that ran Claude Code opens
/// in its folder with the session resumed.
fn resumed_shell(
    resume: Option<&(String, String)>,
    ctx: &egui::Context,
) -> Option<claude::Terminal> {
    let dir = resume.map(|(_, cwd)| std::path::Path::new(cwd.as_str()));
    let mut terminal = claude::Terminal::shell_in(dir, ctx).ok()?;
    if let Some((session, _)) = resume {
        terminal.type_line(&resume_line(session));
    }
    Some(terminal)
}

/// What a restored shell types to bring its Claude Code session back.
fn resume_line(session: &str) -> String {
    format!("claude --resume {session}")
}

/// The strip's windows as the app left them.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct OpenWindows {
    windows: Vec<panes::Panes>,
    window: usize,
    /// The copy that wrote them.
    pid: u32,
    /// A shell pane's `claude`, by pane id: session id and folder, resumed
    /// there when the window comes back.
    #[serde(default)]
    claude: std::collections::BTreeMap<u64, (String, String)>,
}

/// Another live process has this pid. ponytail: a reused pid reads as
/// alive and that start opens empty; a lock file if that ever matters.
fn running_elsewhere(pid: u32) -> bool {
    pid != 0
        && pid != std::process::id()
        && std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

/// How to start another copy of the app: the installed app through `open
/// -n` (a new instance of the bundle), a bare binary (make run) directly.
fn launcher(exe: &std::path::Path) -> (std::path::PathBuf, Vec<String>) {
    let bundle = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"));
    match bundle {
        Some(app) => (
            "open".into(),
            vec!["-n".into(), app.to_string_lossy().into_owned()],
        ),
        None => (exe.to_path_buf(), Vec::new()),
    }
}

/// A drafted comment clicked again is taken back.
fn toggle(drafts: &mut Vec<crate::github::Draft>, draft: crate::github::Draft) {
    match drafts.iter().position(|d| *d == draft) {
        Some(i) => {
            drafts.remove(i);
        }
        None => drafts.push(draft),
    }
}

/// The whole ticket and plan, for what Claude says about its PR: the change
/// in the product's terms, which the title alone does not give.
fn pr_context(card: &crate::story::Card) -> String {
    let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    format!(
        "Ticket {}: {}\n\nDescription:\n{}\n\nPlan:\n{}",
        card.issue.identifier,
        card.issue.title,
        cut(card.issue.description.as_deref().unwrap_or("none"), 6000),
        card.plan.as_ref().map_or("none".to_owned(), |p| cut(
            &format!("{}\n{}", p.tldr, p.body),
            6000
        )),
    )
}

fn clone_session(
    key: &str,
    identifier: Option<&str>,
    missions: &[crate::mission::Initiative],
) -> bool {
    key.starts_with(claude::CHAT)
        || key == claude::OPERATOR
        || identifier == Some("Mission")
        // A free session that owns an initiative is the clone's; a ticket's
        // own session that ran the drive stays with its ticket.
        || (key.starts_with(claude::FREE) && missions.iter().any(|m| m.session == key))
}

/// The build skill (`build`, as the profile names it), run here.
fn builds_here(command: &str, build: &str) -> bool {
    command.split_whitespace().next() == Some(format!("/{build}").as_str())
}

/// The ticket a session works on, from what it left behind, first match
/// wins: its folder is the ticket's work folder (`…/work/ABC-207`, or a repo
/// inside it); its latest PR line is a PR on a card; its latest note names
/// exactly one ticket on the board. Nothing certain: no ticket.
fn ticket_of(
    cwd: Option<&str>,
    reports: &[claude::status::Report],
    cards: &[crate::story::Card],
) -> Option<String> {
    let by_identifier = |identifier: &str| {
        cards
            .iter()
            .find(|c| {
                c.issue.identifier == identifier
                    || c.issue.previous_identifiers.iter().any(|p| p == identifier)
            })
            .map(|c| c.issue.id.clone())
    };
    if let Some(id) = cwd
        .and_then(crate::story::ticket_of_folder)
        .and_then(|i| by_identifier(&i))
    {
        return Some(id);
    }
    if let Some(pr) = claude::status::pr(reports) {
        let url = pr.text.trim();
        if let Some(c) = cards.iter().find(|c| c.prs.iter().any(|p| p.url == url)) {
            return Some(c.issue.id.clone());
        }
    }
    let note = reports
        .iter()
        .rev()
        .find(|r| r.state == claude::status::State::Note)?;
    let mut named: Vec<String> = note
        .text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter_map(by_identifier)
        .collect();
    named.sort();
    named.dedup();
    (named.len() == 1).then(|| named.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_as_is_files_into_the_team_picked_else_the_default() {
        assert_eq!(filing_team("", "ABC"), "ABC");
        assert_eq!(filing_team("XYZ", "ABC"), "XYZ");
    }

    #[test]
    fn a_local_build_is_the_build_skill_not_another_one() {
        assert!(builds_here("/build ABC-1", "build"));
        assert!(builds_here(
            "/build ABC-1 only the failing CI checks",
            "build"
        ));
        assert!(!builds_here("/fix-pr-comments ABC-1", "build"));
        assert!(!builds_here("/builder ABC-1", "build"));
        assert!(builds_here("/bot-build ABC-1", "bot-build"));
    }

    #[test]
    fn a_mission_resumed_as_a_plain_session_stays_the_clones() {
        let it = crate::mission::Initiative {
            session: "free:7".into(),
            ..Default::default()
        };
        assert!(clone_session(
            "free:7",
            Some("Session"),
            std::slice::from_ref(&it)
        ));
        assert!(clone_session("free:8", Some("Mission"), &[]));
        assert!(!clone_session(
            "free:9",
            Some("Session"),
            std::slice::from_ref(&it)
        ));
        let ticket = crate::mission::Initiative {
            session: "6f1c-issue-uuid".into(),
            ..it
        };
        assert!(
            !clone_session("6f1c-issue-uuid", Some("XYZ-809"), &[ticket]),
            "a ticket's session that drives stays the ticket's"
        );
    }

    #[test]
    fn a_draft_clicked_twice_is_gone() {
        let draft = |body: &str| crate::github::Draft {
            path: "a.py".into(),
            line: 3,
            side: "RIGHT".into(),
            body: body.into(),
        };
        let mut drafts = Vec::new();
        toggle(&mut drafts, draft("ask"));
        toggle(&mut drafts, draft("suggest"));
        toggle(&mut drafts, draft("ask"));
        assert_eq!(drafts, [draft("suggest")]);
    }
    use std::path::Path;

    #[test]
    fn a_closed_window_comes_back_where_it_was() {
        let mut closed = Vec::new();
        for n in 0..25 {
            remember(&mut closed, n);
        }
        assert_eq!(closed.len(), CLOSED_KEPT);
        assert_eq!(closed.pop(), Some(24), "the newest first");
        assert_eq!(closed[0], 5, "the oldest went");
        assert_eq!(reopen_at(2, 5), 2);
        assert_eq!(reopen_at(7, 3), 3, "fewer left: last");
    }

    #[test]
    fn closing_a_window_keeps_the_current_one_or_takes_its_neighbour() {
        // Windows 0 1 2, one closed, two left.
        assert_eq!(after_close(2, 0, 2), 1, "the current one, shifted");
        assert_eq!(after_close(0, 2, 2), 0, "the current one stays");
        assert_eq!(after_close(1, 1, 2), 1, "the right neighbour");
        assert_eq!(after_close(2, 2, 2), 1, "the last: its left one");
    }

    #[test]
    fn the_agent_says_an_unchanged_line_once() {
        assert!(repeats("2 PRs wait on you", "2 PRs  wait on You"));
        assert!(!repeats("2 PRs wait on you", "3 PRs wait on you"));
        assert!(!repeats("", "anything"));
    }

    #[test]
    fn a_clone_window_says_it_is_the_clones() {
        assert!(marked_window("CLONE", "Clone Chat 1").starts_with("CLONE ·"));
    }

    #[test]
    fn an_alert_window_says_it_is_an_alert() {
        assert_eq!(marked_window("ALERT", "Disk full"), "ALERT · Disk full");
        assert!(marked_window("INCIDENT", &"x".repeat(60)).ends_with("x…"));
    }

    #[test]
    fn a_prs_session_pane_shows_the_pr() {
        let view = |session: &str| panes::TicketView {
            session: Some(session.into()),
            tab: Tab::Claude,
            ..empty_view()
        };
        let pr = with_pr(view("pr:o.r.1"));
        assert_eq!(pr.selected.as_deref(), Some("pr:o.r.1"));
        assert_eq!(with_pr(view("free:1")).selected, None);
    }

    #[test]
    fn open_windows_read_back_as_written() {
        let mut window = panes::Panes::new(panes::Surface::Ticket(panes::TicketView {
            selected: Some("ABC-1".into()),
            tab: Tab::Claude,
            session: Some("ABC-1".into()),
            diff_pr: None,
        }));
        let shell = window.split(true, panes::Surface::Shell);
        window.focused = shell;
        let saved = OpenWindows {
            windows: vec![window.clone()],
            window: 0,
            pid: 1,
            claude: [(shell, ("s1".into(), "/repo".into()))].into(),
        };
        let back: OpenWindows =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        // The shell's Claude Code comes back with it, resumed.
        assert_eq!(back.claude[&shell].0, "s1");
        assert_eq!(resume_line("s1"), "claude --resume s1");
        // A file from before the field reads.
        let old: OpenWindows =
            serde_json::from_str(r#"{"windows":[],"window":0,"pid":1}"#).unwrap();
        assert!(old.claude.is_empty());
        assert_eq!(back.windows[0].ids(), window.ids());
        assert_eq!(back.windows[0].surface(shell), Some(&panes::Surface::Shell));
        assert_eq!(back.windows[0].focused, shell);
        back.windows[0].reserve();
        assert!(panes::Panes::new(panes::Surface::Shell).focused > shell);
        assert!(!running_elsewhere(std::process::id()));
        assert!(!running_elsewhere(0));
    }

    #[test]
    fn split_claude_puts_the_walkthrough_left_and_the_session_right() {
        let view = panes::TicketView {
            selected: Some("ABC-1".into()),
            tab: Tab::Claude,
            session: Some("ABC-1".into()),
            diff_pr: None,
        };
        let (walk, claude) = claude_aside(view.clone());
        assert_eq!(walk.tab, Tab::Walkthrough);
        assert_eq!(walk.session, None);
        assert_eq!(walk.selected.as_deref(), Some("ABC-1"));
        assert_eq!(claude, view);
    }

    #[test]
    fn a_session_opens_as_the_ticket_its_folder_pr_or_note_names() {
        use crate::claude::status::{Report, State};
        use crate::story::tests::issue;
        let ticket = |id: &str, identifier: &str| {
            let mut i = issue("In Review", "started", vec![]);
            i.id = id.into();
            i.identifier = identifier.into();
            crate::story::card(i, &Default::default())
        };
        let mut a = ticket("a", "ABC-346");
        let pr = crate::story::pr_card("https://github.com/o/r/pull/614", "t", "").unwrap();
        a.prs = pr.prs.clone();
        let cards = vec![a, ticket("b", "ABC-347")];
        let line = |state, text: &str| Report {
            at: std::time::UNIX_EPOCH,
            state,
            text: text.into(),
        };
        // Its folder is the ticket's work folder, or a repo inside it.
        assert_eq!(
            ticket_of(Some("/d/work/ABC-347/web"), &[], &cards).as_deref(),
            Some("b")
        );
        // Its latest PR line is a PR on a card.
        let pr_line = [line(State::Pr, "https://github.com/o/r/pull/614")];
        assert_eq!(
            ticket_of(Some("/d/work/free-1"), &pr_line, &cards).as_deref(),
            Some("a")
        );
        // Its note names exactly one ticket on the board.
        let note = [line(State::Note, "PR 614 green, ABC-346 in review")];
        assert_eq!(ticket_of(None, &note, &cards).as_deref(), Some("a"));
        // Two tickets, or none on the board: no guess.
        let two = [line(State::Note, "ABC-346 waits for ABC-347")];
        assert_eq!(ticket_of(None, &two, &cards), None);
        let unknown = [line(State::Note, "XYZ-9 done")];
        assert_eq!(ticket_of(Some("/d/work/free-2"), &unknown, &cards), None);
    }

    #[test]
    fn every_ticket_has_a_testing_tab_a_bare_pr_none() {
        use crate::story::tests::issue;
        let ticket = crate::story::card(issue("Triage", "triage", vec![]), &Default::default());
        assert!(ticket.has_testing());
        let pr = crate::story::pr_card("https://github.com/o/r/pull/1", "t", "").unwrap();
        assert!(!pr.has_testing());
    }

    #[test]
    fn a_landed_pr_is_queued_again_only_at_the_head_the_user_saw() {
        let gate = |head: &str, green: bool, queued: bool| crate::github::Gate {
            head: head.into(),
            state: "OPEN".into(),
            green,
            queued,
            ..Default::default()
        };
        // Fixed by a re-run, or dropped from the queue: back in.
        assert_eq!(
            landing_next(&gate("abc", true, false), "abc"),
            Landing::Merge
        );
        assert_eq!(
            landing_next(&gate("abc", false, true), "abc"),
            Landing::Wait
        );
        assert_eq!(
            landing_next(&gate("abc", false, false), "abc"),
            Landing::Wait
        );
        // New commits: the user looks first.
        assert_eq!(
            landing_next(&gate("def", true, false), "abc"),
            Landing::Moved
        );
        let merged = crate::github::Gate {
            state: "MERGED".into(),
            ..gate("abc", false, false)
        };
        assert_eq!(landing_next(&merged, "abc"), Landing::Done);
    }

    #[test]
    fn land_approved_merges_green_waits_on_checks_and_lands_the_rest() {
        use crate::github::Gate;
        let gate = |green, auto, checks: &str| Gate {
            head: "abc".into(),
            state: "OPEN".into(),
            review: "APPROVED".into(),
            green,
            can_auto_merge: auto,
            checks: checks.into(),
            branch: "BLOCKED".into(),
            ..Default::default()
        };
        assert_eq!(land_step(&gate(true, false, "SUCCESS"), "abc"), Land::Merge);
        assert_eq!(
            land_step(&gate(false, true, "PENDING"), "abc"),
            Land::WhenReady
        );
        assert_eq!(land_step(&gate(false, false, "FAILURE"), "abc"), Land::Goal);
        assert_eq!(
            land_step(&gate(true, false, "SUCCESS"), "new"),
            Land::Skip,
            "new commits"
        );
    }

    #[test]
    fn land_it_on_one_pr_is_a_landing_session_that_merges() {
        let url = "https://github.com/o/api/pull/1263";
        assert_eq!(pr_ref(url), "api#1263");
        let goal = &crate::github::land_goals(&[url.into()])[0];
        assert!(goal.contains("Done means merged") && goal.contains(url));
        assert!(claude::LANDING_ALLOW.contains(&"Bash(gh pr merge * --auto*)"));
    }

    #[test]
    fn a_click_in_the_sheet_never_replaces_what_a_pane_shows() {
        assert!(takes_a_ticket(&empty_view()));
        let ticket = panes::TicketView {
            selected: Some("ABC-1".into()),
            ..empty_view()
        };
        assert!(!takes_a_ticket(&ticket));
        let incident = panes::TicketView {
            tab: Tab::Claude,
            session: Some("incident:01X".into()),
            ..empty_view()
        };
        assert!(!takes_a_ticket(&incident));
    }

    #[test]
    fn moving_a_window_keeps_the_current_one_current() {
        // Windows 0 1 2 3.
        assert_eq!(moved_index(1, 1, 3), 3, "the moved one itself");
        assert_eq!(moved_index(2, 1, 3), 1, "shifted left");
        assert_eq!(moved_index(1, 3, 0), 2, "shifted right");
        assert_eq!(moved_index(0, 2, 3), 0, "untouched");
    }

    #[test]
    fn a_split_off_claude_shows_the_diff_when_there_is_one() {
        assert_eq!(beside_claude(true), Tab::Diff);
        assert_eq!(beside_claude(false), Tab::Walkthrough);
    }

    #[test]
    fn a_lone_shell_closes_its_window_even_the_last() {
        assert!(closes_window(&[3], 3, 1, true), "exited shell, last window");
        assert!(!closes_window(&[3], 3, 1, false), "last ticket pane stays");
        assert!(closes_window(&[3], 3, 2, false));
        assert!(
            !closes_window(&[3, 4], 3, 2, true),
            "not the window's last pane"
        );
    }

    #[test]
    fn picking_a_ticket_lets_go_of_another_session() {
        let pr = Some("pr:o.r.1".to_owned());
        assert_eq!(own_session(pr, "ABC-1"), None);
        assert_eq!(
            own_session(Some("ABC-1".into()), "ABC-1").as_deref(),
            Some("ABC-1")
        );
    }

    #[test]
    fn triage_has_a_window_of_its_own() {
        let ticket = |selected: Option<&str>, tab| {
            panes::Panes::new(panes::Surface::Ticket(panes::TicketView {
                selected: selected.map(Into::into),
                tab,
                ..empty_view()
            }))
        };
        let windows = [
            ticket(Some("a"), Tab::Walkthrough),
            ticket(Some("b"), Tab::Triage),
            ticket(None, Tab::Triage),
        ];
        assert_eq!(triage_window(&windows), Some(2));
        assert_eq!(triage_window(&windows[..2]), None);
    }

    #[test]
    fn a_new_window_opens_the_bundle_or_the_binary() {
        let (program, args) = launcher(Path::new("/Applications/Orbit.app/Contents/MacOS/orbit"));
        assert_eq!(program, Path::new("open"));
        assert_eq!(args, vec!["-n", "/Applications/Orbit.app"]);
        let (program, args) = launcher(Path::new("/repo/target/release/orbit"));
        assert_eq!(program, Path::new("/repo/target/release/orbit"));
        assert!(args.is_empty());
    }
}
