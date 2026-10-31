#![no_main]

//! Capabilities registry JSON (`ariad-capabilities/0`) and the route planner that runs on it:
//! deserialization, `validate`, `plan` and `reachable_formats` over every format pair the
//! registry mentions, under every profile.

use std::collections::BTreeSet;

use ariad_core::{
    format::Format,
    planner::{Capabilities, Plan, PlanError, Profile, Runtime, plan, reachable_formats},
};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Planning every pair under every profile is quadratic in formats; larger registries are
/// only parsed, validated and round-tripped.
const MAX_PLANNED_EDGES: usize = 48;
const PROFILES: [Profile; 4] = [
    Profile::Editable,
    Profile::Faithful,
    Profile::Fast,
    Profile::Private,
];

/// A plan is a chain of registry edges from `from` to `to` that uses only available engines.
fn assert_route(caps: &Capabilities, route: &Plan, from: Format, to: Format, engines: &[String]) {
    let steps = &route.steps;
    assert!(!steps.is_empty(), "empty route from {from:?} to {to:?}");
    assert_eq!(steps[0].from, from);
    assert_eq!(steps[steps.len() - 1].to, to);
    for pair in steps.windows(2) {
        assert_eq!(pair[0].to, pair[1].from, "route steps do not chain");
    }
    for step in steps {
        assert!(
            engines.contains(&step.engine),
            "route uses an unavailable engine"
        );
        assert!(
            caps.edges.iter().any(|edge| edge.from == step.from
                && edge.to == step.to
                && edge.engine == step.engine),
            "route step is not a registry edge"
        );
    }
    let score = route.score;
    for value in [score.fidelity, score.editability].into_iter().flatten() {
        assert!(
            (0.0..=1.0).contains(&value),
            "route score {value} outside [0, 1]"
        );
    }
    if let Some(duration) = score.estimated_duration_ms {
        assert!(duration.is_finite() && duration >= 0.0);
    }
}

fn check_plans(caps: &Capabilities, engines: &[String]) {
    let sources: BTreeSet<_> = caps.edges.iter().map(|edge| edge.from.id()).collect();
    let targets: BTreeSet<_> = caps.edges.iter().map(|edge| edge.to.id()).collect();
    for profile in PROFILES {
        for from in sources.iter().filter_map(|id| Format::from_id(id)) {
            let reachable = reachable_formats(caps, from, profile, engines);
            for to in targets.iter().filter_map(|id| Format::from_id(id)) {
                match plan(caps, from, to, profile, engines) {
                    Ok(route) => {
                        assert_route(caps, &route, from, to, engines);
                        assert!(route.alternatives.len() <= 2);
                        for alternative in &route.alternatives {
                            assert_route(caps, alternative, from, to, engines);
                            assert_ne!(alternative.steps, route.steps);
                        }
                        assert!(reachable.contains(&to), "planned target is not reachable");
                    }
                    Err(PlanError::NoRoute {
                        reachable: listed, ..
                    }) => {
                        assert!(!reachable.contains(&to), "reachable target has no route");
                        assert!(!listed.contains(&to));
                    }
                    Err(PlanError::SameFormat { .. }) => assert_eq!(from, to),
                    Err(PlanError::InvalidCapabilities(error)) => {
                        panic!("validated capabilities rejected by plan: {error}")
                    }
                }
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let Ok(caps) = serde_json::from_slice::<Capabilities>(data) else {
        return;
    };

    if caps.validate().is_err() {
        // Planning on an invalid registry is refused, never attempted.
        let result = plan(
            &caps,
            Format::Markdown,
            Format::Html,
            Profile::Editable,
            &[],
        );
        assert!(matches!(result, Err(PlanError::InvalidCapabilities(_))));
        return;
    }

    let encoded = serde_json::to_vec(&caps).expect("capabilities serialize");
    let decoded: Capabilities = serde_json::from_slice(&encoded).expect("capabilities decode");
    assert_eq!(
        decoded, caps,
        "capabilities changed across a JSON round trip"
    );

    for runtime in [Runtime::Wasm, Runtime::Local, Runtime::Cloud] {
        let restricted = caps.restricted_to(runtime);
        assert!(
            restricted
                .edges
                .iter()
                .all(|edge| edge.runtime.contains(&runtime))
        );
    }

    if caps.edges.len() > MAX_PLANNED_EDGES {
        return;
    }
    let engines = caps.registered_engines();
    check_plans(&caps, &engines);
    // Removing one engine only removes routes; the planner still answers for every pair.
    if let Some((_, rest)) = engines.split_first() {
        check_plans(&caps, rest);
    }
});
