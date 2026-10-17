---
name: ship
description: "\"/ship\" goes through the tickets in the prompt (the KTLO lane, or the bot's PRs waiting on the user's review): ships the fixes that only need a review and a merge, moves the ones that need a person to the team that owns them. Asks before every approval, merge, move or comment."
---

# Ship what is done

The user clicked **Clear KTLO** or **Ship the bot's PRs** in the app. The prompt lists
the tickets with their stage and PRs. Goal: nothing waits on nobody. The bot writes a
fix in minutes and it then waits days for a review; a fix that works gets approved and
merged today, and a ticket the bot cannot finish goes to the team that can.

**Tools:** Linear MCP (`get_issue`, `list_comments`, `save_issue`, `save_comment`,
`list_teams`). `gh` runs as the user. Observability and incident tools only if this
session has them; if absent, say so and continue without them.

## Hard rules

- Show the proposal, then ask with AskUserQuestion (multi-select: which to ship,
  which to move, plus **Change something** and **Stop**). Write, approve or merge
  nothing before the answer. Do exactly what was picked.
- **Approve and merge only what the user picked, at the head they saw.** Note each
  PR's head commit when you show it. Right before acting, re-read it:
  `gh pr view <url> --json headRefOid,isDraft,state,reviewDecision,mergeStateStatus,author`.
  If the head moved, it is a draft, or it is not open: skip it and say why.
- Approve only when a review is still missing and the PR is not the user's own:
  `gh api repos/<o>/<r>/pulls/<n>/reviews -f event=APPROVE -f commit_id=<head>`.
- Merge only when `mergeStateStatus` is `CLEAN` after the approval and the PR is not
  already in the merge queue: `gh pr merge <url> --match-head-commit <head> --squash`;
  where the base branch merges through a queue, leave out `--squash` (the queue
  picks). Checks still running: `gh pr merge <url> --auto --match-head-commit <head>`
  instead, and say so. A silent `gh pr merge` exit proves nothing (on a queue it may
  only turn on auto-merge): read the PR's `mergeQueueEntry` and `autoMergeRequest` with
  `gh api graphql` and report queued, auto-merge on, or blocked and why. Anything else (failing checks, conflicts, changes requested):
  do not merge; that ticket is a **Move**.
- Never force-push, push, rebase or edit a PR. Never delete or create tickets.
- Never write text that addresses the team's bot (or anyone) by @-mention followed
  by a command word and `!`: posted from the user's account, it runs automation.
- Every fact you write comes from something you read in this session. Otherwise mark
  it *inferred* or leave it out.

## 1. Read each ticket

`get_issue` and `list_comments`; for a PR, `gh pr view` (checks, reviews, head) and
`gh pr diff`. The bot's `🤖` comments say what it found and did.

## 2. Sort each into one bucket

| Bucket | When | Do |
|---|---|---|
| **Ship** | Open PR, checks green or running, the diff fixes what the ticket names, small and contained, no behaviour change beyond the fix | Approve and merge (rules above) |
| **Move** | Needs a person: infra or config the bot cannot touch, a product or customer decision, a failing PR the bot already tried to fix, a change outside the repo, stuck in Triage or planning for a day with nobody on it | Move to the owning team (routing in the prompt, else `list_teams` descriptions; unclear: ask), keep its kind of state, one comment: what a person must do, in two lines |
| **Close** | Fix merged and the alert is quiet since (you checked it in the observability tool) | Done, one comment with the evidence |
| **Leave** | The bot is on it (`🤖 Dispatched …` with no PR yet, a plan waiting to be dispatched), or merged but you cannot check the alert | Nothing; say what it waits on |

A diff that fixes the cause where the value is produced ships; one that only changes how a
wrong value is shown (reads another field, recomputes, aligns two numbers) while the value is
computed wrong elsewhere is a symptom patch: a **Move**, naming the service or repo that
produces the value.

For each **Ship**, give your verdict in one line: what the diff changes and why it
fixes the alert, plus any risk you saw. A diff you would not approve yourself is a
**Move**, never a Ship.

## 3. Propose

```
Ship   ABC-12 · repo#34 @<short head> · <one-line verdict>
Move   ABC-13 → <TEAM> · <what a person must do>
Close  ABC-14 · <evidence>
Leave  ABC-15 · <what it waits on>
```

Ask, do what was picked, then end with what shipped, what moved where, and anything
that failed with its reason.
