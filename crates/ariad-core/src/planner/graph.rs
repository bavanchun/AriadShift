//! Dijkstra shortest-path planning across capability graph edges.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap},
};

use crate::format::Format;
use crate::planner::{
    Capabilities, Plan, PlanError, PlanScore, PlanStep, Profile, Runtime, cost::edge_cost,
};

#[derive(Clone, Copy, PartialEq)]
struct OrderedCost(f64);

impl Eq for OrderedCost {}

impl PartialOrd for OrderedCost {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedCost {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

#[derive(Clone, Eq, PartialEq)]
struct DijkstraNode {
    cost: OrderedCost,
    current: Format,
    steps: Vec<PlanStep>,
    edge_indices: Vec<usize>,
}

impl Ord for DijkstraNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cost
            .cmp(&other.cost)
            .then_with(|| self.steps.len().cmp(&other.steps.len()))
            .then_with(|| {
                for (a, b) in self.steps.iter().zip(other.steps.iter()) {
                    let cmp = a
                        .engine
                        .cmp(&b.engine)
                        .then_with(|| a.to.id().cmp(b.to.id()));
                    if cmp != std::cmp::Ordering::Equal {
                        return cmp;
                    }
                }
                self.steps.len().cmp(&other.steps.len())
            })
    }
}

impl PartialOrd for DijkstraNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct SearchResult {
    cost: OrderedCost,
    steps: Vec<PlanStep>,
    edge_indices: Vec<usize>,
    score: PlanScore,
    measured: bool,
}

fn search_shortest_path(
    caps: &Capabilities,
    from: Format,
    to: Option<Format>,
    profile: Profile,
    available_engines: &[String],
    excluded_edge: Option<usize>,
) -> (Option<SearchResult>, BTreeMap<&'static str, Format>) {
    let mut reachable = BTreeMap::new();
    let mut heap = BinaryHeap::new();
    let mut best_visited: HashMap<&'static str, (OrderedCost, Vec<PlanStep>)> = HashMap::new();

    heap.push(Reverse(DijkstraNode {
        cost: OrderedCost(0.0),
        current: from,
        steps: Vec::new(),
        edge_indices: Vec::new(),
    }));

    while let Some(Reverse(node)) = heap.pop() {
        if let Some(target) = to
            && node.current == target
            && !node.steps.is_empty()
        {
            let mut fidelity = 1.0;
            let mut editability = 1.0;
            let mut duration_ms = 0.0;
            let mut all_measured = true;

            for &idx in &node.edge_indices {
                let edge = &caps.edges[idx];
                if let Some(metrics) = edge.metrics {
                    fidelity *= metrics.fidelity;
                    editability *= metrics.editability;
                    duration_ms += metrics.p50_ms;
                } else {
                    all_measured = false;
                }
            }

            let score = if all_measured {
                PlanScore {
                    fidelity: Some(fidelity),
                    editability: Some(editability),
                    estimated_duration_ms: Some(duration_ms),
                }
            } else {
                PlanScore {
                    fidelity: None,
                    editability: None,
                    estimated_duration_ms: None,
                }
            };

            return (
                Some(SearchResult {
                    cost: node.cost,
                    steps: node.steps,
                    edge_indices: node.edge_indices,
                    score,
                    measured: all_measured,
                }),
                reachable,
            );
        }

        if let Some((best_cost, best_steps)) = best_visited.get(node.current.id()) {
            let ordering = node.cost.cmp(best_cost).then_with(|| {
                node.steps
                    .len()
                    .cmp(&best_steps.len())
                    .then_with(|| node.steps.cmp(best_steps))
            });
            if ordering == std::cmp::Ordering::Greater {
                continue;
            }
        }
        best_visited.insert(node.current.id(), (node.cost, node.steps.clone()));

        for (edge_idx, edge) in caps.edges.iter().enumerate() {
            if Some(edge_idx) == excluded_edge {
                continue;
            }
            if edge.from != node.current {
                continue;
            }
            if !available_engines.iter().any(|eng| eng == &edge.engine) {
                continue;
            }
            if profile == Profile::Private
                && !edge
                    .runtime
                    .iter()
                    .any(|&r| r == Runtime::Local || r == Runtime::Wasm)
            {
                continue;
            }

            // Cycle prevention within a single path
            if node.steps.iter().any(|s| s.from == edge.to) {
                continue;
            }

            if edge.to != from {
                reachable.insert(edge.to.id(), edge.to);
            }

            let transition_cost = edge_cost(edge, profile);
            let next_cost = OrderedCost(node.cost.0 + transition_cost);

            let mut next_steps = node.steps.clone();
            next_steps.push(PlanStep {
                from: edge.from,
                to: edge.to,
                engine: edge.engine.clone(),
            });

            let mut next_indices = node.edge_indices.clone();
            next_indices.push(edge_idx);

            let next_node = DijkstraNode {
                cost: next_cost,
                current: edge.to,
                steps: next_steps,
                edge_indices: next_indices,
            };

            heap.push(Reverse(next_node));
        }
    }

    (None, reachable)
}

/// Finds the optimal conversion plan from `from` to `to` format.
///
/// Returns a [`Plan`] with up to 2 alternative routes, or [`PlanError`] if no route is found
/// or if arguments are invalid.
pub fn plan(
    caps: &Capabilities,
    from: Format,
    to: Format,
    profile: Profile,
    available_engines: &[String],
) -> Result<Plan, PlanError> {
    if from == to {
        return Err(PlanError::SameFormat { from, to });
    }
    caps.validate()?;

    let (best, reachable) =
        search_shortest_path(caps, from, Some(to), profile, available_engines, None);

    let Some(best_route) = best else {
        return Err(PlanError::NoRoute {
            from,
            to,
            reachable: reachable.into_values().collect(),
        });
    };

    let mut candidate_alts: Vec<(OrderedCost, Plan)> = Vec::new();
    for &excluded_idx in &best_route.edge_indices {
        let (alt_opt, _) = search_shortest_path(
            caps,
            from,
            Some(to),
            profile,
            available_engines,
            Some(excluded_idx),
        );
        if let Some(alt) = alt_opt
            && alt.steps != best_route.steps
        {
            let alt_plan = Plan {
                steps: alt.steps,
                score: alt.score,
                measured: alt.measured,
                alternatives: Vec::new(),
            };
            if !candidate_alts
                .iter()
                .any(|(_, a)| a.steps == alt_plan.steps)
            {
                candidate_alts.push((alt.cost, alt_plan));
            }
        }
    }

    candidate_alts.sort_by(|(cost_a, plan_a), (cost_b, plan_b)| {
        cost_a
            .cmp(cost_b)
            .then_with(|| plan_a.steps.cmp(&plan_b.steps))
    });
    candidate_alts.truncate(2);
    let alternatives = candidate_alts.into_iter().map(|(_, p)| p).collect();

    Ok(Plan {
        steps: best_route.steps,
        score: best_route.score,
        measured: best_route.measured,
        alternatives,
    })
}

/// Computes reachable formats from `from` under the given available engines and profile.
#[must_use]
pub fn reachable_formats(
    caps: &Capabilities,
    from: Format,
    profile: Profile,
    available_engines: &[String],
) -> Vec<Format> {
    let (_, reachable) = search_shortest_path(caps, from, None, profile, available_engines, None);
    reachable.into_values().collect()
}
