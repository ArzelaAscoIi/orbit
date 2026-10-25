# LinkedIn series: introducing Orbit

Repo link for every post: https://github.com/ArzelaAscoIi/orbit

| # | Post | The one idea | Visual |
|---|------|--------------|--------|
| 1 | Connect everything, locally | Your work lives in cloud tools, your agents run on your Mac, and nothing connects them. Orbit does, on your machine. | `orbit-arch.png` (or `orbit-clone-overlay.gif`) |
| 2 | One window for every session | A drop-in for your terminal: every ticket, its session and its PR in one place. | `chaos-vs-control@2x.png`, then `one-window.png` |
| 3 | The clone | A Claude session that runs your other sessions. | `clone-works@2x.png`, `demo.gif` |
| 4 | Fork it | Not built to fit everyone. Built to become yours. | `fork-it@2x.png` |

## Post 1: Connect everything, locally

<p align="center"><img src="../assets/orbit-arch.png" alt="Orbit sits between your cloud tools (Linear, GitHub, alerts, cloud agents) and your Mac: your terminal sessions, your setup, one local index" width="700"></p>

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

## Post 2: One window for every session

<p align="center"><img src="../assets/chaos-vs-control@2x.png" alt="Before: four terminals, your brain is the scheduler. After: one window, the backlog is the scheduler" width="700"></p>

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

## Post 3: The clone

<p align="center"><img src="../assets/clone-works@2x.png" alt="The clone is a Claude session that runs your other sessions" width="700"></p>

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

## Post 4: Fork it

<p align="center"><img src="../assets/fork-it@2x.png" alt="Orbit is a blueprint. Fork it" width="700"></p>

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
