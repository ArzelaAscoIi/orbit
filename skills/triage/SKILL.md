---
name: triage
description: "\"/triage ABC-150\" triages one ticket the way the team's bot does (bug vs feedback, dedupe, assessment, consolidate, route); \"/triage ABC\" grooms a whole team's Triage column: clusters, folds duplicates, accepts into Todo."
---

# Triage, locally

You triage Linear intake the way the team's bot triages it, but here, in the
user's own session, as the user. The user approves every write.

**Mode by argument:**
- Ticket id (`ABC-150` or a Linear issue URL) → **Single ticket** (A).
- Team key (`ABC`; no argument → ask which team) → **Batch grooming** (B).

**Tools:** Linear MCP (`get_issue`, `list_issues`, `list_comments`,
`save_issue`, `save_comment`, `list_projects`, `list_issue_labels`,
`list_customers`, `list_teams`). `gh` runs as the user (code search, PRs).
Observability (traces/logs), product analytics (PostHog) and Jam MCP: only if
this session has them; if absent, say so and continue without them. Keep
Linear `fields` tight.

**Team structure.** Routing (which team owns which feature area), the repos,
the layer labels and any agent lane come from the context the app gives: the
session brief may carry a `## Profile` section with the company's routing,
repos and conventions. Fill gaps from Linear team descriptions
(`list_teams`), existing labels (`list_issue_labels`) and repo READMEs. Never
invent a team, label or repo; unclear → a question under **Needs you**.

## Hard rules (both modes)

- Show the proposed writes, then ask with AskUserQuestion: **Apply**,
  **Change something** (take edits, re-show), **Stop**. Write nothing before
  the answer. Apply exactly what was approved; on a failed write, continue and
  list failures at the end.
- Never delete tickets. Never create tickets. Duplicate and Cancel only.
- Never guess a product, pricing, security or compliance decision: make it a
  question or list it under **Needs you**.
- Never change a title. Never overwrite human-set labels. Never drop
  screenshots, attachments, code blocks, logs or config from a description.
- Never touch a ticket the bot is mid-run on (In Progress with a
  `🤖 Dispatched Build` comment and no PR yet). Never approve or merge a PR.
- Keep the `🤖` comment prefixes below verbatim; the app parses them.
- Never write a comment that addresses the bot (or anyone) by @-mention
  followed by a command word and `!`: posted from the user's account, the
  bot's poller runs it as a command.
- Every fact you write (a customer, a count, an error, "alert is quiet") comes
  from something you read or ran in this session. Otherwise mark it *inferred*
  or leave it out.

## A. Single ticket

### A1. Read
`get_issue` with relations, attachments, customer needs; `list_comments`.
Jam link anywhere → if Jam MCP is present, read details, console logs,
network requests (4xx/5xx, stalled streams), user events first; derive repro,
customer org (from the app URL), impact and exact error text from it.
Do not ask for anything the recording answers.
Answers to an earlier `🤖 I need a bit more info…` are the comments after it.

### A2. Classify
**Bug** = 2+ signals: title says bug/broken/error/crash/not working/fails/
exception/500/404; description has repro, stack trace or expected-vs-actual;
reporter says regression/stopped working; clearly a defect, not a UX opinion.
Otherwise **product feedback**.

### A3. Dedupe
Search open tickets across teams (`list_issues` with `query` = 3 to 5 title
terms, all teams, not Done/Canceled). Show up to 3 candidates, each with the
shared evidence (same error text, component, page, or trace), not just a
similar title; no shared evidence → not a duplicate candidate. If the user
confirms a duplicate: propose `save_issue(id, state: "Duplicate",
duplicateOf: <original>)` plus one 2 to 4 sentence comment on the original
with the new reporter and any new evidence. Stop there. If only the reporter
can tell, propose this comment instead and stop:
```
🤖 Possible duplicate — I found a similar issue that may already cover this:

- **[IDENTIFIER]** · _Title_ · TEAM · STATE

Is this the same problem you're reporting?
```

### A4. Fill gaps
Needed before routing: feature area, problem, customer/business impact,
expected behaviour, repro, priority; the relevant config or snippet when the
ticket is about configuration (skip if stated "applies to any").
Derive first: expected from the error, repro from Jam or "I did X", customer
from Linear customer needs, the Jam org or "all customers" in the text.
Ask only for what is genuinely not derivable, as one comment:
```
🤖 I need a bit more info before I can route this
- [ ] <field>: <what to provide>
```
Follow-up after partial answers: `🤖 Thanks for the update!` + remaining
`[ ]` only. Asked and unanswered → leave it, report "waiting on reporter".

### A5. Assess (feedback only)
One product-area label + one type-of-feedback label (skip if a human set
them). Check signal: related issues across teams, Linear customer needs,
product analytics if present. Then one comment, exactly this, no intro/sign-off:
```
**Type:** [Feature request | UX issue | Integration | Workflow gap | Other]
**Area:** [product area]
**Customers:** [N mentions; name them if ≤ 3, else "N customers"]
**Data:** [relevant usage metric if found, or "no data found"]
**Related:** [issue identifiers, or "none"]
**Read:** [2 sentences. What the user actually wants and why it matters to them.]
**Brief:** [Will draft | Not enough signal yet]
```
`Will draft` only at 3+ unique customers or 2+ internal requests; put the
brief itself under **Needs you**, do not write it.

