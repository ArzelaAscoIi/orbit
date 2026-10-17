# Contributing

Orbit is meant to be forked and made yours ([docs/make-it-yours.md](docs/make-it-yours.md)).
Changes that help every team are welcome back here.

## Setup

```sh
make dev      # debug build against target/dev-data, never your real data
make watch    # rebuilt and restarted on every save (brew install watchexec)
make check    # fmt, clippy (warnings are errors), tests: what CI runs
```

Needs Rust (the version `rust-toolchain.toml` pins), Claude Code or Codex, and
`gh auth login`. A Linear key in `.env` skips the sign-in.

## The rules

[`CLAUDE.md`](CLAUDE.md) is the map and the rulebook, for agents and humans
alike. The ones that fail CI when broken:

- **Every product decision has a guard.** A changed behaviour gets a line in
  `docs/decisions.md` and a test or code anchor in `docs/decision-guards.txt`.
  A failing guard means a decided behaviour went missing: restore it, or log
  the decision that replaces it.
- **Every feature has a tutorial step** in `src/ui/tutorial.rs`.
- **Every action has a permission gate** in `src/activity.rs`. Approving and
  merging are only ever the user's click.
- **The UI never mutates state.** Views emit actions; `src/app/actions.rs`
  applies them after the frame.
- **Nothing company-specific.** Names, repos, URLs and prompts go in a
  profile, never in the code.

## Pull requests

One change per PR, `make check` green, the user-visible change in the
description in one or two sentences. The fastest way to make one: Cmd+Shift+F
in Orbit.
