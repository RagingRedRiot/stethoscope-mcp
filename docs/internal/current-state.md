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
> Keep this file honest. If you implement something, update it here.

## What we are building

A local MCP server giving an AI assistant structured, read-only observability
into the user's Linux machine — and eventually remote machines via the user's
own OpenSSH — through narrow, named capabilities rather than shell access.
Full rationale in [`architecture.md`](architecture.md); the decisions and
their reasoning are in [`decisions.md`](decisions.md).

## Current milestone

**v0.1 — local, read-only.** Four tools exist: `system_info`, `system_health`,
`container_list` and `container_health`.

## What actually exists

Everything, in full:

* A Rust binary crate (edition 2024) using `rmcp` 3.2.0.
* Eight source files, ~1,720 lines including their documentation:
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
* Every tool takes a required `target`; anything other than `"local"` is
  rejected with `invalid_params` (decision 15). `container_health` takes a
  `container` ID alongside it (decision 26).
* Four dependencies: `rmcp`, `tokio`, `serde`, and `chrono` — the last only
  to format `collected_at`, and already present transitively before it was
  declared (decision 19).
* `scripts/smoke.sh` — drives the server over stdio with real JSON-RPC and
  asserts fourteen things about the replies. Needs only `jq`; no MCP client, no
  Node, no test framework.
* `.github/workflows/ci.yml` — build, clippy, the guard's unit tests, and a job
  that flags any edit to `src/guard.rs` for review (decision 25).
* `scripts/probe-size/` — the harness that measured what `serde` costs a
  `no_std` probe (decision 28). Four freestanding variants of one storage
  capability plus `measure.sh`. **Deliberately not workspace members**: each
  pins its own link flags and must not inherit the server's profile or
  dependency versions, so `cargo build` at the root does not touch them and CI
  does not run them. It is kept because the first prototype was measured and
  then lost, which left a wrong estimate standing against the question; numbers
  that cannot be re-derived are worth little. It is **not** the beginning of the
  real probe.
* This documentation.

Verified by `scripts/smoke.sh`: the handshake, all four tools advertised,
`target` required on every tool, `target="local"` returning a hostname,
`target="nas"` rejected as `invalid_params`, and for `system_health` a
positive CPU count, memory in bytes rather than KiB, a `collected_at` within
two minutes of the shell's own clock, load shipped with its denominator, and no
verdict field anywhere in the payload; and for the container tools, a count
matching the list, no names or images, and an unknown container rejected as
`invalid_params`. Builds clean under `cargo clippy --all-targets`.

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
* Most of the eventual capability list: disk/storage, processes, listening
  ports, systemd, logs. Containers are now covered; storage is **designed and
  unbuilt** (decision 30), and nothing reads per-process data beyond the `comm`
  values `container_list` reports.
* **The probe architecture — decisions 28, 29, 31, 32, 33, 35, 36, 37, 38 and
  39 — is design only.** (Decision 34 is in that range and is *reversed*; it is
  not part of what is to be built.) There is no core crate, no workspace, no `no_std` anything, no
  probe binary, no `sftp` transfer, no target-side cache, no garbage collection,
  no hash verification, no in-memory target cache and no discovery call. The
  existing collectors are still `async` functions using `tokio::fs` inside the
  server binary, returning `rmcp::ErrorData` and formatting timestamps with
  `chrono` — the two dependencies decision 35 says a core crate cannot have.
  `StethoscopeMcp` is still a unit struct holding no state.
  `scripts/probe-size/` holds working probe prototypes, but they are a
  *measurement harness* — deliberately outside the workspace, not built by CI,
  and not a step toward the real thing.
* **Nothing of decision 33 exists.** One crate, not a workspace; no
  `[profile.probe]`, no `xtask`, no `build.rs`, no `include_bytes!`, no embedded
  digests, no `--payload-manifest` flag, no `rust-toolchain.toml`. The server
  takes no CLI arguments at all, and there is no `--extract-payload`
  (decision 38) either.
* **There is no release process.** No tagged release, no published binary, no
  probe archive, no manifest, no signing. Decision 38 describes three artifacts;
  zero of them have ever been produced.
* **Nothing verifies what a probe can do.** Decision 39 makes the
  syscall-set-by-disassembly check a CI control and a precondition for
  documenting elevated probes; today it is a sentence in decision 32 and no
  code. Nothing has ever disassembled anything here.
* **Nothing of decision 37 exists, and today's code is its opposite.** All four
  tools collect in-process, the server links every collector and the read guard,
  and nothing is ever written to the local filesystem. That is the arrangement
  decision 37 replaces. The end state — a server carrying no collection code,
  executing a hash-verified probe from a directory it owns, on `local` as much
  as on a remote host — arrives capability by capability as each tool is ported,
  and not before the last one moves.
* **No way for the model to learn what targets exist.** See open questions 9
  through 11.
* Mutations of any kind.
* Operator-facing logging beyond a single `eprintln!`.
* Configuration of any kind — no config file, no CLI flags, no env vars.
* Rust tests, outside `src/guard.rs`. `scripts/smoke.sh` is a black-box
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
   trustworthy even though the absolute numbers moved. The harness that produced
   these numbers is kept at `scripts/probe-size/`; re-run it rather than quoting
   a figure across a toolchain change.

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

15. **Where the SHA-256 comes from when the target is `local`.** Decision 33
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
* `scripts/probe-size/` is a measuring instrument, not a starting point. Do not
  grow it into the real probe, and do not let it drift into the workspace — its
  whole value is that its variants differ only in the one dimension being
  measured. If a number from decision 28 is load-bearing for a new argument,
  re-run `measure.sh` rather than quoting a figure that may predate a toolchain
  change. Note that its reason for being outside the workspace — nested
  `.cargo/config.toml` files, which cargo resolves by invocation directory —
  does **not** apply to the real probes, which are workspace members
  (decision 33). Do not read the harness's layout as the intended one.
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
