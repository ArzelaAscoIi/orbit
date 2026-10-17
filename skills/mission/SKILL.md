---
name: mission
description: A mission is one parent ticket on top with its sub-tickets under it, driven through the team's bot until each has a PR. /mission <goal in words> researches, files the parent and sub-tickets, and starts it; /mission <PARENT> [n] starts one on a parent that already has sub-tickets; /mission status [PARENT] says where each stands; /mission stop <PARENT>.
---

# Mission

A mission is one thing, however it starts: a parent ticket on top, its
sub-tickets under it, and Orbit driving them through the team's bot (or
building them here) until each has a PR. The user starts one from a
parent ticket's Start mission row, from Cmd+K `Mission: <goal>`, or here.

Orbit's CLI is the full path your context names; `orbit` below means it.

## Which one

- **`/mission <PARENT> [n]`** (an identifier like `ABC-769`): the parent
  exists. Go to **Start it**.
- **`/mission status [PARENT]`** runs `orbit ctl missions` and gives the
  list from **Start it** for every mission (or only that parent's), with
  the last lines of its log and what the bot did last on each sub-ticket.
- **`/mission stop <PARENT>`** runs `orbit ctl mission stop <PARENT>`.
- **Anything else** is a goal in the user's words: **File it**, then
  **Start it**.

## File it

The user told their clone what they want, in their own words. They may
paste a Slack link, name a team and its state ("ABC triage"), and list
requirements. Keep their words; they are the spec. A "goal" or `/goal` in
them is the mission's outcome, never the weekly `goals` skill.

You file and hand over; you never implement. Every sub-ticket is built by
its own worker (the bot, or an Orbit session on that ticket); see
**Coordinate, never build**.

### Research, briefly

- Read every link they gave: the Slack thread through the Slack tools, a
  Linear ticket through the Linear tools.
- Find the code it touches: which repos, which modules, what exists
  already. Read only enough to split the work correctly.
- Look for an existing ticket or project on the same thing first. If one
  exists, use it as the parent instead of filing a new one.

### File the tickets

- **One parent** in the team and state the user named, or the team that owns
  the area, in Triage. It has three short parts:
  - **Why:** the problem in the user's terms.
  - **What:** their requirements as bullets.
  - **Current code:** the files and functions that matter, as paths.

  Put it in the project it belongs to, if one fits.
- **Sub-tickets** under it, one per change a single PR can make in one repo:
  - **Title:** `<Parent topic>: <the change> (<repo>)`, or
    `(<repo>/<folder>)` in a monorepo.
  - **Body:** what to change, the repo (and folder), the files, and how to
    know it works.

  The repo comes from the profile's repo map (the brief's `## Profile`),
  checked against the code. The map wins over an older plan or ticket that
  names another repo. At most 8 sub-tickets.
- **Order and blockers:** create them in build order. When one needs another
  merged first, add a Linear **blocks** relation from the first to the
  second. Orbit hands a sub-ticket to the bot only once its blockers are
  merged. Every PR goes against the default branch, never stacked on
  another sub-ticket's branch: the order lives in `blocks`.
- **Scope check:** before filing, ask the user once about any item they
  did not name that adds a new setting or touches a separate product surface. It may be its
  own feature, not a sub-ticket of this parent.
- **Feature flags, settings, admin-only, metrics:** when the user names
  these, each gets a sub-ticket or is an explicit line in one. Never drop a
  requirement.
- **Keep it concise.** No acceptance-criteria boilerplate, no restating the
  parent in every sub-ticket.
- **Never write `@<anyone> <Word>!`** anywhere: the bot runs that.

## Start it

Read the parent with `orbit ctl ticket <PARENT>`. If it has no
sub-tickets, say so and offer to file them (**File it**). If no sibling
has a `blocks` relation but the work clearly has an order (an API before
its UI), say so. On the user's word, add it with
`orbit ctl block <BLOCKER> <BLOCKED>`, or start with n = 1.

```sh
orbit ctl mission <PARENT> <n> [<MISSION-ID>]
```

n defaults to 2. A mission you filed passes the mission id from your
context, so the filing and the drive stay one mission.

