# stethoscope-mcp — Decision log

> Lightweight ADR log. The point is to stop a future session — or a future us —
> from casually reversing a decision without understanding why it was made.
>
> If you want to reverse one of these, that is allowed. Read the rationale
> first, then record the reversal here with its own reasoning. Do not silently
> drop one.

Status legend: **accepted** · **superseded** · **reversed** · **open**

*Superseded* means a later decision replaced the mechanism while the problem
and most of the reasoning carried forward. *Reversed* means a later decision
concluded this one was wrong. Both are kept in full.

---

### 1. Implement in Rust — accepted

This is a learning project and Rust is the thing being learned. Beyond that it
suits the job: a single static binary an MCP client can spawn, no runtime to
install, and a type system that makes "what exactly can this tool do" legible.

### 2. Local stdio MCP server first — accepted, **one exception added by decision 39**

The MCP client spawns `stethoscope-mcp` as a subprocess and talks JSON-RPC over
stdin/stdout. No network listener, no ports, no HTTP server, no auth layer.
The process runs as the invoking user and inherits their permissions.

An HTTP transport is explicitly *not* wanted: it would turn a
runs-as-you subprocess into a service with its own identity, lifetime, and
access-control problem.

Consequence: stdout is reserved for the protocol. Diagnostics go to stderr.

> **One exception added 2026-09-12 by decision 39.** "The process runs as the
> invoking user and inherits their permissions" remains true of the *server*,
> without exception. It stops being unconditionally true of *collection*: an
> operator may attach file capabilities or the setuid bit to a specific probe
> hash installed in `/opt/stethoscope`, and collection through that probe then
> exceeds the invoking user's authority. The server never grants itself
> anything, never requests the grant, and works unprivileged by default — but
> the README's flat promise needs the qualifier.

### 3. v0.1 is read-only — accepted, **scope clarified by decision 32**

Inspection only. No mutations of any kind. Getting the read-only architecture
and its boundaries right is the whole point of v0.1; adding writes early would
mean designing the security model under pressure from a feature.

> **Clarified 2026-09-11.** Still exactly true of v0.1, which is local and
> writes nothing whatsoever. Decision 29 pushes a collector to remote targets in
> v0.2, which required saying precisely what "read-only" forbids; decision 32
> is that statement. It narrows nothing here — it draws the line between
> *modifying* a target and *adding to a namespace we own*, and decision 12's
> deferral of real mutations is untouched.

### 4. No arbitrary shell tool — accepted, load-bearing

No `execute_shell(command)`, no `execute_ssh_command(target, command)`, no
tool that accepts a command, command fragment, or shell string from the model.

`stethoscope-mcp` may run OS utilities internally, but *trusted code selects and
constructs the command*. This is the decision the entire project rests on: it
is what makes `stethoscope-mcp` a boundary rather than a transport. Adding a general
escape hatch would nullify decisions 8, 9 and 12 at the same time, and the
narrow tools would become pointless overhead around it.

If a needed operation is not implemented, the answer is to implement that
operation, not to add a general one.

### 5. Use the system OpenSSH client, not an in-process SSH library — accepted

When remote support arrives, `stethoscope-mcp` will invoke the user's OpenSSH client
rather than link an SSH implementation.

OpenSSH already correctly implements config resolution, host-key verification,
agent protocols, PKCS#11/FIDO tokens, `ProxyJump`, and multiplexing. An
embedded library means reimplementing that, diverging from the user's actual
SSH behavior, and taking on a credential-handling role we do not want.

### 6. Do not require passwordless SSH keys — accepted

A tempting shortcut is "just set up a passwordless key for stethoscope-mcp". Rejected:
it degrades the user's security posture to make our implementation easier, and
it creates exactly the standing unattended credential we are trying not to
have.

Whatever authentication the user's normal SSH requires — passphrase, agent,
hardware token, interactive prompt — the design should preserve. See the open
question in `current-state.md` about how interactive auth surfaces through a
stdio MCP server; it is a real problem and it is not a reason to reverse this.

### 7. The user's existing SSH config is the eventual remote inventory — accepted

Remote targets come from `~/.ssh/config`, which the user already maintains.
No separate host list to keep in sync, nothing to duplicate, nothing to drift.

### 8. `Tag stethoscope-mcp` is the explicit opt-in — accepted

Having SSH access to a host does not mean the model may inspect it. Only hosts
whose applicable config entry carries `Tag stethoscope-mcp` are eligible.

Opt-in, not opt-out: adding a host to `stethoscope-mcp`'s reach must be a deliberate
act by the user, and it lives in the same file they already edit. Reading the
whole config and exposing everything in it would silently hand the model the
user's entire infrastructure the first time they run the server.

### 9. Do not expose SSH usernames, addresses, or key paths to the model — accepted

The `stethoscope-mcp` process can see these; that does not make them model-facing
data. The model gets the target alias and what it can do. Errors are
normalized (`"authentication_failed"`), not raw SSH diagnostics.

Detail belongs in operator-facing logs. Rationale: model context is
copied, summarized, and persisted in places the user did not choose; the
information boundary should be drawn at the point of production, not left to
downstream handling.

### 10. No `stethoscope-mcp` target config file unless a concrete requirement appears — accepted

We are not creating a config format on speculation. If a real requirement
emerges that SSH config genuinely cannot express, revisit *then*, with the
requirement in hand to shape the design.

### 11. Local and remote must eventually produce normalized results — accepted (intent, not code)

`disk_usage(target="local")` and `disk_usage(target="nas")` should return the
same shape; collection method is an implementation detail.

Explicitly **not** a license to build a target abstraction now. With one
implementation you cannot tell where the seam goes, and a trait invented ahead
of its second implementer usually ends up wrong. Introduce it when a real
remote implementation pushes on it.

### 12. Mutations deferred to a security-focused milestone — accepted

Restarting a container or service is v0.3 at the earliest, and it starts with
a design discussion — authorization, explicit allowlists, user confirmation,
audit trail, MCP safety semantics — not with an implementation.

The read-only architecture must be mature first. A mutation added
opportunistically to a read-only design is how this project would go wrong.

---

## Bootstrap-session decisions (2026-09-03)

### 13. `rmcp` 3.2.0 as the MCP SDK — accepted

The official Rust MCP SDK (`modelcontextprotocol/rust-sdk`), verified current
at bootstrap time. 3.x is a major break from 2.x, so **treat pre-3.x examples
found online as stale** — the breaks are concentrated in HTTP/OAuth/tasks, but
the API surface moved.

Two things verified against the 3.2.0 source, both easy to get wrong:

* `Implementation::from_build_env()` resolves `CARGO_CRATE_NAME` inside *rmcp*,
  so a server using it reports its name as `rmcp`. We set identity explicitly.
* `#[tool_handler]` defaults to a router expression of `Self::tool_router()`,
  so it never reads a stored `tool_router` field. The struct is therefore a
  stateless unit struct; the usual `tool_router` field + constructor is dead
  weight unless you opt in with `#[tool_handler(router = self.tool_router)]`.

### 14. Minimal dependency set — accepted, **updated by decisions 33, 35 and 41**

`rmcp` (no default features), `tokio` (no default features), `serde`. That is
all. Notably absent and deliberately so:

* `sysinfo` / `procfs` — `local_system_info` reads two `/proc` files with
  `std::fs`. Add a crate when the data actually needs one, not before.
* `tracing` / `tracing-subscriber` — decision 9 implies operator-facing logs
  eventually, but a single `eprintln!` covers today's needs. See the open
  question in `current-state.md`.
* `anyhow` / `thiserror` — three error sites do not justify an error crate.

> **Updated 2026-09-12.** Three changes arrive with the probe architecture, and
> the count is still small. `serde_json` becomes a *direct* dependency of the
> server, which must parse what a probe emits — it was already present
> transitively through `rmcp`. `sha2` enters as a **build-dependency** only, to
> digest the embedded payload at build time (decision 33); it is not linked into
> the shipped binary. And `chrono` **leaves** once `system_health` moves to the
> core crate, because a `no_std` core formats RFC 3339 itself (decision 35).

> **Updated 2026-09-18 by decisions 41 and 42.** `sha2` is now linked into the
> shipped binary as well as used at build time — the local machine's probe is
> hashed in-process (decision 41), so "not linked into the shipped binary" above
> is no longer true. `libc` becomes direct, for `fgetxattr` (file capabilities
> are invisible to the hash), `geteuid` and `O_NOFOLLOW`; it was already present
> transitively through `tokio`. `stethoscope-core` is a path dependency holding
> the storage wire types. `tokio` gains its `process` and `time` features, to run
> a probe and kill it at a deadline — decision 16's `tokio::process` arriving.

### 16. Blocking work goes to the blocking pool, not an async worker — accepted, **subject changed by decision 37**

Tools are `async` and do their I/O through `tokio` (`tokio::fs` today), even
where the underlying operation is not genuinely asynchronous.

`tokio::fs` is not async file I/O — it wraps the ordinary blocking `std::fs`
call in `spawn_blocking`. That is precisely what we want: rmcp `tokio::spawn`s
each incoming request onto the runtime, so a tool that blocks inline occupies
an async worker thread and stalls every request running beside it. Routing the
wait to the blocking pool keeps the workers free.

For `/proc` this is close to pure ceremony — those reads are kernel-generated
and return in microseconds, and the `spawn_blocking` hop costs more than the
wait it avoids. It is adopted as a *policy* rather than an optimization,
because the cost of applying it uniformly is negligible and the cost of
forgetting it exactly once is a frozen server.

The case that will genuinely need it is v0.2: an `ssh` subprocess can hang for
30 seconds on a dead host, or indefinitely on a passphrase prompt. Use
`tokio::process` there — note that `tokio::fs` would not have helped with a
subprocess at all.

> **Subject changed 2026-09-12 by decision 37.** The policy is unchanged; what
> it applies to is not. Collection leaves the server for the probes, and
> `tokio::fs` leaves with it, so the server's blocking work becomes almost
> entirely subprocess management — exactly the case the paragraph above named as
> the one that would genuinely need this. `tokio::process` becomes the main
> instrument and `spawn_blocking` nearly disappears. The last sentence turns out
> to be the operative one.

Cost: tokio's `fs` feature, which is the only change to the dependency set in
decision 14.

### 15. Tools are named for the operation and take a `target` — accepted

Settles open question 4. The tool is `system_info(target)`, not
`local_system_info` / `remote_system_info`. Decision 11 says local and remote
must eventually return the same shape; a tool named for its transport
contradicts that before the second implementation even exists. Settled now
because renaming tools later breaks anything referencing them, and this is the
cheapest moment — there is one tool.

Three sub-decisions, all reversible but worth stating:

* **`target` is required, not optional-defaulting-to-`"local"`.** Ops tooling
  aimed at the wrong machine is a real hazard, so the model should always say
  which machine it means. It also keeps the signature stable across v0.2:
  making a required parameter optional later (or the reverse) silently changes
  how the model calls it.
* **`target` is a `String`, not an enum.** Remote targets will be
  runtime-discovered SSH aliases, not compile-time variants, so `String` is
  the correct long-term shape. Validation is a runtime comparison against
  `LOCAL_TARGET`. Note this is *not* a target abstraction — decision 11 still
  holds; there is no target type, trait, or registry.
* **An unknown alias is `invalid_params`.** This does not settle open
  question 2, which is about a *known but unreachable* target. Unknown-alias
  is request validation; reachability is a remote concern that does not exist
  yet.

---

## Session decisions (2026-09-04)

### 17. `/proc` and `/sys` are the primary local collection substrate — accepted

Local data comes from the kernel's virtual filesystems by preference, not from
running OS utilities and parsing their output.

Three reasons, in order of weight:

* **It reinforces decision 4 mechanically.** A `/proc` read constructs no
  command at all. There is no argument, no quoting, no `PATH` lookup — so
  there is nothing for a later change to accidentally let the model influence.
  Wrapping `ps` or `df` means a command string exists in the code, and a
  command string is a thing that can grow a parameter.
* **It is the same everywhere.** `df`, `ps`, `ss` and `free` differ in flags
  and output format across distributions, and busybox differs from all of
  them. `/proc/meminfo` does not. This is what makes decision 11 mostly fall
  out for free: the parser written against a local read works byte-identically
  against a remote `cat` of the same path, so local and remote normalization
  is not an engineering problem for anything `/proc` covers.
* **It needs no dependency.** `tokio::fs` is sufficient, which keeps
  decision 14 intact.

`/sys` — specifically `/sys/fs/cgroup` under cgroup v2 — is treated as the
same substrate: same properties, different mount point. It is where
per-container PSI, `cpu.stat` throttling counters and `memory.events` OOM
counts live, and it is the *only* place a starved or quota-throttled container
is visible. Host-level PSI is structurally blind to it, because CPU `full`
pressure is always zero at the system level and only becomes meaningful
per-cgroup.

Recorded limits, so a future session does not rediscover them mid-implementation:

* **No free-space data anywhere in `/proc`.** `disk_usage` needs `statfs(2)`.
  That is `architecture.md`'s own worked example, and it is the one early
  capability this decision does not serve.
* **`/proc/net/tcp` yields socket inodes, not PIDs.** Mapping a listening port
  to a process means scanning `/proc/*/fd`, which only succeeds for processes
  the user owns. A partial answer is the correct answer here; the response
  shape should be able to say "listener exists, owner unknown".
* **systemd unit state, journal logs, and Docker container names/images are
  not in `/proc`** in any usable form. They are D-Bus, the journal, and the
  Docker socket respectively.
* **Container cgroups identify containers by ID, never by name.** Everything
  about a container's resource behaviour is readable without Docker; the name
  is not.

Where a capability genuinely requires a different mechanism, that is a reason
to use that mechanism, not a reason to reach for a general command tool.
Decision 4 is unaffected.

### 18. Normalized values in the payload, interpretation in the schema, no verdicts — accepted

Settles open question 5 ("how much of `/proc` is worth normalizing"). The
answer is neither "raw" nor "cooked" but a split by *kind* of transformation:
mechanical conversion and arithmetic go in the payload, judgment does not go
in the payload at all.

1. **Normalize units at the boundary.** `meminfo`'s `kB` is really KiB and PSI
   totals are microseconds. Convert to bytes; name microsecond fields `_us`.
   Lossless and mechanical, so there is no argument for passing the footgun
   downstream where each consumer re-learns it and one of them gets it wrong.

2. **Never return a number without its denominator.** Load average without
   `cpu.count` is uninterpretable; `available_bytes` without `total_bytes` is
   uninterpretable. This is the rule to hold hardest, because breaking it
   produces confident wrong conclusions rather than visible errors.

3. **Compute derived values where the computation is arithmetic, not
   judgment.** `load_1m_per_cpu`, `available_percent`, `swap_used_bytes`,
   busy fraction. The model would otherwise derive these itself and can get
   them wrong — dividing load by the wrong core count is a realistic failure.
   Doing it in trusted code is deterministic, and nothing is lost because the
   inputs ship alongside.

4. **No verdicts in the payload.** No `"status": "critical"`, no
   `"health": "degraded"`. Thresholds are workload-dependent — a batch host at
   load 30 is working as designed, a latency-sensitive service at load 3 may
   not be — so a fixed threshold is wrong for half its uses. And a label is
   lossy in precisely the wrong direction: it discards the number needed to
   reason about the *specific* failure. Collecting is this server's job;
   judging is not.

5. **Interpretation belongs in the JSON Schema, not the response.** Tool and
   field descriptions are model-facing prose sent once at `tools/list` time,
   not per call. That is the right home for "MemFree is deliberately absent",
   "load counts I/O-blocked tasks as well as CPU-waiting ones", and
   "`full_avg10` above 10 means all runnable work was stalled". `schemars`
   lifts `///` doc comments into the schema, so the comment that documents a
   field for a human reviewer *is* the model's reading instruction — one text,
   both audiences, zero per-response cost.

6. **Omission is a design act, and a documented one.** `MemFree`,
   `Active`/`Inactive`, `DirectMap*` and `Vmalloc*` are left out: every field
   returned is a field the model will try to use, and `MemFree` in particular
   invites the most common misreading of `meminfo`. Note notable omissions in
   the schema so they read as deliberate rather than as a bug.

7. **A field whose source may not exist is optional, not an error.** PSI needs
   kernel 4.20+ and can be compiled out or disabled; an older remote host may
   lack it. The absence of one file must not fail the whole call. This is
   decision 11's normalization pressure arriving early, and it is far cheaper
   to build in now than to retrofit.

### 19. `system_health` carries `collected_at`, from the target's own clock — accepted

The response includes a wall-clock timestamp, RFC 3339 in UTC, derived from
`/proc/stat`'s `btime` plus `/proc/uptime`.

**Not for measuring intervals.** The obvious motivation — letting the model
difference two readings to turn the PSI `*_total_us` counters into rates — is
already served by `uptime_seconds`, and served better: it is monotonic, so it
is unaffected by NTP steps, DST, or a clock that is simply wrong. The field
documentation says so explicitly, because handing the model two ways to
compute elapsed time invites it to pick the one that breaks.

What the timestamp actually answers:

* **Staleness.** Model context is summarized, cached and persisted; a reading
  from forty minutes ago looks exactly like a fresh one unless it is dated.
* **Correlation across targets.** The v0.2 case, and the reason not to defer
  this: once `local` and `nas` readings sit side by side, "were these taken at
  the same moment?" is a real question and uptime cannot answer it — two
  machines have unrelated uptimes.
* **Correlation with external evidence**, such as a log line's timestamp.

