//! The probe prologue: everything between "this tool needs the storage probe"
//! and a running process whose bytes are known to be ours.
//!
//! Every tool that collects runs the same sequence (decision 37):
//!
//! 1. **Read chain** — `/opt/stethoscope`, then `$HOME/.stethoscope`. The first
//!    location holding a payload that passes [`judge`] wins (decision 38).
//! 2. **Write chain** — only `$HOME/.stethoscope`. If the read chain found
//!    nothing usable *and* home was merely empty rather than refused, the
//!    embedded probe is placed there and then judged again from scratch, so a
//!    file we just wrote passes exactly the checks a found one does.
//! 3. **Exec** — the exec attempt is the `noexec` test (decision 38). A payload
//!    in `/opt` that will not execute falls through to home.
//!
//! `/tmp` is in neither chain (decision 40). `/opt/stethoscope` is never
//! created, written or tested for writability (decision 38).
//!
//! **The facts are gathered separately from the decision.** [`judge`] is a pure
//! function over [`Facts`], so the rules are unit-tested below without a
//! filesystem, and a remote target will produce the same `Facts` from one
//! discovery round trip (decision 31) rather than from syscalls. Gathering,
//! placing and spawning are the parts that change with the transport; the
//! rules are not.
//!
//! What reaches the model is a location *class* and a reason category, never a
//! path — `$HOME` contains the username (decision 9). Paths and detail go to
//! stderr.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rmcp::ErrorData;
use sha2::{Digest, Sha256};

use crate::payload::{self, Payload};

/// Where an operator installs probes. Read-only to this server, always.
const INSTALLED_DIR: &str = "/opt/stethoscope";
/// The server's own cache, beneath `$HOME`.
const HOME_DIR: &str = ".stethoscope";
/// A probe that has not finished by now is killed. A `statfs` blocked on a dead
/// NFS server is the case this exists for (decision 37).
const DEADLINE: Duration = Duration::from_secs(10);
/// Nothing larger than this is hashed. Probes are tens of kilobytes.
const MAX_PROBE_BYTES: u64 = 8 << 20;

/// Which link of the chain a payload came from. Reported to the model per
/// payload (decision 38).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Location {
    /// `/opt/stethoscope`, put there by an operator.
    Installed,
    /// `$HOME/.stethoscope`, placed by this server.
    Home,
}

impl Location {
    pub fn as_str(self) -> &'static str {
        match self {
            Location::Installed => "installed",
            Location::Home => "home",
        }
    }
}

/// Why a location did not yield a runnable probe. A category, never a
/// diagnostic (decision 9); the detail goes to stderr.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    /// Nothing there. Not a refusal — the ordinary state of `/opt`.
    Absent,
    /// `$HOME` is unset or not an absolute path.
    NoHome,
    /// The directory holding the probe directory can be written by someone
    /// other than root or us, who could therefore rename it away and put their
    /// own in its place. `sshd`'s `StrictModes` applies the same rule to
    /// `~/.ssh`'s parent.
    UnsafeParent,
    /// The probe directory is a symlink or some other non-directory. Refused
    /// outright rather than reasoned about, because whoever created a symlink
    /// controls where it resolves (decision 38's recorded symlink gap).
    NotADirectory,
    /// The directory or file is owned by someone who is neither us nor, for
    /// `/opt`, root (decisions 32 and 38).
    UnsafeOwner,
    /// The directory or file is writable by group or world.
    UnsafeMode,
    /// The probe's name is taken by something that is not a regular file.
    NotARegularFile,
    /// The contents do not hash to the name. Left in place: deleting it would
    /// destroy the only evidence of whoever put it there (decision 32).
    HashMismatch,
    /// Setuid, setgid or file capabilities on a probe outside `/opt`. This
    /// server never places such a file, so one is suspicious (decision 39).
    ElevatedOutsideInstalled,
    /// An elevated probe in `/opt` that is not root-owned, or is writable by
    /// group or world, or executable by world (decision 39) — a grant is for
    /// the users the operator chose, through the group bits.
    ElevatedUnsafe,
    /// Found and verified, but the kernel refused to execute it — a `noexec`
    /// mount or a missing exec bit.
    NotExecutable,
    /// Placement failed for lack of space or quota.
    NoSpace,
    /// Placement failed because we may not write there.
    NotWritable,
    /// Anything else the filesystem said.
    IoError,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Absent => "absent",
            Reason::NoHome => "no_home",
            Reason::UnsafeParent => "unsafe_parent",
            Reason::NotADirectory => "not_a_directory",
            Reason::UnsafeOwner => "unsafe_owner",
            Reason::UnsafeMode => "unsafe_mode",
            Reason::NotARegularFile => "not_a_regular_file",
            Reason::HashMismatch => "hash_mismatch",
            Reason::ElevatedOutsideInstalled => "elevated_outside_installed",
            Reason::ElevatedUnsafe => "elevated_unsafe",
            Reason::NotExecutable => "not_executable",
            Reason::NoSpace => "no_space",
            Reason::NotWritable => "not_writable",
            Reason::IoError => "io_error",
        }
    }
}

