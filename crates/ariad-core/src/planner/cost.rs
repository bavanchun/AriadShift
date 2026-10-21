//! Weights, scoring, and edge costs for routing profiles.

use crate::planner::{CapabilityEdge, Profile};

/// Reference duration used to normalize p50 execution latency into `[0.0, 1.0]`.
pub const TIME_NORMALIZATION_BASE_MS: f64 = 5_000.0;

/// Default penalty added for every graph hop to favor direct or shorter paths.
pub const HOP_PENALTY: f64 = 0.05;

/// Pessimistic default cost assigned to unmeasured edges.
///
/// Must be strictly greater than the worst possible measured cost under any profile
/// (which is `1.0 + hop_penalty` when fidelity=0, editability=0, and normalized latency=1.0).
/// At 1.5 + hop_penalty, any measured edge strictly beats an unmeasured edge even with
/// unfavorable engine ID tie-breaking.
pub const PESSIMISTIC_DEFAULT_COST: f64 = 1.5;

/// Routing weights configuring Dijkstra shortest-path edge costs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfileWeights {
    /// Weight on `(1.0 - fidelity)` (semantic/visual correctness).
    pub weight_fidelity: f64,
    /// Weight on `(1.0 - editability)` (clean native structure: headings, lists, tables).
    pub weight_editability: f64,
    /// Weight on normalized execution latency.
    pub weight_time: f64,
    /// Constant penalty incurred per hop.
    pub hop_penalty: f64,
}

impl ProfileWeights {
    /// Weights for the `editable` profile: prioritizes semantic editability over pure layout.
    pub const EDITABLE: Self = Self {
        weight_fidelity: 0.30,
        weight_editability: 0.60,
        weight_time: 0.10,
        hop_penalty: HOP_PENALTY,
    };

    /// Weights for the `faithful` profile: prioritizes layout/visual preservation.
    pub const FAITHFUL: Self = Self {
        weight_fidelity: 0.70,
        weight_editability: 0.20,
        weight_time: 0.10,
        hop_penalty: HOP_PENALTY,
    };

    /// Weights for the `fast` profile: prioritizes execution speed.
    pub const FAST: Self = Self {
        weight_fidelity: 0.15,
        weight_editability: 0.15,
        weight_time: 0.70,
        hop_penalty: HOP_PENALTY,
    };

    /// Weights for a given profile.
    ///
    /// The `private` profile prunes cloud edges and otherwise optimizes for `editable`.
    #[must_use]
    pub const fn for_profile(profile: Profile) -> Self {
        match profile {
            Profile::Editable | Profile::Private => Self::EDITABLE,
            Profile::Faithful => Self::FAITHFUL,
            Profile::Fast => Self::FAST,
        }
    }
}

/// Normalizes p50 duration in milliseconds into a `[0.0, 1.0]` scalar.
#[must_use]
pub fn normalize_duration(p50_ms: f64) -> f64 {
    if !p50_ms.is_finite() || p50_ms <= 0.0 {
        0.0
    } else {
        (p50_ms / TIME_NORMALIZATION_BASE_MS).min(1.0)
    }
}

/// Computes the routing cost for an edge under a given profile.
#[must_use]
pub fn edge_cost(edge: &CapabilityEdge, profile: Profile) -> f64 {
    let weights = ProfileWeights::for_profile(profile);
    match edge.metrics {
        Some(metrics) => {
            let fid_term = weights.weight_fidelity * (1.0 - metrics.fidelity.clamp(0.0, 1.0));
            let edit_term =
                weights.weight_editability * (1.0 - metrics.editability.clamp(0.0, 1.0));
            let time_term = weights.weight_time * normalize_duration(metrics.p50_ms);
            fid_term + edit_term + time_term + weights.hop_penalty
        }
        None => PESSIMISTIC_DEFAULT_COST + weights.hop_penalty,
    }
}
