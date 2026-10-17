# Cloud agents

Orbit is local first: every session runs on your Mac, in your Claude Code,
with your keys. Many teams also run an agent in the cloud that works the
backlog on its own. Orbit is built to follow one, and to hand it work.

This page shows what that looks like and what your bot needs to do. It does
not ship a bot: yours will be shaped by your stack, your CI and your rules.

## What a cloud agent is here

A service, usually a scheduled job, that:

1. **Listens to Linear.** It polls (or gets webhooks) for tickets in its
   team and for comments addressed to it.
2. **Works a ticket through stages.** Triage it, post a plan, build it in a
   sandbox, open a PR, fix review comments.
3. **Reports on the ticket.** Every step is a comment, so humans and Orbit
   both see where it is.

Linear is the bus. No extra API between Orbit and the bot.

## How it looks in Orbit

- **The rail** on a ticket (triage → plan → build → review → test → done)
  moves as the bot comments. Its plan shows on the ticket; its PR on the rail.
- **For you** shows only where the bot waits on a person: a plan to approve,
  a question it asked, its PR to review.
- **Plan** and **Build** run locally by default. The ▼ next to them offers
  **Remotely, &lt;bot&gt;**: Orbit posts the bot's command as a comment, exactly
  as configured, and shows "asked &lt;bot&gt; 2m ago · picks it up within 5 min".
- **Run the board** and **missions** hand tickets to the bot and wait on it,
  with a cap on how many it builds at once.

```
 ABC-142  Export button does nothing
 01 triage ── 02 plan ── 03 build ── 04 review ── 05 test ── 06 done
                            ▲
   ✳ Build  ▾              │
   ├ Here, with Claude      │  local: this ticket's session, its worktree
   ├ Here, with Codex       │
   └ Remotely, Robo: @Robo Build!   → a Linear comment; Robo's poller does the rest
```

## Wiring yours

In Settings → **bot & PRs** (or your profile):

| Field | Example | What it turns on |
|---|---|---|
| bot name | `Robo` | its comments read as the bot's on the timeline |
| plan / build / investigate command | `@Robo Plan!` | "Remotely, Robo" under each step's ▼ |
| poller note | `picks it up within 5 minutes` | said after you ask it |
| bot's GitHub login | `robo-bot[bot]` | its PRs count as the bot's |

Your bot's comments start with `🤖` and use a few fixed phrases Orbit reads
for the stage, for example `🤖 Here's my implementation plan`,
`🤖 Dispatched Build`, `🤖 PR opened → <url>`. Orbit's own triage, plan and
build skills write the same phrases, so a local session and a cloud bot look
alike on the ticket. The list lives in `src/story.rs`.

## Safety

Linear comments are commands to the bot. Orbit never writes `@<bot> <Word>!`
on its own: only your click on **Remotely, &lt;bot&gt;**, and it posts exactly
the configured command, nothing else. Skills and sessions are told the same.

## Building a bot

The shape that works:

- A scheduled job (GitHub Actions is enough) that polls Linear for its
  commands, every few minutes.
- One agent run per ticket and stage, in a sandbox with the repo checked out.
- The same skills Orbit ships (`skills/triage`, `skills/plan`, `skills/build`)
  as its playbook, so local and cloud runs behave alike.
- A review cap: it stops building while N of its PRs wait for review.

Start local. Move a stage to the cloud once you trust what it does on your Mac.
