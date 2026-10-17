#!/usr/bin/env bash
# Wraps a built binary into an ad-hoc signed app bundle:
#
#   scripts/app.sh <binary> <Name.app>
#
# `make bundle` uses it for the shared zip, `make run` for target/Orbit.app,
# so the app has a bundle id: Mac notifications are Orbit's own then and a
# click brings it forward (osascript's open Script Editor). BUNDLE_ID
# (overlay.mk) overrides the id.
set -euo pipefail
cd "$(dirname "$0")/.."

SRC="$1"
APP="$2"
NAME="$(basename "$APP" .app)"
BIN="$(sed -n 's/^name = "\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

mkdir -p "$APP/Contents/MacOS"
cp "$SRC" "$APP/Contents/MacOS/$BIN"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID:-dev.orbit.$BIN}</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION%%-*}</string>
  <key>CFBundleVersion</key><string>${VERSION%%-*}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

# lipo drops the linker's signatures; Apple Silicon refuses unsigned code.
codesign --force --deep --sign - "$APP" 2>/dev/null
