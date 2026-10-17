---
name: land
description: '"/land <PR URL> [<PR URL> ...]" lands the PRs the user named: merges each once GitHub says it is CLEAN (through the merge queue where the base has one), re-runs flaky checks, updates branches behind their base and fixes real check failures, until every one is merged. Orbit runs it after the user clicks Land; the user can type it too. Never approves, never --admin, never a PR the user did not name.'
---

# Land

Get every PR in the arguments merged. The user named them (by clicking Land in
Orbit, or by typing `/land`), so merging them is what they asked for. Done
means merged. Work on all of them at once, never one after the other, and do
not wait on the user.

**Arguments:** one or more GitHub PR URLs. None → ask for them once. Merge only
these: never another PR, not even one they depend on.

## 1. One sweep over every PR

For each PR, read its state once:

```sh
gh pr view <url> --json state,isDraft,mergeStateStatus,reviewDecision,headRefOid,baseRefName
```

Then, in the same sweep:

- **CLEAN** (approved, every required check green): `gh pr merge <url> --auto`.
  Where the base has a merge queue this queues it; elsewhere it merges once
  green. Never pass `--admin`.
- **A check failed for a flaky reason** (a timeout, a runner or network error,
  a step that passed on the last run): `gh run rerun <run-id> --failed`.
- **Behind its base:** `gh pr update-branch <url>`.
- **Merged or closed already:** done; say so.
- **Not approved** (review required, changes requested) or a draft: leave it
  and say so in one line. Never approve, never ask someone to.

## 2. Fix the real failures, in parallel

A check that fails for a real reason in code the PR touches: one subagent per
PR, the smallest change that makes the check pass, committed and pushed to that
PR's branch. Change nothing the reviewers approved beyond what the failing
check needs. If a fix would change behaviour, skip that PR and tell the user
why in one line. A failure in code the PR does not touch with a stale base:
update the branch, do not patch around it.

## 3. Check together, merge as each turns CLEAN

While the subagents work, read every PR again: `gh pr merge <url> --auto` on
each that turned CLEAN. One the merge queue dropped: find why (its checks in
the queue run, a conflict), fix or re-run, then queue it again. Repeat until
every PR is merged, or 30 turns have passed.

## 4. Say how each one really stands

A silent `gh pr merge` proves nothing: on a merge-queue repo it may only have
turned on auto-merge. For each PR read

```sh
gh api graphql -f query='query { repository(owner:"<owner>", name:"<repo>") {
  pullRequest(number: <n>) { state mergeQueueEntry { state position }
  autoMergeRequest { enabledAt } reviewDecision mergeStateStatus } } }'
```

and report one line per PR, as a full URL: merged, queued (position),
auto-merge on (waiting for checks), or blocked and why (no approval, a
conflict, a failing check you could not fix).

## Rules

- Never approve, never `--admin`, never force-push, never merge a PR that is
  not CLEAN, never a PR outside the arguments.
- The user's "pause", "wait" or "stop" wins: dequeue every queued PR
  (`dequeuePullRequest`), `gh pr merge <url> --disable-auto` on the rest, say so
  once. If a `/goal` drives this run, ask the user to type `/goal clear`
  (only they can).
- Never write a comment that names the team's bot followed by a command word
  and `!`.
- Stop after 30 turns and say where each PR stands.
