#!/usr/bin/env bash
# Builds "$NAME.app" (Orbit.app) for Apple silicon only, ad-hoc signs it,
# and zips it to dist/ for sharing. No Intel slice: macOS warns that an app
# with Intel code "will not work with a future release of macOS".
#
#   ./scripts/bundle-macos.sh
#
# The app is not notarized: recipients right-click it and choose Open once,
# or run: xattr -dr com.apple.quarantine "Orbit.app"
set -euo pipefail
cd "$(dirname "$0")/.."

# The app's name (src/main.rs NAME) and binary (Cargo.toml package name).
NAME="Orbit"
BIN="$(sed -n 's/^name = "\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
APP="dist/$NAME.app"

# Stamped into the binary; the sidebar and Settings show it next to the version.
export ORBIT_COMMIT="$(git rev-parse --short HEAD)"

rustup target add aarch64-apple-darwin >/dev/null
cargo build --release --target aarch64-apple-darwin

rm -rf "$APP" "dist/$BIN-$VERSION-macos.zip"
mkdir -p dist
scripts/app.sh "target/aarch64-apple-darwin/release/$BIN" "$APP"

ditto -c -k --keepParent "$APP" "dist/$BIN-$VERSION-macos.zip"
echo "Built dist/$BIN-$VERSION-macos.zip"
