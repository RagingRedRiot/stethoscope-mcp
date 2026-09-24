//! `cargo xtask` — builds that are more than one `cargo build` (decision 33).
//!
//! `cargo xtask dev [cargo build args…]` builds every probe for the host
//! architecture, stages them under `target/payload/<arch>/<capability>`, and
//! builds the server with `STETHOSCOPE_PAYLOAD_DIR` pointing there so its
//! `build.rs` embeds them. It needs no cross toolchain: a freestanding probe
//! links against the host triple. Extra arguments go to the server's
//! `cargo build`, so `cargo xtask dev --release` works.
//!
//! `cargo xtask release` is the same build for publishing: `--release`,
//! `--locked`, and every path on the building machine remapped out of the
//! binaries, then checked for (decision 33). It builds the host architecture
//! only; the cross-architecture matrix, the manifest and the probe archives are
//! decision 33's and 38's other half and do not exist yet.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs};

/// Capability name → probe package. The capability name is what the server
/// asks for and what goes into the filename `stethoscope-<capability>-<hash>`.
const PROBES: &[(&str, &str)] = &[
    ("storage-health", "stethoscope-storage-health"),
    ("system-health", "stethoscope-system-health"),
    ("system-info", "stethoscope-system-info"),
];

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (name, result) = match args.next().as_deref() {
        Some("dev") => ("dev", dev(args.collect())),
        Some("release") if args.next().is_none() => ("release", release()),
        _ => {
            eprintln!("usage: cargo xtask dev [cargo build args…]\n       cargo xtask release");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {name}: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Workspace {
    root: PathBuf,
    target_dir: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let target_dir =
            env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
        Workspace { root, target_dir }
    }
}

fn dev(server_args: Vec<String>) -> Result<(), String> {
    build(&Workspace::new(), &server_args, None)
}

/// A build fit to publish.
///
/// rustc records absolute source paths in panic locations: for dependencies
/// under `$CARGO_HOME`, for the standard library under `$RUSTUP_HOME` when
/// `rust-src` is installed, and for anything built from outside the workspace
/// root. Those paths contain the builder's home directory and username.
/// Measured 2026-09-18: 133 such strings in a release server, none in the probe.
/// Remapping removes them and does not change a probe's hash.
fn release() -> Result<(), String> {
    let ws = Workspace::new();
    let rules = remaps(&ws)?;
    let mut flags = inherited_rustflags();
    for (from, to) in &rules {
        flags.push(format!("--remap-path-prefix={}={to}", from.display()));
    }
    build(&ws, &["--release".into(), "--locked".into()], Some(&flags))?;

    // The remap is the mechanism; this is the check that it worked. Fail
    // closed rather than publish a binary naming the machine that built it.
    let mut outputs = vec![ws.target_dir.join("release").join("stethoscope-mcp")];
    let arch_dir = ws.target_dir.join("payload").join(env::consts::ARCH);
    outputs.extend(
        PROBES
            .iter()
            .map(|(capability, _)| arch_dir.join(capability)),
    );
    for output in &outputs {
        let bytes = fs::read(output).map_err(|e| format!("{}: {e}", output.display()))?;
        if let Some(path) = leaked_path(&bytes, &rules) {
            return Err(format!(
                "{} still contains the build path {}; not fit to publish",
                output.display(),
                path.display()
            ));
        }
    }
    println!(
        "xtask release: {} and {} probe(s) built, no build-machine paths found",
        outputs[0].display(),
        PROBES.len()
    );
    Ok(())
}

/// The first remapped-away path still present in `bytes`, if any.
fn leaked_path<'a>(bytes: &[u8], rules: &'a [(PathBuf, &str)]) -> Option<&'a Path> {
    rules.iter().map(|(from, _)| from.as_path()).find(|from| {
        let needle = from.as_os_str().as_encoded_bytes();
        bytes.windows(needle.len()).any(|w| w == needle)
    })
}

