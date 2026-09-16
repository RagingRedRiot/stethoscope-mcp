//! Freestanding runtime shared by both probe variants: raw syscalls, a
//! stack-aligning entry stub, a panic handler, and the mem* intrinsics the
//! linker demands. Identical in both builds, so it cancels out of the delta.

use core::arch::{asm, naked_asm};

pub const SYS_READ: usize = 0;
pub const SYS_WRITE: usize = 1;
pub const SYS_OPEN: usize = 2;
pub const SYS_CLOSE: usize = 3;
pub const SYS_MMAP: usize = 9;
pub const SYS_EXIT: usize = 60;
pub const SYS_STATFS: usize = 137;

#[inline(always)]
pub unsafe fn syscall1(n: usize, a1: usize) -> isize {
    let r: isize;
    unsafe {
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1,
             lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    r
}

#[inline(always)]
pub unsafe fn syscall3(n: usize, a1: usize, a2: usize, a3: usize) -> isize {
    let r: isize;
    unsafe {
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1,
             in("rsi") a2, in("rdx") a3,
             lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    r
}

#[inline(always)]
pub unsafe fn syscall6(
    n: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize,
) -> isize {
    let r: isize;
    unsafe {
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1,
             in("rsi") a2, in("rdx") a3, in("r10") a4, in("r8") a5, in("r9") a6,
             lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    r
}

pub fn exit(code: i32) -> ! {
    unsafe { syscall1(SYS_EXIT, code as usize) };
    loop {}
}

pub fn write_all(fd: usize, mut buf: &[u8]) {
    while !buf.is_empty() {
        let n = unsafe { syscall3(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()) };
        if n <= 0 {
            return;
        }
        buf = &buf[n as usize..];
    }
}

/// Reads a whole file into `buf`, truncating past its length. Returns the
/// number of bytes read.
pub fn read_file(path: &[u8], buf: &mut [u8]) -> usize {
    let fd = unsafe { syscall3(SYS_OPEN, path.as_ptr() as usize, 0, 0) };
    if fd < 0 {
        return 0;
    }
    let mut off = 0usize;
    loop {
        if off >= buf.len() {
            break;
        }
        let n = unsafe {
            syscall3(SYS_READ, fd as usize, buf.as_mut_ptr().add(off) as usize, buf.len() - off)
        };
        if n <= 0 {
            break;
        }
        off += n as usize;
    }
    unsafe { syscall1(SYS_CLOSE, fd as usize) };
    off
}

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

/// `path` must be NUL-terminated.
pub fn statfs(path: &[u8]) -> Option<Statfs> {
    let mut s = Statfs::default();
    let r = unsafe {
        syscall3(SYS_STATFS, path.as_ptr() as usize, &mut s as *mut Statfs as usize, 0)
    };
    if r < 0 { None } else { Some(s) }
}

// The kernel enters at `_start` with RSP 16-byte aligned, but the SysV ABI has
// callees assume RSP%16 == 8 on entry (as left by a CALL). Without this stub
// every SSE spill in the callee faults.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    naked_asm!(
        "xor rbp, rbp",
        "and rsp, -16",
        "call {main}",
        main = sym crate::probe_main,
    )
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(101)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *d.add(i) = *s.add(i) };
        i += 1;
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    if (d as usize) < (s as usize) {
        unsafe { memcpy(d, s, n) }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe { *d.add(i) = *s.add(i) };
        }
        d
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(d: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *d.add(i) = c as u8 };
        i += 1;
    }
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let (x, y) = unsafe { (*a.add(i), *b.add(i)) };
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}
