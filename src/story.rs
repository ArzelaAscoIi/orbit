//! Reads the team bot's trail on a ticket and turns it into what a reviewer
//! needs: which stage the ticket is in, whether it needs you and why, where
//! it came from, which story it belongs to, and what the bot did, in order.
//! The bot's name, its commands and the sweeps it files come from the
//! profile (profile.rs); the `🤖 …` markers are the protocol itself.
//!
//! The bot keeps no state of its own. Its `🤖 …` guard comments, the Linear
//! state, and the GitHub PR attachment are the whole record, so everything
//! here is derived from them and recomputed on every refresh.

use std::collections::HashSet;

use crate::linear::{Comment, Issue};

/// The bot whose `🤖 …` comments tell the story, found on each refresh as
/// the most frequent author of such comments (the profile's until then).
static BOT: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// The Linear workspace's name, set on each refresh (linear.rs `Home::org`).
static ORG: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

pub fn set_org(name: &str) {
    *ORG.write().unwrap() = name.trim().to_owned();
}

/// The user's own company: the profile's, else the Linear workspace's name.
pub fn our_company() -> String {
    match crate::profile::get().company.name.trim() {
        "" => ORG.read().unwrap().clone(),
        name => name.to_owned(),
    }
}

/// Never a customer: the profile's company and internal services, and the
/// Linear workspace itself, so a plain install does not list the user's
/// own company.
pub fn is_ours(name: &str) -> bool {
    is_ours_in(&crate::profile::get(), &ORG.read().unwrap(), name)
}

fn is_ours_in(profile: &crate::profile::Profile, org: &str, name: &str) -> bool {
    profile.is_ours(name) || (!org.is_empty() && name.trim().eq_ignore_ascii_case(org))
}

/// What a link to a ticket or a PR shows as a chip in markdown (ui
/// widgets): its title and whether it is done. Keyed by ticket identifier
/// and by PR URL (`https://github.com/o/r/pull/n`); set on each rebuild.
static MENTIONS: std::sync::RwLock<std::collections::BTreeMap<String, (String, bool)>> =
    std::sync::RwLock::new(std::collections::BTreeMap::new());

/// The tickets and PRs the board knows, for `mention`.
pub fn set_mentions<'a>(cards: impl Iterator<Item = &'a Card>) {
    let mut known = std::collections::BTreeMap::new();
    for card in cards {
        let issue = &card.issue;
        known.insert(
            issue.identifier.clone(),
            (issue.title.clone(), card.stage == Stage::Done),
        );
        for attachment in &issue.attachments.nodes {
            if let Some(pr) = card.prs.iter().find(|p| p.url == attachment.url) {
                known.insert(pr.url.clone(), (attachment.title.clone(), pr.merged));
            }
        }
    }
    *MENTIONS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = known;
}

/// A ticket (by identifier) or PR (by URL) the board knows: title, done.
pub fn mention(key: &str) -> Option<(String, bool)> {
    MENTIONS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(key)
        .cloned()
}

pub fn set_bot(name: &str) {
    *BOT.write().unwrap() = name.to_owned();
}

fn bot_is(name: &str) -> bool {
    let bot = BOT.read().unwrap();
    if bot.is_empty() {
        name == crate::profile::get().bot.name
    } else {
        *bot == name
    }
}

/// Who opened a ticket or a PR, for the chip beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    You,
    /// The team's bot, or any bot account (GitHub's `[bot]` apps).
    Bot,
    Other,
}

/// `name` wrote it; `yours` when that is the user (Linear's name, or
/// GitHub's own flag), `bot` when GitHub says the account is a bot.
pub fn who(name: &str, yours: bool, bot: bool) -> Who {
    if yours {
        Who::You
    } else if bot || bot_is(name) || name.ends_with("[bot]") {
        Who::Bot
    } else {
        Who::Other
    }
}

/// The bot's name for text: the one found on the board, else the
/// profile's, else "the bot".
pub fn bot() -> String {
    let found = BOT.read().unwrap().clone();
    if !found.is_empty() {
        return found;
    }
    crate::profile::get().bot_name("the bot").to_owned()
}

/// `bot()` at the start of a sentence.
fn bot_sentence() -> String {
    let name = bot();
    let mut chars = name.chars();
    chars
        .next()
        .map_or(name.clone(), |c| c.to_uppercase().chain(chars).collect())
}

/// The most frequent author of `🤖` comments across the issues.
pub fn find_bot(issues: &[Issue]) -> Option<String> {
    let mut counts: std::collections::HashMap<&str, usize> = Default::default();
    for issue in issues {
        for comment in &issue.comments.nodes {
            if comment.body.starts_with("🤖")
                && let Some(user) = &comment.user
            {
                *counts.entry(user.name.as_str()).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(name, _)| name.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Intake,
    Planning,
    Blocked,
    Building,
    Review,
    Testing,
    Done,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Intake => "Intake",
            Stage::Planning => "Planning",
            Stage::Blocked => "Blocked",
            Stage::Building => "Building",
            Stage::Review => "In review",
            Stage::Testing => "Testing",
            Stage::Done => "Done",
        }
    }

    /// Position on the Triage → Plan → Build → Review → Test → Done rail.
    pub fn step(self) -> usize {
        match self {
            Stage::Intake => 0,
            Stage::Planning | Stage::Blocked => 1,
            Stage::Building => 2,
            Stage::Review => 3,
            Stage::Testing => 4,
            Stage::Done => 5,
        }
    }

    /// Reading order inside a story: what needs you first, finished work last.
    fn rank(self) -> u8 {
        match self {
            Stage::Review => 0,
            Stage::Blocked => 1,
            Stage::Testing => 2,
            Stage::Building => 3,
            Stage::Planning => 4,
            Stage::Intake => 5,
            Stage::Done => 6,
        }
    }
}

pub const STEPS: [&str; 6] = ["Triage", "Plan", "Build", "Review", "Test", "Done"];

/// What a card says, as one number: a For you card marked read comes back
/// when this changes (stage, ask, state, a comment, a PR's state), never on
/// the clock alone, so `why`'s "2h" does not count.
pub fn read_key(card: &Card) -> u64 {
    let prs: String = card
        .prs
        .iter()
        .map(|p| format!("{}{}{}{}{};", p.url, p.open, p.merged, p.approved, p.draft))
        .collect();
    crate::store::text_hash(&format!(
        "{:?}|{:?}|{}|{}|{prs}",
        card.stage,
        card.ask,
        card.issue.state.name,
        card.issue.comments.nodes.len(),
    ))
}

/// The one thing a ticket asks of you.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Review,
    Decide,
    Verify,
}

