# lib.sh — shared by unprivileged.sh and privileged.sh. Sourced, not run.
#
# Every probe-backed tool is checked against the same prologue constraints, so
# the cases are written once and parameterized by one entry of tools.json:
#
#   { "tool": "storage_health", "capability": "storage-health", "arguments": {...} }
#
# `arguments` must produce a successful call on a bare CI runner.
#
# Every probe-backed tool's result carries `probe_location` (decision 38) and
# `privileged` (decision 39); that contract is what lets these checks be
# generic, and coverage.sh enforces it.
#
# Options, parsed here:
#   --tool NAME      run for one tool; without it, re-run for every tool listed
#   --bin PATH       server binary (default target/debug/stethoscope-mcp)
#   --require-all    a skipped case is a failure (CI sets this)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOLS_JSON="$ROOT/scripts/prologue/tools.json"
BIN="$ROOT/target/debug/stethoscope-mcp"
TOOL=""
REQUIRE_ALL=0
SELF_ARGS=("$@")

while [ $# -gt 0 ]; do
    case "$1" in
        --tool) TOOL="$2"; shift 2 ;;
        --bin) BIN="$(realpath "$2")"; shift 2 ;;
        --require-all) REQUIRE_ALL=1; shift ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

command -v jq >/dev/null || { echo "needs jq" >&2; exit 1; }
[ -x "$BIN" ] || { echo "no executable at '$BIN' — run 'cargo xtask dev' first" >&2; exit 1; }

# Without --tool, run the calling script once per listed tool and aggregate.
if [ -z "$TOOL" ]; then
    rc=0
    for t in $(jq -r '.[].tool' "$TOOLS_JSON"); do
        "$0" "${SELF_ARGS[@]}" --tool "$t" || rc=1
    done
    exit "$rc"
fi

ENTRY="$(jq -c --arg t "$TOOL" '.[] | select(.tool == $t)' "$TOOLS_JSON")"
[ -n "$ENTRY" ] || { echo "tool '$TOOL' is not in tools.json" >&2; exit 1; }
CAPABILITY="$(jq -r .capability <<<"$ENTRY")"
ARGUMENTS="$(jq -c .arguments <<<"$ENTRY")"

PROBE="$ROOT/target/payload/$(uname -m)/$CAPABILITY"
[ -f "$PROBE" ] || { echo "no staged probe at '$PROBE' — run 'cargo xtask dev' first" >&2; exit 1; }
NAME="stethoscope-$CAPABILITY-$(sha256sum "$PROBE" | cut -d' ' -f1)"
STALE_NAME="stethoscope-$CAPABILITY-$(printf '0%.0s' {1..64})"

SCRATCH="$(mktemp -d)"
chmod 700 "$SCRATCH"
trap 'rm -rf "$SCRATCH"' EXIT   # privileged.sh replaces this with one that also undoes sudo fixtures
ERR="$SCRATCH/stderr"
: >"$ERR"

request() {
    printf '%s\n' \
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"prologue","version":"0"}}}' \
      '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
      "$(jq -nc --arg name "$TOOL" --argjson args "$ARGUMENTS" \
          '{jsonrpc:"2.0",id:2,method:"tools/call",params:{name:$name,arguments:$args}}')"
}

# call <home> — the tool's reply. An empty <home> runs with HOME unset.
call() {
    local env=(env -i "PATH=$PATH")
    [ -n "$1" ] && env+=("HOME=$1")
    request | "${env[@]}" timeout 10 "$BIN" 2>"$ERR" | jq -c 'select(.id==2)'
}

# Readers over a reply.
location()   { jq -r '.result.structuredContent.probe_location // "error"'; }
privileged() { jq -r '(.result.structuredContent // {}) | if has("privileged") then .privileged else "error" end'; }  # not //: it treats false as missing
refusal()    { jq -r --arg l "$1" '.error.data.locations[$l] // "none"'; }
stderr_has() { grep -qF -- "$1" "$ERR" && echo yes || echo no; }

# home <case> — a fresh, empty scratch home.
home() { local h="$SCRATCH/home-$1"; mkdir -p "$h"; chmod 700 "$h"; printf '%s' "$h"; }

fail=0
skipped=0
check() { # check <label> <actual> <expected>
    if [ "$2" = "$3" ]; then
        printf '  ok    %s\n' "$1"
    else
        printf '  FAIL  %s\n        got:  %s\n        want: %s\n' "$1" "$2" "$3"
        sed 's/^/        stderr: /' "$ERR"
        fail=1
    fi
}

skip() { # skip <label> — a failure under --require-all, because silence is not success
    if [ "$REQUIRE_ALL" -eq 1 ]; then
        printf '  FAIL  %s (skipped, and --require-all is set)\n' "$1"
        fail=1
    else
        printf '  skip  %s\n' "$1"
        skipped=1
    fi
}

finish() { # finish <suite-name>
    echo
    if [ "$fail" -ne 0 ]; then
        echo "$1 [$TOOL]: FAILURES" >&2
        exit 1
    fi
    echo "$1 [$TOOL]: all checks passed$([ "$skipped" -eq 1 ] && echo ' (some skipped)')"
}