**Boot time plus uptime, not `SystemTime::now()`.** The value must be the
*target's* clock, not `stethoscope-mcp`'s, or it is a claim about a machine whose clock
was never consulted. `btime` is in `/proc/stat`, which `system_health` already
reads, so this costs no extra read and stays a pure file parse that will work
unchanged over SSH (decision 17). Verified exact against `date` on the
development machine. Second resolution, since `btime` is only recorded to the
second, and the timestamp is formatted without a fractional part rather than
implying precision it does not have.

A wrong clock on the target produces a wrong `collected_at`. That is
information about the target, not a defect, and the field documentation says
so — skew between hosts is itself an operational problem worth seeing.

**Rejected: having `stethoscope-mcp` compute the delta itself.** A "stall since your
last call" field would require the server to remember the previous reading.
That makes it stateful, the state would be per-session so two clients would get
different answers to the same question, and it would silently choose a
measurement window the caller never asked for. Return the inputs; let the
caller difference two responses.

**`chrono` promoted to a direct dependency.** It was already in the tree
transitively, via both `rmcp` and `schemars`, so declaring it adds no build
time and no supply-chain surface — `Cargo.lock` gained one line and no new
crates. It is recorded here because decision 14 states the dependency set is
deliberate, and a fourth entry should not appear in `Cargo.toml` without a
reason written down. Declared `default-features = false`, matching the other
three: only formatting is needed, not `chrono`'s clock.

---

## Session decisions (2026-09-05)

### 20. The remote batch is plaintext and length-framed, never encoded — accepted, **superseded by decision 28**

> **Superseded 2026-09-11.** The frame protocol below should not be
> implemented: decision 28 ships a probe binary that emits the finished payload,
> so there are no streamed files to reassemble. Everything here about *why*
> — legibility to an analyst, batching versus round trips, and the measured
> latencies — carries forward and is why decision 28 looks the way it does.
> The measurements in particular are still the best numbers we have.

One SSH invocation returns every file a tool needs. The response is a sequence
of frames: a header line, then exactly the bytes it announces.

```
FILE /proc/meminfo 1614
MemTotal:        8192000 kB
...
MISSING /proc/pressure/cpu
```

Three reasons, in order of weight:

* **The command stays legible to whoever audits it.** A `base64` in a remote
  command is an exfiltration signature, and an operator reading `auditd` should
  see `cat /proc/meminfo`, not an encoder. The genuine indicator is an encoded
  *command* — `echo <blob> | base64 -d | sh` — and encoding only the output is
  benign by comparison, but telling the two apart requires reading the
  direction of a pipe, and a reviewer should not have to. This extends
  decision 4 one step: there is not only no command string the model can
  influence, there is nothing that *looks* like there might be.
* **A byte count is stronger than any delimiter.** A delimiter has to be a
  string the content cannot contain, which is an assumption about content a
  future kernel is free to break. A length prefix assumes nothing: the reader
  counts, it never scans. Verified against a hostile payload — a file whose
  content contains a literal `FILE /proc/evil 999` line frames and recovers
  intact, and the fake header never reaches the map.
* **It is smaller.** 5,670 bytes against 7,343 for the same seven files;
  base64 inflates by a third.

Measured against a remote host over a relayed link of roughly 47 ms
round-trip latency:

| | |
|---|---|
| seven separate round trips | 3,761–4,063 ms |
| one batched round trip | 530–556 ms (**7.2x**) |
| cold connect | ~750 ms |
| `ControlMaster` reuse | ~560 ms |

Batching is not an optimization here, it is the difference between a usable
tool and an unusable one — and note that connection pooling recovers far less
than batching does, so it is not a substitute.

**Decision 17 verified, not merely asserted.** Every parser in `proc.rs`, run
unmodified against bytes pulled from a remote host, produced values matching
that host's own tools exactly — memory and swap against `free -b`, CPU count
against `nproc`, uptime against `uptime -p`, and `collected_at` tracking
`date -u`. The claim that a local parser works byte-identically on a remote
`cat` is now measured.

Recorded limits, so a future session does not rediscover them:

* **Never build the frame through a pipe.** `base64 <"$f" | tr -d '\n'` and
  every piped variant return the *pipeline's* exit status, not the redirect's,
  so a missing file reads as a successful empty read — silently collapsing the
  required-versus-optional distinction `read_pressure` depends on. Guard with
  `[ -r "$f" ]` or use no pipe at all.
* **`/proc` files report size 0.** `tar` recovers zero bytes from them and any
  `stat`-based length scheme is dead on arrival. The count has to come from
  content that has already been read.
* **Count the variable, not the file.** `wc -c` applied to the file would read
  it a second time, sampling a different instant for anything volatile.
* **The frame does not preserve trailing whitespace.** `$(...)` strips trailing
  newlines, so a framed file is one byte shorter than a direct read. Harmless
  because `proc::read` already trims and the trimmed content is byte-identical
  (verified on non-volatile files), but a consumer needing exact trailing bytes
  would need a different frame.
* **Root is not required.** All seven files read cleanly as uid 65534, which
  keeps decision 8's least-privilege posture available.

**Rejected: `tar`.** Zero bytes, per the `/proc` size-0 limit above.

**Rejected: bare `cat a b c`.** Concatenation with no way to know where one
file ends.

### 21. Frames are self-labeling; retrieval order is not load-bearing — accepted

Every frame carries its own path, and `read_all` returns a map keyed by path
rather than a positional `Vec`. The caller asks for `/proc/meminfo` by name and
never does index arithmetic.

Retrieval order *is* in fact deterministic — a shell `for` loop returned an
identical sequence across five runs. This decision is not that order is
unreliable; it is that depending on it buys nothing and costs the failure modes
below.

* **Local and remote must not diverge in their contract.** This is the real
  argument. `futures::join_all` preserves input order in its output regardless
  of completion order, so a positional design works perfectly well locally,
  forever, and breaks the first time a remote host prints a banner. That is the
  worst available failure shape: an asymmetry local testing can never surface.
  Keying both implementations by path is decision 11 applied one level below
  the response — normalize the *retrieval* contract, not only the output.
* **`ssh host cmd` stdout is only as clean as the remote account's rc files.**
  It was verified clean on the test host, where `echo MARKER` returned exactly
  six bytes with no motd or banner, but that is a property of that host's
  configuration rather than of SSH. A path-keyed reader skips lines it does not
  recognize; a positional reader silently misattributes every file after the
  noise. Verified: with junk injected before and after the payload, all files
  were still recovered and correctly attributed.
* **It degrades gracefully.** A truncated response loses only the frames it
  lost, and every recovered file is still correct. A duplicate path collapses
  harmlessly instead of shifting everything after it.

What input order *does* control is the order files are read, and therefore
sampling adjacency: `/proc/stat` and `/proc/uptime` belong next to each other
because `collected_at` is `btime` plus uptime. Within one batch all seven land
inside a single remote loop of about a millisecond, against roughly four
seconds spread across seven round trips — so batching narrows skew as well as
latency.

### 22. Container tools read cgroups; runtime knowledge is confined to enumeration — accepted

`container_health` and `container_list` read cgroup v2 files. Health, limits
and pressure are universal across Docker, podman and Kubernetes.

Three reasons, in order of weight:

* **cgroups are the enforcement layer, not a reporting layer.** `docker run
  -m 512m --cpus=2`, a Kubernetes `resources.limits` block and `podman
  --memory` all do the same thing: write cgroup files. Reading them back
  therefore needs no runtime knowledge at all. Verified against a live
  container — `cpu.max`, `cpu.weight`, `memory.max`, `memory.high`,
  `memory.low`, `memory.min`, `memory.swap.max`, `pids.max` and `io.max` are
  all present and readable.
* **The health files are not container concepts.** Verified identical on
  cgroups that have nothing to do with containers — an ordinary systemd
  service, a user slice, `init.scope`. The runtime determines the *path* and
  nothing else.
* **PSI in cgroups is byte-identical to `/proc/pressure/*`.** `psi_avg` and
  `psi_total` parse it unmodified, which is what `proc.rs`'s module doc
  predicted before the use case existed.

**What is not universal: enumeration.** Path layout differs by runtime *and* by
cgroup driver.

| runtime | cgroup path | status |
|---|---|---|
| Docker, systemd driver | `system.slice/docker-<id>.scope` | tested |
| podman rootless | `user.slice/user-<uid>.slice/user@<uid>.service/user.slice/libpod-<id>.scope` | tested |
| podman rootful | `machine.slice/libpod-<id>.scope` | untested |
| Docker, cgroupfs driver | `/sys/fs/cgroup/docker/<id>` | untested |
| Kubernetes, systemd driver | `kubepods.slice/kubepods-<qos>.slice/kubepods-<qos>-pod<uid>.slice/cri-containerd-<id>.scope` | untested |

**Kubernetes will not be verified here, and that is a deliberate choice.**
Standing up a cluster purely to claim compatibility is more work than the claim
is worth, and an unverified claim is worse than an honest gap. The layout above
is a good-faith reading of the documented convention; it stays marked untested
until somebody runs it against a real cluster. The README invites exactly that.
Kubernetes also adds a structural level the others lack — a pod slice containing
several container scopes — so whether the tools report pods, containers or both
is **open** and should be settled by someone holding a real cluster rather than
guessed at here.

**What the podman test established**, against a rootless container on a host
already running Docker:

* The `libpod-` naming and the deep `user.slice` nesting are both real. The
  container sat five levels below `/sys/fs/cgroup`, which is why the discovery
  walk bounds at six rather than the four Docker alone would suggest.
* The io-delegation caveat below is measured, not inferred: `io.stat` and
  `io.max` were absent while `io.pressure` was present.
* **A prefix match alone is not enough.** podman gives each container a sibling
  `libpod-conmon-<id>.scope` for its monitor process, which strips to
  `conmon-<id>` and was reported as a second, non-existent container — a
  phantom in every podman listing, and invisible to any amount of Docker
  testing. `classify` now requires a hex ID after the prefix, which rejects that
  scope structurally rather than by special-casing its name, and so should
  reject comparable infrastructure cgroups from runtimes not yet seen.

Recorded limits, so a future session does not rediscover them:

* **File availability follows controller delegation, not runtime.** Under
  `user.slice`, where rootless podman lives, `io.stat` and `io.max` are absent
  because the `io` controller is not delegated — while `io.pressure` is present,
  because PSI is not controller-gated. Decision 18's rule 7 already covers this:
  the field is simply absent, which is an answer rather than a failure.
* **Container CPU pressure has a meaningful `full` line** where the host's is
  permanently zero. Verified on both. `container_health` therefore reuses
  `Pressure` for CPU where `system_health` uses `CpuPressure` — the type split
  made in decision 18 turns out to be exactly the one the container case needs.
* **`cpu.stat` is cumulative `usage_usec`, not jiffies**, so
  `busy_fraction_since_boot` has no direct analogue.
* **There is no per-cgroup load average.** `/proc/loadavg` is host-wide; that
  field has no container version and should not be invented.
* **Limits are string sentinels.** `memory.max` reads `max` and `cpu.max` reads
  `max 100000` when unset. Normalizing that is decision 18's first rule applied
  again — and unset is the common case, not the edge case.
* **Throttling and OOM counters are the highest-value fields and are inert
  without limits.** `nr_throttled` requires a CPU quota to move; `memory.events`
  requires a memory limit. On a host that sets no limits the tool degrades to
  memory consumption and pressure, which is still useful but not diagnostic.

### 23. Identity is `comm`; process arguments and environment are never read — accepted

A container's identity is the set of distinct `/proc/<pid>/comm` values in its
cgroup. `stethoscope-mcp` does not read `/proc/<pid>/cmdline`, and does not read
`/proc/<pid>/environ`.

**The prohibition is server-wide, not container-scoped.** It is recorded here
because containers are where the question first arose, but `/proc/<pid>` is the
same interface for every process on the machine — a container process is a host
process in a namespace, and nothing about the namespace restricts what a reader
on the host can see. No current tool reads per-process data at all, so the whole
value of this decision is the constraint it places on future ones: a
`process_list`-shaped tool answering "what is using the CPU" is exactly where a
later session would reach for `cmdline` or `environ` as convenient identity.
Measured on a real host, credential-shaped environment variables appeared in
both container and non-container processes; the container ones held actual
database passwords and API keys while the host ones happened to hold only
socket and directory paths, but that is a fact about how that machine was
configured and not a property to rely on. A systemd unit with an
`Environment=` line puts a live secret in a host process immediately.

* **Command lines carry credentials in practice, not in theory.** A scan of a
  running container host found credential-shaped arguments on nine of its 156
  processes, including a literal `--password` flag on a process inside a
  container. Returning command lines would have moved a live credential into
  model context on the first call.
* **`argv[0]` is not a safe subset.** Processes rewrite it: Redis publishes
  `redis-server *:6379` and PostgreSQL `postgres: io worker 0`. Anything a
  process can write there, it can write a secret into.
* **`comm` is structurally incapable of carrying a command line** — fifteen
  characters, kernel-managed. It also proved *better* identity in practice,
  reporting `uvicorn` where `argv[0]` reported `python3.12`.
* **`environ` is prohibited outright, not merely unused.** It is strictly worse
  than a command line, being where database passwords and API keys actually
  live. Writing it down as a prohibition stops a future tool reaching for it as
  a convenient identity source.

**This generalizes decision 9.** That decision refuses SSH usernames, addresses
and key paths. The rule underneath it is: `stethoscope-mcp` returns *operational
measurements*, never *process arguments or environment*. Stated that way it
covers the cases nobody has enumerated yet.

**Rejected: redacting known-sensitive flags.** A blocklist has to be complete to
be safe and cannot be. `-p` means password to `mysql` and port to half a dozen
other tools, and every new application invents its own spelling.

### 24. Container name resolution is deferred to a separate tool — accepted

`container_list` and `container_health` identify a container by cgroup path and
`comm` set. Mapping that to a runtime-assigned name and image is a future tool,
called only for containers already identified as worth investigating.

* **It quarantines the only fragile part.** Name resolution is runtime-specific,
  format-unstable and privilege-requiring: Docker keeps it in an undocumented
  `config.v2.json`, podman in a BoltDB, and Kubernetes exposes only a pod UID in
  the cgroup path with the human name behind the kubelet or API server.
  Isolating it means the health tools never break when any of those change.
* **Lazy resolution scales with faults, not inventory.** Resolving every name on
  a large node is work proportional to the node; resolving the ones that need
  attention is work proportional to the problem.
* **Nothing about health needs a name.** Contention, throttling and OOM counts
  are facts about a cgroup, and `comm` already answers "which one is the
  database".

Recorded limits:

* **Container IDs are ephemeral.** An ID is stable while the container lives and
  gone afterwards, so a name resolved in a later call may name something that no
  longer exists — especially under an orchestrator that replaces workloads. The
  schema must say so, the way `collected_at` says not to use it for interval
  arithmetic.
* **No verdict fields.** "Needs attention" is the model's conclusion drawn from
  `throttled_usec`, `oom_kill` and pressure — not a boolean in the payload.
  Decision 18 continues to hold here.

**Rejected: resolving names eagerly in `container_list`.** It couples the
universal tool to the fragile one and pays the cost on every call, for
information that is only wanted about the exceptions.

### 25. The read guard is an allowlist at the single read chokepoint — accepted, **remit clarified by decision 36**

`src/guard.rs` decides which paths this server may open. `proc::read` and
`proc::read_optional` ask it first, and nothing else in the crate opens a file.

> **Remit clarified 2026-09-12 by decision 36.** "Which paths this server may
> open" is more precisely *which paths this server may read the contents of*.
> The distinction did not matter while every capability was a file read; it
> matters as of `storage_health`, which calls `statvfs(2)` on mountpoints and
> deliberately does **not** go through the guard, because it reads no content.
> Decision 36 has the argument, including why the dynamic allowlist decision 30
> proposed is declined. The guard also **moves into the core crate** with
> collection rather than being duplicated — decision 35, which lists what moves
> with it.
Decision 23 said what must never be read; this is the mechanism that makes it so
rather than a promise that it is.

**An allowlist, not a denylist.** A denylist has to enumerate every dangerous
file forever, and it loses outright to renaming: a symlink at `/tmp/x` pointing
at `/proc/1/environ` has the basename `x` and passes any check on basenames. An
allowlist rejects `/tmp/x` because it is not a shape this server reads. The
first draft of this guard demonstrated the failure mode on itself — a basename
denylist containing `io`, to catch `/proc/<pid>/io`, also blocked the legitimate
`/proc/pressure/io`, and its own tests caught it. A narrow `FORBIDDEN` list
remains underneath the allowlist as a backstop against careless widening.

**Lexical normalization, not `canonicalize`.** `std::fs::canonicalize` resolves
symlinks and `..` correctly, but requires the file to exist — verified — which
would turn every legitimately absent optional file into a resolution failure
instead of the "missing" answer decision 18 rule 7 depends on. Rejecting empty,
`.` and `..` components is sufficient, because the server's paths never contain
them, so their appearance is either a bug or an attempt to get around the
function. That leaves symlinks unresolved and does not need to resolve them: a
symlink can only reach a forbidden file by way of a path that is not in the
allowlist.

**Under `/proc/<pid>/`, only `comm`.** `status`, `stat` and `limits` are denied
by omission — harmless, but unused, and the narrower rule is the one worth
keeping.

**Four layers, because a denial is always a programming error.** No path is
model-controlled, so the compile-time and CI layers matter as much as the
runtime check:

| layer | catches |
|---|---|
| runtime allowlist | a read of any non-approved path, including future dynamic cgroup paths |
| backstop denylist | a careless widening of the allowlist |
| guard unit tests | regressions in the guard itself, including traversal and renaming evasions |
| source-scan test | a forbidden path named as a literal in any other module |