impl Ask {
    pub fn label(self) -> &'static str {
        match self {
            Ask::Review => "Review the PR",
            Ask::Decide => "Decide",
            Ask::Verify => "Verify and close",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ci {
    Green,
    Red,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Pr {
    pub url: String,
    pub owner: String,
    pub repo: String,
    pub number: u64,
    /// Neither merged nor closed.
    pub open: bool,
    pub merged: bool,
    pub conflicts: bool,
    pub approved: bool,
    /// Marked draft on GitHub: not ready for review yet.
    pub draft: bool,
    pub opened_at: String,
}

impl Pr {
    /// `repo#7`.
    pub fn name(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }

    /// Open and not approved by a person: the reviewer's.
    /// Open, not a draft, not approved: waiting for a review.
    pub fn waiting(&self) -> bool {
        self.open && !self.approved && !self.draft
    }

    /// What the PR is, as its row shows it: Linear's answer, sharpened by
    /// GitHub's live one (`gate`, asked while Linear says open) and by a
    /// merge the user started here (`merging`) or finished (`merged`).
    pub fn state(
        &self,
        gate: Option<&crate::github::Gate>,
        merging: bool,
        merged: bool,
    ) -> PrState {
        let live = |state: &str| gate.is_some_and(|g| g.state == state);
        if self.merged || merged || live("MERGED") {
            PrState::Merged
        } else if !self.open || live("CLOSED") {
            PrState::Closed
        } else if merging || gate.is_some_and(|g| g.queued) {
            PrState::Merging
        } else if gate.is_some_and(|g| g.draft) {
            PrState::Draft
        } else if self.approved {
            PrState::Approved
        } else {
            PrState::Open
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrState {
    Draft,
    Open,
    Approved,
    /// In the merge queue, or the user's merge is running.
    Merging,
    Merged,
    Closed,
}

impl PrState {
    pub fn label(self) -> &'static str {
        match self {
            PrState::Draft => "draft",
            PrState::Open => "open",
            PrState::Approved => "approved",
            PrState::Merging => "merging",
            PrState::Merged => "merged",
            PrState::Closed => "closed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Event {
    pub at: String,
    pub who: String,
    pub verb: &'static str,
    pub tone: Tone,
    pub summary: String,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Bot,
    Human,
    /// The ticket's own Claude session, run from this app.
    Claude,
    Problem,
}

#[derive(Clone, Debug, Default)]
pub struct Origin {
    /// "Slack · ask from Sam", "Filed by the bot", …
    pub source: String,
    pub source_url: Option<String>,
    pub came_in_as: Option<String>,
    pub kind: Option<String>,
    pub area: Option<String>,
    pub customers: Option<String>,
    pub seen_again: usize,
}

#[derive(Clone, Debug)]
pub struct Plan {
    /// The bot's one-line header: kind, repos, effort.
    pub tldr: String,
    pub body: String,
}

/// The bot's case on whether to build at all (its plan's `### The ask`):
/// what the user decides on before any code exists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TheAsk {
    pub asked_by: String,
    pub need: String,
    pub evidence: String,
    pub smaller: String,
    /// `Build`, `Push back` or `Ask`.
    pub verdict: String,
    /// For Push back or Ask: what to tell the requester.
    pub reply: String,
}

impl TheAsk {
    pub fn build(&self) -> bool {
        self.verdict.eq_ignore_ascii_case("build")
    }
}

/// A plan's `### The ask`, when it has one: `- **Need:** …` lines up to
/// the next heading or the plan's `>>>` block.
pub fn the_ask(plan: &str) -> Option<TheAsk> {
    let mut lines = plan.lines().skip_while(|l| l.trim() != "### The ask");
    lines.next()?;
    let mut ask = TheAsk::default();
    for line in lines {
        let line = line.trim();
        if line.starts_with("###") || line.starts_with(">>>") {
            break;
        }
        let Some((key, value)) = line
            .strip_prefix("- **")
            .and_then(|rest| rest.split_once(":**"))
        else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_owned();
        match key.trim().to_lowercase().as_str() {
            "asked by" => ask.asked_by = value,
            "need" => ask.need = value,
            "evidence" => ask.evidence = value,
            "smaller answer" => ask.smaller = value,
            "verdict" => ask.verdict = value,
            "reply" => ask.reply = value,
            _ => {}
        }
    }
    (!ask.verdict.is_empty()).then_some(ask)
}

/// The bot's commands the app may post, on the user's explicit pick in a
/// ticket's step menu only. Its poller runs whatever matches
/// `@<anyone> <Command>!`, so no other text the app writes may look like one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum BotCommand {
    Plan,
    Build,
    /// A sweep's ticket checked again in depth (`bot.investigateCommand`).
    Investigate,
}

impl BotCommand {
    /// The whole comment, as the profile names it (`bot.planCommand`,
    /// `bot.buildCommand`); `None` when it names none: the step runs here
    /// only.
    pub fn comment(self) -> Option<String> {
        self.comment_in(&crate::profile::get())
    }

    pub(crate) fn comment_in(self, profile: &crate::profile::Profile) -> Option<String> {
        let command = match self {
            BotCommand::Plan => &profile.bot.plan_command,
            BotCommand::Build => &profile.bot.build_command,
            BotCommand::Investigate => &profile.bot.investigate_command,
        };
        Some(command.trim().to_owned()).filter(|c| !c.is_empty())
    }

    pub fn what(self) -> &'static str {
        match self {
            BotCommand::Plan => "plan",
            BotCommand::Build => "build",
            BotCommand::Investigate => "investigate",
        }
    }
}

/// Whether `text` holds what the bot's poller runs: `@<anyone> <Word>!`
/// (any name, any word, any case). Text a program writes must never.
pub fn has_bot_command(text: &str) -> bool {
    text.match_indices('@').any(|(at, _)| {
        let rest = &text[at + 1..];
        let name = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let after = rest[name..].trim_start();
        let word: usize = after
            .chars()
            .take_while(|c| c.is_alphabetic())
            .map(char::len_utf8)
            .sum();
        name > 0 && after.len() < rest.len() - name && word > 0 && after[word..].starts_with('!')
    })
}

/// Product work, or KTLO: what keeps the lights on and stays out of the
/// product views, so it never eats the time meant for building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Product,
    /// Security findings: a scanner's CVE tickets, the Security label.
    Security,
    /// The alerts and errors the bot's sweeps file.
    Alerts,
    /// Dependabot's PRs waiting on the user's review: no ticket behind them.
    Dependencies,
}

impl Lane {
    pub fn label(self) -> &'static str {
        match self {
            Lane::Product => "Product",
            Lane::Security => "Security",
            Lane::Alerts => "Alerts and errors",
            Lane::Dependencies => "Dependency updates",
        }
    }
}

/// Which lane a ticket is in, from who filed it, its labels and its title,
/// by the profile's KTLO rules.
pub fn lane(issue: &Issue) -> Lane {
    lane_in(&crate::profile::get(), issue)
}

fn lane_in(profile: &crate::profile::Profile, issue: &Issue) -> Lane {
    let rules = &profile.ktlo;
    let title = issue.title.to_lowercase();
    let labelled = |names: &[String]| {
        issue
            .labels
            .nodes
            .iter()
            .any(|l| names.iter().any(|n| l.name.eq_ignore_ascii_case(n)))
    };
    let by_scanner = issue
        .creator
        .as_ref()
        .is_some_and(|c| rules.security_creators.contains(&c.name));
    let titled = rules
        .security_titles
        .iter()
        .any(|t| title.contains(t.as_str()));
    if by_scanner || labelled(&rules.security_labels) || titled {
        return Lane::Security;
    }
    let swept = sweep_in(profile, &issue.title).is_some_and(|s| s.alert);
    if swept || labelled(&rules.alert_labels) {
        return Lane::Alerts;
    }
    Lane::Product
}

#[derive(Clone, Debug)]
pub struct Card {
    pub issue: Issue,
    pub lane: Lane,
    pub stage: Stage,
    pub ask: Option<Ask>,
    /// One sentence: why the ask, or what is happening right now.
    pub why: String,
    /// Every pull request linked to the ticket, newest first.
    pub prs: Vec<Pr>,
    pub ci: Ci,
    pub story_key: String,
    pub story: String,
    pub origin: Origin,
    pub plan: Option<Plan>,
    pub timeline: Vec<Event>,
    /// Customer organisations the ticket is for: Linear customer requests,
    /// or what Claude read from the ticket when there are none.
    pub customers: Vec<String>,
    /// The team's bot works this ticket (it commented on it).
    pub bot_works: bool,
}

impl Card {
    /// In Linear's Triage state: nobody has looked at it yet. What Intake
    /// shows; a Backlog ticket was looked at and parked (Find work).
    pub fn untriaged(&self) -> bool {
        self.stage == Stage::Intake && self.issue.state.kind == "triage"
    }

    /// Untriaged and filed in the last 24 hours: Intake's top block.
    pub fn new_today(&self, now: jiff::Timestamp) -> bool {
        self.untriaged()
            && self
                .issue
                .created_at
                .parse::<jiff::Timestamp>()
                .is_ok_and(|at| now.as_second() - at.as_second() < 24 * 3600)
    }

    /// The ticket's PR at `url`, or its lead one when `url` is `None` or
    /// not the ticket's: the newest waiting for review, else the newest
    /// open one, else the newest.
    pub fn pr(&self, url: Option<&str>) -> Option<&Pr> {
        url.and_then(|u| self.prs.iter().find(|p| p.url == u))
            .or_else(|| self.prs.iter().find(|p| p.waiting()))
            .or_else(|| self.prs.iter().find(|p| p.open))
            .or_else(|| self.prs.first())
    }
}

pub fn card(issue: Issue, parents: &HashSet<String>) -> Card {
    let mut prs = prs(&issue);
    // A PR closed without merging says nothing once there is another.
    if prs.iter().any(|p| p.open || p.merged) {
        prs.retain(|p| p.open || p.merged);
    }
    // The one to act on first: waiting for review, then open, then merged.
    prs.sort_by_key(|p| (!p.waiting(), !p.open, !p.merged));
    let bot_works = bot_works(&issue);
    let ci = ci(&issue.comments.nodes);
    let stage = stage(&issue, &prs);
    // A PR with no ticket is on the board because GitHub asked the user.
    let theirs = bot_works || issue.id.starts_with(PR_ONLY);
    let mut ask = ask(
        stage,
        &prs,
        &issue.comments.nodes,
        theirs,
        person_on_it(&issue),
    );
    let lane = lane(&issue);
    let mut why = why(&issue, stage, ask, &prs, ci);
    if let Some(nobody) = nobody_on_it(lane, stage, &issue) {
        why = nobody;
    }
    // A colleague's ask (Linear Asks, from Slack) not triaged yet is a call
    // to make now, not one row of Intake.
    if let Some(line) = open_ask(lane, stage, &issue).filter(|_| ask.is_none()) {
        ask = Some(Ask::Decide);
        why = line;
    }
    let (story_key, story) = story(&issue, parents);
    Card {
        lane,
        stage,
        ask,
        why,
        ci,
        story_key,
        story,
        origin: origin(&issue),
        plan: plan(&issue.comments.nodes),
        timeline: timeline(&issue.comments.nodes),
        customers: {
            let mut names = linked_customers(&issue);
            names.retain(|n| !is_ours(n));
            names
        },
        bot_works,
        prs,
        issue,
    }
}

/// GitHub's live answer (`gates`) over Linear's PR metadata, which lags: a
/// PR GitHub calls approved or queued asks nobody to review it.
pub fn live(
    card: &mut Card,
    gates: &std::collections::HashMap<String, Result<crate::github::Gate, String>>,
) {
    let mut changed = false;
    for pr in card.prs.iter_mut().filter(|p| p.waiting()) {
        if let Some(Ok(gate)) = gates.get(&pr.url)
            && (gate.review == "APPROVED" || gate.queued)
        {
            pr.approved = true;
            changed = true;
        }
    }
    if !changed {
        return;
    }
    if card.ask == Some(Ask::Review) {
        card.ask = ask(
            card.stage,
            &card.prs,
            &card.issue.comments.nodes,
            card.bot_works,
            person_on_it(&card.issue),
        );
    }
    // The status line too, the user's own PR included: never "waiting for
    // review" under an approval GitHub already reports.
    card.why = why(&card.issue, card.stage, card.ask, &card.prs, card.ci);
}

/// The ship session's prompt (skills/ship): the tickets (`what`: "open
/// KTLO tickets", "of the bot's PRs waiting on your review") with their
/// stage, what they wait on and their PRs, and who owns what.
pub fn ship_brief(skill: &str, what: &str, cards: &[&Card], routing: &str) -> String {
    let mut out = format!("/{skill} There are {} {what}:\n\n", cards.len());
    for (i, card) in cards.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} · {}: {}\n   {}\n   {}\n",
            i + 1,
            card.issue.identifier,
            card.stage.label(),
            card.issue.title,
            card.why.trim(),
            card.issue.url,
        ));
        for pr in card.prs.iter().filter(|p| p.open || p.merged) {
            out.push_str(&format!("   PR: {}\n", pr.url));
        }
    }
    if !routing.trim().is_empty() {
        out.push_str(&format!("\nWho owns what:\n{}\n", routing.trim()));
    }
    out
}

/// The id prefix of a card that is only a PR, with no ticket (`pr_card`).
pub const PR_ONLY: &str = "pr:";

/// The PR a `pr:<owner>.<repo>.<number>` key is about. Repo names may hold
/// dots, so the owner is the first part and the number the last.
pub fn pr_url_of(key: &str) -> Option<String> {
    let rest = key.strip_prefix(PR_ONLY)?;
    let (owner, rest) = rest.split_once('.')?;
    let (repo, number) = rest.rsplit_once('.')?;
    number.parse::<u64>().ok()?;
    Some(format!("https://github.com/{owner}/{repo}/pull/{number}"))
}

/// A PR with no ticket as a card, so it opens like one: the diff, the
/// walkthrough, the gate, Claude. Nothing Linear: `Card::pr_only`. The id
/// has no slash, it names the session's status log.
pub fn pr_card(url: &str, title: &str, at: &str) -> Option<Card> {
    pr_card_as(url, title, at, "open", None)
}

/// The card with what GitHub says of the PR: its title as the headline and
/// its description as the ticket's, and whether it is still open.
pub fn pr_card_from(url: &str, at: &str, gate: &crate::github::Gate) -> Option<Card> {
    let status = match gate.state.as_str() {
        "MERGED" => "merged",
        "CLOSED" => "closed",
        _ => "open",
    };
    let title = Some(gate.title.trim()).filter(|t| !t.is_empty());
    let body = Some(gate.body.trim()).filter(|b| !b.is_empty());
    let name = pr_from_url(url, at).map(|p| format!("{}#{}", p.repo, p.number));
    pr_card_as(url, title.or(name.as_deref())?, at, status, body)
}

fn pr_card_as(
    url: &str,
    title: &str,
    at: &str,
    status: &str,
    description: Option<&str>,
) -> Option<Card> {
    let pr = pr_from_url(url, at)?;
    let (state, kind) = match status {
        "merged" => ("Done", "completed"),
        "closed" => ("Canceled", "canceled"),
        _ => ("In Review", "started"),
    };
    let issue = serde_json::from_value(serde_json::json!({
        "id": format!("{PR_ONLY}{}.{}.{}", pr.owner, pr.repo, pr.number),
        "identifier": format!("{}#{}", pr.repo, pr.number),
        "title": title,
        "url": url,
        "description": description,
        "updatedAt": at,
        "state": { "id": "", "name": state, "color": "", "position": 0.0, "type": kind },
        "team": { "id": "" },
        "assignee": null,
        "creator": null,
        "parent": null,
        "project": null,
        "attachments": { "nodes": [{
            "url": url,
            "title": title,
            "sourceType": "github",
            "metadata": { "repoLogin": pr.owner, "repoName": pr.repo, "number": pr.number, "status": status, "createdAt": at },
        }] },
        "comments": { "nodes": [] },
    }))
    .ok()?;
    Some(card(issue, &HashSet::new()))
}

/// Takes the dependency lane's latest list into the PR-only cards: new PRs
/// join, one the search no longer finds (merged, closed, reviewed) leaves
/// unless `keep` (the one open) is it, and a PR already opened from the
/// Inbox moves into the lane.
pub fn take_dependencies(
    pr_cards: &mut Vec<Card>,
    prs: &[crate::github::MyPr],
    keep: Option<&str>,
) {
    let fresh: Vec<Card> = prs
        .iter()
        .filter_map(|p| pr_card(&p.url, &p.title, &p.at))
        .collect();
    pr_cards.retain(|c| {
        c.lane != Lane::Dependencies
            || keep == Some(c.issue.id.as_str())
            || fresh.iter().any(|f| f.issue.id == c.issue.id)
    });
    for mut card in fresh {
        match pr_cards.iter_mut().find(|c| c.issue.id == card.issue.id) {
            Some(known) => known.lane = Lane::Dependencies,
            None => {
                card.lane = Lane::Dependencies;
                pr_cards.push(card);
            }
        }
    }
}

impl Card {
    /// A PR with no ticket behind it (`pr_card`): no Linear state, comments
    /// or bot commands.
    pub fn pr_only(&self) -> bool {
        self.issue.id.starts_with(PR_ONLY)
    }

    /// The Testing tab is on every ticket, whatever its stage; a PR with no
    /// ticket has none (its plan is a comment on the ticket).
    pub fn has_testing(&self) -> bool {
        !self.pr_only()
    }
}

/// Cards in reading order: needs-you first, then by pipeline stage, newest
/// activity first within a stage.
pub fn sort(cards: &mut [&Card]) {
    cards.sort_by(|a, b| {
        (a.ask.is_none(), a.stage.rank())
            .cmp(&(b.ask.is_none(), b.stage.rank()))
            .then_with(|| b.issue.updated_at.cmp(&a.issue.updated_at))
    });
}

