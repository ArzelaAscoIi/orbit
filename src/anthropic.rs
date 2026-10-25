//! Small Claude helpers that make the interface say less: the one-line
//! "Right now" brief, and short names for long ticket titles and stories.
//!
//! Plain HTTP to the Messages API (there is no Rust SDK), on Claude Haiku:
//! these are short, frequent calls where speed matters more than depth.
//! Without an Anthropic key the same prompts go through `claude -p`, on the
//! user's Claude Code login (`CLAUDE_CODE` in place of the key).

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::story::{Card, Stage};

const URL: &str = "https://api.anthropic.com/v1/messages";
/// The key that means "no Anthropic key: ask the local Claude Code".
pub const CLAUDE_CODE: &str = "claude-code";

/// Whether the user typed `claude-code` for the key: drop the saved key
/// and use Claude Code's login again.
pub fn means_claude_code(typed: &str) -> bool {
    typed
        .trim()
        .trim_matches('"')
        .eq_ignore_ascii_case(CLAUDE_CODE)
}
const MODEL: &str = "claude-haiku-4-5";
/// For the diff walkthrough, which the user asks for: reading code for a
/// reviewer needs more judgment than naming titles.
const REVIEW_MODEL: &str = "claude-sonnet-5";
/// Sonnet thinks before it answers (adaptive, on by default), and the
/// thinking counts against `max_tokens`: its calls get room for both.
const REVIEW_TOKENS: u32 = 16000;

const GUIDE: &str = "You walk a busy reviewer through a pull request. First understand \
the ticket: what the product or its users need, and why. Then read the diff and explain how \
it delivers that. Every diff line starts with its number; cite lines by those numbers. Reply \
in exactly this form, plain text, nothing else:\n\
SUMMARY<TAB>one sentence, at most 25 words: what changes for the product or its users\n\
WHY<TAB>at most 45 words: the problem the ticket describes, and how this change solves it\n\
RISK<TAB>low, medium or high<TAB>at most 35 words: what this could break outside what the \
ticket asks (other callers of what changed, other services, stored data, migrations, \
config, other teams' features), and whether a test in the diff or the repo covers that\n\
ASK<TAB>at most 35 words: whether the ticket asks the right thing: the need behind it, and \
whether a smaller answer (documentation, a setting, an existing feature, no change) would \
meet it; a single - when the ask is sound\n\
STEP<TAB>short title<TAB>the lines it is about, as path:from-to, comma-separated<TAB>at most \
50 words: first what it means for the product or its users and why the ticket needs it, then \
how these lines do it\n\
THINK<TAB>path:line<TAB>the exact characters on that line it is about, copied verbatim<TAB>at \
most 35 words: a concrete question or risk the reviewer should think about<TAB>the review \
comment the reviewer could post on that line as it is, at most 40 words, a friendly direct \
question to the author, no greeting, no names<TAB>2 or 3 ways the author could resolve it, \
each at most 12 words, separated by ' | '\n\
When the reviewer reviewed this PR before, start with one RESOLVED line per earlier \
comment: RESOLVED<TAB>the comment, at most 8 words<TAB>yes, partly or no<TAB>how the new \
commits handle it, at most 25 words. Then put the STEP lines on what changed since that \
review, citing lines of the numbered diff, and THINK only on what is new or still open.\n\
Give 2 to 6 STEP lines in reading order: the core change first, then its callers, then tests \
and config. After each STEP, 1 to 3 THINK lines that belong to it: behaviour changes, missing \
cases, error paths, security, migrations, compatibility, whether it really solves the \
ticket. When the ticket's wrong value is computed or stored elsewhere (a backend, another \
service or repo) and this diff only changes what is shown, which field is read, or how two \
numbers are aligned, the first THINK says it hides the symptom and names where the value \
should be fixed. Point at the few lines that matter, not whole files or hunks. No THINK for a mechanical step. Use the exact paths from the diff.";

