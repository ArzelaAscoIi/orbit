---
name: orbit-improve
description: "\"/orbit-improve [feedback]\" turns the user's feedback on the app itself (a UX annoyance, a bug, a missing feature) into a pull request on the app's own repo: reads the activity log for what they were doing, asks what to change, and, when the app already does it, shows them how and refines only what is still missing; otherwise implements it in a clean worktree, runs the checks, and opens a ready-for-review PR. Never merges."
---

# Improve: the app fixes itself

The user pressed the feedback shortcut in the app. They want the app to be better,
and you are the one who changes it. The session's context names the app's repo, the
worktree command, the activity log, and the app version and view they were on.

## 1. Understand the feedback

- Everything after `/orbit-improve` is the feedback, verbatim. Without it, ask the user in
  one line: "What should be better?" and wait. They are here.
- Read the last 200 lines of the activity log (`tail -n 200 <log>`): one JSON line per
  action, with `event`, `detail`, `view`, `tab`, and `via` (`key`, `click`, or empty when
  the app did it). It holds only action names, never text or keys. Use it to see what
  they did just before: the view, the steps, a shortcut they missed, a loop of clicks.
  The log may be missing (turned off); then go by the feedback alone.
- Unclear what right looks like? Ask at most two short questions, then go.

## 2. Get the repo

Run `<worktree command> <owner/repo> improve/<short-slug>` with the repo from the
context. It prints the path of a clean worktree on a new branch; `cd` there and work
only there. Before the first commit, `git rev-parse --show-toplevel` must print that
path; anything else, stop and tell the user where you are.

Read the repo's `CLAUDE.md` first and follow it: its rules, map, and checks are the law
here (tests next to the code, tutorial steps for user-facing features, no company
specifics in the repo).

## 3. Is it already there?

Before changing anything, check whether the app already does what they ask. Look where
features are written down: the tutorial `STEPS` (`src/ui/tutorial.rs`), the command
palette items (`src/app/palette.rs`), the keys (`keys()` in `src/ui/mod.rs` and
`src/ui/bar.rs`), `settings.json` fields (`src/store.rs`), and `CLAUDE.md`'s map. Then
grep the code for the behaviour itself.

- **It exists**: tell them how to use it in a few lines: the shortcut, where to click,
  the palette name, the setting. If the activity log shows they took a longer way
  (many clicks where one key would do), say so. Then ask whether it does what they
  wanted, and help them refine the request: what is missing, what is in the way, what
  would have made them find it. No PR for a feature that already works.
- **It exists but falls short**, or they could not find it: the gap is the change
  (a better name, a shortcut, a hint, a tutorial step, the missing piece). Agree on it
  with them in one line, then go to step 4.
- **It does not exist**: go to step 4.

## 4. Make the change

- The smallest change that fixes what the user said. No refactors on the side, no
  scope they did not ask for. A second idea goes into the PR body as a suggestion.
- Run the repo's full check (`make check` if there is one) and see it pass. Try the
  change against a scratch data dir (`ORBIT_DATA=target/dev-data`), never the
  user's real one.
- Commit with a message that says what changed for the user.

## 5. Open the PR

Push the branch and `gh pr create` (ready for review, not draft) with:

- **Feedback**: the user's words, verbatim, in a quote.
- **What they were doing**: the few activity log lines that matter, as a short list
  (event, view, via). Never paste the whole log.
- **Change**: what is different now, in two or three sentences.
- **How to try it**: the exact steps.

Then tell the user in two lines: the PR link, and what to try.

## Hard rules

- Never merge, approve, close, or force-push. The user reviews and merges.
- Never touch the user's data dir, keys, or sessions; never `pkill` by pattern.
- Never print or commit keys. Before committing, grep the diff for `lin_api_` and
  `sk-ant-`.
- Nothing lands on the default branch directly: always the branch and a PR.
- Report what you ran, not what you expect. "Checks pass" means you saw them pass.