/// Customers from Linear's customer requests, without duplicates.
fn linked_customers(issue: &Issue) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in issue
        .needs
        .nodes
        .iter()
        .filter_map(|n| n.customer.as_ref().map(|c| c.name.clone()))
    {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Where a ticket stands for its customer, coarsely.
pub fn bucket(card: &Card) -> &'static str {
    match (card.ask, card.stage) {
        (Some(_), _) => "Needs you",
        (_, Stage::Testing | Stage::Done) => "Shipped",
        _ => "Moving",
    }
}

/// The text Claude reads to name a ticket's customers.
pub fn customer_text(card: &Card) -> String {
    let description: String = card
        .issue
        .description
        .as_deref()
        .unwrap_or_default()
        .chars()
        .take(600)
        .collect();
    format!(
        "{} | customers noted by the bot: {} | {}",
        card.issue.title,
        card.origin.customers.as_deref().unwrap_or("none"),
        description.replace('\n', " ")
    )
}

fn by_bot(comment: &Comment) -> bool {
    comment.user.as_ref().is_some_and(|u| bot_is(&u.name))
}

/// A person took the ticket: it is assigned to someone other than the bot,
/// and the bot neither took it itself nor dispatched its build. The bot's
/// triage or routing comments alone do not make it the bot's work.
fn person_on_it(issue: &Issue) -> bool {
    let comments = &issue.comments.nodes;
    issue.assignee.as_ref().is_some_and(|a| !bot_is(&a.name))
        && bot_says(comments, "🤖 Dispatched Build").is_none()
        && bot_says(comments, "🤖 Assigned to myself").is_none()
}

/// The team's bot works this ticket: it commented, and no person took it.
fn bot_works(issue: &Issue) -> bool {
    issue.comments.nodes.iter().any(by_bot) && !person_on_it(issue)
}

/// The bot's ack ("picked this up", "I'm on it") when it is still its last
/// word on the ticket: it is working and has nothing to show yet.
fn picked_up(comments: &[Comment]) -> Option<&Comment> {
    comments.iter().rev().find(|c| by_bot(c)).filter(|c| {
        ["picked this up", "I'm on it"]
            .iter()
            .any(|ack| c.body.trim().starts_with(ack))
    })
}

/// The bot's clarification ask ("🤖 I need a bit more info") when nobody
/// has written since: triage waits on the reporter.
fn asked_more(comments: &[Comment]) -> Option<&Comment> {
    comments
        .last()
        .filter(|c| by_bot(c) && c.body.starts_with("🤖 I need a bit more info"))
}

fn bot_says<'a>(comments: &'a [Comment], prefix: &str) -> Option<&'a Comment> {
    comments
        .iter()
        .rev()
        .find(|c| by_bot(c) && c.body.starts_with(prefix))
}

fn stage(issue: &Issue, prs: &[Pr]) -> Stage {
    let name = issue.state.name.to_lowercase();
    // Landed: something merged and nothing still open to review.
    let landed = prs.iter().any(|p| p.merged) && !prs.iter().any(|p| p.open);
    // State names differ per team ("In Review", "Code Review", "QA",
    // "Testing", "Blocked", "On hold"); the words, not the exact names.
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    let (blocked, testing, review) = (
        has(&["blocked", "on hold"]),
        has(&["testing", "qa", "verify", "verification"]),
        has(&["review"]),
    );
    match issue.state.kind.as_str() {
        "completed" | "canceled" => Stage::Done,
        _ if blocked => Stage::Blocked,
        _ if testing => Stage::Testing,
        _ if review && landed => Stage::Testing,
        _ if review => Stage::Review,
        "triage" | "backlog" => Stage::Intake,
        _ if bot_says(&issue.comments.nodes, "🤖 Dispatched Build").is_some() => Stage::Building,
        // A person's open PR is in review, whatever their team calls the state.
        "started" if !bot_works(issue) && prs.iter().any(|p| p.open && !p.draft) => Stage::Review,
        // Started by a person, not the bot: being built.
        "started" if !bot_works(issue) => Stage::Building,
        _ => Stage::Planning,
    }
}

/// The bot's note when it blocks a ticket for a human call.
const ROUTED_BLOCKED: &str = "🤖 Routed to **Blocked**";

/// The bot blocked it and waits on a human call: its note is the last word,
/// no person has answered since. A ticket a person blocked waits on them or
/// on what they named, not on the user.
fn blocked_by_bot(comments: &[Comment]) -> bool {
    comments
        .iter()
        .rposition(|c| by_bot(c) && c.body.starts_with(ROUTED_BLOCKED))
        .is_some_and(|i| !comments[i + 1..].iter().any(|c| !by_bot(c)))
}

fn ask(stage: Stage, prs: &[Pr], comments: &[Comment], bot: bool, person: bool) -> Option<Ask> {
    match stage {
        // An approved PR is the bot's to land, not yours to review again.
        // Only the bot's PRs wait on the user; a person's PR asks the user
        // when GitHub requests their review (Notifications has those).
        Stage::Review if bot && prs.iter().any(Pr::waiting) => Some(Ask::Review),
        Stage::Blocked if blocked_by_bot(comments) => Some(Ask::Decide),
        // A ticket a person took is theirs to verify, not the user's.
        Stage::Testing if !person => Some(Ask::Verify),
        _ => None,
    }
}

/// PRs whose review the user handed to someone (a review request, the
/// review roulette): a ticket whose every waiting PR is handed off asks the
/// user nothing more and says who has it.
pub fn hand_off(card: &mut Card, handed: &std::collections::HashMap<String, String>) {
    if card.ask != Some(Ask::Review) {
        return;
    }
    let waiting: Vec<&Pr> = card.prs.iter().filter(|p| p.waiting()).collect();
    if waiting.is_empty() || !waiting.iter().all(|p| handed.contains_key(&p.url)) {
        return;
    }
    let mut who: Vec<&str> = waiting.iter().map(|p| handed[&p.url].as_str()).collect();
    who.dedup();
    card.ask = None;
    card.why = format!("Review asked from {}.", who.join(", "));
}

/// A PR the ticket's Claude session opened, before Linear lists it: open,
/// unreviewed, not yet checked.
pub fn pr_from_url(url: &str, opened_at: &str) -> Option<Pr> {
    let mut parts = url.strip_prefix("https://github.com/")?.split('/');
    let (owner, repo, pull, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let number = number.parse().ok()?;
    (pull == "pull").then(|| Pr {
        url: url.to_owned(),
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        number,
        open: true,
        merged: false,
        conflicts: false,
        approved: false,
        draft: false,
        opened_at: opened_at.to_owned(),
    })
}

/// In another team, the state a moved ticket takes: the first of the same
/// kind (Todo stays Todo); a kind the team lacks falls back to its backlog,
/// then its first state.
pub fn state_like<'a>(
    states: &'a [crate::linear::State],
    kind: &str,
) -> Option<&'a crate::linear::State> {
    let first = |k: &str| {
        states
            .iter()
            .filter(|s| s.kind == k)
            .min_by(|a, b| a.position.total_cmp(&b.position))
    };
    first(kind).or_else(|| first("backlog")).or_else(|| {
        states
            .iter()
            .min_by(|a, b| a.position.total_cmp(&b.position))
    })
}

/// Someone's ask that came in through Linear Asks (its "Ask from …" Slack
/// link) and is not triaged yet: "Jo's ask · High · 2h ago: triage it".
fn open_ask(lane: Lane, stage: Stage, issue: &Issue) -> Option<String> {
    // Still in Triage: in the Backlog someone already looked at it.
    if lane != Lane::Product || stage != Stage::Intake || issue.state.kind != "triage" {
        return None;
    }
    let slack =
        issue.attachments.nodes.iter().find(|a| {
            a.source_type.as_deref() == Some("slack") || a.title.starts_with("Ask from ")
        })?;
    let who = issue
        .creator
        .as_ref()
        .map(|c| c.name.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| slack.title.trim_start_matches("Ask from "));
    let first = who.split_whitespace().next().unwrap_or(who);
    let priority = match issue.priority as u8 {
        1 => " · Urgent",
        2 => " · High",
        _ => "",
    };
    Some(format!(
        "{first}'s ask{priority} · {}: triage it",
        ago(&issue.created_at)
    ))
}

/// KTLO stays out of the product views: one nobody picked up says so.
fn nobody_on_it(lane: Lane, stage: Stage, issue: &Issue) -> Option<String> {
    (lane != Lane::Product && stage == Stage::Intake && issue.assignee.is_none())
        .then(|| format!("Nobody on it · filed {}", ago(&issue.created_at)))
}

/// KTLO nobody handles: not picked up (Triage or backlog), no owner, a day
/// old or more. Nothing else surfaces it, since KTLO never shows in the
/// product views; the KTLO tab turns red for it, and an urgent or high one
/// pings the Mac.
pub fn unhandled(card: &Card, now: jiff::Timestamp) -> bool {
    let old = card
        .issue
        .created_at
        .parse::<jiff::Timestamp>()
        .is_ok_and(|at| now.duration_since(at) >= jiff::SignedDuration::from_hours(24));
    card.lane != Lane::Product
        && card.stage == Stage::Intake
        && card.issue.assignee.is_none()
        && old
}

/// Urgent or high in Linear.
pub fn pressing(card: &Card) -> bool {
    (1.0..=2.0).contains(&card.issue.priority)
}

/// The team's state a canceled ticket goes to: the first of type
/// canceled (Linear's Canceled, not Duplicate, which has its own type).
pub fn canceled_state(states: &[crate::linear::State]) -> Option<&crate::linear::State> {
    states
        .iter()
        .filter(|s| s.kind == "canceled")
        .min_by(|a, b| a.position.total_cmp(&b.position))
}

/// The identifier in a Linear issue link (`https://linear.app/<ws>/issue/ENG-1/slug`).
pub fn issue_from_url(url: &str) -> Option<String> {
    let mut parts = url.strip_prefix("https://linear.app/")?.split('/').skip(1);
    match parts.next()? {
        "issue" => parts.next().map(str::to_uppercase),
        // A PR's review page, `review/web-1099-align-…`: named after the PR
        // title, which starts with its ticket.
        "review" => {
            let slug = parts.next()?;
            let mut words = slug.split('-');
            let (team, number) = (words.next()?, words.next()?);
            let ok = !team.is_empty()
                && team.chars().all(|c| c.is_ascii_alphanumeric())
                && number.parse::<u64>().is_ok();
            ok.then(|| format!("{}-{number}", team.to_uppercase()))
        }
        _ => None,
    }
}

/// The ticket's GitHub PR attachments, newest first (Linear lists them
/// oldest first).
fn prs(issue: &Issue) -> Vec<Pr> {
    issue
        .attachments
        .nodes
        .iter()
        .rev()
        .filter(|a| a.source_type.as_deref() == Some("github") && a.url.contains("/pull/"))
        .map(|attachment| {
            let meta = &attachment.metadata;
            let text = |key: &str| meta[key].as_str().unwrap_or_default().to_owned();
            let status = text("status");
            let merged = status == "merged" || !meta["mergedAt"].is_null();
            Pr {
                owner: text("repoLogin"),
                repo: text("repoName"),
                number: meta["number"].as_u64().unwrap_or_default(),
                open: !merged && status != "closed",
                merged,
                conflicts: meta["hasConflicts"].as_bool().unwrap_or(false),
                approved: meta["reviews"].as_array().is_some_and(|reviews| {
                    reviews
                        .iter()
                        .any(|r| r["state"] == "approved" && r["isBot"] != true)
                }),
                draft: status == "draft" || meta["isDraft"].as_bool().unwrap_or(false),
                opened_at: text("createdAt"),
                url: attachment.url.clone(),
            }
        })
        .collect()
}

/// CI as last reported on the ticket, by the bot or by a person's fix note.
fn ci(comments: &[Comment]) -> Ci {
    for comment in comments.iter().rev() {
        let body = comment.body.to_lowercase();
        if !body.contains("ci") {
            continue;
        }
        if body.contains("ci failing") || body.contains("ci red") || body.contains("red ci") {
            // "Fixed the red CI" is good news.
            return if body.contains("fixed the red ci") {
                Ci::Green
            } else {
                Ci::Red
            };
        }
        if body.contains("ci is green") || body.contains("ci green") {
            return Ci::Green;
        }
    }
    Ci::Unknown
}

