# Make it yours

Orbit is meant to be forked. It is one engineer's daily tool, made generic.
The fastest way to get value from it is to make it your team's.

## Why fork instead of configure

Agents write code now. A setting covers what the author thought of; a fork
covers what your team needs. Orbit keeps the line clean:

- **Configuration** (a profile) holds everything company-specific: your bot,
  its commands, routing, what counts as maintenance, your skills.
- **Code** is generic, small and readable: ~50k lines of Rust with one rule
  file (`CLAUDE.md`) Claude Code follows. Most changes are a prompt.

## 1. A profile: your team in one folder

```
profiles/<team>/profile.json              the bot and its commands, GitHub org, routing, maintenance rules
profiles/<team>/skills/<skill>/EXTRA.md   your additions to Orbit's own skills (triage, plan, build, ...)
profiles/<team>/skills/<own>/SKILL.md     skills of your own
profiles/default                          one line: the profile a new install starts with
```

A complete example, used by the tests: [`tests/profiles/acme`](../tests/profiles/acme)
(a bot with commands, routing, maintenance rules, skill extras, k9s views).

Every folder under `profiles/` is compiled in. No profile at all: Orbit is a
plain Linear client with an agent per ticket.

Don't want to write JSON? Settings → **Set up with Claude** asks you a few
questions and writes it. Bot commands, AI review, previews and the test app
are plain fields in Settings → **bot & PRs**.

Example: teach the build skill your repo layout.

```md
<!-- profiles/acme/skills/build/EXTRA.md -->
The API lives in acme/api (Python, uv), the UI in acme/web (pnpm).
An API change that the UI uses is one PR per repo, API first.
Run `make test` before every push.
```

## 2. Change the code by asking

- **Cmd+Shift+F** in Orbit: tell it what annoys you. A Claude session reads
  what you just did (a local log of actions, never text), changes Orbit in a
  worktree, runs `make check` and opens a PR on your fork.
- Or open your fork in Claude Code. `CLAUDE.md` holds the map and the rules:
  UI never mutates state, every action has a permission gate, every product
  decision has a test.

### Example: Jira instead of Linear

Not supported. One prompt:

> Add a Jira backend next to src/linear.rs with the same functions (home,
> set_state, add_comment, save_note). Pick it in settings.json `tracker`.
> Keep Linear the default.

Linear's client is one file with a handful of queries. The rest of the app
works on cards, not on Linear types.

## 3. Keep your fork current

Your fork = this repo + your commits. Keep company specifics in
`profiles/<team>/` and an optional `overlay.mk` (bundle id, release repo), and
upstream changes merge cleanly:

```sh
git remote add upstream https://github.com/ArzelaAscoIi/orbit
git pull upstream main
```

Something generic you built? Send it back as a PR.

Fixed something generic in your fork? `make upstream C="<sha>"` sends it
back as a PR. The whole flow, both ways: [running a fork](forks.md).

## 4. Ship it to your team

```sh
make release V=0.1.0     # tag + push; GitHub Actions builds and publishes the zip
```

Set `ORBIT_RELEASES_REPO` (or `overlay.mk`) to your fork, and Orbit's
**Update** button pulls your releases with each user's own `gh` login.

## What it costs

Honestly: little. The fork that Orbit came from is maintained in spare
minutes, mostly by Orbit itself (Cmd+Shift+F). The decision log and its tests
stop an agent from quietly undoing what you decided. And it is fun: your tool
improves the moment you notice what is wrong with it.
