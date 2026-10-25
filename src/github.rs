//! A PR's diff, for the Diff tab, and its merge gate. GitHub is optional:
//! a token (from `GITHUB_TOKEN` or the Keychain) is used for the diff when
//! there is one, otherwise the user's `gh` login. Merging always goes
//! through `gh`, as the user.

use anyhow::{Result, anyhow, bail};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Hunk(String),
    Added(String),
    Removed(String),
    Context(String),
}

#[derive(Clone, Debug)]
pub struct File {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub lines: Vec<Line>,
    /// Each line's syntax colours (`syntax::spans`), filled when it loads.
    pub spans: Vec<Vec<crate::syntax::Span>>,
}

/// The unified diff of a pull request.
pub async fn diff(
    http: &reqwest::Client,
    token: Option<&str>,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<File>> {
    let slug = format!("{owner}/{repo}");
    let gh = ["pr", "diff", &number.to_string(), "--repo", &slug].map(str::to_owned);
    raw(
        http,
        token,
        &format!("repos/{owner}/{repo}/pulls/{number}"),
        &gh,
    )
    .await
}

/// What changed between two commits of a repo, for a review done before.
pub async fn compare(
    http: &reqwest::Client,
    token: Option<&str>,
    owner: &str,
    repo: &str,
    base: &str,
    head: &str,
) -> Result<Vec<File>> {
    let path = format!("repos/{owner}/{repo}/compare/{base}...{head}");
    let accept = "Accept: application/vnd.github.v3.diff";
    let gh = ["api", "--hostname", "github.com", "-H", accept, &path].map(str::to_owned);
    raw(http, token, &path, &gh).await
}

/// A diff from the REST API with the token, else through `gh` as the user.
async fn raw(
    http: &reqwest::Client,
    token: Option<&str>,
    path: &str,
    gh: &[String],
) -> Result<Vec<File>> {
    let text = match token {
        Some(token) => {
            let response = http
                .get(format!("https://api.github.com/{path}"))
                .header("Accept", "application/vnd.github.v3.diff")
                .header("Authorization", format!("Bearer {token}"))
                .header("User-Agent", crate::NAME)
                .send()
                .await
                .map_err(|error| anyhow!("network error: {}", error.without_url()))?;
            let status = response.status();
            if !status.is_success() {
                bail!("GitHub answered {status}");
            }
            response.text().await?
        }
        None => {
            let args = gh.to_vec();
            tokio::task::spawn_blocking(move || {
                crate::claude::gh(&args.iter().map(String::as_str).collect::<Vec<_>>())
            })
            .await??
        }
    };
    // parse colours the code: off the async workers.
    Ok(tokio::task::spawn_blocking(move || parse(&text)).await?)
}

/// Whether a PR may be merged right now, asked live, and what GitHub says
/// it is, for the status on its row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gate {
    /// The head commit the answer is about; a merge is bound to it.
    pub head: String,
    /// Open, not a draft, not in the merge queue, and GitHub's merge state
    /// is `CLEAN`: mergeable, every check green, required reviews in.
    pub green: bool,
    /// Why not, in a few words, when not green.
    pub why: String,
    /// `OPEN`, `CLOSED` or `MERGED`.
    pub state: String,
    pub draft: bool,
    /// Waiting in the base branch's merge queue.
    pub queued: bool,
    /// Review threads nobody resolved yet: the fix-comments skill has work.
    pub open_threads: usize,
    /// The base branch merges through a queue: merging adds it there.
    pub merge_queue: bool,
    /// Open, not a draft, not the user's own, a review still missing, and
    /// the user has not approved this head yet.
    pub can_approve: bool,
    /// The commit the user's latest own review was at: one with a body,
    /// a new comment, or a verdict. Thread replies (the fix skill posts
    /// them as the user) do not count.
    pub reviewed_at: Option<String>,
    /// The user opened it: GitHub never lets them approve it.
    pub own: bool,
    /// GitHub's words for the three things a merge needs: the review
    /// decision (`APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, empty
    /// when no review is required), the head's checks (`SUCCESS`, `FAILURE`,
    /// `PENDING`, ..., empty without checks), and the branch's merge state
    /// (`CLEAN`, `BEHIND`, `DIRTY`, ...).
    pub review: String,
    pub checks: String,
    pub branch: String,
    /// The PR's node id, for GraphQL mutations.
    pub id: String,
    /// Who opened it: their login, and whether GitHub says it is a bot.
    pub author: String,
    pub author_bot: bool,
    /// GitHub merges it (or queues it) by itself once checks pass.
    pub auto_merge: bool,
    /// The PR's title and description, for a PR with no ticket.
    pub title: String,
    pub body: String,
    /// Its branch, and the branch it goes into.
    pub head_branch: String,
    pub base_branch: String,
    /// Merge when ready can be turned on: only the checks are missing and
    /// the user may enable auto-merge.
    pub can_auto_merge: bool,
    /// Why the merge queue dropped it last time (GitHub's reason, e.g.
    /// `failed_checks`), while it is open, not queued again and nothing was
    /// committed since; empty otherwise, and for a manual removal.
    pub queue_failed: String,
    /// Who else is on it, strongest first per person: "@ana approved",
    /// "@bob requested", "@carl commented"; never the user, the author or
    /// a bot.
    pub on_it: Vec<String>,
    /// The PR's labels, by name.
    pub labels: Vec<String>,
    /// Its last conversation comments, oldest first (a preview's address
    /// comes in one: `profile::Preview::url_in`).
    pub comments: Vec<String>,
}

/// The merge queue's reason for its last removal of the PR (`failed_checks`,
/// `manual`, ...; the merge itself never counts), while it is open, not
/// queued again, and its head was committed before that removal: after a
/// new commit the failure may be fixed already.
fn queue_failed(pr: &serde_json::Value) -> String {
    let committed = pr
        .pointer("/commits/nodes/0/commit/committedDate")
        .and_then(|d| d.as_str())
        .unwrap_or_default();
    let open = pr["state"] == "OPEN" && pr["isInMergeQueue"] != true;
    let failed = pr["timelineItems"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find(|e| !matches!(e["reason"].as_str(), None | Some("" | "merged")));
    match failed {
        // RFC 3339 in UTC: the text orders as the time does.
        Some(e) if open && committed <= e["createdAt"].as_str().unwrap_or_default() => {
            e["reason"].as_str().unwrap_or_default().to_owned()
        }
        _ => String::new(),
    }
}

/// Who else is on a PR, from the gate's review requests, reviews and
/// comments: one line per person, what they did most (approved, changes
/// requested, reviewed, commented, requested), without `skip` (the author),
/// the user or bots.
fn on_it(pr: &serde_json::Value, skip: &str) -> Vec<String> {
    let rank = |what: &str| match what {
        "approved" => 0,
        "changes requested" => 1,
        "reviewed" => 2,
        "commented" => 3,
        _ => 4,
    };
    let mut who: Vec<(String, &'static str)> = Vec::new();
    let mut add = |name: String, what: &'static str| {
        if name.is_empty() || name == skip || name.ends_with("[bot]") {
            return;
        }
        match who.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) if rank(what) < rank(entry.1) => entry.1 = what,
            Some(_) => {}
            None => who.push((name, what)),
        }
    };
    let person = |v: &serde_json::Value| -> Option<String> {
        (v["viewerDidAuthor"] != true && v["author"]["__typename"] != "Bot")
            .then(|| v["author"]["login"].as_str().map(|l| format!("@{l}")))
            .flatten()
    };
    let nodes = |path: &str| {
        pr.pointer(path)
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default()
    };
    for review in nodes("/reviews/nodes") {
        let what = match review["state"].as_str() {
            Some("APPROVED") => "approved",
            Some("CHANGES_REQUESTED") => "changes requested",
            _ => "reviewed",
        };
        if let Some(name) = person(&review) {
            add(name, what);
        }
    }
    for comment in nodes("/comments/nodes") {
        if let Some(name) = person(&comment) {
            add(name, "commented");
        }
    }
    for request in nodes("/reviewRequests/nodes") {
        let r = &request["requestedReviewer"];
        let name = match (r["login"].as_str(), r["slug"].as_str()) {
            (Some(login), _) => format!("@{login}"),
            (_, Some(team)) => format!("team {team}"),
            _ => continue,
        };
        add(name, "requested");
    }
    who.sort_by_key(|(_, what)| rank(what));
    who.into_iter()
        .map(|(name, what)| format!("{name} {what}"))
        .collect()
}

/// How a verdict or one of its facts reads: good to go, waiting on
/// something, or blocked until someone acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Good,
    Waiting,
    Blocked,
}

