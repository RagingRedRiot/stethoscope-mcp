//! The `StorageHealth` shape: the storage probe's report, plus where the probe
//! ran from.
//!
//! The first tool collected by a probe rather than in-process (decision 37).
//! The server carries no storage code: it runs `stethoscope-storage` through
//! the prologue and parses the one JSON document it prints into the type the
//! probe serialized, so a probe emitting anything else is an error here rather
//! than something passed through to the model.

use rmcp::{ErrorData, schemars};
use serde::Serialize;
use stethoscope_core::storage::Report;

use crate::prologue;

/// Which probe location a payload came from (decision 38).
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeLocation {
    /// `/opt/stethoscope`, installed by an operator.
    Installed,
    /// The server's own cache in the invoking user's home directory.
    Home,
}

/// Filesystem capacity on a target machine (decision 30).
#[derive(Serialize, schemars::JsonSchema)]
pub struct StorageHealth {
    /// The target this was collected from.
    pub target: String,
    /// Where the probe that collected this ran from. An operator-installed
    /// probe may carry elevated authority (see `privileged`); one in the home
    /// cache never does.
    pub probe_location: ProbeLocation,
    #[serde(flatten)]
    pub report: Report,
}

pub async fn collect(target: String) -> Result<StorageHealth, ErrorData> {
    let out = prologue::run("storage").await?;
    let report: Report = serde_json::from_slice(&out.stdout).map_err(|e| {
        eprintln!("stethoscope-mcp: storage probe emitted unparseable output: {e}");
        prologue::failure("storage", "malformed_output", &[])
    })?;
    let probe_location = match out.location {
        prologue::Location::Installed => ProbeLocation::Installed,
        prologue::Location::Home => ProbeLocation::Home,
    };
    Ok(StorageHealth {
        target,
        probe_location,
        report,
    })
}
