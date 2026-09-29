#!/usr/bin/env bash
#
# unprivileged.sh — the prologue's constraints for one probe-backed tool, with
# no root anywhere. Runs locally as well as in CI.
#
# Every case runs the server with HOME pointed at a scratch directory, so this
# never reads or writes the invoking user's own ~/.stethoscope. The /opt cases
# run in a user and mount namespace (`unshare -Urm`) with a scratch directory
# bind-mounted over /opt: inside, our uid maps to 0, which stands in for a
# root-owned operator install. Where unprivileged user namespaces are
# unavailable they are skipped — and under --require-all, which CI sets, a skip
# is a failure. privileged.sh covers what a namespace cannot: real root, a real
# third user, real file capabilities.
#
# Usage: scripts/prologue/unprivileged.sh [--tool NAME] [--bin PATH] [--require-all]
# Without --tool, runs for every tool in scripts/prologue/tools.json.

source "$(dirname "$0")/lib.sh"

# call_ns <opt-dir> <extra-mount-opts> <home> — call, with <opt-dir> as /opt.
call_ns() {
    request | unshare -Urm sh -c '
        mount --bind "$1" /opt || exit 1
        [ -z "$2" ] || mount -o "remount,bind,$2" /opt || exit 1
        exec env -i PATH="$4" HOME="$3" timeout 10 "$5"' \
        sh "$1" "$2" "$3" "$PATH" "$BIN" 2>"$ERR" | jq -c 'select(.id==2)'
}

echo "unprivileged [$TOOL]: $BIN"

# --- home: the write chain ---------------------------------------------------

H=$(home cold)
R=$(call "$H")
check "cold start places the probe and runs it from home" "$(location <<<"$R")" "home"
check "the result reports privileged, and it is false" "$(privileged <<<"$R")" "false"
check "the directory is created mode 700" "$(stat -c %a "$H/.stethoscope")" "700"
check "the probe is placed mode 700 under its content hash" "$(stat -c %a "$H/.stethoscope/$NAME")" "700"
check "nothing else is left in the directory" "$(ls -A "$H/.stethoscope" | wc -l)" "1"

before=$(stat -c '%i %Y' "$H/.stethoscope/$NAME")
R=$(call "$H")
check "a warm start reuses the placed probe without rewriting it" \
    "$(location <<<"$R") $(stat -c '%i %Y' "$H/.stethoscope/$NAME")" "home $before"

H=$(home umask)
R=$(umask 0277; call "$H")
check "a restrictive umask still yields a 700 directory and a runnable probe" \
    "$(location <<<"$R") $(stat -c %a "$H/.stethoscope")" "home 700"

H=$(home tampered)
call "$H" >/dev/null
printf 'x' >> "$H/.stethoscope/$NAME"
tampered=$(sha256sum "$H/.stethoscope/$NAME")
R=$(call "$H")
check "a tampered probe is refused" "$(refusal home <<<"$R")" "hash_mismatch"
check "and left in place, untouched" "$(sha256sum "$H/.stethoscope/$NAME")" "$tampered"

H=$(home setuid)
call "$H" >/dev/null
chmod u+s "$H/.stethoscope/$NAME"
R=$(call "$H")
check "a setuid probe in home is refused though its hash still matches" \
    "$(refusal home <<<"$R") $(sha256sum < "$H/.stethoscope/$NAME" | cut -d' ' -f1)" \
    "elevated_outside_installed ${NAME##*-}"

H=$(home symlink)
mkdir -m 700 "$SCRATCH/elsewhere"
ln -s "$SCRATCH/elsewhere" "$H/.stethoscope"
R=$(call "$H")
check "a symlinked probe directory is refused" "$(refusal home <<<"$R")" "not_a_directory"
check "and nothing is written where it points" "$(ls -A "$SCRATCH/elsewhere" | wc -l)" "0"

H=$(home groupdir)
mkdir -m 770 "$H/.stethoscope"
R=$(call "$H")
check "a group-writable probe directory is refused" "$(refusal home <<<"$R")" "unsafe_mode"

H=$(home groupparent)
chmod 770 "$H"
R=$(call "$H")
check "a group-writable home is refused" "$(refusal home <<<"$R")" "unsafe_parent"
check "and nothing is created in it" "$(ls -A "$H" | wc -l)" "0"

R=$(call "")
check "no HOME is a clean refusal" "$(refusal home <<<"$R")" "no_home"
check "and the model is told a category, not a path" \
    "$(jq -r '.error.message | test("/")' <<<"$R")" "false"

# --- /opt: the read chain, in a namespace --------------------------------------

if ! [ -d /opt ] || ! unshare -Urm true 2>/dev/null; then
    skip "/opt cases (need /opt and unprivileged user namespaces)"
else
    opt() { # opt <case> <mode> [file-name] — a scratch /opt holding one probe
        local o="$SCRATCH/opt-$1"
        mkdir -p "$o/stethoscope"
        cp "$PROBE" "$o/stethoscope/${3:-$NAME}"
        chmod 755 "$o" "$o/stethoscope"
        chmod "$2" "$o/stethoscope/${3:-$NAME}"
        printf '%s' "$o"
    }

    H=$(home opt)
    R=$(call_ns "$(opt plain 755)" "" "$H")
    check "an operator install is used" "$(location <<<"$R")" "installed"
    check "and nothing at all is written to home" "$(ls -A "$H" | wc -l)" "0"

    H=$(home noexec)
    R=$(call_ns "$(opt noexec 755)" "noexec" "$H")
    check "a noexec /opt falls through to home" "$(location <<<"$R")" "home"

    H=$(home worldsuid)
    R=$(call_ns "$(opt worldsuid 4755)" "" "$H")
    check "a world-executable setuid install is declined, falling through to home" "$(location <<<"$R")" "home"

    H=$(home groupsuid)
    R=$(call_ns "$(opt groupsuid 4750)" "" "$H")
    check "a root-owned setuid install closed to world is honoured" "$(location <<<"$R")" "installed"

    H=$(home stale)
    R=$(call_ns "$(opt stale 755 "$STALE_NAME")" "" "$H")
    check "a stale install falls through to home" "$(location <<<"$R")" "home"
    check "and says so loudly on stderr" "$(grep -c WARNING "$ERR")" "1"
fi

finish unprivileged