// ---------------------------------------------------------------------------
// Facts and the rules
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dir,
    File,
    Symlink,
    Other,
}

/// What `stat` said about one path, reduced to what the rules read.
#[derive(Clone, Copy, Debug)]
pub struct Meta {
    pub kind: Kind,
    pub uid: u32,
    /// Permission bits including setuid, setgid and sticky: `st_mode & 0o7777`.
    pub mode: u32,
}

#[derive(Debug)]
pub struct FileFacts {
    pub meta: Meta,
    /// A `security.capability` extended attribute is present. The hash cannot
    /// see it — `setcap` leaves the contents byte-identical — so this is the
    /// only thing that tells a verified probe from a verified *and elevated*
    /// one.
    pub capability_xattr: bool,
    /// Lowercase hex SHA-256 of the contents; `None` if it is not a regular
    /// file or is implausibly large.
    pub sha256: Option<String>,
}

/// Everything the rules need about one location. `None` means absent.
#[derive(Debug)]
pub struct Facts {
    /// The directory the probe directory sits in: `/opt`, or `$HOME`.
    pub parent: Option<Meta>,
    /// The probe directory itself, not following a symlink.
    pub dir: Option<Meta>,
    /// The file named for the probe this build carries.
    pub file: Option<FileFacts>,
    /// The directory could not be searched, so nothing is known about the
    /// file. Recorded rather than raised, so that the directory's own
    /// ownership decides the reason when it is someone else's.
    pub file_unknowable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finding {
    /// Safe to execute. `elevated` when setuid, setgid or file capabilities are
    /// present, which the rules admit only in `/opt`.
    Usable { elevated: bool },
    /// The location is safe and holds no probe.
    Absent,
    /// The location may not be used, for this reason.
    Refused(Reason),
}

/// The rules, in one place. Pure: no filesystem, no clock.
///
/// A directory is safe to execute from when it is owned by root or by us and
/// writable by neither group nor world, and safe to write to when it is owned
/// by us under the same condition (decision 38). Home is only ever the second
/// kind, so root ownership is admitted only for `/opt`. The same owner rule is
/// applied to the file, because a file owned by a third user in a safe
/// directory is still theirs to rewrite between our hash and our exec.
pub fn judge(location: Location, uid: u32, facts: &Facts, sha256: &str) -> Finding {
    use Finding::{Absent, Refused, Usable};
    let installed = location == Location::Installed;
    let others_can_write = |m: &Meta| m.mode & 0o022 != 0;
    let owner_ok = |m: &Meta| m.uid == uid || (installed && m.uid == 0);

    let Some(parent) = &facts.parent else {
        return Absent;
    };
    let parent_ok = parent.kind == Kind::Dir
        && (parent.uid == uid || parent.uid == 0)
        && !others_can_write(parent);

    // The parent matters only for a directory we will use, or create. A
    // missing `/opt/stethoscope` is simply absent whatever `/opt` looks like —
    // refusing it would report `unsafe_parent` on every call from any host,
    // container or user namespace with an unusual `/opt`, for a location that
    // holds nothing. A missing `~/.stethoscope` is about to be created, so for
    // home the parent is checked first.
    let Some(dir) = &facts.dir else {
        return if installed || parent_ok {
            Absent
        } else {
            Refused(Reason::UnsafeParent)
        };
    };
    if !parent_ok {
        return Refused(Reason::UnsafeParent);
    }
    if dir.kind != Kind::Dir {
        return Refused(Reason::NotADirectory);
    }
    if !owner_ok(dir) {
        return Refused(Reason::UnsafeOwner);
    }
    if others_can_write(dir) {
        return Refused(Reason::UnsafeMode);
    }

    if facts.file_unknowable {
        return Refused(Reason::IoError);
    }
    let Some(file) = &facts.file else {
        return Absent;
    };
    if file.meta.kind != Kind::File {
        return Refused(Reason::NotARegularFile);
    }
    if !owner_ok(&file.meta) {
        return Refused(Reason::UnsafeOwner);
    }
    if others_can_write(&file.meta) {
        return Refused(Reason::UnsafeMode);
    }
    if file.sha256.as_deref() != Some(sha256) {
        return Refused(Reason::HashMismatch);
    }

    let elevated = file.meta.mode & 0o6000 != 0 || file.capability_xattr;
    if elevated {
        if !installed {
            return Refused(Reason::ElevatedOutsideInstalled);
        }
        if file.meta.uid != 0 || file.meta.mode & 0o001 != 0 {
            return Refused(Reason::ElevatedUnsafe);
        }
    }
    Usable { elevated }
}

// ---------------------------------------------------------------------------
// Gathering facts from the local filesystem
// ---------------------------------------------------------------------------

fn meta(m: &fs::Metadata) -> Meta {
    let ft = m.file_type();
    let kind = if ft.is_symlink() {
        Kind::Symlink
    } else if ft.is_dir() {
        Kind::Dir
    } else if ft.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    Meta {
        kind,
        uid: m.uid(),
        mode: m.mode() & 0o7777,
    }
}

fn absent_ok<T>(r: io::Result<T>) -> io::Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// `parent` is followed — it is `/opt` or `$HOME`, which may legitimately be
/// reached through a symlink. The probe directory and the file are not.
fn gather(parent: &Path, dir: &Path, name: &str) -> io::Result<Facts> {
    let parent = absent_ok(fs::metadata(parent))?.map(|m| meta(&m));
    let dir_meta = absent_ok(fs::symlink_metadata(dir))?.map(|m| meta(&m));
    let (file, file_unknowable) = match dir_meta {
        Some(Meta {
            kind: Kind::Dir, ..
        }) => match file_facts(&dir.join(name)) {
            Ok(f) => (f, false),
            Err(e) if permission_error(&e) => (None, true),
            Err(e) => return Err(e),
        },
        _ => (None, false),
    };
    Ok(Facts {
        parent,
        dir: dir_meta,
        file,
        file_unknowable,
    })
}

fn file_facts(path: &Path) -> io::Result<Option<FileFacts>> {
    let Some(lmeta) = absent_ok(fs::symlink_metadata(path))?.map(|m| meta(&m)) else {
        return Ok(None);
    };
    if lmeta.kind != Kind::File {
        return Ok(Some(FileFacts {
            meta: lmeta,
            capability_xattr: false,
            sha256: None,
        }));
    }
    // Everything below comes from the open descriptor, not the path, so the
    // ownership, mode, xattr and hash all describe one file. O_NOFOLLOW refuses
    // a symlink swapped in since the lstat; O_NONBLOCK keeps a FIFO swapped in
    // from blocking the open, and fstat then reports it as not a file.
    let mut f = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        // A file we may not read is someone else's: report what lstat saw, with
        // no digest, and let the owner rule give the reason.
        Err(e) if permission_error(&e) => {
            return Ok(Some(FileFacts {
                meta: lmeta,
                capability_xattr: false,
                sha256: None,
            }));
        }
        Err(e) => return Err(e),
    };
    let fmeta = meta(&f.metadata()?);
    if fmeta.kind != Kind::File {
        return Ok(Some(FileFacts {
            meta: fmeta,
            capability_xattr: false,
            sha256: None,
        }));
    }
    let capability_xattr = has_capability_xattr(&f)?;
    let mut bytes = Vec::new();
    (&mut f).take(MAX_PROBE_BYTES + 1).read_to_end(&mut bytes)?;
    let sha256 = (bytes.len() as u64 <= MAX_PROBE_BYTES).then(|| hex(&Sha256::digest(&bytes)));
    Ok(Some(FileFacts {
        meta: fmeta,
        capability_xattr,
        sha256,
    }))
}

