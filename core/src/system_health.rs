//! The `system_health` wire format.
//!
//! Units normalized at the boundary, derived values computed alongside the
//! inputs they came from, and no verdicts in the payload (decision 18).
//!
//! `///` on a nested struct or a field is model-facing: `schemars` lifts it
//! into the output schema and every character is spent on each `tools/list`.
//! It earns its place only by changing how the model reads a number. Design
//! rationale goes in `//` instead, which stays invisible to the model. A doc
//! comment on the root struct is dropped by `schemars` and so is free.

use alloc::string::String;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Resource-contention health for a target.
///
/// The four sources are read together deliberately: the useful conclusions
/// come from the combination, not from any single number. The model-facing
/// half of that lives on [`CpuHealth`], since a doc comment here is dropped
/// before the schema is generated.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Report {
    /// Wall-clock time of this reading, RFC 3339 in UTC, to one-second resolution, as the *target's* own clock reports it. If the target's clock is wrong then so is this field.
    ///
    /// Use it to judge staleness and to line readings up against other machines or external logs. Do *not* use it to measure the interval between two readings: subtract `uptime_seconds` instead, which is monotonic and so unaffected by clock steps from NTP.
    pub collected_at: String,
    /// Seconds since boot. On a machine that suspends, this includes the time spent suspended.
    pub uptime_seconds: f64,
    pub cpu: CpuHealth,
    pub memory: MemoryHealth,
    /// Absent when the kernel supplies no pressure-stall information at all: PSI needs Linux 4.20 or later and can be compiled out or disabled. Absence is a normal answer, not a failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<PressureSet>,
    /// Whether collection ran with authority beyond the invoking user's
    /// (decision 39). Nothing here needs it; it is reported because every
    /// probe-backed tool reports it.
    pub privileged: bool,
}

/// CPU demand.
///
/// Linux load average counts tasks in uninterruptible sleep — usually blocked on disk — as well as tasks waiting for a CPU, so high load alone does not mean CPU saturation. Read it against `pressure`, which separates the two causes: high load with low `pressure.cpu` and high `pressure.io` means work is blocked on disk rather than starved of CPU.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CpuHealth {
    /// Online CPUs. The load figures below cannot be interpreted without it.
    pub count: u32,
    /// Load averaged over the last minute. These are exponentially damped rather than sliding windows, so they lag a change in both directions: a still-elevated average does not prove the cause is still present. The trend across the three is the signal — 1m above 15m means it is getting worse now.
    pub load_1m: f64,
    pub load_5m: f64,
    pub load_15m: f64,
    /// `load_1m` divided by `count`. Around 1.0 means fully subscribed; well above 1.0 means more demand than the machine can serve at once.
    pub load_1m_per_cpu: f64,
    /// Share of all CPU time spent doing work since boot, 0.0 to 1.0. A long-run average over `uptime_seconds`, not a current reading.
    pub busy_fraction_since_boot: f64,
    /// Tasks runnable at the instant of reading.
    pub runnable_tasks: u32,
    /// Tasks that exist, kernel threads included. A steadily climbing count across readings suggests something is spawning and not reaping.
    pub total_tasks: u32,
}

// `MemFree` is deliberately not reported. On a healthy busy machine it is near
// zero, because the kernel spends otherwise-idle memory on page cache — which
// makes it the most commonly misread field in `/proc/meminfo`.
// `available_bytes` is the field that answers whether the machine can take on
// more work. No `///` here: the model needs no account of a field it cannot see.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct MemoryHealth {
    pub total_bytes: u64,
    /// The kernel's own estimate of what new work could allocate without pushing the machine into swap, counting the page cache and slab it could reclaim to satisfy the request.
    pub available_bytes: u64,
    /// `available_bytes` as a percentage of `total_bytes`.
    pub available_percent: f64,
    /// Page cache. Reclaimable, so this is not memory lost.
    pub cached_bytes: u64,
    /// Modified pages not yet written back to disk. Large and growing across readings suggests writes backing up against a slow device.
    pub dirty_bytes: u64,
    pub swap_total_bytes: u64,
    /// Swap in use. Occupied swap is not itself a problem — evicting cold pages is correct behavior. Sustained swap *activity* is the problem, and it shows up in `pressure.memory` rather than here.
    pub swap_used_bytes: u64,
    /// Kernel data structures that cannot be reclaimed under pressure. Large and steadily growing across readings is the signature of a kernel or driver memory leak.
    pub kernel_unreclaimable_bytes: u64,
}

/// Kernel pressure-stall information. Each of the three resources is reported independently, and any one of them may be absent while the others are present.
///
/// Each average is a percentage of wall-clock time over the trailing 10, 60 or 300 seconds, already normalized: no dividing by CPU count, no second sample needed. PSI measures time in which work did not happen *because a resource was unavailable* — a task blocked on a socket, a lock, or its own sleep is not stalled in this sense.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PressureSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<CpuPressure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Pressure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io: Option<Pressure>,
}

/// CPU pressure: time during which at least one task was runnable but not scheduled.
// Only `some` is reported. "Every task stalled" is structurally impossible for
// CPU at machine level, because if anything is runnable the CPU is running it —
// the kernel always reports zero there, and returning a permanently-zero field
// would invite meaning to be read into it. Per-cgroup CPU `full` *is*
// meaningful, since a whole container can be starved while others run, but that
// comes from cgroup data rather than this file.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CpuPressure {
    /// Percentage of the last 10 seconds in which at least one task was waiting for a CPU. Below 10 is unremarkable; sustained above 30 is real contention.
    pub some_avg10: f64,
    pub some_avg60: f64,
    pub some_avg300: f64,
    /// Cumulative microseconds of stall since boot. Monotonic, so the difference between two readings is the exact stall time in between — useful when the smoothed averages are too coarse.
    pub some_total_us: u64,
}

/// Stall time for memory or I/O.
///
/// `some` is the share of time at least one task was stalled. `full` is the share in which *every* non-idle task was stalled simultaneously, so nothing useful happened at all — `full_avg10` above roughly 10 means the machine is losing a serious fraction of its capacity to this resource. Memory stalls count refaults, thrashing, direct reclaim and swap-in; I/O stalls count waiting on actual disk reads and writes.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Pressure {
    pub some_avg10: f64,
    pub full_avg10: f64,
    pub some_avg60: f64,
    pub full_avg60: f64,
    pub some_avg300: f64,
    pub full_avg300: f64,
    /// Cumulative microseconds of stall since boot; see `CpuPressure`.
    pub some_total_us: u64,
    pub full_total_us: u64,
}
