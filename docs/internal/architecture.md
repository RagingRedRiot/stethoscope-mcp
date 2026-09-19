# stethoscope-mcp — Architecture

> Internal working document. This is design memory for the humans and Claude
> sessions working on this project, not public project documentation.

## Purpose

`stethoscope-mcp` is a local MCP server that gives an AI assistant structured,
constrained observability into the user's machines — the local Linux machine
first, and eventually remote Linux machines reachable over SSH.

The motivating use case:

> "Why isn't this service/container/application working?"

Today answering that means the user repeatedly explains their environment and
hand-runs commands on the model's behalf, pasting output back. `stethoscope-mcp` lets
the assistant gather that operational information directly, through narrowly
defined tools that return normalized, structured results.

## The central principle

> **Give the model capabilities, not credentials or arbitrary machine access.**

`stethoscope-mcp` is a security boundary between the model and the operating
environment. The model asks for a *named operation on a named target*:

```text
storage_health(target = "nas")
```

It never receives a generic primitive:

```text
execute_shell(command)          # rejected
execute_ssh_command(target, cmd) # rejected
```

`stethoscope-mcp` may internally shell out to OS utilities or OpenSSH. The distinction
that matters is **who chooses the command**: trusted `stethoscope-mcp` code constructs
it, never the model.

### Why arbitrary shell is explicitly rejected

An `execute_shell` tool would collapse every boundary below into one: the
model would hold the user's full OS and SSH authority, gated only by the
model's own judgment and whatever the MCP client asks the user to approve.
It also produces unstructured text the model has to parse and guess at.

Refusing it is what makes the rest of the design meaningful. A narrow tool is
auditable: you can read `stethoscope-mcp`'s source and enumerate exactly what it can
do. That property is worth more than the convenience of a general escape
hatch, and it is lost permanently the moment one is added.

## The redesign, and what forced it (2026-09-11 → 09-12)

