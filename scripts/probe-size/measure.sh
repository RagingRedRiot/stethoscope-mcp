#!/usr/bin/env bash
# Regenerates decision 28's probe size table. Builds the same storage
# capability four ways and reports the bytes each one costs on the wire.
#
# The point of the harness is the *delta* between `handrolled` and `serde`:
# both are built from identical collection code (../common) and differ only in
# how the payload is serialized, so the difference is attributable.
set -euo pipefail
cd "$(dirname "$0")"

TRIPLE=x86_64-unknown-linux-gnu
build() { ( cd "$1" && cargo build --release ${2:+--target $2} >/dev/null 2>&1; ); }

build handrolled "$TRIPLE"
build serde      "$TRIPLE"
build schemars   "$TRIPLE"
build stdver     ""

H=handrolled/target/$TRIPLE/release/probe-handrolled
S=serde/target/$TRIPLE/release/probe-serde
C=schemars/target/$TRIPLE/release/probe-schemars
D=stdver/target/release/probe-std

base=$(stat -c%s "$H")
printf '%-42s %9s %11s\n' variant bytes 'vs base'
while IFS='|' read -r name path; do
  n=$(stat -c%s "$path")
  printf '%-42s %9d %+11d\n' "$name" "$n" "$((n - base))"
done <<EOF
no_std, hand-rolled JSON|$H
no_std + alloc + serde_json|$S
  ... + JsonSchema also derived|$C
static std + serde_json (calibration)|$D
EOF

# The two no_std variants must emit the same wire format, or the delta above
# is comparing two different payloads. Values drift between runs on a live
# filesystem, so compare keys, order and mount identity rather than bytes.
echo
python3 - "$H" "$S" <<'PY'
import json, subprocess, sys
a, b = (json.loads(subprocess.check_output([p])) for p in sys.argv[1:3])
def shape(d):
    return [(f["mount_point"], f["source"], f["fs_type"], tuple(f)) for f in d["filesystems"]]
ok = list(a) == list(b) and shape(a) == shape(b)
print("wire format identical:", "yes" if ok else "NO — delta is not comparable")
sys.exit(0 if ok else 1)
PY
