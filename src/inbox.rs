//! Inbox: what Linear and GitHub notified the user about, in one list,
//! kept to what asks something of them. One row per ticket or PR: a PR's
//! GitHub notification, its Linear notification and the ticket it is on
//! are one row, with one verb.
//!
//! Only an allowlist gets in (`linear_ask`, `github_ask`): review requests
//! still open for the user themselves (not a team), mentions, assignments,
//! SLAs at risk, activity on the user's own PRs. Status changes, CI,
//! subscriptions and team requests stay out, counted in one line, and are
//! never marked read behind the user's back. So do notes on a PR that is
//! merged or closed already: nothing is left to do there. Opening a row or `x` marks
//! its notifications read on both sides.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde_json::Value;

/// Older than this, a row is counted, not listed: GitHub still has it.
const STALE_DAYS: i64 = 14;

/// What a row asks of the user, most pressing first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ask {
    Review,
    Reply,
    Assigned,
    Sla,
    YourPr,
}

impl Ask {
    pub fn verb(self) -> &'static str {
        match self {
            Ask::Review => "review",
            Ask::Reply => "reply",
            Ask::Assigned => "pick up",
            Ask::Sla => "act",
            Ask::YourPr => "look",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Linear,
    GitHub,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub source: Source,
    /// The Linear notification or GitHub thread id; `rr:<url>` for an open
    /// review request, which has nothing to mark read.
    pub id: String,
    pub ask: Ask,
    pub title: String,
    /// Who did what: "Ben mentioned you in a comment".
    pub why: String,
    pub url: String,
    /// `https://github.com/<owner>/<repo>/pull/<n>`, when about a PR.
    pub pr: Option<String>,
    /// The Linear issue id, when about one.
    pub issue: Option<String>,
    pub at: String,
}

/// One row: every note about the same ticket or PR.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub key: String,
    pub ask: Ask,
    pub title: String,
    pub why: String,
    pub url: String,
    pub pr: Option<String>,
    /// The board ticket to open, directly or through its PR.
    pub issue: Option<String>,
    pub at: String,
    pub notes: Vec<Note>,
}

/// One source's notes, and how many it left out as not actionable.
pub type Fetched = (Vec<Note>, usize);

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// Linear's notification types that ask something of the user.
fn linear_ask(kind: &str) -> Option<Ask> {
    match kind {
        "pullRequestReviewRequested" => Some(Ask::Review),
        k if k.contains("Mention") => Some(Ask::Reply),
        "issueAssignedToYou" => Some(Ask::Assigned),
        k if k.starts_with("issueSla") => Some(Ask::Sla),
        _ => None,
    }
}

/// Unread Linear notifications (`Client::notifications` nodes) as notes.
pub fn from_linear(nodes: &[Value]) -> Fetched {
    let mut skipped = 0;
    let notes = nodes
        .iter()
        .filter(|n| n["readAt"].is_null() && n["archivedAt"].is_null())
        .filter(|n| n["snoozedUntilAt"].is_null() || !n["unsnoozedAt"].is_null())
        .filter_map(|n| {
            let done = matches!(
                n["pullRequest"]["status"].as_str(),
                Some("merged" | "closed")
            );
            let Some(ask) = linear_ask(n["type"].as_str().unwrap_or_default()).filter(|_| !done)
            else {
                skipped += 1;
                return None;
            };
            Some(Note {
                source: Source::Linear,
                id: text(&n["id"]),
                ask,
                title: text(&n["title"]),
                why: text(&n["subtitle"]),
                url: text(&n["url"]),
                pr: n["pullRequest"]["url"].as_str().map(str::to_owned),
                issue: n["issue"]["id"].as_str().map(str::to_owned),
                at: text(&n["updatedAt"]),
            })
        })
        .collect();
    (notes, skipped)
}

/// GitHub's notification reasons that ask something of the user. A review
/// request only counts while the search still lists it (`combine`).
fn github_ask(reason: &str) -> Option<Ask> {
    match reason {
        "review_requested" => Some(Ask::Review),
        "mention" => Some(Ask::Reply),
        "assign" => Some(Ask::Assigned),
        "author" => Some(Ask::YourPr),
        _ => None,
    }
}

