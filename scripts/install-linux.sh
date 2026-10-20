#!/usr/bin/env bash
# Installs Enki for the current user: binary in ~/.local/bin, plus the
# launcher entry and icon so it shows up in the GNOME app grid.
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release

BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/512x512/apps"
mkdir -p "$BIN" "$APPS" "$ICONS"

install -m 755 target/release/enki "$BIN/enki"
install -m 644 assets/icon/enki-512.png "$ICONS/enki.png"
sed "s|^Exec=enki|Exec=$BIN/enki|" assets/linux/enki.desktop > "$APPS/enki.desktop"

update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
echo "Enki instalado em $BIN/enki"
