//! `stethoscope-probe-rt` — what every probe shares, so that a probe's own
//! crate holds nothing but its capability.
//!
//! Decision 28's process shape: one capability, one statically linked `no_std`
//! binary, raw syscalls, no arguments, no environment, no subprocesses, one
//! JSON document on stdout. This crate is that shape; a probe supplies one
//! function returning its report and declares it with [`probe!`]:
//!
//! ```ignore
//! stethoscope_probe_rt::probe!(collect);
//! fn collect() -> MyReport { … }
//! ```
//!
//! The report must carry `privileged` (decision 39; decision 43's contract);
//! [`privileged`] computes it.
//!
//! Extracted from the storage probe when the second probe arrived, which is the
//! moment `current-state.md` said copying it would become real drift.

#![no_std]

extern crate alloc;

pub mod sys;

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

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
            let p = unsafe { sys::syscall6(sys::SYS_MMAP, 0, ARENA, 3, 0x22, !0, 0) };
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
pub fn privileged() -> bool {
    let euid = sys::geteuid();
    euid == 0 || euid != sys::getuid() || sys::cap_effective() != 0
}

/// Serialize `report` as one JSON document on stdout, then exit.
///
/// Exit 0 with the document, or exit 1 with nothing on stdout if it could not
/// be serialized — the server treats a non-zero exit as `probe_failed` and
/// never parses a partial document.
pub fn emit<T: Serialize>(report: &T) -> ! {
    let json = match serde_json::to_string(report) {
        Ok(j) => j,
        Err(_) => sys::exit(1),
    };
    sys::write_all(1, json.as_bytes());
    sys::write_all(1, b"\n");
    sys::exit(0)
}

/// Declare a probe's collector: a function taking nothing and returning a
/// serializable report. Expands to the entry point `_start` calls.
///
/// Taking nothing is decision 39's standing constraint made structural: a
/// probe reads neither `argv` nor `environ`, and there is no way to hand it
/// either.
#[macro_export]
macro_rules! probe {
    ($collect:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn stethoscope_probe_main() -> ! {
            $crate::emit(&$collect())
        }
    };
}
