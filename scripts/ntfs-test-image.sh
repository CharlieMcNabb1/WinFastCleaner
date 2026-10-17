#!/usr/bin/env bash
# Builds a small NTFS image filled with synthetic files, for testing the MFT
# reader against a real NTFS volume on Linux. Needs root (to mount) and
# ntfs-3g (mkntfs).
#
#   sudo scripts/ntfs-test-image.sh setup    <dir>   # creates <dir>/test.img, mounts it read-only at <dir>/mnt
#   sudo scripts/ntfs-test-image.sh teardown <dir>   # unmounts and removes only what setup created
#
# Then run the tests with:
#   WFC_NTFS_TEST_IMAGE=<dir>/test.img WFC_NTFS_TEST_MOUNT=<dir>/mnt cargo test -p cleaner-core --test ntfs_image
#
# Everything written here is synthetic. Never point this at real data.
set -euo pipefail

usage() {
    echo "usage: $0 setup|teardown <dir>" >&2
    exit 2
}

cmd=${1:-}
dir=${2:-}
[ -n "$cmd" ] && [ -n "$dir" ] || usage

img="$dir/test.img"
mnt="$dir/mnt"

unmount() {
    if mountpoint -q "$mnt" 2>/dev/null; then
        umount "$mnt" 2>/dev/null || fusermount -u "$mnt"
    fi
}

write() { # write <path> <bytes>: a file with that many non-zero bytes
    head -c "$2" /dev/zero | tr '\0' 'x' >"$1"
}

populate() {
    local m=$1
    mkdir -p "$m/docs/reports/2026" "$m/media/video" "$m/many" "$m/links" \
        "$m/deep/a/b/c/d/e/f/g/h" "$m/unicode/Café ☕" "$m/empty dir"

    # Tiny files stay resident inside their MFT record; larger ones do not.
    write "$m/docs/readme.txt" 12
    write "$m/docs/notes.md" 600
    write "$m/docs/reports/2026/q1.pdf" 5000
    write "$m/docs/reports/2026/q2.pdf" 70000
    write "$m/media/video/clip.mp4" $((5 * 1024 * 1024))
    write "$m/media/song.flac" $((3 * 1024 * 1024 + 17))
    write "$m/deep/a/b/c/d/e/f/g/h/bottom.bin" 123456
    write "$m/unicode/Café ☕/名前.txt" 4242
    write "$m/unicode/emoji-📁.dat" 9999

    # Sparse file: large logical size, almost nothing on disk.
    truncate -s $((40 * 1024 * 1024)) "$m/media/sparse.vhd"
    printf 'end' | dd of="$m/media/sparse.vhd" bs=1 seek=$((20 * 1024 * 1024)) conv=notrunc status=none

    # Hard links in different folders: counted once.
    ln "$m/media/video/clip.mp4" "$m/docs/clip-link.mp4"

    # Many long hard-link names overflow one MFT record, so NTFS moves the
    # names into extension records (an $ATTRIBUTE_LIST).
    write "$m/links/base.bin" 50000
    local long
    long=$(printf 'n%.0s' $(seq 1 200))
    for i in $(seq 1 12); do
        ln "$m/links/base.bin" "$m/links/${long}-$i"
    done

    # Real Windows reparse points (a junction and a file symlink), set through
    # ntfs-3g's raw reparse-data attribute. (`ln -s` would create an Interix
    # symlink, which Windows sees as a plain file.) Neither may be followed.
    mkdir "$m/links/docs-junction"
    : >"$m/links/song-symlink.flac"
    python3 - "$m/links/docs-junction" "$m/links/song-symlink.flac" <<'PY'
import os, struct, sys

def names(sub, prt):
    s, p = sub.encode("utf-16-le"), prt.encode("utf-16-le")
    return s, p

def junction(target):
    s, p = names("\\??\\" + target, target)
    buf = struct.pack("<HHHH", 0, len(s), len(s) + 2, len(p)) + s + b"\0\0" + p + b"\0\0"
    return struct.pack("<IHH", 0xA0000003, len(buf), 0) + buf

def symlink(target):
    s, p = names(target, target)
    buf = struct.pack("<HHHHI", 0, len(s), len(s), len(p), 1) + s + p
    return struct.pack("<IHH", 0xA000000C, len(buf), 0) + buf

os.setxattr(sys.argv[1], "system.ntfs_reparse_data", junction("C:\\Users\\test-user\\docs"))
os.setxattr(sys.argv[2], "system.ntfs_reparse_data", symlink("..\\media\\song.flac"))
PY

    # Alternate data stream (with streams_interface=xattr, ntfs-3g maps
    # user.* xattrs to named streams).
    python3 -c "import os,sys; os.setxattr(sys.argv[1], 'user.extra', b'y' * 60000)" "$m/docs/notes.md"

    # Enough files for multi-block folder indexes and many MFT records.
    for i in $(seq 1 3000); do
        printf '%0*d' $((i % 900 + 1)) 0 >"$m/many/file-$i.txt"
    done

    # Deleted files leave unused MFT records behind.
    for i in $(seq 1 50); do write "$m/many/deleted-$i.tmp" 2000; done
    rm -f "$m"/many/deleted-*.tmp
}

case "$cmd" in
setup)
    command -v mkntfs >/dev/null || { echo "mkntfs not found (install ntfs-3g)" >&2; exit 1; }
    mkdir -p "$mnt"
    unmount
    rm -f "$img"
    truncate -s 256M "$img"
    mkntfs --fast --force --quiet --cluster-size 4096 --label WFCTEST "$img"
    ntfs-3g -o streams_interface=xattr "$img" "$mnt"
    populate "$mnt"
    unmount
    # Read-only remount: tests walk it while reading the raw image.
    ntfs-3g -o ro,allow_other "$img" "$mnt"
    chmod a+rx "$dir"
    chmod a+r "$img"
    echo "NTFS test image ready: $img (mounted read-only at $mnt)"
    ;;
teardown)
    unmount
    rm -f "$img"
    rmdir "$mnt" 2>/dev/null || true
    rmdir "$dir" 2>/dev/null || true
    ;;
*)
    usage
    ;;
esac