**CI flags edits to the guard.** `detect-guard-modifications` raises a PR notice
whenever existing guard code or guard tests are edited or removed, adapted from
a pattern in a sibling project. Pure additions do not trigger it, so adding
tests stays frictionless. Verified against three simulated pull requests: adding
a test posted no notice, deleting a denial assertion was flagged, and widening
the allowlist was flagged.

**Defense in depth, demonstrated rather than claimed.** Adding `/proc/1/environ`
to the allowlist leaves it denied at runtime by the backstop, *and* fails the
guard's own tests because they detect the contradiction, *and* raises the CI
notice on the diff. Three independent layers have to be defeated together.

Recorded limits:

* **This guards programmer error, not an attacker — for now.** Every path in the
  crate is a string literal today. That changes with container enumeration,
  where cgroup directory listings produce the first constructed paths and
  `cgroup.procs` produces the first paths built from file *contents*. The guard
  is deliberately landed before that work rather than after it.
* **Tests are inline `#[cfg(test)]`, not in `tests/`.** The crate is a binary
  with no lib target, so integration tests cannot reach a private function, and
  exposing the guard publicly to enable them would be a worse trade. Verified
  that `#[cfg(test)]` costs nothing at runtime: release binaries built with and
  without a test module are byte-identical.

**Rejected: a `SafePath` newtype with a private constructor.** More machinery
than two chokepoint functions justify. Worth revisiting if the read surface
grows beyond them.

### 26. A container is addressed by ID alongside its target — accepted

`container_list(target)` returns the containers on a machine: an ID, the runtime
that created each, and the process names running inside it.
`container_health(target, container)` returns one container's contention,
configured limits and uptime.

**Identity belongs in the listing, not only in the health tool.** Withholding it
sounds tidier — the listing enumerates, the health tool describes — but it
forces a model wanting one specific container to call `container_health` on
every candidate until it finds the right one. Measured on a host of thirteen
containers, that is thirteen calls returning full metrics against a single
listing of about 1,700 bytes, and thirteen SSH round trips instead of two once
the target is remote. The extra reads cost 54 file reads and a few hundred
microseconds; the names cost roughly 40 bytes per container.

**A second parameter, not a second kind of target.** Decision 7 makes the user's
SSH config the target inventory, and containers are not in it. A container is a
thing *on* a target rather than a target of its own, so `target` keeps meaning
"machine" and remote plus container stays a pair of coordinates instead of a
flattened namespace.

**Limits are reported by presence.** An absent `limit_bytes` or `limit_cpus`
means no limit is configured — the fact worth surfacing, since an unlimited
container can consume the whole machine. A limit that exists but is too low
shows up instead as `nr_throttled` climbing or `oom_kills` non-zero, so both
failure modes are visible without the payload judging either. Verified against a
host whose twelve containers were first entirely unlimited and then limited.

**Enumeration needed a new capability.** Discovery walks the cgroup tree, which
is the first time this server builds a path it did not have as a literal — the
case decision 25 anticipated. Listing is guarded separately from reading and
confined to the cgroup hierarchy, and discovering a directory does not make its
files readable: every file the walk turns up still goes through the read
allowlist.

Recorded limits:

* **`pids.max` is usually a large kernel default rather than a deliberate
  limit**, so a high value there means unset rather than generous.
* **Container CPU `full` pressure is real**, unlike the host's. Measured at 78%
  on a quota-throttled container while the host's own `full` figure stayed at a
  permanent zero — which is why `container_health` reports a full `Pressure` for
  CPU where `system_health` reports only `some`.
* **Identity is the distinct `comm` set** in the cgroup, per decision 23. It
  says what a container is running — `postgres`, `uvicorn`, `redis-server` — not
  which one it is, and repeats across containers by design.

### 27. Container uptime comes from the cgroup directory's timestamp — accepted

`container_health` reports `uptime_seconds`, derived from the modification time
of the container's cgroup directory subtracted from the target's own clock.

**It exists because the cumulative figures were unreadable without it.** Every
`*_total_us` in the payload is monotonic since the cgroup was created, and a
model shown `full_total_us: 709537` cannot tell whether that is a rounding error
over four hours or a crisis over four minutes. The averages decay to zero after
a problem passes, so the totals are the only record that it happened — and a
record without a denominator is not evidence.

**The timestamp means the current run, not the container's age.** Verified on
both Docker and podman: stopping a container *deletes* its cgroup directory, and
starting it again recreates it with the same container ID and a fresh timestamp.
A stopped container therefore has no cgroup at all, which is also why
`container_list` never reports one — correct for a health tool.

**The denominator and the numerators share an origin, which is what makes the
ratio honest.** The PSI totals and `cpu.stat` live inside that same directory
and reset when it is recreated — measured on both runtimes, where a restarted
container came back with totals at zero. So `full_total_us` over
`uptime_seconds` always covers exactly the same span, with no skew to correct
for.

**Derived from the target's clock, not this server's.** `proc::target_now` is
boot time plus uptime, differenced against a file timestamp from the same
machine. Using the local clock would be wrong the moment the reads happen over
SSH, for the reason decision 19 already gives.

**Guarded by `check_dir` rather than a rule of its own.** Reading a directory's
metadata reveals strictly less than enumerating it, and the server may already
enumerate every path this can reach. A third guard rule would have admitted
exactly the same set while implying the two capabilities could diverge.

Verified against fifteen containers across two hosts and both runtimes: on the
Docker host every cgroup timestamp matched the runtime's own `StartedAt` to the
second, and a freshly created rootless podman container reported three seconds.

## Session decisions (2026-09-11)

### 28. Remote collection ships a capability probe, not a shell command — accepted

Remote collection runs a small, statically linked, single-capability binary on
the target, built from the same core crate the server calls in-process for
`local`. One probe per capability: `stethoscope-storage`,
`stethoscope-system-health`, and so on.

**This supersedes decision 20's frame protocol.** That decision's *measurements*
stand — batching beat seven round trips 7.2x, and connection pooling recovers
far less than batching does — but its length-framed plaintext exists to
reassemble files a shell had to stream, and that problem disappears when the
collector is a real program that emits the finished payload. Decision 20's
underlying principle is what carries forward, not its mechanism.

**Decision 11 becomes structural rather than aspirational.** "Local and remote
must produce normalized results" is today an intention maintained by two code
paths agreeing. With one core crate linked by both the server and every probe,
they agree because they are the same code.

**It removes "can this be expressed as a file read?" as a constraint on future
tools.** That constraint was invisible while all four tools happened to be file
reads. `storage_health` is the first that cannot be — free space requires
`statvfs(2)`, which is a syscall, not a file — and it will not be the last:
listening ports, interface addresses and anything from `sysinfo(2)` are the
same shape. The probe settles the category once instead of relitigating it per
tool.

**The audit argument runs the *other* way, and this is the correction that
drove the decision.** Decision 20's stated value is that "the command stays
legible to whoever audits it," and its complaint about base64 is explicitly
about reviewer effort: telling collection from exfiltration "requires reading
the direction of a pipe, and a reviewer should not have to." A named binary
maximizes exactly that. It is self-identifying; it differentiates invocations at
a glance, where the shell approach forces an analyst to diff command text to
tell `system_health` from `container_health`; and it is hashable, so a security
team can pin and allowlist it, which is impossible for "bash running a large
printf loop." This was argued from having done that triage work, and it
outweighs the initial reading that a binary merely "looks worse."

Sizes, all built and run on the development host. These established the
*architecture*; the serialization question below was measured separately and
its absolute numbers are not comparable to these — see the note under that
table before citing either.

| approach | per capability | shared artifact | 10 capabilities | portability |
|---|---|---|---|---|
| static `std` | 291,784 B | — | ~2.9 MB | static musl; good |
| dynamic `std` (`-C prefer-dynamic`) | 5,360 B | 1,214,200 B `libstd.so` | ~1.27 MB | glibc ≥ 2.34 only |
| **`no_std`, raw syscalls** (hand-rolled JSON, first prototype) | **9,464 B** | none | **~95 KB** | any Linux, any libc |

**A `std` probe is 97% runtime floor.** The storage capability itself measured
8,768 bytes of a 300,552-byte binary; the rest is panic machinery, formatting
and allocator. That is what makes per-capability binaries affordable once `std`
is dropped, and it is why "smaller transfers" is the *weakest* argument for
splitting them — ten static `std` probes total 2.9 MB against ~400 KB for one
monolith, so splitting makes transfers larger, not smaller.

**Per-capability rather than monolithic, on security grounds only.** Three
properties, none of which a monolith can offer:

* **Hash stability scopes to the capability.** Any change to a monolith rolls
  the hash and invalidates every allowlist entry on every host, for every
  capability. Fixing storage parsing should invalidate storage's hash and
  nothing else.
* **The hash becomes an enforceable capability claim.** "This host may run
  storage collection and nothing else" becomes expressible by allowlisting one
  hash, enforced by the *target's* EDR with no cooperation from this server.
  The target gets a vote.
* **Decision 25's guard becomes structural.** The allowlist is currently a
  runtime check inside a binary that contains the code to read everything. A
  storage probe has no cgroup-reading code linked into it at all — the
  capability boundary becomes a link-time boundary.

> **A fourth property, unanticipated here and found 2026-09-12.** Splitting by
> capability also makes *privilege* grantable at the right granularity. Decision
> 39 lets an operator attach `cap_dac_read_search` to the storage probe alone;
> against a monolith the same grant would privilege every capability at once.
> Hash-as-capability-claim turns out to be hash-as-privilege-claim, which is a
> better argument for this split than any of the three it was made on.

**Probes are synchronous and single-shot; concurrency is process-level.**
Nothing in a probe has anything to be responsive to. Parallelism comes from the
server's tokio orchestrating N invocations, which is strictly better isolation
than in-process async — and it *fixes* a hazard rather than merely tolerating
one. `statvfs` on a hung NFS mount blocks forever, and in-process that
permanently consumes a tokio blocking-pool thread, which tokio cannot cancel. A
probe is a process: the server sets a deadline and `SIGKILL`s it. Core is
therefore plain synchronous functions, and the server wraps local calls in
`spawn_blocking` — which is what decision 16 already asks for, expressed
directly instead of indirectly through `tokio::fs`.

Recorded limits:

* **Reproducible builds are load-bearing, not a nicety.** Rust embeds absolute
  paths and the hash moves with the toolchain, so "storage's hash is stable
  across releases where storage did not change" requires `--remap-path-prefix`,
  a pinned toolchain, and a CI reproducibility check. Without them the hash
  manifest quietly becomes noise and analysts re-approve every hash every
  release — the exact misery this decision exists to avoid.
* **Hash isolation is not absolute.** A toolchain bump rolls every probe at
  once. Isolation covers capability-local change, which is the common case.
* **Per-arch syscall stubs are hand-written assembly.** x86_64 `syscall` with
  rax/rdi/rsi/rdx; aarch64 `svc #0` with x8/x0/x1/x2. Roughly 30 lines each.
* **Freestanding Rust fails at runtime, not at compile time.** Building the
  prototype hit a missing `memcpy`, a missing `rust_eh_personality`, and a
  `SIGSEGV` from stack misalignment at `_start` — the kernel enters with RSP
  16-byte aligned while the SysV ABI has callees assume RSP%16 == 8 as after a
  `CALL`, so every SSE spill faults. The fix is an entry stub that aligns and
  calls. Expect this class of bug on machines you do not have.
* **Fixed buffers have real limits.** The prototype truncates past 64 KB of
  mountinfo. A host with thousands of mounts needs handling, not a bigger
  constant.

**Rejected: a shared `libstd.so`.** Measured and working — `-C prefer-dynamic`
takes a probe to 5,360 bytes against a 1.2 MB shared library, and at five or
more capabilities it wins on total bytes. It is rejected because dynamic linking
gives up static musl and demands the *target's* glibc: the binary requires
`GLIBC_2.34` (2021), so an older NAS or an Alpine host fails to load it. It
trades the architecture matrix for a libc matrix. Dropping `std` is better than
sharing it — the `no_std` probe is smaller than the shared library alone.

**Rejected: a monolithic probe with subcommands.** It gets capability-level
legibility in the logs, which is most of the benefit, but presents one identity
and one hash for every capability — losing hash-as-capability-claim and
link-time least privilege, which are the whole argument.

**Rejected: fileless execution.** `memfd_create` + `fexecve` avoids writing to
the target entirely and would be smaller and faster. It is indistinguishable
from malware, which destroys the property that motivated the entire design.
Writing a plainly named file and deleting it is the legible choice.

**Settled 2026-09-11: probes use `serde` + `alloc` + `serde_json`.** `schemars`
is load-bearing — decision 18 puts the interpretation in the JSON Schema
descriptions lifted from `///` comments on the response types. If probes
hand-roll their output the way the prototype does, there are *two* definitions
of the wire format free to drift apart silently, which is the same class of bug
the probe architecture exists to prevent, one layer down. `no_std` + `alloc` +
`serde_json` keeps one derived definition and no possible drift. The cost was
estimated at 50–100 KB per probe and **the estimate was wrong by more than an
order of magnitude**:

| variant | bytes | vs hand-rolled |
|---|---|---|
| `no_std`, hand-rolled JSON | 5,800 | — |
| `no_std` + `alloc` + `serde_json` | **8,496** | **+2,696** |
| … + `JsonSchema` also derived | 8,496 | +2,696 |
| static `std` + `serde_json` (calibration) | 303,704 | +297,904 |

Method: one storage probe — read `/proc/self/mountinfo`, `statfs(2)` each mount
with non-zero blocks, emit the filesystem list — built twice from *identical*
collection code, differing only in how the payload is serialized. Both binaries
were run and their output verified to have the same keys, field order and
values, so the two rows serialize the same wire format. `opt-level = "z"`, LTO,
`codegen-units = 1`, `panic = "abort"`, stripped, statically linked, no libc
startup. x86_64, rustc 1.98.1. The delta is `.text` +1,547 B and `.rodata`
+395 B; the rest is alignment. Sensitivity: the delta is +2,696 B at
`opt-level = "z"`, +2,952 B at `"s"`, and +1,496 B at `3` — never above 3 KB.

**Deriving `JsonSchema` on the response types costs the probe nothing.** The
third row is the same binary size to the byte: a probe never calls
`schema_for!`, so LTO and `--gc-sections` remove schemars entirely. The core
crate's types can derive `JsonSchema` unconditionally; no feature gate is
needed to keep it out of probes. This was the one real risk in adopting derived
serialization and it is not a risk.

**Correctness wins, and it wins on size too.** Decision 29's durable cache had
already made size nearly irrelevant — the probe transfers once per target and is
then reused — and at 2,696 bytes the transfer difference is roughly 2 ms, once,
on the link decision 20 measured. Ten capabilities cost ~27 KB more in total.
There is no size argument against the derived definition.

**The figure to quote is 8,496 B per probe, ~85 KB for ten.** That is what will
actually ship: `no_std` + `alloc` + `serde_json`, with `JsonSchema` derived and
stripped. The architecture table above says 9,464 B and ~95 KB, which described
the earlier hand-rolled prototype and is superseded here for any question about
*what a probe costs*. Its rows remain the reference for the questions they were
built to answer — `std` versus `no_std`, and shared versus static linking —
where the relative differences are what carry the argument and are unaffected.

Recorded limits on the measurement:

* **The 9,464 B figure in the table above is not the baseline here.** The
  original prototype binary was not kept and could not be re-measured; the
  5,800 B row is a fresh reimplementation of the same capability. The absolute
  numbers are therefore not comparable to the earlier table row — the *delta*
  between the two rows is, because both were built from the same source with
  the same flags in the same session. The `std` calibration row landing at
  303,704 B against the recorded 291,784 B is the evidence that the
  reimplementation is of comparable complexity rather than a toy.
* **`serde_json` needs a global allocator, which the hand-rolled path does
  not.** The measured variant uses a bump allocator over one 8 MB anonymous
  mapping that never frees — sound for a single-shot process, and its ~40 lines
  are inside the +2,696 B. A probe that outlives one collection would need a
  real allocator and this number would move.

  **This makes "a probe is single-shot" a constraint on future capabilities
  rather than an implementation detail**, and it is worth knowing where it will
  first bite. Any capability that samples over an interval — meaningful CPU
  attribution needs two readings separated by a delay, so a `process_list` is
  the obvious candidate — keeps one process alive across both readings. That is
  still single-shot and almost certainly fine, since a bump arena only has to
  bound *total* allocation rather than support reuse. But it is the case to
  check against this limit deliberately, rather than to discover.
* **The two variants trade fixed BSS for dynamic memory.** Hand-rolled carries
  320 KB of BSS in fixed buffers; the serde variant carries 64 KB plus whatever
  it maps. Neither affects transfer size, and the `mmap` arena is lazily
  faulted, but it is the reason the resident sizes differ.

### 29. Probes are transferred by `sftp` and cached on the target — accepted, **discovery order corrected by decision 38; `/tmp` removed by decision 40**

The probe is transferred with `sftp` and left on the target as a durable,
content-addressed cache. Its filename is its own content hash. It is **not**
deleted after a call, and **not** tied to the MCP session's lifetime.

**`sftp` rather than the binary on stdin.** Piping the probe in on stdin costs
zero extra round trips and is tempting. It is rejected on two grounds: bastion
session recorders (Teleport, CyberArk and similar) capture the channel itself,
so 9 KB of binary lands in a recorded session as an unreadable and alarming
transcript — in exactly the environments that care most about audit; and the
sftp subsystem, when logging is enabled server-side, emits named per-file
records with path, flags, mode and byte count. Against a wall of inline shell in
an `auditd` execve record, that is the difference between recognizing an event
and reading one. Note the stdin bytes would *not* themselves appear in an
`auditd` record — argv is what is captured — so the objection is about session
recording and log quality, not about audit records carrying binary.

