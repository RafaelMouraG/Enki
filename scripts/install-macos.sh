#!/usr/bin/env bash
# Installs Enki.app into /Applications so it shows up in Launchpad and
# Spotlight. Pass --dock to also pin it to the Dock.
set -euo pipefail

cd "$(dirname "$0")/.."
./scripts/bundle-macos.sh

DEST=/Applications/Enki.app
osascript -e 'quit app "Enki"' 2>/dev/null || true
rm -rf "$DEST"
ditto target/release/Enki.app "$DEST"

# Register with Launch Services so Spotlight/Launchpad pick it up right away.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DEST"

if [[ "${1:-}" == "--dock" ]] && ! defaults read com.apple.dock persistent-apps | grep -q "$DEST"; then
  defaults write com.apple.dock persistent-apps -array-add \
    "<dict><key>tile-data</key><dict><key>file-data</key><dict><key>_CFURLString</key><string>file://$DEST/</string><key>_CFURLStringType</key><integer>15</integer></dict></dict></dict>"
  killall Dock
fi

echo "Enki instalado em $DEST"