/// `https://api.github.com/repos/o/r/pulls/7` → `https://github.com/o/r/pull/7`.
fn html_url(api: &str) -> Option<String> {
    let rest = api.strip_prefix("https://api.github.com/repos/")?;
    Some(format!(
        "https://github.com/{}",
        rest.replacen("/pulls/", "/pull/", 1)
    ))
}

/// Unread GitHub notifications (`gh api notifications`) as notes.
pub fn from_github(threads: &[Value]) -> Fetched {
    let mut skipped = 0;
    let notes = threads
        .iter()
        .filter(|t| t["unread"] == true)
        .filter_map(|t| {
            let kind = t["subject"]["type"].as_str().unwrap_or_default();
            let url = html_url(t["subject"]["url"].as_str().unwrap_or_default());
            let ask = github_ask(t["reason"].as_str().unwrap_or_default());
            let (Some(ask), Some(url), "PullRequest" | "Issue") = (ask, url, kind) else {
                skipped += 1;
                return None;
            };
            let why = match ask {
                Ask::Review => "review requested",
                Ask::Reply => "mentioned you",
                Ask::Assigned => "assigned to you",
                _ => "activity on yours",
            };
            Some(Note {
                source: Source::GitHub,
                id: text(&t["id"]),
                ask,
                title: text(&t["subject"]["title"]),
                why: format!("{} · {why}", text(&t["repository"]["name"])),
                pr: (kind == "PullRequest").then(|| url.clone()),
                url,
                issue: None,
                at: text(&t["updated_at"]),
            })
        })
        .collect();
    (notes, skipped)
}

/// Open, non-draft PRs that ask the user's own review (search/issues).
pub fn from_requests(prs: &[Value]) -> Vec<Note> {
    prs.iter()
        .map(|p| {
            let url = text(&p["html_url"]);
            let repo = p["repository_url"].as_str().unwrap_or_default();
            Note {
                source: Source::GitHub,
                id: format!("rr:{url}"),
                ask: Ask::Review,
                title: text(&p["title"]),
                why: format!(
                    "{} · {} asks your review",
                    repo.rsplit('/').next().unwrap_or_default(),
                    text(&p["user"]["login"]).trim_end_matches("[bot]")
                ),
                pr: Some(url.clone()),
                url,
                issue: None,
                at: text(&p["updated_at"]),
            }
        })
        .collect()
}

/// The user's GitHub side, through `gh`: unread notifications and the
/// review requests still open. Blocking; one call each, once a minute.
pub fn github() -> Result<Fetched> {
    let threads = crate::claude::gh(&["api", "notifications?per_page=50"])?;
    let (mut notes, skipped) = from_github(&serde_json::from_str::<Vec<Value>>(&threads)?);
    // `gh search prs` quotes its query and loses `user-review-requested`.
    let requests = crate::claude::gh(&[
        "api",
        "-X",
        "GET",
        "search/issues",
        "-f",
        "q=is:pr is:open draft:false user-review-requested:@me",
        "-f",
        "per_page=50",
    ])?;
    let requests: Value = serde_json::from_str(&requests)?;
    notes.extend(from_requests(
        requests["items"].as_array().map_or(&[][..], Vec::as_slice),
    ));
    // GitHub's notifications say nothing of the PR's state: one query for
    // all of them. Without its answer nothing is hidden.
    let prs: Vec<&str> = notes.iter().filter_map(|n| n.pr.as_deref()).collect();
    let done = match states_query(&prs) {
        Some(query) => crate::claude::gh(&["api", "graphql", "-f", &format!("query={query}")])
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .map(|reply| finished(&reply, &prs))
            .unwrap_or_default(),
        None => HashSet::new(),
    };
    let before = notes.len();
    notes.retain(|n| n.pr.as_ref().is_none_or(|pr| !done.contains(pr)));
    let hidden = before - notes.len();
    Ok((notes, skipped + hidden))
}