fn permission_error(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::EACCES | libc::EPERM))
}

fn has_capability_xattr(f: &File) -> io::Result<bool> {
    // SAFETY: a size query — null buffer, zero length — on a descriptor we own.
    let r = unsafe {
        libc::fgetxattr(
            f.as_raw_fd(),
            c"security.capability".as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    if r >= 0 {
        return Ok(true);
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        // No such attribute, or a filesystem with no xattrs and so no way to
        // carry capabilities.
        Some(libc::ENODATA | libc::ENOTSUP) => Ok(false),
        _ => Err(e),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Other versions of this probe sitting beside the one we looked for. Nothing
/// deletes them — garbage collection waits on open question 14 — but an
/// operator should hear about them, and a stale `/opt` install must be loud
/// (decision 38): it *works*, by quietly falling through to home.
fn note_strays(location: Location, dir: &Path, payload: &Payload, found: bool) {
    let prefix = format!("stethoscope-{}-", payload.capability);
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let strays = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with(&prefix) && !payload::is_ours(n))
        .count();
    if strays == 0 {
        return;
    }
    if location == Location::Installed && !found {
        eprintln!(
            "stethoscope-mcp: WARNING: {} holds {strays} {} probe(s) from another release and none from this one; \
             collection is falling back to $HOME/{HOME_DIR}. Reinstall from this release to use the install.",
            dir.display(),
            payload.capability,
        );
    } else {
        eprintln!(
            "stethoscope-mcp: {}: {strays} other {} probe version(s) present, left in place",
            dir.display(),
            payload.capability,
        );
    }
}

// ---------------------------------------------------------------------------
// The chain
// ---------------------------------------------------------------------------

/// A probe that passed every rule, and where it was.
pub struct Found {
    pub path: PathBuf,
    pub location: Location,
    pub elevated: bool,
}

fn inspect(location: Location, parent: &Path, dir: &Path, payload: &Payload, uid: u32) -> Finding {
    let name = payload.file_name();
    let finding = match gather(parent, dir, &name) {
        Ok(facts) => {
            let finding = judge(location, uid, &facts, payload.sha256);
            let dir_is_safe = matches!(finding, Finding::Usable { .. })
                || (finding == Finding::Absent && facts.dir.is_some());
            if dir_is_safe {
                note_strays(location, dir, payload, finding != Finding::Absent);
            }
            finding
        }
        Err(e) => {
            eprintln!("stethoscope-mcp: {}: cannot inspect: {e}", dir.display());
            Finding::Refused(Reason::IoError)
        }
    };
    if let Finding::Refused(reason) = finding {
        let loud = match reason {
            Reason::HashMismatch | Reason::ElevatedOutsideInstalled => {
                " — left in place; investigate"
            }
            _ => "",
        };
        eprintln!(
            "stethoscope-mcp: {}: refused: {}{loud}",
            dir.join(&name).display(),
            reason.as_str()
        );
    }
    finding
}

/// Walk the read chain, placing into home if nothing usable was found there.
///
/// `skip_installed` is set when `/opt` yielded a probe the kernel then refused
/// to execute.
fn resolve(payload: &Payload, skip_installed: bool) -> Result<Found, Vec<(Location, Reason)>> {
    // SAFETY: geteuid cannot fail and touches no memory.
    let uid = unsafe { libc::geteuid() };
    let mut denied = Vec::new();

    if !skip_installed {
        let dir = Path::new(INSTALLED_DIR);
        match inspect(Location::Installed, Path::new("/opt"), dir, payload, uid) {
            Finding::Usable { elevated } => {
                return Ok(Found {
                    path: dir.join(payload.file_name()),
                    location: Location::Installed,
                    elevated,
                });
            }
            Finding::Absent => denied.push((Location::Installed, Reason::Absent)),
            Finding::Refused(r) => denied.push((Location::Installed, r)),
        }
    }

    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| h.is_absolute())
    else {
        denied.push((Location::Home, Reason::NoHome));
        return Err(denied);
    };
    let dir = home.join(HOME_DIR);
    let mut finding = inspect(Location::Home, &home, &dir, payload, uid);
    if finding == Finding::Absent {
        finding = match place(&dir, payload) {
            Ok(()) => match inspect(Location::Home, &home, &dir, payload, uid) {
                // We wrote it and it is not there: something raced us.
                Finding::Absent => Finding::Refused(Reason::IoError),
                f => f,
            },
            Err(e) => {
                eprintln!(
                    "stethoscope-mcp: {}: cannot place probe: {e}",
                    dir.display()
                );
                Finding::Refused(placement_reason(&e))
            }
        };
    }
    match finding {
        Finding::Usable { elevated } => Ok(Found {
            path: dir.join(payload.file_name()),
            location: Location::Home,
            elevated,
        }),
        Finding::Refused(r) => {
            denied.push((Location::Home, r));
            Err(denied)
        }
        Finding::Absent => unreachable!("placement replaces Absent"),
    }
}

/// Write the embedded probe into home.
///
/// The directory is created mode 700 and the file written mode 700, both set
/// explicitly afterwards because the umask can only remove bits — and on a
/// host with an unusual umask would otherwise leave either unusable, or, the
/// other way round, looser than decision 38 permits. The file is written under
/// a temporary name and renamed into place, so a concurrent session sees either
/// no probe or a complete one; two sessions placing at once write identical
/// bytes to one name, which is benign (decision 29).
fn place(dir: &Path, payload: &Payload) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?,
        // Created by a concurrent session, or already there; judged again below either way.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let name = payload.file_name();
    let tmp = dir.join(format!("{name}.tmp-{}", std::process::id()));
    let written = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)?;
        f.set_permissions(fs::Permissions::from_mode(0o700))?;
        f.write_all(payload.bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, dir.join(&name))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn placement_reason(e: &io::Error) -> Reason {
    match e.raw_os_error() {
        Some(libc::ENOSPC | libc::EDQUOT) => Reason::NoSpace,
        Some(libc::EACCES | libc::EPERM | libc::EROFS) => Reason::NotWritable,
        _ => Reason::IoError,
    }
}

// ---------------------------------------------------------------------------
// Running it
// ---------------------------------------------------------------------------

/// What a probe printed, and where it ran from.
pub struct Output {
    pub stdout: Vec<u8>,
    pub location: Location,
}

/// Resolve, place if needed, execute and collect the probe for `capability`.
///
/// The probe gets no arguments, no environment and `/` as its working
/// directory: decision 39 makes "probes read neither `argv` nor `environ`" a
/// standing constraint, and there is nothing to pass them anyway.
pub async fn run(capability: &'static str) -> Result<Output, ErrorData> {
    let Some(payload) = payload::for_host(capability) else {
        return Err(failure(capability, "payload_unavailable", &[]));
    };
    let mut skip_installed = false;
    let mut prior = Vec::new();
    loop {
        let found = tokio::task::spawn_blocking(move || resolve(payload, skip_installed))
            .await
            .map_err(|e| ErrorData::internal_error(format!("{capability} probe: {e}"), None))?
            .map_err(|denied| {
                prior.extend(denied);
                failure(capability, "no_usable_probe", &prior)
            })?;
        if found.elevated {
            eprintln!(
                "stethoscope-mcp: {}: running operator-elevated probe",
                found.path.display()
            );
        }
        let spawned = tokio::process::Command::new(&found.path)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let child = match spawned {
            Ok(child) => child,
            Err(e) => {
                eprintln!(
                    "stethoscope-mcp: {}: exec failed: {e}",
                    found.path.display()
                );
                let noexec = matches!(
                    e.raw_os_error(),
                    Some(libc::EACCES | libc::EPERM | libc::ENOEXEC)
                );
                let reason = if noexec {
                    Reason::NotExecutable
                } else {
                    Reason::IoError
                };
                prior.push((found.location, reason));
                if noexec && found.location == Location::Installed {
                    skip_installed = true;
                    continue;
                }
                return Err(failure(capability, "no_usable_probe", &prior));
            }
        };

        // Dropping the future on timeout drops the child, which kill_on_drop kills.
        let out = match tokio::time::timeout(DEADLINE, child.wait_with_output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                eprintln!("stethoscope-mcp: {}: {e}", found.path.display());
                return Err(failure(capability, "probe_failed", &[]));
            }
            Err(_) => return Err(failure(capability, "timed_out", &[])),
        };
        if !out.stderr.is_empty() {
            eprintln!(
                "stethoscope-mcp: {} stderr: {}",
                found.path.display(),
                String::from_utf8_lossy(&out.stderr).trim_end()
            );
        }
        if !out.status.success() {
            eprintln!("stethoscope-mcp: {}: {}", found.path.display(), out.status);
            return Err(failure(capability, "probe_failed", &[]));
        }
        return Ok(Output {
            stdout: out.stdout,
            location: found.location,
        });
    }
}

