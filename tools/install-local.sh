#!/bin/sh
# install-local.sh: Gridbay for this user, under ~/.local - no root needed.
#   ~/.local/bin/gridbay, ~/.local/bin/gridbay-gui, ~/.local/bin/gridbay-mcp
#   ~/.local/share/gridbay/duo-motion (for the phone: aarch64-linux-gnu-gcc)
#   ~/.local/share/applications/lab.agentsco.Gridbay.desktop
#   ~/.local/share/icons/hicolor/scalable/apps/lab.agentsco.Gridbay.svg
set -e
cd "$(dirname "$0")/.."
cargo build --release -p gridbay -p gridbay-gui -p gridbay-mcp
# duo-motion, for the phone (Gridbay puts it there as it follows it).
cargo build --release --target aarch64-unknown-linux-gnu -p duo-motion
install -Dm755 target/aarch64-unknown-linux-gnu/release/duo-motion "$HOME/.local/share/gridbay/duo-motion"
install -Dm755 target/release/gridbay "$HOME/.local/bin/gridbay"
install -Dm755 target/release/gridbay-gui "$HOME/.local/bin/gridbay-gui"
# For Claude Code to look at the window and try it (claude mcp add gridbay -- ~/.local/bin/gridbay-mcp).
install -Dm755 target/release/gridbay-mcp "$HOME/.local/bin/gridbay-mcp"
install -Dm644 data/lab.agentsco.Gridbay.desktop "$HOME/.local/share/applications/lab.agentsco.Gridbay.desktop"
install -Dm644 data/lab.agentsco.Gridbay.svg "$HOME/.local/share/icons/hicolor/scalable/apps/lab.agentsco.Gridbay.svg"
# The menu sees the launcher with the full path, also where ~/.local/bin is
# not on the session's PATH.
sed -i "s|^Exec=gridbay-gui|Exec=$HOME/.local/bin/gridbay-gui|" "$HOME/.local/share/applications/lab.agentsco.Gridbay.desktop"
gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$HOME/.local/share/applications" 2>/dev/null || true
echo "Gridbay installed for $USER: in the applications menu, or gridbay-gui / gridbay"
