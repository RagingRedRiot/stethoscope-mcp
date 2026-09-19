//! Raw syscalls, the entry stub, the panic handler, and the `mem*` intrinsics
//! the linker demands of a freestanding binary.
//!
//! Per-architecture and hand-written, which decision 28 records as roughly
//! thirty lines each: x86_64 `syscall` with rax/rdi/rsi/rdx here, aarch64
//! `svc #0` with x8/x0/x1/x2 when the release matrix needs it.
//!
//! Only syscalls every probe may need live here: `open`, `read`, `close`,
//! `write`, `mmap`, `exit`, and `getuid`, `geteuid` and `capget` for the
//! privilege flag. A probe's own syscalls — `statfs`, `uname` — live in that
//! probe, built on [`syscall3`] and friends, so each probe's syscall set stays
//! as small as its capability (decision 32, and decision 39's disassembly
//! check).

use core::arch::{asm, naked_asm};
use core::ffi::c_void;

pub const SYS_READ: usize = 0;
pub const SYS_WRITE: usize = 1;
pub const SYS_OPEN: usize = 2;
pub const SYS_CLOSE: usize = 3;
pub const SYS_MMAP: usize = 9;
pub const SYS_EXIT: usize = 60;
pub const SYS_GETUID: usize = 102;
pub const SYS_GETEUID: usize = 107;
pub const SYS_CAPGET: usize = 125;

/// # Safety
///
/// `n` must be a syscall number and the arguments valid for it: any argument
/// the kernel dereferences must point to memory valid for that access.
#[inline(always)]
pub unsafe fn syscall1(n: usize, a1: usize) -> isize {
    let r: isize;
    unsafe {
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1,
             lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    r
}

/// # Safety
///
/// `n` must be a syscall number and the arguments valid for it: any argument
/// the kernel dereferences must point to memory valid for that access.
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

/// # Safety
///
/// `n` must be a syscall number and the arguments valid for it: any argument
/// the kernel dereferences must point to memory valid for that access.
#[inline(always)]
pub unsafe fn syscall6(
    n: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
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
    // Unreachable: the kernel has already torn the process down.
    loop {
        core::hint::spin_loop();
    }
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
            syscall3(
                SYS_READ,
                fd as usize,
                buf.as_mut_ptr().add(off) as usize,
                buf.len() - off,
            )
        };
        if n <= 0 {
            break;
        }
        off += n as usize;
    }
    unsafe { syscall1(SYS_CLOSE, fd as usize) };
    off
}

/// # Safety
///
/// `n` must be a syscall number and the arguments valid for it: any argument
/// the kernel dereferences must point to memory valid for that access.
#[inline(always)]
pub unsafe fn syscall2(n: usize, a1: usize, a2: usize) -> isize {
    let r: isize;
    unsafe {
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a1, in("rsi") a2,
             lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    r
}

pub fn getuid() -> u32 {
    unsafe { syscall1(SYS_GETUID, 0) as u32 }
}

pub fn geteuid() -> u32 {
    unsafe { syscall1(SYS_GETEUID, 0) as u32 }
}

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

/// The process's effective capability set, as one 64-bit word.
///
/// `_LINUX_CAPABILITY_VERSION_3` returns two 32-bit slices; they are folded
/// together because the only question asked of this is "is it empty". Zero on
/// failure, which is the same answer as "no capabilities" — see the caller for
/// why that is the wrong way to be wrong and how it compensates.
pub fn cap_effective() -> u64 {
    let hdr = CapHeader {
        version: 0x2008_0522,
        pid: 0,
    };
    let mut data = [CapData::default(); 2];
    let r = unsafe {
        syscall2(
            SYS_CAPGET,
            &hdr as *const CapHeader as usize,
            data.as_mut_ptr() as usize,
        )
    };
    if r < 0 {
        return 0;
    }
    (data[1].effective as u64) << 32 | data[0].effective as u64
}

unsafe extern "C" {
    /// Defined in each probe by [`probe!`](crate::probe).
    fn stethoscope_probe_main() -> !;
}

// The kernel enters at `_start` with RSP 16-byte aligned, but the SysV ABI has
// callees assume RSP%16 == 8 on entry (as left by a CALL). Without this stub
// every SSE spill in the callee faults.
/// # Safety
///
/// The process entry point. Only the kernel calls it.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    naked_asm!(
        "xor rbp, rbp",
        "and rsp, -16",
        "call {main}",
        main = sym stethoscope_probe_main,
    )
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(101)
}

// The `mem*` intrinsics the linker demands of a freestanding binary. Their
// signatures use `c_void` rather than `u8` deliberately: rustc checks these
// against the declarations the standard library expects and warns on a
// mismatch, and a warning on every build of the binary that most wants a clean
// one is a warning nobody reads.

/// # Safety
///
/// The C library contract: the pointers must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    let (dst, src) = (d as *mut u8, s as *const u8);
    let mut i = 0;
    while i < n {
        unsafe { *dst.add(i) = *src.add(i) };
        i += 1;
    }
    d
}

/// # Safety
///
/// The C library contract: the pointers must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    let (dst, src) = (d as *mut u8, s as *const u8);
    if (dst as usize) < (src as usize) {
        let mut i = 0;
        while i < n {
            unsafe { *dst.add(i) = *src.add(i) };
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe { *dst.add(i) = *src.add(i) };
        }
    }
    d
}

/// # Safety
///
/// The C library contract: the pointers must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(d: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let dst = d as *mut u8;
    let mut i = 0;
    while i < n {
        unsafe { *dst.add(i) = c as u8 };
        i += 1;
    }
    d
}

/// # Safety
///
/// The C library contract: the pointers must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (x, y) = (a as *const u8, b as *const u8);
    let mut i = 0;
    while i < n {
        let (p, q) = unsafe { (*x.add(i), *y.add(i)) };
        if p != q {
            return p as i32 - q as i32;
        }
        i += 1;
    }
    0
}

/// # Safety
///
/// The C library contract: the pointers must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}