/// The model-facing failure: a category for the call and one per location,
/// with no paths (decision 9). Whether this belongs on the error channel at
/// all is open question 2.
pub fn failure(capability: &str, category: &str, reasons: &[(Location, Reason)]) -> ErrorData {
    let detail: Vec<String> = reasons
        .iter()
        .map(|(l, r)| format!("{}: {}", l.as_str(), r.as_str()))
        .collect();
    let message = if detail.is_empty() {
        format!("{capability} probe on local: {category}")
    } else {
        format!(
            "{capability} probe on local: {category} ({})",
            detail.join(", ")
        )
    };
    let locations: serde_json::Map<String, serde_json::Value> = reasons
        .iter()
        .map(|(l, r)| (l.as_str().into(), r.as_str().into()))
        .collect();
    ErrorData::internal_error(
        message,
        Some(serde_json::json!({ "error": category, "locations": locations })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 1000;
    const OTHER: u32 = 1001;
    const HASH: &str = "abc123";

    fn dir(uid: u32, mode: u32) -> Option<Meta> {
        Some(Meta {
            kind: Kind::Dir,
            uid,
            mode,
        })
    }

    fn file(uid: u32, mode: u32, sha256: &str, xattr: bool) -> Option<FileFacts> {
        Some(FileFacts {
            meta: Meta {
                kind: Kind::File,
                uid,
                mode,
            },
            capability_xattr: xattr,
            sha256: Some(sha256.into()),
        })
    }

    fn home(parent: Option<Meta>, d: Option<Meta>, f: Option<FileFacts>) -> Finding {
        judge(
            Location::Home,
            ME,
            &Facts {
                parent,
                dir: d,
                file: f,
                file_unknowable: false,
            },
            HASH,
        )
    }

    fn installed(d: Option<Meta>, f: Option<FileFacts>) -> Finding {
        judge(
            Location::Installed,
            ME,
            &Facts {
                parent: dir(0, 0o755),
                dir: d,
                file: f,
                file_unknowable: false,
            },
            HASH,
        )
    }

    fn refused(r: Reason) -> Finding {
        Finding::Refused(r)
    }

    #[test]
    fn the_ordinary_cases() {
        let ok = Finding::Usable { elevated: false };
        assert_eq!(
            home(dir(ME, 0o755), dir(ME, 0o700), file(ME, 0o700, HASH, false)),
            ok
        );
        assert_eq!(installed(dir(0, 0o755), file(0, 0o755, HASH, false)), ok);
        // Nothing there is not a refusal: /opt is usually absent, home is placed into.
        assert_eq!(installed(None, None), Finding::Absent);
        assert_eq!(home(dir(ME, 0o700), None, None), Finding::Absent);
        assert_eq!(home(dir(ME, 0o700), dir(ME, 0o700), None), Finding::Absent);
    }

    #[test]
    fn a_symlink_is_not_a_directory_whatever_it_points_at() {
        let link = Some(Meta {
            kind: Kind::Symlink,
            uid: ME,
            mode: 0o777,
        });
        assert_eq!(
            home(dir(ME, 0o700), link, None),
            refused(Reason::NotADirectory)
        );
        let link = Some(FileFacts {
            meta: Meta {
                kind: Kind::Symlink,
                uid: ME,
                mode: 0o777,
            },
            capability_xattr: false,
            sha256: None,
        });
        assert_eq!(
            home(dir(ME, 0o700), dir(ME, 0o700), link),
            refused(Reason::NotARegularFile)
        );
    }

    #[test]
    fn a_third_users_directory_is_refused_even_when_not_group_or_world_writable() {
        // Decision 38: mode 755 owned by another unprivileged user is still theirs to fill.
        assert_eq!(
            installed(dir(OTHER, 0o755), file(0, 0o755, HASH, false)),
            refused(Reason::UnsafeOwner)
        );
        assert_eq!(
            home(dir(ME, 0o700), dir(OTHER, 0o700), None),
            refused(Reason::UnsafeOwner)
        );
    }

    #[test]
    fn root_ownership_is_admitted_only_for_opt() {
        assert_eq!(
            home(dir(ME, 0o700), dir(0, 0o755), None),
            refused(Reason::UnsafeOwner)
        );
        assert_eq!(installed(dir(0, 0o755), None), Finding::Absent);
    }

    #[test]
    fn group_or_world_writable_is_refused_at_every_level() {
        assert_eq!(
            home(dir(ME, 0o775), dir(ME, 0o700), None),
            refused(Reason::UnsafeParent)
        );
        assert_eq!(
            home(dir(ME, 0o700), dir(ME, 0o770), None),
            refused(Reason::UnsafeMode)
        );
        assert_eq!(
            home(dir(ME, 0o700), dir(ME, 0o700), file(ME, 0o702, HASH, false)),
            refused(Reason::UnsafeMode)
        );
        assert_eq!(installed(dir(0, 0o775), None), refused(Reason::UnsafeMode));
    }

    #[test]
    fn an_unusual_opt_does_not_matter_when_nothing_is_installed() {
        let nobody = Some(Meta {
            kind: Kind::Dir,
            uid: 65534,
            mode: 0o755,
        });
        let facts = Facts {
            parent: nobody,
            dir: None,
            file: None,
            file_unknowable: false,
        };
        assert_eq!(
            judge(Location::Installed, ME, &facts, HASH),
            Finding::Absent
        );
        // ...but it does when something is.
        let facts = Facts {
            parent: nobody,
            dir: dir(0, 0o755),
            file: file(0, 0o755, HASH, false),
            file_unknowable: false,
        };
        assert_eq!(
            judge(Location::Installed, ME, &facts, HASH),
            refused(Reason::UnsafeParent)
        );
        // Home is about to be created in its parent, so the parent is checked first.
        assert_eq!(
            home(dir(ME, 0o775), None, None),
            refused(Reason::UnsafeParent)
        );
    }

    #[test]
    fn a_third_users_unreadable_directory_or_file_is_refused_for_ownership_not_io() {
        // Found in CI's sudo tier: mode 700 and owned by someone else, so the
        // file inside cannot even be looked at. Ownership must still decide.
        let facts = Facts {
            parent: dir(ME, 0o700),
            dir: dir(OTHER, 0o700),
            file: None,
            file_unknowable: true,
        };
        assert_eq!(
            judge(Location::Home, ME, &facts, HASH),
            refused(Reason::UnsafeOwner)
        );
        let unreadable = Some(FileFacts {
            meta: Meta {
                kind: Kind::File,
                uid: OTHER,
                mode: 0o700,
            },
            capability_xattr: false,
            sha256: None,
        });
        assert_eq!(
            home(dir(ME, 0o700), dir(ME, 0o700), unreadable),
            refused(Reason::UnsafeOwner)
        );
        // Our own directory we cannot search is still a refusal, just not an ownership one.
        let facts = Facts {
            parent: dir(ME, 0o700),
            dir: dir(ME, 0o000),
            file: None,
            file_unknowable: true,
        };
        assert_eq!(
            judge(Location::Home, ME, &facts, HASH),
            refused(Reason::IoError)
        );
    }

    #[test]
    fn a_parent_owned_by_a_third_user_is_refused() {
        assert_eq!(
            home(dir(OTHER, 0o755), dir(ME, 0o700), None),
            refused(Reason::UnsafeParent)
        );
    }

    #[test]
    fn a_file_owned_by_a_third_user_is_refused_even_with_the_right_hash() {
        assert_eq!(
            installed(dir(0, 0o755), file(OTHER, 0o755, HASH, false)),
            refused(Reason::UnsafeOwner)
        );
    }

    #[test]
    fn a_name_is_never_trusted() {
        assert_eq!(
            home(
                dir(ME, 0o700),
                dir(ME, 0o700),
                file(ME, 0o700, "planted", false)
            ),
            refused(Reason::HashMismatch)
        );
        let unhashable = Some(FileFacts {
            meta: Meta {
                kind: Kind::File,
                uid: ME,
                mode: 0o700,
            },
            capability_xattr: false,
            sha256: None,
        });
        assert_eq!(
            home(dir(ME, 0o700), dir(ME, 0o700), unhashable),
            refused(Reason::HashMismatch)
        );
    }

    #[test]
    fn elevation_is_refused_outside_opt_even_when_the_hash_matches() {
        // The hash cannot see setuid or an xattr; this is the only check that does.
        assert_eq!(
            home(
                dir(ME, 0o700),
                dir(ME, 0o700),
                file(ME, 0o4700, HASH, false)
            ),
            refused(Reason::ElevatedOutsideInstalled)
        );
        assert_eq!(
            home(dir(ME, 0o700), dir(ME, 0o700), file(ME, 0o700, HASH, true)),
            refused(Reason::ElevatedOutsideInstalled)
        );
    }

    #[test]
    fn elevation_in_opt_needs_root_ownership_and_no_world_access() {
        assert_eq!(
            installed(dir(0, 0o755), file(0, 0o750, HASH, true)),
            Finding::Usable { elevated: true }
        );
        assert_eq!(
            installed(dir(0, 0o755), file(0, 0o4750, HASH, false)),
            Finding::Usable { elevated: true }
        );
        assert_eq!(
            installed(dir(0, 0o755), file(0, 0o755, HASH, true)),
            refused(Reason::ElevatedUnsafe)
        );
        assert_eq!(
            installed(dir(0, 0o755), file(ME, 0o4750, HASH, false)),
            refused(Reason::ElevatedUnsafe)
        );
    }
}