/// A headline and the three facts under it, each with its tone.
pub type Verdict = ((Tone, String), [(Tone, &'static str); 3]);

/// Whether the PR can be merged, in one line, and the three facts behind
/// it: approval, checks, branch. `None` for a PR that is not open.
pub fn verdict(gate: &Gate) -> Option<Verdict> {
    if gate.state != "OPEN" {
        return None;
    }
    let approval = match gate.review.as_str() {
        "APPROVED" => (Tone::Good, "approved"),
        "CHANGES_REQUESTED" => (Tone::Blocked, "changes requested"),
        "REVIEW_REQUIRED" => (Tone::Waiting, "approval missing"),
        _ => (Tone::Good, "no approval needed"),
    };
    let checks = match gate.checks.as_str() {
        "SUCCESS" => (Tone::Good, "checks green"),
        "FAILURE" | "ERROR" => (Tone::Blocked, "checks failing"),
        "PENDING" | "EXPECTED" => (Tone::Waiting, "checks running"),
        _ => (Tone::Good, "no checks"),
    };
    let branch = match gate.branch.as_str() {
        "BEHIND" => (Tone::Waiting, "behind the base"),
        "DIRTY" => (Tone::Blocked, "conflicts"),
        "UNKNOWN" | "" => (Tone::Waiting, "GitHub is checking"),
        _ => (Tone::Good, "up to date"),
    };
    let facts = [approval, checks, branch];
    let headline = if gate.green {
        (Tone::Good, "Ready to merge".to_owned())
    } else {
        let worst = if facts.iter().any(|(t, _)| *t == Tone::Blocked) {
            Tone::Blocked
        } else {
            Tone::Waiting
        };
        (worst, format!("Not mergeable: {}", gate.why))
    };
    Some((headline, facts))
}

/// What a merge needs, as one line of facts: "ready to merge" first when
/// it is, else GitHub's reason only where the three facts do not already
/// say it (a draft, the merge queue, a rule). `draft_shown`: the PR's state
/// label beside it already says draft.
pub fn merge_facts(gate: &Gate, draft_shown: bool) -> Vec<(Tone, String)> {
    let Some(((headline, _), facts)) = verdict(gate) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if gate.green {
        out.push((Tone::Good, "ready to merge".to_owned()));
    } else {
        let said = facts.iter().any(|(t, _)| *t != Tone::Good);
        let draft = gate.why == "draft";
        // The facts never say draft or queued: those are named unless a
        // state label beside it already says draft.
        let named = draft || gate.why == "in the merge queue";
        if (!said || named) && !(draft && draft_shown) {
            out.push((headline, gate.why.clone()));
        }
    }
    out.extend(facts.into_iter().map(|(t, f)| (t, f.to_owned())));
    out
}

/// Whether the PR has something for Claude's fix mode: open review
/// threads, a failing check, changes requested, or conflicts. A clean,
/// green PR has nothing; a change of mind goes to the Claude tab.
pub fn needs_fix(gate: &Gate) -> bool {
    gate.open_threads > 0
        || matches!(gate.checks.as_str(), "FAILURE" | "ERROR")
        || gate.review == "CHANGES_REQUESTED"
        || gate.branch == "DIRTY"
}

/// The GitHub login of the org member with this name (Linear knows only
/// names): the org's members, 100 a page, matched without case. `None`
/// when nobody or more than one has the name. Blocking; off the UI thread.
pub fn login_of(org: &str, name: &str) -> Result<Option<String>> {
    const MEMBERS: &str = "query($org: String!, $after: String) { organization(login: $org) { \
        membersWithRole(first: 100, after: $after) { nodes { login name } \
        pageInfo { hasNextPage endCursor } } } }";
    let mut after: Option<String> = None;
    let mut members = Vec::new();
    for _ in 0..10 {
        let mut args = vec![
            "api".to_owned(),
            "graphql".into(),
            "--hostname".into(),
            "github.com".into(),
            "-f".into(),
            format!("query={MEMBERS}"),
            "-f".into(),
            format!("org={org}"),
        ];
        if let Some(cursor) = &after {
            args.extend(["-f".into(), format!("after={cursor}")]);
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let reply: serde_json::Value = serde_json::from_str(&crate::claude::gh(&args)?)?;
        let page = &reply["data"]["organization"]["membersWithRole"];
        for m in page["nodes"].as_array().into_iter().flatten() {
            members.push((
                m["login"].as_str().unwrap_or_default().to_owned(),
                m["name"].as_str().unwrap_or_default().to_owned(),
            ));
        }
        if page["pageInfo"]["hasNextPage"] != true {
            break;
        }
        after = page["pageInfo"]["endCursor"].as_str().map(str::to_owned);
    }
    Ok(member_named(&members, name))
}

/// The one member whose name is `name`, ignoring case and spaces around.
fn member_named(members: &[(String, String)], name: &str) -> Option<String> {
    let want = name.trim().to_lowercase();
    let mut found = members
        .iter()
        .filter(|(_, n)| !want.is_empty() && n.trim().to_lowercase() == want);
    let one = found.next()?;
    found.next().is_none().then(|| one.0.clone())
}

/// Asks `login` to review the PR, as the user. Blocking.
/// Puts `label` on the PR (a review router's, e.g. `review-roulette`).
pub fn add_label(url: &str, label: &str) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not labelling the PR");
    }
    parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    crate::claude::gh(&["pr", "edit", url, "--add-label", label])?;
    Ok(())
}

pub fn request_review(url: &str, login: &str) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not requesting a review");
    }
    let (owner, repo, number) =
        parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    crate::claude::gh(&[
        "api",
        "--hostname",
        "github.com",
        "-X",
        "POST",
        &format!("repos/{owner}/{repo}/pulls/{number}/requested_reviewers"),
        "-f",
        &format!("reviewers[]={login}"),
    ])?;
    Ok(())
}

/// Reviewed and still not mergeable: approved (or no review needed), open,
/// not a draft, not queued, and something a session can fix is in the way
/// (a check, the branch behind or in conflict). Land it offers a goal then.
pub fn landable(gate: &Gate) -> bool {
    gate.state == "OPEN"
        && !gate.draft
        && !gate.queued
        && !gate.green
        // Approved, or no review required and someone reviewed it anyway:
        // never a PR nobody looked at.
        && (gate.review == "APPROVED" || (gate.review.is_empty() && gate.reviewed_at.is_some()))
        && (matches!(
            gate.checks.as_str(),
            "FAILURE" | "ERROR" | "PENDING" | "EXPECTED"
        ) || matches!(gate.branch.as_str(), "BEHIND" | "DIRTY"))
}

/// Claude Code's `/goal` takes at most this many characters.
pub const GOAL_MAX: usize = 4000;

/// Land approved's goals for several PRs: the links and the rules once,
/// as few goals as fit `GOAL_MAX` each (one landing session per goal).
pub fn land_goals(urls: &[String]) -> Vec<String> {
    // The how is the land skill (skills/land): merge each once CLEAN, re-run
    // flaky checks, update branches, fix real failures, report how each
    // stands. The goal keeps the click's scope and the never-list.
    let land = crate::profile::get().skill("land");
    let rules = format!(
        "Land each PR below with /{land}: I clicked Land, so merging them is what I asked \
         for. Done means merged. Fast, all at once, never one after the other, without me: \
         `gh pr merge <url> --auto` on each once CLEAN. Never approve, never use --admin, \
         never force-push, never a PR not listed here. If I say pause, wait or stop, that \
         wins. Stop after 30 turns and tell me where each stands."
    );

    let mut goals: Vec<String> = Vec::new();
    let mut goal = rules.clone();
    let mut listed = 0;
    for url in urls {
        if listed > 0 && goal.len() + url.len() + 3 > GOAL_MAX {
            goals.push(std::mem::replace(&mut goal, rules.clone()));
            listed = 0;
        }
        goal.push_str("\n- ");
        goal.push_str(url);
        listed += 1;
    }
    if listed > 0 {
        goals.push(goal);
    }
    goals
}

/// Merges the base branch into the PR's branch, as GitHub's Update branch
/// does, bound to the head the user saw behind; checked again first.
pub fn update_branch(url: &str, head: &str) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not updating the branch");
    }
    let (owner, repo, number) =
        parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    let now = gate(url)?;
    if now.head != head {
        bail!("the PR got new commits; look again before updating");
    }
    if now.branch != "BEHIND" {
        bail!("the branch is not behind its base any more");
    }
    crate::claude::gh(&[
        "api",
        "--hostname",
        "github.com",
        "-X",
        "PUT",
        &format!("repos/{owner}/{repo}/pulls/{number}/update-branch"),
        "-f",
        &format!("expected_head_sha={head}"),
    ])?;
    Ok(())
}

/// Closes the PR without merging, as the user; GitHub can reopen it.
/// A draft PR marked ready for review.
pub fn ready(url: &str) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not marking the PR ready");
    }
    parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    crate::claude::gh(&["pr", "ready", url])?;
    Ok(())
}

pub fn close(url: &str) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not closing the PR");
    }
    let (owner, repo, number) =
        parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    crate::claude::gh(&[
        "api",
        "--hostname",
        "github.com",
        "-X",
        "PATCH",
        &format!("repos/{owner}/{repo}/pulls/{number}"),
        "-f",
        "state=closed",
    ])?;
    Ok(())
}

/// What a merge did: merged, or joined the base branch's merge queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Merged {
    Commit(String),
    Queued(Box<Gate>),
}

/// `gh pr view --json` has no merge-queue field, so the gate asks GraphQL.
const GATE: &str = "query($owner: String!, $repo: String!, $number: Int!) { \
    repository(owner: $owner, name: $repo) { pullRequest(number: $number) { \
    id title body headRefName baseRefName baseRepository { defaultBranchRef { name } } state isDraft mergeStateStatus headRefOid \
    isInMergeQueue reviewDecision \
    isMergeQueueEnabled viewerDidAuthor viewerLatestReview { state commit { oid } } \
    autoMergeRequest { enabledAt } viewerCanEnableAutoMerge author { login __typename } \
    commits(last: 1) { nodes { commit { committedDate statusCheckRollup { state } } } } \
    timelineItems(last: 10, itemTypes: [REMOVED_FROM_MERGE_QUEUE_EVENT]) { \
    nodes { __typename ... on RemovedFromMergeQueueEvent { reason createdAt } } } \
    reviewThreads(first: 100) { nodes { isResolved } } \
    reviewRequests(first: 10) { nodes { requestedReviewer { __typename \
    ... on User { login } ... on Team { slug } } } } \
    comments(last: 30) { nodes { viewerDidAuthor body author { login __typename } } } \
    labels(first: 30) { nodes { name } } \
    reviews(last: 30) { nodes { viewerDidAuthor state body commit { oid } author { login __typename } \
    comments(first: 20) { nodes { replyTo { id } } } } } } } }";

/// The PR's gate, from `gh api graphql`. Blocking; run it off the UI thread.
pub fn gate(url: &str) -> Result<Gate> {
    let (owner, repo, number) =
        parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    let json = crate::claude::gh(&[
        "api",
        "graphql",
        // The host `gh pr merge <url>` merges on, whatever GH_HOST says;
        // `parts` takes only github.com URLs.
        "--hostname",
        "github.com",
        "-f",
        &format!("query={GATE}"),
        "-f",
        &format!("owner={owner}"),
        "-f",
        &format!("repo={repo}"),
        "-F",
        &format!("number={number}"),
    ])?;
    gate_from(&json)
}

/// Every review thread on the PR, with who said what and whether it is
/// resolved, as text for a re-review. Blocking.
pub fn threads(owner: &str, repo: &str, number: u64) -> Result<String> {
    const THREADS: &str = "query($owner: String!, $repo: String!, $number: Int!) { \
        repository(owner: $owner, name: $repo) { pullRequest(number: $number) { \
        reviewThreads(first: 50) { nodes { isResolved path line \
        comments(first: 10) { nodes { author { login } body } } } } } } }";
    let json = crate::claude::gh(&[
        "api",
        "graphql",
        "--hostname",
        "github.com",
        "-f",
        &format!("query={THREADS}"),
        "-f",
        &format!("owner={owner}"),
        "-f",
        &format!("repo={repo}"),
        "-F",
        &format!("number={number}"),
    ])?;
    Ok(threads_text(&serde_json::from_str(&json)?))
}

