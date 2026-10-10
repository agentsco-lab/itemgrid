#!/bin/bash
# package-windows.sh: the Windows installer (crates/install) cross-built here
# and zipped for a release: itemgrid-install-<version>-windows.zip with
# itemgrid-install.exe and a README.txt.
#
#   apt install mingw-w64; rustup target add x86_64-pc-windows-gnu
#   tools/package-windows.sh            -> target/windows/itemgrid-install-<ver>-windows.zip
#
# The exe brings adb and fastboot (Google's platform-tools) and the system
# image here by itself when it runs; nothing else ships with it.
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
cd "$HERE"
export PATH="$HOME/.cargo/bin:$PATH"
VER=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
TARGET=x86_64-pc-windows-gnu
cargo build --release -p itemgrid-install --target $TARGET
OUT="$HERE/target/windows"
rm -rf "$OUT"; mkdir -p "$OUT/stage"
x86_64-w64-mingw32-strip -o "$OUT/stage/itemgrid-install.exe" "target/$TARGET/release/itemgrid-install.exe"
cat > "$OUT/stage/README.txt" <<EOF
item/grid install $VER - item on a Surface Duo 1, from Windows

What it does: puts item (https://github.com/agentsco-lab/item) on a Surface
Duo 1 that runs stock Android, in one go, about 15 minutes on the cable. The
phone's Android and everything on it are ERASED, and its bootloader is
unlocked. Until 1.0, item is an experiment: nothing is promised to work on
your phone.

Before: on the phone, Settings > About > tap Build number 7 times, then
Settings > System > Developer options > USB debugging on. Plug the phone into
this computer with a USB cable. Windows needs a driver for the phone's
bootloader (fastboot): if the install stops at "into the bootloader" with
no phone seen, open Device Manager, find "Android" with a warning sign, and
let Windows Update find a driver, or install Google's USB driver
(https://developer.android.com/studio/run/win-usb).

Run: double-click itemgrid-install.exe and follow it. It fetches adb and
fastboot (Google's platform-tools) and the system image (1.3 GB) first; on
the phone you confirm the unlock with the volume keys and Power; once
during the install you hold the power button when told.

Where it keeps things: %LOCALAPPDATA%\\itemgrid (the image, the tools, an
ssh key for item/grid).

Source: https://github.com/agentsco-lab/itemgrid (crates/install), MIT.
EOF
ZIP="$OUT/itemgrid-install-$VER-windows.zip"
(cd "$OUT/stage" && zip -q -9 "$ZIP" itemgrid-install.exe README.txt)
(cd "$OUT" && sha256sum "$(basename "$ZIP")" > SHA256SUMS)
rm -rf "$OUT/stage"
echo "OK: $ZIP"
cat "$OUT/SHA256SUMS"
