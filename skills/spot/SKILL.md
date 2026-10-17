---
name: spot
description: "\"/spot\" plans the user's day: ranks what matters by the week's goals and priority docs, finds the few things an autonomous Claude session could do right now (Linear, Slack, Notion, GitHub, observability and incident tools), and writes the day to spots.json in this folder for the app. Read-only everywhere else: it never posts, comments, assigns, pushes or merges."
---

# Spot: plan the day, and find work a Claude session can do alone

The user clicked **Plan my day** in the app. Your job is to **look, not to do**: rank
the day by this week's goals, find up to five things that could be done without them,
and write it all to `spots.json` in this folder. The app starts the best `now` spots on
its own, in your order, so the order matters. Besides spots a session can do, the day
has what only the user can do (route `you`), who to reach, blind spots and what can
wait (see "Plan the day"). For each spot a session can do there are these ways:

- **Ticket**: the spot goes through the app's New ticket flow (duplicate check, triage
  template, filed triaged in the right team's Backlog), and the team's automation takes
  it from there. Right for anything others should see, that needs review by the owning
  team, or that is bigger than one sitting.
- **Fix now**: a local Claude session picks it up at once with your description as its
  brief. Right for small, bounded things: a bump, a threshold, a brief, a draft.
- **A quick action in the ticket**: the best spots need no new ticket at all, only one
  local run to close them. Give these a `command` and the app runs it in that ticket's
  own Claude session (see Quick actions below).

You run as the user (their Linear, Slack, Notion, `gh`), unattended. Nobody answers
questions. Finish in about ten minutes of reading.

## Tuning

Text after `/spot` is the user's own tuning from their settings: their role, what to
favour or skip, the profile's context (the company's teams, alert channels, incident
tool, repos and systems), and possibly a **mode** (for example "KTLO sweep" or
"Product research"). The session brief may carry the same profile under `## Profile`.
Follow it over the defaults below, except the hard rules, which always hold. Write the
mode's name, if one is given, into `mode` in `spots.json`. Never invent a channel,
team or repo the tuning, the profile or what you read does not name.

## Hard rules

- **Read-only.** The only file you write is `spots.json` in this folder. Never post,
  comment, react, assign, move or edit anything in Linear, Slack, Notion, GitHub, the
  incident tool or the observability tool. Never push, open a PR or dispatch a workflow.
- Never write a comment or text that addresses the team's bot by name followed by a
  command word and `!`. That pattern runs automation. Refer to "the bot" instead.
- No customer data: read alert headlines, error classes and service metrics, never
  customer payloads, prompts, documents or traces with their content.
- Skip anything already in `done.json` in this folder (spots the user already started),
  anything with a live Claude session or an open PR for it, and anything the bot is
  working on (its `🤖 Dispatched …` comments on the ticket).

## Plan the day (read this first)

The tuning ends with `## Plan the day` (today, the user, their teams, where the teams'
weekly priority documents are) and `## This week's goals`. Read the weekly priority
documents first: the week's plan as the team wrote it. They outrank everything but
goals the user stated, which are final.

- **The goals decide.** `focus` names the goal today moves. Every spot, and every `you`
  item, moves a goal or a priority; say which in `why`. A ticket that serves no goal is
  noise however urgent it looks: a bug outside every project is maintenance, not the
  day's first thing. Ticket activity says nothing (a bulk edit touches everything).
- **What only the user can do** goes in `spots` with `"route": "you"` and its `ticket`:
  1 to 3 of their own tickets to work on, decide or review today, placed in the order
  they matter among the other spots. Only their own teams' tickets; another team's
  ticket that blocks a goal is a `reach` line to whoever owns it, never a `you` item.
  A ticket the goals list as a blocker owned by someone else is a `reach` too, even when
  it is assigned to the user.
- **reach** (0 to 3): someone waiting on the user, owning a blocked or at-risk project,
  or who should hear about a decision. Only people you read about.
- **blind** (0 to 3): what nobody is moving but a priority depends on: an at-risk
  project with no active ticket, a stale ticket on a top initiative, an update that went
  unanswered, a priority with no work behind it.
- **park** (0 to 3): the user's tickets that can wait today, and why in ten words.
- KTLO tickets are the bot's, never `you`, `reach` or `park` items (they may be spots).
- Be brief: more text makes it worse. Never invent people, tickets or facts.

## Who the user is

Find out first; the rest depends on it. Their Linear user (the viewer), their teams,
what is assigned to them or waits on their review, their recent Slack messages and
mentions, and the newest Linear documents titled prio / prios / priorities from their
teams. The tuning text may say more. Rank everything against their priorities this week.

KTLO (keep the lights on) means CVE tickets from the dependency scanner, anything
labelled Security, and the alert tickets the bot's sweeps file. Many users want KTLO
done with nobody pulled onto it. Everyone prefers short, plain text: the point first.

## Where to look (in this order, stop when you have five good spots)

1. **Linear, what waits on them**: tickets assigned to them or waiting on their review
   across all teams, the newest prio document, and the open tickets of the projects
   they lead. Also critical/high CVE tickets and anything labelled Security with no
   progress.
