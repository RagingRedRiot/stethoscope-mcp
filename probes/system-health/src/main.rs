//! `stethoscope-system-health` — the `system_health` probe.
//!
//! Reads `/proc/{uptime,stat,loadavg,meminfo}` and the three
//! `/proc/pressure/*` files, every one of them through the read guard in core
//! (decision 35). The parsers are core's too, so this file holds only what the
//! capability says: which files, which fields, and the arithmetic decision 18
//! puts beside them.
//!
//! `collected_at` is the *target's* own clock — boot time plus uptime — which
//! is why it is formatted here rather than by the server (decision 19).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};

use stethoscope_core::proc;
use stethoscope_core::system_health::{
    CpuHealth, CpuPressure, MemoryHealth, Pressure, PressureSet, Report,
};
use stethoscope_core::time;
use stethoscope_probe_rt::{privileged, sys};

stethoscope_probe_rt::probe!(collect);

/// One buffer, reused for each file. `/proc/stat` is the largest — roughly
/// 90 bytes per CPU — and this holds a machine with thousands of them.
const BUF: usize = 256 * 1024;
static mut SCRATCH: [u8; BUF] = [0; BUF];

/// A kernel virtual file as a string, or `None` if it could not be read.
fn slurp(path: &str) -> Option<String> {
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(SCRATCH) };
    let n = sys::read_file(path, buf);
    if n == 0 {
        return None;
    }
    Some(core::str::from_utf8(&buf[..n]).ok()?.trim().to_string())
}

/// A file this capability cannot do without. Exiting non-zero is reported by
/// the server as `probe_failed`; there is no partial answer worth sending.
fn require(path: &str) -> String {
    match slurp(path) {
        Some(text) => text,
        None => sys::exit(1),
    }
}

fn parse<T: core::str::FromStr>(value: Option<&str>) -> T {
    match value.and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => sys::exit(1),
    }
}

fn collect() -> Report {
    // /proc/uptime: seconds since boot, then idle seconds summed over all
    // CPUs — which is why the second figure routinely exceeds the first.
    let uptime_raw = require("/proc/uptime");
    let mut uptime_fields = uptime_raw.split_whitespace();
    let uptime_seconds: f64 = parse(uptime_fields.next());
    let idle_seconds: f64 = parse(uptime_fields.next());

    // /proc/stat's per-CPU lines are the authority on how many CPUs are
    // online, and the same file carries boot time, so the target's wall clock
    // costs no extra read.
    let stat = require("/proc/stat");
    let cpu_count = proc::count_cpus(&stat);
    if cpu_count == 0 {
        sys::exit(1);
    }
    let btime = match proc::stat_btime(&stat) {
        Some(b) => b,
        None => sys::exit(1),
    };
    let collected_at = match i64::try_from(btime)
        .ok()
        .and_then(|b| b.checked_add(uptime_seconds as i64))
        .and_then(time::rfc3339)
    {
        Some(t) => t,
        None => sys::exit(1),
    };

    // /proc/loadavg: three averages, runnable/total tasks, and the PID of the
    // last process created.
    let loadavg_raw = require("/proc/loadavg");
    let loadavg: alloc::vec::Vec<&str> = loadavg_raw.split_whitespace().collect();
    let load_1m: f64 = parse(loadavg.first().copied());
    let load_5m: f64 = parse(loadavg.get(1).copied());
    let load_15m: f64 = parse(loadavg.get(2).copied());
    let mut tasks = loadavg.get(3).copied().unwrap_or_default().split('/');
    let runnable_tasks: u32 = parse(tasks.next());
    let total_tasks: u32 = parse(tasks.next());

    let meminfo = require("/proc/meminfo");
    let mem = |key: &str| match proc::meminfo_bytes(&meminfo, key) {
        Some(v) => v,
        None => sys::exit(1),
    };
    let total_bytes = mem("MemTotal");
    let available_bytes = mem("MemAvailable");
    let swap_total_bytes = mem("SwapTotal");
    let swap_free_bytes = mem("SwapFree");

    Report {
        collected_at,
        uptime_seconds,
        cpu: CpuHealth {
            count: cpu_count,
            load_1m,
            load_5m,
            load_15m,
            load_1m_per_cpu: round(load_1m / f64::from(cpu_count), 3),
            busy_fraction_since_boot: round(
                busy_fraction(idle_seconds, uptime_seconds, cpu_count),
                3,
            ),
            runnable_tasks,
            total_tasks,
        },
        memory: MemoryHealth {
            total_bytes,
            available_bytes,
            available_percent: round(percent_of(available_bytes, total_bytes), 1),
            cached_bytes: mem("Cached"),
            dirty_bytes: mem("Dirty"),
            swap_total_bytes,
            swap_used_bytes: swap_total_bytes.saturating_sub(swap_free_bytes),
            kernel_unreclaimable_bytes: mem("SUnreclaim"),
        },
        pressure: pressure(),
        privileged: privileged(),
    }
}