/// Path prefixes to rewrite, and what to rewrite them to. Specific before
/// general, because rustc applies the last matching rule — so `$HOME` goes
/// first and anything beneath it can override it.
fn remaps(ws: &Workspace) -> Result<Vec<(PathBuf, &'static str)>, String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| h.is_absolute() && h.as_os_str().len() > 1)
        .ok_or("HOME is unset or not an absolute path")?;
    let cargo_home = env::var_os("CARGO_HOME").map_or_else(|| home.join(".cargo"), PathBuf::from);
    let rustup_home =
        env::var_os("RUSTUP_HOME").map_or_else(|| home.join(".rustup"), PathBuf::from);
    let mut rules = vec![
        (home, "/home"),
        (cargo_home, "/cargo"),
        (rustup_home, "/rustup"),
        (ws.root.clone(), "/build"),
    ];
    if !ws.target_dir.starts_with(&ws.root) {
        rules.push((ws.target_dir.clone(), "/build/target"));
    }
    Ok(rules)
}

/// Whatever rustflags the caller already set, so a release build adds to them
/// rather than silently discarding them.
fn inherited_rustflags() -> Vec<String> {
    if let Ok(encoded) = env::var("CARGO_ENCODED_RUSTFLAGS") {
        return encoded
            .split('\x1f')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
    env::var("RUSTFLAGS")
        .map(|f| f.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Build every probe for the host, stage them, and build the server over them.
/// `rustflags`, when given, reaches every cargo invocation, passed encoded so
/// that a path containing a space survives.
fn build(
    ws: &Workspace,
    server_args: &[String],
    rustflags: Option<&[String]>,
) -> Result<(), String> {
    let triple = host_triple()?;
    let arch = triple.split('-').next().unwrap();

    let stage = ws.target_dir.join("payload");
    // Start empty, so a probe removed from PROBES cannot linger in the payload.
    let _ = fs::remove_dir_all(&stage);
    let arch_dir = stage.join(arch);
    fs::create_dir_all(&arch_dir).map_err(|e| format!("{}: {e}", arch_dir.display()))?;

    for (capability, package) in PROBES {
        let mut args = vec![
            "build",
            "--profile",
            "probe",
            "--target",
            &triple,
            "-p",
            package,
        ];
        // CI passes --locked; it must hold for the probes as well as the server.
        if server_args.iter().any(|a| a == "--locked") {
            args.push("--locked");
        }
        cargo(&ws.root, &args, None, rustflags)?;
        let built = ws.target_dir.join(&triple).join("probe").join(package);
        let staged = arch_dir.join(capability);
        fs::copy(&built, &staged).map_err(|e| format!("{}: {e}", built.display()))?;
    }

    let mut args = vec!["build", "-p", "stethoscope-mcp"];
    args.extend(server_args.iter().map(String::as_str));
    cargo(&ws.root, &args, Some(&stage), rustflags)
}

fn cargo(
    root: &Path,
    args: &[&str],
    payload: Option<&Path>,
    rustflags: Option<&[String]>,
) -> Result<(), String> {
    let mut cmd = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(root).args(args);
    if let Some(dir) = payload {
        cmd.env("STETHOSCOPE_PAYLOAD_DIR", dir);
    }
    if let Some(flags) = rustflags {
        cmd.env("CARGO_ENCODED_RUSTFLAGS", flags.join("\x1f"))
            .env_remove("RUSTFLAGS");
    }
    let status = cmd.status().map_err(|e| format!("cargo: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("cargo {} failed", args.join(" ")))
}

fn host_triple() -> Result<String, String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("rustc: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV reported no host triple".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_path_anywhere_in_the_binary_is_found() {
        let rules = vec![
            (PathBuf::from("/home/someone"), "/home"),
            (PathBuf::from("/work/repo"), "/build"),
        ];
        let clean = b"\x7fELF\0/cargo/registry/src/tokio/src/lib.rs\0/build/src/main.rs";
        assert_eq!(leaked_path(clean, &rules), None);

        let mut leaked = clean.to_vec();
        leaked.extend_from_slice(b"\0/home/someone/.cargo/registry/src/x.rs");
        assert_eq!(
            leaked_path(&leaked, &rules),
            Some(Path::new("/home/someone"))
        );

        let mut leaked = b"/work/repo/src/main.rs".to_vec();
        leaked.extend_from_slice(clean);
        assert_eq!(leaked_path(&leaked, &rules), Some(Path::new("/work/repo")));
    }
}