The cost is roughly three round trips for open/write/close, ~150 ms on the
47 ms link decision 20 measured, against a ~560 ms warm call. Accepted
deliberately in exchange for the log signature.

**A cache, not a deployment followed by a cleanup.** An earlier draft of this
decision tied the probe's lifetime to the MCP session — pushed on first use,
deleted on session close. That is wrong, and the reason it is wrong is worth
recording because the mistake is natural.

* **Session-scoped cleanup is unimplementable as a guarantee.** For a stdio
  server the session *is* well defined — it is the process, and
  `service.waiting()` returning in `main.rs` is a real graceful hook, with a
  `SIGTERM` handler covering a second case. But `SIGKILL`, a panic, or a closed
  laptop runs nothing. A "we leave no trace" claim that depends on orderly
  shutdown is a promise that will be broken routinely.
* **Shutdown is the worst possible time to do the most expensive thing.**
  Sweeping N targets means N reconnections, with the ControlMasters possibly
  already gone, while the MCP client waits to reap the process and may kill it
  on a timeout. Cleanup would be racing a kill timer.
* **It is a correctness bug, not untidiness.** Two MCP clients are two server
  processes with no coordination — the current-state document already says so.
  Session A finishing and sweeping the probe directory deletes a probe
  Session B is about to execute, and B fails with `ENOENT`. Content-addressed
  naming does not help: it is a shared mutable resource with no owner. **The
  session is a property of the client↔server relationship; the artifact lives
  in the server↔target relationship. They are different lifetimes and the
  target never observes the former.**

**The posture argument inverts on inspection.** Deleting after every call feels
like the careful choice, but a file that appears and disappears around each
execution is *staging behavior*. A stable, hash-named binary that simply sits
there is an installed tool. Leaving it is the more legible option, by the same
reasoning that chose a named binary over a shell command in decision 28.

**Caching collapses the push/pre-install distinction, and that is fine.** Once
the artifact is durable, a cache *is* an installation with extra steps. This is
not an argument against caching — it is why the location question below has the
answer it does.

**The filename is the content hash**, which now carries real weight rather than
being a nicety. `stethoscope-storage-<hash>` makes concurrent collection benign
— two sessions write identical bytes to one path, so the race has no wrong
outcome — makes the "is it already there?" check trivial, and gives garbage
collection an exact rule. It also pays off where it matters most: the filename
*is* the hash an analyst allowlisted, so identity is read off the path rather
than correlated against a manifest.

**Location is decided by discovery order: `/opt/stethoscope` if it exists and
is writable, otherwise `$HOME/.stethoscope`, otherwise `/tmp`.**

> **Corrected 2026-09-12 by decision 38.** "And is writable" is wrong, carries
> no rationale here or anywhere else, and appears never to have been a
> deliberate choice — an operator's `/opt/stethoscope` is root-owned and mode
> 755, as this very decision demands three bullets down when it rejects a
> group-writable `/opt`, so the condition is false exactly when the install has
> been done correctly and the chain would skip the most legible location in the
> design every single time.
>
> Decision 38 splits this into a **read chain** (`/opt`, `$HOME`, `/tmp` — first
> hash-verified, executable payload wins) and a **write chain** (`$HOME`,
> `/tmp`). `/opt` is **only** ever read: the server never creates it, never
> writes to it, and never tests it for writability, because writing there needs
> root and collection must never be root. Using an installed payload requires
> the hash to verify, the directory to be writable by neither group nor world,
> and the file to actually be executable — the last of which this decision never
> checked for anywhere.

> **`/tmp` removed 2026-09-18 by decision 40.** Both chains lose their last
> link: the read chain is `/opt/stethoscope` then `$HOME/.stethoscope`, and the
> write chain is `$HOME/.stethoscope` alone. The `/tmp` reasoning below — its
> complementary `noexec` failure mode, its OS sweep, its lack of quotas — is
> kept as the record of what was given up.

* `/opt` is the FHS-correct home for add-on software and the most legible to an
  analyst, who reads `/opt/stethoscope/stethoscope-storage-<hash>` as installed
  tooling. But creating it needs root, which collection deliberately does not
  have — decision 8 established that collection works as an unprivileged uid,
  and the README promises the server runs with exactly the launching user's
  permissions. So `/opt` is where an *operator* puts it, not where we push it.
* Making `/opt/stethoscope` group-writable so collection could create it is
  rejected outright: a user-writable directory holding executables in a system
  location is a privilege-escalation shape and exactly what a scanner flags. It
  would be a strange thing for a security-observability tool to leave behind.
* `$HOME` and `/tmp` fail in complementary ways, so both remain in the chain.
  `noexec` on `/tmp` is a hardened-host convention; `noexec` on home is an
  NFS-export convention — and NFS-mounted homes are common in enterprise and
  academic fleets, where a shared home also means the binary transits the
  network to a fileserver and one path is live from every host at once.
* Discovery order means zero setup still works, an operator who wants the tidy
  system-wide install creates one directory, and no configuration file is
  needed — decision 10 stays intact. The result must report which location was
  used, so a failure is diagnosable rather than mysterious. **Per decision 38
  that reporting is per payload, not per call**, because a partial pre-install
  mixes locations within one collection.

Decision 38 also supplies the thing this bullet assumes and never provides: how
the operator obtains a probe to install. A hash is not an artifact. Releases
ship the probe binaries pre-named with their hashes, and the server can extract
its own embedded copies.

**Garbage collection is by hash set, on connect.** On connecting to a target,
delete any `stethoscope-*` whose hash is not in the set this server build
carries. That is exact rather than an age heuristic, self-healing after any
crash, and it makes version upgrades collect themselves. Note the distinction
from what an earlier draft implied: connecting does **not** clear the cache, or
the transfer would be paid every session. It removes *stale* entries and keeps
the matching one.

**Deletion, where it happens, is idempotent.** "No such file" is success, as is
true of `rm -f`.

**Rejected: a self-deleting probe.** Unlinking its own path at startup and
running on from the open inode is atomic, needs no sweep, and cannot leak even
if the server dies mid-call. It is also a textbook malware signature, and is
rejected for the same reason as fileless execution in decision 28.

Recorded limits:

* **This is still a write to a machine we have promised only to read from, and
  caching makes it a durable one.** **Resolved by decision 32**, which draws the
  line at additive-versus-mutative and bounds the namespace the server may
  create or delete within. That decision is a precondition for implementing
  this one.
* **Two different stethoscope versions against one target will thrash**, each
  garbage-collecting the other's probes on connect. A grace period — never
  sweep an entry touched recently — covers it.
* **`/tmp` is swept by the OS and `$HOME` is not**, which is a real point in
  `/tmp`'s favour that the discovery order gives up in exchange for
  executability. Acknowledged rather than resolved.
* **`ssh -T` is required** if a stdin variant is ever revisited; a TTY mangles
  binary on stdin.
* **`sftp` logging is a server-side setting** that will not be enabled on every
  target. The log signature is an argument for the mechanism, not a guarantee.
* **Home directories have quotas; `/tmp` usually does not.** 9 KB will not hit
  one, but a full quota must produce a clean error.
* **A cached probe is only as trustworthy as the hash check.** Reusing a file
  found on the target means executing bytes we did not just write, and the hash
  in the filename is a label rather than verification. **Addressed by decision
  32**: contents are verified against the build's own hash set before execution,
  and the directory must be writable only by the collecting user. Without both,
  the cache is a place to plant something.

### 30. `storage_health` reports every filesystem with capacity — accepted, **completed by decisions 36 and 37**

`storage_health(target)` returns one row per filesystem, from
`/proc/self/mountinfo` plus one `statvfs(2)` per mountpoint.

**Named for the family, not for `disk_usage`.** `architecture.md` used
`disk_usage(target = "nas")` as its running example from the start, but that was
illustrative rather than a commitment, and read-only state and inode exhaustion
are not "usage". `storage_health` matches `system_health` and
`container_health`. The examples in `architecture.md` were updated to this name
when this decision was taken, so the two documents agree; the earlier name
survives only in decisions 11 and 17, which are dated records of what was
thought at the time and are left alone.

**Decision 17 called this one in advance.** Its recorded limits say plainly:
"No free-space data anywhere in `/proc`. `disk_usage` needs `statfs(2)`... it is
the one early capability this decision does not serve." That was written on
2026-09-04, before any of the probe work. Storage was therefore always the tool
that would break the `/proc`-as-substrate assumption, and it is why the break
arrived as a known cost rather than a surprise — see decision 28, which is the
architecture that absorbs it.

**`f_blocks > 0` is the filter, which needs no maintained blocklist.** On the
development host it cuts 29 mount entries to 10, and every pure pseudo-
filesystem — `proc`, `sysfs`, `cgroup2`, `bpf`, `debugfs`, `configfs`, `mqueue`,
`pstore`, `tracefs`, `devpts`, `autofs`, `binfmt_misc` — reports exactly zero.
That is a kernel-supplied fact ("this filesystem has capacity to report"), not a
list of names we would own forever; coreutils' `df` uses a hardcoded fstype list
and we do not have to. It keeps the five tmpfs mounts deliberately: a full
`/run` or `/dev/shm` genuinely breaks services, and `fstype` travels in the
payload so the model can weigh RAM against disk. It also keeps `/dev` and
`efivarfs`, which are marginal, and that is the price of a non-judgmental rule.

**Identity is `major:minor`, and the mount source is dropped at the parser.**
mountinfo identifies the root filesystem by its device id, never by the device
mapper path behind it. The device
id answers the one operationally necessary question the source string was needed
for — are two mountpoints the same filesystem, so is this free space
double-counted? — while carrying no device path and, decisively, no NFS or CIFS
address when a remote target arrives. Decision 9 refuses addresses to the model;
this is the same leak arriving through a different door, closed without losing
anything real. Measured on the development host, two tmpfs mounts report equal
totals under different device ids and are genuinely distinct filesystems; a
model without the device id would add them together.

**`f_bavail` and `f_bfree` both ship, because the gap is enormous.** On the
development host's root filesystem the root-reserved pool — free minus
available — is 5.09% of the filesystem, so the free figure overstates available
space by about 13%. A model told only the free figure is wrong by that much
about what an unprivileged service can actually write, and "df
shows free space but writes fail" is exactly the failure this tool exists to
explain. Decision 18's rule that denominators travel with their numerators
covers this directly.

**Inodes are `Option`, absent rather than zero.** vfat has no inode concept and
reports `f_files == 0`; the vfat mounts on the development host therefore omit
the block
entirely, the way a PSI-less kernel omits `pressure`. A filesystem at 1.7%
bytes-used and 100% inodes-used is a failure that is invisible from byte
capacity alone.

**`ST_RDONLY` is reported.** A filesystem remounted read-only after an I/O error
is a first-rank cause of "it worked yesterday," and `/` here mounts with
`errors=remount-ro`.

**Per-mount failure is normal and must not fail the call.** Three mounts refuse
`statvfs` as a non-root user on the development host: the docker overlay and a
netns with `EACCES`, the fuse portal with `EPERM`. Those rows are reported as
unavailable rather than dropped — the same code path is what a **hung NFS mount**
produces, and that is a first-rank cause of "the service froze." Silently
dropping them would hide the single most diagnostic fact the tool could report.
This is the direct parallel to decision 26: discovering a mount does not make it
measurable.

> **Those three mounts are the motivating case for decision 39.** An operator
> who wants them measured can grant the storage probe `cap_dac_read_search` on a
> copy installed in `/opt/stethoscope`. It is opt-in, per capability, per hash,
> and the payload says whether it was in effect — the rows above stay exactly as
> specified when it is not.

**The hang hazard is contained by the process model, not by a timeout.**
`statvfs` on a dead NFS server blocks indefinitely and cannot be cancelled
in-process. Decision 28's probe-as-process makes it killable. Local collection
still blocks a `spawn_blocking` thread unless the server execs the probe locally
too — an option worth considering, probably not worth a process spawn per call
for the common path.

> **Settled 2026-09-12 by decision 37**, and the sentence above turns out to be
> true without qualification. Decision 34 briefly carved out a local case that
> *did* need a timeout — an in-process `spawn_blocking` deadline and a leaked
> thread — and decision 37 reversed it the next day: local collection runs a
> probe like every other target, so a `statvfs` blocked on a dead NFS server is
> killed by deadline everywhere. The timed-out mount is reported **unavailable
> with a reason**, which is the outcome this decision already defines for the
> `EACCES` and `EPERM` mounts below. No timeout machinery is needed inside the
> collector, and none should be built.

**No verdicts, per decision 18.** No "disk full" boolean. `used_percent` is
derived arithmetic shipped alongside the inputs it came from.

Verified: a `no_std` prototype and a `std` prototype produced byte-identical
filesystem lists, and `total_bytes` and `available_bytes` for `/` matched
`df -B1` exactly.

Recorded limits:

* **The read-only path has never been exercised.** No mount on the development
  host is read-only, so `ST_RDONLY` ships untested — the same honest caveat as
  the missing-PSI path in `system_health`.
* **mountinfo is not fixed-column.** The optional-field section is
  variable-length and terminated by ` - `, so `fstype` is not at a fixed index;
  mountpoints are octal-escaped (`\040` for space).
* **A single overlay mount line ran ~1,100 bytes** of containerd snapshot paths.
  Mount options are not reported, and this is why.
* **`statvfs` needs a guard rule of its own.** `guard::check` admits files the
  server opens and `check_dir` admits one tree it enumerates; a mountpoint is
  neither. The allowlist for this capability is *dynamic* — a path qualifies
  only if the kernel just reported it as a mountpoint in the mountinfo read in
  the same call — which makes it the first guard rule whose allowlist is derived
  rather than static.

  **Declined 2026-09-12 by decision 36**, and the limit is worth keeping as
  written because it is what prompted the question. `statvfs` gets no guard rule
  at all: the guard governs paths whose *contents* are read, a derived allowlist
  would be a weaker control than a written one, and the mountinfo read that
  feeds this capability is already a static entry.

### 31. Target state is cached in memory for the life of the process — accepted, **`local` case amended by decision 37 and settled by decision 42**

The server keeps one in-memory entry per target, built by a discovery call and
discarded when the process exits. Nothing is written to disk.

**This reverses a stated property, deliberately.** `src/main.rs` says today that
"the server itself holds no state... there is nothing to construct or carry."
That stops being true. It is the moment decision 11 anticipated — a real remote
implementation is finally pushing on the target seam — and this cache is where
that seam lives.

**Decision 28 does not work without it.** Probes are per-architecture, and
nothing in decision 28 said where the architecture comes from. It can only be
`uname -m` on the target, so a discovery round trip was always mandatory. The
cache is not an optimization layered on top; it is the thing that makes remote
collection possible at all.

**One entry per target, one TTL, one discovery command.** Architecture,
identity, location, payload presence and payload integrity all come back
together, so there is no reason to expire them independently — re-probing the
slow-moving fields costs nothing because they ride along with the volatile one.
Per-field TTLs would be complexity bought with nothing.

```sh
uname -m; uname -n
# which location, and is it safe to use — all three of decision 29's, in order
stat -c '%n %U %a' /opt/stethoscope ~/.stethoscope /tmp/stethoscope 2>/dev/null
# what is present, and is it really ours (decision 32)
sha256sum /opt/stethoscope/stethoscope-* ~/.stethoscope/stethoscope-* \
          /tmp/stethoscope/stethoscope-* 2>/dev/null
```

One round trip, pushing nothing, answering architecture, identity, location,
presence, integrity and directory safety at once.

**Expiry is lazy — checked at use, never swept by a timer.** A reaper that
re-probes on schedule rebuilds the fan-out this decision exists to avoid:
targets nobody asked about get contacted anyway. Check the timestamp when a
target is actually used and re-probe only then. Minutes rather than seconds or
hours is the right order of magnitude, and roughly ten is the starting guess —
**recorded as a guess, to be revisited against real use rather than defended.**

**Discovery must not block the MCP handshake, and must not fan out.** A config
with forty tagged hosts and six down would block a synchronous startup sweep on
TCP timeouts until the client gives up on initialization. Worse, a stdio server
is spawned per client session, so three open sessions means three processes each
contacting every host at once — **a simultaneous connection to every host in a
fleet is the shape of lateral-movement reconnaissance**, and it is the one thing
in this design that would genuinely alarm the analyst decisions 28 and 29 were
written for. Populate lazily, or in the background with bounded concurrency, and
let the handshake complete immediately.

**Reachability is reported as a fact, never by omission.** Hiding a target that
could not be reached is a verdict, and decision 18 forbids those. `reachable:
false` alongside `checked_at` is the fact. Omission also fails badly on
staleness: a NAS down at startup and up five minutes later would stay invisible
for the rest of the session with no way for the model to learn otherwise. The
timestamp is what makes a refresh decision informed rather than superstitious —
decision 18's rule that denominators travel with their numerators, applied to
freshness.

**TTL is hygiene; error-driven invalidation is the safety net.** They cover
different failures and both are needed. A payload that vanished produces a clean
`ENOENT` on exec, which the server can handle by invalidating and re-pushing
without the model ever seeing it. TTL is for the *silent* drift that announces
nothing: a target rebuilt onto a different architecture, or `/opt/stethoscope`
appearing so that the location previously chosen is now the wrong one.

**Ephemeral is more correct, not merely simpler.** The round trip that
persistence would save is exactly the one that tells the truth. A persisted
cache would confidently report a payload present on a host rebuilt overnight, or
an architecture belonging to a machine that has since been replaced.
Rediscovering every run *is* the drift detection. Persistence would buy speed by
trading away accuracy in precisely the cases where being wrong costs most.

