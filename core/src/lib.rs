//! `stethoscope-core` — what the probes and the server share.
//!
//! Decision 35 makes this crate `no_std` + `alloc` and puts collection, the
//! response types and the read guard in it. **Today it holds only the response
//! types**, one module per probe, so that the probe that writes a wire
//! format and the server that reads it cannot disagree about it —
//! the drift decision 28 exists to prevent. Collection and the guard have not
//! moved yet.
//!
//! Decision 35's amendment requires the server to be able to link the response
//! types *without* linking collection. Whether that ends up a feature flag or a
//! second crate is still undecided; while this crate holds nothing but types,
//! the question does not have to be answered.

#![no_std]

extern crate alloc;

pub mod storage;
pub mod system_info;
