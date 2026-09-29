//! Reading the kernel's virtual filesystems, and parsers for the formats found
//! there.
//!
//! Why data comes from `/proc` rather than from running OS
//! utilities: there is no command to construct, the files are identical across
//! distributions, and it needs no dependency.
//!
//! This module knows how these files are *shaped*. It deliberately does not
//! know what any tool wants to say about them: response types and their
//! interpretation live with the tool that returns them. That split is what
//! lets a second consumer reuse a parser without inheriting the first one's
//! response shape — per-container pressure under `/sys/fs/cgroup/*.pressure`
//! is byte-for-byte the format [`psi_avg`] already parses, but it will belong
//! to a different tool with a different output type.

use rmcp::ErrorData;

use stethoscope_core::guard;
// The parsers live in core, where the probes use them too (decision 35).
// Re-exported so this module stays the one place the server's tools look.
pub use stethoscope_core::proc::{psi_avg, psi_total, stat_btime};

/// Read a kernel virtual file whole, trimmed.
///
/// `tokio::fs` is not truly async — it hands the ordinary blocking read to
/// `spawn_blocking`. That is the point: the wait, however brief, lands on the
/// blocking pool instead of an async worker thread, so no tool can stall the
/// runtime for the requests running alongside it. Reads here are microseconds,
/// but the policy is what matters once tools do heavier I/O.
///
/// The underlying io::Error is intentionally not forwarded to the model: it
/// can carry local paths and details the model has no need for. Detail belongs
/// in operator-facing logs (not yet implemented), not in model context.
///
/// This is also where local and remote will eventually diverge, and it is
/// deliberately not behind a trait yet. Every parser in this module takes
/// `&str` rather than a path, so `ssh nas cat /proc/meminfo` would feed them
/// unchanged; the calls to this function are the only local-specific code in
/// the crate. What the seam should look like is the open part. A per-path read
/// forces one SSH round trip per file, where `system_health`'s seven files
/// clearly want a single batched invocation — and a local implementation pays
/// nothing either way, so it cannot settle the question. Interactive
/// authentication and the failure semantics of an unreachable host would shape
/// the signature too, and neither is decided.
pub async fn read(path: &str) -> Result<String, ErrorData> {
    if let Err(denied) = guard::check(path) {
        eprintln!("stethoscope-mcp: refusing to read {path}: {denied}");
        return Err(ErrorData::internal_error(
            "failed to read system information",
            None,
        ));
    }
    tokio::fs::read_to_string(path)
        .await
        .map(|s| s.trim().to_owned())
        .map_err(|e| {
            eprintln!("stethoscope-mcp: failed to read {path}: {e}");
            ErrorData::internal_error("failed to read system information", None)
        })
}

/// Read a file that may legitimately not exist.
///
/// `None` means the kernel does not provide it — an older kernel, or a feature
/// compiled out.
pub async fn read_optional(path: &str) -> Option<String> {
    if let Err(denied) = guard::check(path) {
        eprintln!("stethoscope-mcp: refusing to read {path}: {denied}");
        return None;
    }
    match tokio::fs::read_to_string(path).await {
        Ok(text) => Some(text),
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("stethoscope-mcp: failed to read {path}: {e}");
            }
            None
        }
    }
}

/// List the subdirectories of a kernel virtual directory.
///
/// Only the cgroup tree is listable — see [`guard::check_dir`]. `None` means the
/// directory is absent or unreadable, which is an ordinary answer on a machine
/// with no cgroup controllers delegated where the caller asked anyway.
///
/// Returns full paths rather than names so the caller cannot accidentally
/// rebuild one by string concatenation and get it wrong.
pub async fn read_dir(path: &str) -> Option<Vec<String>> {
    if let Err(denied) = guard::check_dir(path) {
        eprintln!("stethoscope-mcp: refusing to list {path}: {denied}");
        return None;
    }
    let mut entries = tokio::fs::read_dir(path).await.ok()?;
    let mut out = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        // Only directories: a cgroup is a directory, and its files are read by
        // name rather than discovered.
        if entry.file_type().await.is_ok_and(|t| t.is_dir())
            && let Some(p) = entry.path().to_str()
        {
            out.push(p.to_owned());
        }
    }
    Some(out)
}

/// Format an epoch second as RFC 3339 in UTC, for a caller that already has one.
pub fn wall_clock_from(epoch_seconds: i64) -> String {
    stethoscope_core::time::rfc3339(epoch_seconds).unwrap_or_default()
}

/// Read the target's clock for a tool that has no other reason to touch
/// `/proc/stat` and `/proc/uptime`.
///
/// `system_health` reads both files for their own sake and calls
/// [`wall_clock`] directly; the container tools need only the timestamp, so the
/// two reads are done here rather than repeated at every call site.
pub async fn collected_at() -> Result<String, ErrorData> {
    let epoch = target_now().await?;
    stethoscope_core::time::rfc3339(epoch)
        .ok_or_else(|| parse_error("/proc/stat", "boot time out of range"))
}

/// The target's own idea of the current time, as seconds since the epoch.
///
/// Boot time plus uptime, so it is the *target's* clock rather than this
/// server's — which is what lets it be differenced against a file timestamp
/// from the same machine, and what keeps it correct once the reads happen over
/// SSH (decision 19).
pub async fn target_now() -> Result<i64, ErrorData> {
    let uptime_raw = read("/proc/uptime").await?;
    let uptime: f64 = field(
        uptime_raw.split_whitespace().next(),
        "/proc/uptime",
        "uptime",
    )?;
    let stat = read("/proc/stat").await?;
    let btime = stat_btime(&stat).ok_or_else(|| parse_error("/proc/stat", "btime"))?;
    i64::try_from(btime)
        .ok()
        .and_then(|b| b.checked_add(uptime as i64))
        .ok_or_else(|| parse_error("/proc/stat", "boot time out of range"))
}

/// Modification time of a cgroup directory, as seconds since the epoch.
///
/// A cgroup directory is created when a container's processes start and removed
/// when they exit — verified for both Docker and podman, where stopping a
/// container deletes the directory and starting it again recreates it with the
/// same container ID and a fresh timestamp. So this is when the *current run*
/// began rather than when the container was created, and it is the denominator
/// the cumulative `*_total_us` figures need: those counters live in this same
/// directory and reset with it, so the two always share an origin.
///
/// Guarded by [`guard::check_dir`] rather than by a rule of its own. This reads
/// metadata about a directory the server may already enumerate, which reveals
/// strictly less than the enumeration itself does.
pub async fn dir_mtime(path: &str) -> Option<i64> {
    if let Err(denied) = guard::check_dir(path) {
        eprintln!("stethoscope-mcp: refusing to stat {path}: {denied}");
        return None;
    }
    let meta = tokio::fs::metadata(path).await.ok()?;
    let secs = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    i64::try_from(secs).ok()
}

/// Report a malformed file the way a failed read is reported: detail to stderr
/// for the operator, a flat message to the model.
///
/// These files are kernel-generated and their layouts are stable, so reaching
/// here means something is genuinely wrong rather than merely unexpected.
pub fn parse_error(path: &str, what: &str) -> ErrorData {
    eprintln!("stethoscope-mcp: could not parse {what} from {path}");
    ErrorData::internal_error("failed to read system information", None)
}

/// Parse one whitespace-delimited field, or fail with [`parse_error`].
pub fn field<T: std::str::FromStr>(
    value: Option<&str>,
    path: &str,
    what: &str,
) -> Result<T, ErrorData> {
    value
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| parse_error(path, what))
}
