#!/usr/bin/env bash
# Cuts a release: sets the version, commits, tags v<version>, and pushes
# main and the tag to `origin`, or to every remote in RELEASE_REMOTES
# (overlay.mk). GitHub Actions (.github/workflows/
# release.yml) then builds the app and publishes the zip.
#
#   scripts/release.sh 0.2.0        (or: make release V=0.2.0)
#   scripts/release.sh 0.0.1-rc1    a release candidate: a GitHub pre-release
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: scripts/release.sh <version>, e.g. 0.2.0}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc[0-9]+)?$ ]] || { echo "not a version: $version" >&2; exit 1; }
[ "$(git branch --show-current)" = main ] || { echo "release from main" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "commit or stash your changes first" >&2; exit 1; }
git rev-parse -q --verify "refs/tags/v$version" >/dev/null && { echo "v$version exists" >&2; exit 1; }

remotes=(${RELEASE_REMOTES:-origin})
# Up to date with the first remote it pushes to.
git pull --ff-only "${remotes[0]}" main
# The first `version =` is the package's; dependencies' come later.
perl -0pi -e "s/^version = \".*?\"/version = \"$version\"/m" Cargo.toml
cargo check --quiet   # brings Cargo.lock along
make check
git add Cargo.toml Cargo.lock
git commit -m "Release v$version"
git tag -a "v$version" -m "Orbit v$version"
for remote in "${remotes[@]}"; do
  git push "$remote" main "v$version"
done
echo "Tagged v$version. GitHub Actions on each pushed repo builds and publishes it."
