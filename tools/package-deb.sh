#!/bin/sh
# package-deb.sh: item/grid as a .deb in target/deb/ - the command line, the
# window, its icon and desktop entry. Built for this machine's architecture.
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(dpkg --print-architecture)
cargo build --release -p itemgrid -p itemgrid-gui
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
install -Dm755 target/release/itemgrid "$STAGE/usr/bin/itemgrid"
install -Dm755 target/release/itemgrid-gui "$STAGE/usr/bin/itemgrid-gui"
strip "$STAGE/usr/bin/itemgrid" "$STAGE/usr/bin/itemgrid-gui"
install -Dm644 data/lab.agentsco.ItemGrid.desktop "$STAGE/usr/share/applications/lab.agentsco.ItemGrid.desktop"
install -Dm644 data/lab.agentsco.ItemGrid.svg "$STAGE/usr/share/icons/hicolor/scalable/apps/lab.agentsco.ItemGrid.svg"
install -Dm644 README.md "$STAGE/usr/share/doc/itemgrid/README.md"
# WebKit's sandbox for the Microsoft window needs user namespaces (Ubuntu 24.04).
install -Dm644 data/apparmor/itemgrid-gui "$STAGE/etc/apparmor.d/itemgrid-gui"
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: itemgrid
Version: $VERSION
Architecture: $ARCH
Maintainer: agentsco-lab
Section: utils
Priority: optional
Depends: libc6, libgtk-4-1 (>= 4.12), libadwaita-1-0 (>= 1.5), libwebkitgtk-6.0-4, openssh-client
Recommends: adb, fastboot
Description: look after a connected Surface Duo
 item/grid shows what a Surface Duo running Linux (Droidian and item) is doing
 - its versions, battery, heat, storage and screens - and updates item,
 restarts it, reads its journal and takes screenshots, with the device's
 safety rules built in. A command line (itemgrid) and a window (itemgrid-gui).
CONTROL
cat > "$STAGE/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if [ "$1" = configure ] && command -v apparmor_parser >/dev/null && [ -d /sys/kernel/security/apparmor ]; then
  apparmor_parser -r -T -W /etc/apparmor.d/itemgrid-gui || true
fi
POSTINST
chmod 755 "$STAGE/DEBIAN/postinst"
mkdir -p target/deb
OUT=target/deb/itemgrid_${VERSION}_${ARCH}.deb
dpkg-deb --root-owner-group --build "$STAGE" "$OUT" >/dev/null
echo "$OUT"
