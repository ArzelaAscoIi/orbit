---
name: orchestrate
description: "\"/orchestrate\" operates the team's board for the user in one unattended run: lands approved bot PRs, makes bot PRs reviewable, splits Blocked tickets, refills Todo, grooms Triage and repairs stuck tickets, one action at a time until nothing is left, then reports what is ready for the user. Never approves a PR and never writes bot trigger comments."
---

# Orchestrate: operate the board in one run

You stand in for the user as the **operator** of one team's board. The team's bot does
the planning and the coding. The user does exactly one thing: **review and approve pull
requests**. Everything between those two (moving tickets, splitting what is too big,
nudging stuck runs, fixing small review comments, getting approved PRs merged) is yours.

The user started this run from the app with one click. You run as them: their `gh`
login, their Linear account (the Linear MCP tools; call one once to see the exact names).
Nobody answers questions mid-run; anything that needs the user goes in the report. When
the brief says the user is at the screen, the run ends by asking them the report's
decisions (section 4b); an autopilot run never asks.

**Where the facts come from.** The session brief names the board (the team key, e.g.
`ABC`). The company's facts (the bot's Linear name and GitHub login, the GitHub org and
repos, state names, how to start the bot's flows without a comment, observability tools,
channels, ticket and PR URL shapes) come from the brief's `## Profile` section and the
**specifics (from the profile)** section at the end of this skill. The marker shapes
below are the same ones the generic `plan`, `build` and `fix-pr-comments` skills write.
Missing a fact → look it up (`list_teams`, `list_issue_statuses`, `list_users`,
`gh repo list <org>`, repo READMEs); never invent a team, state, repo or user.

Throughout, `<TEAM>` is the board's team key, `<bot>` the bot's GitHub login,
`<org>/<repo>` a repo.

## The run contract

One run works the board until there is nothing left to do:

1. Read the board (section 2).
2. Take the **first** action on the ladder (section 3) that applies. One action, done
   fully.
3. Read the part of the board that action changed, and go back to 2.
4. Stop when the ladder reaches **G. Nothing to do**, when only items that need the user
   are left, or after **8 actions**, whichever comes first. Then write the report
   (section 4).

Two limits keep a run safe: **at most one action per ticket per run** (the next run gets
the second one, after the bot has moved), and **never start the same flow for the same
ticket twice in one run**. Linear ticket state + the bot's `🤖 …` guard comments +
GitHub PR state **are** the memory. Do not write a scratch state file. Clone repos into
`repos/` under this session's folder.

## KTLO stays out of product work

KTLO (keep the lights on) is what the profile's KTLO rules name: tickets from security
scanners, anything with a security label, and the alert tickets the bot's sweeps file
(title prefixes the profile lists). The user wants no person pulled onto them; the app
shows them in their own KTLO view. You keep them apart on the board:

- **The bot owns every KTLO ticket.** One with no assignee, or with a person assigned by
  the flow that filed it (the scanner, a default lead), gets the bot as assignee and one
  plain comment saying why (security needs an owner; the bot takes KTLO so product work
  keeps its people). Leave it alone if a person has commented on it or moved it
  themselves; list it under **Needs you** instead.
- **KTLO has its own WIP cap: at most 2** KTLO tickets in Todo + In Progress, across all
  teams together. They never count against the product cap in 3D, and product never
  waits for KTLO.
- **Refill product first** (3D, 3F). Pull a KTLO ticket only into free KTLO slots,
  `Urgent`/`Critical` before `High` before the rest, oldest first.
