//! `stethoscope-storage` — the `storage_health` probe.
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
mod rt;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

use collect::{MOUNTINFO_CAP, Measured};
use stethoscope_core::storage::{Capacity, Filesystem, Inodes, Report, Unavailable};

static mut MOUNTINFO: [u8; MOUNTINFO_CAP] = [0; MOUNTINFO_CAP];

/// A bump allocator over one anonymous mapping. A probe is single-shot and
/// short-lived, so nothing is ever freed — decision 28 records that this makes
/// "a probe is single-shot" a constraint on future capabilities rather than an
/// implementation detail.
struct Bump {
    base: AtomicUsize,
    next: AtomicUsize,
    end: AtomicUsize,
}

const ARENA: usize = 8 * 1024 * 1024;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if self.base.load(Ordering::Relaxed) == 0 {
            // PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS
            let p = unsafe { rt::syscall6(rt::SYS_MMAP, 0, ARENA, 3, 0x22, !0, 0) };
            if p < 0 {
                return core::ptr::null_mut();
            }
            self.base.store(p as usize, Ordering::Relaxed);
            self.next.store(p as usize, Ordering::Relaxed);
            self.end.store(p as usize + ARENA, Ordering::Relaxed);
        }
        let align = layout.align();
        let cur = self.next.load(Ordering::Relaxed);
        let start = (cur + align - 1) & !(align - 1);
        let new = start + layout.size();
        if new > self.end.load(Ordering::Relaxed) {
            return core::ptr::null_mut();
        }
        self.next.store(new, Ordering::Relaxed);
        start as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: Bump = Bump {
    base: AtomicUsize::new(0),
    next: AtomicUsize::new(0),
    end: AtomicUsize::new(0),
};

/// Whether this probe is running with authority beyond the invoking user's.
///
/// Both mechanisms decision 39 permits have to be covered, and they look
/// nothing alike: setuid shows up as an effective uid differing from the real
/// one (or as root), while file capabilities — the *preferred* grant, because
/// `cap_dac_read_search+ep` is far narrower than setuid root — leave both uids
/// untouched and show up only in the effective capability set.
///
/// Asked of the kernel directly rather than read out of `/proc/self/status`.
/// The obvious file is the wrong source here: `guard.rs` denies every
/// `/proc/<pid>/` path but `comm`, and names `status` in its documentation as
/// denied on purpose (decision 23). Three syscalls cost less than an exception
/// to that rule, and they leave the probe's read set untouched.
fn privileged() -> bool {
    let euid = rt::geteuid();
    euid == 0 || euid != rt::getuid() || rt::cap_effective() != 0
}

#[unsafe(no_mangle)]
pub extern "C" fn probe_main() -> ! {
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
                        read_only: s.f_flags & rt::ST_RDONLY != 0,
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

    let report = Report {
        privileged: privileged(),
        truncated: mounts.truncated,
        filesystems,
    };
    let json = match serde_json::to_string(&report) {
        Ok(j) => j,
        Err(_) => rt::exit(1),
    };
    rt::write_all(1, json.as_bytes());
    rt::write_all(1, b"\n");
    rt::exit(0)
}
