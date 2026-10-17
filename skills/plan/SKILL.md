---
name: plan
description: '"/plan ABC-150" reads the ticket, the code and traces, and posts the bot''s implementation plan on the ticket; "/plan ABC-150 iterate: <feedback>" revises it.'
---

# Plan, locally

Local replacement for the bot's remote plan and iterate commands. You run in the
user's interactive session and act as them: Linear MCP tools, `gh`/`git` with their
auth, Bash/Read/Grep. The plan is posted without asking; the user refines it with
`iterate:`.

**Invocation:** `/plan <TICKET-ID>` (plan from scratch) or
`/plan <TICKET-ID> iterate: <feedback>` (revise the latest plan). A Linear URL works
too: take the `ABC-123` part. No ID → ask for one.

**Optional tools:** an observability MCP (prod traces/logs) and Jam MCP. Check whether
the session has them. Missing or failing → say so in one line, carry on, and note in
the plan what trace data would have confirmed.

A `## <Company> specifics (from the profile)` section at the end of this skill, when
present, refines the steps below with the company's repos, services and tool names.
It wins over the generic wording where they differ.

## 1. Read the ticket

`get_issue` with the ID, then `list_comments`. Note feature area, layers, ACs, linked
trace/log URLs (extract service, environment or namespace, time window) and Jam links. Iterate
mode → go to §7.

Classify:

| Type | Use when | Signal |
|---|---|---|
| **A** | Localised failure + repro | errors, 4xx/5xx, crash |
| **B** | Slow / hangs / scale | slow, timeout, volume |
| **C** | Multi-layer feature | backend + API + UI, new data path |

## 2. Pick the target repo

Find where the **fix** belongs, not where the symptom shows. For a failure in a
downstream service (operator, worker, queue consumer), ask which upstream service
could have stopped the bad state existing. Name every candidate `<owner>/<repo>` with
a one-line role, then state: **Target: `<owner>/<repo>`** because ‹reason›; symptom
in ‹other repo› because ‹why fixing there is wrong›.

The repo map (what each repo is, which service name it shows up as in traces, which
repos a feature area needs) comes from the context the app gives: the session brief
may include a `## Profile` section with the company's repos, routing and conventions,
and the specifics section below may carry a repo table and a feature area → repos
table. Fill gaps from Linear team descriptions, repo READMEs and `gh repo list <org>`. Never
invent a repo.

Never name a vendor or SaaS (a docs host, a helpdesk tool, …) you have not **seen** in a cloned repo (config,
dependency, CI). A guessed vendor gets the ticket Blocked for weeks. Can't find the
owner → `repos: unknown, checked ‹repos›` plus an Open Question.

## 3. Clone

Into the session's working folder (it belongs to this ticket), once per repo:

```bash
gh repo clone <owner>/<repo> -- --depth 1 --filter=blob:none
```

Type C → clone the full stack. Existing checkout → `git -C <repo> pull --ff-only`.
Failed clone → skip it and note it in the plan.

## 4. Research

Read 2 to 4 key files per repo (entry points from the ticket, trace frames). One
directory-bounded `grep` per hypothesis; two misses → revise the hypothesis, never
widen to the whole tree. No subagents for routine search.

- **A:** prod traces/logs for the service, errors only (or the ticket's link
  parameters) → failure point and root cause. Query it yourself; never write "confirm
  in the traces" as a plan step. Jam link → `getDetails`, `getConsoleLogs`,
  `getNetworkRequests`.
- **B:** slow traces (`duration > 1000ms`), aggregates → bottleneck hypothesis with
  confidence, plus what to instrument if unsure.
- **C:** trace the data flow once; find the closest existing analogue per layer and
  the contracts between layers.

Know how you know. Each claim about cause or cost is **measured** (you saw it in a
trace, log, or a command you ran), **inferred** (read from code or a symptom), or a
**guess**. A guessed root cause is fine to post if it is labelled; stating it as fact
is how a build fixes the wrong thing.

Find the one fact the approach is safe because of (a caller that never passes `None`,
a flag that is off in prod, a contract only one consumer reads) and check it: point at
`file:line`, walk the failure path, or run it. Can't check it → say so; it becomes an
Open Question if the change is risky without it.

Stop when you can say: the approach, the key decisions, the safety fact, and what an
implementer can't infer from the code. A short plan with Open Questions beats fifty
more greps.

## 5. Verify every reference

Every path and symbol in the plan must come from a search result in the clone
(`test -f`, bounded `grep`), never from memory or the ticket's wording. Missing → find
the real one or drop it. UI copy tickets: open the component that renders the string
and check its icon, tooltip, placeholder, `aria-label` and helper text; name what you
checked. No clone or search failed → first line inside the block:
`> ⚠️ **Unverified paths**: repo could not be cloned/searched. The implementing agent must confirm all file and function references before starting work.`

## 6. Write the plan