Orbit then moves the parent's sub-tickets to the bot's team in Todo, which
is the bot's go. At most n are in flight, and each goes only once every
sibling that blocks it is merged. After each hand-off it calls the bot at
once (its poller workflow), so the bot's plan-build loop starts now. It
follows them until each has a PR, for days if it must. Orbit waits on the
bot and tells the user once for each thing that is theirs (a PR to review,
the bot's block, a merge to check), so you do not wait. A sub-ticket with
its PR open counts as implemented.

Then run `orbit ctl missions` and tell the user, in one short list:

- what goes now
- what waits, and on what
- what is already implemented

After filing, also send one line: `orbit ctl say "Filed <PARENT> with <n>
sub-tickets; mission started"`, and end with the parent's link and its
sub-tickets, one line each, in build order.

The session that starts the mission owns it: Orbit records it, and a
click on the mission in the clone panel opens this session. Stay the
place the user asks "where are we" and steers from: kick one, re-plan,
build one here, `follow`, stop. A mission nobody started from a session
gets a fresh one when the user clicks it, with the log as its context.

Linear writes the user asks for go through Orbit's own key, never the
Linear connector: `orbit ctl comment <ID> <text>`, `orbit ctl move <ID>
<state name>`, `orbit ctl parent <ID> <PARENT|none>`, `orbit ctl block
<BLOCKER> <BLOCKED>`. Each says `ok: true` only once Linear confirmed;
on `ok: false`, tell the user the `error`.

## Coordinate, never build

Work the board like a lead, not like one agent with a shell. "Build X",
"continue with this" and "your goal is that these land" all mean: keep
coordinating. Each sub-ticket gets its own worker:

- the bot, through the mission or the `AskBot` action;
- or, when the bot is busy, an Orbit session on that ticket:
  `orbit ctl do '{"RunSkill":{"issue_id":"<id>","command":"/build","submit":true}}'`.

One ticket, one worker, one PR, visible on the ticket's card. Never a
worktree, an edit or a commit in your own shell, not even for "the ones
the bot has not picked up". In one run the user said "continue, your goal
is that these land"; the clone stopped the mission and built three
sub-tickets itself, in the wrong repo, with nothing on the tickets. The
user could neither see nor steer it, and its PRs were closed and redone.

### Orbit sessions instead of the bot

When the user says to build here, not through the bot, you own the
sequencing; the user hands off the work, not its coordination. First run
`orbit ctl follow <PARENT>`: Orbit then lists each sub-ticket's step in
your panel (building, fixing comments, your review, merging, waiting on,
done) and hands nothing to the bot. Switching from the bot to here: run `follow`, never `mission stop` (that closes the list). That list is how the user sees where
things stand without asking you; keep each session's note current.

- A session that shows no movement after a command is stuck: never wait
  on it and never ask the user to check it. `Attach` and send the command
  again; still nothing within a minute → `FreshSession` and send it. Check
  with `orbit ctl ticket <ID>`: state `working`, a new note.

- One ticket at a time, in blocker order. Start the next only once its
  blocker has a PR (or is merged). Never start a blocked one early: in one
  run ABC-794 and ABC-795 got `/plan` while ABC-793, their blocker, had no
  plan, and each stopped to ask the user.
- Send `/build`, never a bare `/plan`: a build without a plan plans first
  and posts it without asking. Pick every default yourself.
- Before and after each start, read `orbit ctl sessions`: each row's `run`
  has `note` (its last step), `asks` (the question or approval it waits on),
  `pr` and `failed`. Answer what a default answers, fix what you can (a
  failed start: start it again), and ask the user only what no default can.
- A PR is done only when its gate is clean. After every build read the PR's
  gate (`orbit ctl ticket <ID>`: `openThreads`, `checks`, `branch`,
  `review`) and fix at once, without asking or reporting first: open
  threads → `RunSkill` with `/fix-pr-comments <PR url>` on the ticket's
  session (the card's Fix comments button); red checks → the same session
  fixes them; behind base → `UpdateBranch`. Only a clean PR goes to the
  user, and only then does the next ticket start. ABC-793's PR was reported
  "ready, review it" with 2 open comments; the user had to press the button.
- An AI review older than the PR's head is stale. Before saying "needs your
  approval", compare the last AI review's commit with the head; stale →
  `orbit ctl do '{"AiReview":{"url":"<PR>","head":"<head>"}}'` and wait for
  it. Once a PR sat on REVIEW_REQUIRED after a fix push because the
  AI review never re-ran, while the clone told the user it was theirs.
- Read the PR body's `### Left out` against the ticket's ask. Anything the
  user asked for that is missing goes back to the session, or to the user
  as one line if it needs their call.

### When the bot does not answer

Orbit kicks a mission's sub-ticket gone quiet (30 min), and builds it in an
Orbit session when the kick goes unanswered for another 30 min, telling
the user once. A ticket In Progress with no branch, no PR and no plan is
not "with the bot": check `orbit ctl ticket <ID>` before you wait on it.
When you suspect the bot is down sooner (several tickets moved but none
got a plan), look at its last workflow runs (`gh run list -R <bot repo>`):
a green run whose agent job was skipped is an outage. Say so once and
build in Orbit sessions.

Give the user short commands. A long path in a `!` command wraps in their
terminal and runs as two halves; when they clearly want it done, run it
yourself.

Then check in: `orbit ctl ticket <ID>`, `orbit ctl sessions`, the PR's CI.
Unblock with the missing fact, a kick of a stalled step, or an answer to a
session's question. Check-ins, repo decisions and blockers go as one short
Linear comment on the sub-ticket, on change only, so the user and the
worker read the same thing.

A "goal" or `/goal` in the user's words is the mission's outcome, never
the weekly `goals` skill.

## Merges and pauses

- Merging is the user's. Report each PR as it really is: read
  `gh api graphql` for its `mergeQueueEntry`, `autoMergeRequest`,
  `reviewDecision` and `mergeStateStatus`, and say queued, auto-merge on
  (waiting for checks), or blocked and why (no approval, conflict). A
  silent `gh pr merge` exit proves nothing: on a merge-queue repo it may
  only turn on auto-merge.
- The user's "pause", "wait" or "stop" beats an active `/goal`. You cannot
  clear it yourself: ask the user once to type `/goal clear`. Undo every pending merge: dequeue queued PRs
  (`dequeuePullRequest`) and `gh pr merge --disable-auto` on the rest.
  Never answer a Stop hook again and again.

## Links

Every ticket, PR and session you name gets a link the user can Cmd+click
in Orbit, on its own after the name:

- A ticket or PR: its full `https://linear.app/…` or `https://github.com/…`
  URL, from `orbit ctl ticket`, `tickets`, `prs` or `missions`.
- A session: its `link` from `orbit ctl sessions`, `orbit://session/<key>`.

Write them bare, never as `[text](url)`: Orbit's terminal only sees plain
URLs. For example: `ABC-206 has its PR: https://github.com/acme/app/pull/7001`
