#!/bin/sh
# install-local.sh: Hythe for this user, under ~/.local - no root needed.
#   ~/.local/bin/hythe, ~/.local/bin/hythe-gui, ~/.local/bin/hythe-mcp
#   ~/.local/share/hythe/duo-motion (for the phone: aarch64-linux-gnu-gcc)
#   ~/.local/share/applications/lab.agentsco.Hythe.desktop
#   ~/.local/share/icons/hicolor/scalable/apps/lab.agentsco.Hythe.svg
set -e
cd "$(dirname "$0")/.."
cargo build --release -p hythe -p hythe-gui -p hythe-mcp
# duo-motion, for the phone (Hythe puts it there as it follows it).
cargo build --release --target aarch64-unknown-linux-gnu -p duo-motion
install -Dm755 target/aarch64-unknown-linux-gnu/release/duo-motion "$HOME/.local/share/hythe/duo-motion"
install -Dm755 target/release/hythe "$HOME/.local/bin/hythe"
install -Dm755 target/release/hythe-gui "$HOME/.local/bin/hythe-gui"
# For Claude Code to look at the window and try it (claude mcp add hythe -- ~/.local/bin/hythe-mcp).
install -Dm755 target/release/hythe-mcp "$HOME/.local/bin/hythe-mcp"
install -Dm644 data/lab.agentsco.Hythe.desktop "$HOME/.local/share/applications/lab.agentsco.Hythe.desktop"
install -Dm644 data/lab.agentsco.Hythe.svg "$HOME/.local/share/icons/hicolor/scalable/apps/lab.agentsco.Hythe.svg"
# The menu sees the launcher with the full path, also where ~/.local/bin is
# not on the session's PATH.
sed -i "s|^Exec=hythe-gui|Exec=$HOME/.local/bin/hythe-gui|" "$HOME/.local/share/applications/lab.agentsco.Hythe.desktop"
gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$HOME/.local/share/applications" 2>/dev/null || true
echo "Hythe installed for $USER: in the applications menu, or hythe-gui / hythe"
