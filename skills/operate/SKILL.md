---
name: operate
description: Operate Orbit for the user through its CLI (orbit ctl). One pass over notifications, alerts, releases, For you, PRs and sessions; press what Orbit lets a program press, and tell the user what happened.
---

# Operate Orbit

You sit on top of Orbit and work it the way the user would, only through
its CLI. The user watches Orbit while you work: each button you press
shows up there as a grey `agent` toast. Each `say` line reaches them as a
toast and, while Orbit is behind other windows, as a Mac notification.
Your context gives the binary's full path. `orbit` below means that path.

## Rules

- Use only `orbit ctl`. No gh, git, Linear or file edits of your own:
  everything goes through Orbit, so the user sees it and it is logged.
- Read the gates with `orbit ctl actions`:
  - `look` and `do`: press them when they move the work along.
  - `queue` changes what other people see: a Linear state, a comment, a
    reviewer or label on a PR, closing a PR, a new ticket. Never press it.
    Put it in your report as a proposal, with the exact action JSON.
  - `yours` is approving, merging and answering a permission prompt. Orbit
    refuses these to a program. Leave them to the user and say so.
- After every step that pressed something, send one line on what you did
  and why, at most 120 characters, ticket identifiers rather than ids:
  `orbit ctl say "Closed 6 sessions: their PRs are merged"`.
- **Nothing new, nothing said.** In a `/loop`, compare with your last
  pass. When you pressed nothing and what waits on the user is the same
  as last time, end the pass with no `say` at all: the user already heard
  it. Say it again only when something changed, or once a day.
- One session per piece of work. Before you start one, check
  `orbit ctl sessions` for an existing one on the same ticket. If it is
  working, leave it. If it is your turn, say it waits on the user.
- A press that starts a session puts its key in `orbit ctl state`
  (`session`). Before the next press, check it started:
  `orbit ctl wait <key> working "your turn" done failed --timeout 60`. If it
  says `failed`, `orbit ctl ticket <key>` shows why. Say so; do not
  press it again.
- Never write `@<anyone> <Word>!` anywhere: the team's bot runs that.
- When unsure, do not press. Put it in the report.
- **Shrink the user's queue, never grow it.** Count what waits on them:
  For you rows with ask `Review`, `Decide` or `Verify`, plus sessions that
  need approval or are on their turn. While that count is 5 or more, start
  no new work: no `/plan`, no `/build`, no new ticket session. Only press
  what takes something off their plate (prepare reviews, close finished
  sessions, mark resolved notifications done) and say the count.
- **No build without the user's yes.** Never start `/plan` or `/build` on
  a ticket the user has not said yes to: it is still in Triage or
  Backlog, or it asks `Decide`. Prepare the decision instead (see step 4).

## One pass

When `/operate` comes with words (`/operate alerts sessions`), do only
those parts: `alerts` is step 2, `notifications` step 3, `foryou` step 4,
`prs` step 5, `sessions` step 6, `initiatives` means `orbit ctl
missions` and one `say` per mission on where it stands and what is
the user's. Run inside a `/loop`, keep each pass short and end it with
one `say` line.

1. `orbit ctl help` and `orbit ctl state`.
2. **Alerts, incidents, releases**: `orbit ctl alerts`.
   - A firing alert or a live incident with no session yet: start one, with
     `{"InvestigateAlert": {"id": "<id>", "debug": false}}` or
     `{"Investigate": "<incident id>"}`.
   - A red release with no session: `{"FixRelease": <run id>}`.
3. **Notifications**: `orbit ctl inbox`.
   - A row about a ticket or PR that For you also shows is handled in
     step 4.
   - A row that only informs and is already resolved (the PR merged, the
     ticket done): `{"InboxDone": "<key>"}`.
   - The rest: one `"HandleInbox"` session takes them all, but only if no
     inbox session is working already.
4. **For you**: `orbit ctl tickets`, and `orbit ctl ticket <ABC-1>` where
   the row is not enough.
   - **Review asks**, the bot's PRs waiting on the user: one `"ShipReviews"`
     session prepares them. The approval stays the user's. List them as
     theirs.
   - **A PR with failing checks or open review threads** whose ticket has
     no running session: start the fix with
     `{"RunSkill": {"issue_id": "<id>", "command": "/fix-pr-comments", "submit": true}}`.
   - **Ready to build or plan**: only a ticket the user said yes to (moved
     to Todo or later, not asking `Decide`), with no session:
     `{"RunSkill": {"issue_id": "<id>", "command": "/plan", "submit": true}}`,
     or `/build` once a plan is on the ticket.
   - **`Decide`, or a plan whose The ask says Push back or Ask**
     (`orbit ctl ticket <ABC-1>`, `description` and the plan): list it
     under Yours with the need, the verdict and the suggested reply, so
     the user decides in one look. Never build it, never send the reply.
5. **PRs**: `orbit ctl prs`, the user's own open PRs.
   - A draft that looks done, a PR without a reviewer, or one behind its
     base: propose `ReadyPr`, `RequestReview` or `UpdateBranch`. All three
     are `queue`.
6. **Sessions**: `orbit ctl sessions`.
   - Close a session (`orbit ctl close <key>`) when its `work.finished` is
     true, or when it ended with no open work. Never close one that needs
     approval or is working.
   - Say how many you closed, and why.
7. **Wait for what you started**, at most 10 minutes per session:
   `orbit ctl wait <ABC-1> "your turn" done "needs approval" stopped --timeout 600`.
   Then `orbit ctl ticket <ABC-1>` and `say` what came of it: a PR opened,
   a question for the user, a failure.
8. **Report** in short markdown, in four sections:
   - **Done**: what you pressed, and what came of it.
   - **Yours**: approvals, merges, permission prompts and decisions, each
     with where to click in Orbit.
   - **Proposed**: the `queue` actions you did not press, each with its JSON.
   - **Left alone**: what you skipped, and why.

   End with `orbit ctl say "<one-line summary of the pass>"`.

## Links

Every ticket, PR and session you name gets a link the user can Cmd+click
in Orbit, on its own after the name:

- A ticket or PR: its full `https://linear.app/…` or `https://github.com/…`
  URL, from `orbit ctl ticket`, `tickets`, `prs` or `missions`.
- A session: its `link` from `orbit ctl sessions`, `orbit://session/<key>`.

Write them bare, never as `[text](url)`: Orbit's terminal only sees plain
URLs. For example: `ABC-206 has its PR: https://github.com/acme/app/pull/7001`
