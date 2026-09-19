#!/usr/bin/env bash
# Re-derives every claim this crate's README makes about the probe's payload.
#
# Fifteen assertions: twelve against an ordinary run, three against a run in a
# user namespace, which is the only way to exercise `read_only`, `privileged`
# and an escaped mountpoint on a host where nothing read-only has capacity.
#
# Needs python3 and `unshare -Urm` (unprivileged user namespaces enabled).
set -euo pipefail
cd "$(dirname "$0")"

# Built exactly as `cargo xtask dev` builds it (decision 33): the probe profile,
# an explicit host target, from the workspace root.
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
ROOT=$(cd ../.. && pwd)
(cd "$ROOT" && cargo build --profile probe --target "$TRIPLE" -p stethoscope-storage >/dev/null 2>&1)
B="$ROOT/target/$TRIPLE/probe/stethoscope-storage"

echo "probe: $(stat -c%s "$B") bytes"
echo "files opened: $(strace -e trace=open,openat "$B" 2>&1 >/dev/null | grep -c '= [0-9]')"
echo

"$B" | python3 -c '
import json, re, sys
d = json.load(sys.stdin); fs = d["filesystems"]
fails = 0
def ok(cond, msg):
    global fails
    if not cond: fails += 1
    print(("PASS" if cond else "FAIL"), msg)

ok(d["privileged"] is False, "privileged is false on an ordinary run")
ok("truncated" not in d, "truncated omitted when mountinfo fits")
measured = sum(1 for f in fs if "capacity" in f)
refused  = sum(1 for f in fs if "unavailable" in f)
ok(measured + refused == len(fs), f"every row is measured or unavailable ({measured} + {refused})")
ok(all("source" not in f for f in fs), "no mount source anywhere in the payload")
# A leaked mount source would be the only path-shaped value outside mount_point.
# /dev/shm and friends are legitimate MOUNTPOINTS, so scope the check to the
# other fields rather than to the document as a whole.
others = [v for f in fs for k, v in f.items()
          if k != "mount_point" and isinstance(v, str)]
ok(not any("/" in v for v in others), "no path-shaped value outside mount_point")
ok(all(re.fullmatch(r"\d+:\d+", f["device"]) for f in fs),
   "every row identified by major:minor and nothing else")
ok(len({f["device"] for f in fs}) == len(fs), "every row has a distinct device id")
root = [f for f in fs if f["mount_point"] == "/"][0]
ok(re.fullmatch(r"\d+:\d+", root["device"]) is not None,
   "root identified by device id, not by its backing path")
c = root["capacity"]
ok(c["blocks_free"] > c["blocks_available"], "root ships both free and available, and they differ")
ok(c["total_bytes"] == c["blocks_total"] * c["frame_size"], "derived bytes agree with blocks x frame_size")
ok(all("inodes" not in f["capacity"] for f in fs
       if f.get("capacity") and f["fs_type"] in ("vfat", "efivarfs")),
   "filesystems with no inode concept omit the block")
ok(all("capacity" not in f for f in fs if "unavailable" in f),
   "unavailable rows carry no capacity")
sys.exit(1 if fails else 0)
'

echo
unshare -Urm --propagation private sh -c '
  mkdir -p "/tmp/ro d" && mount -t tmpfs -o size=1M,ro tmpfs "/tmp/ro d"
  '"$B"'
' | python3 -c '
import json, sys
d = json.load(sys.stdin)
fails = 0
def ok(cond, msg):
    global fails
    if not cond: fails += 1
    print(("PASS" if cond else "FAIL"), msg)

ok(d["privileged"] is True, "privileged is true under euid 0 / full caps")
r = [f for f in d["filesystems"] if "ro d" in f["mount_point"]]
ok(bool(r), "escaped mountpoint (/tmp/ro\\040d) unescaped, then measured")
ok(bool(r) and r[0]["capacity"]["read_only"] is True, "read_only true for an ro mount")
sys.exit(1 if fails else 0)
'