### A6. Consolidate (bugs, and feedback that is thin)
Rewrite the description as:
`### Problem` · `### Customer / Business Impact` (Linear customer needs first:
`[Customer]: "[quote]" · Linear customer request`) · `### Expected Behaviour`
· `### How to Reproduce` · `### Acceptance Criteria` (user-observable outcomes
only) · `### Additional Context` (attachments, image links, `🎥 Jam: <url>`).
Lean: symptom, who, correct outcome, minimal steps. Copy every code block,
config, stack trace and image/attachment link from description and comments
verbatim. Reporter RCA or proposed fix goes verbatim into
`<details><summary>Possible solution from the reporter</summary>…</details>`.
Omit empty sections, mark inferred bits *italic*, invent nothing. End with
`---` and `*Consolidated by the bot…*`.

### A7. Priority, labels, route
Priority: **P0** many customers blocked/outage · **P1** one blocked or 2+
intermittent · **P2** one degraded · **P3** non-blocking bug · **P4**
cosmetic. Not yet in production → cap at P2. Set the priority field and the
`P#` label if the team uses one; `Bug` label only for real bugs; the team's
layer labels (backend for API/5xx/DB, frontend for UI, or both) in the target
team's scope.

Team: route by feature area using the profile's routing and the Linear team
descriptions (see **Team structure**). Route by where the user meets the
problem: what a user sees inside one product area belongs to that area's team
even if the text names another component. Customer-hit product bugs route by
area even if the fix touches tooling. If the profile names an agent lane (a
team whose bounded single-repo changes the bot plans and builds): a trivial
copy/visual-only change (no backend, one component) or any bounded
single-repo change with no open product question goes there; Accept = Backlog
then Todo (WIP cap 3, as in B3), else Park in Backlog. Unclear → the
profile's default team if it names one; no match you can defend and no
default → **Needs you**.

Project: `list_projects` for the target team; attach only on an unambiguous
match; keep an existing project unless clearly wrong.

Routing comment, one or two lines, no recap:
```
🤖 Routed to **<SQUAD>** · **<Area>** · **P#**[ · **<Project>**]. <tail>
```
Tail: bug → `Bug filed, focus is on reproduction and root cause. Plan it
with /plan <ID>.`; feature → `The bot will continue with an implementation
plan.` P0/P1 → add `⚠️ Ping the incident channel.` (name it if the profile does).

### A8. Propose
One block: classification, duplicates, comments (full text), description
diff summary, priority/labels, target team/state/project. Then ask (Hard
rules). Apply in order: comments, description, priority/labels, move,
project, routing comment.

## B. Batch grooming

### B1. Read
```
list_issues(team: "<TEAM>", state: "Triage", limit: 100,
            fields: ["id","title","status","labels","priority","createdAt","url","description"])
```
Count the team's `Todo` + `In Progress` for the WIP cap. List open non-Triage
tickets with similar titles so intake folds into existing work. Read comments
only where title and description leave the cause unclear. Much intake comes
from the bot's alert sweeps (recognisable by their title prefix) and repeats:
work on the pattern, never ticket by ticket.

### B2. Cluster
Group by root cause: same exception text, failing component, the same
service crashing across environments, same page in a UX report. A similar
title alone never joins a cluster. Canonical = an existing non-Triage ticket
for the defect, else best evidence, then oldest.

### B3. Decide, per cluster
One action for the canonical, **Duplicate** for the rest:

| Action | When | Write |
|---|---|---|
| **Accept** | Real, actionable, one repo, plannable | `Backlog` with priority+labels, then `Todo` in the same pass |
| **Park** | Actionable but WIP full or low value now | `Backlog` with priority+labels |
| **Cancel** | Alert stopped firing, no lasting defect; or superseded by shipped work | `Canceled` + one-line comment why |
| **Ask** | Too thin, gap not fillable by you | One specific question comment, stays in Triage |
| **Leave** | Needs investigation you cannot do now | Nothing |

- **WIP cap: max 3 in Todo + In Progress.** Accept up to the cap, rest Park.
  Order: Urgent/High, then customer/partner labels, then oldest.
- Before Accept, the description names the repo, observed vs expected, and
  repro (A6 format); add what is missing in the same write.
- Before Cancel on an alert, confirm it is quiet in the observability tool.
  Cannot check → Leave, and say so.
- Multi-repo or open product question → Park and name the question.
- Duplicates: first fold their extra evidence (environment, trace id,
  customer) into one comment on the canonical, then
  `save_issue(id, state: "Duplicate", duplicateOf: <canonical>)`.

### B4. Propose
```
## Triage plan: ABC, 38 tickets → 9 clusters
1. Connection reset "peer closed" (11) → Accept ABC-120, 10 dupes
2. worker OOMKilled (9) → Park ABC-97 (WIP full), 8 dupes
3. [Prod] api OOMKill loop (1) → Cancel, quiet 6 days
Writes: 3 to Todo · 4 to Backlog · 2 canceled · 24 duplicates · 1 question
WIP after: 3/3
```
Ask: **Apply all**, **Apply duplicates only**, **Change something**, **Stop**.
Apply: canonical fixes and evidence comments, then duplicates, then moves.

## After anything lands in Todo
Tell the user they can plan it now with `/plan <id>` instead of waiting for
the bot's poller (say when it runs if the profile says so).

## Report
Short, max ~10 lines, full Linear URLs, no preamble, no method recap:
```
## Done
- ABC-150 → <team> · <area> · P2 (<issue url>)
- 3 to Todo: <urls> · next: /plan ABC-120
- 24 duplicates folded into 6 · 2 canceled · 4 parked
## Needs you
- ABC-97: product call, retry budget per customer or global?
## Left
14 in Triage, 4 clusters. Run /triage ABC again.
```
