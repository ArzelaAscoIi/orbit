---
name: goals
description: "\"/goals\" finds the two to four things the user's week is about, from what they and their leads wrote down (their priority documents, their own posts in team and leadership channels, the initiatives they own, recent meeting notes), and writes them to goals.json in this folder for the app's Plan my day. Read-only everywhere else."
---

# Goals: what this week is about

The app's **Plan my day** turns the user's tickets into a plan. It used to guess the
week's goals from ticket activity, which ranks noise: ops bugs someone touched, a
blocker another team owns. Your job is to find the goals as the user and their leads
**wrote them down**, and write them to `goals.json` in this folder. The plan is
built on them.

You run as the user (their Linear, Slack, Notion, Drive, calendar), unattended.
Nobody answers questions. Finish in about ten minutes of reading.

## Tuning

Text after `/goals` is the user's tuning and the profile's context: their role, the
channels where they post their weekly priorities, the teams they lead. Follow it over
the defaults below, except the hard rules. Never invent a channel, document or
project it does not name and you did not read.

## Hard rules

- **Read-only.** The only file you write is `goals.json` in this folder. Never post,
  comment, react, edit or move anything anywhere.
- **No DMs, no private messages.** Read channels, documents, projects and meeting
  notes only. Direct messages and group DMs are out, even when a search returns them.
- Never write a comment or text that addresses the team's bot by name followed by a
  command word and `!`. That pattern runs automation.
- A goal must come from something written down. Quote it. If nothing says a thing
  matters, it is not a goal, however busy its tickets look.

## Who the user is

First the viewer: their Linear user, their teams (by key), the initiatives they own,
the projects they lead. Everything after is ranked from their seat: what **they**
ship this week, not everything their company does.

## Where to look, strongest first

0. **The teams' weekly priorities.** The tuning says where they live: in Linear, a
   team's cycle that covers the week holds its priority documents (the cycle's
   documents, titled prio or the words the tuning gives). Read them in full, first:
   they are the week's plan as the team wrote it, and every goal they state is a
   goal. The next cycle's document is the one for a plan made on the weekend.
1. **Their own priority documents.** Linear documents whose title reads as priorities
   (prio, prios, priorities, focus, goals, OKR, "cycle"), **created by the user**
   in the last 14 days. Read **every one of them** in full: someone who leads more
   than one team or stream writes one per stream in the same week, and each Goal or
   "this week" section is a goal, stated by the person who owns it. Documents by
   other people count only when the user's own team is their subject.
2. **Their own posts.** The user's messages of the last 14 days in team and
   leadership channels, **private channels included** (they are channels, not DMs):
   search public and private with `from:` the user, once per query: `this week`,
   `next week`, `prio`, `we will`, `I will`, `when I am back`, `ship`, `roll out`,
   `on the <team> team`, and read the channels where they posted their priority
   document links. Weekly status posts are gold: "This week we will …". Read the
   threads under them: a lead backing an idea makes it a goal; "paused" makes it wait.
3. **The initiatives they own and the projects they lead**: priority, health, target
   date. A project due in the next seven days, or overdue and still in progress, is a
   strong goal. Project status updates say what is blocked.
4. **Meeting notes** from the last 10 days (Drive, Notion, calendar descriptions):
   what their team's standups and planning meetings keep returning to.
5. **Quarter-level briefs** (a roadmap, OKRs, a feature brief) only to order goals
   that the sources above already name.

Collect candidates from **all** of 0 to 4 before you pick; never stop at the documents.
A priority document is written once, at the start of the week; the user's posts after
it are newer and say what changed. Something the user proposed or pushed in their own
posts of the last seven days, and that a lead backed or the user said they will act on
("when I am back I will …"), is a goal even if no document names it yet.

Newer beats older, the user's own words beat anyone else's, a named date beats none.
Rank by the user's stake: what they own and ship themselves before what they only
review or coordinate.
**Paused is not dropped.** A goal paused because its owner is away, or waiting on
another piece, stays on the list: check the calendar or the channel for when they
are back, and say "paused until …" in `why`. Only a goal someone called done,
cancelled or deprioritised leaves.

## What a goal is

- **A result, not a ticket**: "Ship the new runtime to production", "Search v2 PoC",
  "Decide on the repo layout". Two to five of them, ranked: what the user's own
  newest words put first goes first. Never pad.
- **Owned by the user's team.** Work another team owns is not the user's goal, even
  when it blocks one; put it under that goal's `blockers` with who owns it. Find the
  blocker's ticket in Linear and give its identifier: a ticket assigned to the user
  or created by them is still a blocker when someone else has to do the work.
- Its **tickets**: the open ones that move it, from the projects behind it and the
  documents that name them, identifiers only. Leave out tickets with no link to the
  goal, however recently they were touched: recent activity is not a signal.

## goals.json

Before you write it, check: at least one goal cites one of the user's own posts. If
none does, you did not search them (step 2); do it now. `summary` names what each of
steps 0 to 4 turned up.

Write exactly this shape, UTF-8, no comments, once, at the end.

```json
{
  "made_at": "2026-09-27T09:10:00+02:00",
  "week": "the Monday of the week the goals are for, 2026-09-28",
  "summary": "one line: what you read and the gist, at most 20 words",
  "goals": [
    {
      "title": "the goal as a result, at most 8 words",
      "why": "why it is this week's, quoting the source, at most 30 words",
      "projects": ["project names behind it"],
      "tickets": ["ABC-742"],
      "blockers": [
        { "what": "what blocks it, at most 12 words", "ticket": "XYZ-1, or null", "owner": "the team or person who owns it" }
      ],
      "sources": [{ "title": "what it is: Prio doc 09-20, #team post 09-21", "url": "the link" }]
    }
  ]
}
```

Then reply with the summary line and the goal titles, nothing more.