- **Security tickets on every team, not just `<TEAM>`.** Scanners file onto whichever
  board owns the repo. Read them across all teams each run, with no `team` filter, once
  per scanner creator and once per security label the profile names:
  ```
  list_issues(creator: "<scanner>", limit: 100,
              fields: ["id","title","status","statusType","assignee","labels","team","url","updatedAt","priority"])
  list_issues(label: "<security label>", limit: 100, fields: [same])
  ```
  Keep the open ones (`statusType` not completed or canceled). They stay on their own
  team's board; never move a ticket to `<TEAM>`. The bot's automation may only drive
  `<TEAM>`, so outside it you drive the chain yourself, one step per ticket per run, and
  the guard comments are the memory:
  1. Not owned by the bot → assign the bot (the rule above).
  2. No `🤖 Here's my implementation plan` comment → start the bot's Plan flow for it
     (section 1, "Starting the bot's flows").
  3. Plan there, no `🤖 Dispatched Build` → start the bot's Build flow, then leave the
     `🤖 Dispatched Build — writing the code now…` marker. A plan that spans several
     repos is split like 3C, sub-issues on the same team.
  4. PR opened → treat it like any bot PR: 3A to land it once the user approved it, 3B
     to make it reviewable.
  5. PR merged → move the ticket to its team's completed state and comment the PR. A
     security fix is verified by its CI and the scanner's next scan, not by a Testing
     column.
  If the scanner opened a fix PR itself (linked, or matching the finding's package), use
  that PR instead of starting a Build.
- **Report KTLO on one line** under In flight, e.g.
  `KTLO: 2 in flight, 5 waiting, ABC-148 now owned by the bot`. A KTLO PR waiting on the
  user's review still goes under Ready for you, marked `KTLO`.

## 1. How the board works

The board has **no cycles** unless the specifics say so: every ticket sits in Backlog
until someone pulls it into Todo, and pulling it into Todo is the trigger for the whole
chain. The bot's poller sweeps the board on a schedule and drives it, no AI, no
judgement:

| # | Precondition on a `<TEAM>` ticket | Bot does | Ticket ends in | Guard comment |
|---|---|---|---|---|
| 1 | in **Todo**, no plan comment | Plan | In Progress | `🤖 Here's my implementation plan` |
| 2 | in **In Progress** + plan + not yet routed | route | In Progress *or* Blocked | `🤖 Assigned to …` / `🤖 Routed to **Blocked** — <reason>` |
| 3 | in **In Progress** + assigned to the bot + not building | Build | In Review | `🤖 Dispatched Build` then `🤖 PR opened → <url>` |

The bot's Build opens a **ready-for-review** (not draft) PR titled `<TEAM>-<n>: <summary>`,
authored by `<bot>`, waits for that PR's CI, pushes **one** self-fix pass if red,
comments the link on the ticket, then moves the ticket to **In Review**. The specifics
name the flows, the poller's schedule and where the bot's own docs live; read those
docs before second-guessing any of this.

**Two consequences you must respect:**

- **Guard comments are one-way.** Once a ticket carries `🤖 Assigned to` or
  `routed to **Blocked**`, the poller's Todo sweep skips it **forever**. Dragging a
  Blocked ticket back to Todo does nothing. That is why unblocking means **new
  sub-issues**, never re-triggering the parent.
- **Outside the poller's schedule it is asleep.** Moving a ticket to Todo then achieves
  nothing until it wakes. If you act off-hours, start the flow yourself (3E) instead of
  relying on the column move.

### States

`Triage` → `Backlog` (and parking states like `Ice box`) → `Todo` → `In Progress` →
(`Blocked`) → `In Review` → `Testing` → `Done`, plus `Canceled` and `Duplicate`. The
team's real names come from `list_issue_statuses` and the specifics.

Only these matter to you: **Triage** (raw intake, much of it auto-filed), **Backlog**
(groomed, ready to pull), **Todo** (the trigger column), **In Progress**, **Blocked**,
**In Review** (PR open, waiting on the user), **Testing** (PR merged, awaiting
verification; the user closes these to Done). A team without Testing goes straight to
its completed state.

### Trigger comments: know them, never write them

The poller re-runs the bot on a comment from the user's Linear account that @-mentions
someone followed by a command word and `!` (the profile's plan and build commands are
two of them). **You run as the user, so never write that pattern anywhere**: not in a
Linear comment, not in a PR comment, not quoted, not as an example, not even the
profile's own commands (only the user's click in the app posts those). A comment you
meant as a note would fire a flow.