const TEST_PLAN: &str = "Write the shortest test plan that shows a merged change works, \
for someone who wants to close the ticket in one minute. One check that proves the change; \
a second only when the change has two separate places (for example create and rename). \
No setup, no reproducing the old bug, no edge cases, no regression checks, no restating \
the ticket. Decide where it is checked: in the product UI, or in a terminal (backend, API, \
CLI, infrastructure). Reply in exactly this form, nothing else:\n\
SUMMARY<TAB>what to verify, at most 6 words\n\
KIND<TAB>ui or terminal\n\
URL<TAB>for ui, the path of the page to test on the web app{APP}, for example \
/settings; - when you cannot tell\n\
STEP<TAB>title, at most 3 words<TAB>what to do, at most 8 words<TAB>one shell command on \
one line, or - when the step has none<TAB>what they should see, at most 8 words\n\
Give 1 or 2 STEP lines. For ui, name the screen and the button or field as the diff names \
them. For terminal, give one runnable command per step (curl, kubectl, the repo's CLI, \
pytest) with placeholders like <TOKEN> or <WORKSPACE> for secrets and names you do not \
know; never invent real values or hosts.";

/// Lines of one file in the diff, by their number in the numbered diff
/// (`github::numbered`: 1 is the file's first line, hunk headers count).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub path: String,
    pub from: usize,
    pub to: usize,
}

/// Something to think about, pinned to the characters it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Think {
    pub at: Span,
    pub snippet: String,
    pub note: String,
    /// What to post on the PR to ask it; the note when Claude gave none.
    pub comment: String,
    /// Ways the author could resolve it, each postable as a suggestion.
    pub options: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub title: String,
    /// The files the step covers, in the order the spans name them.
    pub files: Vec<String>,
    pub spans: Vec<Span>,
    pub check: String,
    pub think: Vec<Think>,
}

/// Whether the new commits handle one comment of an earlier review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub comment: String,
    /// `yes`, `partly` or `no`.
    pub verdict: String,
    pub how: String,
}

#[derive(Clone, Debug)]
pub struct Guide {
    pub summary: String,
    pub why: String,
    pub steps: Vec<Step>,
    /// Earlier comments and whether they were handled, on a re-review.
    pub resolved: Vec<Resolved>,
    /// The head of the earlier review this one follows up on.
    pub since: Option<String>,
    /// `low`, `medium` or `high`, and what the change could break outside
    /// the ticket: what the reviewer reads instead of every line.
    pub risk: Option<(String, String)>,
    /// Whether the ticket asked the right thing; none when the ask is sound.
    pub challenge: Option<String>,
}

const BRIEF: &str = "You look at a Linear board for someone who supervises {BOT}, a bot \
that plans and builds tickets and opens PRs; they review PRs, decide blocked tickets and \
verify merged work. Reply with exactly one line of at most 60 characters: the single most \
useful next action, starting with the ticket id and a colon, for example \
'ABC-12: review api#345, waiting 3d'. Nothing else.";

const SHORTEN: &str = "Shorten each line's text to a name of at most 5 words that still \
says what it is about. Drop ticket ids, bracketed prefixes like [Prod alert], and \
filler. Reply with one line per input, in the form 'key<TAB>short name', same keys, same \
order, nothing else. Copy each key exactly, prefix and all (tab:…, win:…, story:…).";

const CUSTOMERS: &str = "For each line (a support or engineering ticket), name the customer \
organisations it is for: the companies whose users are affected or who asked for it. Give \
the company's plain name, not a workspace, namespace or org id: 'acme-prod-eu' is 'Acme'. \
Spell the same company the same way on every line. {US}, its staff, internal services, and \
people named without a company are not customers.{HINTS} Reply with one line per input, \
'key<TAB>Company A, Company B' or 'key<TAB>none', same keys, nothing else.";

/// A prompt with the profile's names in: the bot, the company, the
/// customer hints, the test app.
fn filled(prompt: &str) -> String {
    let profile = crate::profile::get();
    let us = match crate::story::our_company() {
        name if name.is_empty() => "The user's own company".to_owned(),
        name => name,
    };
    let mut hints = String::new();
    if !profile.company.internal_prefixes.is_empty() {
        hints.push_str(&format!(
            " Names starting {} are internal, not customers.",
            profile.company.internal_prefixes.join(", ")
        ));
    }
    if !profile.customers.trim().is_empty() {
        hints.push(' ');
        hints.push_str(profile.customers.trim());
    }
    let app = crate::store::load_settings().test_app_url;
    let app = match app.trim() {
        "" => String::new(),
        url => format!(", which runs at {url}"),
    };
    prompt
        .replace("{BOT}", &crate::story::bot())
        .replace("{US}", &us)
        .replace("{HINTS}", &hints)
        .replace("{APP}", &app)
}

