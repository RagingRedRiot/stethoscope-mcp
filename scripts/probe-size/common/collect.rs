//! The storage capability itself: parse `/proc/self/mountinfo`, `statfs(2)`
//! each real mount. Identical in both builds — only the serialization of
//! `Raw` differs, which is exactly the delta being measured.

use crate::rt::{self, Statfs};

pub const MOUNTINFO_CAP: usize = 64 * 1024;
pub const MAX_MOUNTS: usize = 256;

/// One filesystem as collected, before it is put on the wire.
#[derive(Clone, Copy)]
pub struct Raw<'a> {
    pub mount_point: &'a str,
    pub source: &'a str,
    pub fs_type: &'a str,
    pub stat: Statfs,
}

fn field<'a>(line: &'a [u8], want: usize) -> Option<&'a [u8]> {
    let mut i = 0;
    let mut n = 0;
    while i < line.len() {
        while i < line.len() && line[i] == b' ' {
            i += 1;
        }
        let start = i;
        while i < line.len() && line[i] != b' ' {
            i += 1;
        }
        if i > start {
            if n == want {
                return Some(&line[start..i]);
            }
            n += 1;
        }
    }
    None
}

/// Index of the `-` separator field, after which fstype and source sit.
fn separator(line: &[u8]) -> Option<usize> {
    let mut i = 0;
    let mut n = 0;
    while i < line.len() {
        while i < line.len() && line[i] == b' ' {
            i += 1;
        }
        let start = i;
        while i < line.len() && line[i] != b' ' {
            i += 1;
        }
        if i > start {
            if i - start == 1 && line[start] == b'-' {
                return Some(n);
            }
            n += 1;
        }
    }
    None
}

fn as_str(b: &[u8]) -> &str {
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => "",
    }
}

/// Fills `out` with every mount that reports non-zero blocks. Returns the count.
pub fn collect<'a>(buf: &'a [u8], out: &mut [Raw<'a>]) -> usize {
    let mut count = 0;
    for line in buf.split(|&c| c == b'\n') {
        if line.is_empty() || count >= out.len() {
            continue;
        }
        let sep = match separator(line) {
            Some(s) => s,
            None => continue,
        };
        let mp = match field(line, 4) {
            Some(f) => f,
            None => continue,
        };
        let fs_type = match field(line, sep + 1) {
            Some(f) => f,
            None => continue,
        };
        let source = match field(line, sep + 2) {
            Some(f) => f,
            None => continue,
        };

        // statfs needs a NUL-terminated path.
        let mut path = [0u8; 4096];
        if mp.len() >= path.len() {
            continue;
        }
        path[..mp.len()].copy_from_slice(mp);
        let stat = match rt::statfs(&path[..mp.len() + 1]) {
            Some(s) => s,
            None => continue,
        };
        if stat.f_blocks == 0 {
            continue;
        }

        out[count] = Raw {
            mount_point: as_str(mp),
            source: as_str(source),
            fs_type: as_str(fs_type),
            stat,
        };
        count += 1;
    }
    count
}

/// Reads mountinfo into `buf` and collects. Returns the number of entries.
pub fn run<'a>(buf: &'a mut [u8], out: &mut [Raw<'a>]) -> usize {
    let n = rt::read_file(b"/proc/self/mountinfo\0", buf);
    collect(&buf[..n], out)
}
