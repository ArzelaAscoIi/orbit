---
name: profile
description: "\"/profile\" sets up or improves the app's profile for this user and their company: it reads what it can (Linear teams, labels, the team's bot, recent tickets), asks the user short questions for the rest, and writes profile.json (and any profile skills) in this folder. Nothing outside this folder changes."
---

# Set up the profile

The user wants the app to fit how their company works. Everything specific to the
company or to the person lives in one profile: `profile.json` in this folder, plus
`skills/<name>/SKILL.md` for skills of their own. You write it with them.

If the first message says `new skills: a, b`, the app has skills the profile was not
set up with. Ask only about those: for each, what it is for (read its SKILL.md among
your skills) and whether the profile maps its role to one of the company's own skills
or uses the app's. Then finish as below, even if nothing changes, so the app knows the
profile has seen them.

If `profile.json` exists, you are **improving** it: read it first, say in three lines
what it covers and what is missing, and ask only about the gaps or what they want to
change. If it does not exist, you are **setting it up** from nothing.

## Rules

- Only write inside this folder: `profile.json` and `skills/`. Never post, comment,
  assign or change anything in Linear, Slack or GitHub.
- Read first, ask second. Anything you can learn from Linear (the Linear MCP tools) or
  the user's repos, look up and propose; ask the user to confirm or correct it.
- One question at a time, short, with your best guess as the default, so the user can
  answer "yes". At most about ten questions in a first setup.
- Never put keys, tokens or passwords in the profile.
- Never write a comment or text that addresses a bot by name followed by a command word
  and `!` anywhere except the two command fields below, and only with the user's words.
- Finish in this order. First tell the user in five lines what the profile will do,
  that they can edit it later in Settings (the profile editor) or run `/profile` again,
  and ask if it is right. Then build the file as `profile.json.tmp` in this folder,
  check it with `python3 -c "import json; json.load(open('profile.json.tmp'))"`, and
  move it into place with `mv profile.json.tmp profile.json`. The app watches
  `profile.json`: once it is saved, the app ends this session and takes the user to
  their API keys, so write nothing after the move.

## What to find out, in this order

1. **Company**: name, what the product is (one line, used in prompts), name prefixes of
   internal services or orgs that are never customers.
2. **Teams**: list the workspace's teams with the Linear tools. For each team the user
   cares about, one line on what it owns (product areas). Which team takes a note that
   fits none. This becomes `intake.routing` and `intake.defaultTeam`.
3. **The team's bot** (if they have one that works tickets): its Linear name, the exact
   comments that make it plan or build a ticket remotely (only if it has such commands),
   and when it picks them up. Look at recent tickets: the author of most comments that
   start with 🤖 is likely the bot.
4. **KTLO** (keep the lights on): who files security findings (a scanner's Linear user),
   label names for security and alerts, and title prefixes of tickets its sweeps file,
   each with the story name it groups under and whether it is an alert. Look at recent
   tickets for these patterns and propose them.
5. **How the user wants text**: a style note, for example "short, the point first".
6. **Run the board** (optional): a team whose board an unattended session operates, and
   whether they have a skill for it.
7. **Skills**: the app's own skills (`triage`, `plan`, `build`, `fix-pr-comments`,
   `spot`, `orchestrate`; read them among your skills) do the work everywhere. Fine-tune
   one for this company with `skills/<folder>/EXTRA.md`: only the company's facts it
   needs (repos and what each is, team routing, tool and MCP names, branch and label
   conventions, the bot's login), under `###` headings named after the skill's sections.
   No frontmatter, no repeating the skill's rules. The app appends it to the skill. A
   skill the app does not have at all goes in `skills/<name>/SKILL.md`, mapped under
   `skills` in profile.json. Skills live in this folder only; nothing is synced with
   Linear.
8. **AI reviews**: how someone asks for an AI review of their own PR, and asks again
   after new commits: a comment (`github.aiReview.comment`, e.g. a slash command), a
   label (`label`, taken off and put back), or a reviewer (`reviewer`). Set one.
9. **Weekly priorities**: where each team writes the week's priorities. The app reads
   them first for Plan my day: in Linear, team -> the cycle that covers the week -> its
   documents whose title contains `linear.priorities.title` (default "prio"). Ask
   which teams' cycles to read (`linear.priorities.teams`; empty means the teams the
   user watches); a person who leads certain teams gets theirs under `people`.
10. **Work and incidents**: what the spotter should know (alert channels, where support
   asks land, observability tools), extra read-only MCP tools it may use, and what an
   incident investigation should use (skills, dashboards, tools).
11. **Settings the profile brings** (optional): debug environments (`name`, `setup`
   command, `panes` tools), what Investigate opens, the test app URL.

## profile.json

Every field is optional; leave out what does not apply. Keep strings plain and short,
except `intake.routing`, `spotter.context` and `investigate`, which may be a few lines.

```json
{
  "name": "acme",
  "company": { "name": "Acme", "product": "Acme's analytics platform", "internalPrefixes": ["acme-"] },
  "bot": {
    "name": "Robo",
    "planCommand": "the exact comment that makes it plan",
    "buildCommand": "the exact comment that makes it build",
    "pollerNote": "it picks commands up within about 5 minutes"
  },
  "github": {
    "org": "acme",
    "botLogin": "acme-bot[bot]",
    "aiReview": { "comment": "/ai-review", "label": "", "reviewer": "" }
  },
  "linear": {
    "oauthClientId": "",
    "noteUrl": "",
    "priorities": { "teams": [], "title": "prio" }
  },
  "intake": {
    "routing": "WEB is the web app and dashboards; API is the public API and SDKs",
    "defaultTeam": "WEB",
    "urgentNote": "Ping #oncall."
  },
  "ktlo": {
    "securityCreators": ["Scanner"],
    "securityLabels": ["security"],
    "securityTitles": [" cve:", "cve-"],
    "alertLabels": ["ktlo"],
    "sweeps": [{ "prefix": "[prod]", "story": "Prod alerts", "alert": true }]
  },
  "customers": "extra lines for recognising customers, e.g. 'acme-eu-prod' is 'Acme'",
  "style": "short, the point first",
  "orchestrator": { "team": "OPS", "allow": [] },
  "spotter": { "context": "alert channels, support channel, observability tools", "tools": [] },
  "investigate": "which skills, dashboards and tools an investigation should use",
  "settings": {
    "testAppUrl": "https://staging.acme.example",
    "debug": [{ "name": "prod", "setup": "the command that logs into prod", "panes": ["claude"] }],
    "investigate": { "name": "prod", "setup": "", "panes": [] }
  }
}
```

`skills` (rarely needed) maps a role to a skill of the company's own instead of the
app's; prefer an `EXTRA.md` on the app's skill. `linear.oauthClientId` is only for a company that registered its own
Linear OAuth app; ask, never invent one.
