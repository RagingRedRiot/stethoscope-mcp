//! `statfs(2)`: the one syscall this probe makes that no other probe needs.

use stethoscope_probe_rt::sys::syscall3;

const SYS_STATFS: usize = 137;

/// The kernel's `struct statfs` for x86_64: 120 bytes, all 64-bit words.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Statfs {
    pub f_type: u64,
    pub f_bsize: u64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_fsid: u64,
    pub f_namelen: u64,
    pub f_frsize: u64,
    pub f_flags: u64,
    pub f_spare: [u64; 4],
}

/// `ST_RDONLY`, as reported in `f_flags`. Present since Linux 2.6.36.
pub const ST_RDONLY: u64 = 1;

/// `statfs(2)` on a NUL-terminated path. On failure returns the negated errno
/// the kernel gave, because decision 30 reports *why* a mount could not be
/// measured rather than dropping the row.
///
/// This is `statfs(2)`, not glibc's `statvfs(3)` — there is no `statvfs`
/// syscall, and with no libc there is nothing to wrap it. The fields decision
/// 30 names are the same ones; see this crate's README for the one place the
/// two differ (`f_frsize` as the frame size).
pub fn statfs(path: &[u8]) -> Result<Statfs, i32> {
    let mut s = Statfs::default();
    let r = unsafe {
        syscall3(
            SYS_STATFS,
            path.as_ptr() as usize,
            &mut s as *mut Statfs as usize,
            0,
        )
    };
    if r < 0 { Err(-r as i32) } else { Ok(s) }
}
