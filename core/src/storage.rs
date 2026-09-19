//! The `storage_health` wire format (decision 30).
//!
//! Written by `probes/storage/`, read by the server. `Serialize` is what the
//! probe uses and `Deserialize` is what the server uses; a probe never calls
//! the latter, so LTO strips it, exactly as it strips `JsonSchema` (decision
//! 28).

use alloc::string::String;
use alloc::vec::Vec;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Inode capacity. Absent entirely on a filesystem with no inode concept —
/// vfat reports `f_files == 0`, and reporting that as a zero total would read
/// as "completely full" rather than "not applicable" (decision 30).
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Inodes {
    /// Total inodes the filesystem can hold.
    pub total: u64,
    /// Inodes not currently in use. A filesystem at 2% bytes and 100% inodes
    /// is full, and that is invisible from byte capacity alone.
    pub free: u64,
}

/// Capacity for one filesystem.
///
/// Block counts and the frame size ship alongside the byte figures derived
/// from them: decision 18 keeps denominators with their numerators and puts
/// plain arithmetic in the payload, so the model never has to reconstruct a
/// total and never has to guess a unit.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Capacity {
    /// True when the filesystem is mounted read-only. A filesystem remounted
    /// read-only after an I/O error is a first-rank cause of "it worked
    /// yesterday". Read-only image mounts report 100% used by nature; this is
    /// what tells them apart from a filesystem that filled up.
    pub read_only: bool,
    /// Size in bytes of the block the counts below are expressed in.
    pub frame_size: u64,
    /// Total blocks in the filesystem.
    pub blocks_total: u64,
    /// Blocks not in use, *including* the root-reserved pool.
    pub blocks_free: u64,
    /// Blocks an unprivileged process may actually write. On a typical ext4
    /// root this is ~5% below `blocks_free`; a model told only the free figure
    /// is wrong by that much about what a service can write, and "df shows
    /// free space but writes fail" is exactly the failure this explains.
    pub blocks_available: u64,
    /// `blocks_total` × `frame_size`.
    pub total_bytes: u64,
    /// `blocks_free` × `frame_size`.
    pub free_bytes: u64,
    /// `blocks_available` × `frame_size`.
    pub available_bytes: u64,
    /// Inode capacity, absent on a filesystem with no inode concept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inodes: Option<Inodes>,
}

/// Why a discovered mountpoint could not be measured.
///
/// **Open question 13 is unresolved**, so these names are the pilot's
/// assumption rather than a decision. They follow decision 9's rule that the
/// model gets a category and never a diagnostic string.
#[derive(Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// The invoking user may not measure it. May become measurable if an
    /// operator elevates the probe (decision 39).
    PermissionDenied,
    /// Unmounted between being listed and being measured.
    NotFound,
    /// The filesystem returned an I/O error.
    IoError,
    /// Measuring it timed out — usually an unreachable network mount.
    TimedOut,
    /// A dead network-filesystem handle.
    StaleHandle,
    /// Any other failure.
    Unavailable,
}

impl Unavailable {
    /// Errno to the vocabulary the payload uses.
    pub fn from_errno(errno: i32) -> Self {
        match errno {
            1 | 13 => Self::PermissionDenied, // EPERM, EACCES
            2 => Self::NotFound,              // ENOENT — unmounted mid-read
            5 => Self::IoError,               // EIO
            110 => Self::TimedOut,            // ETIMEDOUT
            116 => Self::StaleHandle,         // ESTALE — dead NFS handle
            _ => Self::Unavailable,
        }
    }
}

/// One filesystem.
///
/// Every mountpoint the kernel reports which has capacity appears here, and so
/// does every mountpoint that *refused* to be measured — the latter carrying
/// `unavailable` in place of `capacity`. Dropping those would hide a hung NFS
/// mount, which is a first-rank cause of "the service froze" (decision 30).
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Filesystem {
    /// Where the filesystem is mounted.
    pub mount_point: String,
    /// The kernel's device identity, `major:minor`. Two mountpoints sharing it
    /// are the same filesystem, so their free space must not be added up. The
    /// converse does not hold: container overlay mounts get their own device
    /// id while reporting the underlying filesystem's numbers, so equal
    /// capacity figures under different ids may still be one filesystem. The
    /// mount *source* is deliberately not reported: it carries device paths
    /// and, for a network filesystem, an address (decision 30).
    pub device: String,
    /// Filesystem type, e.g. `ext4`. Worth weighing: a full `tmpfs` is RAM.
    pub fs_type: String,
    /// Present when the filesystem was measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<Capacity>,
    /// Present when it was not. The mount is real and was discovered; its
    /// capacity could not be read. A `permission_denied` row may become
    /// measurable if an operator elevates this probe (decision 39); a
    /// `timed_out` or `io_error` row is usually a broken network mount.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}

/// The storage capability's complete response, as the probe emits it.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Report {
    /// Whether collection ran with authority beyond the invoking user's —
    /// setuid, or file capabilities on this binary. False is the default, the
    /// guarantee, and what an operator has to deliberately change (decision
    /// 39). It travels with the data because it changes how many rows are
    /// measurable, and without it the row count varies between hosts for no
    /// visible reason.
    pub privileged: bool,
    /// True when `/proc/self/mountinfo` was larger than the probe's buffer, so
    /// rows are missing. False in every ordinary case.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub truncated: bool,
    /// One row per mountpoint with capacity, plus one per mountpoint that
    /// refused to be measured.
    pub filesystems: Vec<Filesystem>,
}
