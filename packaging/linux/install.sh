#!/bin/sh
# Installs SosiskaKillerLauncher for the current user (no root needed).
# Run with --uninstall to remove it.
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"; APPS="$HOME/.local/share/applications"; ICONS="$HOME/.local/share/icons/hicolor/256x256/apps"
if [ "$1" = "--uninstall" ]; then
  rm -f "$BIN/sosiskakillerlauncher" "$APPS/sosiskakillerlauncher.desktop" "$ICONS/sosiskakillerlauncher.png"
  echo "Removed. Game data stays in ~/.local/share/.skl"; exit 0
fi
mkdir -p "$BIN" "$APPS" "$ICONS"
install -m755 "$DIR/sosiskakillerlauncher" "$BIN/sosiskakillerlauncher"
install -m644 "$DIR/sosiskakillerlauncher.png" "$ICONS/sosiskakillerlauncher.png"
sed "s|^Exec=.*|Exec=$BIN/sosiskakillerlauncher|" "$DIR/sosiskakillerlauncher.desktop" > "$APPS/sosiskakillerlauncher.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" || true
echo "Installed: $BIN/sosiskakillerlauncher (also in the app menu)"
