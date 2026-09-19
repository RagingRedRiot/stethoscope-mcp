//! The probes this server carries, embedded at build time (decision 33).
//!
//! `build.rs` generates the list. It is empty in a plain `cargo build`; `cargo
//! xtask dev` fills it with the host architecture's probes.

/// One embedded probe.
pub struct Payload {
    /// Capability name, e.g. `storage`.
    pub capability: &'static str,
    /// Architecture it runs on, as `std::env::consts::ARCH` names it.
    pub arch: &'static str,
    /// Lowercase hex SHA-256 of `bytes`, computed by `build.rs` over the same
    /// bytes it embedded.
    pub sha256: &'static str,
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/payload.rs"));

impl Payload {
    /// The name this probe has on disk, wherever it is found or placed:
    /// `stethoscope-<capability>-<sha256>` (decision 29).
    pub fn file_name(&self) -> String {
        format!("stethoscope-{}-{}", self.capability, self.sha256)
    }
}

/// The probe for `capability` on the machine this server runs on, which is
/// the only architecture `local` needs.
pub fn for_host(capability: &str) -> Option<&'static Payload> {
    PAYLOADS
        .iter()
        .find(|p| p.capability == capability && p.arch == std::env::consts::ARCH)
}

/// Whether a file name is one this build would place, for any capability on
/// any architecture.
pub fn is_ours(file_name: &str) -> bool {
    PAYLOADS.iter().any(|p| p.file_name() == file_name)
}
