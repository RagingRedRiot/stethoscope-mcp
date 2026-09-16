//! Storage probe, hand-rolled JSON. The wire format lives here, written by
//! hand, with no derived definition anywhere — this is the baseline.

#![no_std]
#![no_main]

#[path = "../../common/rt.rs"]
mod rt;
#[path = "../../common/collect.rs"]
mod collect;

use collect::{MAX_MOUNTS, MOUNTINFO_CAP, Raw};
use rt::Statfs;

static mut MOUNTINFO: [u8; MOUNTINFO_CAP] = [0; MOUNTINFO_CAP];
static mut OUTPUT: [u8; 256 * 1024] = [0; 256 * 1024];

/// A bump writer over a fixed buffer — the hand-rolled serializer's only state.
struct Out {
    buf: &'static mut [u8],
    len: usize,
}

impl Out {
    fn raw(&mut self, s: &[u8]) {
        let end = if self.len + s.len() > self.buf.len() {
            self.buf.len()
        } else {
            self.len + s.len()
        };
        let n = end - self.len;
        self.buf[self.len..end].copy_from_slice(&s[..n]);
        self.len = end;
    }

    /// JSON string escaping, per RFC 8259.
    fn string(&mut self, s: &str) {
        self.raw(b"\"");
        for &c in s.as_bytes() {
            match c {
                b'"' => self.raw(b"\\\""),
                b'\\' => self.raw(b"\\\\"),
                0x08 => self.raw(b"\\b"),
                0x0c => self.raw(b"\\f"),
                b'\n' => self.raw(b"\\n"),
                b'\r' => self.raw(b"\\r"),
                b'\t' => self.raw(b"\\t"),
                0x00..=0x1f => {
                    let hex = b"0123456789abcdef";
                    self.raw(b"\\u00");
                    self.raw(&[hex[(c >> 4) as usize], hex[(c & 0xf) as usize]]);
                }
                _ => self.raw(&[c]),
            }
        }
        self.raw(b"\"");
    }

    fn u64(&mut self, mut v: u64) {
        let mut d = [0u8; 20];
        let mut i = d.len();
        loop {
            i -= 1;
            d[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.raw(&d[i..]);
    }

    fn field_u64(&mut self, name: &[u8], v: u64) {
        self.raw(b",\"");
        self.raw(name);
        self.raw(b"\":");
        self.u64(v);
    }
}

fn emit(out: &mut Out, entries: &[Raw]) {
    out.raw(b"{\"filesystems\":[");
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            out.raw(b",");
        }
        let Statfs { f_bsize, f_blocks, f_bavail, f_files, f_ffree, f_bfree, .. } = e.stat;
        out.raw(b"{\"mount_point\":");
        out.string(e.mount_point);
        out.raw(b",\"source\":");
        out.string(e.source);
        out.raw(b",\"fs_type\":");
        out.string(e.fs_type);
        out.field_u64(b"total_bytes", f_blocks * f_bsize);
        out.field_u64(b"available_bytes", f_bavail * f_bsize);
        out.field_u64(b"used_bytes", (f_blocks - f_bfree) * f_bsize);
        out.field_u64(b"total_inodes", f_files);
        out.field_u64(b"available_inodes", f_ffree);
        out.raw(b"}");
    }
    out.raw(b"]}\n");
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

    let mut out = Out { buf: unsafe { &mut *core::ptr::addr_of_mut!(OUTPUT) }, len: 0 };
    emit(&mut out, &entries[..n]);
    rt::write_all(1, &out.buf[..out.len]);
    rt::exit(0)
}