/// What people said on a PR: review threads on their lines, and the
/// conversation (comments and review bodies), oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Talk {
    pub threads: Vec<Thread>,
    pub conversation: Vec<Said>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Thread {
    pub path: String,
    /// `RIGHT` (the new file) or `LEFT` (a removed line), and its line.
    pub side: String,
    pub line: usize,
    pub resolved: bool,
    pub comments: Vec<Said>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Said {
    pub author: String,
    pub body: String,
    pub at: String,
}

/// The PR's review threads and conversation. Blocking; off the UI thread.
pub fn talk(url: &str) -> Result<Talk> {
    const TALK: &str = "query($owner: String!, $repo: String!, $number: Int!) { \
        repository(owner: $owner, name: $repo) { pullRequest(number: $number) { \
        reviewThreads(first: 100) { nodes { isResolved path line originalLine diffSide \
        comments(first: 30) { nodes { author { login } body createdAt } } } } \
        comments(first: 50) { nodes { author { login } body createdAt } } \
        reviews(first: 50) { nodes { author { login } body createdAt state } } } } }";
    let (owner, repo, number) =
        parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    let json = crate::claude::gh(&[
        "api",
        "graphql",
        "--hostname",
        "github.com",
        "-f",
        &format!("query={TALK}"),
        "-f",
        &format!("owner={owner}"),
        "-f",
        &format!("repo={repo}"),
        "-F",
        &format!("number={number}"),
    ])?;
    Ok(talk_from(&serde_json::from_str(&json)?))
}

fn talk_from(reply: &serde_json::Value) -> Talk {
    let pr = &reply["data"]["repository"]["pullRequest"];
    let said = |c: &serde_json::Value| Said {
        author: c["author"]["login"].as_str().unwrap_or("ghost").to_owned(),
        body: c["body"].as_str().unwrap_or_default().trim().to_owned(),
        at: c["createdAt"].as_str().unwrap_or_default().to_owned(),
    };
    let nodes = |v: &serde_json::Value| v["nodes"].as_array().cloned().unwrap_or_default();
    let threads = nodes(&pr["reviewThreads"])
        .iter()
        .map(|t| Thread {
            path: t["path"].as_str().unwrap_or_default().to_owned(),
            side: t["diffSide"].as_str().unwrap_or("RIGHT").to_owned(),
            // An outdated thread has no line any more: where it was written.
            line: t["line"]
                .as_u64()
                .or_else(|| t["originalLine"].as_u64())
                .unwrap_or(0) as usize,
            resolved: t["isResolved"] == true,
            comments: nodes(&t["comments"]).iter().map(said).collect(),
        })
        .collect();
    // Review bodies with text join the conversation; an empty approval does not.
    let mut conversation: Vec<Said> = nodes(&pr["comments"])
        .iter()
        .chain(nodes(&pr["reviews"]).iter())
        .map(said)
        .filter(|s| !s.body.is_empty())
        .collect();
    conversation.sort_by(|a, b| a.at.cmp(&b.at));
    Talk {
        threads,
        conversation,
    }
}

fn threads_text(reply: &serde_json::Value) -> String {
    let empty = Vec::new();
    let nodes = reply
        .pointer("/data/repository/pullRequest/reviewThreads/nodes")
        .and_then(|n| n.as_array())
        .unwrap_or(&empty);
    let mut out = String::new();
    for t in nodes {
        let done = if t["isResolved"] == true {
            " (resolved)"
        } else {
            ""
        };
        let line = t["line"]
            .as_u64()
            .map_or(String::new(), |l| format!(":{l}"));
        out.push_str(&format!(
            "- {}{line}{done}\n",
            t["path"].as_str().unwrap_or("?")
        ));
        for c in t["comments"]["nodes"].as_array().unwrap_or(&empty) {
            let who = c
                .pointer("/author/login")
                .and_then(|l| l.as_str())
                .unwrap_or("?");
            let body = c["body"].as_str().unwrap_or_default().replace('\n', " ");
            out.push_str(&format!("  {who}: {body}\n"));
        }
    }
    out
}

/// `(owner, repo, number)` of `https://github.com/<owner>/<repo>/pull/<n>`.
pub fn parts(url: &str) -> Option<(&str, &str, u64)> {
    let mut parts = url.strip_prefix("https://github.com/")?.split('/');
    let (owner, repo, pull, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if pull != "pull" || owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo, number.parse().ok()?))
}

fn gate_from(json: &str) -> Result<Gate> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct View {
        state: String,
        is_draft: bool,
        merge_state_status: String,
        head_ref_oid: String,
        is_in_merge_queue: bool,
        #[serde(default)]
        review_threads: Threads,
        #[serde(default)]
        review_decision: Option<String>,
        #[serde(default)]
        commits: serde_json::Value,
        #[serde(default)]
        is_merge_queue_enabled: bool,
        #[serde(default)]
        viewer_did_author: bool,
        #[serde(default)]
        viewer_latest_review: serde_json::Value,
        #[serde(default)]
        reviews: serde_json::Value,
        #[serde(default)]
        id: String,
        #[serde(default)]
        auto_merge_request: serde_json::Value,
        #[serde(default)]
        viewer_can_enable_auto_merge: bool,
        #[serde(default)]
        base_repository: serde_json::Value,
        #[serde(default)]
        author: serde_json::Value,
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        head_ref_name: String,
        #[serde(default)]
        base_ref_name: String,
    }
    #[derive(serde::Deserialize, Default)]
    struct Threads {
        nodes: Vec<Thread>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Thread {
        is_resolved: bool,
    }
    let reply: serde_json::Value = serde_json::from_str(json)?;
    let pr = reply
        .pointer("/data/repository/pullRequest")
        .filter(|pr| !pr.is_null())
        .ok_or_else(|| anyhow!("GitHub has no such pull request"))?;
    let v: View = serde_json::from_value(pr.clone())?;
    let why = match (v.state.as_str(), v.is_draft, v.is_in_merge_queue) {
        ("MERGED", ..) => "already merged",
        ("OPEN", true, _) => "draft",
        // On its way in already; a second merge is not ours to start.
        ("OPEN", _, true) => "in the merge queue",
        ("OPEN", ..) => match v.merge_state_status.as_str() {
            "CLEAN" => "",
            // Which of the two: the review, or the checks.
            "BLOCKED" => match (
                v.review_decision.as_deref(),
                v.commits
                    .pointer("/nodes/0/commit/statusCheckRollup/state")
                    .and_then(|s| s.as_str()),
            ) {
                (Some("CHANGES_REQUESTED"), _) => "changes requested",
                (Some("REVIEW_REQUIRED"), _) => "a review is missing",
                (_, Some("FAILURE" | "ERROR")) => "a check failed",
                (_, Some("PENDING" | "EXPECTED")) => "checks still running",
                // Approved and green yet blocked: a rule on the base branch
                // (a stacked PR's base, a required reviewer group).
                (Some("APPROVED"), Some("SUCCESS")) => "blocked by a base branch rule",
                _ => "checks or reviews missing",
            },
            "UNSTABLE" => "a check is not green",
            "DIRTY" => "conflicts with the base",
            "BEHIND" => "behind the base branch",
            _ => "GitHub is still working it out",
        },
        _ => "closed",
    };
    let approved_here = v.viewer_latest_review["state"] == "APPROVED"
        && v.viewer_latest_review.pointer("/commit/oid") == Some(&v.head_ref_oid.clone().into());
    let can_approve = v.state == "OPEN"
        && !v.is_draft
        && !v.viewer_did_author
        && v.review_decision.as_deref() == Some("REVIEW_REQUIRED")
        && !approved_here;
    let auto_merge = !v.auto_merge_request.is_null();
    // A stacked PR: its base is another PR's branch, not the default one.
    // GitHub has no auto-merge and no merge queue for it.
    let stacked = v
        .base_repository
        .pointer("/defaultBranchRef/name")
        .and_then(|n| n.as_str())
        .is_some_and(|default| default != v.base_ref_name);
    let author = format!("@{}", v.author["login"].as_str().unwrap_or_default());
    let names = |list: &serde_json::Value, key: &str| -> Vec<String> {
        list["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n[key].as_str().map(str::to_owned))
            .collect()
    };
    Ok(Gate {
        on_it: on_it(pr, &author),
        labels: names(&pr["labels"], "name"),
        comments: names(&pr["comments"], "body"),
        title: v.title.clone(),
        body: v.body.clone(),
        head_branch: v.head_ref_name.clone(),
        base_branch: v.base_ref_name.clone(),
        author: v.author["login"].as_str().unwrap_or_default().to_owned(),
        author_bot: v.author["__typename"] == "Bot",
        id: v.id.clone(),
        auto_merge,
        can_auto_merge: why == "checks still running"
            && !auto_merge
            && v.viewer_can_enable_auto_merge
            && !stacked,
        reviewed_at: v.reviews["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .find(|r| {
                let new_comment = r["comments"]["nodes"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|c| c["replyTo"].is_null()));
                r["viewerDidAuthor"] == true
                    && (r["state"] != "COMMENTED"
                        || r["body"].as_str().is_some_and(|b| !b.is_empty())
                        || new_comment)
            })
            .and_then(|r| r.pointer("/commit/oid")?.as_str())
            .map(str::to_owned),
        own: v.viewer_did_author,
        review: v.review_decision.clone().unwrap_or_default(),
        checks: v
            .commits
            .pointer("/nodes/0/commit/statusCheckRollup/state")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_owned(),
        branch: v.merge_state_status.clone(),
        merge_queue: v.is_merge_queue_enabled && !stacked,
        can_approve,
        head: v.head_ref_oid,
        green: why.is_empty(),
        why: why.into(),
        state: v.state,
        draft: v.is_draft,
        queue_failed: queue_failed(pr),
        queued: v.is_in_merge_queue,
        open_threads: v
            .review_threads
            .nodes
            .iter()
            .filter(|t| !t.is_resolved)
            .count(),
    })
}

/// Approves the PR as the user, bound to `head`: only if GitHub still has
/// that head and still wants the user's review. Blocking.
pub fn approve(url: &str, head: &str, body: Option<&str>) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not approving");
    }
    let (owner, repo, number) =
        parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    let now = gate(url)?;
    if now.head != head {
        bail!("the PR got new commits; look again before approving");
    }
    if !now.can_approve {
        bail!("nothing to approve any more");
    }
    let path = format!("repos/{owner}/{repo}/pulls/{number}/reviews");
    let commit = format!("commit_id={head}");
    let mut args = vec![
        "api",
        "--hostname",
        "github.com",
        "-X",
        "POST",
        &path,
        "-f",
        &commit,
        "-f",
        "event=APPROVE",
    ];
    // The user's own short note, when they asked about the PR first, and
    // the line saying it came from here.
    let body = format!(
        "body={}",
        crate::profile::get()
            .attribution
            .signed(body.unwrap_or_default())
    );
    args.extend(["-f", &body]);
    crate::claude::gh(&args)?;
    Ok(())
}

