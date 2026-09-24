//! `stethoscope-core` — what the probes and the server share.
//!
//! Decision 35 makes this crate `no_std` + `alloc` and puts collection, the
//! response types and the read guard in it. It now holds the response types
//! (one module per probe), the read guard, the parsers for the kernel formats
//! the probes read, and RFC 3339 formatting. Probe and server link the same
//! code, which is what stops the two from disagreeing about a wire format or
//! an allowlist — the drift decision 28 exists to prevent.
//!
//! Decision 35's amendment requires the server to be able to link the response
//! types *without* linking collection. Whether that ends up a feature flag or a
//! second crate is still undecided; while this crate holds nothing but types,
//! the question does not have to be answered.

#![no_std]

extern crate alloc;

pub mod guard;
pub mod proc;
pub mod storage;
pub mod system_health;
pub mod system_info;
pub mod time;