fn why(issue: &Issue, stage: Stage, ask: Option<Ask>, prs: &[Pr], ci: Ci) -> String {
    let comments = &issue.comments.nodes;
    // Whether the team's bot works this ticket: its words only then.
    let botted = bot_works(issue);
    // One PR by name; several by count, since the ticket lists each on a
    // row of its own.
    let named = |keep: fn(&Pr) -> bool, several: &str| {
        let kept: Vec<&Pr> = prs.iter().filter(|p| keep(p)).collect();
        match kept.as_slice() {
            [] => String::new(),
            [one] => one.name(),
            many => format!("{} {several}", many.len()),
        }
    };
    match (ask, stage) {
        (Some(Ask::Review), _) => {
            let waiting: Vec<&Pr> = prs.iter().filter(|p| p.waiting()).collect();
            let mut parts = vec![named(Pr::waiting, "PRs to review")];
            match ci {
                Ci::Green => parts.push("CI green".into()),
                Ci::Red => parts.push("CI red".into()),
                Ci::Unknown => {}
            }
            if waiting.iter().any(|p| p.conflicts) {
                parts.push("merge conflicts".into());
            }
            // The longest wait; the timestamps share one ISO format.
            if let Some(oldest) = waiting
                .iter()
                .map(|p| p.opened_at.as_str())
                .filter(|at| !at.is_empty())
                .min()
            {
                parts.push(format!("waiting {}", ago(oldest)));
            }
            parts.join(" · ")
        }
        (Some(Ask::Decide), _) => bot_says(comments, ROUTED_BLOCKED)
            .map(|c| first_sentence(c.body.trim_start_matches("🤖 Routed to **Blocked** — ")))
            .unwrap_or_else(|| "Blocked, no reason recorded on the ticket.".into()),
        // Blocked by a person: their latest word is the reason.
        (None, Stage::Blocked) => comments
            .iter()
            .rev()
            .find(|c| !by_bot(c) && !c.body.trim().is_empty())
            .and_then(|c| Some((c.user.as_ref()?.name.clone(), first_sentence(&c.body))))
            .map_or_else(
                // No word on it: the person it is assigned to holds it.
                || match issue.assignee.as_ref() {
                    Some(a) => format!("Waiting on {}: no reason recorded on the ticket.", a.name),
                    None => "Blocked, no reason recorded on the ticket.".into(),
                },
                |(who, why)| format!("Waiting on {who}: {why}"),
            ),
        (Some(Ask::Verify), _) => match named(|p| p.merged, "PRs") {
            merged if merged.is_empty() => "Merged. Check it works, then close it.".into(),
            merged => format!("{merged} merged. Check it works, then close it."),
        },
        // A person's PR their team still reviews.
        (_, Stage::Review) if !botted && prs.iter().any(Pr::waiting) => {
            let who = issue
                .assignee
                .as_ref()
                .map_or("unassigned", |a| a.name.as_str());
            format!("{} waiting for review · {who}", named(Pr::waiting, "PRs"))
        }
        (_, Stage::Review) if !botted => "PR approved: ready to merge.".into(),
        (_, Stage::Review) => format!(
            "PR approved. {} lands it through the merge queue.",
            bot_sentence()
        ),
        (_, Stage::Building) if !botted => {
            let who = issue
                .assignee
                .as_ref()
                .map_or("unassigned", |a| a.name.as_str());
            format!("{} · {who}", issue.state.name)
        }
        (_, Stage::Building) => format!("{} is writing the code.", bot_sentence()),
        // The bot asked the reporter and nobody answered: it is stuck.
        (None, Stage::Intake | Stage::Planning) if asked_more(comments).is_some() => {
            let reason = asked_more(comments)
                .and_then(|c| {
                    c.body
                        .lines()
                        .skip(1)
                        .map(str::trim)
                        .find(|l| !l.is_empty())
                        .map(first_sentence)
                })
                .unwrap_or_default();
            format!("{} waits on the reporter: {reason}", bot_sentence())
                .trim_end_matches([':', ' '])
                .to_owned()
        }
        // The bot said it is on it and has not said more yet.
        (None, Stage::Intake | Stage::Planning) if picked_up(comments).is_some() => {
            let when = picked_up(comments)
                .map(|c| ago(&c.created_at))
                .unwrap_or_default();
            let when = if when.is_empty() {
                String::new()
            } else {
                format!(" {when} ago")
            };
            format!("{} picked it up{when}, assessment coming.", bot_sentence())
        }
        (_, Stage::Planning) if bot_says(comments, "🤖 Assigned to myself").is_some() => {
            format!("Plan done, {} took it and builds next.", bot())
        }
        (_, Stage::Planning) if plan(comments).is_some() => {
            "Plan posted, waiting to be dispatched.".into()
        }
        // A ticket the bot never touched: its plain Linear state.
        (_, Stage::Planning) if !botted => {
            let who = issue
                .assignee
                .as_ref()
                .map_or("unassigned", |a| a.name.as_str());
            format!("{} · {who}", issue.state.name)
        }
        (_, Stage::Planning) => format!("Waiting for {}'s plan.", bot()),
        (_, Stage::Intake) => format!("In {}, not pulled yet.", issue.state.name),
        (_, Stage::Done) => "Done.".into(),
        _ => String::new(),
    }
}

/// A pasted GitHub PR or Linear ticket link Orbit can open, cleaned up
/// (trimmed, trailing punctuation off); anything else: none.
pub fn openable(text: &str) -> Option<String> {
    let url = text
        .trim()
        .trim_end_matches(['.', ',', ';', ')', '>'])
        .to_owned();
    if url.contains(char::is_whitespace) {
        return None;
    }
    (pr_from_url(&url, "").is_some() || issue_from_url(&url).is_some()).then_some(url)
}

/// The ticket a work folder is for: the part after `/work/`, when it is
/// an identifier (`DEV-207`), with or without a repo folder under it.
pub fn ticket_of_folder(cwd: &str) -> Option<String> {
    let rest = cwd.rsplit_once("/work/")?.1;
    let first = rest.split('/').next()?;
    let (team, n) = first.split_once('-')?;
    (!team.is_empty() && team.chars().all(|c| c.is_ascii_uppercase()) && n.parse::<u32>().is_ok())
        .then(|| first.to_owned())
}

/// Parent issue, then project, then the product area the bot's triage
/// named, then the sweep that filed it.
fn story(issue: &Issue, parents: &HashSet<String>) -> (String, String) {
    if let Some(parent) = &issue.parent {
        return (
            format!("parent:{}", parent.id),
            format!("{} {}", parent.identifier, parent.title),
        );
    }
    if parents.contains(&issue.id) {
        return (
            format!("parent:{}", issue.id),
            format!("{} {}", issue.identifier, issue.title),
        );
    }
    if let Some(project) = &issue.project {
        return (format!("project:{}", project.name), project.name.clone());
    }
    if let Some(area) = triage_field(&issue.comments.nodes, "Area") {
        return (format!("area:{area}"), area);
    }
    if let Some(flow) = filing_flow(&issue.title) {
        return (format!("flow:{flow}"), flow.to_owned());
    }
    ("loose".into(), "Loose tickets".into())
}

/// The profile's sweep whose title prefix this ticket has: the bot's
/// scheduled runs file tickets titled like `[prod] ...`.
fn sweep(title: &str) -> Option<crate::profile::Sweep> {
    sweep_in(&crate::profile::get(), title)
}

fn sweep_in(profile: &crate::profile::Profile, title: &str) -> Option<crate::profile::Sweep> {
    let title = title.to_lowercase();
    profile
        .ktlo
        .sweeps
        .iter()
        .find(|s| !s.prefix.is_empty() && title.starts_with(&s.prefix.to_lowercase()))
        .cloned()
}

fn filing_flow(title: &str) -> Option<String> {
    sweep(title).map(|s| s.story)
}

/// A `**Name:** value` line from the bot's triage assessment.
fn triage_field(comments: &[Comment], name: &str) -> Option<String> {
    let needle = format!("**{name}:**");
    comments.iter().filter(|c| by_bot(c)).find_map(|c| {
        c.body
            .lines()
            .find_map(|line| line.trim().strip_prefix(&needle))
            .map(|value| plain(value.trim()))
    })
}

fn origin(issue: &Issue) -> Origin {
    let comments = &issue.comments.nodes;
    let slack = issue
        .attachments
        .nodes
        .iter()
        .find(|a| a.source_type.as_deref() == Some("slack"));
    let creator = issue
        .creator
        .as_ref()
        .map_or("Someone", |c| c.name.as_str());
    let (source, source_url) = match (slack, creator) {
        (Some(slack), _) => (
            format!("Slack · {}", slack.title.trim_start_matches("Ask from ")),
            Some(slack.url.clone()),
        ),
        (None, name) if bot_is(name) => (
            match filing_flow(&issue.title) {
                Some(flow) => format!("{} sweep · {flow}", bot_sentence()),
                None => format!("Filed by {}", bot()),
            },
            None,
        ),
        (None, name) => (format!("Filed by {name}"), None),
    };
    Origin {
        source,
        source_url,
        // Linear keeps every identifier a ticket had; the first is where it started.
        came_in_as: issue.previous_identifiers.first().cloned(),
        kind: triage_field(comments, "Type"),
        area: triage_field(comments, "Area"),
        customers: triage_field(comments, "Customers"),
        seen_again: comments
            .iter()
            .filter(|c| {
                by_bot(c)
                    && ["🤖 Still firing", "🤖 Saw this again", "🤖 Still recurring"]
                        .iter()
                        .any(|p| c.body.starts_with(p))
            })
            .count(),
    }
}

