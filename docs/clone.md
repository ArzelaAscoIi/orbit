# The clone

One agent per ticket is easy. A feature of five tickets that land in the
right order, while you do something else, is the hard part. In Orbit that is
the clone's job.

<p align="center"><img src="img/clone.svg" alt="You give the clone a goal; it files a parent with sub-tickets; Orbit drives them through the bot or local sessions, two at a time, blockers first; you hear only about reviews, blocks and merges." width="900"></p>

## What it is

The clone is your agent double: a Claude Code chat on the right of the window
(**Cmd+J**), running on your own login. It does not write code. It works
**Orbit** for you, through `orbit ctl`, the same buttons you click:

```sh
orbit ctl state                  # where Orbit is, what waits on you
orbit ctl tickets                # For you, in its order
orbit ctl sessions               # every session: working, asking, its PR, why it failed
orbit ctl do '{"Show":"Sessions"}'   # press any button Orbit has
```

Every action it takes shows in your window as it happens (a grey "agent"
toast) and is logged as the clone's, never as yours. The buttons that belong
to you stay yours: `orbit ctl` refuses to approve or merge, and Orbit never
posts a bot command the clone wrote.

## How a goal becomes merged PRs

**1. You say what you want, in your words.**

> Ship saved views for dashboards: per-user saved filters, shareable links,
> a default view. Robo builds, I review.

**2. The clone files it.** A mission run reads the code and the backlog, then
files one parent ticket with short sub-tickets in the right team, linked with
*blocks* relations for their order. If an item adds a setting or touches
another product surface, it asks you once before filing.

```
WEB-170  Saved views on dashboards
├─ WEB-171  Store filters per user
├─ WEB-172  Pick a default view
├─ WEB-173  Share a view by link       blocked by WEB-171
└─ WEB-174  The views menu
```

**3. Orbit drives the parent.** This part is plain Rust, not an agent: every
two minutes it reads the parent again and moves it forward.

- **Next** are the sub-tickets whose blockers have merged.
- **At most two** are in flight at once.
- Each goes to a worker: the team's cloud bot (moved to its Todo) or a local
  session on that ticket, in its own worktree, started with `/build`.
- A sub-ticket the bot sits on for 30 minutes (plan done, no build, a red PR)
  gets the bot's exact Plan or Build command once, and you hear that it did.
- A sub-ticket with its PR open counts as implemented. The review is yours.

Waiting costs nothing: no session sits there polling, and no tokens are spent
while the bot works.

**4. You hear only what is yours.** A PR to review, a sub-ticket the bot
blocked, a merge. Each is one notification and one line in the initiative's
log (`orbit ctl initiatives`), with times and why each sub-ticket is where it
is: *with the bot*, *waiting on WEB-171*, *next*.

<p align="center"><img src="img/clone.png" alt="The clone panel driving WEB-170: one PR for your review, one with the bot, one waiting on its blocker, one next; below, the chat where the goal came in." width="900"></p>

**5. You review and merge.** The next sub-ticket starts as soon as its blocker
merges. When every sub-ticket has its PR, the initiative is done.

## The hourly pass

Beside initiatives, the clone can run your board every hour while you work
on something else. Tick what it may take care of (alerts, notifications, For
you, PRs, sessions, initiatives) and press **Start**: one Claude Code session
runs `/loop 1h /operate` on exactly those. It stays quiet when nothing changed,
says once when something did, and shows `last run failed` until a later run
gets through. Untick everything to pause it.

## Rules it keeps

- **It coordinates and never implements.** "Build X" means handing X to a
  worker, never a commit in its own shell.
- **Check-ins go on the ticket** as Linear comments, written through Orbit's
  own key, so your team sees them where they look.
- **Your pause wins.** Stop or wait undoes pending merges (out of the merge
  queue, auto-merge off) and stays until you clear it.
- **It learns from your complaints.** When you correct it, it writes the
  learning to a file first, so the next run already knows.

## Make it drive your way

`settings.json` → `clone`:

```json
"clone": {
  "instructions": "Prefer the bot for API work, local sessions for UI.",
  "skills": ["operate", "mission"],
  "tasks": ["alerts", "notifications", "prs"]
}
```

The skills it runs (`skills/operate`, `skills/mission`) are plain Markdown in
this repo. Change them like any other code; see
[make it yours](make-it-yours.md).
