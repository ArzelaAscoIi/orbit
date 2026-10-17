---
name: orbit
description: "You run inside Orbit, the desktop app that shows this session's row to the user. Use this to put a one-line summary of what you are on next to your session, and to say what you wait for when the hooks cannot. Only inside an Orbit session (the brief has an Orbit section, or ORBIT_STATUS_LOG is set)."
---

# Reporting to Orbit

Orbit shows every session as one row: a state (working, your turn, needs
approval), the last thing you said, and, if you write one, a one-line summary
of what you are on. The summary is what the user reads when they glance at
twenty rows, so keep it current.

## The command

The brief's **Orbit** section names the exact command, binary and log included:

```sh
'<orbit>' report '<log>' note "Writing the parser for the status log"
```

Inside this session's shells the same is reachable through the environment:

```sh
"$ORBIT_BIN" report "$ORBIT_STATUS_LOG" note "Running the tests"
```

If neither the brief has an Orbit section nor `ORBIT_STATUS_LOG` is set, you
are not inside Orbit: do nothing.

## What to report

- `note "<summary>"`: one sentence, under 80 characters, present tense, what
  you are on now ("Reading the ticket's comments", "Fixing the failing CI
  check", "Waiting for the PR checks"). Send it when it changes, not every
  tool call. Only the first line is kept.
- The hooks already tell Orbit when you start a turn, ask a question, wait for
  a permission, finish or fail. Report a state yourself only when you know the
  hooks miss it: `working`, `needs-decision`, `needs-approval`, `turn`,
  `failed`, each with one line of text. Never `ended`; that is Claude Code's.

## Rules

- A session on a ticket leaves one short line on the ticket that it works on it
  (`🤖 <what> locally in Claude Code…`), once, unless its skill already posts one. The
  user reads the ticket, not only this row.

- No secrets, tokens or file contents in a note; it is shown on screen and
  kept in a log.
- Never an @-mention followed by a word and `!` (`@bot Build!`): the team's
  bot runs comments shaped like that.
- A report that fails (the binary moved, no log) is not your problem: ignore
  the error and carry on with the work.
