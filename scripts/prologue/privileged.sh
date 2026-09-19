#!/usr/bin/env bash
#
# privileged.sh — the prologue cases that need fixtures only root can make: a
# real root-owned /opt/stethoscope, a real third user, real file capabilities,
# real noexec and full filesystems. unprivileged.sh approximates some of these
# in a user namespace; this is the real thing.
#
# sudo is used to BUILD FIXTURES ONLY. The server always runs as the invoking,
# unprivileged user — collection must never run as root (decisions 32, 38) —
# and the script refuses to start as root.
#
# It creates, rebuilds and removes /opt/stethoscope and mounts tmpfs filesystems,
# so it runs only on a disposable machine: when CI=true, or with
# PROLOGUE_ALLOW_SUDO_FIXTURES=1. It refuses if /opt/stethoscope already exists,
# so it can never destroy a real install. It expects a user named
# stethoscope-other to exist (the workflow creates it).
#
# Usage: scripts/prologue/privileged.sh [--tool NAME] [--bin PATH] [--require-all]

source "$(dirname "$0")/lib.sh"

OTHER=stethoscope-other
OPT=/opt/stethoscope
GROUP="$(id -gn)"

refuse() { echo "privileged: $1" >&2; exit 1; }
[ "$(id -u)" -ne 0 ] || refuse "must not run as root: the server under test would too"
[ "${CI:-}" = true ] || [ "${PROLOGUE_ALLOW_SUDO_FIXTURES:-}" = 1 ] \
    || refuse "builds fixtures in /opt with sudo; set CI=true or PROLOGUE_ALLOW_SUDO_FIXTURES=1 on a disposable machine"
sudo -n true 2>/dev/null || refuse "needs passwordless sudo"
id "$OTHER" >/dev/null 2>&1 || refuse "needs a user named $OTHER (sudo useradd --no-create-home $OTHER)"
[ ! -e "$OPT" ] || refuse "$OPT already exists; refusing to touch a real install"
command -v setcap >/dev/null || [ -x /usr/sbin/setcap ] || refuse "needs setcap (libcap2-bin)"
SETCAP="$(command -v setcap || echo /usr/sbin/setcap)"

MOUNTS=()
cleanup() {
    for m in "${MOUNTS[@]}"; do sudo umount "$m" 2>/dev/null || true; done
    sudo umount "$OPT" 2>/dev/null || true
    sudo rm -rf "$OPT" "$SCRATCH"
}
trap cleanup EXIT

reset_opt() { sudo umount "$OPT" 2>/dev/null || true; sudo rm -rf "$OPT"; }

# opt_install <dir-owner:group> <dir-mode> <file-owner:group> <file-mode> [file-name]
opt_install() {
    reset_opt
    sudo install -d -o "${1%%:*}" -g "${1##*:}" -m "$2" "$OPT"
    sudo install -o "${3%%:*}" -g "${3##*:}" "$PROBE" "$OPT/${5:-$NAME}"
    # chmod after the chown install performs, which would clear setuid.
    sudo chmod "$4" "$OPT/${5:-$NAME}"
}

echo "privileged [$TOOL]: $BIN (server runs as uid $(id -u), fixtures via sudo)"

# --- /opt: a real operator install -------------------------------------------

H=$(home p1)
opt_install root:root 755 root:root 755
R=$(call "$H")
check "P1  a root-owned install is used" "$(location <<<"$R")" "installed"
check "    and nothing at all is written to home" "$(ls -A "$H" | wc -l)" "0"
check "    and collection is not privileged" "$(privileged <<<"$R")" "false"

H=$(home p2)
opt_install "$OTHER:$OTHER" 755 root:root 755
R=$(call "$H")
check "P2  an install directory owned by a third user is refused, falling through to home" \
    "$(stderr_has "refused: unsafe_owner") $(location <<<"$R")" "yes home"