Three further consequences of never touching disk:

* **The worst class of cache bug is impossible.** Stale state surviving a
  restart and quietly disagreeing with reality cannot happen; the recovery
  procedure for any cache weirdness is "restart," and it always works.
* **Decision 10 stays intact** — no state file means no location to choose, no
  format to version, no migration, no corruption path.
* **It is a security property.** The cache is a fleet inventory: which hosts are
  reachable, their architectures, what is installed where. Persisting it would
  leave a map of the user's infrastructure on disk for a backup or a sync client
  to carry off. Decision 9 keeps that class of information from the model;
  writing it to disk would give it away by another route.

The shape reads backwards at first and is worth stating plainly: **ephemeral
local state describing durable remote state.** What is on each target persists;
our memory of it deliberately does not.

Recorded limits:

* **`main.rs` gets harder to read, and that matters.** `StethoscopeMcp` gains
  fields and must be constructed, and tool bodies grow a lookup. The virtue that
  one short file enumerates everything this server does is worth protecting:
  keep the cache's type and logic in its own module so bodies still read as
  validate, look up, delegate.
* **`local` short-circuits everything here.** No discovery, no entry, no SSH.
  The cache is remote-only.

  **Amended 2026-09-12 by decision 37.** `local` now runs a probe too, so it
  needs three of the fields this decision caches — location, payload presence
  and payload integrity — answered from the local filesystem. It still needs no
  `uname` (the server knows its own architecture at compile time), no
  reachability check and no SSH, and the checks cost microseconds rather than a
  round trip, so the *reason* for caching largely evaporates even though the
  fields do not. Whether `local` gets a cache entry for uniformity or is
  answered directly on each call is deliberately left open.

  **Settled 2026-09-18 by decision 42:** answered directly on each call, with
  no cache entry. Running the whole chain every call is what decision 37 wants.
* **Two sessions may both push the same payload**, each believing it absent.
  Content-addressing makes that benign — identical bytes, same path.
* **The ten-minute TTL is unmeasured.** Nothing has been run against a real
  fleet.

### 32. The server adds to a target's namespace; it never modifies the target — accepted, **scope clarified by decision 37; "collectors write nothing" promoted by decision 39; `/tmp` fallback removed by decision 40**

This is the invariant that replaces "every tool is a pure read" now that
decision 29 writes a collector to remote machines, and it is what resolves
whether decisions 3 and 12 permit that.

> **No tool modifies a target system.** Nothing that existed before this server
> connected is ever changed, moved, or removed — no configuration, service,
> package, permission, or data. The server creates and deletes only within a
> namespace it owns: a directory it created, holding files it named. Deletion is
> permission-bounded and best-effort — payloads it cannot remove, such as a
> collector an operator installed into a root-owned `/opt`, are left alone, and
> that is not an error. The collectors themselves perform no writes at all,
> their sole output being stdout, which for a static probe is verifiable by
> disassembly rather than asserted. Locally nothing is written at all, because
> collection runs in-process.
>
> **Generalized 2026-09-12 by decision 38.** The directory rule below says
> "owned by the collecting user and writable by no one else," which was written
> for a directory the server creates. The property it protects is *nobody less
> privileged than us can put a file here*, and that admits root: a directory is
> safe to **execute** from when it is owned by root or by the collecting user
> and writable by neither group nor world, and safe to **write** to when it is
> owned by the collecting user under the same condition. An operator's
> root-owned `/opt/stethoscope` satisfies the first and not the second, which is
> correct. A hostile root is already outside this threat model.
>
> **A payload found on a target is never trusted by its name.** Filenames are
> content-addressed and therefore predictable from any public release, so anyone
> able to write to the probe directory could pre-place a file under a name the
> server expects. The server verifies a found payload's actual hash against the
> set its own build carries and executes nothing that does not match. The probe
> directory must additionally be owned by the collecting user and writable by no
> one else, or the server declines that location and falls through decision 29's
> discovery order.

> **Scope clarified 2026-09-12 by decision 37.** The clause "locally nothing is
> written at all, because collection runs in-process" is **no longer true and
> should be read as struck**. Local collection runs a probe like every other
> target: the server creates `~/.stethoscope` (or uses an operator's
> `/opt/stethoscope`), writes a hash-named collector into it, verifies and
> executes it. Every other sentence of the invariant is unchanged and now
> applies to the local machine as well — the namespace rule, the
> directory-ownership precondition, the hash check, the refusal to delete a
> suspicious payload, and the collectors writing nothing but stdout.
>
> **The threat model below reads the same locally, which is the argument rather
> than a caveat.** "Another unprivileged user on a shared host plants a file at
> the name we are about to execute" describes a shared workstation as readily as
> a shared server. Applying the rules to the local machine means they are
> exercised by ordinary daily use instead of on whatever schedule remote targets
> happen to be inspected.

**The distinction is additive versus mutative.** Placing a collector in a
directory we created is reversible by deleting one directory and changes nothing
anything else depends on. Editing a config, restarting a service or installing a
package is not. **Decision 12's scope is unchanged** — that deferral is about a
target's operational state, and saying so explicitly stops this being read as
the camel's nose.

**Two claims, deliberately separated.** That placing the file is the only write
we perform, and that the thing we placed writes nothing itself. The second is
what makes the first safe, and it is checkable rather than promised: a 9 KB
static `no_std` probe's syscall set can be read off its disassembly, so "writes
nothing but stdout" is verifiable. That is the remote analogue of what
`src/guard.rs` does locally.

> **Promoted 2026-09-12 by decision 39.** "Verifiable by disassembly" was
> recorded here as a good property and left unverified, which is defensible for
> an unprivileged binary. Decision 39 lets an operator grant a probe elevated
> authority, and the same sentence then carries the safety of that grant. It
> becomes a **CI control**: disassemble each release probe and fail the build if
> its syscall set is not a subset of an allowlist. Same claim, much heavier, and
> a precondition for documenting elevation as supported.

**The namespace needs a testable boundary**, or "owned by the server" is a claim
rather than a constraint. Decisions 28 and 29 supply one: a directory the server
created, files named `stethoscope-<capability>-<hash>`, hashes drawn from the
set this build carries.

**Deletion's sharp edge is the inverse of its purpose.** The server must never
remove anything *outside* that namespace; a garbage-collection bug that reached
past it would be the worst failure this system could produce. Removal is
therefore bounded twice over — by directory and by name pattern — and failure to
remove is never an error, because an operator-installed payload in a root-owned
`/opt` is exactly the case we cannot and should not touch.

**The threat model, which explains the scope of the hash check.** If root on the
target is hostile, no client-side verification helps — they control execution
outright. The realistic attack is *another unprivileged user on a shared host*
who plants a file at the name we are about to execute, escalating into the
collecting user's account. Content-addressed naming makes this easier rather
than harder, because the filename derives from a public release and is perfectly
predictable.

**Which is why the directory permission rule is a precondition, not an extra.**
`/tmp`'s sticky bit prevents replacing a file we already own, but nothing
prevents *pre-placing* one before our first write. So the probe directory must
be owned by the collecting user and writable by nobody else. The `/tmp` fallback
must therefore be `/tmp/stethoscope`, a mode-700 directory the server creates,
never files dropped into `/tmp` itself — and this is the same rule that made a
group-writable `/opt` unacceptable in decision 29. If that directory already
exists and is owned by someone else, the location is refused rather than used:
on a shared host it is exactly what an attacker would pre-create.

> **The `/tmp` fallback is gone as of 2026-09-18 (decision 40)**, and with it
> the pre-creation attack this paragraph was written against: nobody but the
> user can create an entry in their own home. The rule — refuse a probe
> directory owned by someone else — still applies to `~/.stethoscope`, and
> decision 42 adds the same check one level up, to the directory that holds it.

**A mismatched payload is not deleted.** Silently removing a planted binary
destroys the only evidence that anyone tried. The split is by recency:

* Pattern match, unknown hash, old mtime — almost certainly a previous version
  of ours. Garbage-collect it.
* Pattern match, unknown hash, **recent** mtime — suspicious. Refuse the
  location, log loudly to the operator per decision 9, leave it in place.

Either way it is never executed, so the execution risk is zero regardless of
what collection does with it.

Recorded limits:

* **Hash-then-execute has a TOCTOU window a client cannot close.** The directory
  permission rule is what actually shuts it, which is why it is a precondition
  rather than a supplement.
* **This states what we promise, not what monitoring will conclude.** A new
  executable appearing on a host is a security event whatever our taxonomy says.
  That is answered by decision 29's legibility work — named file, stable
  location, allowlistable hash — not by this invariant. Separate concerns, both
  required.
* **Verification depends on the target's own `sha256sum`.** An attacker who can
  replace that binary owns the box already, so this is not a gap in the threat
  model above, but it is worth knowing the check is not independent of the
  machine being checked.

---

## Session decisions (2026-09-12)

> These four come out of an end-to-end coherence review of decisions 28–32,
> taken before writing any of the code they describe. Three settle things the
> probe architecture implied but never stated; one resolves a contradiction
> between two decisions taken the same day. The remaining findings from that
> review are recorded as open questions 12–14 in `current-state.md` rather than
> decided here.

### 33. The payload is embedded in the server binary and built by an `xtask` — accepted, **dev-build path amended by decision 37; distribution added by decision 38; runtime SHA-256 settled by decision 41; build paths remapped out of releases 2026-09-18**

The server carries every probe it can push, as `include_bytes!`, one per
capability per architecture. Nothing is located on disk at runtime. Probes are
built by a separate `cargo xtask` step, not by `cargo build`.

**Decisions 28 through 32 imply embedding everywhere and state it nowhere.**
Decision 32 verifies a found payload "against the set its own build carries";
decision 31 `sha256sum`s the target and compares. Neither says where the bytes
come from. This is that decision.

**Embedding rather than shipping a directory of artifacts.** Three reasons, in
order of weight:

* A runtime lookup gives an attacker a swap point *on the local machine* — the
  one place decision 32 promises nothing is written and nothing is executed
  that we did not build. Everything decision 32 does to protect the remote
  probe directory would have to be redone for a local one, to protect a
  lookup that exists only because we chose to have one.
* "The set its own build carries" is literally true only if the build carries
  it. A manifest pointing at sibling files is a second source of truth, and
  decision 10's reasoning about config files applies unchanged.
* It keeps decisions 1 and 2's single-artifact property: an MCP client spawns
  one binary, and there is no installation layout to get wrong.

**Size is not a consideration.** 8,496 B per probe (decision 28) times ten
capabilities times two architectures is ~170 KB on a binary already around
3 MB. The transfer argument that made per-capability probes look expensive
does not apply here — the server binary is fetched once by a human, not pushed
per target.

**Probes are workspace members, excluded from `default-members`.** This
*corrects* an inference the `scripts/probe-size/` README invites. That harness
is deliberately outside the workspace because each variant pins its own link
flags in a nested `.cargo/config.toml`, and cargo reads `.cargo/config.toml`
relative to the **invocation** directory rather than the package directory — so
non-membership is load-bearing *for the harness*. It does not transfer to the
real probes, for which the same constraints have in-workspace answers:

* Link flags come from each probe's own `build.rs` emitting
  `cargo::rustc-link-arg-bins=...`, which applies to that crate alone and needs
  no nested config file.
* `panic = "abort"`, `lto` and `opt-level = "z"` come from a custom
  `[profile.probe]` at the workspace root. Per-package profile overrides cannot
  set `panic` or `lto`, which is why this is a profile rather than an override.
* Sharing the workspace lockfile is a **feature, not a compromise**. One
  `serde` version across the server and every probe is precisely the no-drift
  property decision 28 is buying; two lockfiles would reintroduce it one layer
  down.

The build is therefore
`cargo build --profile probe --target <triple> -p stethoscope-storage`, driven
by an `xtask` that knows the capability × architecture matrix.

**`cargo build` must keep working on a machine with no cross toolchain.** A
fresh clone without `x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl`
installed embeds an **empty payload set**: the server is local-only and any
remote target reports `payload_unavailable`. `cargo xtask release` builds the
matrix and then the server; CI asserts the release binary carries a non-empty
entry for every capability × architecture. Without this split, contributor
build and CI both break the day the first probe lands, and the failure is a
linker error rather than a legible one.

> **Amended 2026-09-12 by decision 37.** "The server is local-only" is no longer
> a meaningful fallback: with no in-process path, a server carrying an empty
> payload set can collect **nothing at all**. The split still stands but the
> ordinary build command changes.
>
> * **`cargo xtask dev` is what a contributor runs**, and it needs no cross
>   toolchain. A freestanding `no_std` probe with raw syscalls links against the
>   *host* triple — `scripts/probe-size/` already builds this way against
>   `x86_64-unknown-linux-gnu` — so the host-architecture probe is always
>   buildable with the toolchain that is already installed. Only the cross-arch
>   release matrix needs `rustup target add`.
> * **`cargo build` remains valid and produces a server that answers no tool.**
>   That must be a legible startup diagnostic on stderr, not a confusing failure
>   at first call.
> * **CI builds probes.** It cannot exercise a single tool otherwise, which also
>   means CI gains real coverage of the payload chain it would never have had
>   under decision 34.
> * **The release matrix now bounds which machines can inspect *themselves*.**
>   See decision 37's recorded limits; this belongs in the README before any
>   binary is published.

**Digests are computed in `build.rs`, over the same bytes it embeds.** Drift
between the payload and the hash set becomes impossible by construction rather
than by discipline. `sha2` enters as a `[build-dependencies]` entry, so the
shipped dependency surface decision 14 constrains is unchanged. The server
needs no runtime SHA-256: the *target* hashes, per decision 31, and the server
compares strings.

> **Unsettled 2026-09-16 by decision 37**, which is the one consequence of that
> reversal nothing here or in decision 14 caught. "The target hashes" was written
> when `local` collected in-process and hashed nothing. Under decision 37 the
> local machine is a target like any other and its payload must be verified
> before execution — so the server either shells out to the local `sha256sum`
> (a subprocess on the common path, a coreutils dependency, and decision 32's
> "verification depends on the target's own `sha256sum`" limit turned
> self-referential on the one machine where we could simply compute it), or
> `sha2` becomes a runtime dependency after all and decision 14's amendment is
> wrong. Neither is chosen here. See open question 15.

> **Settled 2026-09-18 by decision 41:** `sha2` is linked at runtime and the
> local probe is hashed in-process. A remote target still hashes with its own
> `sha256sum`, and the server still only compares strings — decision 42's rules
> take a hex digest and do not know who computed it.

**The manifest must be publishable, and nothing in decisions 28–32 published
it.** Decision 28's strongest argument is that a security team can pin and
allowlist a probe's hash — which requires obtaining the hashes *before* the
binary appears on any host. Today the only way to learn them would be to run
the server against a target, which is exactly backwards. So:
`stethoscope-mcp --payload-manifest` prints capability, architecture, hash and
size, and exits; the same manifest ships as a release artifact. It is ten lines
of code and it is the difference between hash-as-capability-claim being a real
control and being rhetoric.

**The toolchain is pinned in `rust-toolchain.toml`, coupling the server's
toolchain to the probes'.** Decision 28 makes reproducible builds load-bearing:
"storage's hash is stable across releases where storage did not change" is
false without a pinned toolchain, and a hash manifest that rolls every release
is noise that trains analysts to re-approve blindly. Pinning the probe
toolchain alone is not expressible, so the server gets pinned too. Accepted
deliberately — it is one file, and it makes a toolchain bump a visible act
whose consequence (every probe hash rolls at once, per decision 28) is
understood at the moment it is taken.

