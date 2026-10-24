#!/bin/sh
# build-recovery.sh: item/grid's own recovery for the Duo, booted from RAM
# (fastboot boot) to put a system on the phone or take it off - in place of
# TWRP. A boot image of the port's kernel and device tree as they are, with
# an initramfs of:
#   /init         item-recovery (crates/recovery): init, the USB gadget, adbd
#   /bin/busybox  the shell and its tools, from the port's own initramfs
#   /bin/mke2fs   e2fsprogs' mke2fs with its libraries, from the release image
#   the font      item/grid's (Open Sans Light, OFL) for the screen
# Nothing is downloaded. Out: target/recovery/item-recovery.img, and a copy
# in ~/.local/share/itemgrid/recovery/ where item/grid finds it.
#
#   tools/build-recovery.sh [PORT_TREE]
#     BOOT_IMG=...     the port's boot image (default: its perf-127-lid)
#     ROOTFS_ZST=...   a release's rootfs.img.zst (default: the newest)
set -eu
cd "$(dirname "$0")/.."
TREE=$(pwd)
PORT=${1:-$HOME/Desktop/projects/surfaceduo/surfaceduo-droidian}
BOOT_IMG=${BOOT_IMG:-$PORT/out/boot-duo1-droidian-perf-127-lid.img}
ROOTFS_ZST=${ROOTFS_ZST:-$(ls -t "$PORT"/out/release/*/rootfs.img.zst 2>/dev/null | head -1)}
MKBOOTIMG=$PORT/tools/mkbootimg
W=$TREE/target/recovery
die() { echo "build-recovery: $*" >&2; exit 1; }
[ -f "$BOOT_IMG" ] || die "no boot image at $BOOT_IMG"
[ -f "$ROOTFS_ZST" ] || die "no release rootfs.img.zst (ROOTFS_ZST=)"
[ -f "$MKBOOTIMG/mkbootimg.py" ] || die "no mkbootimg in $MKBOOTIMG"
for t in debugfs zstd cpio gzip readelf python3; do
    command -v $t >/dev/null || die "$t is wanted here"
done
mkdir -p "$W"

echo "== item-recovery, static for the phone"
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target aarch64-unknown-linux-gnu -p item-recovery
BIN=$TREE/target/aarch64-unknown-linux-gnu/release/item-recovery
aarch64-linux-gnu-strip -o "$W/item-recovery" "$BIN"

echo "== the port's boot image unpacked"
rm -rf "$W/boot" && mkdir -p "$W/boot"
python3 -I "$MKBOOTIMG/unpack_bootimg.py" --boot_img "$BOOT_IMG" --out "$W/boot" --format=mkbootimg > "$W/boot/args"
mkdir -p "$W/boot/rd"
(cd "$W/boot/rd" && zcat ../ramdisk | cpio -idm --quiet --no-absolute-filenames 2>/dev/null)
[ -x "$W/boot/rd/bin/busybox" ] || die "no busybox in the port's initramfs"

echo "== mke2fs from $(basename "$(dirname "$ROOTFS_ZST")")"
E2=$W/e2fs
STAMP="$(stat -c %Y "$ROOTFS_ZST") $ROOTFS_ZST"
if [ "$(cat "$E2/.from" 2>/dev/null)" != "$STAMP" ]; then
    rm -rf "$E2" && mkdir -p "$E2"
    zstd -q -d -f "$ROOTFS_ZST" -o "$W/rootfs.img"
    python3 -I - "$W/rootfs.img" "$E2" <<'PY'
import os, re, subprocess, sys
img, out = sys.argv[1], sys.argv[2]
def dbg(cmd):
    return subprocess.run(["debugfs", "-R", cmd, img], capture_output=True, text=True).stdout
def resolve(path, depth=0):
    # Symlinks followed inside the image (debugfs's stat shows a fast link).
    m = re.search(r'Fast link dest: "([^"]*)"', dbg(f"stat {path}"))
    if not m or depth > 8:
        return path
    dest = m.group(1)
    if not dest.startswith("/"):
        dest = os.path.normpath(os.path.join(os.path.dirname(path), dest))
    return resolve(dest, depth + 1)
def dump(path, name):
    real = resolve(path)
    subprocess.run(["debugfs", "-R", f"dump {real} {out}/{name}", img], check=True, capture_output=True)
    if not os.path.getsize(f"{out}/{name}"):
        sys.exit(f"{path} ({real}): empty")
dump("/usr/sbin/mke2fs", "mke2fs")
dump("/etc/mke2fs.conf", "mke2fs.conf")
lib = "/usr/lib/aarch64-linux-gnu"
need = re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", subprocess.run(["readelf", "-d", f"{out}/mke2fs"], capture_output=True, text=True).stdout)
seen = set()
while need:
    n = need.pop()
    if n in seen:
        continue
    seen.add(n)
    dump(f"{lib}/{n}", n)
    need += re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", subprocess.run(["readelf", "-d", f"{out}/{n}"], capture_output=True, text=True).stdout)
dump(f"{lib}/ld-linux-aarch64.so.1", "ld-linux-aarch64.so.1")
print("mke2fs with", ", ".join(sorted(seen)))
PY
    rm -f "$W/rootfs.img"
    echo "$STAMP" > "$E2/.from"
fi

echo "== the initramfs"
R=$W/root
rm -rf "$R" && mkdir -p "$R"/bin "$R"/sbin "$R"/lib/e2fs "$R"/etc "$R"/proc "$R"/sys "$R"/dev "$R"/tmp "$R"/run "$R"/mnt
install -m755 "$W/item-recovery" "$R/sbin/item-recovery"
ln -s sbin/item-recovery "$R/init"
for a in reboot poweroff halt truncate mountpoint; do ln -s item-recovery "$R/sbin/$a"; done
install -m755 "$W/boot/rd/bin/busybox" "$R/bin/busybox"
# Every applet busybox has (its own list, run here under qemu; else the
# names the port's initramfs gives it, hard links there), but those that
# are PID 1's (init; reboot: busybox's would only signal PID 1) or ours.
if command -v qemu-aarch64-static >/dev/null; then
    APPLETS=$(qemu-aarch64-static "$R/bin/busybox" --list)
else
    APPLETS=$(for f in "$W/boot/rd/bin"/* "$W/boot/rd/sbin"/*; do [ "$f" -ef "$W/boot/rd/bin/busybox" ] && basename "$f"; done)
fi
for a in $APPLETS; do
    case "$a" in busybox|init|reboot|poweroff|halt|linuxrc|truncate|mountpoint) continue ;; esac
    [ -e "$R/bin/$a" ] || ln -s busybox "$R/bin/$a"
done
[ -e "$R/bin/sh" ] || die "no sh among busybox's applets"
cp "$E2"/mke2fs "$E2"/*.so* "$R/lib/e2fs/"
chmod 755 "$R/lib/e2fs/mke2fs" "$R/lib/e2fs/ld-linux-aarch64.so.1"
cp "$E2/mke2fs.conf" "$R/etc/mke2fs.conf"
# The screen's letters: item/grid's own font (Open Sans Light, OFL).
FONT=${FONT:-$HOME/.local/share/fonts/itemgrid/OpenSans-Light.ttf}
[ -f "$FONT" ] || die "no font at $FONT (tools/fetch-fonts.sh)"
install -Dm644 "$FONT" "$R/usr/share/item-recovery/font.ttf"
cat > "$R/bin/mke2fs" <<'EOF'
#!/bin/sh
exec /lib/e2fs/ld-linux-aarch64.so.1 --library-path /lib/e2fs /lib/e2fs/mke2fs "$@"
EOF
chmod 755 "$R/bin/mke2fs"
ln -s mke2fs "$R/bin/mkfs.ext4"
printf 'root:x:0:0:root:/tmp:/bin/sh\n' > "$R/etc/passwd"
printf 'root:x:0:\n' > "$R/etc/group"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
printf 'item-recovery %s %s\n' "$VERSION" "$(git rev-parse --short HEAD 2>/dev/null || echo -)" > "$R/etc/item-recovery"
(cd "$R" && find . | LC_ALL=C sort | cpio -o -H newc -R 0:0 --quiet | gzip -9n) > "$W/ramdisk.gz"

echo "== the boot image"
# The port's header, kernel, device tree and command line, the ramdisk
# ours; Droidian's own words left out of the command line.
# (mkbootimg imports its own gki module from beside it: not -I.)
ARGS=$(cat "$W/boot/args")
CMDLINE=$(python3 -I -c 'import shlex,sys
a=shlex.split(sys.argv[1]); c=a[a.index("--cmdline")+1]
print(" ".join(w for w in c.split() if not w.startswith(("datapart=","droidian."))))' "$ARGS")
python3 -I - "$ARGS" "$W/ramdisk.gz" "$CMDLINE" "$W/item-recovery.img" "$MKBOOTIMG/mkbootimg.py" <<'PY'
import shlex, subprocess, sys
args, ramdisk, cmdline, out, mk = sys.argv[1:6]
a = shlex.split(args)
a[a.index("--ramdisk") + 1] = ramdisk
a[a.index("--cmdline") + 1] = cmdline
subprocess.run(["python3", "-s", "-E", mk] + a + ["--output", out], check=True)
PY
mkdir -p "$HOME/.local/share/itemgrid/recovery"
cp "$W/item-recovery.img" "$HOME/.local/share/itemgrid/recovery/item-recovery.img"
echo "item-recovery.img: $(($(stat -c %s "$W/item-recovery.img") >> 20)) MB ($(($(stat -c %s "$W/ramdisk.gz") >> 10)) KB of initramfs), in ~/.local/share/itemgrid/recovery/"