### Starting the bot's flows

Drive the bot the way the specifics say it can be started without a comment (for
example a workflow dispatch with the ticket's identifier and URL). The three flows:

- **Plan**: plan from scratch.
- **Iterate**: revise the existing plan with notes.
- **Build**: write the code; on a ticket with a bot PR this is fix mode (review comments
  + red checks).

Every start is followed by the guard comment the poller would have left (3E). Where a
reviewer's intent would go with a Build, put it in a plain comment on the ticket first.

**No such way in the specifics** → you cannot start the bot yourself. Column moves (3D)
still work while the poller is awake; a step that needs a manual start goes under
**Needs you** with the local command the user can run (`/plan <ID>`, `/plan <ID> iterate:
<notes>`, `/build <ID>`). Do not run `/plan` or `/build` in this session: they wait for
the user's answers and this run is unattended.

## 2. Read the board

```
# Linear MCP: keep `fields` tight; the default response blows the token budget.
list_issues(team: "<TEAM>", limit: 100,
            fields: ["id","title","status","assignee","labels","url","updatedAt","parentId","priority"])
```

`id` carries the identifier (`ABC-74`). There is no `identifier` field; don't ask for one.

```bash
# Open bot PRs for the team's tickets. Scope the query with "<TEAM>-" and keep the limit
# high: results come back newest first, and a PR that has waited weeks for review is
# exactly the one an unscoped, low-limit search silently drops.
gh search prs --owner <org> --state open --author <bot as gh wants it, e.g. app/<name>> "<TEAM>-" \
  --limit 100 --json number,title,url,repository,createdAt \
  --jq '.[] | select(.title | startswith("<TEAM>-")) | "\(.repository.nameWithOwner) \(.number) \(.createdAt[:10]) \(.title)"'
```

The authoritative ticket to PR link is a bot comment on the ticket
(`list_comments(issueId: "<TEAM>-74")`). Match **any**
`https://github.com/<org>/<repo>/pull/<n>` URL in a comment authored by the bot, newest
first. The current shapes are `🤖 PR opened → <url>` / `🤖 PR updated → <url>`, but older
tickets may carry others, so don't anchor on the emoji form. The `gh search` above is the
cross-check for PRs whose ticket comment went missing.

For any PR you're about to act on:

```bash
gh pr view <n> --repo <org>/<repo> \
  --json number,state,isDraft,mergedAt,mergeable,mergeStateStatus,reviewDecision,autoMergeRequest,headRefName,author,statusCheckRollup
```

## 3. The action ladder: first match wins

### A. Land a PR the user already approved (highest value, always check first)

`reviewDecision == "APPROVED"`, author is `<bot>`, `state == "OPEN"`. The user has done
their job; getting it merged is yours.

Arm auto-merge. If the repo's default branch has a **merge queue**, the queue owns the
merge method, so pass **no** strategy flag (a `--squash` then fails with "The merge
strategy for main is set by the merge queue"):

```bash
gh pr merge <n> --repo <org>/<repo> --auto
```

No queue → `gh` says so, and *then* you pick the strategy the repo allows. GitHub
enqueues the PR itself and tests a fresh merge commit against the current default
branch. **Never** `--admin`, never a direct merge, never a manual rebase to "help" the
queue. A silent exit proves nothing: read the PR's state
(`gh pr view <n> --repo <org>/<repo> --json autoMergeRequest,mergeStateStatus`) and
report queued, auto-merge on, or blocked and why.

- **`mergeable == "CONFLICTING"` / `mergeStateStatus == "DIRTY"`** → the branch has
  drifted. Arming auto-merge does nothing; the PR sits until someone resolves it.
  **Resolve it yourself, as part of the same action**; it is part of landing the PR, and
  usually a small additive conflict:

  ```bash
  git clone --filter=blob:none https://github.com/<org>/<repo>.git repos/<repo> && cd repos/<repo>
  git checkout <headRefName>
  git merge origin/<default branch>   # merge, never rebase: the branch is public
  # resolve, then:
  git add -A && git commit && git push origin <headRefName>    # plain push, never force
  ```

  Most of these are two commits appending independent things at the same spot (a new
  test each side, a new entry in the same map). Keep **both** sides unless they
  genuinely contradict: deleting the other side's work to make the merge quiet is the one
  real way to get this wrong. Re-check `mergeable` afterwards; approval and an armed
  auto-merge both survive the push.

  Can't run the repo's tests locally (private package index, missing credentials) → say
  so in the report and let the PR's own CI be the check. Don't skip the resolution
  because you couldn't test it locally.

- **Required checks green** → the command above, note it in the report, done.
- **Required checks red** → find which checks are actually *required* before you touch
  anything; a red non-required check blocks nothing:
  ```bash
  gh api repos/<org>/<repo>/rules/branches/<default branch> \
    --jq '.[] | select(.type=="required_status_checks") | .parameters.required_status_checks[].context'
  ```
  A **required** check red → read the actual job log (never trust the truncated check
  summary), fix **every** root cause it reports, commit, push on the existing branch
  (plain push, never force), then arm auto-merge:
  ```bash
  gh pr checks <n> --repo <org>/<repo>            # find the failing run
  gh run view <run_id> --repo <org>/<repo> --log-failed | tail -80
  ```
- **`autoMergeRequest` is null but the PR was armed before** → GitHub dequeued it after a
  failed merge-queue check and does not retry on its own. Read that run's log. Flaky
  (timeout, `ECONNRESET`, DNS, runner OOM, nondeterministic snapshot) → re-arm
  auto-merge, up to **3** times total per PR (count your own re-arm comments on the
  ticket). Real (assertion, type, lint, import error) → fix it like the red-required case.
- **`mergedAt` set** → move the ticket to **Testing** and comment the PR link. The user
  closes Testing to Done after verifying.

### B. Make a bot PR reviewable

An open `<bot>` PR the user cannot usefully act on yet. Two flavours, both yours:

**B1. Red required check, no review yet** (`reviewDecision: REVIEW_REQUIRED`). The bot's
Build gets exactly **one** self-fix pass and then reports whatever colour it ended on, so
a PR that stayed red has nobody coming for it and will sit for weeks. Resolve required vs.
decorative first (the `gh api …/rules/branches/…` call in 3A). Then read the failing
job's real log, fix every root cause it names, and push on the existing branch:

```bash
gh pr checks <n> --repo <org>/<repo>
gh run view <run_id> --repo <org>/<repo> --log-failed | tail -80
```

If the log shows a genuine infra flake (runner OOM, `ECONNRESET`, a shard the test tool
itself reports as passing) rather than a real defect, re-run the failed jobs instead of
inventing a fix (`gh run rerun <run_id> --repo <org>/<repo> --failed`) and say so in the
report. Don't paper over a flake with a code change.

**B2. Review feedback to address** (`reviewDecision: CHANGES_REQUESTED`, or unresolved
review threads).

**Default: fix it yourself, now.** Invoke the **`fix-pr-comments`** skill with the PR
(`/fix-pr-comments <PR URL>`): it fetches the comments, applies each fix, pushes,
replies with the commit sha and resolves the threads. That is the fast path and what the
user wants for small PRs.

**Delegate to the bot instead** when the feedback needs plan-level rework: it spans
several files, changes the approach, or asks for behaviour the plan never covered. Then
write the reviewer's ask on the ticket in one or two plain sentences, start the bot's
Build (fix mode, section 1), and leave the `🤖 Dispatched Build — writing the code now…`
marker. No way to start it → **Needs you** with `/build <ID>`.

After fixing yourself, still leave the reviewer a reply on each thread. Do **not**
resolve a thread that asks for a product decision; answer it and leave it open for the
user.

For both flavours: when several PRs qualify, take the **oldest**. A bot PR red for two
weeks costs more than a fresh one.

### C. Unblock a Blocked ticket: split it

Read the `🤖 Routed to **Blocked** — <reason>` comment and the plan above it. Three
shapes, three responses:

**C1. Multi-repo plan** (`stack: a → b → c`, or 2+ repos, or `**Phase N — \`repo\`**`
blocks). Split it:

1. One sub-issue per phase/repo, via `save_issue` with `team: "<TEAM>"`,
   `parentId: "<TEAM>-<parent>"`, `state: "Backlog"`, the parent's labels and priority.
2. Title: `<TEAM>-<parent> pt.N: <phase goal>`. Description: **that phase's spec, copied
   in full and made self-contained**: repo, root cause or goal, entry points, approach,
   acceptance criteria. A sub-issue must stand alone: the bot re-plans it from scratch
   and will not read the parent.
3. Where the plan has `depends:` ordering, wire it with `blockedBy` on the dependent
   sub-issue so nothing gets pulled out of order.
4. Comment on the parent: what you split it into and why. Leave the parent **Blocked**;
   it is now a tracking shell.
5. Move **only the sub-issues with no unmet blocker** to Todo, respecting the WIP cap in
   3D. The rest wait in Backlog.

**C2. Open product question in the plan.** Try to answer it from the codebase, the repos
or the ticket's own history; that is real work and usually possible. Answered → start
the bot's Iterate with the answer as the notes, move the ticket back to **Todo**, and the
chain restarts with a revised plan (the parent's `routed to Blocked` guard means the
poller won't re-drive it, which is why you start it yourself; no way to start it → put
the answer on the ticket and list `/plan <ID> iterate: <answer>` under **Needs you**).
Genuinely a product, pricing or security call only the user can make → leave it Blocked
and surface it under **Needs your call**. Never guess a product decision.

**C3. No split can solve it**: the plan says it needs console or admin access, an
external vendor, a purchase, or a judgement only a human can make.

**First, verify the claim; do not take the plan's word for it.** A plan that concludes
"no code change required" is often simply wrong about who owns the thing. Identify the
owning repo yourself before accepting there is nothing to build.

**If it genuinely can't be split, don't ask: this run is unattended.** Put it under
**Needs you** with the concrete options you see (cancel it / a console action only the
user can do / re-scope to something buildable / leave it parked), and say what you
verified. Write the same options as a comment on the ticket once, so the next run sees
it was raised and doesn't raise it again unless something changed.

Always attempt a split before escalating. "Too complex" is a split, not a blocker.

### D. Refill the queue: Backlog → Todo

**WIP cap: at most 3 `<TEAM>` tickets in Todo + In Progress combined** (unless the
specifics set another). In Review doesn't count; that's the user's queue, not the bot's.

Under the cap and Backlog is non-empty → move **one** ticket Backlog → Todo
(`save_issue(id: "<TEAM>-<n>", state: "Todo")`). That single column move is the trigger;
the poller takes it from there. Do not also start the bot's Plan: you'd race the poller
into two duplicate plans.

Pick by: `Urgent`/`High` priority first, then a customer or partner label, then oldest
`createdAt`. Skip anything whose description is too thin for the bot to plan from; send
that back through 3F grooming instead.

**Tickets that arrive in Backlog already analysed.** Some flows hand work straight to
this lane, landing in Backlog (not Triage) because they already did the investigation;
the specifics name them and their marker. They applied the same bounded, single-repo bar
3C uses, so prefer them when refilling and pull them straight to Todo, no grooming.

Backlog empty → go to 3F, which grooms Triage **and** refills from it in the same pass.

### E. Repair a stuck ticket

Compare `updatedAt` against the expected transition. A ticket past its window with the
next guard comment missing means the poller missed it (throttled schedule, off-hours,
failed run). Start the flow yourself (section 1):

| Symptom | Window | Flow |
|---|---|---|
| Todo, no plan comment | > 20 min | Plan |
| In Progress + plan, no `🤖 Assigned to` / `Blocked` | > 20 min | route (the specifics name it) |
| In Progress + `🤖 Assigned to`, no `🤖 Dispatched Build` | > 20 min | Build |
| In Progress + `🤖 Dispatched Build`, no PR comment | > 60 min | check the run first (below), then Build |

Before re-starting a Build, check whether the previous one actually failed (the
specifics say where the bot's runs are, e.g. `gh run list` on its repo, then
`gh run view <id> --log-failed`). A Build that's simply still running is not stuck.
**Re-start at most once per ticket per run**, and comment on the ticket saying you did,
so the next run doesn't pile on. No way to start flows → list the stuck ticket under
**Needs you** with the local command.

**Leave the marker the poller would have left.** The poller decides "does this ticket
still need a Build?" by looking for a `🤖 Dispatched Build` comment, which normally only
it writes. If you start a Build yourself and don't leave that comment, the ticket still
looks build-ready and the **next poller tick builds it a second time**. So immediately
after a manual Build start, comment:

```
🤖 Dispatched Build — writing the code now…
```

Same principle for any flow you start by hand: write the guard comment the poller would
have written, or its sweep will redo your work.

### Closing the run: wake the poller

The poller runs on a schedule, and anything you changed on the board (moved to Todo,
assigned to the bot, promoted out of Triage) sits idle until its next tick. If the
specifics name a way to run the poller now, **end every run that touched the board** with
it. Skip it only if your actions touched nothing the poller reads (e.g. you merged a PR
and moved a ticket to Testing). Mention in the report that you woke it.

### F. Groom Triage: cluster first, then decide, then refill

Triage is often a large auto-filed intake queue, heavily duplicated: the same root error
arrives repeatedly under different titles and different customer names.

**Start with the pattern, not the ticket.** Before touching anything, read the whole
Triage column and group it into clusters by root error or symptom. Working ticket by
ticket down a duplicated queue is exactly how dozens of near-identical tickets pile up.
For each cluster: pick the canonical ticket (best evidence, oldest, or already labelled),
fold any unique evidence from the others into it, then mark the rest Duplicate of it.

Then make at most **5** grooming writes in this one action, cheapest first:

1. **Duplicate**: same root error or symptom as an existing `<TEAM>` ticket. Set
   `duplicateOf: "<TEAM>-<original>"` and `state: "Duplicate"`. Fold extra evidence into
   the original's description first if it adds something.
2. **Cancel**: a prod alert that is no longer firing and left no lasting defect, or an
   intake ticket superseded by shipped work. `state: "Canceled"` plus a one-line comment
   saying why. Verify it's really quiet first with the observability tools this session
   has (the specifics name them). No such tools → leave it.
3. **Promote**: a real, actionable, single-repo defect. Set `state: "Backlog"`, a
   priority, and the right existing labels (`list_issue_labels`; never invent one). If
   the description is too thin for the bot to plan from, add the missing specifics
   (repro, affected repo, observed vs. expected).
4. **Leave it**: needs investigation you can't do in this run. Say nothing; a later run
   or a human picks it up.

**Report the clusters, not just the writes.** "Nine tickets are the same crash across
nine customers" tells the user the underlying defect is one fix, not nine; say it in the
report even for clusters you didn't action.

**Then refill, as part of the same action.** If you promoted anything and Todo + In
Progress is still under the 3D cap, pull the best candidate into Todo now. Backlog stays
the intermediate step (set Backlog, then Todo, never Triage straight to Todo), but both
moves belong to this action.

### G. Nothing to do

Every PR is waiting on the user, the WIP cap is full, Backlog and Triage are clean.
Report idle. Do not invent work.

## 4. The report

Your last message is the report; the user reads it in the app. What isn't on screen is
forgotten, and a wall of text buries the one thing they need to do. Budget ~20 lines, one
line per item; the reasoning goes on the ticket or the PR, linked.

```
## Ready for you (N)
1. ABC-74: "Search" button relabelled to "Run". CI green, open 1d.
   <ticket URL> · https://github.com/<org>/<repo>/pull/6296

## Did (N actions)
- Landed ABC-61: armed auto-merge after fixing the red lint check. <PR URL>
- Split ABC-17 (spanned 2 repos) → ABC-75 + ABC-76, both to Todo. <ticket URL>

## Needs you (N)
- ABC-58: needs admin access to a vendor console, no code to write. <ticket URL>

## In flight
ABC-75 + ABC-76 in Todo (WIP 2/3) · ABC-66 CI running · Backlog empty · 36 in Triage · poller woken

## Next
Check 6207 is green → ABC-66 joins your queue.
```

Rules:

- The first line is the first thing to act on. No preamble, no recap of the task.
- **Ready for you**: cap 5, oldest first. Only open, bot-authored, `REVIEW_REQUIRED` PRs
  whose **required** checks are green.
- State CI in three words or fewer: `CI green`, `CI red on <check>`, `CI running`.
- **Did**: one line per action this run, what changed and where.
- **In flight**: one line, ` · ` separated. Counts, not narrative.
- **Next**: exactly one concrete thing the next run or the user picks up.
- Drop an empty section entirely. Never write "none". Full URLs, never `#6296`.
- State problems flatly: cause and fix.

## 4b. Ask the decisions, then do them

Only when the brief says the user is at the screen. After the report, every **Needs you**
item that is a choice the user can make in a word (park or not, which option, cancel or
re-scope) becomes a question with AskUserQuestion. Items that need the user to *do*
something outside this session (grant access, talk to someone) stay in the report only.

- Up to 4 questions per call; more items: a second call after the first is answered.
- **header**: the ticket id (`ABC-191`) or a 1-2 word topic.
- **question**: one sentence, under 20 words, the decision only. No background; the
  report above has it. "ABC-191: non-admin changes a key's role. Allow or 403?"
- **options**: 2 to 4, each label 1-4 words, the one you recommend first with
  "(Recommended)". Description: one short clause on what you will do if picked.
- The user may answer in their own words (Other); take it as the decision.

Then carry out each answer within the hard rules below: move or cancel the ticket,
write the decision on the ticket as a comment in the user's words (never a trigger
comment), start the bot's flow where the answer unblocks it, or park it. One line per
answer under **Did** in a short closing message. An answer that would need a rule here
broken (approve, merge without the gate, a trigger comment) goes back under **Needs
you** with why.

## 5. Hard rules

- **Never approve a PR.** That is the user's only job and the whole point of the split.
  You run as the user, so `gh pr review --approve` would work; never run it.
- **Never write a trigger comment** (an @-mention followed by a command word and `!`),
  see section 1.
- **Never `--admin` merge, never bypass the merge queue, never force-push.**
- **Never touch a PR not authored by `<bot>`**: a human opened it or took it over. Check
  before every PR action; takeover can happen mid-run.
- **Never edit the description of a ticket the bot is mid-run on** (In Progress with a
  `🤖 Dispatched Build` comment and no PR yet): you'd change the spec underneath it.
- **Never create tickets** other than sub-issues from a split in 3C.
- **Never delete a ticket.** Cancel or mark Duplicate; both are reversible.
- **Never guess a product, pricing, security or compliance decision.** Escalate it under
  **Needs you**.
- **Stay inside the run contract**: first match on the ladder, one action per ticket, 8
  actions at most.
