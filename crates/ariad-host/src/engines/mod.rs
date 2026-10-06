pub mod pandoc;

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use ariad_core::protocol::{Event, ToolAvailability, ToolStatus};
use tokio_util::sync::CancellationToken;

use crate::runner::{self, RunError};

/// Availability status for an engine.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineAvailability {
    /// Engine identifier (e.g. `"ariad-core"` or `"pandoc"`).
    pub engine: String,
    /// Whether the engine is available for conversions.
    pub available: bool,
    /// Detailed tool status if available.
    pub tool_status: Option<ToolStatus>,
    /// Engine package version if known.
    pub version: Option<String>,
}

/// Aggregated availability report for all known engines.
#[derive(Clone, Debug, PartialEq)]
pub struct AvailabilityReport {
    /// List of engine statuses.
    pub engines: Vec<EngineAvailability>,
}

impl AvailabilityReport {
    /// Returns the identifiers of all engines that are currently available.
    #[must_use]
    pub fn available_engine_ids(&self) -> Vec<String> {
        self.engines
            .iter()
            .filter(|e| e.available)
            .map(|e| e.engine.clone())
            .collect()
    }

    /// Checks if a specific engine is available.
    #[must_use]
    pub fn is_available(&self, engine: &str) -> bool {
        self.engines
            .iter()
            .any(|e| e.engine == engine && e.available)
    }
}

type AvailabilityCacheKey = (PathBuf, Vec<OsString>);
type AvailabilityCache = Mutex<HashMap<AvailabilityCacheKey, AvailabilityReport>>;

static AVAILABILITY_CACHE: OnceLock<AvailabilityCache> = OnceLock::new();

fn availability_cache() -> &'static AvailabilityCache {
    AVAILABILITY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Single availability source querying engine capabilities.
///
/// `ariad-core` is in-process and always available.
/// `pandoc` runs out-of-process and is queried via `describe`. If `tool.status == Found`,
/// it is marked available; otherwise unavailable.
///
/// Availability is cached per process run for performance.
pub fn availability(
    engine_program: &Path,
    engine_args: &[OsString],
    cancel: CancellationToken,
) -> Result<AvailabilityReport, RunError> {
    let default_args = [OsString::from("__engine"), OsString::from("pandoc")];
    let normalized_args: &[OsString] = if engine_args.is_empty() {
        &default_args[..]
    } else {
        engine_args
    };
    let key = (engine_program.to_path_buf(), normalized_args.to_vec());
    {
        let cache = match availability_cache().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(report) = cache.get(&key) {
            return Ok(report.clone());
        }
    }

    let report = query_availability(engine_program, normalized_args, cancel)?;
    let mut cache = match availability_cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    cache.insert(key, report.clone());
    Ok(report)
}

/// The in-process engine identifier for pure-Rust operations.
pub const IN_PROCESS_ENGINE: &str = "ariad-core";

/// Queries engine availability directly without cache.
pub fn query_availability(
    engine_program: &Path,
    engine_args: &[OsString],
    cancel: CancellationToken,
) -> Result<AvailabilityReport, RunError> {
    let mut engines = Vec::new();

    // 1. In-process ariad-core is always available
    engines.push(EngineAvailability {
        engine: IN_PROCESS_ENGINE.to_owned(),
        available: true,
        tool_status: Some(ToolStatus {
            name: IN_PROCESS_ENGINE.to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            status: ToolAvailability::Found,
        }),
        version: Some(env!("CARGO_PKG_VERSION").to_owned()),
    });

    // 2. Out-of-process engine queried via describe
    let default_args = [OsString::from("__engine"), OsString::from("pandoc")];
    let describe_args = if engine_args.is_empty() {
        &default_args[..]
    } else {
        engine_args
    };

    match runner::describe(engine_program, describe_args, cancel) {
        Ok(Event::Capabilities {
            engine,
            version,
            tool,
            ..
        }) => {
            let available = tool.status == ToolAvailability::Found;
            engines.push(EngineAvailability {
                engine: "pandoc".to_owned(),
                available,
                tool_status: Some(tool.clone()),
                version: Some(version.clone()),
            });
            if !engine.is_empty() && engine != "pandoc" {
                engines.push(EngineAvailability {
                    engine,
                    available,
                    tool_status: Some(tool),
                    version: Some(version),
                });
            }
        }
        Err(RunError::Spawn(ref source)) if source.kind() == std::io::ErrorKind::NotFound => {
            engines.push(EngineAvailability {
                engine: "pandoc".to_owned(),
                available: false,
                tool_status: None,
                version: None,
            });
        }
        Err(err) => return Err(err),
        Ok(_) => {
            return Err(RunError::ProtocolViolation(
                "expected capabilities event from describe",
            ));
        }
    }

    Ok(AvailabilityReport { engines })
}
