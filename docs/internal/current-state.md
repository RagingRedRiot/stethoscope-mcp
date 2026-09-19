# stethoscope-mcp — Current state

> **Read this first.** It answers: what are we building, what has actually been
> implemented, and what only *looks* implemented because it is described in
> `architecture.md`?
>
> Last updated: 2026-09-12 (end-to-end coherence review of the probe
> architecture, taken before writing any of it. Seven new decisions — 33 embeds
> the payload in the server binary and builds probes with an `xtask`, 35 states
> what the core crate may depend on and moves the guard into it, 36 declines a
> guard rule for `statvfs`, **37 makes local collection run a probe exactly as
> remote does, reversing 34** — which had kept local in-process and lasted a day
> — and 38 ships the probes as release artifacts and corrects decision 29's
> discovery order, whose unexamined "and is writable" condition on `/opt` meant
> it could never have used an operator's install, and 39 lets an operator grant
> one specific probe hash elevated authority there.
> Decisions 14, 16, 25, 29, 30, 31, 32, 33 and 35 amended to match. Three new
> open questions — 12, 13 and 14. **Still no new code: the count below is
> unchanged.**)
>
> Updated 2026-09-16 (coherence pass over the redesign, before committing it; no
> code changed and no decision was taken). Four documentation defects fixed: a
> blockquote inserted mid-sentence in decision 30, decision 38's installed-payload
> requirements omitting the directory-*ownership* clause that decision 32's threat
> model turns on, `architecture.md`'s milestone table still planning reversed
> decision 34, and its claim that decision 39's elevation is a v0.2 concern. Two
> real gaps recorded rather than decided: **open question 15**, where the SHA-256
> comes from when the target is `local` — decisions 14 and 33 both say the server
> needs none, which decision 37 quietly made false — and a symlink hole in
> decision 38's directory checks, recorded as a limit there. **The narrative of
> what the redesign was and why remote forced it is now a section at the top of
> [`architecture.md`](architecture.md)**, including the sequencing: build
> `storage_health` on the new design first, port the existing four after the
> design has carried real weight.
>
> Updated 2026-09-17. **The first probe exists: `probes/storage/`.** It
> collects, it serializes, its output is verified, and the payload is decision
> 30's shape. It is real code under development — the pilot — and not a
> fixture, but it is one piece of a chain whose other pieces do not exist: no
> workspace, no core crate, no `xtask`, no embedding, no hash verification, no
> discovery, and the server does not know the binary exists. `storage_health`
> is not a tool yet.
>
> Three things were learned by building it rather than designing it, all
> recorded below; one of them — the syscall scan — makes decision 39's
> precondition harder than decision 28 sketched it. No decision was taken and
> no open question was closed.
>
> **`scripts/probe-size/` was deleted the same day.** It had answered its
> question (decision 28, open question 7 — settled 2026-09-11), and a real
> probe is a better place to re-derive a probe's size than four variants of a
> capability nobody ships. It is recoverable at commit `8e4ea2b`, which is
> where decisions 33 and 38 still point when they cite it as evidence; those
> are dated records and were left alone.
>
> Updated 2026-09-18. **`storage_health` is a tool, and the first one collected
> by a probe.** The prologue — resolve through the read chain, place into
> `~/.stethoscope` if needed, verify, execute — exists in `src/prologue.rs`, and
> with it the smallest slice of decision 33 that makes it possible: a workspace,
> `cargo xtask dev`, a `build.rs` that embeds and digests the probe, and a
> `stethoscope-core` holding the storage wire types. Three decisions: **40**
> drops `/tmp` from both chains, **41** hashes the local probe with `sha2`
> (settling open question 15), **42** records the prologue's rules. One new open
> question, **16**, because building it showed decision 32's recency rule for
> unknown files would break the tool on every upgrade.
>
> Later the same day, **decision 43**: `prologue.yml` runs the prologue's cases
> for every probe-backed tool, with a sudo tier for real ownership, real
> `setcap` and real mounts. Its first run (in a container, not yet on GitHub)
> found that a third user's mode-700 directory or file was refused as
> `io_error` instead of `unsafe_owner`; fixed.
>
> Keep this file honest. If you implement something, update it here.

## What we are building

A local MCP server giving an AI assistant structured, read-only observability
into the user's Linux machine — and eventually remote machines via the user's
own OpenSSH — through narrow, named capabilities rather than shell access.
Full rationale in [`architecture.md`](architecture.md); the decisions and
their reasoning are in [`decisions.md`](decisions.md).

## Current milestone

**v0.1 — local, read-only.** Five tools exist: `system_info`, `system_health`,
`container_list`, `container_health` and `storage_health`. The first four
collect in-process; `storage_health` runs a probe (decision 37). The four are
pre-redesign and are ported one at a time (architecture.md).

## What actually exists

Everything, in full:

* A cargo workspace (decision 33): the server at the root, `core/`,
  `probes/storage/` and `xtask/`. Probes are members but not
  `default-members`, so `cargo build` does not build them.
