# LinkedIn series: introducing Orbit

Four posts that introduce Orbit one idea at a time. Each one stands alone,
links the repo, and sets up the next. The drafts are starting points: the
shape and the hook are decided, the wording is not.

Repo link for every post: https://github.com/ArzelaAscoIi/orbit

## The arc

| # | Post | The one idea | Visual |
|---|------|--------------|--------|
| 1 | Connect everything, locally | Your work lives in cloud tools, your agents run on your Mac, and nothing connects them. Orbit does, on your machine. | `orbit-arch.png` (or `orbit-clone-overlay.gif`) |
| 2 | One window for every session | A drop-in for your terminal: every ticket, its session and its PR in one place. | `chaos-vs-control@2x.png`, then `one-window.png` |
| 3 | The clone | A Claude session that runs your other sessions. | `clone-works@2x.png`, `demo.gif` |
| 4 | Fork it | Not built to fit everyone. Built to become yours. | `fork-it@2x.png` |

Cadence: two a week (Tuesday and Thursday mornings), so the series runs two
weeks and each post can point back to the last one.

## How the story builds

1. **The big idea first: connect, locally.** Post 1 names the gap everyone
   has but nobody draws: Linear, GitHub, alerts and cloud agents on one side,
   your terminal, your Claude Code setup and your repos on the other. Orbit
   is the local layer between them: no server, nothing to migrate, one index
   that links a ticket, its PR and its session. It frames Orbit as
   infrastructure for how you work, not as one more agent UI.
2. **Then what it feels like.** Post 2 shows the payoff of that connection
   on screen: the before (four terminals, your brain is the scheduler) and
   the after (one window, the backlog is the scheduler). The before-and-after
   is the hook that makes post 1's diagram concrete.
3. **Then who runs it.** Post 3: once everything is connected and visible,
   a Claude session can drive it for you. The clone only makes sense after
   posts 1 and 2: it works through the same connections and the same window.
4. **Then make it yours.** The call to action of the whole series goes last
   and asks for something concrete: fork it, press Cmd+Shift+F, tell me what
   you changed.

Same frame on every post: a one-line hook, the problem in two or three
lines, what Orbit does about it, one visual, the repo link, a question.
Readers learn the format and skim faster.

## Post 1: Connect everything, locally

Visual: `../assets/orbit-arch.png`. The animated version,
`../assets/orbit-clone-overlay.gif`, works as a second slide or as the video.

<p align="center"><img src="../assets/orbit-arch.png" alt="Orbit sits between your cloud tools (Linear, GitHub, alerts, cloud agents) and your Mac: your terminal sessions, your setup, one local index" width="700"></p>

> Draft
>
> My work lives in the cloud: tickets in Linear, PRs in GitHub, alerts in
> incident.io, cloud agents building in the background.
>
> My agents live on my Mac: Claude Code sessions, my skills, my repos, my
> worktrees.
>
> Nothing connects the two. So I was the glue: copying ticket links into
> terminals, checking which PR belongs to which session.
>
> I built Orbit to be that glue, locally:
> - it reads the cloud tools and writes back only what I did,
> - every ticket gets its own agent session in its own git worktree,
> - one local index links a ticket, its PR and its session.
>
> No server, nothing to migrate, my own keys. Open source, native Mac app.
>
> https://github.com/ArzelaAscoIi/orbit
>
> Where is the glue in your setup today?

TBD: whether to say "open source" here or save it for post 4.

## Post 2: One window for every session

Visual: `../assets/chaos-vs-control@2x.png` first, `../assets/one-window.png`
as the second slide.

<p align="center"><img src="../assets/chaos-vs-control@2x.png" alt="Before: four terminals, your brain is the scheduler. After: one window, the backlog is the scheduler" width="700"></p>

> Draft
>
> Connected tools are only half of it. You still have to see what is going
> on.
>
> Before: four terminals. Which one is waiting for me? What was this one
> about? My brain was the scheduler, and every switch cost focus.
>
> After: one window. Left, every ticket I am on with the one move it needs
> (fix comments, merge, verify, approve). Middle, that ticket's real Claude
> Code session. "Your turn" comes from the session's own hooks, not a guess.
>
> It is a drop-in for the terminal: your Claude Code, your skills, your
> settings, unchanged.
>
> How many agent sessions do you run at once, and how do you keep track?

TBD: a short screen recording instead of the second still.

## Post 3: The clone

Visual: `../assets/clone-works@2x.png`, `demo.gif` as the video.

<p align="center"><img src="../assets/clone-works@2x.png" alt="The clone is a Claude session that runs your other sessions" width="700"></p>

> Draft
>
> Seeing every session in one window helped. Watching them all still took
> my day.
>
> So Orbit has a clone: a Claude session that runs my other sessions. It
> drives Orbit through its CLI (`orbit ctl`), the same buttons I would
> press:
>
> - checks the queue and closes what is finished,
> - starts the next ticket, locally or on the cloud bot,
> - restarts a fix when CI goes red,
> - prepares my reviews.
>
> Give it a goal and it files the tickets in the right order, then drives
> them two at a time. I hear about a PR to review, a block, a merge.
>
> It never writes code, and approve and merge stay my click.
>
> What part of your day would you hand to it first?

TBD: a 20 second clip of the clone filing a mission.

## Post 4: Fork it

Visual: `../assets/fork-it@2x.png`.

<p align="center"><img src="../assets/fork-it@2x.png" alt="Orbit is a blueprint. Fork it" width="700"></p>

> Draft
>
> Orbit will not fit your team on day one. That is on purpose.
>
> Tools used to grow a setting for every team that asked. With agents,
> changing the code is cheaper than configuring it. So Orbit is a
> blueprint:
>
> 1. Fork it. Your team's setup lives in one folder of its own.
> 2. Shape it. Press Cmd+Shift+F, say what bothers you, Claude changes Orbit
>    and opens a PR on your fork.
> 3. Stay current. `make sync` brings in public Orbit every Monday.
>
> Not built to fit everyone. Built to become yours, faster.
>
> Fork it and tell me the first thing you changed:
> https://github.com/ArzelaAscoIi/orbit

TBD: a real example of a change made with Cmd+Shift+F, with its PR.

## Before posting

- Check every visual for real names, tickets or paths. The BEFORE half of
  `chaos-vs-control@2x.png` shows a company name and a home path.
- Post the repo link in the post itself, not only in a comment.
- Pin the series' first post to the profile while it runs.