/// One inline comment of a review the user is putting together: on the
/// new side (`RIGHT`) or, for a removed line, the old side (`LEFT`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Draft {
    pub path: String,
    pub line: usize,
    /// `LEFT` or `RIGHT`.
    pub side: String,
    pub body: String,
}

/// The file line GitHub knows diff line `n` by (1-based, as `numbered`
/// counts), from the hunk headers. A hunk header has none.
/// One of the user's open PRs, for the palette's repo scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MyPr {
    pub url: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub at: String,
    pub draft: bool,
}

/// `owner/repo` of a GitHub PR, issue or repo URL.
pub fn slug_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

/// One open PR on a repo page (`View::Repo`), as `gh pr list` tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoPr {
    pub url: String,
    pub number: u64,
    pub title: String,
    /// `@login`, or the bot's name.
    pub author: String,
    /// The author's display name ("Jamie Fox"), for the search.
    pub name: String,
    pub at: String,
    pub draft: bool,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED` or empty.
    pub review: String,
    /// `green`, `red`, `pending`, or empty without checks.
    pub checks: String,
    /// The user is asked to review it.
    pub review_requested: bool,
}

/// The open PRs of `owner/repo`, newest change first, with the ones that
/// wait on the user's review marked. Blocking.
pub fn repo_prs(slug: &str) -> Result<Vec<RepoPr>> {
    // Only the rollup's state, not every check run: `gh pr list --json
    // statusCheckRollup` is 600 KB for 60 PRs and GitHub cut it short now
    // and then ("unexpected end of JSON input").
    const PRS: &str = "query($owner: String!, $repo: String!) { repository(owner: $owner, name: $repo) { \
        pullRequests(states: OPEN, first: 60, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { \
        url number title updatedAt isDraft reviewDecision \
        author { __typename login ... on User { name } } \
        commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } } } } }";
    let (owner, repo) = slug
        .split_once('/')
        .ok_or_else(|| anyhow!("not owner/repo: {slug}"))?;
    let open = crate::claude::gh(&[
        "api",
        "graphql",
        "--hostname",
        "github.com",
        "-f",
        &format!("query={PRS}"),
        "-f",
        &format!("owner={owner}"),
        "-f",
        &format!("repo={repo}"),
    ])?;
    let review = crate::claude::gh(&[
        "pr",
        "list",
        "--repo",
        slug,
        "--state",
        "open",
        "--search",
        "review-requested:@me",
        "--limit",
        "60",
        "--json",
        "url",
    ])
    .unwrap_or_else(|_| "[]".into());
    repo_prs_from(&rows_of(&open)?, &review)
}

/// The GraphQL answer as `gh pr list --json` rows: the author's bot flag
/// and name, and the head's rollup state as the one check.
fn rows_of(json: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let nodes = value["data"]["repository"]["pullRequests"]["nodes"]
        .as_array()
        .ok_or_else(|| anyhow!("GitHub did not list the pull requests"))?;
    let rows: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            let state = &n["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"];
            let rollup = match state.as_str() {
                Some(state) => serde_json::json!([{ "state": state }]),
                None => serde_json::json!([]),
            };
            serde_json::json!({
                "url": n["url"], "number": n["number"], "title": n["title"],
                "updatedAt": n["updatedAt"], "isDraft": n["isDraft"],
                "reviewDecision": n["reviewDecision"],
                "author": {
                    "login": n["author"]["login"],
                    "name": n["author"]["name"],
                    "is_bot": n["author"]["__typename"] == "Bot",
                },
                "statusCheckRollup": rollup,
            })
        })
        .collect();
    Ok(serde_json::to_string(&rows)?)
}

fn repo_prs_from(open: &str, review: &str) -> Result<Vec<RepoPr>> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(open)?;
    let review: Vec<serde_json::Value> = serde_json::from_str(review).unwrap_or_default();
    let requested: Vec<&str> = review.iter().filter_map(|r| r["url"].as_str()).collect();
    let mut prs: Vec<RepoPr> = rows
        .iter()
        .map(|r| {
            let url = r["url"].as_str().unwrap_or_default().to_owned();
            RepoPr {
                review_requested: requested.contains(&url.as_str()),
                url,
                number: r["number"].as_u64().unwrap_or_default(),
                title: r["title"].as_str().unwrap_or_default().to_owned(),
                author: match r["author"]["login"].as_str() {
                    Some(login) if r["author"]["is_bot"] == true => login.to_owned(),
                    Some(login) => format!("@{login}"),
                    None => String::new(),
                },
                name: r["author"]["name"].as_str().unwrap_or_default().to_owned(),
                at: r["updatedAt"].as_str().unwrap_or_default().to_owned(),
                draft: r["isDraft"] == true,
                review: r["reviewDecision"].as_str().unwrap_or_default().to_owned(),
                checks: checks_summary(&r["statusCheckRollup"]),
            }
        })
        .filter(|p| !p.url.is_empty())
        .collect();
    prs.sort_by(|a, b| b.at.cmp(&a.at));
    Ok(prs)
}

/// One word for a PR's checks: every one green, any red, or still running.
fn checks_summary(rollup: &serde_json::Value) -> String {
    let Some(items) = rollup.as_array().filter(|a| !a.is_empty()) else {
        return String::new();
    };
    let words: Vec<&str> = items
        .iter()
        .filter_map(|c| c["conclusion"].as_str().or_else(|| c["state"].as_str()))
        .collect();
    if words.iter().any(|w| {
        matches!(
            *w,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "STARTUP_FAILURE"
        )
    }) {
        "red".into()
    } else if words.len() == items.len()
        && words
            .iter()
            .all(|w| matches!(*w, "SUCCESS" | "NEUTRAL" | "SKIPPED"))
    {
        "green".into()
    } else {
        "pending".into()
    }
}

/// Runs a workflow given as `owner/repo:workflow.yml` (`gh workflow run`).
pub fn run_workflow(target: &str) -> Result<()> {
    let (repo, workflow) = workflow_target(target)
        .ok_or_else(|| anyhow::anyhow!("not owner/repo:workflow.yml: {target}"))?;
    crate::claude::gh(&["workflow", "run", workflow, "--repo", repo])?;
    Ok(())
}

fn workflow_target(target: &str) -> Option<(&str, &str)> {
    let (repo, workflow) = target.trim().split_once(':')?;
    (repo.contains('/') && !workflow.is_empty()).then_some((repo, workflow))
}

/// Who can be asked to review on `owner/repo`: GitHub's assignable users,
/// login and name, so a name typed finds its login.
pub fn reviewers(slug: &str) -> Result<Vec<(String, String)>> {
    let (owner, repo) = slug
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("not owner/repo: {slug}"))?;
    let query = "query($o:String!,$r:String!,$endCursor:String){repository(owner:$o,name:$r)\
        {assignableUsers(first:100,after:$endCursor){nodes{login name} \
        pageInfo{hasNextPage endCursor}}}}";
    let out = crate::claude::gh(&[
        "api",
        "graphql",
        "--paginate",
        "-f",
        &format!("query={query}"),
        "-f",
        &format!("o={owner}"),
        "-f",
        &format!("r={repo}"),
        "-q",
        ".data.repository.assignableUsers.nodes[]|[.login,(.name//\"\")]|@tsv",
    ])?;
    Ok(reviewers_from(&out))
}

fn reviewers_from(tsv: &str) -> Vec<(String, String)> {
    let mut people: Vec<(String, String)> = tsv
        .lines()
        .filter_map(|l| {
            let (login, name) = l.split_once('\t').unwrap_or((l, ""));
            (!login.trim().is_empty()).then(|| (login.trim().to_owned(), name.trim().to_owned()))
        })
        .collect();
    people.sort();
    people.dedup();
    people
}

/// The people whose login or name holds `typed`, case-insensitive.
pub fn matching<'a>(people: &'a [(String, String)], typed: &str) -> Vec<&'a (String, String)> {
    let typed = typed.trim().trim_start_matches('@').to_lowercase();
    if typed.is_empty() {
        return Vec::new();
    }
    people
        .iter()
        .filter(|(login, name)| {
            login.to_lowercase().contains(&typed) || name.to_lowercase().contains(&typed)
        })
        .collect()
}

/// Every open PR the user opened, on any repo, newest first.
pub fn my_prs() -> Result<Vec<MyPr>> {
    search_prs(&["--author", "@me"])
}

/// KTLO's Dependabot search, never on an archived repo: its PRs can
/// never merge.
const DEPENDABOT: [&str; 3] = ["--author", "app/dependabot", "--archived=false"];

/// Open Dependabot PRs on the repos the user deploys (`repos`, the
/// watched release workflows' repos) and the ones that ask the user or a
/// team of theirs for review, newest first: KTLO's dependency lane.
/// Org-wide would be hundreds, most on repos nobody looks after.
pub fn dependabot_prs(repos: &[String]) -> Result<Vec<MyPr>> {
    let mut prs = search_prs(&[&["review-requested:@me"][..], &DEPENDABOT].concat())?;
    if !repos.is_empty() {
        let mut args = DEPENDABOT.to_vec();
        for repo in repos {
            args.extend(["--repo", repo.as_str()]);
        }
        prs.extend(search_prs(&args)?);
    }
    prs.sort_by(|a, b| b.at.cmp(&a.at));
    prs.dedup_by(|a, b| a.url == b.url);
    Ok(prs)
}

fn search_prs(query: &[&str]) -> Result<Vec<MyPr>> {
    let mut args = vec!["search", "prs"];
    args.extend(query);
    args.extend([
        "--state",
        "open",
        "--limit",
        "100",
        "--json",
        "url,title,repository,number,updatedAt,isDraft",
    ]);
    my_prs_from(&crate::claude::gh(&args)?)
}

fn my_prs_from(json: &str) -> Result<Vec<MyPr>> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(json)?;
    let mut prs: Vec<MyPr> = rows
        .iter()
        .map(|r| MyPr {
            url: r["url"].as_str().unwrap_or_default().to_owned(),
            repo: r["repository"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            number: r["number"].as_u64().unwrap_or_default(),
            title: r["title"].as_str().unwrap_or_default().to_owned(),
            at: r["updatedAt"].as_str().unwrap_or_default().to_owned(),
            draft: r["isDraft"] == true,
        })
        .filter(|p| !p.url.is_empty())
        .collect();
    prs.sort_by(|a, b| b.at.cmp(&a.at));
    Ok(prs)
}

