//! Parsers for the kernel virtual-file formats a probe reads.
//!
//! This module knows how those files are *shaped*. It deliberately does not
//! know what any tool wants to say about them: response types and their
//! interpretation live with the tool that returns them. That split is what lets
//! a second consumer reuse a parser without inheriting the first one's response
//! shape — per-container pressure under `/sys/fs/cgroup/*.pressure` is
//! byte-for-byte the format [`psi_avg`] already parses.
//!
//! Every function takes `&str` and returns `Option`. What a `None` becomes —
//! an exit code in a probe, an `ErrorData` in the server — is the caller's
//! concern, and neither belongs in core (decision 35).

/// Count the per-CPU lines in `/proc/stat` — `cpu0`, `cpu1`, … — skipping the
/// leading `cpu` aggregate line.
pub fn count_cpus(stat: &str) -> u32 {
    stat.lines()
        .filter(|line| {
            line.strip_prefix("cpu")
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        })
        .count() as u32
}

/// Boot time from `/proc/stat`, as seconds since the Unix epoch.
///
/// Adding `/proc/uptime` to this gives the target's own idea of the current
/// wall-clock time, from files already being read — no second mechanism, and
/// nothing that stops working when the read is happening over SSH.
pub fn stat_btime(stat: &str) -> Option<u64> {
    let value = stat.lines().find_map(|line| line.strip_prefix("btime "))?;
    value.split_whitespace().next()?.parse().ok()
}

/// Pull one `/proc/meminfo` field, converting to bytes.
///
/// The file labels its values `kB` but reports KiB. Correcting that here,
/// once, in trusted code is the whole of decision 18's first rule: the
/// alternative is every consumer downstream re-learning the same footgun.
pub fn meminfo_bytes(meminfo: &str, key: &str) -> Option<u64> {
    let value = meminfo
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))?;
    let kib: u64 = value.split_whitespace().next()?.parse().ok()?;
    kib.checked_mul(1024)
}

/// A pressure-stall file is two lines, `some` and `full`, each of the form
/// `some avg10=0.00 avg60=0.00 avg300=0.00 total=1714525`.
///
/// `line` selects which of the two ("some " or "full "), `key` which token
/// ("avg10=", "total=").
fn psi_token<'a>(text: &'a str, line: &str, key: &str) -> Option<&'a str> {
    text.lines()
        .find(|l| l.starts_with(line))?
        .split_whitespace()
        .find_map(|token| token.strip_prefix(key))
}

/// One pressure average, as a percentage of wall-clock time.
pub fn psi_avg(text: &str, line: &str, key: &str) -> Option<f64> {
    psi_token(text, line, key)?.parse().ok()
}

/// Cumulative stall microseconds since boot for one of the two lines.
pub fn psi_total(text: &str, line: &str) -> Option<u64> {
    psi_token(text, line, "total=")?.parse().ok()
}
