//! Engines command implementation.

use std::{collections::BTreeSet, ffi::OsString, path::Path};

use ariad_core::protocol::{Event, ToolAvailability};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    convert::ConvertError,
    engines::{self, IN_PROCESS_ENGINE, docling},
    packs::{self, InstalledPack, PackContext},
    runner::{self, RunError},
};

/// Engine pack that backs an engine row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnginePack {
    /// Pack name.
    pub name: String,
    /// Active version.
    pub version: String,
    /// Target triple of the active build.
    pub target: String,
    /// `installed`, or `override` for an `ASHIFT_PACK_<NAME>` development directory.
    pub source: String,
    /// Every installed version, highest first.
    pub versions: Vec<String>,
}

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
    /// Engine pack providing this engine, when it comes from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<EnginePack>,
}

/// Routes of `engine` in the capabilities registry, as sorted `"from -> to"` strings.
fn registry_routes(caps: &ariad_core::planner::Capabilities, engine: &str) -> Vec<String> {
    let mut routes: Vec<String> = caps
        .edges
        .iter()
        .filter(|e| e.engine == engine)
        .map(|e| format!("{} -> {}", e.from.id(), e.to.id()))
        .collect();
    routes.sort_unstable();
    routes.dedup();
    routes
}

fn registry_license(caps: &ariad_core::planner::Capabilities, engine: &str) -> Option<String> {
    caps.edges
        .iter()
        .find(|e| e.engine == engine)
        .map(|e| e.license.clone())
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
    let report =
        engines::availability(engine_program, &default_args, cancel.clone()).map_err(|error| {
            match error {
                RunError::SandboxUnavailable(detail) => ConvertError::SandboxUnavailable(detail),
                RunError::Cancelled => ConvertError::Interrupted,
                _ => ConvertError::Failed,
            }
        })?;

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
        pack: None,
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
        pack: None,
    });

    // 3. Engine packs: docling is always listed; any other installed or overridden pack too.
    rows.extend(pack_rows(engine_program, caps, &cancel)?);

    Ok(rows)
}

/// Pack name always listed, installed or not.
const LISTED_PACK: &str = "docling";

/// License shown for a pack row that has no registry edge and no running engine to ask.
fn placeholder_license(name: &str) -> String {
    if name == LISTED_PACK {
        "Apache-2.0".to_owned()
    } else {
        "-".to_owned()
    }
}

fn pack_rows(
    engine_program: &Path,
    caps: &ariad_core::planner::Capabilities,
    cancel: &CancellationToken,
) -> Result<Vec<EngineRow>, ConvertError> {
    let installed = PackContext::from_env()
        .ok()
        .and_then(|context| packs::list_installed(&context).ok())
        .unwrap_or_default();
    let mut names: BTreeSet<String> = installed.iter().map(|v| v.name.clone()).collect();
    names.extend(packs::override_names());
    names.remove(LISTED_PACK);
    let mut rows = Vec::new();
    for name in std::iter::once(LISTED_PACK.to_owned()).chain(names) {
        if cancel.is_cancelled() {
            return Err(ConvertError::Interrupted);
        }
        let versions: Vec<String> = installed
            .iter()
            .filter(|v| v.name == name)
            .map(|v| v.version.clone())
            .collect();
        let routes = registry_routes(caps, &name);
        let license = registry_license(caps, &name);
        if name == LISTED_PACK && std::env::var_os(docling::OVERRIDE_ENV).is_some() {
            rows.push(describe_override(engine_program, routes, license, cancel)?);
            continue;
        }
        match packs::try_resolve(&name) {
            Ok(Some(pack)) => rows.push(describe_pack(
                engine_program,
                &pack,
                versions,
                routes,
                license,
                cancel,
            )?),
            Ok(None) => rows.push(EngineRow {
                license: license.unwrap_or_else(|| placeholder_license(&name)),
                id: name,
                version: "-".to_owned(),
                status: EngineStatus::NotInstalled,
                note: Some("available in v0.2".to_owned()),
                routes,
                pack: None,
            }),
            Err(_) => rows.push(EngineRow {
                license: license.unwrap_or_else(|| placeholder_license(&name)),
                id: name,
                version: versions.first().cloned().unwrap_or_else(|| "-".to_owned()),
                status: EngineStatus::Missing,
                note: Some("pack is unusable; run 'ashift doctor'".to_owned()),
                routes,
                pack: None,
            }),
        }
    }
    Ok(rows)
}