/// The old and the new file's line numbers of diff line `n`: a removed
/// line has only the old, an added one only the new, an unchanged one both.
/// None for a hunk header.
pub fn numbers_at(file: &File, n: usize) -> Option<(Option<usize>, Option<usize>)> {
    let (mut old, mut new) = (0, 0);
    let start = |hunk: &str, sign: char| -> Option<usize> {
        let part = hunk.split_whitespace().find(|p| p.starts_with(sign))?;
        part[1..].split(',').next()?.parse().ok()
    };
    for (i, line) in file.lines.iter().enumerate() {
        let here = i + 1 == n;
        let numbers = match line {
            Line::Hunk(h) => {
                old = start(h, '-')?;
                new = start(h, '+')?;
                None
            }
            Line::Removed(_) => Some((Some(old), None)),
            Line::Added(_) => Some((None, Some(new))),
            Line::Context(_) => Some((Some(old), Some(new))),
        };
        if here {
            return numbers;
        }
        match line {
            Line::Hunk(_) => {}
            Line::Removed(_) => old += 1,
            Line::Added(_) => new += 1,
            Line::Context(_) => {
                old += 1;
                new += 1;
            }
        }
    }
    None
}

pub fn line_at(file: &File, n: usize) -> Option<(&'static str, usize)> {
    let (mut old, mut new) = (0, 0);
    let start = |hunk: &str, sign: char| -> Option<usize> {
        let part = hunk.split_whitespace().find(|p| p.starts_with(sign))?;
        part[1..].split(',').next()?.parse().ok()
    };
    for (i, line) in file.lines.iter().enumerate() {
        let here = i + 1 == n;
        match line {
            Line::Hunk(h) => {
                old = start(h, '-')?;
                new = start(h, '+')?;
                if here {
                    return None;
                }
            }
            Line::Removed(_) if here => return Some(("LEFT", old)),
            Line::Removed(_) => old += 1,
            _ if here => return Some(("RIGHT", new)),
            Line::Added(_) => new += 1,
            Line::Context(_) => {
                old += 1;
                new += 1;
            }
        }
    }
    None
}

/// Whether a text reads like a bot command (`@<name> <Word>!`), which
/// the app never posts anywhere.
pub fn command_like(text: &str) -> bool {
    text.match_indices('@').any(|(at, _)| {
        let rest = &text[at + 1..];
        let name = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        let Some(rest) = rest[name..].strip_prefix(' ') else {
            return false;
        };
        let word = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        name > 0 && word > 0 && rest[word..].starts_with('!')
    })
}

/// Asks for an AI review of the user's PR the way the profile says (see
/// `profile::AiReview`): the comment, the label off and on again, or the
/// reviewer requested again. Only from the user's click. Blocking.
pub fn request_ai_review(url: &str, how: &crate::profile::AiReview) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not asking for a review");
    }
    parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    let (comment, label, reviewer) = (how.comment.trim(), how.label.trim(), how.reviewer.trim());
    if !comment.is_empty() {
        if command_like(comment) {
            bail!("the AI review comment reads like a bot command; not posting it");
        }
        crate::claude::gh(&["pr", "comment", url, "--body", comment])?;
    } else if !label.is_empty() {
        // Off first, so putting it on fires the automation again; it may
        // not be there.
        let _ = crate::claude::gh(&["pr", "edit", url, "--remove-label", label]);
        crate::claude::gh(&["pr", "edit", url, "--add-label", label])?;
    } else if !reviewer.is_empty() {
        crate::claude::gh(&["pr", "edit", url, "--add-reviewer", reviewer])?;
    } else {
        bail!("the profile does not say how to ask for an AI review (github.aiReview)");
    }
    Ok(())
}

/// Posts the user's review: their summary and every draft as an inline comment, as a
/// Comment review, bound to `head` and re-checked right before. Blocking.
/// `changes`: a "Request changes" review instead of a comment.
pub fn review(url: &str, head: &str, drafts: &[Draft], body: &str, changes: bool) -> Result<()> {
    if crate::read_only() {
        bail!("read-only: not commenting");
    }
    let request = review_request(head, drafts, body, changes)?;
    let (owner, repo, number) =
        parts(url).ok_or_else(|| anyhow!("not a GitHub pull request: {url}"))?;
    if gate(url)?.head != head {
        bail!("the PR got new commits; look again before commenting");
    }
    crate::claude::gh_with(
        &[
            "api",
            "--hostname",
            "github.com",
            "-X",
            "POST",
            &format!("repos/{owner}/{repo}/pulls/{number}/reviews"),
            "--input",
            "-",
        ],
        Some(&request.to_string()),
    )?;
    Ok(())
}

/// The reviews API body: the line drafts and the user's summary, as one
/// COMMENT or `REQUEST_CHANGES` review bound to `head`. Refused when there
/// is nothing to say (request changes needs its reason written out), or
/// when any text reads like a bot command.
fn review_request(
    head: &str,
    drafts: &[Draft],
    body: &str,
    changes: bool,
) -> Result<serde_json::Value> {
    let body = body.trim();
    if changes && body.is_empty() {
        bail!("say what should change");
    }
    if drafts.is_empty() && body.is_empty() {
        bail!("no comments to post");
    }
    if command_like(body) || drafts.iter().any(|d| command_like(&d.body)) {
        bail!("a comment reads like a bot command; not posting it");
    }
    // A JSON body: gh's -f/-F fields scramble arrays of objects when
    // strings and numbers mix, and -F reads a body starting with @ as a file.
    let comments: Vec<serde_json::Value> = drafts
        .iter()
        .map(
            |d| serde_json::json!({"path": d.path, "line": d.line, "side": d.side, "body": d.body}),
        )
        .collect();
    Ok(serde_json::json!({
        "commit_id": head,
        "event": if changes { "REQUEST_CHANGES" } else { "COMMENT" },
        "body": crate::profile::get().attribution.signed(body),
        "comments": comments,
    }))
}

/// Merges the PR only if its head is still `head`: squash-merged, or, where
/// the base branch has a merge queue, added to the queue (the queue picks
/// the method). Then reads the result back. Blocking.
pub fn merge(url: &str, head: &str) -> Result<Merged> {
    if crate::read_only() {
        bail!("read-only: not merging");
    }
    // Checked again right before merging, not trusted from the last look.
    let now = gate(url)?;
    if now.head != head {
        bail!("the PR got new commits; look again before merging");
    }
    if !now.green {
        bail!("not mergeable any more: {}", now.why);
    }
    crate::claude::gh(&merge_args(url, head, now.merge_queue))?;
    let json = crate::claude::gh(&["pr", "view", url, "--json", "state,mergeCommit"])?;
    if let Some(sha) = merged_sha(&json) {
        return Ok(Merged::Commit(sha));
    }
    let after = gate(url)?;
    if now.merge_queue && after.queued {
        return Ok(Merged::Queued(Box::new(after)));
    }
    bail!("gh merged nothing; the PR is still open")
}

/// Merge when ready: GitHub merges the PR (or adds it to the merge queue)
/// once its checks pass, bound to `head` (`expectedHeadOid`) and checked
/// again first. `None` turns it off again. Only from the user's click, or
/// off when new commits came after it (events.rs). Blocking.
pub fn auto_merge(url: &str, head: Option<&str>) -> Result<Gate> {
    if crate::read_only() {
        bail!("read-only: not changing auto-merge");
    }
    let now = gate(url)?;
    let mut args = vec!["api", "graphql", "--hostname", "github.com"];
    let (query, id, oid);
    id = format!("id={}", now.id);
    match head {
        Some(head) => {
            if now.head != head {
                bail!("the PR got new commits; look again first");
            }
            if !now.can_auto_merge {
                bail!(
                    "not for merge when ready: {}",
                    match now.why.as_str() {
                        "" => "it can be merged now",
                        "checks still running" => "GitHub does not allow it here (a stacked PR?)",
                        why => why,
                    }
                );
            }
            // A queue picks its own method.
            query = if now.merge_queue {
                "query=mutation($id: ID!, $oid: GitObjectID!) { enablePullRequestAutoMerge(input: \
                 { pullRequestId: $id, expectedHeadOid: $oid }) { clientMutationId } }"
            } else {
                "query=mutation($id: ID!, $oid: GitObjectID!) { enablePullRequestAutoMerge(input: \
                 { pullRequestId: $id, expectedHeadOid: $oid, mergeMethod: SQUASH }) { clientMutationId } }"
            };
            oid = format!("oid={head}");
            args.extend(["-f", query, "-f", &id, "-f", &oid]);
        }
        None => {
            query = "query=mutation($id: ID!) { disablePullRequestAutoMerge(input: \
                     { pullRequestId: $id }) { clientMutationId } }";
            args.extend(["-f", query, "-f", &id]);
        }
    }
    crate::claude::gh(&args)?;
    gate(url)
}

/// Auto-merge is still on, but the PR moved past the head it was turned on at.
pub fn moved_past(head: &str, gate: &Gate) -> bool {
    gate.auto_merge && gate.head != head
}

/// Always bound to the head; a queue picks its own method, so no `--squash`.
fn merge_args<'a>(url: &'a str, head: &'a str, queue: bool) -> Vec<&'a str> {
    let mut args = vec!["pr", "merge", url, "--match-head-commit", head];
    if !queue {
        args.push("--squash");
    }
    args
}

fn merged_sha(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    (v["state"] == "MERGED")
        .then(|| v["mergeCommit"]["oid"].as_str().map(str::to_owned))
        .flatten()
}

/// The diff as text again, for Claude to read, cut at `limit` characters.
pub fn text(files: &[File], limit: usize) -> String {
    render(files, limit, false)
}

/// `text` with each line's number in its file (1 is the first, hunk
/// headers count), so Claude can point at lines the Diff tab can find.
pub fn numbered(files: &[File], limit: usize) -> String {
    render(files, limit, true)
}

fn render(files: &[File], limit: usize, numbered: bool) -> String {
    let mut out = String::new();
    for file in files {
        out.push_str(&format!(
            "\n=== {} (+{} -{})\n",
            file.path, file.added, file.removed
        ));
        for (i, line) in file.lines.iter().enumerate() {
            let (sign, text) = match line {
                Line::Hunk(t) => ("", t),
                Line::Added(t) => ("+", t),
                Line::Removed(t) => ("-", t),
                Line::Context(t) => (" ", t),
            };
            if numbered {
                out.push_str(&format!("{:>5} ", i + 1));
            }
            out.push_str(sign);
            out.push_str(text);
            out.push('\n');
            if out.len() > limit {
                out.push_str("\n[diff cut here; the rest is not shown]\n");
                return out;
            }
        }
    }
    out
}

