//! The `system_info` wire format.
//!
//! Written by `probes/system-info/`, read by the server, which adds `target`
//! and `probe_location` around it.

use alloc::string::String;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Basic identity of a machine, as the kernel reports it.
///
/// The field set is deliberately small: it exists to prove the plumbing, not to
/// be an inventory.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Report {
    /// The machine's hostname, as the kernel holds it (`uname -n`).
    pub hostname: String,
    /// The running kernel's release string (`uname -r`).
    pub kernel_release: String,
    /// The operating system. Always `linux`.
    pub os: String,
    /// The CPU architecture, as the kernel names it (`uname -m`), e.g.
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// Whether collection ran with authority beyond the invoking user's
    /// (decision 39). Nothing here depends on it; it is reported because every
    /// probe-backed tool reports it.
    pub privileged: bool,
}
