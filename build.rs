//! Embeds the probes `cargo xtask dev` staged, with their digests (decision 33).
//!
//! Reads `$STETHOSCOPE_PAYLOAD_DIR/<arch>/<capability>`, copies each file into
//! `OUT_DIR` under its content-addressed name, and generates `payload.rs`
//! listing them. The digest is computed over the copy that `include_bytes!`
//! embeds, so the bytes and the hash cannot drift apart.
//!
//! Without the variable — a plain `cargo build` — the payload set is empty and
//! the server says so on stderr at startup.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::{env, fs};

use sha2::{Digest, Sha256};

fn main() {
    println!("cargo::rerun-if-env-changed=STETHOSCOPE_PAYLOAD_DIR");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut entries = String::new();

    if let Some(dir) = env::var_os("STETHOSCOPE_PAYLOAD_DIR") {
        let dir = PathBuf::from(dir);
        println!("cargo::rerun-if-changed={}", dir.display());
        for (arch, file) in staged(&dir) {
            let capability = file.file_name().unwrap().to_str().unwrap().to_owned();
            let bytes = fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let sha256: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let embedded = out.join(format!("{arch}-stethoscope-{capability}-{sha256}"));
            fs::write(&embedded, &bytes).unwrap();
            writeln!(
                entries,
                "    Payload {{ capability: {capability:?}, arch: {arch:?}, sha256: {sha256:?}, \
                 bytes: include_bytes!({:?}) }},",
                embedded.display().to_string(),
            )
            .unwrap();
        }
    }

    fs::write(
        out.join("payload.rs"),
        format!("pub static PAYLOADS: &[Payload] = &[\n{entries}];\n"),
    )
    .unwrap();
}

/// `(arch, file)` for every staged probe, sorted so the generated file is
/// stable.
fn staged(dir: &PathBuf) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    for arch in fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let arch = arch.unwrap().path();
        let name = arch.file_name().unwrap().to_str().unwrap().to_owned();
        for file in fs::read_dir(&arch).unwrap() {
            found.push((name.clone(), file.unwrap().path()));
        }
    }
    found.sort();
    found
}
