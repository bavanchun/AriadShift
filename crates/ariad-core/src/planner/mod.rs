//! Conversion graph, capability metrics, profiles, and path planning.

pub mod cost;
pub mod graph;

use std::{collections::HashSet, sync::OnceLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::format::Format;

pub use cost::{
    HOP_PENALTY, PESSIMISTIC_DEFAULT_COST, ProfileWeights, TIME_NORMALIZATION_BASE_MS, edge_cost,
    normalize_duration,
};
pub use graph::{plan, reachable_formats};

/// The root capabilities registry specifying format conversion edges and empirical metrics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Schema version, e.g. `"ariad-capabilities/0"`.
    pub version: String,
    /// ISO 8601 / RFC 3339 timestamp of generation.
    pub generated_at: String,
    /// Benchmark environment and toolchain metadata.
    pub bench: BenchMetadata,
    /// Directed format conversion capability edges.
    pub edges: Vec<CapabilityEdge>,
}

/// Metadata about the benchmark run that produced the capabilities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchMetadata {
    /// Number of fixtures measured.
    pub fixture_count: u32,
    /// Pandoc version used, if present.
    #[serde(default)]
    pub pandoc_version: Option<String>,
    /// AriadShift CLI/core version.
    pub ashift_version: String,
}

/// Supported runtime environments for conversion capabilities.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    /// In-process WebAssembly or pure-Rust environment.
    Wasm,
    /// Local native executable or OS process.
    Local,
    /// Remote network or cloud execution service.
    Cloud,
}

/// A directed edge representing an engine's ability to convert from one format to another.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityEdge {
    /// Source format.
    pub from: Format,
    /// Target format.
    pub to: Format,
    /// Engine identifier, e.g. `"ariad-core"` or `"pandoc"`.
    pub engine: String,
    /// Supported runtime environments, e.g. `["wasm", "local"]` or `["local"]`.
    #[schemars(length(min = 1))]
    pub runtime: Vec<Runtime>,
    /// Software license identifier of the engine or component.
    pub license: String,
    /// Empirical benchmark metrics, or `null` if unmeasured.
    #[serde(deserialize_with = "deserialize_metrics")]
    pub metrics: Option<EdgeMetrics>,
}

fn deserialize_metrics<'de, D>(deserializer: D) -> Result<Option<EdgeMetrics>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<EdgeMetrics>::deserialize(deserializer)
}

/// Empirical quality and performance metrics for a conversion edge.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeMetrics {
    /// Structural and content fidelity score in `[0.0, 1.0]`.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub fidelity: f64,
    /// Semantic editability score in `[0.0, 1.0]`.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub editability: f64,
    /// Median conversion latency in milliseconds.
    #[schemars(range(min = 0.0, max = 86_400_000.0))]
    pub p50_ms: f64,
    /// Peak memory residency in megabytes.
    #[schemars(range(min = 0.0, max = 1_048_576.0))]
    pub peak_mem_mb: f64,
    /// Number of benchmark samples.
    #[schemars(range(min = 1))]
    pub samples: u32,
}

/// User routing profile specifying conversion priorities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Semantic accuracy, structure, headings, lists, tables (default).
    #[default]
    Editable,
    /// Visual and page layout preservation.
    Faithful,
    /// Execution speed.
    Fast,
    /// Only local and wasm runtimes; cloud edges are pruned.
    Private,
}

impl Profile {
    /// The string identifier for the profile.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Editable => "editable",
            Self::Faithful => "faithful",
            Self::Fast => "fast",
            Self::Private => "private",
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Profile {
    type Err = ProfileParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "editable" => Ok(Self::Editable),
            "faithful" => Ok(Self::Faithful),
            "fast" => Ok(Self::Fast),
            "private" => Ok(Self::Private),
            _ => Err(ProfileParseError(s.to_owned())),
        }
    }
}

/// Error returned when parsing an unrecognized profile name.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unknown profile: '{0}'; valid profiles: editable, faithful, fast, private")]
pub struct ProfileParseError(pub String);

