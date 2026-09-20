# stethoscope-mcp

A local [MCP](https://modelcontextprotocol.io) server giving an AI assistant
structured, read-only observability into a Linux machine — narrow, named
capabilities instead of shell access.

**Status: early. v0.1 in progress.** Five tools exist: `system_info`,
`system_health`, `container_list`, `container_health` and `storage_health`.
This is a learning project, built deliberately over multiple sessions.

## Container runtime support

Container data comes from the kernel's cgroup v2 hierarchy rather than from any
container runtime's API, so the health, limit and pressure figures are the same
whatever created the container. Only *discovery* is runtime-specific, because
each runtime names its cgroups differently.

| runtime | discovery | status |
|---|---|---|
| Docker (systemd cgroup driver) | `system.slice/docker-<id>.scope` | **tested** |
| podman, rootless | `user.slice/…/user@<uid>.service/user.slice/libpod-<id>.scope` | **tested** |
| podman, rootful | `machine.slice/libpod-<id>.scope` | untested |
| Docker (cgroupfs driver) | `docker/<id>` | untested |
| containerd / CRI-O (Kubernetes) | `kubepods.slice/…/cri-containerd-<id>.scope` | **untested** |

Kubernetes support is a best-effort reading of the documented cgroup layout. No
cluster has been stood up to verify it, and none will be purely to claim
compatibility — so treat that row as unproven rather than broken. Kubernetes
also raises a question the other runtimes do not: whether the tools should
report pods, containers, or both.

**If you run Kubernetes (or rootful podman, or the cgroupfs driver) and would
like to test this, contributions are very welcome.** The useful report is what
`container_list` returns against a real cluster, and if it returns nothing, the
output of `find /sys/fs/cgroup -maxdepth 6 -name "*.scope" | head`. Discovery
lives in one function, `classify` in [`src/cgroup.rs`](src/cgroup.rs), and
adding a naming convention is usually a one-line change plus a test.

## Design

The guiding principle is *give the model capabilities, not credentials or
arbitrary machine access*. There is deliberately no `execute_shell` tool, and
there never will be.

The second one is about what the tools return: **the server and its probes
gather data; the model diagnoses and translates what it means to the operator.**
So the payloads are kernel primitives — block counts with the frame size they
are counted in, free space *and* the smaller figure an unprivileged process can
actually write, pressure-stall figures, a mount that refused to be measured and
the reason it gave. Nothing here computes a verdict or ranks a problem.

Nobody reads primitives to diagnose a machine; that is what `top`, `df` and
`free` are for, and they aggregate away most of what the kernel said. A model
can work from the primitives directly, which is how one structured payload can
answer questions that would otherwise take several tools: whether a full
filesystem is full for everyone or only for non-root, whether a full `tmpfs` is
really RAM, whether a read-only image mount at 100% is a problem at all.

Design documentation lives in [`docs/internal/`](docs/internal/) and is written
as working memory rather than polished docs:

* [`current-state.md`](docs/internal/current-state.md) — start here: what
  exists versus what is only planned
* [`architecture.md`](docs/internal/architecture.md) — the design and its
  trust boundaries
* [`decisions.md`](docs/internal/decisions.md) — what we decided and why

## Build and run

```sh
cargo xtask release
```

The binary lands in `target/release/stethoscope-mcp`. Not `cargo build`: some
tools collect by running a small, separate probe binary, and the xtask builds
those probes and embeds them in the server. `cargo xtask dev` is the unoptimized
equivalent for development. `release` also strips the building machine's paths
(home directory, username) out of the binary, and fails if any remain.
A plain `cargo build` produces a server with no probes, which says so on stderr
at startup and cannot answer `storage_health`.

`stethoscope-mcp` speaks MCP over stdio and is meant to be spawned by an MCP client.
It runs as the user who launches it and has exactly that user's permissions.

### What it writes: `~/.stethoscope`

**Unless you install the probes under `/opt/stethoscope`, the server caches them
in `~/.stethoscope`.** That is the only place it writes. It creates the
directory mode 700 and places each probe there under a name that is its own
SHA-256, `stethoscope-<capability>-<sha256>`. The probe stays there between
sessions as a cache.

Before executing a probe, from either location, the server checks that the
file's contents match the hash in its name and that the directory and the file
can be written by nobody but you (or root, for `/opt`). It refuses anything
that fails, leaves it in place, and reports why. Nothing in `~/.stethoscope` is
deleted automatically yet.

`/opt/stethoscope` is read, never written. If an operator puts probes there,
the server uses them and writes nothing at all. The file names must be the
hashed names above. There is no packaged way to obtain them yet — they are the
files under `target/payload/<arch>/` after a build, renamed to include their
hash.

An install is used only if `/opt` itself, `/opt/stethoscope` and each probe are
owned by root and writable by neither group nor world. If anyone else could
write to `/opt`, they could swap `/opt/stethoscope` for a directory of their
own, so the server ignores the install and falls back to `~/.stethoscope`, with
an `unsafe_parent` line on stderr. Some images loosen `/opt` — GitHub's hosted
runners make it mode 777 — so check it with `stat -c '%U %a' /opt` if an
install is not being picked up.

A machine where `~/.stethoscope` is on a `noexec` mount, and `/opt/stethoscope`
does not exist, cannot be inspected.

Collection runs with your permissions unless an operator has deliberately
granted a specific probe more in `/opt/stethoscope`. This software never makes,
requests or requires such a grant.
