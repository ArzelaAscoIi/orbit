# Orbit

A native Linear client (Rust, egui) that shows what the team's bot is doing
and what needs the user, with Claude Code running per ticket in the
background. Views draw and emit `Action`s, state changes after drawing,
network work never blocks the UI thread.

## Run and check

```sh
cp .env.example .env        # once; put LINEAR_API_KEY in it
make run                    # release build, keys from .env, no Keychain prompt
make dev                    # debug build against target/dev-data (safe to break)
make watch                  # make dev, rebuilt + restarted on every save (brew install watchexec)
make check                  # fmt + clippy -D warnings + tests. Run before every commit.
make shot VIEW=intake       # PNG of one view in target/shots/ (see scripts/shot.sh)
make bundle                 # dist/Orbit.app
make release V=0.2.0        # tag + push; .github/workflows/release.yml publishes the zip
orbit ctl help              # drive the running app from a shell or an agent (control.rs)
```

`make run` sets `ORBIT_NO_KEYCHAIN=1`, so keys come from `.env` only
and a rebuilt binary never triggers the macOS password dialog. Keys in env
variables always win over the Keychain. The user's real state lives in
`~/Library/Application Support/Orbit/`; point
`ORBIT_DATA` elsewhere for anything experimental.

`make watch` kills the app with SIGTERM on every save, before the rebuild:
a failed build leaves no window until the next save compiles. With the
Keychain off, keys typed into the app go to `<data>/keys.json` (mode 600)
and survive restarts; `.env` still wins and is read once, restart
`make watch` after editing it.

## Map