Reader: an implementing agent. Decisions, approach, pitfalls, contracts only; no line
numbers, variable names or test boilerplate. Don't paste the ACs: write
*ACs: see issue description (consolidated).* Body ≤ ~300 words (A/B), ≤ ~500 (C). Full
`<owner>/<repo>` names. Keep the `###` headings and `**Phase N — \`repo\`**` lines
exact; the split and dispatch logic parses them.

```
🤖 Here's my implementation plan

>>> My analysis

**Bug** · repos: `<owner>/<repo>` · effort: S/M/L (one phrase)

### Root Cause            (B: ### Bottleneck, ≤3 bullets, + confidence High/Med/Low)
≤2 sentences: failing `path::symbol` + why. Each claim ends with (measured: <trace,
log or command>), (inferred: <from what>) or (guess).

### Entry Points          (C: ### How It Should Work (End-to-End), ≤6 bullets,
1 or 2 verified paths.         then ### Existing Pattern to Follow, 1 to 3 verified paths)

### Approach              (C: ### Implementation Plan with one block per repo:
≤6 bullets, the last           **Phase N — `repo`** · Goal · ≤4 decisions · depends: <contract>)
`Safe because: <fact> (<pointed at file:line | walked it | ran it | unchecked>)`.

### Open Questions        (product/design blockers only, ≤3; omit if none)

### Performance Impact    (only if a heavy editor/canvas view, page load or render
render frequency,              frequency is involved)
API calls per render, graph updates; else "profile before and after"

### Acceptance Criteria
ACs: see issue description (consolidated).

---
*Draft, verify in repo.*
>>>

💬 `/plan <ID> iterate: <feedback>` to refine, `/build <ID>` to build it.
```

Type C uses `**Feature**`; type B uses `**Bug**` with the Bottleneck section. Type C
puts the `Safe because:` line at the end of the Implementation Plan. The labels
replace prose; they do not raise the word budget. The first line must be exactly
`🤖 Here's my implementation plan`: the poller reads it as "plan ready". Never use it
for a draft.

## 7. Iterate mode

Find the latest `🤖 Here's my implementation plan` comment. Feedback is the text after
`iterate:`. Research only if it names new files or areas. Count prior plan comments
and title the new one `🤖 Here's my implementation plan (vN)`. Keep the same template;
right after `>>> My analysis` put `### What changed` (≤4 bullets), then the short
revised body. Post it as a new comment; never edit the old one.

## 8. Post, route

1. Show the full plan in the session, then post it at once. Never ask before posting
   the plan: no Post / Change / Stop question. Anything worth flagging (a deviation
   from the ticket's suggestion, an assumption) goes in the plan itself; the user
   revises with `/plan <ID> iterate: <feedback>`.
2. Post with `save_comment` on the ticket, exactly as shown.
3. Ticket in **Todo** → move it to **In Progress** (`save_issue`, `state: "In Progress"`).
4. Route. **Blocked** if any: plan names 2+ distinct repos or has `**Phase N**`
   blocks; non-empty Open Questions; the ticket needs multi-team coordination, a
   pricing/entitlements/security/compliance change, or customer-facing behaviour that
   needs product sign-off.
   - **Blocked** → `AskUserQuestion` with the specific reason (Move to Blocked / Leave it).
     On yes: `save_issue` `state: "Blocked"`, then comment exactly
     `🤖 Routed to **Blocked** — <specific reason, e.g. "plan spans \`<owner>/<repo-a>\` and \`<owner>/<repo-b>\`">. Needs a human call before this can be assigned.`
     This guard is one-way: the poller never re-drives the ticket. Unblocking means
     splitting it into sub-issues.
   - **Simple** (single repo, no open question, no product risk) → `AskUserQuestion`:
     **Build now, locally** / **Hand to the bot (cloud)**.
     - Build now → comment exactly
       `🤖 Assigned to <your Linear name> — building locally in Claude Code.` (your
       name from the Linear MCP's current user) then run `/build <ID>` in this session
       straight away. The build posts `🤖 Dispatched Build` before anything else;
       together the two comments keep the cloud poller off the ticket (it builds only
       In Progress tickets that have `🤖 Assigned to` and no `🤖 Dispatched Build`).
     - Hand to the bot → write nothing more; the bot's poller dispatches and builds
       it in the cloud.
     Never post the `🤖 Assigned to` comment without starting the build at once: on
     its own it tells the poller to build in the cloud.

Rules: never write a comment that @-mentions the bot (or anyone) followed by a command
word and `!` (the poller treats it as a command from the user). Skip any step whose
guard already exists (a later plan, `🤖 Assigned to`, `routed to **Blocked**`). Never
edit other ticket metadata. Never log tokens.

## 9. Report

Max ~8 lines, full URLs:

```
ABC-150 · plan v1 posted → <comment url>
Bug · <owner>/<repo> · effort S
Root cause: <one line> (measured | inferred | guess)
Moved Todo → In Progress · Routed: simple
Skipped: traces (not connected)
Next: /build ABC-150
```