const DIGEST: &str = "Each input is a customer or a story with its tickets (id | stage | \
what it needs from the lead | name). For each key, say what is going on in at most 12 \
words, for a busy lead: what shipped, what is stuck, what waits on them. Plain words, no \
ticket ids unless one needs action, no filler. Reply 'key<TAB>line', same keys, nothing else.";

/// What the brief depends on. The same fingerprint means the same brief.
pub fn fingerprint(cards: &[Card]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut rows: Vec<(&str, String, String)> = cards
        .iter()
        .filter(|c| c.stage != Stage::Done)
        .map(|c| {
            (
                c.issue.identifier.as_str(),
                format!("{:?}", c.stage),
                format!("{:?}", c.ask),
            )
        })
        .collect();
    rows.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rows.hash(&mut hasher);
    hasher.finish()
}

/// The board as Claude reads it: one line per open ticket.
pub fn board(cards: &[Card]) -> String {
    cards
        .iter()
        .filter(|c| c.stage != Stage::Done)
        .map(|c| {
            format!(
                "{} | {} | {} | {} | {}",
                c.issue.identifier,
                c.stage.label(),
                c.ask.map_or("nothing for you", |a| a.label()),
                c.why,
                c.issue.title,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The one line: what to do next.
pub async fn write(http: &reqwest::Client, key: &str, board: &str) -> Result<String> {
    let text = ask(http, key, &filled(BRIEF), board, 200).await?;
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("Claude wrote an empty brief"))
}

/// Short names for `(key, text)` pairs; keys Claude skipped are left out.
pub async fn shorten(
    http: &reqwest::Client,
    key: &str,
    items: &[(String, String)],
) -> Result<Vec<(String, String)>> {
    let text = ask(http, key, SHORTEN, &rows(items), 2000).await?;
    Ok(parse_short(&text, items))
}

/// The customers each `(key, ticket text)` is for; `none` becomes empty.
pub async fn customers(
    http: &reqwest::Client,
    key: &str,
    items: &[(String, String)],
) -> Result<Vec<(String, Vec<String>)>> {
    let text = ask(http, key, &filled(CUSTOMERS), &rows(items), 2000).await?;
    Ok(parse_short(&text, items)
        .into_iter()
        .map(|(k, names)| {
            let names = names
                .split(',')
                .map(str::trim)
                .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("none"))
                .map(str::to_owned)
                .collect();
            (k, names)
        })
        .collect())
}

/// One line on what is going on, per customer or story.
pub async fn digests(
    http: &reqwest::Client,
    key: &str,
    items: &[(String, String)],
) -> Result<Vec<(String, String)>> {
    let text = ask(http, key, DIGEST, &rows(items), 2000).await?;
    Ok(parse_short(&text, items))
}

fn rows(items: &[(String, String)]) -> String {
    items
        .iter()
        .map(|(k, text)| format!("{k}\t{}", text.replace('\n', " / ")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `key<TAB>answer` lines for the asked keys. Claude sometimes drops a
/// key's prefix (`tab:`, `win:`, `story:parent:`): a key the reply names
/// by its tail still counts.
fn parse_short(text: &str, items: &[(String, String)]) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(k, short)| {
            let (k, short) = (k.trim(), short.trim());
            let key = items
                .iter()
                .map(|(key, _)| key)
                .find(|key| *key == k)
                .or_else(|| {
                    items
                        .iter()
                        .map(|(key, _)| key)
                        .find(|key| key.ends_with(&format!(":{k}")))
                })?;
            (!short.is_empty()).then(|| (key.clone(), short.to_owned()))
        })
        .collect()
}

/// A reading order for a diff: summary, then steps with files and what
/// to check. One call, made only when the user asks.
pub async fn guide(http: &reqwest::Client, key: &str, context: &str, diff: &str) -> Result<Guide> {
    let user = format!("{context}\n\n{diff}");
    let text = ask_with(http, key, REVIEW_MODEL, GUIDE, &user, REVIEW_TOKENS).await?;
    let guide = parse_guide(&text);
    if guide.steps.is_empty() {
        bail!("Claude did not return a walkthrough");
    }
    Ok(guide)
}

const ASK_PR: &str = "You help a reviewer understand a pull request. You get the ticket, \
the diff with line numbers, the step of the walkthrough they are on, and the questions they \
asked before with your answers. Answer in at most three short sentences, one paragraph, the \
answer first (yes, no, or which), then only the one reason that matters. Plain words, no \
headings, no lists, no dashes as punctuation. Name file:line only when it helps. When the diff \
does not show enough, say in one sentence what to check; never guess, never invent code.";

/// The reviewer's own question about a PR, answered from its diff: a few
/// sentences in the Diff tab, never posted anywhere.
pub async fn ask_pr(
    http: &reqwest::Client,
    key: &str,
    context: &str,
    diff: &str,
    question: &str,
) -> Result<String> {
    let user = format!("{context}\n\n{diff}\n\nQuestion: {question}");
    let text = ask_with(http, key, REVIEW_MODEL, ASK_PR, &user, REVIEW_TOKENS).await?;
    match text.trim() {
        "" => bail!("Claude did not answer"),
        answer => Ok(answer.to_owned()),
    }
}

const APPROVE_NOTE: &str = "A reviewer is about to approve a pull request. You get the \
questions they asked about it and the answers they got. Write the comment that goes with their \
approval, in their voice: one short sentence, at most 20 words, saying it makes sense and what \
they checked, e.g. \"Makes sense, I also checked that only dl_meta fields get keyword, so the \
rest of the index is unaffected.\" First person, casual, no greeting, no names, no dashes as \
punctuation. Reply with the sentence only.";

/// What the reviewer asked and learned, as one short sentence for their
/// approval. Haiku: it only condenses what is already written.
pub async fn approve_note(
    http: &reqwest::Client,
    key: &str,
    asked: &[(String, String)],
) -> Result<String> {
    let user: String = asked
        .iter()
        .map(|(q, a)| format!("They asked: {q}\nThe answer: {a}\n\n"))
        .collect();
    let text = ask(http, key, APPROVE_NOTE, &user, 300).await?;
    let text = text.trim().trim_matches('"').trim();
    match text {
        "" => bail!("Claude wrote no note"),
        note => Ok(note.to_owned()),
    }
}

/// A test plan for a ticket in Testing, from the ticket, its plan and the
/// merged diff. Sonnet, like the diff guide: the user acts on it.
pub async fn test_plan(
    http: &reqwest::Client,
    key: &str,
    context: &str,
    diff: &str,
) -> Result<crate::testplan::TestPlan> {
    let user = format!("{context}\n\n{diff}");
    let text = ask_with(
        http,
        key,
        REVIEW_MODEL,
        &filled(TEST_PLAN),
        &user,
        REVIEW_TOKENS,
    )
    .await?;
    crate::testplan::from_reply(&text).ok_or_else(|| anyhow!("Claude did not return a test plan"))
}

/// A quick note read as a ticket: kind, title, squad, area, priority, and
/// what to ask first (src/capture.rs). Haiku.
pub async fn intake(
    http: &reqwest::Client,
    key: &str,
    note: &str,
    teams: &str,
) -> Result<crate::capture::Intake> {
    let prompt = crate::capture::intake_prompt(teams);
    let text = ask_with(http, key, MODEL, &prompt, note, 600).await?;
    crate::capture::from_reply(&text)
        .ok_or_else(|| anyhow!("Claude could not read a ticket from that"))
}

/// Which of the found tickets are the same report, with what they share
/// (src/capture.rs). Haiku.
pub async fn duplicates(
    http: &reqwest::Client,
    key: &str,
    note: &str,
    candidates: &[crate::capture::Duplicate],
) -> Result<Vec<crate::capture::Duplicate>> {
    let input = crate::capture::dedupe_input(note, candidates);
    let text = ask_with(http, key, MODEL, crate::capture::DEDUPE, &input, 400).await?;
    Ok(crate::capture::from_dedupe_reply(&text, candidates))
}

/// The ticket's description in the triage template (src/capture.rs). Haiku.
pub async fn ticket_description(http: &reqwest::Client, key: &str, input: &str) -> Result<String> {
    let text = ask_with(http, key, MODEL, crate::capture::DRAFT, input, 1500).await?;
    let text = text.trim();
    if text.is_empty() {
        bail!("Claude wrote no description");
    }
    Ok(text.to_owned())
}

/// A reply line's tab-separated cells, forgiving how a model bends the
/// `LABEL<TAB>value` form: `KIND: bug`, `**KIND**: bug`, `- KIND bug`, a
/// literal `<TAB>`. Every line-per-field parser reads through this.
pub fn cells(line: &str) -> Vec<String> {
    let line = line.replace("<TAB>", "\t");
    let line = line.trim().trim_start_matches(['-', '*', ' ']);
    let clean = |c: &str| c.trim().trim_matches(['*', '`', ':']).trim().to_owned();
    let mut cells: Vec<String> = line.split('\t').map(clean).collect();
    // No tab: the label is the leading capital word, the rest its value.
    if cells.len() == 1 {
        let first = std::mem::take(&mut cells[0]);
        let end = first
            .find(|c: char| !(c.is_ascii_uppercase() || c == '_'))
            .unwrap_or(first.len());
        let rest = &first[end..];
        if end > 1 && !rest.starts_with(char::is_alphabetic) && !rest.trim().is_empty() {
            return vec![first[..end].to_owned(), clean(rest)];
        }
        cells[0] = first;
    }
    cells
}

pub(crate) fn parse_guide(text: &str) -> Guide {
    let mut guide = Guide {
        summary: String::new(),
        why: String::new(),
        steps: Vec::new(),
        resolved: Vec::new(),
        since: None,
        risk: None,
        challenge: None,
    };
    for line in text.lines() {
        let cells = cells(line);
        let parts: Vec<&str> = cells.iter().map(String::as_str).collect();
        match parts.as_slice() {
            ["SUMMARY", summary, ..] => guide.summary = (*summary).to_owned(),
            ["WHY", why, ..] => guide.why = (*why).to_owned(),
            ["RISK", level, what, ..] => {
                guide.risk = Some((level.trim().to_lowercase(), (*what).to_owned()));
            }
            ["ASK", ask, ..] if ask.trim() != "-" && !ask.trim().is_empty() => {
                guide.challenge = Some((*ask).to_owned());
            }
            ["RESOLVED", comment, verdict, how, ..] => guide.resolved.push(Resolved {
                comment: (*comment).to_owned(),
                verdict: verdict.to_lowercase(),
                how: (*how).to_owned(),
            }),
            ["STEP", title, refs, check, ..] => {
                // `a.py:5-9, 20-24`: a bare range is in the file before it.
                let mut spans: Vec<Span> = Vec::new();
                for part in refs.split(',') {
                    let part = match spans.last() {
                        Some(last) if part.trim().starts_with(|c: char| c.is_ascii_digit()) => {
                            format!("{}:{}", last.path, part.trim())
                        }
                        _ => part.to_owned(),
                    };
                    spans.extend(span(&part));
                }
                let mut files: Vec<String> = Vec::new();
                for s in &spans {
                    if !files.contains(&s.path) {
                        files.push(s.path.clone());
                    }
                }
                guide.steps.push(Step {
                    title: (*title).to_owned(),
                    files,
                    spans,
                    check: (*check).to_owned(),
                    think: Vec::new(),
                });
            }
            ["THINK", at, snippet, note, rest @ ..] => {
                if let (Some(step), Some(at)) = (guide.steps.last_mut(), span(at)) {
                    let comment = rest.first().filter(|c| !c.is_empty()).unwrap_or(note);
                    step.think.push(Think {
                        at,
                        snippet: (*snippet).to_owned(),
                        note: (*note).to_owned(),
                        comment: (*comment).to_owned(),
                        options: rest.get(1).map_or(Vec::new(), |o| {
                            o.split('|')
                                .map(|o| o.trim().to_owned())
                                .filter(|o| !o.is_empty())
                                .collect()
                        }),
                    });
                }
            }
            _ => {}
        }
    }
    guide
}

/// `path:12-18`, `path:L12`, or a bare path (the whole file).
fn span(text: &str) -> Option<Span> {
    let text = text.trim();
    let number = |n: &str| n.trim().trim_start_matches('L').parse::<usize>().ok();
    let lines = text.rsplit_once(':').and_then(|(path, lines)| {
        let (from, to) = lines.split_once('-').unwrap_or((lines, lines));
        Some((path, number(from)?, number(to)?))
    });
    match lines {
        Some((path, from, to)) => Some(Span {
            path: path.trim().to_owned(),
            from: from.min(to),
            to: from.max(to),
        }),
        None if !text.is_empty() => Some(Span {
            path: text.to_owned(),
            from: 1,
            to: usize::MAX,
        }),
        None => None,
    }
}

async fn ask(
    http: &reqwest::Client,
    key: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String> {
    ask_with(http, key, MODEL, system, user, max_tokens).await
}

async fn ask_with(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String> {
    if key == CLAUDE_CODE {
        return ask_claude_code(model, system, user).await;
    }
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [{ "role": "user", "content": user }],
    });
    let response = http
        .post(URL)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|error| anyhow!("network error: {}", error.without_url()))?;
    let status = response.status();
    let reply: Value = response
        .json()
        .await
        .map_err(|_| anyhow!("Anthropic answered {status} without a readable body"))?;
    if !status.is_success() {
        let message = reply["error"]["message"]
            .as_str()
            .unwrap_or("request failed");
        bail!("Anthropic answered {status}: {message}");
    }
    if reply["stop_reason"] == "refusal" {
        bail!("Claude declined");
    }
    let text = reply_text(&reply);
    // Cut off before it said anything: thinking took the room.
    if reply["stop_reason"] == "max_tokens" && text.trim().is_empty() {
        bail!("Claude ran out of room before answering; try again");
    }
    Ok(text)
}

/// One prompt through `claude -p`, the user on stdin (a diff is too long
/// for an argument).
async fn ask_claude_code(model: &str, system: &str, user: &str) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let mut command = tokio::process::Command::from(crate::claude::print(model, system));
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| anyhow!("could not start Claude Code: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(user.as_bytes()).await?;
    }
    let output = child.wait_with_output().await?;
    let reply: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow!("Claude Code failed: {}", error.trim())
    })?;
    cli_text(&reply)
}