fn plan(comments: &[Comment]) -> Option<Plan> {
    let comment = bot_says(comments, "🤖 Here's my implementation plan")
        .or_else(|| bot_says(comments, "🤖 Here is my implementation plan"))?;
    let body: String = comment.body.lines().skip(1).collect::<Vec<_>>().join("\n");
    // The header line comes after the ask's section, when the plan has one.
    let after_ask = body
        .lines()
        .skip_while(|l| {
            l.trim() == "### The ask" || l.trim().starts_with("- **") || l.trim().is_empty()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tldr = after_ask
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with(">>>") && !l.starts_with('#'))
        .map(plain)
        .unwrap_or_default();
    Some(Plan { tldr, body })
}

fn timeline(comments: &[Comment]) -> Vec<Event> {
    comments
        .iter()
        .filter_map(|c| {
            let who = c.user.as_ref().map_or("Linear", |u| u.name.as_str());
            let body = c.body.trim();
            if body.starts_with("This comment thread is synced") {
                return None;
            }
            // A local session posts its 🤖 markers as the user: still a step.
            let (verb, tone, rest) = if by_bot(c) || body.starts_with("🤖") {
                classify(body)
            } else {
                ("commented", Tone::Human, body)
            };
            Some(Event {
                at: c.created_at.clone(),
                who: who.to_owned(),
                verb,
                tone,
                summary: summarize(verb, rest, body),
                body: c.body.clone(),
            })
        })
        .collect()
}

/// A step of the work (planned, building, a PR, blocked) or a message
/// around it (a question, an answer, a comment): steps lead the timeline,
/// messages sit quiet under them.
pub fn is_step(verb: &str) -> bool {
    !matches!(verb, "commented" | "answered" | "asks" | "noted")
}

/// The timeline as it reads best: every summary plain text without the
/// 🤖 marker, the same step or message said again dropped (a PR opened
/// three times is opened once), oldest first.
pub fn condense(events: &[Event]) -> Vec<Event> {
    let mut seen = HashSet::new();
    events
        .iter()
        .filter_map(|e| {
            let summary = plain(e.summary.trim_start_matches("🤖").trim());
            seen.insert((e.verb, summary.clone())).then(|| Event {
                summary,
                ..e.clone()
            })
        })
        .collect()
}

/// Which messages a timeline shows folded: in a run of more than `keep`
/// messages between two steps, all but the last `keep` (the latest say
/// most). `true` hides that event until its run is opened.
pub fn folded(events: &[Event], keep: usize) -> Vec<bool> {
    let mut out = vec![false; events.len()];
    let mut start = 0;
    for i in 0..=events.len() {
        let message = events.get(i).is_some_and(|e| !is_step(e.verb));
        if !message {
            let run = i - start;
            if run > keep {
                out[start..i - keep].iter_mut().for_each(|h| *h = true);
            }
            start = i + 1;
        }
    }
    out
}

/// The day a timestamp falls on, as a heading: Today, Yesterday, else the
/// weekday and date (`Thu 8 Oct`).
pub fn day(timestamp: &str) -> String {
    let Ok(then) = timestamp.parse::<jiff::Timestamp>() else {
        return String::new();
    };
    let tz = jiff::tz::TimeZone::system();
    let (then, today) = (then.to_zoned(tz.clone()).date(), jiff::Zoned::now().date());
    match (today - then).get_days() {
        0 if then == today => "Today".into(),
        _ if then.tomorrow().ok() == Some(today) => "Yesterday".into(),
        _ => then.strftime("%a %-d %b").to_string(),
    }
}

/// The time of day, `14:05`, for a step's row.
pub fn clock(timestamp: &str) -> String {
    timestamp
        .parse::<jiff::Timestamp>()
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// One plain line per event, specific to what happened: which PR and how
/// CI stood, where it was routed, what the triage found.
fn summarize(verb: &str, rest: &str, body: &str) -> String {
    let line = match verb {
        "assessed it" => {
            let field = |name: &str| {
                body.lines()
                    .find_map(|l| l.trim().strip_prefix(&format!("**{name}:**")))
                    .map(|v| plain(v.trim()))
            };
            ["Type", "Area", "Customers"]
                .iter()
                .filter_map(|name| field(name))
                .collect::<Vec<_>>()
                .join(" · ")
        }
        "routed it" => format!("To {}", first_sentence(rest).trim_end_matches('.')),
        "opened a PR" | "updated the PR" => {
            let pr = rest
                .split_whitespace()
                .find(|w| w.contains("/pull/"))
                .map(short_link)
                .unwrap_or_default();
            let lower = body.to_lowercase();
            let ci = if lower.contains("ci failing") || lower.contains("ci red") {
                " · CI failing"
            } else if lower.contains("ci is green") || lower.contains("ci green") {
                " · CI green"
            } else if lower.contains("ci still running") {
                " · CI running"
            } else {
                ""
            };
            format!("{pr}{ci}")
        }
        _ => first_sentence(rest),
    };
    let line = line
        .trim_start_matches(['—', '–', '-', ':', '·', ' '])
        .to_owned();
    let line = shorten_links(&line);
    // Sentences start upper case; a repo name keeps its own spelling.
    if matches!(verb, "opened a PR" | "updated the PR") {
        return line;
    }
    let mut chars = line.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Every link in a line shortened with `short_link`.
pub fn shorten_links(line: &str) -> String {
    line.split_whitespace()
        .map(|w| {
            if w.starts_with("http") {
                short_link(w)
            } else {
                w.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `https://github.com/org/repo/pull/7` → `repo#7`; any other link → its host.
fn short_link(word: &str) -> String {
    let url = word.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | '<' | '>'));
    let parts: Vec<&str> = url.split('/').collect();
    match parts.as_slice() {
        [_, _, "github.com", _, repo, "pull" | "issues", n, ..] => format!("{repo}#{n}"),
        [_, _, "github.com", _, repo, ..] => (*repo).to_owned(),
        [_, _, host, ..] => (*host).to_owned(),
        _ => url.to_owned(),
    }
}

fn classify(body: &str) -> (&'static str, Tone, &str) {
    const VERBS: &[(&str, &str, Tone)] = &[
        ("🤖 Routed to **Blocked** — ", "blocked it", Tone::Problem),
        ("🤖 Routed to ", "routed it", Tone::Bot),
        (
            "🤖 Here's my implementation plan",
            "planned the fix",
            Tone::Bot,
        ),
        (
            "🤖 Here is my implementation plan",
            "planned the fix",
            Tone::Bot,
        ),
        ("🤖 Assigned to myself", "took it", Tone::Bot),
        ("🤖 Dispatched Build", "started building", Tone::Bot),
        ("🤖 Assigned to ", "took it", Tone::Bot),
        (
            "🤖 Fixing the review comments",
            "is fixing the review",
            Tone::Bot,
        ),
        ("🤖 PR opened → ", "opened a PR", Tone::Bot),
        ("🤖 PR is open", "opened a PR", Tone::Bot),
        ("🤖 PR updated → ", "updated the PR", Tone::Bot),
        ("🤖 Build complete", "finished the build", Tone::Bot),
        ("🤖 Still firing", "saw it again", Tone::Problem),
        ("🤖 Saw this again", "saw it again", Tone::Problem),
        ("🤖 Still recurring", "saw it again", Tone::Problem),
        ("🤖 Possible duplicate", "flagged a duplicate", Tone::Bot),
        (
            "🤖 I need a bit more info",
            "asked a question",
            Tone::Problem,
        ),
        ("**Type:**", "assessed it", Tone::Bot),
        ("picked this up", "picked it up", Tone::Bot),
        ("I'm on it", "picked it up", Tone::Bot),
    ];
    for (prefix, verb, tone) in VERBS {
        if let Some(rest) = body.strip_prefix(prefix) {
            return (verb, *tone, rest);
        }
    }
    ("noted", Tone::Bot, body.trim_start_matches("🤖").trim())
}

/// Markdown down to readable text: emphasis, code ticks and link targets go.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        match (tail.find("]("), tail.find(')')) {
            (Some(mid), Some(end)) if mid < end => {
                out.push_str(&tail[1..mid]);
                rest = &tail[end + 1..];
            }
            _ => {
                out.push('[');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "").replace("__", "")
}

fn first_sentence(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with(">>>") && *l != "---")
        .unwrap_or_default();
    let line = plain(line);
    let end = line
        .char_indices()
        .find(|&(i, c)| c == '.' && line[i + 1..].starts_with(' '))
        .map_or(line.len(), |(i, _)| i + 1);
    let sentence = &line[..end];
    if sentence.chars().count() > 160 {
        let cut: String = sentence.chars().take(157).collect();
        format!("{cut}…")
    } else {
        sentence.to_owned()
    }
}

/// "5m", "3h", "2d" since an ISO 8601 timestamp.
/// When something happened on the user's clock: "today 09:13",
/// "yesterday 22:40", else "Mon 29 Sep 14:05". Empty for a bad timestamp.
pub fn when(timestamp: &str) -> String {
    when_in(
        timestamp,
        jiff::Timestamp::now(),
        &jiff::tz::TimeZone::system(),
    )
}

fn when_in(timestamp: &str, now: jiff::Timestamp, tz: &jiff::tz::TimeZone) -> String {
    let Ok(then) = timestamp.parse::<jiff::Timestamp>() else {
        return String::new();
    };
    let (then, now) = (then.to_zoned(tz.clone()), now.to_zoned(tz.clone()));
    let time = then.strftime("%H:%M");
    let days = (now.date() - then.date()).get_days();
    match days {
        0 => format!("today {time}"),
        1 => format!("yesterday {time}"),
        _ => then.strftime("%a %-d %b %H:%M").to_string(),
    }
}

/// How long from `from` to `to`, short: "11m", "3h", "2d".
pub fn took(from: &str, to: &str) -> String {
    let (Ok(a), Ok(b)) = (
        from.parse::<jiff::Timestamp>(),
        to.parse::<jiff::Timestamp>(),
    ) else {
        return String::new();
    };
    let secs = (b.as_second() - a.as_second()).max(0);
    match secs {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// The last step on a ticket and how long ago, `opened a PR · 12m`: what
/// the bot did last, for a sub-ticket no session here reports on.
pub fn last_step(events: &[Event]) -> Option<String> {
    let step = events.iter().rev().find(|e| is_step(e.verb))?;
    Some(match ago(&step.at).as_str() {
        "" => step.verb.to_owned(),
        age => format!("{} · {age}", step.verb),
    })
}

pub fn ago(timestamp: &str) -> String {
    let Ok(then) = timestamp.parse::<jiff::Timestamp>() else {
        return String::new();
    };
    let secs = (jiff::Timestamp::now().as_second() - then.as_second()).max(0);
    match secs {
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_linear_workspace_is_never_a_customer() {
        let plain = crate::profile::Profile::default();
        assert!(is_ours_in(&plain, "Acme", "acme "));
        assert!(!is_ours_in(&plain, "Acme", "Globex"));
        assert!(!is_ours_in(&plain, "", "Acme"));
        let acme = crate::profile::acme();
        assert!(is_ours_in(&acme, "", &acme.company.name));
    }

    #[test]
    fn a_sub_ticket_says_what_was_done_last_and_when() {
        let event = |verb: &'static str, at: &str| Event {
            at: at.into(),
            who: "bot".into(),
            verb,
            tone: Tone::Bot,
            summary: String::new(),
            body: String::new(),
        };
        let hour_ago = (jiff::Timestamp::now() - jiff::SignedDuration::from_hours(1)).to_string();
        let said = last_step(&[
            event("opened a PR", &hour_ago),
            event("commented", &hour_ago),
        ]);
        assert_eq!(
            said.as_deref(),
            Some("opened a PR · 1h"),
            "a comment is no step"
        );
        assert_eq!(last_step(&[event("commented", &hour_ago)]), None);
    }

    #[test]
    fn intake_is_only_triage_and_never_the_backlog() {
        // Intake and its badge count `untriaged`: Linear's Triage state only.
        let triage = card(issue("Triage", "triage", vec![]), &HashSet::new());
        let backlog = card(issue("Backlog", "backlog", vec![]), &HashSet::new());
        assert!(triage.untriaged());
        assert!(!backlog.untriaged());
    }

    #[test]
    fn dependabot_prs_join_the_dependency_lane_and_leave_when_gone() {
        let pr = |n: u64| crate::github::MyPr {
            url: format!("https://github.com/acme/app/pull/{n}"),
            repo: "app".into(),
            number: n,
            title: format!("bump {n}"),
            at: "2026-09-26T07:31:37Z".into(),
            draft: false,
        };
        let inbox = pr_card(&pr(1).url, "app#1", "").unwrap();
        let mut cards = vec![inbox];
        take_dependencies(&mut cards, &[pr(1), pr(2), pr(3)], None);
        assert_eq!(cards.len(), 3);
        assert!(
            cards
                .iter()
                .all(|c| c.lane == Lane::Dependencies && c.pr_only())
        );
        // 2 is open here, so it stays; 3 is gone.
        take_dependencies(&mut cards, &[pr(1)], Some("pr:acme.app.2"));
        let ids: Vec<&str> = cards.iter().map(|c| c.issue.id.as_str()).collect();
        assert_eq!(ids, ["pr:acme.app.1", "pr:acme.app.2"]);
    }

    #[test]
    fn a_pr_card_takes_githubs_title_and_body_and_ends_when_the_pr_does() {
        let url = "https://github.com/acme/app/pull/12";
        let at = "2026-09-26T07:31:37Z";
        let open = pr_card(url, "app#12", at).unwrap();
        assert_ne!(open.stage, Stage::Done);
        let gate = |state: &str| crate::github::Gate {
            state: state.into(),
            title: "fix: retry the flaky upload".into(),
            body: "Retries once on a 503.".into(),
            ..Default::default()
        };
        let known = pr_card_from(url, at, &gate("OPEN")).unwrap();
        assert_eq!(known.issue.title, "fix: retry the flaky upload");
        assert_eq!(
            known.issue.description.as_deref(),
            Some("Retries once on a 503.")
        );
        assert_eq!(
            known.issue.id, open.issue.id,
            "the same card, so its session stays"
        );
        let merged = pr_card_from(url, at, &gate("MERGED")).unwrap();
        assert_eq!((merged.stage, merged.ask), (Stage::Done, None));
        assert!(merged.prs[0].merged && !merged.prs[0].open);
        let closed = pr_card_from(url, at, &gate("CLOSED")).unwrap();
        assert_eq!((closed.stage, closed.ask), (Stage::Done, None));
        // No title yet: the PR's name.
        let bare = pr_card_from(url, at, &crate::github::Gate::default()).unwrap();
        assert_eq!(bare.issue.title, "app#12");
    }

    #[test]
    fn a_colleagues_ask_from_slack_is_a_call_until_it_is_triaged() {
        let mut ask = issue("Triage", "triage", vec![]);
        ask.creator = Some(crate::linear::User {
            name: "Jo Doe".into(),
        });
        ask.priority = 2.0;
        ask.created_at = "2026-09-29T07:58:55Z".into();
        ask.attachments.nodes.push(crate::linear::Attachment {
            url: "https://acme.slack.com/archives/C0123456789/p1790668737089739".into(),
            title: "Ask from jo.doe".into(),
            source_type: Some("slack".into()),
            metadata: serde_json::Value::Null,
        });
        let line = open_ask(Lane::Product, Stage::Intake, &ask).unwrap();
        assert!(line.starts_with("Jo's ask · High · "), "{line}");
        assert!(line.ends_with(": triage it"));
        assert_eq!(
            open_ask(Lane::Product, Stage::Planning, &ask),
            None,
            "triaged"
        );
        assert_eq!(
            open_ask(Lane::Alerts, Stage::Intake, &ask),
            None,
            "KTLO stays out"
        );
        let mut parked = ask.clone();
        parked.state.kind = "backlog".into();
        assert_eq!(
            open_ask(Lane::Product, Stage::Intake, &parked),
            None,
            "in the Backlog someone looked at it"
        );
        let plain = issue("Triage", "triage", vec![]);
        assert_eq!(
            open_ask(Lane::Product, Stage::Intake, &plain),
            None,
            "not an ask"
        );
    }

    #[test]
    fn ktlo_nobody_picked_up_is_flagged_after_a_day() {
        let mut alert = issue("Triage", "triage", vec![]);
        alert.title = "[Prod] worker pool at 99% memory limit".into();
        alert.created_at = "2026-09-24T03:08:46Z".into();
        alert.priority = 2.0;
        // The lane is the profile's call (lane_in); here it is set.
        let why = nobody_on_it(Lane::Alerts, Stage::Intake, &alert).unwrap();
        assert!(why.starts_with("Nobody on it · filed "), "{why}");
        assert_eq!(nobody_on_it(Lane::Product, Stage::Intake, &alert), None);
        let ktlo = |issue: Issue| {
            let mut card = super::card(issue, &HashSet::new());
            card.lane = Lane::Alerts;
            card
        };
        let card = ktlo(alert.clone());
        let later: jiff::Timestamp = "2026-09-29T08:00:00Z".parse().unwrap();
        assert!(unhandled(&card, later) && pressing(&card));
        let soon: jiff::Timestamp = "2026-09-24T10:00:00Z".parse().unwrap();
        assert!(!unhandled(&card, soon), "not a day yet");
        let mut owned = alert;
        owned.assignee = Some(crate::linear::User {
            name: "Robo".into(),
        });
        assert!(!unhandled(&ktlo(owned), later));
    }

    #[test]
    fn a_moved_ticket_keeps_its_kind_of_state() {
        let state = |id: &str, kind: &str, position: f64| crate::linear::State {
            id: id.into(),
            name: id.into(),
            color: String::new(),
            position,
            kind: kind.into(),
        };
        let bui = [
            state("backlog", "backlog", 0.0),
            state("todo", "unstarted", 1.0),
            state("doing", "started", 2.0),
            state("review", "started", 3.0),
        ];
        let id = |kind| state_like(&bui, kind).map(|s| s.id.as_str());
        assert_eq!(id("unstarted"), Some("todo"));
        assert_eq!(id("started"), Some("doing"));
        // WEB has no Triage: the backlog.
        assert_eq!(id("triage"), Some("backlog"));
        assert_eq!(
            state_like(&bui[1..2], "triage").map(|s| s.id.as_str()),
            Some("todo")
        );
    }

    #[test]
    fn a_canceled_ticket_goes_to_the_first_canceled_state() {
        let state = |id: &str, kind: &str, position: f64| crate::linear::State {
            id: id.into(),
            name: id.into(),
            color: String::new(),
            position,
            kind: kind.into(),
        };
        let states = [
            state("done", "completed", 1.0),
            state("wontdo", "canceled", 3.0),
            state("canceled", "canceled", 2.0),
            state("dup", "duplicate", 0.0),
        ];
        assert_eq!(
            canceled_state(&states).map(|s| s.id.as_str()),
            Some("canceled")
        );
        assert!(canceled_state(&states[..1]).is_none());
    }

    #[test]
    fn a_linear_link_names_its_ticket() {
        let id = issue_from_url("https://linear.app/acme/issue/eng-12/fix-the-thing");
        assert_eq!(id.as_deref(), Some("ENG-12"));
        assert_eq!(issue_from_url("https://linear.app/acme/project/x"), None);
        assert_eq!(issue_from_url("https://github.com/o/r/pull/1"), None);
    }

    #[test]
    fn who_opened_it_is_you_a_bot_or_someone() {
        assert_eq!(who("Kim", true, false), Who::You);
        assert_eq!(who("acme-bot[bot]", false, false), Who::Bot);
        assert_eq!(who("acme-bot", false, true), Who::Bot);
        assert_eq!(who("Alex", false, false), Who::Other);
    }

    #[test]
    fn a_pr_without_a_ticket_is_a_card_to_review() {
        let url = "https://github.com/acme/app/pull/12";
        let card = pr_card(url, "fix: it", "2026-09-26T07:31:37Z").unwrap();
        assert!(card.pr_only());
        assert!(!card.issue.id.contains('/'));
        assert_eq!(card.issue.identifier, "app#12");
        assert_eq!((card.stage, card.ask), (Stage::Review, Some(Ask::Review)));
        assert_eq!(card.pr(None).map(|p| p.url.as_str()), Some(url));
        assert!(pr_card("https://github.com/acme/app/issues/3", "", "").is_none());
    }
    use crate::linear::{Attachment, Nodes, State, TeamRef, User};
    use serde_json::json;

    pub(crate) fn comment(who: &str, body: &str) -> Comment {
        Comment {
            id: body.into(),
            body: body.into(),
            created_at: "2026-09-21T10:00:00.000Z".into(),
            user: Some(User { name: who.into() }),
        }
    }

    pub(crate) fn issue(state: &str, kind: &str, comments: Vec<Comment>) -> Issue {
        // The tests' bot, as a refresh would find it.
        set_bot("Robo");
        Issue {
            id: "i".into(),
            identifier: "DEV-1".into(),
            title: "Fix the thing".into(),
            url: String::new(),
            description: None,
            updated_at: String::new(),
            created_at: String::new(),
            priority: 0.0,
            previous_identifiers: vec!["PRO-9".into(), "WEB-3".into()],
            state: State {
                id: state.into(),
                name: state.into(),
                color: String::new(),
                position: 0.0,
                kind: kind.into(),
            },
            team: TeamRef { id: "t".into() },
            assignee: None,
            creator: Some(User {
                name: "Dana".into(),
            }),
            parent: None,
            project: None,
            attachments: Nodes { nodes: vec![] },
            needs: Nodes { nodes: vec![] },
            labels: Nodes { nodes: vec![] },
            children: Nodes { nodes: vec![] },
            comments: Nodes { nodes: comments },
        }
    }

    #[test]
    fn a_bot_command_is_found_in_any_text() {
        assert!(has_bot_command("@Robo Plan!"));
        assert!(has_bot_command("done, now @robo   build! please"));
        assert!(has_bot_command("x\n@Robo Investigate!"));
        assert!(!has_bot_command("ask @Robo to plan it"));
        assert!(!has_bot_command("@Robo Plan"));
        assert!(!has_bot_command("@ Plan!"));
        assert!(!has_bot_command("Plan! @Robo"));
    }

    #[test]
    fn the_only_trigger_comments_the_app_posts_are_the_profiles() {
        // The bot's poller runs `@<anyone> <Command>!`; the app writes
        // exactly the profile's, and only on the user's pick in the step menu.
        let mut profile = crate::profile::Profile::default();
        assert_eq!(
            BotCommand::Plan.comment_in(&profile),
            None,
            "no profile, no remote step"
        );
        profile.bot.plan_command = "@Robo Plan!".into();
        profile.bot.build_command = " @Robo Build! ".into();
        assert_eq!(
            BotCommand::Plan.comment_in(&profile).as_deref(),
            Some("@Robo Plan!")
        );
        assert_eq!(
            BotCommand::Build.comment_in(&profile).as_deref(),
            Some("@Robo Build!")
        );
        assert_eq!(BotCommand::Investigate.comment_in(&profile), None);
        profile.bot.investigate_command = "@Robo Investigate!".into();
        assert_eq!(
            BotCommand::Investigate.comment_in(&profile).as_deref(),
            Some("@Robo Investigate!")
        );
    }

    /// The lane and story rules as they were hardcoded before profiles, and
    /// a profile that names them, on the same tickets: the same answers.
    #[test]
    fn a_profile_keeps_the_old_ktlo_rules() {
        fn old_flow(title: &str) -> Option<&'static str> {
            let title = title.to_lowercase();
            [
                ("[job-error]", "Job errors"),
                ("[job error]", "Job errors"),
                ("[prod]", "Prod alerts"),
                ("[ux stuck]", "UX friction"),
                ("[reports feature]", "Reports feature"),
            ]
            .into_iter()
            .find(|(prefix, _)| title.starts_with(prefix))
            .map(|(_, flow)| flow)
        }
        fn old_lane(issue: &Issue) -> Lane {
            let title = issue.title.to_lowercase();
            let labelled = |name: &str| {
                issue
                    .labels
                    .nodes
                    .iter()
                    .any(|l| l.name.eq_ignore_ascii_case(name))
            };
            let by_scanner = issue.creator.as_ref().is_some_and(|c| c.name == "Scanner");
            if by_scanner
                || labelled("security")
                || title.contains(" cve:")
                || title.contains("cve-")
            {
                return Lane::Security;
            }
            let swept = matches!(old_flow(&issue.title), Some("Job errors" | "Prod alerts"));
            if swept || labelled("ktlo") || labelled("prod alerts") {
                return Lane::Alerts;
            }
            Lane::Product
        }
        let acme = crate::profile::acme();
        let titles = [
            "Critical CVE: libfoo header parsing",
            "Bump libfoo for cve-2026-1234",
            "[Job error] Report must contain a date",
            "[job-error] Export step errors",
            "[Prod] Worker pods not ready",
            "[UX stuck] Users abandon the report editor",
            "[Reports feature] Schedule exports",
            "Add filter control to dashboard",
        ];
        let creators = [None, Some("Scanner"), Some("Sam")];
        let labels = [
            None,
            Some("Security"),
            Some("KTLO"),
            Some("Prod alerts"),
            Some("Bug"),
        ];
        for title in titles {
            for creator in creators {
                for label in labels {
                    let mut i = issue("s", "backlog", vec![]);
                    i.title = title.into();
                    i.creator = creator.map(|name| User { name: name.into() });
                    if let Some(label) = label {
                        i.labels
                            .nodes
                            .push(crate::linear::Named { name: label.into() });
                    }
                    assert_eq!(
                        lane_in(&acme, &i),
                        old_lane(&i),
                        "{title} {creator:?} {label:?}"
                    );
                }
            }
            assert_eq!(
                sweep_in(&acme, title).map(|s| s.story),
                old_flow(title).map(str::to_owned),
                "{title}"
            );
        }
        assert_eq!(
            BotCommand::Plan.comment_in(&acme).as_deref(),
            Some("@Robo Plan!")
        );
        assert_eq!(
            BotCommand::Build.comment_in(&acme).as_deref(),
            Some("@Robo Build!")
        );
        // UX friction tickets offer the UX friction investigation.
        assert_eq!(
            BotCommand::Investigate.comment_in(&acme).as_deref(),
            Some("@Robo Investigate!")
        );
        let investigates: Vec<&str> = acme
            .ktlo
            .sweeps
            .iter()
            .filter(|s| s.investigate)
            .map(|s| s.prefix.as_str())
            .collect();
        assert_eq!(investigates, ["[ux stuck]"]);
    }

    #[test]
    fn a_ticket_the_bot_never_touched_reads_as_its_linear_state() {
        let mut plain = issue("s", "started", vec![]);
        plain.state.name = "In Progress".into();
        let card = card(plain, &HashSet::new());
        assert!(!card.why.contains("plan"), "{}", card.why);
        assert!(card.why.starts_with("In Progress · "), "{}", card.why);
        // A draft PR is not waiting for review.
        let draft = Pr {
            draft: true,
            ..pr_from_url("https://github.com/acme/app/pull/1", "").unwrap()
        };
        assert!(!draft.waiting());
    }

    #[test]
    fn review_qa_and_blocked_states_are_found_by_their_words() {
        let at = |state: &str, kind: &str| {
            let mut i = issue("s", kind, vec![]);
            i.state.name = state.into();
            stage(&i, &[])
        };
        assert_eq!(at("In Review", "started"), Stage::Review);
        assert_eq!(at("Code Review", "started"), Stage::Review);
        assert_eq!(at("QA", "started"), Stage::Testing);
        assert_eq!(at("Testing", "started"), Stage::Testing);
        assert_eq!(at("Blocked", "started"), Stage::Blocked);
        assert_eq!(at("On Hold", "backlog"), Stage::Blocked);
        // Started by a person (no bot on it): being built.
        assert_eq!(at("In Progress", "started"), Stage::Building);
        assert_eq!(at("Done", "completed"), Stage::Done);
        assert_eq!(at("Won't do", "canceled"), Stage::Done);
    }

    #[test]
    fn socket_cves_and_sweep_alerts_are_ktlo_the_rest_is_product() {
        // A scanner that files CVE tickets, and a sweep's alerts: the profile's.
        let mut profile = crate::profile::Profile::default();
        profile.ktlo.security_creators = vec!["Socket".into()];
        profile.ktlo.sweeps = vec![crate::profile::Sweep {
            prefix: "[prod]".into(),
            story: "Prod alerts".into(),
            alert: true,
            ..Default::default()
        }];
        let lane = |issue: &Issue| lane_in(&profile, issue);
        let mut cve = issue("s", "backlog", vec![]);
        cve.title = "Critical CVE: libfoo: header parsing overflow".into();
        cve.creator = Some(User {
            name: "Socket".into(),
        });
        assert_eq!(lane(&cve), Lane::Security);
        let mut hardening = issue("s", "backlog", vec![]);
        hardening.title = "Harden acme-auth pods: run container as non-root".into();
        hardening.labels.nodes.push(crate::linear::Named {
            name: "Security".into(),
        });
        assert_eq!(lane(&hardening), Lane::Security);
        let mut alert = issue("s", "backlog", vec![]);
        alert.title = "[Prod] Worker pods not ready on node".into();
        assert_eq!(lane(&alert), Lane::Alerts);
        assert_eq!(lane(&issue("s", "backlog", vec![])), Lane::Product);
        // Without a profile the sweep is product work; the Security label
        // and CVE titles are KTLO everywhere.
        assert_eq!(lane_in(&Default::default(), &alert), Lane::Product);
        assert_eq!(lane_in(&Default::default(), &hardening), Lane::Security);
    }

    #[test]
    fn a_card_marked_read_comes_back_when_it_changes_not_with_the_clock() {
        let base = issue("In Review", "started", vec![]);
        let key = |i: &Issue| read_key(&super::card(i.clone(), &HashSet::new()));
        let seen = key(&base);
        let mut later = base.clone();
        later.updated_at = "2030-01-01T00:00:00Z".into();
        assert_eq!(key(&later), seen);
        let mut commented = base.clone();
        commented
            .comments
            .nodes
            .push(comment("Gina", "still broken"));
        assert_ne!(key(&commented), seen);
        assert_ne!(key(&with_pr(base, "merged")), seen);
    }

    fn with_pr(mut issue: Issue, status: &str) -> Issue {
        issue.attachments.nodes.push(Attachment {
            url: "https://github.com/acme/web/pull/7".into(),
            title: "DEV-1: fix".into(),
            source_type: Some("github".into()),
            metadata: json!({ "status": status, "repoName": "web", "number": 7,
                "userLogin": "acme-bot", "reviews": [] }),
        });
        issue
    }

    #[test]
    fn a_persons_pr_waiting_for_review_is_not_the_users_to_review() {
        // Nobody asked the user: the person's team reviews it.
        let card = super::card(
            with_pr(issue("In Review", "started", vec![]), "inReview"),
            &HashSet::new(),
        );
        assert_eq!((card.stage, card.ask), (Stage::Review, None));
        assert_eq!(card.why, "web#7 waiting for review · unassigned");
        // The bot's PR waits on the user.
        let bot = vec![comment("Robo", "🤖 PR opened → web#7")];
        let card = super::card(
            with_pr(issue("In Review", "started", bot), "inReview"),
            &HashSet::new(),
        );
        assert_eq!(card.ask, Some(Ask::Review));
    }

    #[test]
    fn a_ticket_a_person_took_is_theirs_even_after_the_bot_routed_it() {
        let routed = vec![comment("Robo", "🤖 Routed to **Todo** · P1")];
        let mut taken = issue("In Progress", "started", routed.clone());
        taken.assignee = Some(crate::linear::User { name: "Ada".into() });
        let card = super::card(taken.clone(), &HashSet::new());
        assert!(!card.bot_works);
        assert_eq!(card.stage, Stage::Building);
        assert_eq!(card.why, "In Progress · Ada");

        // Their approved PR: in review and ready, not waiting on the bot's plan.
        let mut with = with_pr(taken.clone(), "open");
        with.attachments.nodes[0].metadata["reviews"] =
            json!([{ "state": "approved", "isBot": false }]);
        let card = super::card(with, &HashSet::new());
        assert_eq!(card.stage, Stage::Review);
        assert_eq!(card.why, "PR approved: ready to merge.");

        // Unassigned, or the bot dispatched it: still the bot's.
        let card = super::card(issue("In Progress", "started", routed), &HashSet::new());
        assert!(card.bot_works);
        taken
            .comments
            .nodes
            .push(comment("Robo", "🤖 Dispatched Build for this ticket"));
        assert!(super::card(taken, &HashSet::new()).bot_works);
    }

    #[test]
    fn the_bots_trail_becomes_stage_ask_and_story() {
        let trail = vec![
            comment(
                "Robo",
                "**Type:** UX issue\n**Area:** `Builder`\n**Customers:** 1",
            ),
            comment(
                "Robo",
                "🤖 Here's my implementation plan\n\n>>> My analysis\n\n**Bug** · repos: `web`",
            ),
            comment("Robo", "🤖 Assigned to myself — single-repo."),
        ];
        let card = card(
            issue("In Progress", "started", trail.clone()),
            &HashSet::new(),
        );
        assert_eq!(card.stage, Stage::Planning);
        assert_eq!(card.ask, None);
        assert_eq!(card.story, "Builder");
        assert_eq!(card.origin.came_in_as.as_deref(), Some("PRO-9"));
        assert_eq!(card.plan.unwrap().tldr, "Bug · repos: web");
        assert_eq!(card.timeline.len(), 3);

        let mut building = trail.clone();
        building.push(comment("Robo", "🤖 Dispatched Build — writing."));
        let card = super::card(issue("In Progress", "started", building), &HashSet::new());
        assert_eq!(card.stage, Stage::Building);

        let mut review = trail;
        review.push(comment(
            "Robo",
            "🤖 PR opened → url\n\n**Status: CI failing after one self-fix pass.**",
        ));
        let card = super::card(
            with_pr(issue("In Review", "started", review), "inReview"),
            &HashSet::new(),
        );
        assert_eq!(card.ask, Some(Ask::Review));
        assert_eq!(card.ci, Ci::Red);
        assert!(card.why.starts_with("web#7 · CI red"));

        // Merged but not yet moved: already the tester's, not the reviewer's.
        let card = super::card(
            with_pr(issue("In Review", "started", vec![]), "merged"),
            &HashSet::new(),
        );
        assert_eq!(card.ask, Some(Ask::Verify));

        // One a person took is theirs to verify.
        let mut taken = with_pr(issue("In Review", "started", vec![]), "merged");
        taken.assignee = Some(User { name: "Ben".into() });
        assert_eq!(super::card(taken, &HashSet::new()).ask, None);
    }

    #[test]
    fn the_ship_brief_lists_each_ticket_with_its_prs_and_the_routing() {
        let card = super::card(
            with_pr(issue("In Review", "started", vec![]), "inReview"),
            &HashSet::new(),
        );
        let brief = ship_brief("ship", "open KTLO tickets", &[&card], "ABC: the web app");
        assert!(brief.starts_with("/ship There are 1 open KTLO tickets:"));
        assert!(brief.contains(&format!("PR: {}", card.prs[0].url)));
        assert!(brief.contains(&card.issue.url));
        assert!(brief.ends_with("Who owns what:\nABC: the web app\n"));
        assert!(!ship_brief("ship", "x", &[&card], "").contains("Who owns what"));
    }

    #[test]
    fn a_linear_link_names_its_ticket_also_from_a_review_page() {
        let org = "https://linear.app/acme";
        assert_eq!(
            issue_from_url(&format!("{org}/issue/web-1099/trace")).as_deref(),
            Some("WEB-1099")
        );
        assert_eq!(
            issue_from_url(&format!(
                "{org}/review/web-1099-align-waterfall-bar-widths-62dc78d92a6f#comment-5d7514d2"
            ))
            .as_deref(),
            Some("WEB-1099")
        );
        assert_eq!(
            issue_from_url(&format!("{org}/review/fix-the-build-62dc78d92a6f")),
            None
        );
        assert_eq!(issue_from_url(&format!("{org}/project/x")), None);
    }

    #[test]
    fn a_pr_key_names_its_pr() {
        assert_eq!(
            pr_url_of("pr:acme.api.1197").as_deref(),
            Some("https://github.com/acme/api/pull/1197")
        );
        assert_eq!(
            pr_url_of("pr:o.my.repo.7").as_deref(),
            Some("https://github.com/o/my.repo/pull/7")
        );
        assert_eq!(pr_url_of("DEV-1"), None);
        assert_eq!(pr_url_of("pr:o.repo.x"), None);
    }

    #[test]
    fn a_pr_github_calls_approved_or_queued_asks_no_review() {
        for gate in [
            crate::github::Gate {
                review: "APPROVED".into(),
                ..Default::default()
            },
            crate::github::Gate {
                queued: true,
                ..Default::default()
            },
        ] {
            let bot = vec![comment("Robo", "🤖 PR opened → web#7")];
            let mut card = super::card(
                with_pr(issue("In Review", "started", bot), "inReview"),
                &HashSet::new(),
            );
            assert_eq!(card.ask, Some(Ask::Review));
            let gates = std::collections::HashMap::from([(card.prs[0].url.clone(), Ok(gate))]);
            live(&mut card, &gates);
            assert_eq!(card.ask, None);
        }
        // Review still required: still the user's to review.
        let bot = vec![comment("Robo", "🤖 PR opened → web#7")];
        let mut card = super::card(
            with_pr(issue("In Review", "started", bot), "inReview"),
            &HashSet::new(),
        );
        let gate = crate::github::Gate {
            review: "REVIEW_REQUIRED".into(),
            ..Default::default()
        };
        let gates = std::collections::HashMap::from([(card.prs[0].url.clone(), Ok(gate))]);
        live(&mut card, &gates);
        assert_eq!(card.ask, Some(Ask::Review));
        // The user's own PR (no ask): the status line follows the approval.
        let mut card = super::card(
            with_pr(issue("In Review", "started", vec![]), "inReview"),
            &HashSet::new(),
        );
        assert!(card.why.contains("waiting for review"), "{}", card.why);
        let gate = crate::github::Gate {
            review: "APPROVED".into(),
            ..Default::default()
        };
        let gates = std::collections::HashMap::from([(card.prs[0].url.clone(), Ok(gate))]);
        live(&mut card, &gates);
        assert!(!card.why.contains("waiting for review"), "{}", card.why);
    }

    #[test]
    fn a_prs_status_takes_githubs_live_word_over_linears_lagging_one() {
        let open = pr_from_url("https://github.com/acme/web/pull/7", "").unwrap();
        let gate = |state: &str, draft: bool, queued: bool| crate::github::Gate {
            head: "abc".into(),
            green: false,
            why: String::new(),
            state: state.into(),
            draft,
            queued,
            open_threads: 0,
            merge_queue: false,
            can_approve: false,
            reviewed_at: None,
            own: false,
            ..Default::default()
        };
        let state = |pr: &Pr, gate: Option<crate::github::Gate>, merging: bool| {
            pr.state(gate.as_ref(), merging, false)
        };
        assert_eq!(state(&open, None, false), PrState::Open);
        let approved = Pr {
            approved: true,
            ..open.clone()
        };
        assert_eq!(state(&approved, None, false), PrState::Approved);
        assert_eq!(
            state(&open, Some(gate("OPEN", true, false)), false),
            PrState::Draft
        );
        // The bot queued it, or the user's merge is running.
        assert_eq!(
            state(&approved, Some(gate("OPEN", false, true)), false),
            PrState::Merging
        );
        assert_eq!(state(&open, None, true), PrState::Merging);
        // GitHub says merged or closed before Linear does.
        assert_eq!(
            state(&open, Some(gate("MERGED", false, false)), false),
            PrState::Merged
        );
        assert_eq!(
            state(&open, Some(gate("CLOSED", false, false)), false),
            PrState::Closed
        );
        // Once Linear says closed, a gate from while it was open is stale.
        let closed = Pr {
            open: false,
            ..open.clone()
        };
        assert_eq!(
            state(&closed, Some(gate("OPEN", true, false)), false),
            PrState::Closed
        );
        assert_eq!(open.state(None, false, true), PrState::Merged);
    }

    #[test]
    fn every_linked_pr_counts_and_the_one_waiting_leads() {
        let attach = |issue: &mut Issue, number: u64, status: &str| {
            issue.attachments.nodes.push(Attachment {
                url: format!("https://github.com/acme/web/pull/{number}"),
                title: "DEV-1: fix".into(),
                source_type: Some("github".into()),
                metadata: json!({ "status": status, "repoName": "web", "number": number,
                    "reviews": [] }),
            });
        };
        // The older PR still waits; the newer one merged.
        let bot = vec![comment("Robo", "🤖 PR opened → web#7")];
        let mut two = issue("In Review", "started", bot);
        attach(&mut two, 7, "inReview");
        attach(&mut two, 9, "merged");
        let card = super::card(two.clone(), &HashSet::new());
        let names: Vec<String> = card.prs.iter().map(Pr::name).collect();
        // The one waiting for review comes first, newer or not.
        assert_eq!(names, ["web#7", "web#9"]);
        assert_eq!((card.stage, card.ask), (Stage::Review, Some(Ask::Review)));
        assert_eq!(card.why, "web#7");
        assert_eq!(card.pr(None).map(|p| p.number), Some(7));
        let nine = "https://github.com/acme/web/pull/9";
        assert_eq!(card.pr(Some(nine)).map(|p| p.number), Some(9));

        // Both waiting: counted, since each has its own row.
        two.attachments.nodes[1].metadata["status"] = json!("inReview");
        let card = super::card(two.clone(), &HashSet::new());
        assert_eq!(card.why, "2 PRs to review");

        // Both merged: the tester's, counted the same way.
        for pr in &mut two.attachments.nodes {
            pr.metadata["status"] = json!("merged");
        }
        let card = super::card(two, &HashSet::new());
        assert_eq!((card.stage, card.ask), (Stage::Testing, Some(Ask::Verify)));
        assert_eq!(card.why, "2 PRs merged. Check it works, then close it.");
    }

    #[test]
    fn a_ticket_a_person_blocked_is_not_the_users_call() {
        let trail = vec![
            comment("Robo", "🤖 Routed to **Blocked** — plan spans `a` and `b`."),
            comment(
                "Alex Kim",
                "Blocked until we do the refactoring of the export pipeline",
            ),
            comment(
                "Robo",
                "Ticket Health — this ticket is in progress but has no assignee.",
            ),
        ];
        let card = card(issue("Blocked", "started", trail), &HashSet::new());
        assert_eq!((card.stage, card.ask), (Stage::Blocked, None));
        assert_eq!(
            card.why,
            "Waiting on Alex Kim: Blocked until we do the refactoring of the export pipeline"
        );
        let mut held = issue("Blocked", "started", vec![]);
        held.assignee = Some(User { name: "Ana".into() });
        assert_eq!(
            super::card(held, &HashSet::new()).why,
            "Waiting on Ana: no reason recorded on the ticket."
        );
        let silent = super::card(issue("Blocked", "started", vec![]), &HashSet::new());
        assert_eq!(
            silent.ask, None,
            "nobody said why: still not the user's call"
        );
    }

    #[test]
    fn a_triage_ticket_filed_today_is_new() {
        let issue_of = issue;
        let mut issue = issue_of("Triage", "triage", vec![]);
        issue.created_at = "2026-10-01T09:00:00Z".into();
        let card = card(issue, &HashSet::new());
        let at = |t: &str| t.parse::<jiff::Timestamp>().unwrap();
        assert!(card.new_today(at("2026-10-01T20:00:00Z")));
        assert!(!card.new_today(at("2026-10-02T10:00:00Z")));
        let mut parked = issue_of("Backlog", "backlog", vec![]);
        parked.created_at = "2026-10-01T09:00:00Z".into();
        assert!(!super::card(parked, &HashSet::new()).new_today(at("2026-10-01T20:00:00Z")));
    }

    #[test]
    fn a_bot_waiting_on_the_reporter_says_so() {
        let ask = comment(
            "Robo",
            "🤖 I need a bit more info before I can route this\n\nThe Jam recording was provided but couldn't be loaded. More text.",
        );
        let card = card(
            issue("Triage", "triage", vec![ask.clone()]),
            &HashSet::new(),
        );
        assert_eq!(
            card.why,
            "Robo waits on the reporter: The Jam recording was provided but couldn't be loaded."
        );
        let answered = vec![ask, comment("Jo", "steps: open settings")];
        let card = super::card(issue("Triage", "triage", answered), &HashSet::new());
        assert_eq!(card.why, "In Triage, not pulled yet.");
    }

    #[test]
    fn when_says_today_yesterday_or_the_date() {
        let tz = jiff::tz::TimeZone::UTC;
        let now: jiff::Timestamp = "2026-10-02T15:00:00Z".parse().unwrap();
        assert_eq!(when_in("2026-10-02T09:13:00Z", now, &tz), "today 09:13");
        assert_eq!(when_in("2026-10-01T22:40:00Z", now, &tz), "yesterday 22:40");
        assert_eq!(
            when_in("2026-09-29T14:05:00Z", now, &tz),
            "Tue 29 Sep 14:05"
        );
        assert_eq!(when_in("nope", now, &tz), "");
        assert_eq!(took("2026-10-02T09:13:00Z", "2026-10-02T09:24:00Z"), "11m");
        assert_eq!(took("2026-10-01T09:00:00Z", "2026-10-02T12:00:00Z"), "1d");
    }

    #[test]
    fn a_handed_off_review_asks_the_user_nothing() {
        let mut card = card(
            with_pr(
                issue(
                    "In Review",
                    "started",
                    vec![comment("Robo", "🤖 PR opened → url")],
                ),
                "inReview",
            ),
            &HashSet::new(),
        );
        assert_eq!(card.ask, Some(Ask::Review));
        let url = card.prs[0].url.clone();
        hand_off(&mut card, &std::collections::HashMap::new());
        assert_eq!(card.ask, Some(Ask::Review), "nothing handed off yet");
        let handed = std::collections::HashMap::from([(url, "@ana".to_owned())]);
        hand_off(&mut card, &handed);
        assert_eq!(card.ask, None);
        assert_eq!(card.why, "Review asked from @ana.");
    }

    #[test]
    fn the_bots_ack_shows_until_it_says_more() {
        let ack = comment(
            "Robo",
            "picked this up — reading and will share my assessment shortly.",
        );
        let card = card(
            issue("Triage", "triage", vec![ack.clone()]),
            &HashSet::new(),
        );
        assert!(card.why.starts_with("Robo picked it up "), "{}", card.why);
        assert!(
            card.why.ends_with(" ago, assessment coming."),
            "{}",
            card.why
        );
        assert_eq!(card.timeline[0].verb, "picked it up");
        let assessed = vec![ack, comment("Robo", "**Type:** Bug")];
        let card = super::card(issue("Triage", "triage", assessed), &HashSet::new());
        assert_eq!(card.why, "In Triage, not pulled yet.");
    }

    #[test]
    fn blocked_reason_is_the_ask() {
        let trail = vec![comment(
            "Robo",
            "🤖 Routed to **Blocked** — plan spans `a` and `b`. Needs a human call.",
        )];
        let card = card(issue("Blocked", "started", trail), &HashSet::new());
        assert_eq!(card.ask, Some(Ask::Decide));
        assert_eq!(card.why, "plan spans a and b.");
        assert_eq!(card.timeline[0].tone, Tone::Problem);
    }

    #[test]
    fn the_timeline_says_each_step_once_without_markers() {
        let at = |verb: &'static str, summary: &str| Event {
            at: "2026-10-11T10:00:00Z".into(),
            who: "You".into(),
            verb,
            tone: Tone::Claude,
            summary: summary.into(),
            body: summary.into(),
        };
        let shown = condense(&[
            at("opened a PR", "billing#393"),
            at("answered", "**Next:** run `/build`."),
            at("opened a PR", "billing#393"),
            at("planned the fix", "🤖 Here's my plan"),
        ]);
        let lines: Vec<&str> = shown.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(
            lines,
            ["billing#393", "Next: run /build.", "Here's my plan"]
        );
        assert!(is_step("opened a PR") && !is_step("answered"));
        let run = [
            at("answered", "a"),
            at("answered", "b"),
            at("answered", "c"),
            at("answered", "d"),
            at("opened a PR", "x"),
            at("answered", "e"),
        ];
        assert_eq!(folded(&run, 2), [true, true, false, false, false, false]);
        // A local session's marker, posted as the user, is a step too.
        assert_eq!(
            classify("🤖 Dispatched Build — building locally").0,
            "started building"
        );
    }

    #[test]
    fn timeline_lines_are_specific_and_short() {
        let pr = "🤖 PR is open and CI is green: https://github.com/acme/web-ui/pull/6398\n\n**Root cause:** x";
        let (verb, _, rest) = classify(pr);
        assert_eq!(verb, "opened a PR");
        assert_eq!(summarize(verb, rest, pr), "web-ui#6398 · CI green");

        let took = "🤖 Assigned to myself — single-repo, straightforward per the plan above.";
        let (verb, _, rest) = classify(took);
        assert_eq!(
            summarize(verb, rest, took),
            "Single-repo, straightforward per the plan above."
        );

        let card = "**Type:** UX issue\n**Area:** Builder\n**Customers:** 1 mention";
        let (verb, _, rest) = classify(card);
        assert_eq!(
            summarize(verb, rest, card),
            "UX issue · Builder · 1 mention"
        );

        let note = "Fixed the red CI on https://github.com/acme/web-ui/pull/6398 now.";
        assert_eq!(
            summarize("commented", note, note),
            "Fixed the red CI on web-ui#6398 now."
        );
    }

    #[test]
    fn plain_strips_markdown() {
        assert_eq!(
            plain("**Bug** in [`web`](https://x/y) now"),
            "Bug in web now"
        );
        assert_eq!(plain("a [b] c"), "a [b] c");
    }

    #[cfg(test)]
    mod the_ask_tests {
        #[test]
        fn a_plan_says_whether_to_build_before_how() {
            let plan = "### The ask\n- **Asked by:** a customer in #product-feedback\n- **Need:** filter runs with no date\n- **Evidence:** none found\n- **Smaller answer:** the existing `is empty` filter\n- **Verdict:** Push back\n- **Reply:** \"The is-empty filter already does this.\"\n\n**Bug** · web-ui · S\n>>>\n### Root Cause\n- x\n>>>";
            let ask = crate::story::the_ask(plan).unwrap();
            assert_eq!(ask.asked_by, "a customer in #product-feedback");
            assert_eq!(ask.verdict, "Push back");
            assert!(!ask.build());
            assert_eq!(ask.reply, "The is-empty filter already does this.");
            assert_eq!(
                crate::story::the_ask("**Bug** · repo\n>>>\n### Root Cause"),
                None
            );
        }
    }
}

#[cfg(test)]
mod folder_tests {
    #[test]
    fn a_session_in_a_tickets_folder_belongs_to_that_ticket() {
        let of = crate::story::ticket_of_folder;
        assert_eq!(of("/u/x/work/DEV-207").as_deref(), Some("DEV-207"));
        assert_eq!(of("/u/x/work/DEV-207/web-ui").as_deref(), Some("DEV-207"));
        assert_eq!(of("/u/x/work/free-1790529279672"), None);
        assert_eq!(of("/u/x/work/chat-1791"), None);
        assert_eq!(of("/Users/k/Development/orbit"), None);
    }
}

#[cfg(test)]
mod paste_tests {
    #[test]
    fn a_pasted_pr_or_ticket_link_opens_and_other_text_does_not() {
        let open = crate::story::openable;
        assert_eq!(
            open("  https://github.com/acme/web-ui/pull/6546 \n").as_deref(),
            Some("https://github.com/acme/web-ui/pull/6546")
        );
        assert!(open("https://linear.app/acme/issue/ABC-769/x").is_some());
        assert_eq!(open("fix the build"), None);
        assert_eq!(open("https://example.com/a"), None);
        assert_eq!(open("see https://github.com/o/r/pull/1 and more"), None);
    }
}
