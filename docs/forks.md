# Running a company fork

A fork is this repo plus a thin layer only you have: your profile, your build
settings, your notes. Kept that way, improvements flow both ways with two
commands.

```
 upstream (public)           your fork (private)
 ─────────────────           ───────────────────
 generic code      ── sync ─▶  generic code
                               + profiles/<you>/, overlay.mk, notes   (the overlay)
 generic fix PR    ◀ upstream ─ a generic fix made in the fork
```

## The overlay

Everything company-specific lives in paths upstream never has:

| Path | What |
|---|---|
| `profiles/<you>/` | your profile: bot, routing, skills, k9s, your own decisions |
| `profiles/default` | one line, your profile's name |
| `overlay.mk` | bundle id, release repo, remotes, and the settings below |
| `.github/README.md` | your fork's README (GitHub shows it before the root one) |
| `.github/workflows/sync-upstream.yml` | the scheduled sync below |
| anything else only you have | notes, screenshots with real data |

Upstream never edits these, so merging upstream never conflicts with them.
The one rule: **a commit touches either the overlay or generic code, never
both.** That is what makes sending fixes back a cherry-pick.

```make
# overlay.mk
export UPSTREAM_REPO := acme-oss/orbit
export OVERLAY_PATHS := profiles overlay.mk .github/README.md .github/workflows/sync-upstream.yml notes
export UPSTREAM_DENY := profiles/acme/upstream-deny.txt
```

`upstream-deny.txt` holds extended regexes, one per line, that must never
reach the public repo: your company's name, internal hosts, team keys,
customer names.

```
acme
\bacme-internal\.
\bOPS-[0-9]+
```

## Upstream to fork: `make sync`

```sh
make sync     # merge upstream's main into this branch, then make check
```

Then push the branch and merge it in the fork like any PR. Upstream's
changes never touch the overlay, so this is a fast, clean merge.

To have it happen on its own, put this workflow in the fork (it is part of
the overlay, upstream does not have it). Every Monday it merges upstream
into `sync/upstream` and opens or updates a PR; CI then runs on it as usual.

```yaml
# .github/workflows/sync-upstream.yml (in the fork only)
name: sync upstream
on:
  schedule: [{ cron: "0 6 * * 1" }]
  workflow_dispatch:
permissions:
  contents: write
  pull-requests: write
jobs:
  sync:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with: { fetch-depth: 0 }
      - name: Merge upstream main
        env:
          GH_TOKEN: ${{ github.token }}
          UPSTREAM: acme-oss/orbit
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          git remote add upstream "https://github.com/$UPSTREAM.git"
          git fetch upstream main
          git switch -C sync/upstream
          git merge --no-edit upstream/main
          git push -f origin sync/upstream
          gh pr view sync/upstream >/dev/null 2>&1 \
            || gh pr create --head sync/upstream --title "Sync upstream" --body "Merges $UPSTREAM main."
```

A merge conflict fails the run: someone edited generic code in the fork.
Send that change upstream (below), and the next sync is clean again.

## Fork to upstream: `make upstream`

You fixed a bug or built a feature in the fork that every team could use:

```sh
make upstream C="3f2a1bc 9d8e7f6"
```

It cherry-picks those commits onto upstream's main in a new branch, and
refuses when one of them touches the overlay or when the change or its
message matches `UPSTREAM_DENY`. Then it runs `make check`, pushes the
branch upstream and opens the PR. Once upstream merges it, the next
`make sync` brings it back as the same change.

A commit that mixes both? Split it first: `git reset HEAD~` and commit the
generic part and the overlay part separately.

## Working day to day

- **Generic change:** make it upstream first (or `make upstream` right after).
  The fork gets it with the next sync.
- **Company change** (your bot's commands, a repo map, a skill): edit
  `profiles/<you>/` in the fork. It never goes upstream.
- **Cmd+Shift+F in the fork's build** opens its PR on the fork
  (`ORBIT_RELEASES_REPO`). For a generic improvement, send it upstream
  afterwards with `make upstream`.
