# Probe size measurement

Answers decision 28's open question: what does `serde` + `alloc` +
`serde_json` cost a `no_std` probe, against hand-rolling the JSON?

Run `./measure.sh`. Requires a nightly-free stable toolchain with
`x86_64-unknown-linux-gnu` installed; builds are stable-only.

Result, 2026-09-11 on rustc 1.98.1: **+2,696 bytes**, and deriving
`JsonSchema` on top of that adds **zero**, because a probe never calls
`schema_for!` and LTO removes schemars entirely. The full write-up, including
the recorded limits, is in decision 28.

## Why the harness is kept

The first prototype was built, measured, and then lost with its scratch
directory — so when the serialization question came up, the only thing standing
against it was a 50–100 KB guess, which turned out to be wrong by more than an
order of magnitude. The numbers in decision 28 are only worth anything if they
can be re-derived, so the code that produced them lives here.

## Layout

* `common/` — the parts that must be identical across variants: the
  freestanding runtime (raw syscalls, stack-aligning entry stub, panic handler,
  `mem*` intrinsics) and the storage capability itself (parse
  `/proc/self/mountinfo`, `statfs(2)` each mount with non-zero blocks).
* `handrolled/` — writes the JSON by hand into a fixed buffer. The baseline.
* `serde/` — derives `Serialize` and calls `serde_json::to_string`, over a bump
  allocator on one anonymous mapping.
* `schemars/` — `serde/` plus `#[derive(JsonSchema)]`, to check whether the
  schema derive follows the types into the probe. It does not.
* `stdver/` — the same capability built normally on `std`. Not a candidate;
  it exists to calibrate this reimplementation against decision 28's recorded
  291,784 B row, since the original prototype binary no longer exists.

These are deliberately *not* workspace members. Each is freestanding, pins its
own link flags in `.cargo/config.toml`, and must not inherit the server's
profile or dependency versions.
