#!/usr/bin/env bash
# A company fork and the upstream repo, both ways (docs/forks.md).
#
#   scripts/fork.sh sync [--no-check] merge upstream's main into this branch, then make check
#                                     (relinks first when a squash dropped upstream's history)
#   scripts/fork.sh upstream <commit>...
#                                     open a PR upstream with these commits, cherry-picked
#                                     onto upstream's main; refused when one touches the
#                                     fork's own files or carries one of its private words
#
# Config, from the fork's overlay.mk:
#   UPSTREAM_REPO   owner/repo of upstream (required)
#   OVERLAY_PATHS   paths only the fork has (default: profiles overlay.mk)
#   UPSTREAM_DENY   file of extended regexes, one per line, that must never
#                   go upstream (e.g. the company's name, hosts, team keys)
set -euo pipefail
cd "$(dirname "$0")/.."
repo=${UPSTREAM_REPO:?set UPSTREAM_REPO=owner/repo in overlay.mk}
paths=${OVERLAY_PATHS:-profiles overlay.mk}
deny=${UPSTREAM_DENY:-}

# The remote that points at upstream, whatever this clone calls it.
# Exact repo only: owner/repo must not also match owner/repo-private.
remote=$(git remote -v | awk -v r="${repo%.git}" '$3 == "(fetch)" {
  u = $2; sub(/\.git$/, "", u); sub(/\/$/, "", u)
  n = length(u) - length(r)
  if (u == r || (n > 0 && substr(u, n + 1) == r && substr(u, n, 1) ~ /[:\/]/)) { print $1; exit }
}')
if [ -z "$remote" ]; then
  remote=upstream
  git remote add "$remote" "https://github.com/$repo.git"
fi
git diff --quiet && git diff --cached --quiet || { echo "commit or stash your changes first" >&2; exit 1; }
git fetch -q "$remote"

# A squash-merged sync PR drops upstream's commits from the fork's history,
# so git would see unrelated histories. Before merging: find the newest
# upstream commit the fork already matches outside its overlay and, when it
# is not yet an ancestor, record it as merged (keeping the fork's files).
relink() {
  local exclude=() c
  for p in $paths; do exclude+=(":(exclude)$p"); done
  for c in $(git rev-list "$remote/main"); do
    if [ -z "$(git diff --name-only "$c" HEAD -- . "${exclude[@]}")" ]; then
      git merge-base --is-ancestor "$c" HEAD && return 0
      git merge -q -s ours --allow-unrelated-histories --no-edit "$c" \
        -m "Record upstream $(git rev-parse --short "$c") as merged (its content is already here)"
      return 0
    fi
  done
}

case "${1:-}" in
  sync)
    relink
    git merge --no-edit "$remote/main"
    if [ "${2:-}" = "--no-check" ]; then
      echo "Merged $repo main (not checked)."
    else
      make check
      echo "Merged $repo main. Push this branch and open its PR in the fork."
    fi
    ;;
  upstream)
    shift
    [ $# -gt 0 ] || { echo "usage: scripts/fork.sh upstream <commit>..." >&2; exit 2; }
    # As SHAs now: HEAD and branch names mean something else after the switch.
    shas=()
    for c in "$@"; do shas+=("$(git rev-parse --verify "$c^{commit}")"); done
    set -- "${shas[@]}"
    for c in "$@"; do
      # shellcheck disable=SC2086
      own=$(git diff-tree --no-commit-id --name-only -r "$c" -- $paths)
      [ -z "$own" ] || { echo "$c touches the fork's own files ($own): split it first" >&2; exit 1; }
    done
    # The word list is part of the overlay, which upstream's branch lacks: copy it first.
    words=$(mktemp)
    if [ -n "$deny" ]; then cp "$deny" "$words"; fi
    start=$(git rev-parse --abbrev-ref HEAD)
    branch="fork/$(git log -1 --format=%f "$1" | cut -c1-50)"
    git switch -q -c "$branch" "$remote/main"
    # Back on the user's branch whatever happens next; a conflict stays to resolve.
    trap 'git switch -q "$start"' EXIT
    if ! git cherry-pick "$@"; then
      trap - EXIT
      echo "Conflict. Resolve, git cherry-pick --continue, then: make check && git push -u $remote $branch" >&2
      exit 1
    fi
    if [ -n "$deny" ]; then
      # perl, not grep: \b and friends mean the same on macOS and Linux.
      leaks=$( { git diff "$remote/main"..HEAD | grep '^+' || true; git log "$remote/main"..HEAD --format=%B; } \
        | perl -e 'open my $f, "<", shift or die "no $ARGV[0]\n"; my @p = grep { length } map { chomp; $_ } <$f>;
                   while (my $l = <STDIN>) { for my $p (@p) { if ($l =~ /$p/i) { print "$p: $l"; last } } }' "$words")
      if [ -n "$leaks" ]; then
        git switch -q "$start" && git branch -q -D "$branch"
        trap - EXIT
        printf 'Refused, private words in the change or its message:\n%s\n' "$leaks" >&2
        exit 1
      fi
    fi
    make check
    git push -u "$remote" "$branch"
    gh pr create -R "$repo" --head "$branch" --fill
    ;;
  *)
    sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
