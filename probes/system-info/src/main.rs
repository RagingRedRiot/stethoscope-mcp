//! `stethoscope-system-info` — the `system_info` probe.
//!
//! One `uname(2)` call. It returns the hostname, kernel release and machine
//! architecture the in-process collector used to read from
//! `/proc/sys/kernel/{hostname,osrelease}` and a compile-time constant, with no
//! file opened and nothing to parse — so this probe needs no read guard.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use stethoscope_core::system_info::Report;
use stethoscope_probe_rt::{privileged, sys};

stethoscope_probe_rt::probe!(collect);

const SYS_UNAME: usize = 63;

/// The kernel's `struct utsname`: six NUL-terminated fields of 65 bytes.
#[repr(C)]
struct Utsname {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

fn collect() -> Report {
    let mut u = Utsname {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };
    // Cannot fail with a valid buffer. If it somehow does, exiting non-zero is
    // reported as `probe_failed`, which is truer than empty strings.
    if unsafe { sys::syscall1(SYS_UNAME, &mut u as *mut Utsname as usize) } < 0 {
        sys::exit(1);
    }
    Report {
        hostname: field(&u.nodename),
        kernel_release: field(&u.release),
        os: "linux".into(),
        arch: field(&u.machine),
        privileged: privileged(),
    }
}

/// A NUL-terminated `utsname` field as a string.
fn field(raw: &[u8; 65]) -> String {
    let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..len]).into_owned()
}
