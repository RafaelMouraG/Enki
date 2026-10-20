#!/usr/bin/env bash
# Builds target/release/Enki.app: release binary, icon and Info.plist,
# ad-hoc signed so it runs on Apple Silicon without a developer account.
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release

APP=target/release/Enki.app
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/enki "$APP/Contents/MacOS/enki"

ICONSET=$(mktemp -d)/Enki.iconset
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z $size $size assets/icon/enki.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) assets/icon/enki.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Enki.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Enki</string>
  <key>CFBundleDisplayName</key><string>Enki</string>
  <key>CFBundleIdentifier</key><string>io.github.rafaelmourag.enki</string>
  <key>CFBundleExecutable</key><string>enki</string>
  <key>CFBundleIconFile</key><string>Enki</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>O Enki controla o Spotify para mostrar e tocar a música atual.</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
echo "Pronto: $APP"