> Read this before the sections below. Everything from here to the end of
> [The payload](#the-payload) describes a design that is **two days younger than
> the code**, and knowing why it changed is what stops the older sections from
> being read as current.

The first four tools — `system_info`, `system_health`, `container_list`,
`container_health` — were built against a set of assumptions that were never
stated, because while everything was local and every collector was a file read
there was nothing to state. Designing remote collection is what made them
visible, one at a time, and every one of them turned out to be false:

* **"A capability is expressible as a set of file reads."** Invisible while all
  four tools happened to be file reads. `storage_health` is the first that is
  not — free space needs `statvfs(2)` — and listening ports, interface addresses
  and anything from `sysinfo(2)` are the same shape. Decision 20's frame
  protocol was built on this assumption and is superseded by decision 28.
* **"The server is the thing that reads."** `src/guard.rs` is a runtime
  allowlist inside a binary that links the code to read everything. That is a
  promise checked at runtime; across SSH there is nothing to check it with.
  Decisions 28 and 35 replace it with a link-time boundary — a storage probe has
  no cgroup-reading code in it at all — and decision 37 extends that to the
  binary a human installs.
* **"The server is stateless."** `main.rs` still says so. Probes are
  per-architecture, so a target's architecture has to be discovered and
  remembered; decision 31 is that state, and it is the seam decision 11 held the
  door open for.
* **"Read-only means we write nothing."** Remote collection has to put a
  collector on the target. Decision 32 draws the line that makes that compatible
  with decision 3 — additive inside a namespace we own, never mutative — and it
  is a genuinely different promise from the one v0.1 makes.
* **"`local` is the simple case."** The last and most stubborn one. Decision 34
  kept local in-process for exactly one day; decision 37 reversed it. The
  argument that won is not uniformity for its own sake — it is that ordinary
  local use then exercises location discovery, the ownership check, hash
  verification, exec and garbage collection every single call, which is the only
  continuous verification available for the most security-critical code here.

**What came out the other side is one mechanism.** Collection runs a
hash-verified probe from a directory the server owns; `local` is that minus SSH;
the server carries no collection code at all. That is a smaller design than the
one it replaces, and it is not the design the four existing tools were written
against.

**So the four existing tools are not wrong — they are pre-redesign.** They work,
they are correct about `/proc`, and they are the wrong *shape*. The sequencing
that follows from this is deliberate and is the plan of record:

1. **Build the current feature focus on the new design.** `storage_health` is
   the pilot precisely because it is new: it risks nothing that currently works,
   and it is the capability that broke the old assumption in the first place.
   Its floor is the whole chain — it ships when the payload chain works, or it
   does not ship (decision 37).
2. **Port the existing four only once the design has taken weight.** A design
   that has carried one real capability end to end has earned the right to be
   copied onto four working tools; one that has carried none has not. Each tool
   is ported, not patched, and it is not ported until it runs as a probe on
   `local`.

Until the last one moves, the server links the core crate and both shapes exist
side by side. That is expected, it is temporary, and `current-state.md` is the
file that says which is which on any given day.

## Local execution model (v0.1)

```text
Claude / MCP client
        |
       MCP (stdio)
        |
        v
     stethoscope-mcp
        |
        v
 local operating system
```

> **Updated 2026-09-12 by decision 37.** This is what exists today and it is not
> the end state. The server will not read the local machine itself: it places a
> capability probe in a directory it owns, verifies its hash and executes it,
> exactly as it will for a remote target minus SSH. The diagram gains one box —
> see [The payload](#the-payload) — and the server ends up linking no collection
> code at all. Written here rather than only in the decision log because "the
> server reads `/proc`" is the mental model this section currently teaches, and
> it is about to stop being true.

Launched by a local MCP client, `stethoscope-mcp` runs as the OS user who launched it,
and inherits exactly that user's permissions. This is intentional. (Collection
gains one operator-granted exception — see [The payload](#the-payload) and
decision 39 — which the server never grants itself and never needs. It is not a
v0.2 concern: under decision 37 the local machine runs a probe too, so an
operator can elevate a probe in `/opt/stethoscope` on this machine as readily as
on a remote one, from the first probe onward.) v0.1 deliberately does **not**
introduce:

* root privileges
* a dedicated `stethoscope-mcp` system account
* a daemon or privileged service
* containers
* any additional credential storage

If the user cannot read something, neither can `stethoscope-mcp`, and that is the
correct behavior — not a limitation to engineer around.

v0.1 is local, read-only inspection only. No mutations. No SSH.

### Practical consequence: stdout is the protocol

On a stdio MCP server, stdout belongs to the JSON-RPC transport. Nothing may
print to it. Diagnostics go to stderr (and eventually to an operator-facing
log). This is a real footgun; see the note at the top of `src/main.rs`.

## Remote architecture (v0.2, not built)

```text
Claude
   |
   | MCP
   v
stethoscope-mcp
   |
   | constrained operation
   v
system OpenSSH client
   |
   | user's existing SSH configuration/authentication
   v
remote host
```

### OpenSSH delegation

`stethoscope-mcp` delegates SSH connectivity and authentication to the system OpenSSH
client. It does **not** become an SSH credential manager, and does not store
SSH passwords, private-key passphrases, or equivalent secrets.

The user keeps managing SSH the normal way: `~/.ssh/config`, `~/.ssh/known_hosts`,
encrypted private keys, `ssh-agent`, hardware-backed keys, askpass/interactive
authentication, `ProxyJump`, `Include`, and so on.

We specifically do **not** require passwordless SSH keys. If a user's normal
SSH setup demands a passphrase, an agent, a touch on a hardware key, or an
interactive prompt, the design should preserve that behavior rather than
pressure the user into weakening it. (How interactive auth surfaces through an
MCP server is an open question — see `current-state.md`.)

The division of responsibility:

> **OpenSSH owns connection and authentication.**
> **`stethoscope-mcp` owns model-facing authorization and capabilities.**

### SSH config as target inventory

There is deliberately no `stethoscope-mcp` target configuration file. The user's
existing OpenSSH configuration is the eventual canonical remote inventory —
it already exists, the user already maintains it, and duplicating it would
create a second source of truth that drifts.

But **SSH access to a host does not imply the model may inspect it.** Opt-in
is explicit, via an OpenSSH `Tag`:

```sshconfig
Host nas
    HostName 192.168.1.50
    User some-user
    IdentityFile ~/.ssh/some-key
    Tag stethoscope-mcp
```

If discovery is implemented, it stays deliberately narrow: find literal `Host`
aliases whose applicable entry is tagged `stethoscope-mcp`, and leave every other bit
of SSH semantics to OpenSSH. We are not writing an SSH configuration
implementation — OpenSSH already owns those semantics, including wildcards,
match blocks, and precedence rules. Do not prematurely solve them.

`Include` may eventually matter for discovery. It is not a bootstrap concern.

### Batched retrieval and the frame format — **superseded, do not build**

> **Superseded 2026-09-11 by decision 28.** Remote collection runs a capability
> probe that emits the finished payload, so there are no streamed files to
> reassemble and this frame format has no job to do. Everything below is kept
> because the reasoning behind it still holds and shaped what replaced it —
> legibility to whoever audits the command, a length prefix assuming nothing
> about content, and the measured cost of round trips. The *mechanism* is
> retired; the *constraints it was satisfying* are what decision 28 inherits.
> One thing here did not survive contact with a real requirement: this design
> assumes every collector is expressible as a set of file reads, which
> `storage_health` broke by needing `statvfs(2)`. See decisions 28 and 30.

One tool call is one SSH invocation. The remote command reads every file the
tool needs and writes them as a stream of self-describing frames. Decision 20
covers why this is plaintext and length-prefixed rather than encoded;
decision 21 covers why frames carry their path. This section is the mechanism,
recorded so it is not re-derived at implementation time.

**Wire format.** A frame is a header line, then — for `FILE` only — exactly
the number of bytes the header announces, then a single newline:

```text
FILE <path> <byte-count>\n
<byte-count bytes, verbatim>\n
MISSING <path>\n
```

**Reader.** The cursor arithmetic is the whole algorithm. Content is never
inspected and never scanned:

```text
cursor = 0
while cursor < input.len():
    eol = index of b'\n' at or after cursor        # no newline left -> stop
    header = input[cursor .. eol]

    "FILE <path> <n>":
        start = eol + 1
        end   = start + n
        if end + 1 > input.len(): -> truncated, stop
        if input[end] != b'\n':  -> length disagreement, fail the batch
        emit (path, Present(input[start .. end]))
        cursor = end + 1

    "MISSING <path>":
        emit (path, Missing)
        cursor = eol + 1

    anything else:
        skip the line                              # rc-file / motd noise
        cursor = eol + 1
```

**Why it cannot be confused by content.** The reader takes `n` bytes because
the header said `n`, so a file whose content happens to contain a line reading
`FILE /proc/evil 999` is returned as content and never becomes a frame. This is
the property a delimiter cannot have, and it is worth preserving in any future
variant of the format.

**The framing is self-checking.** After a well-formed stream the cursor lands
exactly on `input.len()`. Any other outcome means the stream is corrupt, and it
is detectable at parse time rather than as implausible values several layers
higher.

**Invariants for the implementation:**

* Slice **bytes**, not characters. `wc -c` counts bytes, and `/proc/version`
  can carry non-ASCII in a compiler version string. Convert to UTF-8 after
  slicing, never before.
* Verify the byte following the content is `\n`. It is the cheapest available
  integrity check on the announced length, and everything after a mismatch is
  misaligned.
* Never pre-allocate from `n`. Bound it against the bytes actually remaining,
  so a corrupt header cannot request an enormous allocation.
* Do not require the response to be complete. Frames that arrived are usable
  whether or not later ones did.

**Behavior on malformed input**, verified by implementing the reader above
verbatim and running it against each case:

| input | result |
|---|---|
| well-formed batch (7 files) | 7 frames, cursor lands exactly on `len` |
| content containing a fake `FILE` header | returned as content; no phantom frame |
| junk lines before/after payload | junk skipped, all frames recovered |
| truncated mid-content | completed frames kept, stream flagged incomplete |
| header length disagrees with content | batch rejected |
| absurd length in a corrupt header | no allocation, no panic, stream flagged incomplete |
| empty input | zero frames; caller sees every path absent |

**Three outcomes, matching the local reader.** The map returned to the tool
answers each requested path in one of three ways, which is the same three-way
result `proc::read` and `proc::read_optional` already produce locally:

| outcome | required file (`read`) | optional file (`read_optional`) |
|---|---|---|
| `Present(bytes)` | content | `Some(content)` |
| `Missing` | error: file absent | `None` |
| absent from response | error: read failed | `None` |

## The payload

> Decisions 28, 29, 31, 32, 33, 37, 38, 40, 41 and 42. **Built for `local` and
> one capability as of 2026-09-18** — embedding, discovery, the ownership and
> hash checks, placement and exec, in `src/prologue.rs`. Everything remote and
> everything release-shaped is still design; `current-state.md` has the split.

Remote collection does not send a command; it runs a program. This section is
the end-to-end life of that program, because it is the part of the design that
spans the most decisions and is therefore the easiest to hold incoherently.

```text
  build time                    server start           first use of a target
  ──────────                    ────────────           ─────────────────────
  xtask builds one probe        binary carries the     discovery: arch, identity,
  per capability × arch    ───► payload set and   ───► location, what is present,
  build.rs embeds bytes         its digest set          whether it is really ours
  and digests                                                    │
                                                                 ▼
                                                      execute what is already
                                                      installed, or place the
                                                      missing probe in a directory
                                                      we own, mode 700 (sftp if
                                                      remote, write if local)
                                                                 │
                                                                 ▼
                                                      verify hash, exec, read one
                                                      JSON document from stdout
```

**`local` takes this same path.** It skips SSH, `uname` and the reachability
check, and it writes with `write(2)` rather than `sftp` — everything else is
identical, including the owned directory, the mode-700 rule, the hash check and
garbage collection. That is decision 37, and it reverses decision 34's
one-day-old carve-out for an in-process local path.

**One release, one artifact — and nothing else is required.** There is no probe
package that must be installed and no fleet-wide rollout. The server binary *is*
the payload, and drops what a given target needs, when that target needs it
(decision 33).

**Pre-installing is offered, not required, and it is a genuinely different
posture.** An operator who creates `/opt/stethoscope` and puts the probes there
gets a server that only ever *reads and executes* on that target — it writes
nothing at all, because there is nothing left to place. Decision 38 covers how
they obtain the binaries: releases ship them pre-named with their hashes, and
`--extract-payload` produces them from the server binary itself.

`/opt/stethoscope` is also the only place an operator may grant a probe more
authority than the invoking user has — file capabilities, or the setuid bit, on
one specific hash. That is opt-in, never requested by this software, never
required by any capability, and reported in the payload when it is in effect;
decision 39 has the rules and the preconditions. It is the one deliberate
exception to the promise below.

`/opt/stethoscope` is a **read-only** location, without exception. The server
never creates it, never writes to it, and never tests whether it could — writing
there requires root, and collection must never run as root. Discovery is
therefore two chains rather than one: a read chain (`/opt`, then `$HOME`)
asking *is there a payload here I can verify and execute*, and a write chain
(`$HOME` alone) asking *where may I place one*. `/tmp` is in neither (decision
40): a cache the OS sweeps is not a cache. Using what it finds takes more than a
matching name — the hash must verify, the directory and the one above it must
be writable by nobody less privileged than us, and the file must actually be
executable (decision 42 has the full table).

That same directory is the escape hatch for a host where `$HOME` is `noexec` —
the case decisions 37 and 40 otherwise decline to serve.

**There is exactly one collection mechanism.** Not "one for local, one for
remote, kept in agreement" — one. The reason is not elegance: it is that
ordinary local use then exercises location discovery, the ownership check, hash
verification, exec, output parsing and garbage collection every single call, on
the development machine, forever. That is the strongest verification available
for the most security-critical code here, and it costs a fork.

Two consequences follow, and neither is hidden:

* **A machine with no writable, executable location cannot be inspected** —
  including the local one. The operator's fix is one directory
  (`/opt/stethoscope`). Decision 37 accepts this deliberately.
* **The server carries no collection code.** No `/proc` parsing, no cgroup
  walking, no `statvfs`, no read guard. Decision 28's link-time capability
  boundary applies to the binary a human installs, not only to what it pushes.

**The same code answers both, because it is the same code and the same
process shape.** Each probe links the core crate with exactly one capability in
it, and nothing else runs a collector. That is what turns decision 11's
normalization from an intention maintained by two code paths agreeing into a
structural property (decisions 28, 35 and 37).

**What is durable and what is not is deliberately inverted.** The probe on a
target persists across sessions and reboots; the server's knowledge of it is
rebuilt from scratch every run (decision 31). Ephemeral local state describing
durable remote state.

**Everything the server places is legible on purpose.** A named binary in a
stable location under a filename that *is* its own content hash, transferred by
a protocol that logs per-file records, published in a manifest an analyst can
allowlist before the binary ever reaches a host. Every one of those choices had
a smaller, faster, quieter alternative — inline shell, stdin transfer,
`memfd_create`, a self-deleting probe — and each was rejected for looking like
what an attacker would do. See decisions 28, 29 and 33.

## The model-visible information boundary

A target may internally resolve to:

```text
alias:     nas
hostname:  192.168.1.50
username:  example-user
identity:  ~/.ssh/example-key
jump host: example-bastion
```

The model needs to see only:

```json
{
  "target": "nas",
  "platform": "linux",
  "capabilities": ["system", "storage", "processes", "containers"]
}
```

Usernames, addresses, ports, key paths, and ProxyJump topology stay
implementation-private. OpenSSH resolves them when `stethoscope-mcp` eventually
invokes the equivalent of `ssh nas <trusted operation>`.

Errors respect the same boundary. Prefer:

```json
{ "target": "nas", "error": "authentication_failed" }
```

over an SSH diagnostic dump containing the username, address, and key path.
Detailed diagnostics belong in local logs intended for the human operator.

The general rule:

> **Information available to the `stethoscope-mcp` process is not automatically
> information that should enter model context.**

This already applies locally: `read_proc` in `src/main.rs` logs the underlying
`io::Error` to stderr and returns a flat message to the model.

## Authorization model

The effective permission model is a conjunction — defense in depth:

```text
  User's OS permissions
    AND user's SSH permissions
    AND target explicitly opted into stethoscope-mcp
    AND operation explicitly implemented/allowed by stethoscope-mcp
  = capability available to the AI
```

Possession of SSH access alone does not imply AI authorization. Opting a
machine into `stethoscope-mcp` does not imply arbitrary command execution on it.

## Local/remote normalization

Local and remote targets should eventually expose the same model-facing
operations returning the same response shape:

```text
storage_health(target = "local")
storage_health(target = "nas")
```

```text
                   Target
                     |
             +-------+-------+
             |               |
          Local            Remote
                             |
                          OpenSSH
```

How the information was collected is an implementation detail.

Tools are therefore named for the *operation*, not the transport:
`system_info(target)`, never `local_system_info` (decision 15). The parameter
is required — an ops tool pointed at the wrong machine is a real hazard, so
the model always states which machine it means.

**This is a design intention, not a code structure.** A Rust trait for targets
will probably make sense eventually. It does not exist and should not be
created until a second implementation actually pushes on it — one
implementation cannot tell you where the seam belongs. Today `target` is a
`String` compared against one constant; that is the whole mechanism, and it is
deliberately not a target type, trait, or registry.

> **Updated 2026-09-11.** A second implementation is now pushing, in design if
> not yet in code: decision 28's probes are per-architecture, so a target's
> architecture has to be discovered and remembered, and decision 31 is that
> state. The seam is therefore no longer hypothetical — it is an in-memory map
> of per-target facts, rebuilt every run. That licenses exactly that map. It
> still does not license a trait, a registry, or a dispatch layer written ahead
> of a caller that needs one, and `target` remains a `String`.

> **Updated 2026-09-12.** The seam is much smaller than "local versus remote,"
> and knowing where it actually falls matters before the pilot draws it in the
> wrong place. Under decision 37 local and remote do not diverge at collection
> *or* at the process boundary: both execute the same probe binary and parse the
> same JSON. They diverge only at **transport** — how the bytes get to the
> directory and how the process is started, `write` and `fork` here against
> `sftp` and `ssh` there. Everything else in the chain is shared: location
> discovery, the ownership rule, hash verification, output parsing, garbage
> collection.
>
> So the abstraction this design will eventually want is a *transport*, not a
> target type — and it will have two implementations that differ in perhaps two
> functions. That is a much better-defined seam than decision 11 could have
> guessed at, and it is still not a reason to build it before the second
> implementation exists.

## Milestone boundaries

| Milestone | Scope | Status |
|---|---|---|
| v0.1 | Local, read-only inspection | in progress — five tools exist; `storage_health` and `system_info` run as probes, the other three are still in-process |
| v0.1 infrastructure | Workspace, core crate, embedded payload, probe chain on `local`, `xtask` release | in progress — workspace, `xtask dev`, embedding and the probe chain on `local` built (decisions 40–42); core holds types only; no `xtask release`, manifest, extraction or disassembly check (decisions 33, 35, 38, 39). 34 is reversed and is not part of the plan |
| v0.2 | Remote targets via user's OpenSSH; `Tag stethoscope-mcp` discovery; capability probes | designed, not started — decisions 28, 29, 31, 32; blocked on interactive SSH auth |
| v0.3 | Narrowly scoped mutations, after an explicit security design discussion | not started, deliberately |

**The payload machinery lands in v0.1, ahead of the transport that needs it.**
That looks like scope creep on a milestone defined as "local, read-only" and is
not: the core-crate extraction, the workspace and the embedded payload are what
make a probe *buildable at all*, and the whole chain — location discovery, the
ownership rule, hash verification, exec, garbage collection — can be exercised
against the local filesystem with no SSH involved. Deferring it to v0.2 would
mean the most security-critical code in the design gets its first real run on a
remote host, behind an authentication question nobody has answered. What stays
firmly in v0.2 is the transport: `sftp`, the SSH config inventory, discovery
over the wire, and the target cache.

v0.2's write to a target — caching a collector there — is **not** the beginning
of v0.3. Decision 32 draws that line: adding to a namespace the server owns is
not mutating the target, and v0.3's deferral is untouched.

v0.3 (restart an allowlisted container or service) must not begin as a
casual feature addition. It requires deciding authorization, confirmation,
auditing, allowlisting, and MCP safety semantics first.