/// Fraction of CPU time spent doing work since boot.
///
/// `/proc/uptime`'s idle figure is summed across CPUs, so it has to be divided
/// by their number before it can be compared against wall-clock uptime. The
/// clamp guards the edges: the two figures come from different clocks and can
/// disagree slightly, which would otherwise produce a negative "busy".
fn busy_fraction(idle_seconds: f64, uptime_seconds: f64, cpu_count: u32) -> f64 {
    if uptime_seconds <= 0.0 {
        return 0.0;
    }
    let idle_per_cpu = idle_seconds / f64::from(cpu_count);
    (1.0 - idle_per_cpu / uptime_seconds).clamp(0.0, 1.0)
}

fn percent_of(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 * 100.0 / whole as f64
}

/// Round a derived value, so arithmetic on kernel readings does not hand back
/// more apparent precision than the readings ever had.
fn round(value: f64, places: i32) -> f64 {
    let scale = pow10(places);
    round_half_away(value * scale) / scale
}

/// `10^places` for the small positive exponents [`round`] uses. `f64::powi`
/// and `f64::round` are libm calls, which a freestanding binary does not have;
/// these two functions are why.
fn pow10(places: i32) -> f64 {
    let mut scale = 1.0;
    for _ in 0..places {
        scale *= 10.0;
    }
    scale
}

/// Round half away from zero, which is what `f64::round` does.
///
/// Via `i64`, so it is exact for everything this rounds — percentages, loads
/// and fractions, all far inside the integer range. Anything that would not
/// fit is returned unrounded rather than wrapping.
fn round_half_away(value: f64) -> f64 {
    const LIMIT: f64 = 9.0e15;
    if !(-LIMIT..=LIMIT).contains(&value) {
        return value;
    }
    if value >= 0.0 {
        (value + 0.5) as i64 as f64
    } else {
        (value - 0.5) as i64 as f64
    }
}

/// The pressure block, or `None` if this kernel offers no PSI at all.
///
/// Every failure here collapses to an absent field rather than failing the
/// call: a machine without PSI still has a perfectly good answer for load and
/// memory (decision 18, rule 7). The three resources are read independently —
/// one unreadable file says nothing about the other two.
fn pressure() -> Option<PressureSet> {
    let cpu = slurp("/proc/pressure/cpu")
        .as_deref()
        .and_then(cpu_pressure);
    let memory = slurp("/proc/pressure/memory")
        .as_deref()
        .and_then(full_pressure);
    let io = slurp("/proc/pressure/io")
        .as_deref()
        .and_then(full_pressure);

    // All three absent is the ordinary "kernel has no PSI" answer, and an
    // empty object would say less than no object at all.
    if cpu.is_none() && memory.is_none() && io.is_none() {
        return None;
    }
    Some(PressureSet { cpu, memory, io })
}

fn cpu_pressure(text: &str) -> Option<CpuPressure> {
    Some(CpuPressure {
        some_avg10: proc::psi_avg(text, "some ", "avg10=")?,
        some_avg60: proc::psi_avg(text, "some ", "avg60=")?,
        some_avg300: proc::psi_avg(text, "some ", "avg300=")?,
        some_total_us: proc::psi_total(text, "some ")?,
    })
}

fn full_pressure(text: &str) -> Option<Pressure> {
    Some(Pressure {
        some_avg10: proc::psi_avg(text, "some ", "avg10=")?,
        full_avg10: proc::psi_avg(text, "full ", "avg10=")?,
        some_avg60: proc::psi_avg(text, "some ", "avg60=")?,
        full_avg60: proc::psi_avg(text, "full ", "avg60=")?,
        some_avg300: proc::psi_avg(text, "some ", "avg300=")?,
        full_avg300: proc::psi_avg(text, "full ", "avg300=")?,
        some_total_us: proc::psi_total(text, "some ")?,
        full_total_us: proc::psi_total(text, "full ")?,
    })
}
