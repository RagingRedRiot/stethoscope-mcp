//! `stethoscope-storage-health` — the `storage_health` probe.
//!
//! Decision 28's process shape: one capability, one statically linked `no_std`
//! binary, raw syscalls, no arguments, no environment, no subprocesses, one
//! JSON document on stdout. Decision 28's settled serialization: `serde` +
//! `serde_json` with `JsonSchema` derived on the same types, so there is no
//! second hand-written wire format to drift from this one.
//!
//! The response types live in `stethoscope-core` (decision 35) and are linked
//! from there, so the probe that writes the storage wire format and the server
//! that reads it share one definition. This crate carries no read guard yet,
//! because the guard has not moved into core; its entire read set is
//! `/proc/self/mountinfo`, one `EXACT` entry.
//!
//! See the README for the four open questions this code is standing on.

#![no_std]
#![no_main]

extern crate alloc;

mod collect;
mod statfs;

use alloc::vec::Vec;

use collect::{MOUNTINFO_CAP, Measured};
use stethoscope_core::storage::{Capacity, Filesystem, Inodes, Report, Unavailable};
use stethoscope_probe_rt::privileged;

stethoscope_probe_rt::probe!(collect_report);

static mut MOUNTINFO: [u8; MOUNTINFO_CAP] = [0; MOUNTINFO_CAP];

fn collect_report() -> Report {
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(MOUNTINFO) };
    let mounts = collect::run(buf);

    let mut filesystems = Vec::with_capacity(mounts.rows.len());
    for row in mounts.rows {
        let (capacity, unavailable) = match row.measured {
            Measured::Capacity(s) => {
                // `statvfs(3)` expresses its block counts in `f_frsize`.
                // `statfs(2)` sets both, and they are equal on every
                // filesystem seen here; prefer `f_frsize` so the meaning
                // matches decision 30's "frame size", and fall back for a
                // filesystem that leaves it zero.
                let frame = if s.f_frsize > 0 {
                    s.f_frsize
                } else {
                    s.f_bsize
                };
                let inodes = (s.f_files > 0).then_some(Inodes {
                    total: s.f_files,
                    free: s.f_ffree,
                });
                (
                    Some(Capacity {
                        read_only: s.f_flags & statfs::ST_RDONLY != 0,
                        frame_size: frame,
                        blocks_total: s.f_blocks,
                        blocks_free: s.f_bfree,
                        blocks_available: s.f_bavail,
                        total_bytes: s.f_blocks.saturating_mul(frame),
                        free_bytes: s.f_bfree.saturating_mul(frame),
                        available_bytes: s.f_bavail.saturating_mul(frame),
                        inodes,
                    }),
                    None,
                )
            }
            Measured::Failed(e) => (None, Some(Unavailable::from_errno(e))),
            Measured::Pseudo => continue,
        };
        filesystems.push(Filesystem {
            mount_point: row.mount_point,
            device: row.device,
            fs_type: row.fs_type,
            capacity,
            unavailable,
        });
    }

    Report {
        privileged: privileged(),
        truncated: mounts.truncated,
        filesystems,
    }
}