/// The answer in `claude -p --output-format json`'s result.
fn cli_text(reply: &Value) -> Result<String> {
    let text = reply["result"].as_str().unwrap_or_default();
    if reply["is_error"] == true {
        bail!("Claude Code failed: {text}");
    }
    Ok(text.to_owned())
}

/// The text blocks of a reply, without its thinking.
fn reply_text(reply: &Value) -> String {
    reply["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walkthrough_flags_a_display_patch_over_a_value_computed_elsewhere() {
        assert!(GUIDE.contains("hides the symptom"));
        assert!(GUIDE.contains("where the value"));
    }

    #[test]
    fn typing_claude_code_for_the_key_means_its_login() {
        for typed in ["claude-code", " Claude-Code\n", "\"claude-code\""] {
            assert!(means_claude_code(typed), "{typed}");
        }
        assert!(!means_claude_code("sk-ant-test-abcDEF123_xyz-000"));
        assert!(!means_claude_code(""));
    }

    #[test]
    fn a_claude_code_reply_is_its_result_or_its_error() {
        let ok = json!({"type": "result", "is_error": false, "result": "Login fails"});
        assert_eq!(cli_text(&ok).unwrap(), "Login fails");
        let failed = json!({"is_error": true, "result": "Not logged in"});
        assert!(
            cli_text(&failed)
                .unwrap_err()
                .to_string()
                .contains("Not logged in")
        );
    }

    #[test]
    fn a_reply_is_its_text_not_its_thinking() {
        let reply = json!({"stop_reason": "end_turn", "content": [
            {"type": "thinking", "thinking": ""},
            {"type": "text", "text": "FIRST\tDEV-1\twhy"},
        ]});
        assert_eq!(reply_text(&reply), "FIRST\tDEV-1\twhy");
    }

    /// The walkthrough of a real PR, with its body as the ticket.
    /// `GUIDE_PR=<pr url> GUIDE_KEY=<key> cargo test live_guide -- --ignored --nocapture`,
    /// with `GUIDE_SINCE=<commit>` as a re-review after a review at that commit.
    #[tokio::test]
    #[ignore]
    async fn live_guide() {
        let url = std::env::var("GUIDE_PR").expect("set GUIDE_PR");
        let key = std::env::var("GUIDE_KEY").expect("set GUIDE_KEY");
        let diff = crate::claude::gh(&["pr", "diff", &url]).unwrap();
        let mut body = crate::claude::gh(&["pr", "view", &url, "--json", "title,body"]).unwrap();
        if let Ok(base) = std::env::var("GUIDE_SINCE") {
            let head = crate::claude::gh(&[
                "pr",
                "view",
                &url,
                "--json",
                "headRefOid",
                "-q",
                ".headRefOid",
            ])
            .unwrap();
            let parts: Vec<&str> = url.trim_end_matches('/').rsplit('/').collect();
            let since = crate::backend::Since {
                owner: parts[3].into(),
                repo: parts[2].into(),
                number: parts[0].parse().unwrap(),
                token: None,
                base,
                head: head.trim().into(),
                comments: Vec::new(),
            };
            body.push_str(&since.context(&crate::http()).await);
        }
        let files = crate::github::parse(&diff);
        let diff = crate::github::numbered(&files, 60_000);
        let guide = guide(&crate::http(), &key, &body, &diff).await.unwrap();
        println!("{guide:#?}");
        // Where each comment would land on GitHub, to check by hand.
        for t in guide.steps.iter().flat_map(|s| &s.think) {
            let file = files.iter().find(|f| f.path.ends_with(&t.at.path));
            let line = file.and_then(|f| crate::github::line_at(f, t.at.from));
            println!("GITHUB {}:{} -> {line:?}", t.at.path, t.at.from);
        }
    }

    #[test]
    fn guide_says_what_could_break_and_whether_the_ask_was_right() {
        let guide = parse_guide(
            "SUMMARY\tx\nRISK\tMedium\tOther callers of parse() get empty dates now; no test covers them.\nASK\t-",
        );
        assert_eq!(guide.risk.as_ref().unwrap().0, "medium");
        assert!(guide.risk.unwrap().1.contains("callers"));
        assert_eq!(guide.challenge, None, "a sound ask says nothing");
        let guide = parse_guide(
            "ASK\tThe need is CSV export; the API already has it, a doc link would do.",
        );
        assert!(guide.challenge.unwrap().contains("doc link"));
    }

    #[test]
    fn guide_reply_becomes_summary_and_steps() {
        let reply = "Here you go\nSUMMARY\tMakes model search match names.\nWHY\tUsers could not find models.\nRESOLVED\tNon-ASCII names\tPartly\tLowercases, no normalization.\nSTEP\tFilter logic\tsrc/a.tsx:12-18, src/b.tsx:L4, 30-31\tCheck case-insensitive match.\nTHINK\tsrc/a.tsx:14\t`toLowerCase()`\tWhat about non-ASCII names?\tDoes this match `Ölmodell`?\tUse localeCompare | Normalize first\nTHINK\tsrc/a.tsx:15\tx\tOld form only.\nSTEP\tTests\tsrc/a.cy.tsx\tlooks mechanical\nSTEP\tbroken";
        let guide = parse_guide(reply);
        assert_eq!(guide.summary, "Makes model search match names.");
        assert_eq!(guide.why, "Users could not find models.");
        assert_eq!(guide.resolved[0].verdict, "partly");
        assert_eq!(guide.steps.len(), 2);
        let first = &guide.steps[0];
        assert_eq!(first.files, vec!["src/a.tsx", "src/b.tsx"]);
        assert_eq!((first.spans[1].from, first.spans[1].to), (4, 4));
        assert_eq!(
            first.spans[2].path, "src/b.tsx",
            "a bare range keeps the path"
        );
        assert_eq!(first.think[0].snippet, "toLowerCase()");
        assert_eq!(first.think[0].at.from, 14);
        assert_eq!(first.think[0].comment, "Does this match `Ölmodell`?");
        assert_eq!(
            first.think[0].options,
            ["Use localeCompare", "Normalize first"]
        );
        assert_eq!(
            first.think[1].comment, "Old form only.",
            "no comment: the note"
        );
        assert_eq!(guide.steps[1].spans[0].to, usize::MAX);
        assert_eq!(guide.steps[1].check, "looks mechanical");
    }

    #[test]
    fn a_key_whose_prefix_claude_dropped_still_counts() {
        let items = vec![
            ("tab:a7".to_owned(), "x".to_owned()),
            ("win:df".to_owned(), "y".to_owned()),
        ];
        let got = parse_short("a7\tCopy settings\nwin:df\tReplica fix\nzz\tnope", &items);
        assert_eq!(
            got,
            [
                ("tab:a7".to_owned(), "Copy settings".to_owned()),
                ("win:df".to_owned(), "Replica fix".to_owned())
            ]
        );
    }

    #[test]
    fn short_names_keep_only_known_keys() {
        let items = vec![
            (
                "a1".to_owned(),
                "[Export error] Register unclassified errors".to_owned(),
            ),
            (
                "story:x".to_owned(),
                "Developer CLI to export reports".to_owned(),
            ),
        ];
        let reply =
            "a1\tClassify export errors\nstory:x\tReport export CLI\nzz\tInvented\nnot a row";
        assert_eq!(
            parse_short(reply, &items),
            vec![
                ("a1".to_owned(), "Classify export errors".to_owned()),
                ("story:x".to_owned(), "Report export CLI".to_owned()),
            ]
        );
    }
}
