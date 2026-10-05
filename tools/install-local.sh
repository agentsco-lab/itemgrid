#!/bin/sh
# install-local.sh: Cradle for this user, under ~/.local - no root needed.
#   ~/.local/bin/cradle, ~/.local/bin/cradle-gui
#   ~/.local/share/cradle/duo-motion (for the phone: aarch64-linux-gnu-gcc)
#   ~/.local/share/applications/lab.agentsco.Cradle.desktop
#   ~/.local/share/icons/hicolor/scalable/apps/lab.agentsco.Cradle.svg
set -e
cd "$(dirname "$0")/.."
cargo build --release -p cradle -p cradle-gui
# duo-motion, for the phone (Cradle puts it there as it follows it).
cargo build --release --target aarch64-unknown-linux-gnu -p duo-motion
install -Dm755 target/aarch64-unknown-linux-gnu/release/duo-motion "$HOME/.local/share/cradle/duo-motion"
install -Dm755 target/release/cradle "$HOME/.local/bin/cradle"
install -Dm755 target/release/cradle-gui "$HOME/.local/bin/cradle-gui"
install -Dm644 data/lab.agentsco.Cradle.desktop "$HOME/.local/share/applications/lab.agentsco.Cradle.desktop"
install -Dm644 data/lab.agentsco.Cradle.svg "$HOME/.local/share/icons/hicolor/scalable/apps/lab.agentsco.Cradle.svg"
# The menu sees the launcher with the full path, also where ~/.local/bin is
# not on the session's PATH.
sed -i "s|^Exec=cradle-gui|Exec=$HOME/.local/bin/cradle-gui|" "$HOME/.local/share/applications/lab.agentsco.Cradle.desktop"
gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$HOME/.local/share/applications" 2>/dev/null || true
echo "Cradle installed for $USER: in the applications menu, or cradle-gui / cradle"