/// Splits a unified diff into files and classified, coloured lines.
pub fn parse(text: &str) -> Vec<File> {
    let mut files: Vec<File> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.split(" b/").nth(1).unwrap_or(rest).to_owned();
            files.push(File {
                path,
                added: 0,
                removed: 0,
                lines: Vec::new(),
                spans: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with("+++") || line.starts_with("---") || line.starts_with("index ") {
            continue;
        }
        let parsed = if let Some(hunk) = line.strip_prefix("@@") {
            Line::Hunk(format!("@@{hunk}"))
        } else if let Some(added) = line.strip_prefix('+') {
            file.added += 1;
            Line::Added(added.to_owned())
        } else if let Some(removed) = line.strip_prefix('-') {
            file.removed += 1;
            Line::Removed(removed.to_owned())
        } else if file.lines.is_empty() {
            // File-level metadata before the first hunk (modes, renames).
            continue;
        } else {
            Line::Context(line.strip_prefix(' ').unwrap_or(line).to_owned())
        };
        file.lines.push(parsed);
    }
    // Syntax colours once, as it loads, never per frame.
    for file in &mut files {
        file.spans = crate::syntax::spans(file);
    }
    files
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_review_says_something_and_is_bound_to_the_head() {
        let draft = super::Draft {
            path: "src/a.rs".into(),
            line: 3,
            side: "RIGHT".into(),
            body: "why?".into(),
        };
        let r = super::review_request("abc123", &[], "Looks off here", true).unwrap();
        assert_eq!(r["event"], "REQUEST_CHANGES");
        assert_eq!(r["commit_id"], "abc123");
        assert!(r["body"].as_str().unwrap().starts_with("Looks off here"));
        let r = super::review_request("abc123", std::slice::from_ref(&draft), "", false).unwrap();
        assert_eq!(r["event"], "COMMENT");
        assert_eq!(r["comments"][0]["path"], "src/a.rs");
        // Requesting changes needs the reason; a comment needs something.
        assert!(super::review_request("abc123", std::slice::from_ref(&draft), " ", true).is_err());
        assert!(super::review_request("abc123", &[], "  ", false).is_err());
        assert!(super::review_request("abc123", &[], "@Robo Build!", false).is_err());
    }

    #[test]
    fn the_bots_poller_is_named_as_repo_and_workflow() {
        assert_eq!(
            super::workflow_target("acme/robo:poller.yml"),
            Some(("acme/robo", "poller.yml"))
        );
        assert_eq!(super::workflow_target("robo.yml"), None);
        assert_eq!(super::workflow_target(""), None);
    }

    #[test]
    fn a_name_typed_finds_the_reviewers_login() {
        let people = super::reviewers_from("lee-x\tLee Park\nkim\t\nkim\t\n");
        assert_eq!(people.len(), 2);
        let found = super::matching(&people, "park");
        assert_eq!(found[0].0, "lee-x");
        assert_eq!(super::matching(&people, "@KIM")[0].0, "kim");
        assert!(super::matching(&people, "  ").is_empty());
    }

    #[test]
    fn ktlo_leaves_archived_repos_out() {
        assert!(super::DEPENDABOT.contains(&"--archived=false"));
    }

    #[test]
    fn a_repo_page_reads_githubs_short_answer_with_names_and_one_rollup() {
        let json = r#"{"data":{"repository":{"pullRequests":{"nodes":[
          {"url":"https://github.com/o/r/pull/9","number":9,"title":"Durable","updatedAt":"2026-10-06T10:00:00Z",
           "isDraft":false,"reviewDecision":null,"author":{"__typename":"User","login":"jfox","name":"Jamie Fox"},
           "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"FAILURE"}}}]}},
          {"url":"https://github.com/o/r/pull/8","number":8,"title":"Bump","updatedAt":"2026-10-05T10:00:00Z",
           "isDraft":true,"reviewDecision":null,"author":{"__typename":"Bot","login":"dependabot"},
           "commits":{"nodes":[{"commit":{"statusCheckRollup":null}}]}}
        ]}}}}"#;
        let prs = super::repo_prs_from(&super::rows_of(json).unwrap(), "[]").unwrap();
        assert_eq!(
            (prs[0].author.as_str(), prs[0].name.as_str()),
            ("@jfox", "Jamie Fox")
        );
        assert_eq!(prs[0].checks, "red");
        assert_eq!(
            (prs[1].author.as_str(), prs[1].checks.as_str()),
            ("dependabot", "")
        );
        assert!(super::rows_of(r#"{"errors":[{"message":"x"}]}"#).is_err());
    }

    #[test]
    fn a_repo_page_lists_open_prs_with_who_waits_on_you() {
        let open = r#"[
          {"url":"https://github.com/o/r/pull/2","number":2,"title":"Two","author":{"login":"bob"},
           "updatedAt":"2026-10-02T10:00:00Z","isDraft":false,"reviewDecision":"REVIEW_REQUIRED",
           "statusCheckRollup":[{"conclusion":"SUCCESS"},{"state":"SUCCESS"}]},
          {"url":"https://github.com/o/r/pull/1","number":1,"title":"One","author":{"login":"dependabot","is_bot":true},
           "updatedAt":"2026-10-01T10:00:00Z","isDraft":true,"reviewDecision":"",
           "statusCheckRollup":[{"conclusion":"FAILURE"},{"conclusion":"SUCCESS"}]},
          {"url":"https://github.com/o/r/pull/3","number":3,"title":"Three","author":{"login":"ann"},
           "updatedAt":"2026-10-03T10:00:00Z","isDraft":false,"reviewDecision":"APPROVED",
           "statusCheckRollup":[{"status":"IN_PROGRESS"}]}
        ]"#;
        let review = r#"[{"url":"https://github.com/o/r/pull/2"}]"#;
        let prs = super::repo_prs_from(open, review).unwrap();
        let numbers: Vec<u64> = prs.iter().map(|p| p.number).collect();
        assert_eq!(numbers, [3, 2, 1], "newest change first");
        assert!(prs[1].review_requested && !prs[0].review_requested);
        assert_eq!(prs[0].checks, "pending");
        assert_eq!(prs[1].checks, "green");
        assert_eq!(prs[2].checks, "red");
        assert_eq!(prs[1].author, "@bob");
        assert_eq!(prs[2].author, "dependabot");
        assert!(prs[2].draft);
        assert_eq!(
            super::slug_of("https://github.com/o/r/pull/2").as_deref(),
            Some("o/r")
        );
        assert_eq!(super::slug_of("https://github.com/o"), None);
        assert_eq!(super::slug_of("https://linear.app/o/r"), None);
    }

    use super::*;

    #[test]
    fn my_prs_are_read_from_gh_search_newest_first() {
        let json = r#"[
            {"isDraft":true,"number":1196,"repository":{"name":"api"},
             "title":"feat(helm): xl","updatedAt":"2026-09-29T05:53:36Z",
             "url":"https://github.com/acme/api/pull/1196"},
            {"isDraft":false,"number":1207,"repository":{"name":"api"},
             "title":"ci(workers): ship workers","updatedAt":"2026-09-29T21:14:20Z",
             "url":"https://github.com/acme/api/pull/1207"}
        ]"#;
        let prs = my_prs_from(json).unwrap();
        assert_eq!(
            prs.iter().map(|p| p.number).collect::<Vec<_>>(),
            [1207, 1196]
        );
        assert_eq!(prs[1].repo, "api");
        assert!(prs[1].draft && !prs[0].draft);
    }

    #[test]
    fn a_linear_name_finds_its_one_github_login() {
        let members = [
            ("akim".to_owned(), "Alex Kim".to_owned()),
            ("slee".to_owned(), "Sam Lee".to_owned()),
            ("sam1".to_owned(), "Sam Lee".to_owned()),
            ("sam2".to_owned(), "sam lee ".to_owned()),
        ];
        assert_eq!(member_named(&members, "alex kim").as_deref(), Some("akim"));
        assert_eq!(
            member_named(&members, "Sam Lee"),
            None,
            "two of them: not a guess"
        );
        assert_eq!(member_named(&members, "Nobody"), None);
        assert_eq!(member_named(&members, ""), None);
    }

    #[test]
    fn a_ktlo_landing_merges_what_the_land_click_covers() {
        let goal = &land_goals(&["https://github.com/o/r/pull/1".into()])[0];
        assert!(goal.contains("merging them is what I asked"));
        assert!(goal.contains("all at once, never one after the other"));
        assert!(goal.contains("gh pr merge <url> --auto"));
        assert!(goal.contains("never use --admin") && goal.contains("Never approve"));
        assert!(!goal.contains("never merge,"));
        // The how lives in the land skill; the goal runs it.
        assert!(goal.contains("with /land"));
        let skill = include_str!("../skills/land/SKILL.md");
        assert!(skill.contains("gh pr merge <url> --auto") && skill.contains("never `--admin`"));
        assert!(skill.contains("never a PR outside the arguments"));
        assert!(skill.contains("mergeQueueEntry") && skill.contains("--disable-auto"));
        let allow = crate::claude::LANDING_ALLOW;
        assert!(allow.iter().all(|a| !a.contains("admin")));
        assert!(
            crate::claude::LANDING_DENY
                .iter()
                .any(|d| d.contains("--admin"))
        );
    }

    #[test]
    fn landing_goals_stay_under_claude_codes_limit() {
        let urls: Vec<String> = (0..120)
            .map(|n| format!("https://github.com/acme/api/pull/{n}"))
            .collect();
        let goals = land_goals(&urls);
        assert!(goals.len() > 1, "120 PRs need more than one goal");
        assert!(goals.iter().all(|g| g.len() <= GOAL_MAX));
        let listed: usize = goals.iter().map(|g| g.matches("\n- ").count()).sum();
        assert_eq!(listed, 120);
        assert_eq!(land_goals(&urls[..3]).len(), 1);
        assert!(land_goals(&[]).is_empty());
    }

    #[test]
    fn who_is_on_a_pr_without_the_user_the_author_or_bots() {
        let pr: serde_json::Value = serde_json::from_str(
            r#"{
              "reviews": {"nodes": [
                {"state": "COMMENTED", "author": {"login": "bob"}},
                {"state": "APPROVED", "author": {"login": "bob"}},
                {"state": "APPROVED", "viewerDidAuthor": true, "author": {"login": "me"}},
                {"state": "COMMENTED", "author": {"login": "coderabbit", "__typename": "Bot"}}]},
              "comments": {"nodes": [
                {"author": {"login": "carl"}}, {"author": {"login": "ana"}},
                {"author": {"login": "dependabot[bot]"}}]},
              "reviewRequests": {"nodes": [
                {"requestedReviewer": {"login": "dana"}},
                {"requestedReviewer": {"slug": "backend"}}]}
            }"#,
        )
        .unwrap();
        assert_eq!(
            on_it(&pr, "@ana"),
            [
                "@bob approved",
                "@carl commented",
                "@dana requested",
                "team backend requested"
            ]
        );
    }

    #[test]
    fn an_unchanged_line_has_its_old_number_on_the_left() {
        let file = File {
            path: "a".into(),
            added: 2,
            removed: 0,
            lines: vec![
                Line::Hunk("@@ -10,2 +10,4 @@".into()),
                Line::Context("a".into()),
                Line::Added("b".into()),
                Line::Added("c".into()),
                Line::Context("d".into()),
            ],
            spans: Vec::new(),
        };
        assert_eq!(numbers_at(&file, 2), Some((Some(10), Some(10))));
        assert_eq!(numbers_at(&file, 3), Some((None, Some(11))));
        assert_eq!(numbers_at(&file, 5), Some((Some(11), Some(13))));
        assert_eq!(numbers_at(&file, 1), None);
    }

    #[test]
    fn a_reviewed_pr_held_back_by_ci_or_its_branch_can_be_landed() {
        let gate = |review: &str, checks: &str, branch: &str| Gate {
            state: "OPEN".into(),
            review: review.into(),
            checks: checks.into(),
            branch: branch.into(),
            base_branch: "main".into(),
            ..Default::default()
        };
        assert!(landable(&gate("APPROVED", "FAILURE", "BLOCKED")));
        assert!(landable(&gate("APPROVED", "PENDING", "BLOCKED")));
        assert!(
            !landable(&gate("", "SUCCESS", "BEHIND")),
            "no review required, but nobody reviewed it"
        );
        let reviewed = Gate {
            reviewed_at: Some("2026-10-01T10:00:00Z".into()),
            ..gate("", "SUCCESS", "BEHIND")
        };
        assert!(landable(&reviewed));
        assert!(
            !landable(&gate("REVIEW_REQUIRED", "FAILURE", "BLOCKED")),
            "not reviewed yet"
        );
        assert!(!landable(&gate("CHANGES_REQUESTED", "FAILURE", "BLOCKED")));
        let green = Gate {
            green: true,
            ..gate("APPROVED", "SUCCESS", "CLEAN")
        };
        assert!(!landable(&green), "nothing to land");
    }

    #[test]
    fn the_gate_reads_the_prs_branch_and_its_base() {
        let json = r#"{"data":{"repository":{"pullRequest":{"state":"OPEN","isDraft":false,"isInMergeQueue":false,"mergeStateStatus":"CLEAN","headRefOid":"abc","headRefName":"abc-660-build","baseRefName":"main","title":"ABC-660: fix","body":"Fixes it."}}}}"#;
        let gate = gate_from(json).unwrap();
        assert_eq!(
            (gate.head_branch.as_str(), gate.base_branch.as_str()),
            ("abc-660-build", "main")
        );
        assert_eq!(
            (gate.title.as_str(), gate.body.as_str()),
            ("ABC-660: fix", "Fixes it.")
        );
    }

    #[test]
    fn a_prs_talk_puts_threads_on_their_lines_and_the_rest_in_order() {
        let reply = serde_json::json!({"data":{"repository":{"pullRequest":{
            "reviewThreads":{"nodes":[
                {"isResolved":false,"path":"a.ts","line":12,"diffSide":"RIGHT",
                 "comments":{"nodes":[{"author":{"login":"robin"},"body":"Why lowercase?","createdAt":"2026-09-28T10:00:00Z"}]}},
                {"isResolved":true,"path":"b.ts","line":null,"originalLine":4,"diffSide":"LEFT",
                 "comments":{"nodes":[]}}]},
            "comments":{"nodes":[{"author":{"login":"bot"},"body":"CI green","createdAt":"2026-09-28T12:00:00Z"}]},
            "reviews":{"nodes":[
                {"author":{"login":"sam"},"body":"Looks good","createdAt":"2026-09-28T11:00:00Z","state":"APPROVED"},
                {"author":{"login":"kim"},"body":"","createdAt":"2026-09-28T13:00:00Z","state":"APPROVED"}]}}}}});
        let talk = talk_from(&reply);
        assert_eq!(talk.threads.len(), 2);
        assert_eq!(
            (talk.threads[0].line, talk.threads[0].side.as_str()),
            (12, "RIGHT")
        );
        assert_eq!(talk.threads[0].comments[0].author, "robin");
        assert_eq!((talk.threads[1].line, talk.threads[1].resolved), (4, true));
        let order: Vec<&str> = talk
            .conversation
            .iter()
            .map(|s| s.author.as_str())
            .collect();
        assert_eq!(
            order,
            ["sam", "bot"],
            "oldest first, the empty approval left out"
        );
    }

    #[test]
    fn merge_when_ready_only_while_checks_run_and_off_after_new_commits() {
        let json = |auto: &str, can: bool| {
            format!(
                r#"{{"data":{{"repository":{{"pullRequest":{{"id":"PR_1","state":"OPEN","isDraft":false,
                "mergeStateStatus":"BLOCKED","headRefOid":"abc","isInMergeQueue":false,
                "reviewDecision":"APPROVED","autoMergeRequest":{auto},"viewerCanEnableAutoMerge":{can},
                "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{{"state":"PENDING"}}}}}}]}}}}}}}}}}"#
            )
        };
        let waiting = gate_from(&json("null", true)).unwrap();
        assert!(waiting.can_auto_merge && !waiting.auto_merge && !waiting.green);
        assert_eq!(waiting.id, "PR_1");
        assert!(!gate_from(&json("null", false)).unwrap().can_auto_merge);
        let on = gate_from(&json(r#"{"enabledAt":"2026-09-27T10:00:00Z"}"#, true)).unwrap();
        assert!(on.auto_merge && !on.can_auto_merge);
        assert!(!moved_past("abc", &on));
        assert!(moved_past("old", &on));
        assert!(!moved_past("old", &waiting));
        // A stacked PR (base is not the default branch): GitHub refuses.
        let stacked = json("null", true).replace(
            r#""id":"PR_1""#,
            r#""id":"PR_1","baseRefName":"feature","baseRepository":{"defaultBranchRef":{"name":"main"}}"#,
        );
        let stacked = stacked.replace(
            r#""isDraft":false"#,
            r#""isDraft":false,"isMergeQueueEnabled":true"#,
        );
        let gate = gate_from(&stacked).unwrap();
        // Neither auto-merge nor the queue: gh would ask for auto-merge.
        assert!(!gate.can_auto_merge && !gate.merge_queue);
        let on_main = stacked.replace(r#""baseRefName":"feature""#, r#""baseRefName":"main""#);
        let gate = gate_from(&on_main).unwrap();
        assert!(gate.can_auto_merge && gate.merge_queue);
    }

    #[test]
    fn the_merge_line_says_why_only_where_the_facts_do_not() {
        let gate = |green: bool, why: &str, branch: &str| Gate {
            state: "OPEN".into(),
            green,
            why: why.into(),
            review: "APPROVED".into(),
            checks: "SUCCESS".into(),
            branch: branch.into(),
            ..Default::default()
        };
        let words = |g: &Gate, shown| {
            merge_facts(g, shown)
                .into_iter()
                .map(|(_, f)| f)
                .collect::<Vec<_>>()
        };
        assert_eq!(words(&gate(true, "", "CLEAN"), true)[0], "ready to merge");
        // Conflicts are a fact already: no second "conflicts with the base".
        assert_eq!(
            words(&gate(false, "conflicts with the base", "DIRTY"), true),
            ["approved", "checks green", "conflicts"]
        );
        assert_eq!(words(&gate(false, "draft", "CLEAN"), true).len(), 3);
        assert_eq!(words(&gate(false, "draft", "CLEAN"), false)[0], "draft");
        // A draft with a red fact still says draft where nothing else does.
        assert_eq!(words(&gate(false, "draft", "DIRTY"), false)[0], "draft");
    }

    #[test]
    fn a_verdict_says_mergeable_or_why_not_in_three_facts() {
        let gate = |green: bool, why: &str, review: &str, checks: &str, branch: &str| Gate {
            state: "OPEN".into(),
            green,
            why: why.into(),
            review: review.into(),
            checks: checks.into(),
            branch: branch.into(),
            ..Default::default()
        };
        let (headline, facts) = verdict(&gate(true, "", "APPROVED", "SUCCESS", "CLEAN")).unwrap();
        assert_eq!(headline, (Tone::Good, "Ready to merge".into()));
        assert!(facts.iter().all(|(t, _)| *t == Tone::Good));
        let (headline, facts) = verdict(&gate(
            false,
            "behind the base branch",
            "APPROVED",
            "SUCCESS",
            "BEHIND",
        ))
        .unwrap();
        assert_eq!(headline.0, Tone::Waiting);
        assert_eq!(headline.1, "Not mergeable: behind the base branch");
        assert_eq!(facts[2], (Tone::Waiting, "behind the base"));
        let (headline, facts) = verdict(&gate(
            false,
            "a check failed",
            "REVIEW_REQUIRED",
            "FAILURE",
            "BLOCKED",
        ))
        .unwrap();
        assert_eq!(headline.0, Tone::Blocked);
        assert_eq!(facts[0], (Tone::Waiting, "approval missing"));
        assert_eq!(facts[1], (Tone::Blocked, "checks failing"));
        let merged = Gate {
            state: "MERGED".into(),
            ..Default::default()
        };
        assert_eq!(verdict(&merged), None);
    }

    #[test]
    fn a_clean_green_pr_has_nothing_to_fix() {
        let gate = |threads: usize, checks: &str, review: &str, branch: &str| Gate {
            open_threads: threads,
            checks: checks.into(),
            review: review.into(),
            branch: branch.into(),
            ..Default::default()
        };
        assert!(!needs_fix(&gate(0, "SUCCESS", "REVIEW_REQUIRED", "BEHIND")));
        assert!(needs_fix(&gate(2, "SUCCESS", "", "CLEAN")));
        assert!(needs_fix(&gate(0, "FAILURE", "", "CLEAN")));
        assert!(needs_fix(&gate(0, "SUCCESS", "CHANGES_REQUESTED", "CLEAN")));
        assert!(needs_fix(&gate(0, "SUCCESS", "", "DIRTY")));
    }

    /// The gate query against a real PR through the user's gh login.
    /// `GATE_PR=https://github.com/o/r/pull/n cargo test live_gate -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_gate() {
        let url = std::env::var("GATE_PR").expect("set GATE_PR");
        println!("{:#?}", gate(&url).unwrap());
    }

    #[test]
    fn a_diff_line_maps_to_the_file_line_github_knows() {
        let files = parse(
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -10,3 +20,4 @@ fn f\n keep\n-old\n+new\n+more\n tail\n",
        );
        let file = &files[0];
        assert_eq!(line_at(file, 1), None, "a hunk header");
        assert_eq!(line_at(file, 2), Some(("RIGHT", 20)));
        assert_eq!(line_at(file, 3), Some(("LEFT", 11)));
        assert_eq!(line_at(file, 4), Some(("RIGHT", 21)));
        assert_eq!(line_at(file, 6), Some(("RIGHT", 23)), "context after adds");
    }

    #[test]
    fn bot_commands_are_spotted_mentions_are_not() {
        assert!(command_like("please @Robo Build! now"));
        assert!(!command_like("uses @property and @Robo here"));
        assert!(!command_like("@ Build!"));
    }

    #[test]
    fn threads_read_as_who_said_what_where() {
        let reply = serde_json::json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[
            {"isResolved":true,"path":"a.py","line":3,"comments":{"nodes":[
                {"author":{"login":"me"},"body":"Why?"},{"author":{"login":"bot"},"body":"Fixed\nin x"}]}}]}}}}});
        assert_eq!(
            threads_text(&reply),
            "- a.py:3 (resolved)\n  me: Why?\n  bot: Fixed in x\n"
        );
    }

    #[test]
    fn numbered_lines_count_from_the_file_start_hunks_included() {
        let files =
            parse("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n");
        let text = numbered(&files, 10_000);
        assert!(text.contains("    1 @@"), "{text}");
        assert!(text.contains("    4 +new"), "{text}");
    }

    #[test]
    fn a_queue_merge_is_bound_to_the_head_and_leaves_the_method_to_the_queue() {
        let url = "https://github.com/o/r/pull/1";
        assert_eq!(
            merge_args(url, "abc", false),
            ["pr", "merge", url, "--match-head-commit", "abc", "--squash"]
        );
        assert_eq!(
            merge_args(url, "abc", true),
            ["pr", "merge", url, "--match-head-commit", "abc"]
        );
    }

    #[test]
    fn approve_only_when_a_review_is_missing_and_not_ours() {
        let pr = |extra: &str| {
            let json = format!(
                r#"{{"data":{{"repository":{{"pullRequest":{{"state":"OPEN","isDraft":false,"isInMergeQueue":false,"mergeStateStatus":"BLOCKED","headRefOid":"abc","reviewDecision":"REVIEW_REQUIRED","isMergeQueueEnabled":true{extra}}}}}}}}}"#
            );
            gate_from(&json).unwrap()
        };
        let open = pr("");
        assert!(open.can_approve);
        assert!(open.merge_queue);
        assert!(
            !pr(r#","viewerDidAuthor":true"#).can_approve,
            "not our own PR"
        );
        let approved = |oid: &str| {
            pr(&format!(
                r#","viewerLatestReview":{{"state":"APPROVED","commit":{{"oid":"{oid}"}}}}"#
            ))
            .can_approve
        };
        assert!(!approved("abc"), "already approved at this head");
        assert_eq!(open.reviewed_at, None);
        let review = |oid: &str, comment: &str| {
            format!(
                r#"{{"viewerDidAuthor":true,"state":"COMMENTED","body":"","commit":{{"oid":"{oid}"}},"comments":{{"nodes":[{comment}]}}}}"#
            )
        };
        let mine = review("old", r#"{"replyTo":null}"#);
        let reply = review("new", r#"{"replyTo":{"id":"x"}}"#);
        let reviewed = pr(&format!(r#","reviews":{{"nodes":[{mine},{reply}]}}"#));
        assert_eq!(
            reviewed.reviewed_at.as_deref(),
            Some("old"),
            "replies do not count"
        );
        assert!(approved("old"), "approved an older head: approve again");
    }

    #[test]
    fn merge_gate_only_opens_when_clean() {
        let view = |state: &str, draft: bool, queued: bool, merge: &str| {
            format!(
                r#"{{"data":{{"repository":{{"pullRequest":{{"state":"{state}","isDraft":{draft},"isInMergeQueue":{queued},"mergeStateStatus":"{merge}","headRefOid":"abc"}}}}}}}}"#
            )
        };
        let green = gate_from(&view("OPEN", false, false, "CLEAN")).unwrap();
        assert!(green.green);
        assert_eq!(green.head, "abc");
        assert_eq!(green.open_threads, 0, "no threads asked for: none open");
        let threads = r#"{"data":{"repository":{"pullRequest":{"state":"OPEN","isDraft":false,"isInMergeQueue":false,"mergeStateStatus":"BLOCKED","headRefOid":"abc","reviewThreads":{"nodes":[{"isResolved":false},{"isResolved":true},{"isResolved":false}]}}}}}"#;
        assert_eq!(gate_from(threads).unwrap().open_threads, 2);
        // BLOCKED says which: the review or the checks.
        let blocked = |extra: &str| {
            format!(
                r#"{{"data":{{"repository":{{"pullRequest":{{"state":"OPEN","isDraft":false,"isInMergeQueue":false,"mergeStateStatus":"BLOCKED","headRefOid":"abc"{extra}}}}}}}}}"#
            )
        };
        let why = |extra: &str| gate_from(&blocked(extra)).unwrap().why;
        assert_eq!(
            why(r#","reviewDecision":"REVIEW_REQUIRED""#),
            "a review is missing"
        );
        assert_eq!(
            why(r#","reviewDecision":"CHANGES_REQUESTED""#),
            "changes requested"
        );
        assert_eq!(
            why(
                r#","reviewDecision":"APPROVED","commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"FAILURE"}}}]}"#
            ),
            "a check failed"
        );
        assert_eq!(why(""), "checks or reviews missing");
        assert_eq!(
            why(
                r#","reviewDecision":"APPROVED","commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]}"#
            ),
            "blocked by a base branch rule"
        );
        for (json, why) in [
            (
                view("OPEN", false, false, "UNSTABLE"),
                "a check is not green",
            ),
            (
                view("OPEN", false, false, "BLOCKED"),
                "checks or reviews missing",
            ),
            (view("OPEN", true, false, "CLEAN"), "draft"),
            // Queued and clean: already on its way in, never merged twice.
            (view("OPEN", false, true, "CLEAN"), "in the merge queue"),
            (view("MERGED", false, false, "UNKNOWN"), "already merged"),
            (view("CLOSED", false, false, "UNKNOWN"), "closed"),
        ] {
            let gate = gate_from(&json).unwrap();
            assert!(!gate.green);
            assert_eq!(gate.why, why);
        }
        let queued = gate_from(&view("OPEN", false, true, "CLEAN")).unwrap();
        assert!(queued.queued && !queued.draft && queued.state == "OPEN");
        assert!(gate_from(r#"{"data":{"repository":{"pullRequest":null}}}"#).is_err());
        assert_eq!(
            parts("https://github.com/acme/web/pull/7"),
            Some(("acme", "web", 7))
        );
        assert_eq!(parts("https://github.com/acme/web/issues/7"), None);
        assert_eq!(
            merged_sha(r#"{"state":"MERGED","mergeCommit":{"oid":"f00"}}"#),
            Some("f00".into())
        );
        assert_eq!(merged_sha(r#"{"state":"OPEN","mergeCommit":null}"#), None);
    }

    #[test]
    fn unified_diff_becomes_files_and_lines() {
        let text = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@ fn main\n keep\n-old\n+new\n+more\ndiff --git a/b.txt b/b.txt\nnew file mode 100644\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1 @@\n+hello\n";
        let files = parse(text);
        assert_eq!(files.len(), 2);
        assert_eq!(
            (files[0].path.as_str(), files[0].added, files[0].removed),
            ("src/a.rs", 2, 1)
        );
        assert_eq!(
            files[0].lines[0],
            Line::Hunk("@@ -1,3 +1,3 @@ fn main".into())
        );
        assert_eq!(files[0].lines[1], Line::Context("keep".into()));
        assert_eq!((files[1].path.as_str(), files[1].added), ("b.txt", 1));
    }

    #[test]
    fn a_pr_the_queue_dropped_for_a_failure_says_why_until_a_new_commit() {
        let pr = |reason: &str, committed: &str, state: &str, queued: bool| {
            serde_json::json!({
                "state": state,
                "isInMergeQueue": queued,
                "commits": {"nodes": [{"commit": {"committedDate": committed}}]},
                "timelineItems": {"nodes": [{
                    "__typename": "RemovedFromMergeQueueEvent",
                    "reason": reason,
                    "createdAt": "2026-10-15T11:14:46Z"
                }]}
            })
        };
        let before = "2026-10-15T10:00:00Z";
        assert_eq!(
            queue_failed(&pr("failed_checks", before, "OPEN", false)),
            "failed_checks"
        );
        // Fixed since, merged, queued again, closed.
        assert_eq!(
            queue_failed(&pr("failed_checks", "2026-10-15T12:00:00Z", "OPEN", false)),
            ""
        );
        assert_eq!(queue_failed(&pr("manual", before, "OPEN", false)), "manual");
        assert_eq!(queue_failed(&pr("merged", before, "MERGED", false)), "");
        assert_eq!(queue_failed(&pr("failed_checks", before, "OPEN", true)), "");
        assert_eq!(
            queue_failed(&pr("failed_checks", before, "CLOSED", false)),
            ""
        );
        // Failed, queued again, then taken out by hand: the last one says why.
        let mut later = pr("failed_checks", before, "OPEN", false);
        later["timelineItems"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"reason": "manual", "createdAt": "2026-10-15T13:48:38Z"}));
        assert_eq!(queue_failed(&later), "manual");
        // Never dropped.
        let none = serde_json::json!({"state": "OPEN", "timelineItems": {"nodes": []}});
        assert_eq!(queue_failed(&none), "");
    }
}
