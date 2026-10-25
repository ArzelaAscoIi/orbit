#!/usr/bin/env bash
# Renders the app icon from assets/icon/orbit.svg: AppIcon.icns for the
# bundle (scripts/app.sh) and icon-256.png for the window (src/main.rs).
# macOS only: AppKit draws the SVG on a transparent canvas (Quick Look
# would fill the corners white), iconutil packs the icns.
set -euo pipefail
cd "$(dirname "$0")/../assets/icon"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
set_dir="$tmp/AppIcon.iconset"
mkdir "$set_dir"
cat > "$tmp/render.swift" <<'SWIFT'
import AppKit
let a = CommandLine.arguments
let n = Int(a[2])!
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: n, pixelsHigh: n,
  bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
  colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSImage(contentsOfFile: a[1])!.draw(in: NSRect(x: 0, y: 0, width: n, height: n))
NSGraphicsContext.current = nil
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[3]))
SWIFT
swiftc -O "$tmp/render.swift" -o "$tmp/render" 2>/dev/null
for size in 16 32 64 128 256 512 1024; do
  "$tmp/render" orbit.svg "$size" "$tmp/$size.png"
done
for size in 16 32 128 256 512; do
  cp "$tmp/$size.png" "$set_dir/icon_${size}x${size}.png"
  cp "$tmp/$((size * 2)).png" "$set_dir/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$set_dir" -o AppIcon.icns
cp "$tmp/256.png" icon-256.png
