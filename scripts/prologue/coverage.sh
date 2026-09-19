#!/usr/bin/env bash
#
# coverage.sh — the gate that keeps tools.json honest.
#
# Asks the server which tools it has, and treats every tool whose output schema
# declares `probe_location` as probe-backed. Fails if:
#   * a probe-backed tool is missing from tools.json — it would otherwise ship
#     with no prologue coverage at all;
#   * a tool in tools.json is not advertised, or is not probe-backed;
#   * a listed tool's output schema lacks `privileged` (decision 39).
#
# Usage: scripts/prologue/coverage.sh [path-to-binary]

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${1:-$ROOT/target/debug/stethoscope-mcp}"
TOOLS_JSON="$ROOT/scripts/prologue/tools.json"

TOOLS=$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"coverage","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | HOME=/nonexistent timeout 10 "$BIN" 2>/dev/null | jq -c 'select(.id==2) | .result.tools')
[ -n "$TOOLS" ] || { echo "coverage: server returned no tool list" >&2; exit 1; }

probe_backed=$(jq -r '.[] | select(.outputSchema.properties.probe_location) | .name' <<<"$TOOLS" | sort)
listed=$(jq -r '.[].tool' "$TOOLS_JSON" | sort)
advertised=$(jq -r '.[].name' <<<"$TOOLS" | sort)

fail=0
for t in $(comm -23 <(echo "$probe_backed") <(echo "$listed")); do
    echo "  FAIL  $t is probe-backed but missing from scripts/prologue/tools.json"; fail=1
done
for t in $(comm -23 <(echo "$listed") <(echo "$advertised")); do
    echo "  FAIL  $t is in tools.json but the server does not advertise it"; fail=1
done
for t in $(comm -12 <(echo "$listed") <(echo "$advertised")); do
    jq -e --arg t "$t" '.[] | select(.name==$t) | .outputSchema.properties.probe_location' <<<"$TOOLS" >/dev/null \
        || { echo "  FAIL  $t is in tools.json but does not report probe_location"; fail=1; }
    jq -e --arg t "$t" '.[] | select(.name==$t) | .outputSchema.properties.privileged' <<<"$TOOLS" >/dev/null \
        || { echo "  FAIL  $t does not report privileged (decision 39)"; fail=1; }
done

[ "$fail" -eq 0 ] || exit 1
echo "coverage: every probe-backed tool is listed ($(echo "$listed" | paste -sd, -))"
