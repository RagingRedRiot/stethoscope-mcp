//! The storage capability: parse `/proc/self/mountinfo`, `statfs(2)` every
//! mountpoint, and report each one as capacity, as pseudo, or as unavailable.
//!
//! Decision 30 governs the shape of the result:
//!
//! * identity is `major:minor`; **the mount source never leaves this file**,
//!   because it carries device paths and, on a remote target, NFS and CIFS
//!   addresses that decision 9 refuses the model,
//! * `f_blocks > 0` is the filter for "has capacity to report", which is a
//!   kernel-supplied fact rather than a blocklist of fstypes we would own,
//! * a mount that refuses `statfs` is reported, not dropped — that is the same
//!   code path a hung NFS mount produces, and it is the most diagnostic thing
//!   this capability can say.

use alloc::string::String;
use alloc::vec::Vec;

use crate::statfs::{self, Statfs};
use stethoscope_probe_rt::sys;

/// `/proc/self/mountinfo` is read into a fixed buffer before anything is
/// allocated. Decision 28 records that a host with thousands of mounts needs
/// handling rather than a bigger constant; this probe truncates, and
/// [`Mounts::truncated`] says so rather than silently reporting fewer rows.
pub const MOUNTINFO_CAP: usize = 256 * 1024;

/// What `statfs(2)` said about one mountpoint.
pub enum Measured {
    /// Capacity was read and the filesystem reports blocks.
    Capacity(Statfs),
    /// Capacity was read and the filesystem reports none — a pure pseudo
    /// filesystem. Dropped from the payload entirely.
    Pseudo,
    /// The call failed. Carries the errno so the row can say why.
    Failed(i32),
}

/// One mountpoint as collected, before it is put on the wire.
pub struct Raw {
    pub mount_point: String,
    pub device: String,
    pub fs_type: String,
    pub measured: Measured,
}

pub struct Mounts {
    pub rows: Vec<Raw>,
    /// True when mountinfo did not fit in [`MOUNTINFO_CAP`], so the last line
    /// read may be partial and later mounts are missing.
    pub truncated: bool,
}

/// Splits on runs of spaces, yielding the `want`-th field.
fn field(line: &[u8], want: usize) -> Option<&[u8]> {
    fields(line).nth(want)
}

fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(|&c| c == b' ').filter(|f| !f.is_empty())
}

/// Index of the `-` separator field, after which fstype and source sit.
fn separator(line: &[u8]) -> Option<usize> {
    fields(line).position(|f| f == b"-")
}

/// mountinfo octal-escapes space, tab, newline and backslash in the paths it
/// prints. `statfs` needs the real bytes, so a mountpoint under `/mnt/my
/// disk` is unreachable without this.
fn unescape(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            let d = &b[i + 1..i + 4];
            if d.iter().all(|c| (b'0'..=b'7').contains(c)) {
                let v = (d[0] - b'0') * 64 + (d[1] - b'0') * 8 + (d[2] - b'0');
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn as_string(b: &[u8]) -> String {
    match core::str::from_utf8(b) {
        Ok(s) => String::from(s),
        Err(_) => String::new(),
    }
}

fn measure(path: &[u8]) -> Measured {
    // statfs needs a NUL-terminated path.
    let mut c = Vec::with_capacity(path.len() + 1);
    c.extend_from_slice(path);
    c.push(0);
    match statfs::statfs(&c) {
        Ok(s) if s.f_blocks == 0 => Measured::Pseudo,
        Ok(s) => Measured::Capacity(s),
        Err(e) => Measured::Failed(e),
    }
}

pub fn collect(buf: &[u8]) -> Vec<Raw> {
    let mut rows = Vec::new();
    for line in buf.split(|&c| c == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Some(sep) = separator(line) else { continue };
        let (Some(device), Some(mp), Some(fs_type)) =
            (field(line, 2), field(line, 4), field(line, sep + 1))
        else {
            continue;
        };
        // Field sep + 2 is the mount source. It is read by nothing: decision 30
        // drops it at the parser, and `device` above is what replaces it.

        let path = unescape(mp);
        let measured = measure(&path);
        if matches!(measured, Measured::Pseudo) {
            continue;
        }
        rows.push(Raw {
            mount_point: as_string(&path),
            device: as_string(device),
            fs_type: as_string(fs_type),
            measured,
        });
    }
    rows
}

/// Reads mountinfo into `buf` and collects.
pub fn run(buf: &mut [u8]) -> Mounts {
    let n = sys::read_file(b"/proc/self/mountinfo\0", buf);
    Mounts {
        rows: collect(&buf[..n]),
        truncated: n == buf.len(),
    }
}