> **Amended 2026-09-18: a release binary must not contain the paths of the
> machine that built it, and `cargo xtask release` enforces this.** rustc
> records absolute source paths as panic locations. The workspace's own crates
> are recorded relative (`src/main.rs`), but dependencies are compiled from
> `$CARGO_HOME/registry` and generic standard-library code from `$RUSTUP_HOME`
> when `rust-src` is installed, and those paths contain the builder's home
> directory and username. Measured on a release server: 133 such strings. The
> probe had none — `no_std`, `panic = "abort"` and stripped leave no panic
> locations to record — and the `include_bytes!` path is read at compile time
> and never stored.
>
> This is the repository-wide rule in `CLAUDE.local.md` — nothing identifying a
> real machine is published — applied to the one artifact that is published.
> It is also a reproducibility requirement: a binary embedding where it was
> built cannot be rebuilt byte-for-byte anywhere else, which undermines a
> published hash manifest.
>
> **The mechanism is rustc's `--remap-path-prefix`**, set by `cargo xtask
> release` itself for every cargo invocation it makes: `$HOME` → `/home`,
> `$CARGO_HOME` → `/cargo`, `$RUSTUP_HOME` → `/rustup`, the workspace root →
> `/build` (and the target directory, if it lies outside the root). Passed as
> `CARGO_ENCODED_RUSTFLAGS`, so a path containing a space survives, and
> appended to any rustflags the caller set rather than replacing them. Measured
> with these rules: zero home-path strings and zero occurrences of the username
> in the release server, and **an unchanged probe hash** — remapping costs
> reproducibility nothing.
>
> **It belongs in the `xtask`, not in a workflow file or `.cargo/config.toml`.**
> A workflow would protect only builds that run through it; a release built on a
> laptop would leak again. A config file cannot expand `$HOME`, so it would have
> to hardcode a path — publishing the very thing it exists to remove.
>
> **The remap is the mechanism; a scan is the control.** After building,
> `cargo xtask release` searches the server binary and every staged probe for
> each remapped-away prefix and fails if one is present. A dependency that
> embeds a path by some route the remap does not cover then fails the release
> rather than shipping. The scan's logic is unit-tested with a planted path.
>
> Limits: `cargo xtask release` builds the host architecture only, and is
> otherwise `cargo xtask dev --release --locked`. The matrix, the manifest,
> `--extract-payload` and the archives are still unbuilt. The scan looks for the
> remapped prefixes, not for every string that could identify a machine — a
> hostname, say, embedded by a build script, would not be caught. Dev builds are
> deliberately not remapped: debuggers and editors resolve sources through those
> paths.

**Rejected: `build.rs` invoking `cargo` to build the probes.** It would make
`cargo build` produce a complete artifact, which is genuinely attractive.
Rejected because recursive cargo invocations contend on the lockfile and the
target directory, break under `sccache` and most CI caching, and — decisively —
would make a plain `cargo build` *fail* on any machine lacking the cross
targets. The `xtask` split makes the expensive path explicit and the cheap path
always work.

Recorded limits:

* **The architecture matrix is a policy choice.** x86_64 and aarch64 to start.
  Anything else — `armv7l`, `riscv64`, a BSD — is a clean
  `unsupported_architecture` outcome, never a fallback to something that might
  run. See open question 13 on the failure vocabulary.
* **The server binary's own hash rolls whenever any probe changes.** The hash
  isolation decision 28 promises is for the artifacts on targets; an analyst
  allowlisting the *server* gets none of it. That is the right trade — the
  server is the thing a human installs deliberately — but it should not be
  claimed otherwise.
* **One probe per tool, not per domain.** `container_list` and
  `container_health` each link the cgroup substrate, duplicating perhaps 2 KB.
  Per-domain would be smaller and would forfeit hash-as-capability-claim for
  both tools at once, which is the whole argument of decision 28.
* **`--payload-manifest` is the first CLI flag**, and decision 10 should not be
  read as forbidding it. That decision is about a *configuration file* and a
  format to version; a flag that prints and exits configures nothing.
* **Nothing here is measured.** The 170 KB figure is 8,496 B (decision 28)
  multiplied out, not a built artifact. Re-derive it when the first two probes
  exist rather than quoting it.

### 34. Local collection runs in-process, permanently — ~~accepted~~ **reversed by decision 37**

> **Reversed 2026-09-12, one day after it was taken**, by decision 37. It is
> kept in full because this log does not silently drop decisions and because
> the arguments below sounded reasonable when they were made — which is the
> useful part of the record. Decision 37 answers each of them. In short: the
> spawn cost was never real, "no gain" was backwards (the gain is that ordinary
> local use continuously exercises the payload chain), the `smoke.sh`-only
> verification path it proposed was production-unreachable code existing to be
> tested, and its own recorded limits conceded that local got a worse answer to
> the `statvfs` hang than remote did.

`target = "local"` calls the core crate directly. The server never drops or
executes a payload in order to collect from the machine it is running on.

**This settles a contradiction between two decisions taken on 2026-09-11.**
Decision 32 states flatly that "locally nothing is written at all, because
collection runs in-process." Decision 30 says the `statvfs` hang "is contained
by the process model, not by a timeout" and floats exec'ing the probe locally
as "an option worth considering, probably not worth a process spawn per call."
`storage_health` is the pilot and it is the exact capability with the hang
hazard, so the question cannot stay open.

**The decisive argument is `noexec`, not cost.** Decision 29 already documents
that `noexec` on `/tmp` is a hardened-host convention and `noexec` on home is
an NFS-export convention. A local machine with no writable, executable
directory would lose collection **entirely** — a regression from four tools
that work today, on precisely the hardened hosts this project is meant to serve.
The local machine is the one target where running in-process is guaranteed to
work. Trading that for uniformity buys tidiness and pays with the ability to
run at all.

**The secondary arguments agree.** A process spawn per call on the common path
is real cost for no gain, and exec'ing locally would drag decision 29's
location discovery, decision 32's directory-ownership rule and the garbage
collector onto the local machine — a large amount of security-critical
machinery, introduced to solve a problem the local case does not have.

**The hang hazard gets the answer decision 30 already had machinery for.** Each
mount's `statvfs` runs in its own `spawn_blocking` task, raced against a
deadline. A mount that exceeds it is reported as **unavailable with a reason**,
which is the outcome decision 30 already defines for the `EACCES` and `EPERM`
mounts on the development host. A hung NFS mount therefore produces the single
most diagnostic row the tool can emit, rather than a tool call that never
returns. The abandoned thread is not cancellable and leaks.

**Uniformity is preserved where it actually matters.** Decision 28's real claim
is that local and remote agree *because they are the same code*, and that is
untouched: both paths call the same core functions. Only the process boundary
differs, and decision 11 was always explicit that collection method is an
implementation detail.

Recorded limits:

* **Leaked blocking threads are unbounded in principle.** Tokio's blocking pool
  defaults to 512 threads; a host with a permanently dead NFS mount leaks one
  per `storage_health` call until the pool is exhausted, at which point the
  server stops doing file I/O at all. Restarting clears it. This is slow-burn
  rather than fatal, and it is a limit **the remote path genuinely does not
  have** — decision 28's `SIGKILL` is a real fix, not a mitigation. State the
  asymmetry rather than hiding it.
* **The deadline value is unmeasured.** Single-digit seconds is the right order
  of magnitude and is a guess, to be revisited against a real hung mount.
* **This does not forbid dropping and exec'ing a payload locally for
  verification.** Exercising decisions 29 and 32 against the local filesystem
  is how that machinery gets tested before it ever meets a remote host — see
  the amendment to decision 32 and the working agreements in
  `current-state.md`. That path is operator-driven and is not how any tool
  collects.

### 35. The core crate depends on neither `rmcp` nor `chrono`, and the read guard moves into it — accepted, **amended by decision 37**

`stethoscope-core` is `no_std` + `alloc`: collection, the response types, their
`Serialize`/`JsonSchema` derives, and `src/guard.rs`. It links into the server
(a `std` binary) without ceremony and into each probe unchanged.

> **Amended 2026-09-12 by decision 37.** Three changes, all consequences of the
> server no longer collecting anything itself.
>
> * **The server links core only during the transition.** Once every tool is a
>   probe, `stethoscope-core` is a dependency of the probes alone, and the server
>   depends on nothing of it but the *response types* — which must therefore be
>   separable, or the server re-acquires the collection code by linking the
>   crate that holds it. Whether that is a feature flag or a second crate is not
>   decided here; that it must be possible, is.
> * **The read guard ends up only in the probes**, so the "belt and link-time
>   braces" framing below is wrong in its final state: the runtime allowlist
>   stops being redundant with the link-time boundary and becomes the only
>   path-level control in the process that does the reading. That is a stronger
>   reason to keep it, not a weaker one. Decision 25's four layers all survive
>   the move.
> * **The error type still has to cross a process boundary**, not just a crate
>   boundary. Core's error must serialize into the probe's output and map to
>   `ErrorData` in the server, which makes the mapping decision 9 already wanted
>   into a wire-format question rather than a type-conversion one. Open question
>   13 gets to settle the vocabulary.

**Decision 28's "same core crate" has a dependency consequence nothing
recorded.** Every function in `src/proc.rs` returns `rmcp::ErrorData`, and
`collected_at` is formatted with `chrono`. A `no_std` crate can carry neither.
This is the first thing the pilot hits and it is not incidental work.

* **Core gets its own error type**, and the server maps it to `ErrorData` at
  the tool boundary. That is where decision 9's normalization already belongs —
  the boundary between what the process knows and what the model is told — so
  the mapping has a job beyond appeasing the type system.
* **RFC 3339 formatting moves into core** as a civil-from-days conversion,
  roughly forty lines. It cannot be done server-side: decision 19 requires the
  *target's* clock, so the probe has to format the timestamp it computed.
  `chrono` then leaves the server's dependency list entirely once
  `system_health` is ported, which is a small improvement to decision 14.

**The guard moves rather than being duplicated.** Two copies of the allowlist —
one in the server for `local`, one in core for the probes — is exactly the
silent-drift failure decision 28 exists to prevent, one layer down, and in the
one file where drift would be least visible and most consequential. Three
things move with it and must not be forgotten:

* CI's `detect-guard-modifications` job matches a path that changes.
* The source-scan test currently scans "any other module" of one crate; it must
  scan the workspace.
* The guard's unit tests move with the guard. They are part of the control
  (decision 25), not a satellite of it.

**A probe keeps the runtime guard even though its allowlist is mostly dead
code.** Decision 28 is right that a storage probe has no cgroup-reading code
linked into it, and that the capability boundary becomes a link-time boundary.
The runtime check is retained anyway: it costs bytes in the hundreds, and a
control present in one build and absent in another is the kind of asymmetry a
later session reasons about incorrectly. Belt and link-time braces.

Recorded limits:

* **This is the bulk of the pilot's work and should not be estimated as
  incidental.** It is mechanical, and it touches every collector, the guard,
  CI, and the dependency list.
* **`no_std` + `alloc` in core forces an allocator question on the probes and
  none on the server.** Decision 28 measured a bump arena over one 8 MB mapping
  and recorded that it makes "a probe is single-shot" a constraint on future
  capabilities. Core itself must therefore not assume it can free and reuse.
* **`schemars` derives stay unconditional**, per decision 28's measurement that
  they cost a probe zero after LTO. No feature gate.

### 36. `statvfs` is outside the read guard's remit — accepted

The guard is not extended to cover mountpoints. `guard::check` continues to
govern only files whose **contents** the server reads.

**This declines decision 30's own recorded limit**, which said `statvfs` "needs
a guard rule of its own" and that the rule's allowlist would be *dynamic* — "a
path qualifies only if the kernel just reported it as a mountpoint in the
mountinfo read in the same call." That framing is what is being rejected, and
the reason is worth stating plainly: **an allowlist that is computed is a
weaker control than one that is written down**, and adopting one here would set
the precedent that the allowlist can be derived from data. Decision 25's whole
argument is that the set of things this server may open is enumerable by
reading a file.

**The correct boundary is content, not paths.** Decisions 23 and 25 exist
because file *contents* enter model context — `cmdline`, `environ`, anything
under `/proc/<pid>`. `statvfs` returns fixed-shape capacity integers about a
path and reads no content whatsoever. What genuinely needs guarding in
`storage_health` is the `/proc/self/mountinfo` read, which is one static
allowlist entry.

**The claim in the guard's own documentation has to be restated.** "The guard
decides which paths this server may open" becomes "which paths this server may
read the contents of." `guard.rs`'s module documentation gets that sentence
when `storage_health` lands; without it, the first reader to notice a `statvfs`
call with no guard check will reasonably conclude the control has a hole.

Recorded limits:

* **A mountpoint path is itself information** — a bind-mount path can carry a
  project or customer name. That is not a new leak: decision 30 already ships
  every mountpoint in the payload, so `statvfs` reveals nothing the response
  does not already contain. If that changes, this decision changes with it.
* **This covers `statvfs` and nothing else.** A future capability whose syscall
  returns *content* rather than fixed-shape metadata is not covered by the
  reasoning here and does not get to cite it.

### 37. Local collection runs a probe; there is no in-process path — accepted, **reverses decision 34**

`target = "local"` places a probe in a directory the server owns, verifies its
hash, executes it, and reads one JSON document from its stdout — the same
sequence as a remote target, minus SSH. The server links no collection code at
all.

**Decision 34 was decided one day earlier and was wrong.** It is kept above,
per this log's rule about not silently dropping decisions, and the reasoning
that overturned it is worth recording precisely because decision 34's arguments
sounded reasonable at the time.

* **The process-spawn cost was never an argument.** One to two milliseconds to
  fork an 8,496-byte static binary, inside a tool call that has already crossed
  JSON-RPC and a model round trip.
* **"It drags the payload machinery onto local for no gain" was backwards.** The
  gain is that every local call exercises location discovery, the
  directory-ownership check, hash verification, exec, output parsing and garbage
  collection. That is continuous verification of the most security-critical code
  in the design, obtained from ordinary use rather than from tests someone has
  to remember to write. Nothing else available buys that.
* **Decision 34's proposed compromise — exercising the chain from
  `scripts/smoke.sh` on a path no tool could reach — is a smell.** Code that
  exists in production only to be tested is the code that rots first and is
  trusted anyway. If a path is good enough to exercise locally it is good enough
  to *use* locally, and this decision deletes the construct.
* **Decision 34 argued against itself in its own recorded limits.** It conceded
  that remote gets a real fix for the `statvfs` hang while local gets a leaked
  thread, and recorded the asymmetry as a known defect. An asymmetry a decision
  has to apologize for is usually the decision being wrong.
* **Two mechanisms is a permanent documentation and maintenance tax.**
  "Collection runs a probe; local is the same minus SSH" is one sentence.
  "Local collects in-process, remote by probe, and here is the list of ways they
  differ" is a paragraph per capability, forever, in a project whose
  documentation is its principal artifact. Decision 28 claims local and remote
  agree because they are the same code; with two invocation paths that was true
  of the parsing and false of everything around it — error handling, timeouts,
  cancellation, output handling — which is precisely where plumbing drift lives.

**Decision 30's original claim becomes true without exception.** "The hang
hazard is contained by the process model, not by a timeout" was written before
decision 34 carved out a local case that needed a timeout after all. There is
now no such case: a `statvfs` blocked on a dead NFS server is killed by deadline
on every target, including this one, and the `spawn_blocking` deadline dance
decision 34 introduced is deleted rather than implemented.

**The payoff decision 34 foreclosed: the server links no collection code.** No
`/proc` parsing, no cgroup walking, no `statvfs`, and no read guard. Decision
28's argument that a probe's capability boundary becomes a *link-time* boundary
now applies to the binary a human actually installs. The server is reduced to
the MCP surface, target validation, the payload store, location discovery, hash
verification, process execution, output parsing, the target cache and SSH. What
it can read is bounded by which probes exist, not by a runtime check inside a
binary that contains the code to read everything.

**What this gives up, stated plainly rather than minimized:**

* **A host with no writable, executable location cannot be inspected at all** —
  including the local one. Decision 29 records that `noexec` on `/tmp` is a
  hardened-host convention and `noexec` on home an NFS-export convention, so the
  failing case is a machine with both and no operator-created
  `/opt/stethoscope`. That is real in enterprise and academic fleets, and it is
  accepted on two grounds.

  The narrow one: the same condition already made a *remote* target
  uncollectable, and a design where the local machine is quietly the exception
  is precisely what this decision removes. The operator's fix is one directory.

  **The broader one is about who this tool is for, and it is the load-bearing
  argument.** This exists to surface problems that are silently burning on
  machines an administrator is responsible for. An environment locked down hard
  enough that no location on the box is both writable and executable has a risk
  appetite that almost certainly does not extend to letting an AI assistant
  inspect it at all — the `noexec` policy is a symptom of the posture, not an
  obstacle within it. Contorting the architecture to serve a population that
  would decline the tool on other grounds buys nothing and costs the single
  mechanism that makes the rest of this design verifiable. Better to fail
  cleanly and legibly on those hosts than to carry a second collection path
  forever for them.
* **`cargo build` alone produces a server that can collect nothing.** See the
  amendment to decision 33: `cargo xtask dev` becomes the ordinary build
  command, and it needs no cross toolchain because the host-architecture probe
  builds against the host triple.
* **A released binary can only inspect a machine whose architecture is in the
  release matrix.** Previously the local machine always worked, whatever it was.
  Now a user on an architecture we do not ship must build from source — where
  the `xtask` always produces a host probe. This is a new failure mode with no
  precedent in v0.1 and it should be in the README before any binary is
  published.
* **The local machine now carries a `~/.stethoscope` directory** with a binary
  in it. Decision 32's invariant governs it exactly as it governs a remote one,
  and decision 29 already argues a stable, hash-named file is *more* legible
  than one that appears and vanishes around each call. It still needs saying in
  user-facing documentation rather than discovered.

**Decision 32's threat model applies locally unchanged, and that is the point.**
"Another unprivileged user on a shared host plants a file at the name we are
about to execute" describes a shared workstation as readily as a shared server.
The directory-ownership precondition, the hash check, and the refusal to delete
a suspicious payload are the same rules on the same code, exercised every day
rather than on whatever schedule remote targets happen to get used.

**The transition is staged, and the end state is not instant.** The four
existing tools collect in-process today. They keep doing so until each is ported
to a probe; the server links the core crate until the last one moves, and only
then does the "no collection code in the server" property arrive. This
*replaces* the working agreement that deferred porting until the probe chain had
run against a real remote host — under this decision the chain runs locally, so
the precondition is satisfiable on the development machine, and porting stops
being a payoff that waits on open question 1.

Recorded limits:

* **The pilot's floor is now much higher.** `storage_health` cannot ship as
  "local, in-process, plus a probe artifact built on the side." It ships when
  the payload chain works, or it does not ship. That is a real cost of this
  decision and it should be planned for rather than discovered in the middle.
* **Two MCP sessions on one workstation are common** — an editor and a terminal
  — where two concurrent sessions against one *remote* target are not.
  Concurrent placement is benign by content-addressing (decision 29), but
  garbage collection is not, which raises the priority of open question 14 from
  a fleet concern to a local one.
* **`local` stops being a total short-circuit of decision 31.** It still needs
  no `uname`, no reachability check and no SSH, but it does need a location, a
  payload presence check and a hash verification — the same fields, answered
  from the local filesystem in microseconds. Whether it gets a cache entry for
  uniformity or is answered directly each call is not decided here.
* **Nothing here has been run.** The claim that a host-triple freestanding probe
  needs no `rustup target add` comes from `scripts/probe-size/`'s README, which
  builds against `x86_64-unknown-linux-gnu`. It is strong evidence and it is not
  the same thing as having built the real probe that way.

### 38. Probes are distributed, not only embedded — and an installed directory is read-only in the chain — accepted, **extended by decision 39; `/tmp` removed from both chains by decision 40; directory rule extended by decision 42**

Releases ship the probe binaries themselves, pre-named exactly as the server
looks for them, and the server can extract its own embedded copies. Decision
29's discovery order is split into a **read chain** and a **write chain**,
because an operator's install is correctly *not* writable by the collecting
user.

**The gap.** Decision 29 says `/opt` "is where an *operator* puts it, not where
we push it," and decision 33 publishes a manifest so an analyst can allowlist
hashes. Neither says how the operator obtains the *bytes*. A hash is not an
artifact, and "build it yourself and hope the hash matches" is not an install
procedure — reproducibility is load-bearing precisely so that it does not have
to be.

**Chasing that exposed a bug in decision 29's discovery order, which matters
more than the missing artifact.** The chain takes `/opt/stethoscope` "if it
exists and is writable." An operator install is root-owned and mode 755 — that
is the whole point, and decision 29 itself rejects a group-writable `/opt` as a
privilege-escalation shape. So the condition is false exactly when the install
is done correctly, the chain falls through to `$HOME`, and the most legible
location in the design is the one it can never use.

**The condition was never a deliberate choice, and its own surroundings say
so.** Every other clause in decision 29 carries a paragraph of reasoning;
"and is writable" carries none, anywhere. It reads as a symmetry that got
written down because the other two locations needed it — and then was inherited
unexamined, including by the first attempt at this correction, which preserved
it in miniature as "`/opt` is read-only *unless the collecting user happens to
own it*." That clause is the same error at smaller scale and is struck.

**Writability is never a condition on `/opt`, in either direction.** Writing to
`/opt` requires root; collection must never run as root, and no one should be
configuring SSH to a target as root in order to use this tool. A design with a
path that works only when collection is privileged is a design that pressures
users toward privileging it — the same failure decision 6 refuses for
passwordless keys. So:

> **`/opt/stethoscope` is a read-only location, always. The server never
> creates it, never writes into it, and never tests it for writability — on any
> target, local or remote.**

The two chains, with no overlap and no conditional membership:

| chain | locations | question answered |
|---|---|---|
| **read** | `/opt/stethoscope`, `$HOME/.stethoscope`, `/tmp/stethoscope` | is there a payload here I can verify and execute? first hit wins |
| **write** | `$HOME/.stethoscope`, `/tmp/stethoscope` | where may I place one, having found none? |

> **Amended 2026-09-18 by decision 40.** `/tmp/stethoscope` is removed from both
> rows. The chains are now `/opt/stethoscope`, `$HOME/.stethoscope` (read) and
> `$HOME/.stethoscope` (write). The symlink gap recorded below was written about
> `/tmp/stethoscope` and applies equally to `~/.stethoscope`; decision 42 closes
> it for both by refusing any candidate that is not a real directory.

**What using an installed payload actually requires**, since "found it" is not
sufficient and hash verification alone is not either:

* the file exists under a name whose hash is in this build's set;
* its contents hash-verify against that hash (decision 32);
* the directory is **owned by root or by the collecting user** *and* is writable
  by neither group nor world. Both clauses are load-bearing and the first is the
  one easy to drop: a mode-755 directory owned by some *third* unprivileged user
  is writable by neither group nor world and is still entirely theirs to fill.
  That is decision 32's threat model — another unprivileged user pre-placing a
  file at a predictable, content-addressed name — arriving through the read
  chain rather than the write chain;
* **the file is executable by the collecting user** — an install that lost the
  exec bit, or a `/opt` on a `noexec` mount, is found and hash-verified and
  still cannot run.

The mode and ownership facts come free: decision 31's discovery already runs
`stat` and `sha256sum` across the candidate directories in one round trip. The
`noexec` case cannot be predicted cheaply, so **the exec attempt is the test** —
a failure with `EACCES`, `EPERM` or `ENOEXEC` falls through to the next read
location and then to the write chain, rather than failing the call. That makes
`noexec` detection empirical instead of a mount-option guess.

**Decision 32's directory rule generalizes rather than changes.** It required
the probe directory be "owned by the collecting user and writable by no one
else," which was written for a directory we create. The property it was actually
protecting is *nobody less privileged than us can put a file here*, and that
admits root as well:

> A directory is safe to execute from when it is owned by root or by the
> collecting user, and is writable by neither group nor world. A directory is
> safe to write to when it is owned by the collecting user and writable by
> neither group nor world.

The threat model is untouched — decision 32 already concedes that a hostile root
owns the machine outright, so root having placed the binary is not a new
exposure. The hash check still runs on whatever is found, wherever it is found.

**Two artifact axes, which is why this is not one bundle.** The server binary is
built for the arch of the *workstation running the MCP client*; probes are
needed for the arch of each *target*. An admin on x86_64 pre-installing to an
aarch64 NAS needs an aarch64 probe and no aarch64 server. Bundling them would
force a matrix of pairs to express a pair of independent axes.

| artifact | per | contents |
|---|---|---|
| `stethoscope-mcp-<version>-<host-arch>` | server host arch | the server; embeds every probe for every arch, self-sufficient |
| `stethoscope-probes-<version>-<target-arch>.tar.gz` | target arch | probe binaries named `stethoscope-<capability>-<hash>`, mode 755 |
| `stethoscope-manifest-<version>.txt` | release | capability × arch × hash × size (decision 33) |

**The server binary is the source of truth for probe bytes, and the archive is
derived from it.** `stethoscope-mcp --extract-payload <dir> [--arch <arch>]`
writes the embedded probes out under the exact names the server will later look
for, with the exec bit set. That is the primitive; CI builds the tarballs by
running it against the freshly built binary. Two consequences worth the
mechanism:

* **An archive cannot drift from the binary that expects it.** They are the same
  bytes by construction, not by a checksum someone remembers to update.
* **The common install path needs no second download.** The operator already
  trusts the server binary; extracting from it and copying the files out is
  fewer moving parts than fetching a matching tarball. The tarballs exist for
  config management and packaging, which want a URL rather than a program.

**Filenames must ship pre-hashed.** `stethoscope-storage-<hash>`, not
`stethoscope-storage`. An operator who renames or installs an unhashed binary
gets a directory the server reads, finds nothing it recognizes in, and cannot
write to — a confusing dead end. Content-addressed names in the distribution
make the install self-verifying: the file's name is the thing to check it
against.

**Decision 39 extends this.** An `/opt` install is not only the no-write mode
below; it is the only place an operator may grant a probe elevated authority,
because it is the only location root owns and we never write to. The read chain
gains one refusal: an elevated payload found in `$HOME` or `/tmp` is never
executed.

**The payoff is a mode worth naming: with a complete pre-install, the server
never writes to the target at all.** It reads a directory, verifies hashes, and
executes. Decision 32's invariant strengthens from "we write only inside a
namespace we own" to "we wrote nothing," for exactly the operators most likely
to care about the difference. It is also the answer to the host decision 37
declines to serve — a `noexec` home and `noexec` `/tmp` stop mattering when
`/opt/stethoscope` exists, is mounted executable, and holds payloads whose
hashes this build carries. **The operator's one
directory turns out to be the escape hatch for the one case decision 37 gives
up on**, which is a better outcome than either decision reached on its own.

**Garbage collection never touches a read-only location.** Decision 32 already
says a payload we cannot remove is left alone and that this is not an error.
Sharpen it: the collector does not *attempt* removal in a directory outside the
write chain. Trying and failing on every connect is noise in the operator log
and, worse, is an unlink attempt against a system directory showing up in an
audit trail every few minutes.

**Each payload reports the location it came from**, not the call. Decision 29
asks for the location to be reported; a partial pre-install makes that
per-capability — storage from `/opt`, a newer system-health from `$HOME` — and
one field per call could not express it.

Recorded limits:

* **Pre-installs go stale on upgrade, and the failure is silent.** Server N+1
  with a changed storage probe finds no matching hash in a root-owned `/opt`,
  cannot write there, and falls through to `$HOME` — it *works*, which is the
  problem: the operator who deliberately installed system-wide is now quietly
  not using their install. This must be loud in the operator-facing log rather
  than inferred from a location field. It is the most concrete requirement yet
  for open question 3.
* **Nothing here signs anything.** The manifest is the integrity story for the
  archives and it is only as good as the channel it arrives on. Release signing
  is unsolved and is release engineering rather than architecture.
* **`--extract-payload` is the second CLI flag**, after `--payload-manifest`.
  Decision 10 still stands — these print or write what the binary already
  contains and configure nothing — but a third should prompt a look at whether
  a subcommand structure is arriving by accident.
* **A partial pre-install is legal and will happen.** An operator installs the
  three capabilities they care about; the rest come from `$HOME`. Nothing breaks,
  and the mixed result is why locations are reported per payload.
* **The exec-bit and `noexec` cases have no test and no host.** Every
  development-host mount is executable, so "found, verified, and refused to
  run" ships unexercised — the same honest caveat as `ST_RDONLY` in decision 30
  and the missing-PSI path in `system_health`.
* **Falling through after a failed exec can mean two attempts in one call.**
  That is accepted: an exec that failed cost nothing, and predicting the failure
  instead would mean parsing mount options to guess at what trying tells us
  outright.
* **The ownership and mode checks say nothing about symlinks, and `stat`
  follows them.** Decision 31's discovery runs `stat -c '%n %U %a'` on each
  candidate directory, which reports the *target's* owner and mode when the
  candidate is a symlink. A `/tmp/stethoscope` symlink owned by another user,
  pointing at a root-owned directory, therefore passes both checks while its
  creator still controls where the name resolves. The fix is to refuse a
  candidate that is not a real directory (`%F`, or `stat -c` without dereference)
  rather than to reason about where it points. Recorded here because the
  requirement above is what an implementer will read, and it is currently
  satisfiable by a directory that is not one.
* **None of this is built or run.** In particular, "the read chain finds an
  operator's payload and executes it" has never happened, and it is now the
  path most likely to be exercised first by someone other than us.

### 39. An operator may grant a probe elevated authority; the server never grants itself any — accepted

A probe installed in `/opt/stethoscope` may carry file capabilities or the setuid
bit, set deliberately by the operator on a specific hash. The server honours it,
reports that collection ran privileged, and refuses elevated payloads anywhere
else. Collection remains unprivileged by default and must always work that way.

**The unprivileged path stays the default and the guarantee.** Nothing here is
required, nothing degrades without it, and no capability may be designed to need
it. Decision 2 and the README promise the server runs as the invoking user with
exactly that user's permissions; this is the one deliberate exception, granted by
someone who is not us, on a machine we do not administer.

**A `no_std` static probe is an unusually good candidate for elevation, and the
reasons are structural rather than reassuring.** The standard setuid attack
surface is almost entirely absent by construction:

| classic setuid vector | why it is absent |
|---|---|
| `LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT` | no dynamic linker; statically linked (decision 28) |
| libc behaviour driven by env — `MALLOC_*`, locale, NSS | no libc; raw syscalls (decision 28) |
| argument parsing | probes take no arguments — decision 28 rejected subcommands |
| environment parsing | probes read no environment variables |
| config-file parsing | there is no configuration (decision 10) |
| `$PATH` and subprocess execution | a probe execs nothing |
| unbounded syscall surface | ~six syscalls, enumerable by disassembly (decision 32) |

Most setuid binaries in the wild can make none of these claims. This one can make
all of them, which is what moves the proposal from alarming to defensible.

**The risk is not injection — it is read-set widening, and that is the thing to
reason about per capability.** A setuid reader cannot be made to *do* anything
new; it can be made to *see* more. Running as root, a probe reads files the
invoking user cannot, and pipes what it finds toward a model. For
`storage_health` that means capacity figures for three mounts that currently
return `EACCES`/`EPERM` — plainly what was wanted. For a hypothetical privileged
`container_list` it would mean enumerating other users' containers, which may be
exactly right on a server the admin owns and a disclosure they did not intend on
a shared host. **So elevation is per capability, decided by the operator, and the
question they are answering is "should this collector see what root sees."**

**Per-capability probes pay off here in a way they were not designed for.**
Decision 28 split probes for hash stability, allowlisting and link-time least
privilege. It turns out the same split makes privilege grantable at exactly the
right granularity: `setcap` on storage, nothing on the rest. Hash-as-capability-
claim becomes hash-as-*privilege*-claim, and the admin's grant is as narrow as
the binary it is attached to.

**Privilege does not survive an upgrade, and that falls out for free.** A grant
attaches to a specific hash. Release N+1 with a changed storage probe finds no
matching hash in `/opt`, falls through to the write chain, and runs unprivileged
from `$HOME` — reporting less data, never silently reusing an old grant on new
code. This is the inverse of the usual setuid-package problem, where an upgrade
quietly re-privileges a binary nobody re-reviewed. Here, re-granting is a
deliberate act per release, which is the correct amount of friction.

**Prefer file capabilities to setuid.** `setcap cap_dac_read_search+ep` grants
exactly the authority `storage_health` needs — bypass file read and directory
search permission checks — and nothing else. Full setuid root grants everything
and relies on the binary's restraint. Same explicit-grant property, far smaller
blast radius, and it composes with every rule below. Setuid remains the
documented fallback because file capabilities live in extended attributes: they
are lost by a plain `cp`, by `tar` without `--xattrs`, and on filesystems without
xattr support, which makes them the more fragile choice operationally even though
they are the better one securityly.

**The rules the server enforces.** All are cheap, and all fail safe:

* **Elevation is honoured only in `/opt/stethoscope`.** A setuid or
  capability-bearing payload found in `$HOME` or `/tmp` is refused, not executed,
  and logged loudly — the server never writes one, so its presence is decision
  32's "suspicious" branch.
* **The probe must be owned by root, and writable by neither group nor world.**
  Decision 38 already requires the directory to satisfy this; for an elevated
  payload the *file* must too.
* **A loosely permissioned elevated payload is refused rather than run.** If it
  is world-executable, we decline it and fall through. Invoking a
  badly-permissioned setuid binary is how a tool becomes the delivery vehicle for
  someone else's escalation, and declining costs one `stat` we already have.
* **The hash is verified before execution exactly as for any other payload.** The
  TOCTOU window decision 32 records is closed here by root's ownership of the
  directory, which is stronger than the mode-700 rule it relies on elsewhere.

**The payload reports whether collection ran privileged.** This is decision 18's
rule — a figure travels with what is needed to read it — applied to authority.
Without it, `storage_health` returns three more mounts on some hosts than others
and alternates mysteriously after an upgrade rolls the hash, and neither the
model nor the user can tell why. The privilege flag and decision 38's per-payload
location together answer "why did this call see more than that one."

**Two preconditions, and they are not optional.**

1. **"The probe only reads and writes stdout" must become an enforced control,
   not a claim.** Decision 32 says a static probe's syscall set is verifiable by
   disassembly and leaves it there — acceptable for an unprivileged binary,
   insufficient for a privileged one. CI must disassemble each release probe and
   assert its syscall set is a subset of an allowlist, failing the build
   otherwise. This is tractable precisely because the syscall stubs are
   hand-written (decision 28): find each `syscall` / `svc #0` and check the
   immediate loaded into `rax` / `x8`. **Elevation should not be documented as
   supported until this check exists.**