/// An execution plan specifying the sequence of conversions and projected scores.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    /// Conversion steps from source to target.
    pub steps: Vec<PlanStep>,
    /// Aggregated score along the route.
    pub score: PlanScore,
    /// Whether every edge in the route has empirical measurements.
    pub measured: bool,
    /// Up to 2 alternative routes with lower rank.
    pub alternatives: Vec<Plan>,
}

/// A single conversion step within a plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanStep {
    /// Step input format.
    pub from: Format,
    /// Step output format.
    pub to: Format,
    /// Engine executing this step.
    pub engine: String,
}

impl Ord for PlanStep {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.engine
            .cmp(&other.engine)
            .then_with(|| self.from.id().cmp(other.from.id()))
            .then_with(|| self.to.id().cmp(other.to.id()))
    }
}

impl PartialOrd for PlanStep {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Aggregated metrics for a conversion plan.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanScore {
    /// Aggregated fidelity score (multiplied along steps), or `None` if unmeasured.
    pub fidelity: Option<f64>,
    /// Aggregated editability score (multiplied along steps), or `None` if unmeasured.
    pub editability: Option<f64>,
    /// Estimated duration in milliseconds (summed along steps), or `None` if unmeasured.
    pub estimated_duration_ms: Option<f64>,
}

/// Error returned when planning a conversion route.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PlanError {
    /// No route exists connecting the requested format pair.
    #[error("no route from {} to {}{}", .from.id(), .to.id(), reachable_summary(.reachable))]
    NoRoute {
        /// Requested source format.
        from: Format,
        /// Requested target format.
        to: Format,
        /// Format targets reachable from the source.
        reachable: Vec<Format>,
    },
    /// Direct same-format conversions are disallowed.
    #[error("same format route: {} to {}", .from.id(), .to.id())]
    SameFormat {
        /// Source format.
        from: Format,
        /// Target format.
        to: Format,
    },
    /// The capability set is malformed or invalid.
    #[error("invalid capabilities: {0}")]
    InvalidCapabilities(#[from] CapabilitiesError),
}

fn reachable_summary(reachable: &[Format]) -> String {
    if reachable.is_empty() {
        "; no reachable target formats".to_owned()
    } else {
        let targets = reachable
            .iter()
            .map(|f| f.id())
            .collect::<Vec<_>>()
            .join(", ");
        format!("; reachable target formats: {targets}")
    }
}

/// Validation error for capability sets.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CapabilitiesError {
    /// Schema version mismatch.
    #[error("unsupported capabilities version: '{actual}', expected '{expected}'")]
    InvalidVersion {
        /// Expected version.
        expected: &'static str,
        /// Actual version found in JSON.
        actual: String,
    },
    /// No capability edges found.
    #[error("capabilities edge set is empty")]
    EmptyEdges,
    /// Duplicate edge definition.
    #[error("duplicate edge for {} -> {} with engine '{engine}'", .from.id(), .to.id())]
    DuplicateEdge {
        /// Source format.
        from: Format,
        /// Target format.
        to: Format,
        /// Engine name.
        engine: String,
    },
    /// Self-looping edge.
    #[error("self-loop edge from {} to {}", .format.id(), .format.id())]
    SelfLoop {
        /// Format of the loop.
        format: Format,
    },
    /// Missing or invalid engine identifier.
    #[error("edge {edge_index} has empty engine")]
    EmptyEngine {
        /// Index of the edge.
        edge_index: usize,
    },
    /// Missing or invalid runtime declaration.
    #[error("edge {edge_index} has empty runtime list")]
    EmptyRuntime {
        /// Index of the edge.
        edge_index: usize,
    },
    /// Duplicate runtime declared for an edge.
    #[error("edge {edge_index} has duplicate runtime")]
    DuplicateRuntime {
        /// Index of the edge.
        edge_index: usize,
    },
    /// Missing or invalid license identifier.
    #[error("edge {edge_index} has empty license")]
    EmptyLicense {
        /// Index of the edge.
        edge_index: usize,
    },
    /// Invalid metrics values (NaN, negative, out of range).
    #[error("edge {edge_index} has invalid metrics: {reason}")]
    InvalidMetrics {
        /// Index of the edge.
        edge_index: usize,
        /// Specific validation failure reason.
        reason: &'static str,
    },
}