/// One GraphQL query for the state of each PR, aliased `p0`, `p1`, ...
/// Only plain owner and repo names get in.
fn states_query(prs: &[&str]) -> Option<String> {
    let plain = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    let parts: Vec<String> = prs
        .iter()
        .enumerate()
        .filter_map(|(i, url)| {
            let (owner, repo, number) = crate::github::parts(url)?;
            (plain(owner) && plain(repo)).then(|| {
                format!(
                    "p{i}: repository(owner: \"{owner}\", name: \"{repo}\") {{ pullRequest(number: {number}) {{ state }} }}"
                )
            })
        })
        .collect();
    (!parts.is_empty()).then(|| format!("{{ {} }}", parts.join(" ")))
}

/// The PRs `states_query` found merged or closed.
fn finished(reply: &Value, prs: &[&str]) -> HashSet<String> {
    prs.iter()
        .enumerate()
        .filter(|(i, _)| {
            matches!(
                reply["data"][format!("p{i}")]["pullRequest"]["state"].as_str(),
                Some("MERGED" | "CLOSED")
            )
        })
        .map(|(_, url)| (*url).to_owned())
        .collect()
}

/// Marks GitHub threads read, as the user. Blocking.
pub fn github_read(ids: &[String]) -> Result<()> {
    if crate::read_only() {
        anyhow::bail!("read-only: nothing is marked read on GitHub");
    }
    for id in ids {
        crate::claude::gh(&["api", "-X", "PATCH", &format!("notifications/threads/{id}")])?;
    }
    Ok(())
}

fn stale(at: &str) -> bool {
    at.parse::<jiff::Timestamp>()
        .is_ok_and(|t| jiff::Timestamp::now().as_second() - t.as_second() > STALE_DAYS * 86_400)
}

/// The rows: notes grouped by ticket (a PR on a board card counts as its
/// ticket), else by PR, else by link; most pressing first, then newest.
/// Review notes for a PR the search no longer lists are done already;
/// `requests_known` is false when the GitHub side did not answer. Returns
/// the rows and how many stale ones were left out.
pub fn combine(
    notes: &[Note],
    requests_known: bool,
    ticket_of_pr: &HashMap<String, String>,
    done: &HashSet<String>,
) -> (Vec<Item>, usize) {
    let requested: HashSet<&str> = notes
        .iter()
        .filter(|n| n.id.starts_with("rr:"))
        .filter_map(|n| n.pr.as_deref())
        .collect();
    let mut items: Vec<Item> = Vec::new();
    for note in notes {
        if done.contains(&note.id) {
            continue;
        }
        let answered = note.ask == Ask::Review
            && !note.id.starts_with("rr:")
            && requests_known
            && note.pr.as_deref().is_some_and(|pr| !requested.contains(pr));
        if answered {
            continue;
        }
        let issue = note.issue.clone().or_else(|| {
            note.pr
                .as_ref()
                .and_then(|pr| ticket_of_pr.get(pr).cloned())
        });
        let key = issue
            .clone()
            .or(note.pr.clone())
            .unwrap_or(note.url.clone());
        match items.iter_mut().find(|i| i.key == key) {
            Some(item) => {
                if (note.ask, std::cmp::Reverse(&note.at)) < (item.ask, std::cmp::Reverse(&item.at))
                {
                    item.ask = note.ask;
                    item.why = note.why.clone();
                }
                if note.at > item.at {
                    item.at = note.at.clone();
                }
                item.pr = item.pr.take().or(note.pr.clone());
                item.notes.push(note.clone());
            }
            None => items.push(Item {
                key,
                ask: note.ask,
                title: note.title.clone(),
                why: note.why.clone(),
                url: note.pr.clone().unwrap_or(note.url.clone()),
                pr: note.pr.clone(),
                issue,
                at: note.at.clone(),
                notes: vec![note.clone()],
            }),
        }
    }
    let before = items.len();
    items.retain(|i| !stale(&i.at));
    let old = before - items.len();
    items.sort_by(|a, b| a.ask.cmp(&b.ask).then(b.at.cmp(&a.at)));
    (items, old)
}