```
src/main.rs        NAME, NOTE_URL, module list, eframe launch; `hook <log>`, `report <log> <state> <text>`, `worktree <repos> <owner/repo> <branch>` and `--skill` subcommands (no window)
src/app/           state and the Action/Event loop
  mod.rs           App, Action, View, Tab, App::new, apply/handle wiring
  actions.rs       apply(): what the user did
  events.rs        handle(): what the backend answered
  palette.rs       command palette (Cmd+K): items() from the app (ticket actions, quick actions, tickets, sessions by state, repos, shortcuts, stories, customers, Linear search), rank()
  mod.rs           View::Repos / View::Repo(slug) (the 9 tab: `App::repos()` from pins, deploys and PR links; a page's PRs in `repo_prs`), `toasts` (notify.rs lines inside the app, bottom right, 10 s)
  panes.rs         right column split tree: ticket panes (each a full ticket view; the active one's view is App's) + shells
  board.rs         optimistic overlay (Shown/Local, request-numbered holds) + session_overlay (a session's PR, question, events on its card)
  mod.rs           keep_fresh(): Linear re-asked every 20s and at once when a session's status log grows; logic() runs it (and sessions, backend events, notify) while the window is hidden too
  settings.rs      settings.json as text (Settings view): save, keys to the Keychain as SAVED, follow the file
  ai.rs            Haiku helpers (brief, short names, customers, digests) + Linear notes
  review.rs        ORBIT_VIEW/TAB/SCREENSHOT hooks, off by default
  control.rs       `say` (agent toast + Mac notification when behind), `alerts`, `prs`; answers `orbit ctl` between frames: apply() as a click does, logged via cli, a grey agent toast, gate `Yours` refused; `work_of` looks up off-board tickets/PRs once
src/ui/            drawing only; every function takes &App and pushes Actions
  mod.rs           show(): login → team picker → bottom bar, windows sidebar (bar::sidebar, left, collapsible) | panes, then the sheet, toasts, capture and tutorial overlay
  widgets.rs       shared frames, rows, chips, secret_field, markdown
  login.rs bar.rs list.rs ticket.rs diff.rs sessions.rs settings.rs capture.rs
  find.rs          Find on the screen (Cmd+F): every text drawn this frame, matches marked, Enter/Shift+Enter step
  panes.rs         draws the panes: one terminal per session, draggable dividers; pane keys in ui/mod.rs keys()
  palette.rs       the palette window: arrows, Enter, Esc; create-ticket / new-session fallbacks
  testing.rs       Testing tab: the ticket's test plan, steps to tick, commands to copy, a browser for UI plans, Mark done
  tutorial.rs      guided tour: STEPS (view + anchor + text), mark(), overlay; `tutorial` link in the sidebar footer
src/backend.rs     tokio runtime; Command in, Event out
src/control.rs     `orbit ctl`: <data>/orbit.sock (first copy serves), the CLI (`close`, `sessions` with each one's work state), `actions` read from the Action enum's source
src/linear.rs      GraphQL client: home(), save_note (idempotent by URL), set_state, add_comment
src/editor.rs      the plain text editor pane (Surface::Editor): shift() moves selected lines (Tab, Shift+Tab: shared indent), save() to a typed path; text in App::editors, memory only
src/browser.rs     embedded WebKit view (wry, macOS only) for UI test plans: views place() it per frame, App::ui follows; hidden under the tutorial
src/capture.rs     New ticket (Cmd+T): Haiku intake (kind, squad, area, P#, obvious?, questions), duplicate check (search all teams + Haiku, shared evidence only), triage-template draft; filed triaged in Backlog
src/laya.rs        Laya, optional and exchangeable with Haiku: New ticket's bug-or-not (evaluated; team/priority were not better than guessing), shadow or use by confidence, laya-shadow.jsonl; QUICK questions on the open ticket (before any PR) propose quick actions, answers and clicks in laya-quick.jsonl
  laya/model.rs    Laya in-process: pinned HF checkpoint downloaded once (Settings), candle ModernBERT + ported head; tests/laya-parity.json keeps it equal to Python's
src/inbox.rs       Notifications (0): Linear + GitHub notifications and direct review requests in one list, allowlist of what asks something, one row per ticket/PR; open or x marks read on both; a PR no ticket has opens as `story::pr_card` (id `pr:`, `Card::pr_only`: no Linear parts), in `App::pr_cards`
src/k9s.rs         a profile's k9s/ (aliases, views, plugins + scripts) merged into the user's k9s config in a marked block before a debug pane opens k9s; scripts to <data>/bin
src/incidents.rs   incident.io REST: live incidents (KTLO view), alerts of the last day (Alerts view, 8), the Investigate briefs
src/signoz.rs      SigNoz through its MCP gateway (profile `stats.mcp`, no API key: private network): initialize, initialized, tools/call; `team()` the dashboard's PR numbers for the Stats view (Cmd+K, Settings)
src/stats.rs       the user's week from activity.jsonl and the status logs, for the Stats view
src/herdr.rs       herdr next to Orbit: its agents (`herdr agent list`) in the sidebar, focus, start a ticket in herdr; the user's herdr config (keys, theme, toasts, sounds, sidebar) read with toml; the plugin in herdr-plugin/ (orbit needs / orbit open) linked from Settings
src/releases.rs    settings.json `releases` (repo + workflow): each one's latest run via `gh run list`, Fix it brief for a red one
src/testplan.rs    TestPlan: Sonnet reply parser, Linear comment render/parse, per-ticket marker
src/work.rs        Plan my day (the sheet's 2 tab): the spotter session (skills/spot) reads the goals + cycle prio docs first, writes the day to spots.json (focus, spots best first incl. route `you`, reach, blind, park); done.json (spots started; read-modify-write); `to_start`: the best `now` spots start on their own, at most `work.autoStart`; a spot's brief (Fix now) and ticket note (File a ticket → capture.rs)
src/today.rs       date(): the key of what is made once a day
src/tray.rs        the menu bar icon: count of what waits on the user, a menu of sessions and For you; `menuBar: false` hides it
src/claude/idle.rs which of Orbit's own background sessions to stop (idle, out of sight, quiet 12 h or an hour after done), every 10 min off the UI thread, and `orbit sessions-stop [--dry-run]`
src/cleanup.rs     disk cleanup, daily off the UI thread and `orbit cleanup [--dry-run]`: artifacts in idle session folders, finished folders (clean and pushed), unlinked .deps; rules in its module comment
src/update.rs      update in place: the newest release's *-macos.zip via `gh`, swapped in for the running Orbit.app
src/demo.rs        ORBIT_DEMO=1: a made-up company's board, PRs and sessions from tests/demo/, never the network (README GIF, screenshots)
src/goals.rs       the week's goals: goals.json from the goals run (skills/goals: read-only, the user's prio docs, own channel posts, owned initiatives, meeting notes), settings.json `goals` wins; Plan my day finds them first when this week's are missing
src/mission.rs     missions: the clone drives a parent's sub-tickets to the bot (Todo, 2 in flight, blockers merged first); missions.json; skills/mission files the tickets
src/autopilot.rs   Run the board every hour while on, off by itself after 8 runs (switch in Sessions); autopilot.json read-modify-write
src/notify.rs      Mac notifications (the app's own via objc2 from a bundle, else osascript with the line as argv): a card that newly asks (review/decide/verify), a new Notifications row, a session waiting on the user (your turn not looked at, needs approval), only while Orbit is not in front and only once open on two checks (`stable`); notified.json read-modify-write, first run says nothing
src/story.rs       `the_ask` (the plan's `### The ask`: whether to build at all, shown on the ticket); Issue → Card: lane (Product / KTLO security / KTLO alerts), stage, ask, story grouping, bot detection, timeline, customers
src/anthropic.rs   raw Messages API; prompts; parse helpers. Haiku for helpers, Sonnet for the diff guide
src/github.rs      `gh pr diff` → File/Line; merge gate (`gh api graphql`: CLEAN, not draft, not queued, can_approve) and head-bound squash merge / merge queue / approve; `repo_prs` (a repo page's open PRs, review-requested marked), `slug_of`
src/claude/        Claude Code integration
  mod.rs           Sessions: saved map, start/attach/stop/resume, TRIAGE and ORCHESTRATOR keys, free:<ms> sessions without a ticket; two-part brief (intent verbatim + spec)
  worktree.rs      one clone per repo under <data>/repos, a worktree per ticket on the build branch, isolation check
  agents.rs        `claude agents --json` rows, transcripts, activity
  shell.rs         login-shell PATH, env scrubbing, claude/gh binaries
  codex.rs         Codex sessions: its local daemon, a native pane with Allow / Decline per permission
  pi.rs            pi sessions: `pi --session-id` in an Orbit terminal; pi-orbit.ts (its extension) reports to the status log and asks before what the rules do not allow; tests/pi-extension.test.mjs
  terminal.rs      PTY + vt100 → egui; `Title` keeps the OSC title, `glance()` reads the screen for screen.rs
  screen.rs        herdr-style screen reading of an attached terminal: title spinner / "esc to interrupt" = Working, permission or question prompt = Blocked, prompt box = Prompt; `Live::label()` reads it before the hooks
  status.rs        hook-written status log per session (working / needs-decision / needs-approval / turn / pr / failed / ended / note); `report` is the session's own line (`note` = its one-line summary), `progress_hint` ends every brief
  mod.rs           `seen` (key → when the user last had the session in front; a Turn before that reads idle, herdr's done vs idle), `notes` (started fresh: no transcript), `glance()` each frame
src/activity.rs    activity.jsonl: one line per Action (name, view, tab, via key/click, its `gate`, and for a decision `ctx`: the ticket/PR/session/notification it acted on as enums and counts, id hashed), never text; `GATES` says per Action who may take it (look / app / do / queue / yours), groundwork for an operator agent; the orbit-improve skill reads it
src/oauth.rs       Sign in with Linear: PKCE, loopback :53682, token refresh (no client secret)
src/secrets.rs     env var, else one Keychain entry (read once); ORBIT_NO_KEYCHAIN: <data>/keys.json instead
src/skills.rs      GENERIC skills (repo skills/: every core capability, incl. `orbit`: how a session reports to the app), each + the profile's skills/<folder>/EXTRA.md (with_extra); local only
src/profile.rs     the profile: everything company- or person-specific (bot + commands, role→skill map, KTLO rules, intake routing, prompts' extras, orchestrator, spotter, OAuth app, settings it brings); folder in place or a bundle unpacked; profile::get()
src/store.rs       data_dir(), load/save JSON (atomic), text_hash, Settings (camelCase, every field defaulted; debug envs, investigate, testAppUrl, keys)
src/theme.rs       4 palettes (phosphor/amber/graphite/paper) + system, p(), fonts (read from the Mac), egui style; the only file with hex colours
docs/decisions.md  append-only product decision log (see Rules)
docs/decision-guards.txt  what holds each decision (test / code anchor / replaced); src/guards.rs fails make check when one is missing or broken
vendor/option-ext/ MIT stand-in for option-ext (MPL-2.0), [patch.crates-io]
skills/            generic operate (Run the operator: works Orbit through orbit ctl, claude::operator), triage, plan, build, fix-pr-comments, land (`/land <PR>`: merge named PRs once CLEAN; Land it and KTLO's batch run it), fix-deploy (a red deploy's Fix it), spot (Work's spotter), goals (Plan my day's goals run), orchestrate (Run the board), profile (/profile: sets up the profile with the user),
                   orbit-improve (Cmd+Shift+F feedback: a session changes this app in a worktree and opens a PR).
                   Company facts go in the profile as skills/<folder>/EXTRA.md (e.g. the company's repo map), never here
```

## Rules

- Decisions: `docs/decisions.md` is an append-only log of product and
  user-facing decisions (features, UX, what views show, what skills do for
  the user; never code patterns or libraries). Read it before reworking a
  feature and keep what it records. When the user asks for a product
  behaviour, append it there in the same change as one line (date, `user`,
  area, the decision in their terms). A product tradeoff Claude makes gets
  a `claude` line with the reason. Never edit or delete a line; a changed
  decision is a new line naming the one it replaces. Only log what the user
  said or what was actually decided; never invent one.
- `src/ui/` never mutates `App`. Emit an `Action`; `actions.rs` applies it
  after the frame. Backend answers arrive as `Event`s in `events.rs`.
- Every decision line has a guard in `docs/decision-guards.txt` (src/guards.rs,
  `every_decision_has_a_guard_that_holds`): a test that checks the behaviour,
  a `code:` anchor for what is drawn, or `replaced:` naming the later line.
  A new decision gets its guard in the same change. A failing guard means a
  decided behaviour was removed: restore it, or log a new decision that
  replaces it; never delete the guard to make the test pass.
- Optimistic always: show the user's change at once, hold Linear's lagging
  answer (`board.rs`), never let something flicker away and come back.
- No blocking on the UI thread. Network and processes go through
  `backend.rs` or a thread in `claude/`.
- Read the module comment at the top of a file before changing its state
  machine; `claude/mod.rs` and `app/board.rs` carry the rules.
- Small footprint: reuse widgets in `ui/widgets.rs`, no new crate without a
  one-line reason next to it in `Cargo.toml`, no abstractions for one use.
- Every changed behaviour gets one focused test next to the code.
- Do not write em dashes in code, comments, or Linear text.
- Every user-facing feature has a tutorial step. Adding, changing or
  removing a feature means updating `STEPS` in `src/ui/tutorial.rs` in the
  same change: a step with the view it needs, an anchor recorded with
  `mark(app, ui, "<anchor>", rect)` where the feature draws, and two or
  three plain sentences on what it does and how to use it. The test
  `every_anchor_is_marked_in_the_ui` fails when an anchor is not marked.
  Check a step with `ORBIT_TUTORIAL=<n> make shot`.
- Parallel work goes in panes (Cmd+D right, Cmd+Shift+D down) and tabs
  (Cmd+N; an empty one offers Claude, a shell or a ticket). A second window is another copy of the app (palette: New
  window), never a second egui viewport in this one: it would load the
  board and hold sessions twice in one process. Each ticket pane is a full ticket view;
  the list and ticket actions work on the active one. A session view
  attaches with `Action::Attach`, which never changes what a pane shows.
- Several copies of the app may run at once (`open -n`, two `make run`).
  A file more than one may write is read-modify-write: `sessions.json`
  (`Sessions::persist`, re-read on every poll), `settings.json`, the
  Keychain entry (`secrets::set` re-reads it). A failed OAuth refresh
  takes the tokens another copy stored. Keep new shared state to the same rule.
- KTLO (`story::lane`: Socket/Security/CVE tickets, sweep alerts) never shows in a
  product view (For you and its day plan, Stories, Intake, needs you); it has the KTLO view.
  New product views filter on `Lane::Product` too.
- `src/ui/` never writes into `App`: `the_ui_never_writes_into_the_app` scans
  for it, and every key `ui/mod.rs keys()` consumes has a line in
  `ui::SHORTCUTS` (the palette's Keyboard shortcuts), checked by
  `every_key_the_window_takes_is_listed_as_a_shortcut`. Every settings.json
  field has a doc comment (`every_settings_field_has_a_doc_comment`).
- Every `Action` variant has a gate in `activity::GATES` (`every_action_has_a_gate`):
  `yours` is only ever the user's click (approve, merge, a permission answer),
  `queue` changes what others see, `do` starts work that asks first or is local.
  Classify a new action when you add it; never move one to `do` to save a click.
- A click target around other widgets is `widgets::clickable` (or `row`),
  never `.interact(Sense::click())` on a response after its contents:
  egui gives the click to the widget registered last, so every button and
  field inside goes dead. `no_click_added_after_contents` fails on it.
- Anything Laya decided or proposed carries `widgets::laya_chip` next to
  it, so it never passes for Haiku's answer or the user's own.
- Colours only through `theme::p()`; no corner radius, no filled cards
  (hairline rows, 2px marker on the selected one), metadata in mono.

## Safety

- Merging: only the user's click, only when `gh` says `CLEAN` at the head
  the user saw and the PR is not already in the merge queue, re-checked
  right before `gh pr merge --match-head-commit` (`--squash`, or no method
  where the base branch has a merge queue: the queue picks). Nothing merges
  on its own.
  Merge when ready (GitHub auto-merge) is the one deferred merge: the
  user's click, only while checks are all that is missing, bound with
  `expectedHeadOid` to the head they saw; a later gate at another head
  turns it off (`github::moved_past`).
  Land on KTLO's batch, and Land it on one reviewed PR, are the clicks
  that cover merging later: the landing session it starts merges each PR
  it covers with
  `gh pr merge --auto` once GitHub says CLEAN (approved, checks green),
  and again after the merge queue drops it (`claude::LANDING_ALLOW`);
  never `--admin`, never a PR outside what was clicked, never approving.
- Approving: only the user's click, only while the gate says their review
  is missing and the PR is not theirs, bound to the head they saw
  (reviews API `commit_id`), re-checked right before. Nothing approves on
  its own.
- Never log or print keys. Before committing, grep the diff for `lin_api_`
  and `sk-ant-`. `.env` is gitignored.
- Never `pkill -f claude` or `pkill -f orbit`: that kills the user's own
  sessions and app. Kill only PIDs you started.
- Never touch the user's real data dir from a script. Use
  `ORBIT_DATA=target/dev-data`.
- Nothing company-specific in this repo: names, teams, repos, channels,
  customers, URLs, prompts' examples and skills go in a profile
  (profile.rs); the app must work generic without one. Tests use
  explicit test profiles (`*_in(&Profile)`), never the global one.
  This repo is open-source ready: no company names, people, ticket ids,
  internal URLs or real data, in code, tests, docs or screenshots.
- A commit touches either generic code or a fork's overlay, never both
  (`make upstream` cherry-picks generic commits from a fork, docs/forks.md).
- A company fork = this repo + one overlay commit: `profiles/<company>`,
  `profiles/default`, `overlay.mk` (bundle id, release repo, release
  remotes; the Makefile includes it) and its own notes. Generic changes
  land here first and merge into the fork; a company need that the code
  cannot meet yet becomes a new profile field here, never a fork-only
  code change.
- Linear comments are commands: the bot's poller runs `@<bot> <Word>!`
  written by the user. Skills, the orchestrator, and any text the app or
  Claude writes must never contain that pattern. The one exception: the
  user picking "Remotely, <bot>" under a ticket step's ▼ arrow posts exactly
  `story::BotCommand::comment()` (the profile's `bot.planCommand` /
  `bot.buildCommand` / `bot.investigateCommand`, e.g. `@Bot Plan!`; the
  last only on a sweep's ticket whose sweep says `investigate`),
  nothing more. Driving a parent is that click for its own sub-tickets
  (`mission::kick`): one the drive handed the bot that sits quiet 30 min
  where the bot waits (plan done, no plan, a red PR) gets the same exact
  Plan or Build comment, once per state, and the user hears it; never
  while it builds, never a ticket the drive holds back.
- Linear notes: one attachment per ticket keyed by `NOTE_URL`; Linear
  upserts on URL, so writing again replaces, never duplicates.
- Test plans: one comment per ticket, carrying `mcr-test-plan:<issue id>`.
  The backend reads every comment on the ticket (`find_comment`) before
  and after asking Sonnet, and reuses a match; never post a plan any
  other way.

## Claude Code facts the code relies on

- Every session the app starts gets `--settings` with hooks that run this
  binary as `hook <data>/status/<key>.log`. `Live::label()` reads that log
  first, so "your turn" means Claude asked or finished. Test a hook with
  `printf '%s' '<event json>' | target/release/orbit hook x.log`.

- CLI 2.1.280+: `claude --bg`, `claude attach <short>`, `claude agents
  --json --all`, `claude stop`, `claude rm`.
- Transcripts: `~/.claude/projects/<cwd with non-alnum → '-'>/<id>.jsonl`.
  A session without a transcript cannot be resumed; start fresh instead.
- A child `claude` inherits `CLAUDECODE`-style env markers and then refuses
  to persist; `shell.rs::SCRUB` removes them.
- Launched from Finder the app has no PATH; `shell.rs` asks the login shell.
- A fresh folder hits the workspace-trust dialog, which a `--bg` session
  cannot answer. `shell::trust()` sets `hasTrustDialogAccepted` for the
  folder in `~/.claude.json` (or `$CLAUDE_CONFIG_DIR/.claude.json`) first.

## Screenshots

macOS stops drawing an occluded window, so a screenshot only works while
the window is frontmost. `scripts/shot.sh` handles that (osascript loop)
and converts PPM → PNG with `sips`. If it reports no frame, the window was
covered; run it again without moving the mouse.
