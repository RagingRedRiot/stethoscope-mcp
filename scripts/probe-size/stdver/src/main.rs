//! The same storage capability built normally on `std`, to calibrate the
//! no_std numbers against decision 28's recorded 291,784 B static-`std` row.

use serde::Serialize;
use std::arch::asm;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Statfs {
    f_type: u64, f_bsize: u64, f_blocks: u64, f_bfree: u64, f_bavail: u64,
    f_files: u64, f_ffree: u64, f_fsid: u64, f_namelen: u64, f_frsize: u64,
    f_flags: u64, f_spare: [u64; 4],
}

fn statfs(path: &str) -> Option<Statfs> {
    let mut c = path.as_bytes().to_vec();
    c.push(0);
    let mut s = Statfs::default();
    let r: isize;
    unsafe {
        asm!("syscall", inlateout("rax") 137isize => r, in("rdi") c.as_ptr(),
             in("rsi") &mut s as *mut Statfs, in("rdx") 0,
             lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    if r < 0 { None } else { Some(s) }
}

#[derive(Serialize)]
struct Filesystem {
    mount_point: String, source: String, fs_type: String,
    total_bytes: u64, available_bytes: u64, used_bytes: u64,
    total_inodes: u64, available_inodes: u64,
}

#[derive(Serialize)]
struct Report { filesystems: Vec<Filesystem> }

fn main() {
    let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let mut filesystems = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let Some(sep) = f.iter().position(|&x| x == "-") else { continue };
        if f.len() < sep + 3 || sep < 5 { continue }
        let (mp, fs_type, source) = (f[4], f[sep + 1], f[sep + 2]);
        let Some(s) = statfs(mp) else { continue };
        if s.f_blocks == 0 { continue }
        filesystems.push(Filesystem {
            mount_point: mp.to_string(),
            source: source.to_string(),
            fs_type: fs_type.to_string(),
            total_bytes: s.f_blocks * s.f_bsize,
            available_bytes: s.f_bavail * s.f_bsize,
            used_bytes: (s.f_blocks - s.f_bfree) * s.f_bsize,
            total_inodes: s.f_files,
            available_inodes: s.f_ffree,
        });
    }
    println!("{}", serde_json::to_string(&Report { filesystems }).unwrap());
}
