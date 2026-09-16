//! Storage probe, `serde` derive + `serde_json`. The wire format is one
//! derived definition on the response types — the same types the server would
//! hand to `schemars` — so there is no second hand-written format to drift.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../common/rt.rs"]
mod rt;
#[path = "../../common/collect.rs"]
mod collect;

use alloc::{string::{String, ToString}, vec::Vec};
use collect::{MAX_MOUNTS, MOUNTINFO_CAP, Raw};
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use rt::Statfs;
use serde::Serialize;
use schemars::JsonSchema;

static mut MOUNTINFO: [u8; MOUNTINFO_CAP] = [0; MOUNTINFO_CAP];

/// A bump allocator over one anonymous mapping. A probe is single-shot and
/// short-lived, so nothing is ever freed.
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

/// One filesystem on the wire. Byte counts are absolute; denominators travel
/// with their numerators so the model never has to reconstruct a total.
#[derive(Serialize, JsonSchema)]
struct Filesystem {
    /// Where the filesystem is mounted, e.g. `/var`.
    mount_point: String,
    /// The backing device or source, as the kernel reports it.
    source: String,
    /// Filesystem type, e.g. `ext4`.
    fs_type: String,
    total_bytes: u64,
    available_bytes: u64,
    used_bytes: u64,
    total_inodes: u64,
    available_inodes: u64,
}

/// The storage capability's complete response.
#[derive(Serialize, JsonSchema)]
struct Report {
    filesystems: Vec<Filesystem>,
}

#[unsafe(no_mangle)]
pub extern "C" fn probe_main() -> ! {
    let mountinfo = unsafe { &mut *core::ptr::addr_of_mut!(MOUNTINFO) };
    let mut entries = [Raw {
        mount_point: "",
        source: "",
        fs_type: "",
        stat: Statfs::default(),
    }; MAX_MOUNTS];

    let n = collect::run(mountinfo, &mut entries);

    let mut filesystems = Vec::with_capacity(n);
    for e in &entries[..n] {
        let Statfs { f_bsize, f_blocks, f_bavail, f_files, f_ffree, f_bfree, .. } = e.stat;
        filesystems.push(Filesystem {
            mount_point: e.mount_point.to_string(),
            source: e.source.to_string(),
            fs_type: e.fs_type.to_string(),
            total_bytes: f_blocks * f_bsize,
            available_bytes: f_bavail * f_bsize,
            used_bytes: (f_blocks - f_bfree) * f_bsize,
            total_inodes: f_files,
            available_inodes: f_ffree,
        });
    }

    let json = match serde_json::to_string(&Report { filesystems }) {
        Ok(j) => j,
        Err(_) => rt::exit(1),
    };
    rt::write_all(1, json.as_bytes());
    rt::write_all(1, b"\n");
    rt::exit(0)
}