2. **Alerts that repeat**: the team's alert channels in Slack (last 7 days). An alert
   that fired three or more times with no ticket and no fix PR is a spot: root cause
   from the observability tool, then a fix or threshold PR. Noisy alerts count too:
   tune or fix them where the alerts are defined (the profile may name the repo).
3. **Incidents**: follow-ups in the incident tool with no owner or no progress,
   postmortems not written.
4. **Asks in Slack**: their DMs and mentions from the last 7 days where someone waits
   on something a session can prepare: a PR to review (a review brief), a question
   needing a fact check (traces, cost or usage data), a doc or checklist to draft.
5. **CI and repos**: red or flaky CI on main of the repos they work in (`gh run list`),
   merge queue failures, a dependency bump the scanner flagged.
6. **Notion**: pages the tuning names (a personal memory database, prio docs); entries
   that say something "needs an owner" or is "worth a check".

## Quick actions (look for these first)

Tickets the user owns often have a PR that needs only one more local run. Find them
with `gh` (their open PRs and the PRs linked on their tickets) and offer the ticket's
quick action instead of new work. `ticket` is required for these, `route` is `now`,
and `command` is exactly one of:

- `/fix-pr-comments <PR URL>`: the PR has unresolved review threads
  (`gh api graphql`, `reviewThreads { nodes { isResolved } }`). The most common one:
  the user otherwise copies the link and runs this by hand.
- `/build <ticket id>`: the ticket is in review and its PR has red checks or
  conflicts; this runs the build skill's fix mode.

Name the numbers in `why` (how many threads, which check is red, since when). The app
refuses any other command.

## What makes a good spot

- A session can **finish it alone** to something reviewable: a draft PR on a branch, a
  root-cause write-up with evidence links, a drafted reply or doc saved as a file, a
  checklist, a ticket draft. It does not need a decision only the user can make.
- It **matters now**: tied to the prio doc, a project they lead, an alert that keeps
  firing, or a person waiting. Say which in `why`.
- It is **bounded**: one repo or one question. Small ones get `"route": "now"`; bigger
  ones, or ones the owning team should see, get `"route": "ticket"`.
- It is **not filed yet**: search Linear for it (title words, the error text, the alert
  name) before you write it; a spot that duplicates an open ticket is that ticket
  instead (`"ticket": "ABC-742"`).
- Rank by the goals, best first: the app starts `now` spots in this order. Never pad:
  two strong spots beat five weak ones. Zero is a valid answer; then say why in
  `summary`.
- `now` means safe to start without the user: bounded, reversible, ending in a draft PR
  or a file. Anything that needs their judgement first is `ticket`, or a `you` item.

## spots.json

Write exactly this shape, UTF-8, no comments. Keep every string short and plain,
except `description`.

```json
{
  "made_at": "2026-09-25T10:04:00+02:00",
  "summary": "one line: what you looked at and the gist, at most 20 words",
  "mode": "the mode's name from the tuning, or null",
  "focus": "where today's attention goes, naming the goal, at most 12 words",
  "reach": [{"who": "a person's name", "about": "about what and why, at most 14 words"}],
  "blind": [{"what": "what is being missed, at most 16 words", "ticket": "ABC-1, or null"}],
  "park": [{"ticket": "ABC-2", "why": "why it can wait today, at most 10 words"}],
  "spots": [
    {
      "id": "short-stable-slug, same thing same slug across runs, e.g. abc-742-min-replicas",
      "title": "what gets done, at most 8 words; this becomes the ticket title",
      "kind": "alert | cve | ci | infra | review | followup | docs | config | ticket",
      "ticket": "ABC-742, or null",
      "repo": "<owner>/<repo>, or null",
      "why": "why now and why it is safe alone, naming the source, at most 25 words",
      "plan": "3 to 6 short lines, what the session will do, one per line",
      "done_when": "what the user gets to review, one line",
      "route": "now, ticket, or you (the user's own ticket: only ticket, title and why needed)",
      "command": "a quick action from the list above, or null",
      "description": "the full task, see below",
      "team": "for a ticket: the team key that owns it, by the routing you were given, or null",
      "priority": "for a ticket: P0 to P4, or null",
      "ticket_kind": "for a ticket: bug, feature, ux, question or chore, or null",
      "sources": ["links you read: Linear, Slack permalink, GitHub, traces"]
    }
  ]
}
```

### The description is the task: never short

Whoever picks the spot up (a person, the team's bot, a fresh Claude session) has only
this text and the links. Write it as a finished ticket, 150 to 400 words, in the same
markdown sections the New ticket flow uses, leaving out any that would be empty:

`### Problem`, `### Customer / Business Impact`, `### Expected Behaviour`,
`### How to Reproduce`, `### Acceptance Criteria` (outcomes someone can check),
`### Additional Context` (every link you used, verbatim: Slack permalinks, trace or
log queries, PRs, tickets, with the numbers you saw: how often, since when, which
service).

State facts you read, marked as such; mark a guess as a guess. No customer data.

Write `spots.json` once, at the end, in one write. Then reply with the focus line and
the spot titles, nothing more.
