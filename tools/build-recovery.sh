#!/bin/sh
# build-recovery.sh: item/grid's own recovery for the Duo, booted from RAM
# (fastboot boot) to put a system on the phone or take it off - in place of
# TWRP. A boot image of the port's kernel and device tree as they are, with
# an initramfs of:
#   /init         item-recovery (crates/recovery): init, the USB gadget, adbd,
#                 and every command item/grid asks of it (its verbs: no shell,
#                 no busybox, no mke2fs - its own mkfs)
#   the font      item/grid's (Open Sans Light, OFL) for the screen
# Nothing is downloaded. Out: target/recovery/item-recovery.img, and a copy
# in ~/.local/share/itemgrid/recovery/ where item/grid finds it.
#
#   tools/build-recovery.sh [PORT_TREE]
#     BOOT_IMG=...     the port's boot image (default: its perf-127-lid)
set -eu
cd "$(dirname "$0")/.."
TREE=$(pwd)
PORT=${1:-$HOME/Desktop/projects/surfaceduo/surfaceduo-droidian}
BOOT_IMG=${BOOT_IMG:-$PORT/out/boot-duo1-droidian-perf-127-lid.img}
W=$TREE/target/recovery
die() { echo "build-recovery: $*" >&2; exit 1; }
[ -f "$BOOT_IMG" ] || die "no boot image at $BOOT_IMG"
for t in cpio gzip; do
    command -v $t >/dev/null || die "$t is wanted here"
done
mkdir -p "$W"

echo "== itemgrid here, for the boot images (its bootimg: in place of AOSP's mkbootimg)"
cargo build --release -p itemgrid
IG=$TREE/target/release/itemgrid

echo "== item-recovery, static for the phone"
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target aarch64-unknown-linux-gnu -p item-recovery
BIN=$TREE/target/aarch64-unknown-linux-gnu/release/item-recovery
aarch64-linux-gnu-strip -o "$W/item-recovery" "$BIN"

echo "== the initramfs"
R=$W/root
rm -rf "$R" && mkdir -p "$R"/sbin "$R"/etc "$R"/proc "$R"/sys "$R"/dev "$R"/tmp "$R"/run "$R"/mnt
install -m755 "$W/item-recovery" "$R/sbin/item-recovery"
ln -s sbin/item-recovery "$R/init"
# The screen's letters: item/grid's own font (Open Sans Light, OFL).
FONT=${FONT:-$HOME/.local/share/fonts/itemgrid/OpenSans-Light.ttf}
[ -f "$FONT" ] || die "no font at $FONT (tools/fetch-fonts.sh)"
install -Dm644 "$FONT" "$R/usr/share/item-recovery/font.ttf"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
printf 'item-recovery %s %s\n' "$VERSION" "$(git rev-parse --short HEAD 2>/dev/null || echo -)" > "$R/etc/item-recovery"
(cd "$R" && find . | LC_ALL=C sort | cpio -o -H newc -R 0:0 --quiet | gzip -9n) > "$W/ramdisk.gz"

echo "== the boot image"
# The port's header, kernel, device tree and command line, the ramdisk
# ours; Droidian's own words left out of the command line.
"$IG" bootimg repack "$BOOT_IMG" --ramdisk "$W/ramdisk.gz" --drop datapart= --drop droidian. --out "$W/item-recovery.img" >/dev/null
mkdir -p "$HOME/.local/share/itemgrid/recovery"
cp "$W/item-recovery.img" "$HOME/.local/share/itemgrid/recovery/item-recovery.img"
echo "item-recovery.img: $(($(stat -c %s "$W/item-recovery.img") >> 20)) MB ($(($(stat -c %s "$W/ramdisk.gz") >> 10)) KB of initramfs), in ~/.local/share/itemgrid/recovery/"
