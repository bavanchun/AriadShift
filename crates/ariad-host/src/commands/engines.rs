//! Engines command implementation.

use std::{ffi::OsString, path::Path};

use ariad_core::protocol::ToolAvailability;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    convert::ConvertError,
    engines::{self, IN_PROCESS_ENGINE},
};

/// Closed enumeration of engine availability statuses.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum EngineStatus {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "wrong version")]
    WrongVersion,
    #[serde(rename = "not installed")]
    NotInstalled,
}

impl EngineStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::WrongVersion => "wrong version",
            Self::NotInstalled => "not installed",
        }
    }
}

/// Row describing one engine's availability, license, and supported routes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineRow {
    pub id: String,
    pub version: String,
    pub license: String,
    pub status: EngineStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub routes: Vec<String>,
}

/// Queries all engines using `engines::availability` and capabilities registry.
pub fn engines(
    engine_program: &Path,
    caps: &ariad_core::planner::Capabilities,
    cancel: CancellationToken,
) -> Result<Vec<EngineRow>, ConvertError> {
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let default_args: [OsString; 0] = [];
    let report = engines::availability(engine_program, &default_args, cancel)
        .map_err(|_| ConvertError::Failed)?;

    let mut rows = Vec::new();

    // 1. ariad-core (native)
    let mut core_routes: Vec<String> = caps
        .edges
        .iter()
        .filter(|e| e.engine == IN_PROCESS_ENGINE)
        .map(|e| format!("{} -> {}", e.from.id(), e.to.id()))
        .collect();
    core_routes.sort_unstable();
    core_routes.dedup();
    let core_license = caps
        .edges
        .iter()
        .find(|e| e.engine == IN_PROCESS_ENGINE)
        .map(|e| e.license.clone())
        .unwrap_or_else(|| "Apache-2.0".to_owned());

    rows.push(EngineRow {
        id: IN_PROCESS_ENGINE.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        license: core_license,
        status: EngineStatus::Ready,
        note: None,
        routes: core_routes,
    });

    // 2. pandoc
    let pandoc_avail = report.engines.iter().find(|e| e.engine == "pandoc");
    let (pandoc_status, pandoc_version) = match pandoc_avail {
        Some(avail) => match &avail.tool_status {
            Some(tool) => match tool.status {
                ToolAvailability::Found => (
                    EngineStatus::Ready,
                    tool.version.clone().unwrap_or_else(|| "-".to_owned()),
                ),
                ToolAvailability::WrongVersion => (
                    EngineStatus::WrongVersion,
                    tool.version.clone().unwrap_or_else(|| "-".to_owned()),
                ),
                ToolAvailability::Missing => (EngineStatus::Missing, "-".to_owned()),
            },
            None => {
                if avail.available {
                    (EngineStatus::Ready, "-".to_owned())
                } else {
                    (EngineStatus::Missing, "-".to_owned())
                }
            }
        },
        None => (EngineStatus::Missing, "-".to_owned()),
    };

    let mut pandoc_routes: Vec<String> = caps
        .edges
        .iter()
        .filter(|e| e.engine == "pandoc")
        .map(|e| format!("{} -> {}", e.from.id(), e.to.id()))
        .collect();
    pandoc_routes.sort_unstable();
    pandoc_routes.dedup();
    let pandoc_license = caps
        .edges
        .iter()
        .find(|e| e.engine == "pandoc")
        .map(|e| e.license.clone())
        .unwrap_or_else(|| "GPL-2.0-or-later".to_owned());

    let pandoc_note = match pandoc_status {
        EngineStatus::WrongVersion => Some("supported: >=3.12, <4".to_owned()),
        _ => None,
    };

    rows.push(EngineRow {
        id: "pandoc".to_owned(),
        version: pandoc_version,
        license: pandoc_license,
        status: pandoc_status,
        note: pandoc_note,
        routes: pandoc_routes,
    });

    // 3. docling
    let mut docling_routes: Vec<String> = caps
        .edges
        .iter()
        .filter(|e| e.engine == "docling")
        .map(|e| format!("{} -> {}", e.from.id(), e.to.id()))
        .collect();
    docling_routes.sort_unstable();
    docling_routes.dedup();
    let docling_license = caps
        .edges
        .iter()
        .find(|e| e.engine == "docling")
        .map(|e| e.license.clone())
        .unwrap_or_else(|| "Apache-2.0".to_owned());

    rows.push(EngineRow {
        id: "docling".to_owned(),
        version: "-".to_owned(),
        license: docling_license,
        status: EngineStatus::NotInstalled,
        note: Some("available in v0.2".to_owned()),
        routes: docling_routes,
    });

    Ok(rows)
}