impl Capabilities {
    /// Returns the distinct engine identifiers defined across all edges, sorted.
    #[must_use]
    pub fn registered_engines(&self) -> Vec<String> {
        let mut engines: Vec<String> = self.edges.iter().map(|e| e.engine.clone()).collect();
        engines.sort_unstable();
        engines.dedup();
        engines
    }

    /// Validates the structural integrity, version, and metric bounds of the capabilities.
    pub fn validate(&self) -> Result<(), CapabilitiesError> {
        const EXPECTED_VERSION: &str = "ariad-capabilities/0";
        if self.version != EXPECTED_VERSION {
            return Err(CapabilitiesError::InvalidVersion {
                expected: EXPECTED_VERSION,
                actual: self.version.clone(),
            });
        }
        if self.edges.is_empty() {
            return Err(CapabilitiesError::EmptyEdges);
        }

        let mut seen_edges = HashSet::new();
        for (i, edge) in self.edges.iter().enumerate() {
            if edge.from == edge.to {
                return Err(CapabilitiesError::SelfLoop { format: edge.from });
            }
            if edge.engine.trim().is_empty() {
                return Err(CapabilitiesError::EmptyEngine { edge_index: i });
            }
            if edge.runtime.is_empty() {
                return Err(CapabilitiesError::EmptyRuntime { edge_index: i });
            }
            let mut seen_runtimes = std::collections::BTreeSet::new();
            for r in &edge.runtime {
                if !seen_runtimes.insert(*r) {
                    return Err(CapabilitiesError::DuplicateRuntime { edge_index: i });
                }
            }
            if edge.license.trim().is_empty() {
                return Err(CapabilitiesError::EmptyLicense { edge_index: i });
            }

            let key = (edge.from.id(), edge.to.id(), edge.engine.clone());
            if !seen_edges.insert(key) {
                return Err(CapabilitiesError::DuplicateEdge {
                    from: edge.from,
                    to: edge.to,
                    engine: edge.engine.clone(),
                });
            }

            if let Some(metrics) = edge.metrics {
                if !metrics.fidelity.is_finite() || !(0.0..=1.0).contains(&metrics.fidelity) {
                    return Err(CapabilitiesError::InvalidMetrics {
                        edge_index: i,
                        reason: "fidelity must be a finite number between 0.0 and 1.0",
                    });
                }
                if !metrics.editability.is_finite() || !(0.0..=1.0).contains(&metrics.editability) {
                    return Err(CapabilitiesError::InvalidMetrics {
                        edge_index: i,
                        reason: "editability must be a finite number between 0.0 and 1.0",
                    });
                }
                if !metrics.p50_ms.is_finite()
                    || metrics.p50_ms < 0.0
                    || metrics.p50_ms > 86_400_000.0
                {
                    return Err(CapabilitiesError::InvalidMetrics {
                        edge_index: i,
                        reason: "p50_ms must be a non-negative finite duration in milliseconds",
                    });
                }
                if !metrics.peak_mem_mb.is_finite()
                    || metrics.peak_mem_mb < 0.0
                    || metrics.peak_mem_mb > 1_048_576.0
                {
                    return Err(CapabilitiesError::InvalidMetrics {
                        edge_index: i,
                        reason: "peak_mem_mb must be a non-negative finite memory in megabytes",
                    });
                }
                if metrics.samples == 0 {
                    return Err(CapabilitiesError::InvalidMetrics {
                        edge_index: i,
                        reason: "samples must be greater than zero",
                    });
                }
            }
        }
        Ok(())
    }
}

/// Returns the embedded bootstrap capabilities file compiled into `ariad-core`.
///
/// # Panics
/// Panics if the embedded capabilities data cannot be parsed or validated.
#[must_use]
pub fn embedded() -> &'static Capabilities {
    static EMBEDDED: OnceLock<Capabilities> = OnceLock::new();
    EMBEDDED.get_or_init(|| {
        let content = include_str!("../../data/capabilities.json");
        let caps: Capabilities = serde_json::from_str(content)
            .expect("embedded capabilities.json must be valid JSON matching Capabilities");
        caps.validate()
            .expect("embedded capabilities.json must be structurally valid");
        caps
    })
}
