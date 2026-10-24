#!/usr/bin/env bash
# Renders the app icon from assets/icon/orbit.svg: AppIcon.icns for the
# bundle (scripts/app.sh) and icon-256.png for the window (src/main.rs).
# macOS only: Quick Look draws the SVG, iconutil packs the icns.
set -euo pipefail
cd "$(dirname "$0")/../assets/icon"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
set_dir="$tmp/AppIcon.iconset"
mkdir "$set_dir"
for size in 16 32 64 128 256 512 1024; do
  qlmanage -t -s "$size" -o "$tmp" orbit.svg >/dev/null 2>&1
  mv "$tmp/orbit.svg.png" "$tmp/$size.png"
done
for size in 16 32 128 256 512; do
  cp "$tmp/$size.png" "$set_dir/icon_${size}x${size}.png"
  cp "$tmp/$((size * 2)).png" "$set_dir/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$set_dir" -o AppIcon.icns
cp "$tmp/256.png" icon-256.png