* A Rust binary crate (edition 2024) using `rmcp` 3.2.0.
* Eleven server source files. The eight below predate the probe work; the
  three after them are new:
  * `src/main.rs` (~152 lines) — MCP **stdio** server plus the complete
    model-facing surface. Every tool is declared here and every body does only
    two things: validate the target and delegate. Kept that way on purpose, so
    decision 4's claim stays checkable by reading one short file. Also holds
    `TargetParams` (one shared parameter type, so every tool generates the same
    input schema), `ContainerParams` for tools addressing a single container
    (decision 26), and `check_target`, which is the entire target mechanism —
    a string compared against one constant, deliberately not a type, trait or
    registry (decision 11).
  * `src/proc.rs` (~265 lines) — reading kernel virtual files and parsing the
    formats found there: `read`/`read_optional`, `parse_error`, `field`,
    `count_cpus`, `meminfo_bytes`, `psi_avg`/`psi_total`, and `target_now`
    (boot time plus uptime, the target's own clock — decision 27). It knows how
    the files are shaped and nothing about what any tool says about them, which
    is what lets the container tools reuse the PSI parser —
    `/sys/fs/cgroup/*.pressure` is the same format — without inheriting
    `system_health`'s response shape. Reads go through `tokio::fs` so they land
    on the blocking pool (decision 16), ask `guard` first (decision 25), and
    log the real `io::Error` to stderr while returning a flat message to the
    model (decision 9).
  * `src/guard.rs` (~339 lines) — the read guard: the single chokepoint
    deciding which paths the server may open, as an **allowlist** (decision 25).
    `check` admits `EXACT` paths, `/proc/<pid>/comm` and a fixed set of cgroup
    basenames; `check_dir` admits directory enumeration beneath
    `/sys/fs/cgroup` only. `FORBIDDEN` sits underneath as a backstop against
    careless widening. Its own unit tests are part of the control, and CI flags
    any edit to the file.
  * `src/cgroup.rs` (~282 lines) — the cgroup v2 substrate: discovery by
    walking the hierarchy, and `classify`, the one function that knows how each
    container runtime names its scopes (decision 22).
  * `src/container_list.rs` (~62 lines) — the `ContainerList` shape: container
    IDs, the runtime that created each, and the distinct `comm` set running
    inside (decision 23). No names, no images (decision 24).
  * `src/container_health.rs` (~287 lines) — the `ContainerHealth` shape:
    CPU use and throttling, memory against its limit, OOM kills, process count,
    full PSI, and `uptime_seconds` from the cgroup directory's timestamp
    (decision 27). Limits are reported by presence (decision 26).
  * `src/system_info.rs` (~38 lines) — the `SystemInfo` shape and its
    collector: `target`, `hostname`, `kernel_release`, `os`, `arch`, from
    `/proc/sys/kernel/{hostname,osrelease}` and `std::env::consts`.
  * `src/system_health.rs` (~296 lines, mostly field documentation) — the
    `SystemHealth` shape and its collector: `collected_at` (RFC 3339 UTC, from
    `/proc/stat`'s `btime` plus uptime, so it is the target's own clock —
    decision 19), uptime, load with the CPU count
    needed to read it, memory availability and swap, and pressure-stall figures
    for CPU, memory and I/O, from `/proc/{uptime,loadavg,stat,meminfo}` and
    `/proc/pressure/*`. Normalized per decision 18: bytes rather than meminfo's
    mislabelled KiB, derived arithmetic alongside the inputs it came from, no
    verdict fields, and the interpretation in `///` comments that `schemars`
    lifts into the output schema. `pressure` is `Option` and simply absent on a
    kernel without PSI.
  * `src/payload.rs` (~40 lines) — the probes this build carries, generated by
    `build.rs` (decision 33): capability, architecture, SHA-256 and bytes.
    Empty in a plain `cargo build`, which the server reports on stderr at
    startup.
  * `src/prologue.rs` (~890 lines, about a third of them tests) — the probe prologue.
    `judge` is the rules as one pure function over gathered facts (decision
    42); `resolve` walks `/opt/stethoscope` then `~/.stethoscope` (decision 40),
    placing into home when nothing usable was found; `run` executes with no
    arguments, no environment and a ten-second deadline, falling through from
    `/opt` to home on a `noexec` refusal (decision 38). Hashes in-process with
    `sha2` (decision 41). Tells the model a location class and a reason, never
    a path.
  * `src/storage_health.rs` (~55 lines) — runs the storage probe through the
    prologue and parses its output into `stethoscope_core::storage::Report`,
    adding `target` and `probe_location`. No storage code in the server.
* `build.rs` — reads `$STETHOSCOPE_PAYLOAD_DIR`, copies each probe into
  `OUT_DIR` under its content hash and generates `payload.rs`. The digest is
  computed over the bytes `include_bytes!` embeds.
* `xtask/` — `cargo xtask dev [cargo build args]` builds every probe for the
  host triple under `[profile.probe]`, stages it in `target/payload/<arch>/`,
  and builds the server pointed at it. **This is now the build command.**
  `cargo xtask release` is the same build with `--release --locked` and
  `--remap-path-prefix` for `$HOME`, `$CARGO_HOME`, `$RUSTUP_HOME` and the
  workspace root, then scans the outputs and fails if any of those paths
  survived (decision 33's 2026-09-18 amendment). Host architecture only; no
  cross-architecture matrix, manifest or archives.
* `core/` — `stethoscope-core`, `no_std` + `alloc`, holding the storage wire
  types and nothing else yet (decision 35). Both the probe and the server link
  it, so there is one definition of the storage wire format. `unavailable`
  became an enum on the move, so the schema lists its six values.
* Every tool takes a required `target`; anything other than `"local"` is
  rejected with `invalid_params` (decision 15). `container_health` takes a
  `container` ID alongside it (decision 26).
* Server dependencies: `rmcp`, `tokio` (now with `process` and `time`), `serde`,
  `chrono`, and — new with the prologue — `serde_json`, `sha2`, `libc` and the
  `stethoscope-core` path dependency (decisions 14 and 41). `sha2` is also a
  build-dependency.
* `scripts/smoke.sh` — drives the server over stdio with real JSON-RPC and
  asserts fourteen things about the replies. Needs only `jq`; no MCP client, no
  Node, no test framework.
* `scripts/prologue/` — the prologue's constraints, written once and run for
  every probe-backed tool listed in `tools.json` (decision 43). Each case runs
  the server with `HOME` pointed at a scratch directory.
  * `unprivileged.sh` — no root anywhere; runs locally too. Cold placement and
    warm reuse, modes set despite a hostile umask, a tampered probe refused and
    left in place, a setuid probe in home refused though its hash matches, a
    symlinked or group-writable directory, a group-writable home, no `HOME`.
    `/opt` cases in a user namespace: an install used with nothing written to
    home, `noexec` falling through, setuid declined or honoured by mode, a stale
    install with a warning. `--require-all` makes a skip a failure.
  * `privileged.sh` — fourteen cases whose fixtures only root can make, run in
    CI (or with `PROLOGUE_ALLOW_SUDO_FIXTURES=1` on a disposable machine):
    a real root-owned install, a real third user owning the install directory,
    the installed probe, `~/.stethoscope` or the cached probe, a group-writable
    install directory, **real `setcap` and setuid grants honoured and declined**
    with `privileged` checked, a real `noexec` mount, a full home giving
    `no_space`, and a world-writable `/opt` refusing an otherwise perfect
    install (P0). It sets `/opt` to root:root 755 for the run and restores it,
    because GitHub's runner image makes it 777. sudo builds fixtures only; the
    server runs unprivileged, and the script refuses to run as root or to touch
    an existing `/opt/stethoscope`.
  * `coverage.sh` — fails if a tool reporting `probe_location` is missing from
    `tools.json`, or a listed tool lacks `probe_location` or `privileged`.
* `.github/workflows/ci.yml` — build, clippy (including the probe under its own
  profile), the unit tests, `cargo xtask dev` followed by `scripts/smoke.sh`,
  and a job that flags any edit to `src/guard.rs` for review (decision 25).
* `.github/workflows/prologue.yml` — builds once, runs the coverage gate, then
  one matrix job per tool in `tools.json` running both tiers, with unprivileged
  user namespaces enabled and a `stethoscope-other` user created. No path
  filter. **First GitHub run (PR #3)**: the gate and the unprivileged tier
  passed; the privileged tier's `/opt` cases failed because the runner's `/opt`
  is mode 777 — the prologue correctly refusing, the fixtures wrong. Fixed as
  above; decision 43's limits have the detail.
* `probes/storage/` — the `storage_health` probe. 11,064 bytes, `no_std` +
  `alloc` + `serde_json`, raw syscalls, statically linked, built to decision
  30's payload shape. Opens exactly one file, `/proc/self/mountinfo`. A
  workspace member: link flags from its own `build.rs`, size settings from
  `[profile.probe]`; the nested `.cargo/config.toml` and its lockfile are gone.
  Plus `verify.sh`, fifteen assertions that re-derive every claim its README
  makes.
* This documentation.

Verified by `scripts/smoke.sh`: the handshake, all five tools advertised,
`target` required on every tool, `target="local"` returning a hostname,
`target="nas"` rejected as `invalid_params`, and for `system_health` a
positive CPU count, memory in bytes rather than KiB, a `collected_at` within
two minutes of the shell's own clock, load shipped with its denominator, and no
verdict field anywhere in the payload; and for the container tools, a count
matching the list, no names or images, and an unknown container rejected as
`invalid_params`; and `storage_health` placing its probe in a scratch home and
returning rows that are each measured or say why not. Verified by
`scripts/prologue/` and by the prologue's unit tests: everything listed
above. Builds clean under `cargo clippy --all-targets`.

Not covered by anything: the missing-PSI path. `pressure` has only ever been
exercised on a kernel that has it. Kubernetes cgroup discovery is likewise
unverified — see the table in the README.

## What is deliberately NOT implemented

Described in `architecture.md`, **none of it exists in code**:

* SSH of any kind — no connection, no config parsing, no `Tag` handling, no
  target discovery, no remote execution.
* Any notion of a target beyond a `String` compared against one constant.
  There is no target type, no trait, no registry, no dispatch, and no way to
  learn what targets exist — the model must already know that `"local"` is the
  answer.
* Most of the eventual capability list: processes, listening ports, systemd,
  logs. Containers and storage are covered, and nothing reads per-process data
  beyond the `comm` values `container_list` reports.
* **The probe architecture is now partly built, on `local` only.** What exists
  is the chain decisions 32, 37, 38, 40, 41 and 42 describe for one capability
  on one machine: embedded payload, location discovery, ownership and hash
  checks, placement, exec. What does not is everything remote and everything
  release-shaped. The paragraph below is the pre-2026-09-18 record and
  overstates what is missing.

  ~~**The probe architecture — decisions 28, 29, 31, 32, 33, 35, 36, 37, 38 and
  39 — is design only.**~~ (Decision 34 is in that range and is *reversed*; it is
  not part of what is to be built.) There is no core crate, no workspace, no `no_std` anything, no
  probe binary, no `sftp` transfer, no target-side cache, no garbage collection,
  no hash verification, no in-memory target cache and no discovery call. The
  existing collectors are still `async` functions using `tokio::fs` inside the
  server binary, returning `rmcp::ErrorData` and formatting timestamps with
  `chrono` — the two dependencies decision 35 says a core crate cannot have.
  `StethoscopeMcp` is still a unit struct holding no state.

  **`probes/storage/` is the one qualification to "design only," and it is a
  narrow one.** The probe exists, builds, runs and emits decision 30's payload.
  Nothing executes it but a shell: the server does not know it exists, nothing
  places it, nothing verifies its hash, and it links no core crate because there
  is none. It answers "is decision 30's payload buildable as specified" — yes —
  and no other question. Landing `storage_health` as a *tool* still costs the
  whole of decisions 33, 35 and 37.
* **Decision 33 is half built.** The workspace, `[profile.probe]`, `cargo xtask
  dev`, `build.rs`, `include_bytes!`, the embedded digests and a host-only
  `cargo xtask release` with build paths remapped out exist. There is still no
  cross-architecture matrix, no
  `--payload-manifest` flag and no `rust-toolchain.toml` — so probe hashes are
  not yet reproducible across toolchains, which decision 33 says they must be
  before a manifest means anything. The server takes no CLI arguments at all,
  and there is no `--extract-payload` (decision 38).
* **There is no release process.** No tagged release, no published binary, no
  probe archive, no manifest, no signing. Decision 38 describes three artifacts;
  zero of them have ever been produced.
* **Nothing verifies what a probe can do.** Decision 39 makes the
  syscall-set-by-disassembly check a CI control and a precondition for
  documenting elevated probes; today it is a sentence in decision 32 and no
  code. One binary has now been disassembled by hand, and the result is a
  warning rather than a reassurance — see the finding below.
* **Decision 37 exists for one tool.** `storage_health` runs a probe placed in
  `~/.stethoscope`; the other four still collect in-process, and the server
  still links their collectors and the read guard. That is the arrangement
  decision 37 replaces. The end state — a server carrying no collection code,
  executing a hash-verified probe from a directory it owns, on `local` as much
  as on a remote host — arrives capability by capability as each tool is ported,
  and not before the last one moves.
* **No way for the model to learn what targets exist.** See open questions 9
  through 11.
* Mutations of any kind.
* Operator-facing logging beyond a single `eprintln!`.
* Configuration of any kind — no config file, no CLI flags, no env vars.
* Rust tests outside `src/guard.rs` and `src/prologue.rs`. `scripts/smoke.sh` is a black-box
  protocol check, not a test suite — there is no `#[test]` anywhere else and no
  integration test using rmcp's client side. The parsing helpers added with
  `system_health` are the first code here with logic worth unit-testing against
  fixture strings; that is the argument for finally adding `#[test]`, and it
  has not been acted on.

## Open questions

Unresolved; do not assume an answer has been chosen.

1. **Interactive SSH authentication through a stdio MCP server.** If `ssh nas`
   needs a passphrase, an agent confirmation, or a hardware-key touch, where
   does that prompt go? The MCP client owns the terminal, and `stethoscope-mcp`'s
   stdio is the protocol channel. Options not yet evaluated: rely on a
   pre-warmed `ssh-agent`, `SSH_ASKPASS`, ControlMaster sockets the user opens
   out-of-band, or return a distinct "authentication required" result and let
   the user act. This is the biggest unknown in v0.2 and it is a design
   question, not an implementation detail. It is not grounds to reverse
   decision 6.

   Two leads, both from thinking about the stdio process lifecycle
   (2026-09-03). Neither is a solution; both are starting points.

   * **Authenticate once per session, not once per call.** A stdio MCP server
     is a child process the client spawns once and keeps alive until the
     session ends, so it can hold warm state across tool calls. A
     ControlMaster socket opened on first use would make a passphrase prompt
     or key touch a once-per-session event rather than a per-call one. That
     turns "prompt on every call" (unusable) into "get through auth once at
     first use" (tractable), and it is probably the shape the answer takes.
     Note the process is per client session, not a shared daemon — two
     concurrent sessions are two processes with no coordination between them,
     so anything cached here is per-session.
   * **MCP elicitation may be the missing channel.** MCP lets a server request
     structured input from the user *through the client*, and `rmcp` ships an
     `elicitation` feature flag. That is plausibly how "target nas needs a
     passphrase" reaches the user without `stethoscope-mcp` ever handling a
     credential. Unverified in two respects: whether the clients we care about
     actually support elicitation, and — the harder problem — that `ssh`
     reads a passphrase from its controlling terminal, not stdin, so bridging
     elicited input into OpenSSH likely still needs `SSH_ASKPASS` or an agent.
     Investigate before assuming this works.

2. **Failure semantics for a well-formed request to an unreachable target.**
   MCP tool error vs. a successful result carrying a normalized error payload.
   Decision 9 constrains the *content*; it does not settle the *channel*.
   Still open — decision 15 settled only the *unknown alias* case, which is
   request validation rather than reachability.

   Decision 31 leans one way without settling this. It requires a target
   *listing* to report `reachable: false` with `checked_at` rather than
   omitting the target, because hiding it would be a verdict. That is the
   listing case; this question is about a tool *call* against a target that
   turns out to be unreachable, which is still undecided. Deciding them
   inconsistently would be defensible but should at least be deliberate.

   One data point for whenever this is decided: rmcp is not itself consistent
   here. A missing parameter comes back as a *successful* JSON-RPC response
   carrying `isError: true` in the tool result, while our explicit
   `invalid_params` comes back as a JSON-RPC error object. Both are legal MCP;
   worth picking one deliberately rather than inheriting the split.

3. **Where operator-facing logs go.** Decision 9 implies a real log, but a
   stdio subprocess has no obvious place to put one. Deferred until something
   actually needs it.

4. ~~**Tool naming and granularity.**~~ **Settled 2026-09-03** — see
   decision 15. Tools are named for the operation and take a required
   `target`: `system_info(target)`. Numbering of the remaining questions is
   left alone so existing references stay valid.

5. ~~**How much of `/proc` is worth normalizing.**~~ **Settled 2026-09-04** —
   see decision 18. The split is by *kind* of transformation: mechanical unit
   conversion and plain arithmetic go in the payload, judgment does not go in
   the payload at all. Denominators always travel with their numerators, there
   are no verdict fields, and interpretation lives in the JSON Schema
   descriptions rather than the response.

6. ~~**What `/proc/<pid>` may put into model context.**~~ **Settled
   2026-09-05** — see decision 23. Identity is `comm`; `cmdline` and `environ`
   are never read, and the prohibition is server-wide rather than
   container-scoped. Decision 25's guard is the mechanism that makes it so
   rather than a promise. A future `process_list` tool inherits this and does
   not get to revisit it.

7. ~~**`serde` + `alloc` versus hand-rolled JSON in the probes.**~~ **Settled
   2026-09-11** — see decision 28. Measured: `serde` costs **+2,696 bytes**
   (8,496 against 5,800 for hand-rolled), not the 50–100 KB estimated. Deriving
   `JsonSchema` on the same types adds **zero** — a probe never calls
   `schema_for!`, so LTO strips schemars entirely, and no feature gate is needed
   to keep it out of probes. Probes get the derived definition; there is no size
   argument against it. Note the 5,800 B baseline is a fresh reimplementation,
   not the lost 9,464 B prototype binary — decision 28 records why the delta is
   trustworthy even though the absolute numbers moved.

   The harness that produced these numbers was deleted 2026-09-17, having
   answered the question; it is at commit `8e4ea2b` if the delta ever needs
   re-deriving. What replaces it for the ordinary case is better: `probes/storage/`
   is a real probe whose size can be measured directly, and building it with and
   without the `JsonSchema` derive re-confirmed the zero-cost row on a richer
   payload than the harness ever carried.

8. ~~**Whether caching a probe on a target is compatible with the read-only
   posture.**~~ **Settled 2026-09-11** — see decision 32. The line is additive
   versus mutative: the server creates and deletes only inside a namespace it
   owns, never touches anything that existed before it connected, and the
   collectors themselves write nothing but stdout. Decision 12's deferral of
   real mutations is untouched.

9. **The shape of a `targets` tool.** Decision 31's cache makes it possible for
   the model to learn what targets exist — which it currently cannot do at all;
   it must already know that `"local"` is the answer. Decision 9 constrains the
   payload hard: aliases only, never usernames, addresses, or key paths. Per
   decision 31 it must report unreachable targets as facts with `checked_at`
   rather than omitting them. Nothing else about it is decided.

   It also breaks decision 15's shape, which is worth resolving rather than
   quietly excepting: every tool takes a required `target`, and a tool that
   *enumerates* targets cannot. Either decision 15 gains an explicit carve-out
   for enumeration, or this is not a tool at all — an MCP resource, or
   something folded into the server instructions.

10. **The shape of a `refresh_targets` tool.** The companion to decision 31's
    TTL, for staleness the server cannot detect on its own — the user just
    fixed the network, or created `/opt/stethoscope`. It should invalidate
    rather than re-probe, so it stays O(1) and cannot be turned into a fan-out
    by a model calling it reflexively. **It would be the first tool with a side
    effect**, which is a real break from "every tool is a pure read" even though
    clearing in-memory state is plainly inside decision 32's line. Name it for
    the operation, not the implementation — the model should not need to know a
    cache exists (decision 15).

11. **How targets are enumerated from the SSH config.** Decision 7 makes the
    user's SSH config the inventory and decision 8 makes `Tag stethoscope-mcp`
    the opt-in, but nothing parses either. Decision 31 assumes a list of
    "locally identified targets" exists; it does not yet.

    One lead, unverified (2026-09-12), which fits `architecture.md`'s standing
    refusal to write an SSH configuration implementation: scan the config for
    literal `Host` lines to get *candidate* aliases, then run `ssh -G <alias>`
    on each and read the resolved `tag` from its output. `ssh -G` performs no
    connection, so this is local and fast, and it hands every wildcard, `Match`,
    `Include` and precedence question back to OpenSSH — which is the only
    implementation that will ever get them right. Unverified in two respects:
    whether `ssh -G` reports `tag` on the OpenSSH versions we care about, and
    what it costs to fork it once per candidate on a large config.

12. **Framing for collected output.** Superseding decision 20 dropped a
    property that is still needed and has no replacement. Its frame reader
    explicitly tolerated junk — "rc-file / motd noise" — before and after the
    payload. A probe emitting one bare JSON document on stdout has no such
    tolerance, and bash **does** source `~/.bashrc` for non-interactive remote
    commands, so a single stray `echo` in a user's dotfiles would corrupt every
    collection from that host. The same problem applies to decision 31's
    discovery command, whose output is parsed the same way.

    Likely answer: a sentinel line bracketing the payload, on both paths. Not
    decided, and worth deciding before the first probe emits anything, because
    it is a wire-format change afterwards.

    A probe has now emitted something (`probes/storage/`, 2026-09-17). It
    writes a bare JSON document with no sentinel, because inventing one here
    would have pre-empted this question — so the deadline in the paragraph above
    is intact, but it is no longer hypothetical.

13. **The normalized failure vocabulary.** Open question 2 asks which *channel*
    a failure uses; this asks what the failures are *called*. The probe
    architecture introduced at least six with no names: unsupported target
    architecture, no writable-and-executable location in decision 29's chain,
    a found payload whose hash does not match, decision 32's "recent mtime,
    unknown hash" suspicious case, a probe killed by its deadline, and an `sftp`
    subsystem disabled server-side. Decision 33 adds `payload_unavailable` for a
    server built without probes.

    Deciding this is cheap now and expensive after five capabilities have each
    invented their own. Decision 9 constrains the content — aliases and
    categories, never addresses or diagnostics — and settles nothing else.

    `probes/storage/` needed six names before this was settled and invented
    them: `permission_denied`, `not_found`, `io_error`, `timed_out`,
    `stale_handle`, `unavailable`. They are a *per-mount* vocabulary, which is a
    category this question did not anticipate — its six failures are all about
    reaching or trusting a probe, and none of them is about one row of a payload
    that otherwise succeeded. Worth deciding whether those are one vocabulary or
    two.

    **Partly settled 2026-09-18 by decision 42**, for the probe-reaching half:
    fourteen per-location reasons and five call-level categories, reported as a
    location class and a reason, never a path. The per-mount vocabulary is still
    the probe's guess, now an enum in `stethoscope-core`. Whether they are one
    vocabulary or two is still open.

14. **What garbage collection is scoped to.** Decision 29 sweeps on connect,
    deleting any `stethoscope-*` whose hash is not in this build's set, and its
    own recorded limits admit two different server versions against one target
    will thrash, each collecting the other's probes. A grace period is proposed
    there and not specified.

    A narrower rule may avoid the problem rather than patch it: collect only
    after successfully placing or verifying a payload **for that capability**,
    removing older entries of that same capability outside a grace window. It
    never sweeps a capability this build does not carry, so two versions
    collecting different capability sets cannot fight. Unverified against the
    upgrade case decision 29 wanted the sweep for.

    **Decision 37 raises the priority of this question.** Two MCP sessions on
    one workstation — an editor and a terminal — are ordinary, where two
    concurrent sessions against one remote target are not, and under decision 37
    both of those sessions now place and collect payloads in the *local* probe
    directory. The thrash decision 29 predicted for two server versions across a
    fleet will show up first on the development machine.

    **Decision 40 adds a case (2026-09-18).** An NFS-shared home is one probe
    cache for every host that mounts it, so a sweep by one host's server version
    can remove a probe another host's server is about to execute. Nothing
    collects yet — the prologue only reports other versions on stderr.

15. ~~**Where the SHA-256 comes from when the target is `local`.**~~ **Settled
    2026-09-18** — see decision 41: `sha2` at runtime, hashing from the same
    descriptor the ownership and mode are read from. The original question is
    kept below.

    **Where the SHA-256 comes from when the target is `local`.** Decision 33
    says "the server needs no runtime SHA-256: the *target* hashes, per
    decision 31, and the server compares strings," and decision 14's amendment
    records `sha2` as a build-dependency that "is not linked into the shipped
    binary." Both were written while decision 34 kept local collection
    in-process, hashing nothing. Decision 37 made the local machine a target
    like any other, and its payload must be hash-verified before it is executed.

    Two answers, neither chosen. Shell out to the local `sha256sum` — uniform
    with the remote path, but a subprocess on the most common path in the
    product, a hard dependency on coreutils being present and correct, and it
    turns decision 32's recorded limit ("verification depends on the target's
    own `sha256sum`") self-referential on the one machine where we could simply
    compute the digest ourselves. Or link `sha2` at runtime for the local case —
    a few hundred KB, no subprocess, no external trust — which makes decision
    14's amendment wrong as written and means the two paths verify by different
    means, which is the shape of drift decision 37 exists to prevent.

    Cheap to settle now, and it is a dependency-list change afterwards.

16. **What an unknown `stethoscope-*` file in a probe directory means.**
    Decision 32 splits a pattern-matching file with an unknown hash by mtime: old
    is a previous version (collect it), recent is suspicious (refuse the
    location, log, leave it). Building the prologue showed the recent branch
    cannot work as written. After an upgrade, the previous version's probe was
    placed recently and has a hash the new build does not know, so the tool
    refuses its own home until the grace period passes. With two server versions
    running side by side — an editor and a terminal on different releases — each
    one's probe is always the other's "recent, unknown hash", and neither can
    collect at all.

    The argument for not refusing: decision 32's threat is another unprivileged
    user planting a file, and a directory that passes decision 42's ownership,
    mode and parent checks is one only we or root can write to. A file there
    was put there by us, a version of us, the user or root — none of them the
    attacker. The file itself is never executed, since only this build's hashes
    are. Suspicion belongs to the directory checks, which already refuse.

    **What the code does meanwhile**: logs such files on stderr and leaves them,
    without refusing the location. Not a decision — it needs one, and probably
    folds into open question 14, since both are about what other versions' files
    mean.

## Findings from building and running the probe (2026-09-17)

> Hosts are described by role only. Nothing here names a machine, a user, a
> mountpoint, a device id or an address — decision 9's rule about what may reach
> the model applies at least as strongly to a file that gets pushed to a public
> repository. Where a figure would fingerprint a host it is given as a ratio.

Two sources of evidence, kept apart because they are not equally strong:
building the probe, and running it on three machines — the development host and
two remote Linux hosts reached over SSH, one of which allowed both an
unprivileged and a root login and is therefore the only *controlled* comparison
here.

### From building it

* **Decision 39's disassembly check is harder than decision 28 sketched it, and
  fails in the direction that matters.** Decision 28 describes it as "find each
  `syscall` and check the immediate loaded into `rax`." Against the probe that
  method reports nine syscalls, cleanly and with no ambiguity, and the binary
  makes ten. (Scope: one binary, x86_64, rustc 1.98.1. The *instruction forms*
  are compiler-specific and another version may emit others — which is part of
  the point. The failure mode is not: a backwards scan can attribute one
  function's constant to another and cannot tell that it has.) `opt-level = "z"` writes the constant four different ways
  (`mov`, `movabs`, `push`/`pop`, `xor`), and in `write_all` the compiler
  noticed `SYS_WRITE` and the `fd` argument are both `1`, so the site is
  `mov %rdi,%rax; syscall` with no immediate at all — and a scanner walking
  backwards past the function boundary picks up the *previous* function's
  constant instead. The result is not "one site I could not resolve": it is a
  complete-looking set with `exit` counted twice and **`write` absent**, which
  is the only syscall that sends anything off the machine. The check must fail
  closed on an unresolved site and must be able to tell that a site is
  unresolved. The cheaper fix is probably on the probe side — write the syscall
  stubs so the number cannot be folded into a register — rather than teaching
  the checker dataflow. Worth settling before elevation is documented as
  supported, which decision 39 already requires.

* **The privilege flag does not need a file, and should not use one.** The
  obvious source for decision 39's flag is `/proc/self/status`, which carries
  both the uid set and `CapEff`. The guard denies it: `PER_PROCESS` admits only
  `comm`, and `guard.rs`'s own documentation names `status` among the files
  denied on purpose (decision 23). `geteuid`, `getuid` and `capget` answer the
  same question, keep the probe's read set at exactly one path — the single
  `EXACT` entry decision 36 says `storage_health` needs — and came out 768 bytes
  smaller than the version that parsed the file. Three syscalls cost less than
  an exception to the most carefully argued list in the codebase.

* **mountinfo escapes its paths.** Space, tab, newline and backslash appear
  octal-escaped, so a mount at `/tmp/my disk` is printed `/tmp/my\040disk`.
  Passed to `statfs` unescaped it fails, which under decision 30's rules
  produces a plausible-looking `unavailable` row for a filesystem that is
  perfectly healthy — a silent wrong answer rather than a visible failure. The
  measurement harness had this bug and it never showed, because no such mount
  exists on the development host and the harness only had to serialize the same
  thing twice. It matters in a collector. `probes/storage/` unescapes, and
  `verify.sh` exercises it in a user namespace; the same code and the same test
  need to survive the move into core.

### From running it

**The transfer and execution chain works end to end, by hand.** Discovery
(architecture, home, whether anything is `noexec`, whether an operator install
exists), a mode-700 directory the caller owns, `sftp` of the probe under a
filename that is its own content hash, the hash verified by the *target's* own
`sha256sum` before anything runs, `chmod`, exec, one JSON document on stdout.
Clean exit and empty stderr on both remote hosts. The server did none of this
and still knows nothing about it.

**`storage_health` gained nothing from elevated privileges on any host tested.**
This is the headline. Every mount that became measurable under root or under
`setcap cap_dac_read_search+ep` turned out to be either a byte-identical
duplicate of the root filesystem or a pseudo filesystem reporting
`f_blocks == 0`. On the one host where privilege could be varied alone — same
binary, same machine, same minute — the grant made twelve mountpoints newly
measurable and **zero** of them were anything but a duplicate. Elevation also
made the payload worse: it introduced thirteen identical rows under thirteen
distinct device ids, where summing the available bytes overstates real capacity
by an order of magnitude. The unprivileged run has no such duplication, because
its refused rows carry no numbers at all.

**Scope, because it is narrow.** Three hosts, all Linux running the same
container storage driver, and on all three the unreadable mounts happened to be
container overlays, network-namespace handles and a desktop portal. A host whose
unreadable mounts are real filesystems would look nothing like this, and no
other capability has been examined. Decision 30 named those mounts "the
motivating case for decision 39"; on everything measured so far that case is
empty. **The consequence for the default posture is that the unprivileged path
is not a degraded one for this capability — it is the whole picture.**

**Hash verification cannot detect elevation.** The SHA-256 is byte-identical
before and after `setcap`, because file capabilities live in extended attributes
and `sha256sum` does not see them. Decision 39 requires the server to check
ownership, mode and location separately from the hash; that is not
belt-and-braces, it is **the only thing** distinguishing a verified payload from
a verified *and silently elevated* one. A server that checked the digest and
executed would run an elevated binary while reporting a clean match. Worth
stating in decision 39 rather than leaving to be inferred.

**The `capget` branch of the probe's privilege check is what caught that**, and
it had never been exercised before. Under a file capability the uid comparison
sees nothing — effective and real uid are equal — so the capability set is the
only signal. Every previous `privileged: true` came from an effective uid of
zero. It works.

**`cap_dac_read_search` is not root-equivalent, and decision 30 recorded why
without knowing it.** It measured all but one of the mounts root could; the
holdout was a FUSE mount. Decision 30 noted that the container overlays and
namespace handles refused with `EACCES` while the FUSE mount refused with
`EPERM`, as a descriptive detail. That distinction has a consequence: the
capability overrides DAC permission checks, and a FUSE owner check is not one.
An operator granting it should be told it buys the `EACCES` rows and nothing
else.

**`read_only` earned its place, and on evidence the development host cannot
produce.** On one remote host, 26 of 43 rows are read-only and 25 of those report
100% used with zero bytes available — read-only image mounts of the kind a
package system creates. Without the flag a model is handed 25 filesystems that
look completely full and no way to tell that full is what such an image *is*.
Decision 30 justified `ST_RDONLY` on the remount-after-IO-error case; this is a
second and far more common justification it did not anticipate. Locally nothing
with capacity is mounted read-only, so the field had to be exercised with a
contrived namespace mount to be seen true even once.

**On a busy container host the unprivileged payload is mostly refusals** — on
one host, 26 of 36 rows. Decision 30 reports these rather than dropping them so a
hung network mount stays visible, and that reasoning holds; what it did not
anticipate is the ratio. Whether that many refusal rows help or bury the ten
carrying data is a real question this raises and does not answer.

**The device-id rule does not catch container overlay mounts.** Mounts reporting
byte-identical capacity appeared on every host examined. Where the duplication
came from bind mounts the rule worked — the mounts share a device id and are
plainly the same filesystem. Where it came from container overlays it did not:
each gets its own device id while reporting the underlying filesystem's numbers.
Decision 30 ships the device id precisely so a model can avoid double-counting,
and in the overlay case it cannot. This reproduced on all three hosts, which is
a reproduction within one storage driver rather than a general claim about
overlay filesystems.

**`inodes` is correctly absent** on every filesystem type encountered that has
no inode concept, including one type the development host does not have.

**Two mechanical notes for whoever implements decision 29.** `sftp` created the
payload with the *target's* umask, observed as two different modes on two hosts
— one of them group-writable, which decision 38 forbids outright. The placement
step must set the mode explicitly rather than assume it; on one host, not doing
so would have produced exactly the condition decision 38 says to refuse. And
neither host emitted rc-file noise on stdout or stderr, so open question 12's
framing problem did not bite — that is two quiet hosts, not evidence the problem
is absent.

**Every figure decision 30 recorded reproduces on the development host**, and
decision 28's "`JsonSchema` costs a probe zero" holds on a payload considerably
richer than the one it was measured on — identical to the byte with the derive
and without it. That second one is re-derivable from the probe itself rather than
from a harness, which is why deleting the harness cost nothing.

## Working agreements for future sessions

* When you find yourself writing "we'll probably need this eventually" —
  document it here, do not build it.
* Do not introduce a target abstraction beyond what a real remote
  implementation actually demands (decision 11). **The condition has now been
  met in design**: decision 31's cache is the seam decision 11 was holding the
  door open for, because probes are per-architecture and that fact has to live
  somewhere. That licenses the cache and nothing more — still no target trait,
  no registry, no dispatch layer built ahead of a caller that needs it.
* Do not add an arbitrary-command tool. If an operation is missing, implement
  that operation (decision 4).
* Update this file when you change what exists. A stale current-state file is
  worse than none, because the next session will believe it.
* New tools follow decision 15: name the operation, take a required `target`,
  return the same shape regardless of how the data was collected.
* Decision 20's frame protocol is **superseded** by decision 28. Its
  measurements about batching and latency remain valid and worth reading; its
  length-framed plaintext mechanism should not be implemented.
* ~~Do not port the four working tools to the probe architecture until the probe
  chain has actually run against a real remote host.~~ **Superseded 2026-09-12
  by decision 37** — see the agreement above. The original reasoning was that
  the payoff for porting arrives only with remote collection; under decision 37
  it arrives as soon as the chain works locally. What survives unchanged is the
  choice of pilot: `storage_health` is new, so it risks nothing that currently
  works.
* Decision 32 is a **precondition** for decision 29, not a companion to it. Do
  not push a payload to any target before hash verification and the
  directory-ownership check exist — without them the target-side cache is a
  place for another user on that host to plant something.
* When decision 31's cache lands, `src/main.rs` stops being stateless. Keep the
  cache's type and logic in its own module so tool bodies still read as
  validate, look up, delegate — the claim that one short file enumerates
  everything this server can do is worth protecting (decision 4).
* ~~**`probes/storage/` declares response types it does not own, and that is a
  debt with a deadline.**~~ **Discharged 2026-09-18**: the types are in
  `stethoscope-core` and both the probe and the server link them. The original
  agreement is kept below.
* **`probes/storage/` declares response types it does not own, and that is a
  debt with a deadline.** They belong in `stethoscope-core` (decision 35) and
  are local only because core does not exist. Every day they stay is a day two
  definitions of the storage wire format *could* appear — the drift decision 28
  exists to prevent, in the first capability built on the new design. When core
  lands, the types move and the probe links them. Do not add a second collector
  anywhere in the meantime, and do not copy these types to start a second probe:
  the second probe is the moment the debt becomes real.
* **`probes/storage/`'s README lists four open questions its code is standing
  on** — framing (OQ12), the failure vocabulary (OQ13), whether `collected_at`
  travels, and `statfs` versus `statvfs`. Those are the pilot's inputs, not
  precedents, and settling them may change this code. A second probe must not
  inherit them by copying; it should wait for the answers or force them.
* ~~**The probe's nested `.cargo/config.toml` is temporary.**~~ **Done
  2026-09-18**: the flags moved to the probe's `build.rs` as
  `rustc-link-arg-bins` and the profile to the workspace root.
* **The probe's nested `.cargo/config.toml` is temporary.** Cargo resolves it by
  invocation directory, so it stops working the moment the crate joins the
  workspace decision 33 specifies. The flags move to the workspace root then.
  Do not read the current layout as the intended one.
* **A probe-backed tool is not done until it is in `scripts/prologue/tools.json`**
  (decision 43). It must report `probe_location` and `privileged`; the coverage
  gate fails CI otherwise. Its `arguments` must succeed on a bare runner.
* **There is one collection mechanism, and `local` is not an exception to it**
  (decision 37). Resist re-introducing an in-process path — as a fast path, as a
  fallback, or as a convenience for tests. It was tried for one day as decision
  34 and reversed; the reasoning is recorded in both. The payload chain being
  exercised by ordinary local use is the property that makes decisions 29 and 32
  trustworthy, and a second path silently erodes it by giving the first one less
  to do.
* **A tool is not ported until it runs as a probe on `local`.** This replaces
  the older agreement that deferred porting until the chain had run against a
  real remote host — under decision 37 the chain runs on the development
  machine, so nothing about porting waits on open question 1 any more. Port
  them, verify locally, and only then worry about SSH.
* **The core-crate extraction is the pilot's real work, not a preliminary.**
  Decision 35: every collector's error type changes, `collected_at` needs an
  RFC 3339 formatter that is not `chrono`, and the guard moves — taking its
  tests, CI's `detect-guard-modifications` path, and the source-scan test's
  scope with it. Budget for it accordingly.
* **Done 2026-09-18** — `guard.rs`'s module documentation now says the guard
  governs paths whose contents are read as collected data, and names the two
  things outside its remit: `statvfs` and the prologue. That edit is
  documentation only, and CI's guard job will flag it anyway, as intended.
* When `storage_health` lands, `guard.rs`'s module documentation must say that
  the guard governs paths whose *contents* are read, not paths the server
  touches (decision 36). Without that sentence the first reader to find a
  `statvfs` call with no guard check will reasonably conclude the control has a
  hole.
* Two small consistency items, noted so they are not lost: decision 30 does not
  say whether `storage_health` carries `collected_at`, and three of the four
  existing payloads do; and nothing reserves `"local"` against a user who has a
  `Host local` entry tagged in their SSH config.
* **The operator pre-install path will be exercised first by someone who is not
  us.** Decision 38's read chain — find an operator's payload in a root-owned
  `/opt/stethoscope`, verify it, execute it — is the one path in this design
  whose first real use is plausibly a stranger following the README on a host we
  have never seen. It deserves a deliberate test rather than being assumed to
  work because the write path does.
* **The privileged-collection path must not become the assumed one.** Decision
  39 permits an operator to elevate a probe and requires the unprivileged path to
  remain the default, the guarantee, and — the part that will slip — the tested
  one. If `storage_health` is only ever exercised against a privileged `/opt`
  install, the configuration every user actually has is the one with no coverage.

  **Partly discharged 2026-09-17**: the unprivileged path now has real coverage
  on a remote host, and on every host tested it returns the complete storage
  picture with no elevated run adding to it. That is a reason to keep testing it
  first, not a reason to stop testing the elevated one — and it is a finding
  about this capability, not about the mechanism. The server itself still runs
  nothing elevated that an operator did not deliberately configure.
* **The `xtask` has landed, and three of the README's five changes are made**
  (2026-09-18): the build command, `~/.stethoscope`, and the `/opt` sentence.
  The other two wait on a published release: the architecture matrix, and the
  `--extract-payload` install procedure. The operator qualifier to "runs with
  exactly that user's permissions" is stated without advertising elevation,
  which decision 39 withholds until the disassembly check exists.
* **The README is accurate today and will need five changes when the `xtask`
  lands** — do not make them early, but do not forget them. `cargo build
  --release` stops being the build command (decision 33's amendment); the server
  creates `~/.stethoscope` and executes a binary from it on the local machine
  (decision 37), which users should read rather than discover; a published
  binary can only inspect a machine whose architecture is in the release matrix,
  which is a new failure mode with no precedent in v0.1; and the operator
  pre-install procedure needs documenting — `/opt/stethoscope`,
  `--extract-payload`, and the fact that a complete install means the server
  writes nothing to that host at all (decision 38); and "it runs as the user who
  launches it and has exactly that user's permissions" needs the qualifier
  decision 39 introduces, since an operator-elevated probe exceeds it.