H=$(home p3)
opt_install root:root 755 "$OTHER:$OTHER" 755
R=$(call "$H")
check "P3  an installed probe owned by a third user is refused, falling through to home" \
    "$(stderr_has "refused: unsafe_owner") $(location <<<"$R")" "yes home"

H=$(home p4)
opt_install "root:$GROUP" 775 root:root 755
R=$(call "$H")
check "P4  a group-writable install directory is refused, falling through to home" \
    "$(stderr_has "refused: unsafe_mode") $(location <<<"$R")" "yes home"

# --- elevation (decision 39), with real grants --------------------------------

H=$(home p5)
opt_install root:root 755 "root:$GROUP" 750
sudo "$SETCAP" cap_dac_read_search+ep "$OPT/$NAME"
R=$(call "$H")
check "P5  a file-capability grant closed to world is honoured, and collection is privileged" \
    "$(location <<<"$R") $(privileged <<<"$R")" "installed true"

H=$(home p6)
opt_install root:root 755 root:root 755
sudo "$SETCAP" cap_dac_read_search+ep "$OPT/$NAME"
R=$(call "$H")
check "P6  a world-executable capability grant is declined; collection falls to home unprivileged" \
    "$(stderr_has "refused: elevated_unsafe") $(location <<<"$R") $(privileged <<<"$R")" "yes home false"

H=$(home p7)
opt_install root:root 755 "root:$GROUP" 4750
R=$(call "$H")
check "P7  a setuid-root grant closed to world is honoured, and collection is privileged" \
    "$(location <<<"$R") $(privileged <<<"$R")" "installed true"
reset_opt

H=$(home p8)
call "$H" >/dev/null
sudo "$SETCAP" cap_dac_read_search+ep "$H/.stethoscope/$NAME"
R=$(call "$H")
check "P8  a capability on the probe cached in home is refused" \
    "$(refusal home <<<"$R")" "elevated_outside_installed"
check "    and the file is left in place, grant and all" \
    "$("${SETCAP%setcap}getcap" "$H/.stethoscope/$NAME" | grep -c cap_dac_read_search)" "1"

# --- home: ownership only root can arrange ------------------------------------

H=$(home p9)
sudo install -d -o "$OTHER" -g "$OTHER" -m 700 "$H/.stethoscope"
R=$(call "$H")
check "P9  a probe directory in home owned by a third user is refused" "$(refusal home <<<"$R")" "unsafe_owner"
check "    and nothing is written into it" "$(sudo ls -A "$H/.stethoscope" | wc -l)" "0"

H=$(home p10)
call "$H" >/dev/null
sudo chown "$OTHER:$OTHER" "$H/.stethoscope/$NAME"
R=$(call "$H")
check "P10 a cached probe owned by a third user is refused" "$(refusal home <<<"$R")" "unsafe_owner"

# --- real mounts ---------------------------------------------------------------

H=$(home p11)
reset_opt
sudo mkdir "$OPT"
sudo mount -t tmpfs -o noexec,mode=755 tmpfs "$OPT"
sudo install -o root -g root -m 755 "$PROBE" "$OPT/$NAME"
R=$(call "$H")
check "P11 a real noexec install falls through to home" \
    "$(stderr_has "exec failed") $(location <<<"$R")" "yes home"
reset_opt

H=$(home p12)
sudo mount -t tmpfs -o "size=4k,uid=$(id -u),gid=$(id -g),mode=700" tmpfs "$H"
MOUNTS+=("$H")
R=$(call "$H")
check "P12 a full home is a clean no_space" "$(refusal home <<<"$R")" "no_space"
check "    with no partial or temporary file left behind" "$(ls -A "$H/.stethoscope" 2>/dev/null | wc -l)" "0"

H=$(home p13)
opt_install root:root 755 root:root 755 "$STALE_NAME"
R=$(call "$H")
check "P13 a stale install falls through to home" "$(location <<<"$R")" "home"
check "    and says so loudly on stderr" "$(grep -c WARNING "$ERR")" "1"

finish privileged