2. **Probes must never read `argv` or `environ`**, stated as a standing
   constraint rather than an accident of current implementation. It is true today
   because decision 28 rejected subcommands; under elevation it becomes
   load-bearing, and a future capability that wants a parameter must be told no.

**Decision 32's "the collectors write nothing" changes status, not content.** It
was recorded as a tidiness property with a nice verifiability story. Under
elevation it is a safety property, and its verification is the CI check above.
Same sentence, much heavier.

**The README's promise must change.** "It runs as the user who launches it and
has exactly that user's permissions" becomes conditionally false the moment an
operator elevates a probe. The honest form says collection runs with the invoking
user's permissions unless an operator has deliberately granted a specific probe
more, and that no such grant is made, requested, or required by this software.

Recorded limits:

* **A grant is durable and will outlive the memory of it.** A setuid binary
  placed in `/opt` in 2026 is still there in 2029, and decision 38 makes `/opt`
  read-only so garbage collection will never remove it. Stale privilege nobody
  remembers granting is a real operational hazard, and the only mitigation
  offered is that `--payload-manifest` makes the grant auditable. Not solved.
* **`nosuid` on `/opt` silently degrades.** Both setuid and file capabilities are
  ignored there, so collection runs unprivileged and simply returns less. The
  privilege field in the payload is what makes this visible rather than
  baffling — which is a second, independent reason for it.
* **Only `storage_health` has a demonstrated gap.** Decision 30 measured three
  mounts refusing `statvfs` on the development host. No other capability has
  shown a need, and none should be given one speculatively.
* **The unprivileged path must stay tested.** It is the default and the
  guarantee, and the risk of a supported privileged mode is that the ordinary one
  quietly becomes the untested one.
* **Nothing here is built, and the disassembly check least of all.** It is a new
  kind of test for this project — inspecting a build artifact rather than running
  it — and its difficulty is unmeasured.
* **`ptrace` and core dumps are disabled by the kernel for elevated binaries**,
  so debugging a privileged probe differs from debugging an ordinary one. Noted
  because it will be surprising in the moment.

---

## Session decisions (2026-09-18)

> Taken while designing and building the probe prologue — the code every
> probe-backed tool runs before its probe does. 40 and 41 were decided before
> any code was written; 42 records what building it settled. `storage_health`
> is the first tool collected by a probe.

### 40. Probes live in `/opt/stethoscope` or `~/.stethoscope`, never `/tmp` — accepted, **amends decisions 29, 32 and 38**

