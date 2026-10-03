#!/bin/sh
# package-deb.sh: Cradle as a .deb in target/deb/ - the command line, the
# window, its icon and desktop entry. Built for this machine's architecture.
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(dpkg --print-architecture)
cargo build --release -p cradle -p cradle-gui
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
install -Dm755 target/release/cradle "$STAGE/usr/bin/cradle"
install -Dm755 target/release/cradle-gui "$STAGE/usr/bin/cradle-gui"
strip "$STAGE/usr/bin/cradle" "$STAGE/usr/bin/cradle-gui"
install -Dm644 data/lab.agentsco.Cradle.desktop "$STAGE/usr/share/applications/lab.agentsco.Cradle.desktop"
install -Dm644 data/lab.agentsco.Cradle.svg "$STAGE/usr/share/icons/hicolor/scalable/apps/lab.agentsco.Cradle.svg"
install -Dm644 README.md "$STAGE/usr/share/doc/cradle/README.md"
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: cradle
Version: $VERSION
Architecture: $ARCH
Maintainer: agentsco-lab
Section: utils
Priority: optional
Depends: libc6, libgtk-4-1 (>= 4.12), libadwaita-1-0 (>= 1.5), openssh-client
Recommends: adb, fastboot
Description: look after a connected Surface Duo
 Cradle shows what a Surface Duo running Linux (Droidian and item) is doing
 - its versions, battery, heat, storage and screens - and updates item,
 restarts it, reads its journal and takes screenshots, with the device's
 safety rules built in. A command line (cradle) and a window (cradle-gui).
CONTROL
mkdir -p target/deb
OUT=target/deb/cradle_${VERSION}_${ARCH}.deb
dpkg-deb --root-owner-group --build "$STAGE" "$OUT" >/dev/null
echo "$OUT"
