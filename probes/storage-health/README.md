# `stethoscope-storage-health`

The `storage_health` probe. Decision 30's payload, on decision 28's process
shape — one capability, one statically linked `no_std` binary, raw syscalls, no
arguments, no environment, no subprocesses, one JSON document on stdout.

```sh
cargo xtask dev                                                     # from the repo root
./target/x86_64-unknown-linux-gnu/probe/stethoscope-storage-health | jq
probes/storage-health/verify.sh                                            # re-derives every claim below
```

11,232 bytes. It opens exactly one file — `/proc/self/mountinfo` — which is the
single static allowlist entry decision 36 says is all `storage_health` needs
guarding.

This is the pilot (`current-state.md`: *"`storage_health` is new, so it risks
nothing that currently works"*). It is real code under development, not a
throwaway, and as of 2026-09-18 the server runs it: `storage_health` is the
first tool collected by a probe (decision 37). What is still missing is listed
below.

## Building and testing it

It is a workspace member, excluded from `default-members`, so `cargo build` at
the repo root does not touch it (decision 33). `cargo xtask dev` builds it with
the rest of the payload and embeds it in the server:

```sh
cargo build --profile probe --target x86_64-unknown-linux-gnu -p stethoscope-storage-health
# -> target/x86_64-unknown-linux-gnu/probe/stethoscope-storage-health
```

**One binary per capability is the design, not a convenience** (decision 28).
The hash of this binary is the capability claim a security team can allowlist,
and the unit an operator can grant privilege to (decision 39); a monolith with
subcommands would present one identity for every capability and lose both. So
each probe is its own crate producing its own binary, and this one is the first.

Size and panic settings come from `[profile.probe]` at the workspace root. The
freestanding link flags (`-nostartfiles -static`) come from this crate's
`build.rs` as `cargo::rustc-link-arg-bins`, which applies to this binary and to
nothing else. That replaced a nested `.cargo/config.toml`, which had two
problems: cargo only reads such a file when invoked from its directory, and the
flags it set reached build scripts and proc macros too — *host* binaries, which
then have no entry point and `SIGSEGV` the moment cargo runs them, with an error
pointing at `serde`'s build script rather than at the flags. The explicit
`--target` is still passed, as decision 33 specifies, and keeps host and probe
artifacts apart. The probe builds only under the probe profile: the dev
profile's `panic = "unwind"` has no meaning in a `no_std` binary.

### Testing it

Three levels, cheapest first:

```sh
./target/x86_64-unknown-linux-gnu/probe/stethoscope-storage-health | jq   # look at it
probes/storage-health/verify.sh                                           # assert on it
strace -c ./target/x86_64-unknown-linux-gnu/probe/stethoscope-storage-health   # what it did
```

`verify.sh` is the test suite. It rebuilds, runs the probe twice and makes
fifteen assertions, exiting non-zero on any failure. Twelve run against an
ordinary invocation; three run inside `unshare -Urm`, because `read_only`,
`privileged` and an escaped mountpoint cannot be exercised on this host
otherwise — nothing with capacity is mounted read-only, the probe is not
elevated, and no mountpoint has a space in it. Without the namespace those three
fields would ship having never been true.

It asserts against the live filesystem rather than a fixture, so it is a smoke
test in the same spirit as `scripts/smoke.sh`: no test framework, no mocks,
`python3` and `unshare` its only requirements. Every assertion checks a shape —
that a device field *is* `major:minor`, that each row is measured or says why
not — rather than a value belonging to one machine, so it runs anywhere.

There are no `#[test]`s. `collect.rs`'s parsing is the code here that most wants
them — mountinfo field offsets, the `-` separator scan, octal unescaping — and
they should arrive with the move into core (decision 35), where they can run on
a normal `std` test harness instead of inside a `no_std` binary with no
allocator at test time.

### Other architectures

Not yet possible: **the runtime is x86_64-only and not arch-gated.** The shared
runtime (`probes/rt/src/sys.rs`) uses `syscall` with rax/rdi/rsi/rdx and a naked
`_start` written in x86 assembly, and this probe's `statfs.rs` lays out
`struct statfs` for x86_64. Building for aarch64 fails to
compile rather than producing a wrong binary, which is the right failure.

Decision 28 records what the second architecture costs — `svc #0` with
x8/x0/x1/x2, roughly thirty lines — and decision 33 makes the cross-arch matrix
an `xtask` concern with `rustup target add`. Neither is built.

## What is built here, and what is not

Built: the capability, and — since 2026-09-18 — the chain that runs it. The
server embeds this binary (`cargo xtask dev`), and `storage_health` places it in
`~/.stethoscope` or finds it in `/opt/stethoscope`, verifies its hash and
ownership, executes it and parses its output (`src/prologue.rs`; decisions 37,
38 and 40). `storage_health` is a tool.

Not built:

* **The runtime is shared, since 2026-09-19.** Syscalls, entry, allocator,
  panic handler and emitting the report live in `probes/rt/` (decision 44);
  this crate holds only `collect.rs`, its `statfs` wrapper, and the function
  that builds the report. Moving the runtime changed this binary's hash, as
  expected, and grew it by 168 bytes; its output and syscall set did not change.
* **Core holds the types and nothing else.** The response types moved to
  `stethoscope-core` (decision 35) and this crate links them, so the probe that
  writes the wire format and the server that reads it share one definition.
  Collection has not moved, and neither has the read guard.
* **No read guard.** Decision 35 puts `guard.rs` in core and decision 25 makes
  it a control; nothing here checks an allowlist. One `EXACT` entry covers this
  probe's entire read set.
* **No release matrix.** `cargo xtask dev` builds the host architecture only;
  `cargo xtask release`, the manifest and `--extract-payload` do not exist.
* **No garbage collection.** Other versions of this probe in a probe directory
  are reported on stderr and left alone (open question 14).

## Decision 30, point by point

| decision 30 says | here |
|---|---|
| one row per filesystem, from mountinfo + one `statvfs` per mountpoint | yes |
| `f_blocks > 0` is the filter, no fstype blocklist | yes — 29 mounts to 10 measurable |
| identity is `major:minor`; the mount source is dropped **at the parser** | yes — `collect.rs` never stores it |
| `f_bavail` *and* `f_bfree` both ship | yes, with the block counts and frame size they were derived from |
| inodes are `Option`, absent rather than zero | yes — vfat and efivarfs omit the block |
| `ST_RDONLY` is reported | yes |
| a mount that refuses `statvfs` is reported, not dropped | yes — `unavailable` in place of `capacity` |
| the payload says whether collection ran privileged (decision 39) | yes — `privileged` |

Serialization is decision 28's settled answer: `no_std` + `alloc` +
`serde_json`, with `JsonSchema` derived on the same types so there is no second
hand-written wire format. The derive costs zero here too — identical to the byte
with it and without it, re-confirming that measurement on a payload richer than
the one it was taken on.

## Verified on the development host, 2026-09-17

`./verify.sh` asserts all fifteen of the following and exits non-zero on any
failure. Everything decision 30 measured on 2026-09-12 reproduces:

* mountinfo entries reduce to the same measurable and unavailable counts
  decision 30 recorded, and the unavailable rows are the ones it named: two
  refusing with `EACCES`, one with `EPERM`.
* the root filesystem is identified by its device id, never by its mapper path.
* the root reserve — free minus available — is **5.09%** of the filesystem,
  matching decision 30's figure to three significant figures. The raw block
  counts have drifted since, because the filesystem is live; the derived reserve
  has not.
* two tmpfs mounts report equal totals under different device ids, which is
  decision 30's double-counting example.
* the vfat and efivarfs mounts omit `inodes`.

Three things decision 30's shape needs that the development host cannot show
without help, exercised in a user namespace (`unshare -Urm`):

* **`read_only`** — a `tmpfs` mounted `-o ro` reports `"read_only": true`. No
  filesystem with capacity on this host is mounted read-only, so without this
  the field would have shipped never having been true.
* **`privileged`** — `true` inside the namespace (euid 0), `false` on an
  ordinary run. Both of decision 39's grant mechanisms are covered: the uid
  comparison catches setuid, and `CapEff` catches file capabilities, which are
  the *preferred* grant and which leave both uids untouched.
* **Escaped mountpoints** — mountinfo octal-escapes space, tab, newline and
  backslash, so a mount whose path contains a space appears with `\040` in it.
  Unescaped here before `statfs`; without that the mount is simply unmeasurable
  and would have appeared as a spurious `not_found` row.

## One run under `sudo` — an observation, not a conclusion

The probe has been run with elevated authority exactly once, on one host, with
one overlay configuration, on 2026-09-17. What follows is what that run showed.
It is not evidence about decision 30 or decision 39, and it should not be cited
as any. It is here because it names two things worth checking on the next host.

**Two mounts reported distinct device ids and identical capacity.** The docker
overlay came back under its own device id, distinct from the root filesystem's,
with capacity equal to the root filesystem in every field, byte for byte. A model
summing those rows' free space would double-count.

Decision 30 ships `major:minor` so that two mountpoints on one filesystem can be
distinguished from two filesystems, and here two views of the same storage
carried different ids. Whether that generalises is **unknown and was not
investigated** — an overlay whose upper layer sits on a separate filesystem
would report different numbers and the rule would hold. One observation says
nothing about how common either case is, on which kernels, or under which
overlay configurations.

**Rows can disappear under privilege, not only gain data.** The overlay gained
capacity; the netns and fuse portal vanished, because `statfs` succeeded and
reported `f_blocks == 0`, so the filter dropped them as pseudo. That mechanism
follows from the code rather than from the run — any mount that refuses `statfs`
unprivileged and has no blocks behaves this way. *Which* mounts those are on a
given host is precisely what one run cannot establish.

Nothing here says whether elevation is worth granting for this capability.
Decision 30 named these three mounts as the motivating case for decision 39; on
this host they happened to hold nothing, and a host with a hung NFS mount,
container quotas or a different fuse layout could look entirely different.

`sudo` is not decision 39's mechanism — that is setuid or file capabilities on
one hash in `/opt/stethoscope`, and decisions 32 and 37 say collection must
never run as root. It is a fine way to see what an elevated probe would report.

## Open questions this probe is standing on

Four. It had to act before they were settled, and none of these choices is a
decision — settling them may change this code.

1. **Open question 12 — framing.** This emits a bare JSON document on stdout
   with no sentinel. OQ12 says that is exactly what a stray `echo` in a remote
   user's `~/.bashrc` will corrupt, and that it is "worth deciding before the
   first probe emits anything, because it is a wire-format change afterwards."
   It has not been decided, so nothing was invented here. **This probe is now
   what makes that deadline concrete.**
2. **Open question 13 — the failure vocabulary.** `unavailable` carries one of
   `permission_denied`, `not_found`, `io_error`, `timed_out`, `stale_handle`,
   `unavailable`. These follow decision 9 (a category, never a diagnostic) and
   are otherwise this probe's guess. Note they are a *per-mount* vocabulary,
   which is a category OQ13 did not anticipate — its six failures are all about
   reaching or trusting a probe, not about one row of an otherwise successful
   payload.
3. **`collected_at` is absent.** `current-state.md` records that decision 30
   does not say whether `storage_health` carries it while three of the four
   existing payloads do. Adding it needs the RFC 3339 civil-from-days formatter
   decision 35 puts in core, which does not exist. Left out rather than invented.
4. **`statfs(2)`, not `statvfs(3)`.** There is no `statvfs` syscall — it is a
   glibc wrapper — and there is no libc here. The fields are the same ones, with
   one subtlety: `statvfs` expresses block counts in `f_frsize`, so that is what
   `frame_size` reports, falling back to `f_bsize` where a filesystem leaves it
   zero. On every filesystem seen here they are equal.

## A finding for decision 39's CI check

Decision 39 makes "disassemble each release probe and assert its syscall set is
a subset of an allowlist" a precondition for supporting elevated probes, and
records that its difficulty is unmeasured. Some of it is now measured, and it is
harder than decision 28's sketch — "find each `syscall` and check the immediate
loaded into `rax`" — assumes.

Scope: one binary, x86_64, rustc 1.98.1, `opt-level = "z"`. The specific
instruction forms below are what *this* compiler emitted and another version may
emit others — which is itself part of the point, since a scanner keyed to a list
of forms is keyed to a compiler. What does not depend on the toolchain is the
failure mode: a backwards scan for an immediate can attribute one function's
constant to another, and cannot tell that it has done so.

This binary makes ten syscalls and no others: `read`, `write`, `open`, `close`,
`mmap`, `exit`, `getuid`, `geteuid`, `capget`, `statfs`. That is the ground
truth, from `strace -c` — which is itself only what this run did, not what the
binary can do; the disassembly is what answers that, which is why the check
exists.

A scan of `objdump -d` finds **nine**, and reports no ambiguity while doing it.

Two things get in the way. The first is that `opt-level = "z"` writes the
constant four different ways, so a scanner matching one form silently
under-reports:

| form | seen for |
|---|---|
| `mov $0x89,%eax` | `statfs` |
| `movabs $0x9,%rax` | `mmap` |
| `push $0x3c; pop %rax` | `exit`, `open`, `close`, `getuid`, `geteuid`, `capget` |
| `xor %eax,%eax` | `read` |

The second is worse. In `write_all` the compiler noticed that `SYS_WRITE` and
the `fd` argument are both `1`, loaded the constant into `%rdi`, and emitted
`mov %rdi,%rax; syscall` — no immediate at the site at all. A scanner walking
backwards for one then runs off the end of the function and finds the
`push $0x3c; pop %rax` belonging to the *previous* one.

So the naive result is not "nine syscalls and one I could not resolve." It is
nine syscalls, `exit` counted twice, and **`write` absent** — a clean-looking,
complete-looking, fully-resolved set that omits the only syscall that sends data
off the machine. An allowlist check built this way passes a binary nobody has
established the output behaviour of.

Two requirements fall out for whoever writes the CI check:

* **It must fail closed on a site it cannot resolve**, and it must be able to
  tell that a site is unresolved — which means not accepting the first immediate
  it finds walking backwards, because that immediate may belong to another
  function.
* **It needs enough dataflow to follow a register**, or the syscall stubs need
  to be written so the number cannot be folded away — `#[inline(never)]` on the
  wrappers, or a form the optimizer will not rewrite. The second is cheaper and
  is a change to the probes rather than to the checker.

The enumeration above was done by hand against `objdump -d` and confirmed
against `strace`. The real scanner belongs in CI with the workspace
(decision 33).