The read chain is `/opt/stethoscope`, then `$HOME/.stethoscope`. The write chain
is `$HOME/.stethoscope` alone. `/tmp` is in neither.

**The rule becomes one sentence a user can hold:** *unless you install the probe
under `/opt/stethoscope`, it is cached in `~/.stethoscope`.* Three locations
with conditional fallthrough between them was a paragraph, and the README would
have had to carry it.

**`/tmp` was never compatible with the probe being a cache.** Decision 29 made
the probe a durable, content-addressed cache precisely so that it is *not* a
per-session deployment, and then kept a fallback location the OS sweeps on
reboot or on age. A cache in `/tmp` is re-transferred on a schedule nobody
chose, which is the staging-shaped behaviour decision 29 rejected in other
words. Decision 29's own recorded limit named the sweep as "a real point in
`/tmp`'s favour" — true for tidiness, and the opposite of what a cache wants.

**It also removes the location with the hardest threat model.** Decision 32's
attack — another unprivileged user pre-creating the directory at a predictable
name — is only possible in a world-writable parent. Nobody but the user can
create an entry in their own home. The symlink gap decision 38 recorded was
written about `/tmp/stethoscope`; it no longer has a shared directory to arrive
through, though decision 42 still refuses a symlinked `~/.stethoscope`.

Recorded limits:

* **A host with a `noexec` home and no `/opt` install cannot be inspected.**
  Previously that took `noexec` on both home and `/tmp`. Decision 29 calls
  `noexec` home an NFS-export convention common in enterprise and academic
  fleets, so this population is larger than the one decision 37 accepted. The
  argument is decision 37's, unchanged, and so is the fix: one directory.
* **Accounts with no usable home fail cleanly.** Service accounts whose home is
  `/` or nonexistent, or `$HOME` unset, get `no_home` or `not_writable` rather
  than a fallback.
* **A full home quota is terminal.** Decision 29 required a clean error; with no
  second location it is the only outcome (`no_space`).
* **An NFS-shared home is one cache for many hosts.** Placement stays benign —
  architectures differ in hash and therefore in name — but open question 14
  gets harder: garbage collection by one host's server version could remove a
  probe another host's server is using.

### 41. The local probe is hashed in-process with `sha2` — accepted, **settles open question 15; amends decisions 14 and 33**

`sha2` is a runtime dependency. On `local` the server hashes the probe itself;
a remote target will still hash with its own `sha256sum`.

**Why not shell out to the local `sha256sum`.** It puts a subprocess on the most
common path in the product, makes coreutils a hard dependency, and turns
decision 32's recorded limit — "verification depends on the target's own
`sha256sum`" — self-referential on the one machine where the digest can simply
be computed.

**The drift decision 37 fears does not arrive through this.** Decision 42's rules
take a hex digest as one field of the facts about a location and compare it to a
string; they do not know or care who computed it. The two transports differ in
*how the fact is gathered*, which they already had to — `lstat` against `stat`
output — and not in how it is judged.

**Hashing from the descriptor makes the local check stronger than the remote one
can be.** The file is opened `O_NOFOLLOW`, and its owner, mode, capability
xattr and contents are all read from that one descriptor, so they describe one
file. Remote collection cannot do this through `stat` and `sha256sum` output,
and that asymmetry is accepted: it is in the gathering, not the rules.

Recorded limits:

* **Cost unmeasured.** `sha2` is pure Rust and brings `digest`, `block-buffer`,
  `hybrid-array`, `typenum`, `crypto-common`, `const-oid` and `cpufeatures`.
  Nothing has been measured against the server binary's size.
* **Hash-then-exec is still two operations.** The descriptor is hashed; the path
  is executed. The window between them is closed by the directory rules, as
  decision 32 says it must be, not by the hash. `fexecve` on the hashed
  descriptor would close it locally and was not adopted: nothing equivalent
  exists over SSH, and a local-only strengthening is the divergence decision 37
  exists to prevent.

### 42. The prologue: rules separated from facts, and what they check — accepted, **settles decision 31's `local` question; extends decision 38**

`src/prologue.rs` is what every probe-backed tool runs before its probe:
resolve through the read chain, place into home if nothing usable was found,
execute, collect. Its design choices, several of which go beyond decisions
32–40:

**The rules are one pure function over gathered facts.** `judge` takes the
metadata of the parent directory, the probe directory and the probe file, plus
the file's digest and whether it carries a capability xattr, and returns
*usable*, *absent* or *refused with a reason*. It touches no filesystem, so it is
unit-tested with fixtures — including the cases no unprivileged test can
produce, like a directory owned by a third user. Gathering, placing and spawning
are the parts a remote transport replaces (decision 31's discovery output
parsed into the same facts). This is the transport seam architecture.md
predicted, and it is still two functions and a struct rather than a trait
(decision 11).

**The rules**, extending decision 38 in three places:

| check | `/opt/stethoscope` | `~/.stethoscope` |
|---|---|---|
| parent (`/opt`, `$HOME`) owned by root or us, not group/world-writable | required if `/opt/stethoscope` exists | required |
| probe directory is a real directory, not a symlink | required | required |
| probe directory owner | root or us | us |
| probe directory group/world-writable | refused | refused |
| probe file is a regular file, owner as for the directory, not group/world-writable | required | required |
| contents hash to the name | required | required |
| setuid, setgid or `security.capability` | honoured if root-owned and not world-executable | refused |
| read / write | read only, never tested for writability | both |

* **The parent check is new.** Without it, whoever can write to `$HOME` can
  rename `~/.stethoscope` away and put their own in its place after we checked
  it. `sshd`'s `StrictModes` applies the same rule to `~/.ssh`'s parent, and
  refuses a group-writable home for the same reason.
  For `/opt` it applies only when `/opt/stethoscope` exists: found by testing
  through MCP in a user namespace, where the host's root-owned `/opt` appears
  owned by the overflow uid and every call reported `installed: unsafe_parent`
  for a location holding nothing. Home keeps the check unconditionally, because
  a missing `~/.stethoscope` is about to be created in that parent.
* **The file gets the owner and mode rules, not only the directory.** A probe
  owned by a third user in a root-owned directory is still theirs to rewrite
  between our hash and our exec.
* **Elevation is detected from the file, not the hash** — the mode's setuid and
  setgid bits and the `security.capability` xattr — because `setcap` leaves the
  contents byte-identical (current-state findings, 2026-09-17).

**Elevation is honoured in `/opt` from the start**, per decision 39 as written.
The alternative considered was refusing elevated probes everywhere until
decision 39's disassembly check exists. It was not chosen. What still waits on
that check is decision 39's other precondition: elevation is not *documented as
supported* until the check is in CI.

**What the model is told** is a category for the call and one reason per
location — `installed: absent, home: hash_mismatch` — as an MCP error carrying
the same in structured `data`. Never a path: `$HOME` contains the username
(decision 9). Paths, errno and the stray-file inventory go to stderr. This
settles the prologue's part of open question 13; the per-row vocabulary is
still open, and so is open question 2's channel.

The reasons: `absent`, `no_home`, `unsafe_parent`, `not_a_directory`,
`unsafe_owner`, `unsafe_mode`, `not_a_regular_file`, `hash_mismatch`,
`elevated_outside_installed`, `elevated_unsafe`, `not_executable`, `no_space`,
`not_writable`, `io_error`. Call-level categories: `payload_unavailable`,
`no_usable_probe`, `probe_failed`, `timed_out`, `malformed_output`.

**Placement** creates the directory and the file mode 700 and then sets the mode
explicitly, because the umask can only remove bits — the finding that `sftp`
honoured two different target umasks applies to `mkdir` just as well. The file
is written under a temporary name in the same directory and renamed into place,
so a concurrent session sees no probe or a whole one. Then the location is
**judged again from scratch**: nothing we wrote is trusted for having been
written by us.

**Execution** gives the probe no arguments, no environment, `/` as its working
directory and `/dev/null` as stdin (decision 39's standing constraint, enforced
at the call site rather than only promised by the probe). A probe that has not
finished in ten seconds is killed. An `EACCES`, `EPERM` or `ENOEXEC` from exec in
`/opt` falls through to home (decision 38).

**`local` has no cache entry.** Decision 31 left this open. Resolution is a
handful of `stat` calls and one hash; running the whole chain every call is the
point of decision 37.

**Not decided here: files with an unknown hash.** Decision 32 says a
`stethoscope-*` file with an unknown hash and a recent mtime is suspicious and
refuses the location. Building this showed that rule breaks the tool on every
upgrade, and permanently while two server versions run side by side — each
version's freshly placed probe is the other's "recent, unknown hash". The
prologue therefore **logs such files and leaves them, without refusing the
location**, pending a decision. See open question 16 in `current-state.md`.

Recorded limits:

* ~~**The ownership rules are only partly exercised on a real filesystem.**~~
  **Now exercised, by decision 43's privileged tier** — a real third user, a
  real root-owned install, real `setcap` and setuid. The first run of it found a
  defect the unit tests could not: when a third user's directory or file was
  mode 700, *gathering* the facts failed with `EACCES` before the rules saw the
  ownership, and the location was refused as `io_error` rather than
  `unsafe_owner`. Still refused, never executed — but the model was told the
  wrong reason. Fixed by recording an unreadable file as a fact
  (`file_unknowable`, or a file with no digest) so the owner rule decides.
* **The namespace `/opt` cases are a stand-in**; decision 43's privileged tier is
  the real root-owned install. Decision 38's warning that the operator install
  path will be exercised first by a stranger now applies to hosts unlike an
  Ubuntu runner, not to the path itself.
* **`elevated_unsafe` checks world-execute, not group.** An operator who grants a
  capability to a probe executable by a broad group has made a choice the server
  honours.
* **Remote gathering must include the capability xattr**, and `getcap` and
  `getfattr` are not guaranteed to be installed on a target. Decision 31's
  discovery command does not yet ask for it. Unsolved.
* **Hosts with a world-writable `/opt` exist, and on them an install is never
  used.** Found on GitHub's hosted runners (decision 43's limits), which make
  `/opt` mode 777. The parent rule refusing it is correct, and collection falls
  back to home, which is safe. The cost is an operator who installed into `/opt`
  and cannot see why the install is ignored: stderr says `unsafe_parent`, and the
  README now states the requirement on `/opt` itself.
* **Ten seconds is a guess**, like decision 31's TTL. Revisit it against a real
  hung mount rather than defend it.
* **Nothing is ever deleted.** Garbage collection waits on open question 14; the
  prologue only reports other versions, and loudly when an `/opt` install is
  stale (decision 38's recorded limit).

### 43. Every probe-backed tool is held to the prologue's constraints in CI — accepted

`.github/workflows/prologue.yml` runs the same prologue cases for every
probe-backed tool, driven by `scripts/prologue/tools.json`. A tool is
probe-backed exactly when its output reports **`probe_location`** (decision 38),
and every such tool must also report **`privileged`** (decision 39). That
contract is what makes the cases tool-independent.

**Adding a tool is one line in `tools.json`.** Its `arguments` must make a
successful call on a bare runner. A tool needing more — a running container,
for `container_health` — is the one foreseeable per-tool cost, and gets an
optional setup field when it arrives rather than before.

**The coverage gate fails the build.** `coverage.sh` asks the server for its
tools and fails if one reports `probe_location` without being listed, or a
listed one lacks either field. Without it, the natural failure is a new tool
shipping with no prologue coverage and nothing saying so.

**Two tiers.** `unprivileged.sh` needs no root and runs anywhere; its `/opt`
cases use a user namespace. `privileged.sh` builds fixtures with sudo — a real
root-owned install, a real third user, real `setcap` and setuid grants, real
`noexec` and full filesystems — and runs only in CI or on an explicitly
disposable machine. **sudo builds fixtures and nothing else: the server always
runs as the unprivileged runner user**, and the script refuses to start as
root. It also refuses to run if `/opt/stethoscope` exists, so it cannot destroy
a real install.

**Skipping is failing.** Ubuntu 24.04 runners block unprivileged user
namespaces, so the namespace cases would pass silently by skipping. The workflow
enables them and passes `--require-all`, which turns a skip into a failure.

**No path filter.** Almost any change can break the prologue. A filter that
misses one path is a check that silently stops running.

Recorded limits:

* **One runner image.** Everything here is Ubuntu 24.04 on x86_64. A host whose
  `/opt` is on another filesystem, or with a different AppArmor or SELinux
  policy, is untested.
* **The deadline is still untested.** No case produces a probe that hangs.
* **A container is not the runner image.** Both tiers passed first in a stock
  `ubuntu:24.04` container and then failed on GitHub (PR #3, 2026-09-19): every
  `/opt` case in the privileged tier was refused with `unsafe_parent`, because
  GitHub's runner image runs `chmod -R 777 /opt`
  (`images/ubuntu/scripts/build/configure-system.sh` in `actions/runner-images`).
  The prologue was right — a world-writable `/opt` lets anyone swap
  `/opt/stethoscope` out from under it — and the fixtures had assumed a normal
  `/opt`. `privileged.sh` now records `/opt`'s owner and mode, sets root:root 755
  for the run (on `/opt` alone), restores it on exit, and **P0** tests the
  world-writable case on purpose. A rehearsal must now start from the runner's
  conditions, not the distribution's defaults.

---

## Session decisions (2026-09-19)

### 44. Probes share one runtime; capabilities are named for their tool; porting is a checklist — accepted, **defers decision 35's guard move to the `system_health` port**

`system_info` is the second probe-backed tool, and porting it settled how every
later port goes.

**The runtime is a crate, `probes/rt/` (`stethoscope-probe-rt`).** Syscalls,
`_start`, the panic handler, the `mem*` intrinsics, the bump allocator,
`privileged()` and `emit()` moved out of the storage probe the moment a second
probe needed them — `current-state.md` had said copying them would be the
moment the drift decision 28 exists to prevent became real. A probe is now one
function returning its report, declared with `probe!(collect)`; `_start` calls a
fixed symbol the macro defines. The collector takes nothing, which makes
decision 39's "probes read neither `argv` nor `environ`" structural rather than
a promise.

**A probe's own syscalls stay in the probe.** `statfs` is in the storage probe,
`uname` in the system-info probe; the runtime carries only what every probe may
need. LTO then removes whatever a probe does not call: the system-info probe
opens no file, so `open`, `read` and `close` are absent from its binary. Each
probe's syscall set is its capability's, which is what decision 39's
disassembly check wants to read.

**Capabilities are named for their tool, hyphenated.** `system_info` is
`system-info`: package `stethoscope-system-info`, directory
`probes/system-info/`, on-disk name `stethoscope-system-info-<sha256>`. The
storage probe became `storage-health` to match; the name is not part of the
hash, and nothing had been released. The prologue now requires a 64-digit hash
after the prefix when it counts other versions of a probe, so a future
`container` capability cannot mistake `stethoscope-container-list-…` for one of
its own.

**The server wraps any probe's report in one generic `Collected<T>`**
(`src/prologue.rs`): the report flattened, plus `target` and `probe_location`.
Per-tool server modules disappear; a probe-backed tool's body is one call to
`prologue::collect("<capability>", target)`. No tool's output schema has a root
title, so the generic changes nothing the model sees.

**`system_info` reads `uname(2)`, not two files.** One syscall returns the
hostname, kernel release and machine architecture that the in-process
collector read from `/proc/sys/kernel/{hostname,osrelease}` and a compile-time
constant. Nothing to parse and no failure worth naming, so the port forced none
of open questions 12 or 13.

**Which defers the read guard's move into core.** Decision 35 puts the guard in
core so probes enforce it. `system_info` now reads no file, so moving the guard
in this port would have been work with no consumer. It moves with
`system_health`, the first probe reading several files.

**The port checklist**, each step of which this port exercised:

1. The wire type in core, one module per probe, carrying `privileged`.
2. A probe crate on `probes/rt`: a `collect()` returning the report, its own
   syscalls, a three-line `build.rs` for the link flags.
3. The tool body in `main.rs` becomes `prologue::collect("<capability>", target)`.
4. The capability in the xtask's `PROBES` and in `scripts/prologue/tools.json`;
   the coverage gate fails CI if the second is forgotten, which it did during
   this port before the line was added.
5. **Parity before deletion**: the new tool's output against the old collector's
   on the same machine — every old field the same value, only `privileged` and
   `probe_location` added — then the old collector is deleted, with no fallback
   (decision 37).

**Measured:** the system-info probe is 6,488 bytes with 60 lines of its own. The
storage probe on the shared runtime is 11,232 bytes, 168 more than before, with
identical output and syscall set and a new hash (`5ff57a55…` → `21725d2a…`).
Both probes pass both prologue tiers, including the runner-condition `/opt`
cases; `system_info` reports `privileged: true` under real `setcap` and setuid
grants without a line of tool-specific test.

Recommended order for the rest, from the discussion that chose this port:
`system_health` (the guard move, the `collected_at` formatter in core, `chrono`
leaving the server), then `container_list` (a `getdents64` syscall, CI needs a
running container), then `container_health`, which takes a `container`
argument that a probe cannot receive — either it reports every container and
the server selects, or decision 39's constraint is revisited. That choice is
not made here.

Recorded limits:

* **The runtime is still x86_64-only.** Two probes now depend on it, which
  raises the cost of the aarch64 half from one probe to a shared crate — the
  right place for it, and still unbuilt.
* **The per-probe `build.rs` is duplicated**, three lines each. Link arguments
  do not propagate from a library to its dependents, so the runtime cannot
  carry them. Accepted.
* **`_start`'s call to the probe is resolved at link time.** A probe that
  forgets `probe!` fails to link rather than to compile — a clear failure, but
  a later one.
