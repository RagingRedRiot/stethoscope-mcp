//! Freestanding link flags for this binary alone (decision 33).
//!
//! `rustc-link-arg-bins` applies to this package's binaries and nothing else,
//! which is what the nested `.cargo/config.toml` this replaces could not
//! promise: flags set there reached build scripts and proc macros too — host
//! binaries, which then had no entry point and crashed when cargo ran them —
//! and they were only read at all when cargo was invoked from this directory.
fn main() {
    for arg in [
        "-nostartfiles",
        "-static",
        "-Wl,--build-id=none",
        "-Wl,--gc-sections",
    ] {
        println!("cargo::rustc-link-arg-bins={arg}");
    }
}