/// The first message of the session that handles every notification: each
/// row with its link, most pressing first, then how to go through them.
pub fn brief(items: &[Item]) -> String {
    let mut out = format!(
        "Handle my notifications. There are {}, most pressing first:\n\n",
        items.len()
    );
    for (i, item) in items.iter().enumerate() {
        out.push_str(&format!("{}. {}: {}\n", i + 1, item.ask.verb(), item.title));
        if !item.why.trim().is_empty() {
            out.push_str(&format!("   {}\n", item.why.trim()));
        }
        out.push_str(&format!("   {}\n", item.url));
        if let Some(pr) = item.pr.as_ref().filter(|pr| **pr != item.url) {
            out.push_str(&format!("   PR: {pr}\n"));
        }
    }
    out.push_str(
        "\nGo through them one by one. For each, open the link, then write:\n\
         - What it asks of me, in one line.\n\
         - What you did: read the PR or thread, drafted the answer, checked the ticket.\n\
         - My move: exactly what I should do next, one line (approve, reply with the draft \
           below, decide between two options, ignore it and why).\n\
         For a review, also: your verdict with the reason, and what I should look at myself: \
         the two or three places that matter, as file:line, each with why (risk, an edge case, \
         a behaviour change, a missing test). For a question or mention, the draft answer. \
         Ask me before posting, approving, merging or changing anything.\n\n\
         At the end, my list: every move above as a short checklist, most pressing first.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> String {
        jiff::Timestamp::now().to_string()
    }

    #[test]
    fn the_handle_all_brief_lists_every_row_with_its_link_and_the_rules() {
        let row = |title: &str, url: &str, pr: Option<&str>| Item {
            key: title.into(),
            ask: Ask::Review,
            title: title.into(),
            why: "Robin requested your review".into(),
            url: url.into(),
            pr: pr.map(str::to_owned),
            issue: None,
            at: String::new(),
            notes: Vec::new(),
        };
        let brief = brief(&[
            row(
                "fix: it",
                "https://github.com/o/r/pull/7",
                Some("https://github.com/o/r/pull/7"),
            ),
            row(
                "DEV-1",
                "https://linear.app/a/issue/DEV-1",
                Some("https://github.com/o/r/pull/8"),
            ),
        ]);
        assert!(brief.starts_with("Handle my notifications. There are 2"));
        assert!(brief.contains("1. review: fix: it\n"));
        assert!(
            brief
                .contains("https://linear.app/a/issue/DEV-1\n   PR: https://github.com/o/r/pull/8")
        );
        assert_eq!(
            brief.matches("PR: ").count(),
            1,
            "no PR line repeating the link"
        );
        assert!(brief.contains("Ask me before posting, approving, merging"));
        // Every row ends with the user's move; a review says what to look at.
        assert!(brief.contains("My move: exactly what I should do next"));
        assert!(brief.contains("what I should look at myself"));
    }

    #[test]
    fn keeps_only_what_asks_something() {
        let at = now();
        let linear = [
            json!({"id": "l1", "type": "pullRequestReviewRequested", "title": "fix", "subtitle": "bot requested your review", "url": "https://linear.app/x/review/fix", "readAt": null, "archivedAt": null, "snoozedUntilAt": null, "unsnoozedAt": null, "updatedAt": at, "pullRequest": {"url": "https://github.com/o/r/pull/7"}}),
            json!({"id": "l2", "type": "issueStatusChanged", "readAt": null, "archivedAt": null, "snoozedUntilAt": null, "updatedAt": at}),
            json!({"id": "l3", "type": "issueCommentMention", "readAt": "2026-01-01", "archivedAt": null, "updatedAt": at}),
        ];
        let (notes, skipped) = from_linear(&linear);
        assert_eq!((notes.len(), skipped), (1, 1));
        assert_eq!(
            notes[0].pr.as_deref(),
            Some("https://github.com/o/r/pull/7")
        );

        let github = [
            json!({"id": "g1", "unread": true, "reason": "review_requested", "updated_at": at, "repository": {"name": "r"}, "subject": {"type": "PullRequest", "title": "fix", "url": "https://api.github.com/repos/o/r/pulls/7"}}),
            json!({"id": "g2", "unread": true, "reason": "state_change", "updated_at": at, "repository": {"name": "r"}, "subject": {"type": "PullRequest", "title": "x", "url": "https://api.github.com/repos/o/r/pulls/8"}}),
            json!({"id": "g3", "unread": true, "reason": "review_requested", "updated_at": at, "repository": {"name": "r"}, "subject": {"type": "PullRequest", "title": "done", "url": "https://api.github.com/repos/o/r/pulls/9"}}),
        ];
        let (mut all, skipped) = from_github(&github);
        assert_eq!((all.len(), skipped), (2, 1));
        all.extend(notes);
        all.extend(from_requests(&[json!({"html_url": "https://github.com/o/r/pull/7", "title": "fix", "updated_at": at, "user": {"login": "bot[bot]"}, "repository_url": "https://api.github.com/repos/o/r"})]));

        // PR 7 from both sides and the search is one row, on its ticket;
        // PR 9's request was answered, so its notification is not a row.
        let tickets =
            HashMap::from([("https://github.com/o/r/pull/7".to_owned(), "T1".to_owned())]);
        let (items, old) = combine(&all, true, &tickets, &HashSet::new());
        assert_eq!((items.len(), old), (1, 0));
        assert_eq!(items[0].issue.as_deref(), Some("T1"));
        assert_eq!(items[0].notes.len(), 3);
        assert_eq!(items[0].ask.verb(), "review");

        // Without the search's answer nothing counts as answered.
        assert_eq!(combine(&all, false, &tickets, &HashSet::new()).0.len(), 2);
        // Marked read here: gone at once, before either side answers.
        let done: HashSet<String> = items[0].notes.iter().map(|n| n.id.clone()).collect();
        assert!(combine(&all, true, &tickets, &done).0.is_empty());
    }

    #[test]
    fn merged_or_closed_prs_ask_nothing() {
        let linear = [
            json!({"id": "l1", "type": "pullRequestCommentMention", "readAt": null, "archivedAt": null, "snoozedUntilAt": null, "updatedAt": now(), "pullRequest": {"url": "https://github.com/o/r/pull/7", "status": "merged"}}),
        ];
        assert_eq!(from_linear(&linear).0.len(), 0);

        let prs = [
            "https://github.com/o/r/pull/7",
            "https://github.com/o/r/pull/8",
            "https://github.com/o/r/pull/9",
        ];
        let query = states_query(&prs).unwrap();
        assert!(
            query.contains(r#"p1: repository(owner: "o", name: "r") { pullRequest(number: 8)"#)
        );
        assert!(states_query(&[r#"https://github.com/o"){x}/r/pull/1"#]).is_none());
        let reply = json!({"data": {"p0": {"pullRequest": {"state": "MERGED"}}, "p1": {"pullRequest": {"state": "OPEN"}}, "p2": {"pullRequest": {"state": "CLOSED"}}}});
        let done = finished(&reply, &prs);
        assert_eq!(done.len(), 2);
        assert!(!done.contains(prs[1]));
    }

    #[test]
    fn old_rows_are_counted_not_listed() {
        let note = |at: &str| Note {
            source: Source::GitHub,
            id: format!("rr:{at}"),
            ask: Ask::Review,
            title: String::new(),
            why: String::new(),
            url: format!("https://github.com/o/r/pull/{at}"),
            pr: Some(format!("https://github.com/o/r/pull/{at}")),
            issue: None,
            at: at.to_owned(),
        };
        let (items, old) = combine(
            &[note(&now()), note("2026-01-01T00:00:00Z")],
            true,
            &HashMap::new(),
            &HashSet::new(),
        );
        assert_eq!((items.len(), old), (1, 1));
    }
}