/// Builds the docling row for the `ASHIFT_DOCLING_ENGINE` development override, which no pack
/// backs.
fn describe_override(
    engine_program: &Path,
    routes: Vec<String>,
    license: Option<String>,
    cancel: &CancellationToken,
) -> Result<EngineRow, ConvertError> {
    let mut row = EngineRow {
        id: LISTED_PACK.to_owned(),
        version: "-".to_owned(),
        license: license
            .clone()
            .unwrap_or_else(|| placeholder_license(LISTED_PACK)),
        status: EngineStatus::Missing,
        note: Some(format!(
            "{} is not a usable command line",
            docling::OVERRIDE_ENV
        )),
        routes,
        pack: None,
    };
    if let Some(command) = docling::resolve() {
        row.note = None;
        apply_describe(
            &mut row,
            runner::describe_command(
                &command.runner_command(engine_program),
                runner::DESCRIBE_TIMEOUT,
                cancel.clone(),
            ),
            license.is_some(),
        )?;
        let note = row.note.take();
        row.note = Some(match note {
            Some(note) => format!("development override; {note}"),
            None => "development override".to_owned(),
        });
    }
    Ok(row)
}

/// Builds an engine row for a resolved pack by running `describe` through its entrypoint.
fn describe_pack(
    engine_program: &Path,
    pack: &InstalledPack,
    versions: Vec<String>,
    routes: Vec<String>,
    license: Option<String>,
    cancel: &CancellationToken,
) -> Result<EngineRow, ConvertError> {
    let info = EnginePack {
        name: pack.name.clone(),
        version: pack.version.clone(),
        target: pack.target.clone(),
        source: pack.source.as_str().to_owned(),
        versions,
    };
    let mut row = EngineRow {
        id: pack.name.clone(),
        version: pack.version.clone(),
        license: license.clone().unwrap_or_else(|| "-".to_owned()),
        status: EngineStatus::Missing,
        note: None,
        routes,
        pack: Some(info),
    };
    apply_describe(
        &mut row,
        runner::describe_command(
            &runner::EngineCommand::from_pack(engine_program, pack),
            runner::DESCRIBE_TIMEOUT,
            cancel.clone(),
        ),
        license.is_some(),
    )?;
    Ok(row)
}

/// Fills `row` from the outcome of an engine's `describe`; `registry_license` says whether the
/// registry already supplied the license.
fn apply_describe(
    row: &mut EngineRow,
    described: Result<Event, RunError>,
    registry_license: bool,
) -> Result<(), ConvertError> {
    match described {
        Ok(Event::Capabilities {
            version,
            tool,
            license: engine_license,
            routes: engine_routes,
            ..
        }) => {
            // Docling converts digital documents without Tesseract; the program only enables
            // OCR, so a missing one is a note, not a missing engine.
            let tool_optional = row.id == LISTED_PACK;
            row.status = match tool.status {
                ToolAvailability::Found => EngineStatus::Ready,
                _ if tool_optional => EngineStatus::Ready,
                ToolAvailability::Missing => EngineStatus::Missing,
                ToolAvailability::WrongVersion => EngineStatus::WrongVersion,
            };
            if tool.status != ToolAvailability::Found {
                row.note = Some(if tool_optional {
                    format!("{} is not ready; OCR is unavailable", tool.name)
                } else {
                    format!("{} is not ready", tool.name)
                });
            }
            if row.version == "-" {
                row.version = version;
            }
            if !registry_license {
                row.license = engine_license;
            }
            if row.routes.is_empty() {
                let mut described: Vec<String> = engine_routes
                    .iter()
                    .map(|route| format!("{} -> {}", route.input, route.output))
                    .collect();
                described.sort_unstable();
                described.dedup();
                row.routes = described;
            }
        }
        Err(RunError::Cancelled) => return Err(ConvertError::Interrupted),
        Err(RunError::SandboxUnavailable(detail)) => {
            return Err(ConvertError::SandboxUnavailable(detail));
        }
        Ok(_) | Err(_) => row.note = Some("engine describe failed".to_owned()),
    }
    Ok(())
}
