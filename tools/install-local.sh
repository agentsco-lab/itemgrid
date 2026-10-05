#!/bin/sh
# install-local.sh: item/grid for this user, under ~/.local - no root needed.
#   ~/.local/bin/itemgrid, ~/.local/bin/itemgrid-gui, ~/.local/bin/itemgrid-mcp
#   ~/.local/share/itemgrid/duo-motion (for the phone: aarch64-linux-gnu-gcc)
#   ~/.local/share/applications/lab.agentsco.ItemGrid.desktop
#   ~/.local/share/icons/hicolor/scalable/apps/lab.agentsco.ItemGrid.svg
set -e
cd "$(dirname "$0")/.."
cargo build --release -p itemgrid -p itemgrid-gui -p itemgrid-mcp
# duo-motion, for the phone (item/grid puts it there as it follows it).
cargo build --release --target aarch64-unknown-linux-gnu -p duo-motion
install -Dm755 target/aarch64-unknown-linux-gnu/release/duo-motion "$HOME/.local/share/itemgrid/duo-motion"
install -Dm755 target/release/itemgrid "$HOME/.local/bin/itemgrid"
install -Dm755 target/release/itemgrid-gui "$HOME/.local/bin/itemgrid-gui"
# For Claude Code to look at the window and try it (claude mcp add itemgrid -- ~/.local/bin/itemgrid-mcp).
install -Dm755 target/release/itemgrid-mcp "$HOME/.local/bin/itemgrid-mcp"
install -Dm644 data/lab.agentsco.ItemGrid.desktop "$HOME/.local/share/applications/lab.agentsco.ItemGrid.desktop"
install -Dm644 data/lab.agentsco.ItemGrid.svg "$HOME/.local/share/icons/hicolor/scalable/apps/lab.agentsco.ItemGrid.svg"
# The menu sees the launcher with the full path, also where ~/.local/bin is
# not on the session's PATH.
sed -i "s|^Exec=itemgrid-gui|Exec=$HOME/.local/bin/itemgrid-gui|" "$HOME/.local/share/applications/lab.agentsco.ItemGrid.desktop"
gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$HOME/.local/share/applications" 2>/dev/null || true
echo "item/grid installed for $USER: in the applications menu, or itemgrid-gui / itemgrid"
