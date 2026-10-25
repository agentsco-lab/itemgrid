u8() { od -An -t u8 -j "$2" -N 8 "$1" | tr -d ' '; }
u4() { od -An -t u4 -j "$2" -N 4 "$1" | tr -d ' '; }
for slot in a b; do
    dev=/dev/disk/by-partlabel/boot_$slot
    part=$(readlink -f "$dev")
    name=${part##*/}
    dname=$(printf '%s' "$name" | sed 's/[0-9]*$//')
    disk=/dev/$dname
    num=${name#"$dname"}
    ss=$(cat "/sys/block/$dname/queue/logical_block_size")
    # The GPT's header at LBA 1: its entries' first LBA at 72, an entry's
    # size at 84; the entry's attributes at 48.
    lba=$(u8 "$disk" $((ss + 72)))
    esize=$(u4 "$disk" $((ss + 84)))
    attr=$(u8 "$disk" $((lba * ss + (num - 1) * esize + 48)))
    sha=$(sha256sum "$dev" | cut -d' ' -f1)
    kernel=
    if [ "$(head -c 8 "$dev")" = "ANDROID!" ]; then
        ksize=$(u4 "$dev" 8)
        page=$(u4 "$dev" 36)
        magic=$(od -An -t x1 -j "$page" -N 2 "$dev" | tr -d ' ')
        kernel=$(dd if="$dev" bs="$page" skip=1 count=$(((ksize + page - 1) / page)) 2>/dev/null | head -c "$ksize" | if [ "$magic" = 1f8b ]; then zcat 2>/dev/null; else cat; fi | grep -a -o -m1 'Linux version [^ ]*' | cut -d' ' -f3)
    fi
    echo "slot=$slot attr=$attr sha=$sha kernel=$kernel"
done
