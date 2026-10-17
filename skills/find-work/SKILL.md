---
name: find-work
description: "\"/find-work\" searches the user's teams' Backlog for the tickets that move this week's goals and priority docs, ranks the few worth pulling now, and moves the ones the user picks to Todo. Asks before every write."
---

# Find work in the backlog

The user clicked **Find work** in the app. The Backlog holds tickets someone already
looked at and parked. Your job: find the few that move this week's goals, show them
ranked, and pull the ones the user picks into Todo. Finish the reading in about five
minutes.

**Tools:** Linear MCP (`list_issues`, `get_issue`, `list_comments`, `list_documents`,
`get_document`, `list_cycles`, `list_projects`, `list_initiatives`, `save_issue`).
`gh` runs as the user (to check whether a ticket is already fixed). Keep Linear
`fields` tight.

## Hard rules

- Show the proposal, then ask with AskUserQuestion (multi-select: the tickets to pull,
  plus **Change something** and **Stop**). Write nothing before the answer. Apply
  exactly what was picked.
- The only writes: a ticket's state to its team's `Todo`, and its priority when the
  user changes it. Never create, cancel, assign, relabel or retitle a ticket, never
  comment.
- Never write text that addresses the team's bot (or anyone) by @-mention followed by
  a command word and `!`: posted from the user's account, it runs automation.
- **WIP cap:** say how many tickets each team already has in Todo + In Progress. Past
  three, warn under the proposal; the user decides.
- Every reason you give comes from something you read in this session: a goal, a
  priority doc line, a project, a customer on the ticket. Otherwise leave it out.
  Never invent people, tickets or facts.

## 1. Read the goals

The prompt carries `## Plan the day` (today, the user, their teams, where the weekly
priority documents are) and `## This week's goals`. Read the priority documents first;
goals the user stated are final. Note the projects and initiatives they name.

## 2. Read the backlog

```
list_issues(team: "<TEAM>", state: "Backlog", limit: 100,
            fields: ["id","title","priority","labels","project","createdAt","updatedAt","url"])
```

For each of the user's teams. Skip: tickets with an open PR or a live branch, tickets
the bot is working on (its `🤖 Dispatched …` comments), and security or alert-sweep
tickets (the KTLO lane; the app shows those elsewhere). Read a description or comments
only for candidates.

## 3. Rank

A ticket earns a place only if it moves a goal or a priority: it sits in a named
project, closes a gap a priority doc names, or unblocks one. Then, in order: a
customer waiting on it, its priority, how small it is. Age and recent activity say
nothing. A ticket that serves no goal stays in the Backlog however loud it looks.

Before proposing, check the top candidates are still real: `gh` search for a merged
fix, or a newer ticket that replaced it. If fixed, say so under **Already done?**
instead of proposing it.

## 4. Propose

At most five, best first, one line each:

```
1. ABC-123 · P2 · <title>  →  <goal or priority it moves>. <why now, ten words>
```

Then, only if there are any:
- **Goals with no backlog behind them**: a goal or priority no ticket serves (a gap
  for the user, not a ticket to file).
- **Already done?**: tickets that look fixed, with the PR.

Ask which to pull. Move the picked ones to their team's `Todo` and end with the list of
what moved.
