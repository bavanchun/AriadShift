use ariad_core::{
    format::Format,
    planner::{
        BenchMetadata, Capabilities, CapabilitiesError, CapabilityEdge, EdgeMetrics, PlanError,
        PlanStep, Profile, ProfileParseError, Runtime, embedded, plan, reachable_formats,
    },
};

fn synthetic_caps(edges: Vec<CapabilityEdge>) -> Capabilities {
    Capabilities {
        version: "ariad-capabilities/0".to_owned(),
        generated_at: "2026-10-06T00:00:00Z".to_owned(),
        bench: BenchMetadata {
            fixture_count: 10,
            pandoc_version: Some("3.12".to_owned()),
            ashift_version: "0.0.0".to_owned(),
        },
        edges,
    }
}

fn edge(
    from: Format,
    to: Format,
    engine: &str,
    runtime: &[Runtime],
    metrics: Option<EdgeMetrics>,
) -> CapabilityEdge {
    CapabilityEdge {
        from,
        to,
        engine: engine.to_owned(),
        runtime: runtime.to_vec(),
        license: "Apache-2.0".to_owned(),
        metrics,
    }
}

#[test]
fn embedded_capabilities_matches_fixed_table_routes_and_length_two() {
    let caps = embedded();
    let formats = [Format::Markdown, Format::Html, Format::Docx, Format::Epub];
    let engines = ["ariad-core".to_owned(), "pandoc".to_owned()];

    for &from in &formats {
        for &to in &formats {
            if from == to {
                let err = plan(caps, from, to, Profile::Editable, &engines).unwrap_err();
                assert_eq!(err, PlanError::SameFormat { from, to });
                continue;
            }

            let p = plan(caps, from, to, Profile::Editable, &engines)
                .unwrap_or_else(|err| panic!("expected route for {from:?} -> {to:?}: {err}"));

            // Real routes must always pass through IR: length 2
            assert_eq!(
                p.steps.len(),
                2,
                "route for {from:?} -> {to:?} must be length 2"
            );
            assert_eq!(p.steps[0].from, from);
            assert_eq!(p.steps[0].to, Format::AriadIrJson);
            assert_eq!(p.steps[1].from, Format::AriadIrJson);
            assert_eq!(p.steps[1].to, to);

            // Assert exact engine choices matching the fixed route table
            let expected_reader_engine = match from {
                Format::Markdown | Format::Html => "ariad-core",
                Format::Docx | Format::Epub => "pandoc",
                _ => unreachable!(),
            };
            let expected_writer_engine = match to {
                Format::Markdown | Format::Html => "ariad-core",
                Format::Docx | Format::Epub => "pandoc",
                _ => unreachable!(),
            };

            assert_eq!(
                p.steps[0],
                PlanStep {
                    from,
                    to: Format::AriadIrJson,
                    engine: expected_reader_engine.to_owned(),
                }
            );
            assert_eq!(
                p.steps[1],
                PlanStep {
                    from: Format::AriadIrJson,
                    to,
                    engine: expected_writer_engine.to_owned(),
                }
            );
            assert!(p.alternatives.is_empty());
            assert!(p.measured);
        }
    }
}

#[test]
fn two_competing_readers_profile_routing() {
    // Engine A: high fidelity (0.98), lower speed (p50 = 2000ms), editability 0.90
    // Engine B: lower fidelity (0.80), high speed (p50 = 100ms), editability 0.90
    let edge_a = edge(
        Format::Docx,
        Format::AriadIrJson,
        "engine-accurate",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.98,
            editability: 0.90,
            p50_ms: 2000.0,
            peak_mem_mb: 128.0,
            samples: 10,
        }),
    );
    let edge_b = edge(
        Format::Docx,
        Format::AriadIrJson,
        "engine-fast",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.80,
            editability: 0.90,
            p50_ms: 100.0,
            peak_mem_mb: 32.0,
            samples: 10,
        }),
    );
    let writer = edge(
        Format::AriadIrJson,
        Format::Markdown,
        "ariad-core",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.99,
            editability: 0.99,
            p50_ms: 50.0,
            peak_mem_mb: 16.0,
            samples: 10,
        }),
    );

    let caps = synthetic_caps(vec![edge_a, edge_b, writer]);
    let engines = [
        "engine-accurate".to_owned(),
        "engine-fast".to_owned(),
        "ariad-core".to_owned(),
    ];

    // Under Faithful, accurate engine must win
    let plan_faithful = plan(
        &caps,
        Format::Docx,
        Format::Markdown,
        Profile::Faithful,
        &engines,
    )
    .unwrap();
    assert_eq!(plan_faithful.steps[0].engine, "engine-accurate");
    assert!(plan_faithful.measured);
    assert_eq!(plan_faithful.alternatives.len(), 1);
    assert_eq!(plan_faithful.alternatives[0].steps[0].engine, "engine-fast");

    // Under Fast, fast engine must win
    let plan_fast = plan(
        &caps,
        Format::Docx,
        Format::Markdown,
        Profile::Fast,
        &engines,
    )
    .unwrap();
    assert_eq!(plan_fast.steps[0].engine, "engine-fast");
    assert!(plan_fast.measured);
    assert_eq!(plan_fast.alternatives.len(), 1);
    assert_eq!(plan_fast.alternatives[0].steps[0].engine, "engine-accurate");
}

#[test]
fn unmeasured_versus_measured_edge_preference() {
    // Measured reader with moderate quality
    let measured_reader = edge(
        Format::Html,
        Format::AriadIrJson,
        "measured-reader",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.85,
            editability: 0.85,
            p50_ms: 200.0,
            peak_mem_mb: 64.0,
            samples: 5,
        }),
    );
    // Unmeasured reader (metrics: None)
    let unmeasured_reader = edge(
        Format::Html,
        Format::AriadIrJson,
        "unmeasured-reader",
        &[Runtime::Local],
        None,
    );
    let writer = edge(
        Format::AriadIrJson,
        Format::Docx,
        "pandoc",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.95,
            editability: 0.95,
            p50_ms: 300.0,
            peak_mem_mb: 128.0,
            samples: 5,
        }),
    );

    let caps = synthetic_caps(vec![measured_reader, unmeasured_reader, writer]);
    let engines = [
        "measured-reader".to_owned(),
        "unmeasured-reader".to_owned(),
        "pandoc".to_owned(),
    ];

    let p = plan(
        &caps,
        Format::Html,
        Format::Docx,
        Profile::Editable,
        &engines,
    )
    .unwrap();
    // Measured reader must win due to pessimistic cost on unmeasured edge
    assert_eq!(p.steps[0].engine, "measured-reader");
    assert!(p.measured);
    // Alternative should be the unmeasured reader
    assert_eq!(p.alternatives.len(), 1);
    assert_eq!(p.alternatives[0].steps[0].engine, "unmeasured-reader");
    assert!(!p.alternatives[0].measured);
}

#[test]
fn cloud_only_edge_and_private_pruning() {
    // Cloud-only reader with superior metrics
    let cloud_reader = edge(
        Format::Pdf,
        Format::AriadIrJson,
        "cloud-vlm",
        &[Runtime::Cloud],
        Some(EdgeMetrics {
            fidelity: 0.99,
            editability: 0.99,
            p50_ms: 1000.0,
            peak_mem_mb: 256.0,
            samples: 10,
        }),
    );
    // Local reader with lower metrics
    let local_reader = edge(
        Format::Pdf,
        Format::AriadIrJson,
        "local-ocr",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.80,
            editability: 0.80,
            p50_ms: 3000.0,
            peak_mem_mb: 512.0,
            samples: 10,
        }),
    );
    let writer = edge(
        Format::AriadIrJson,
        Format::Markdown,
        "ariad-core",
        &[Runtime::Wasm, Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 1.0,
            editability: 1.0,
            p50_ms: 10.0,
            peak_mem_mb: 16.0,
            samples: 10,
        }),
    );

    let caps = synthetic_caps(vec![cloud_reader, local_reader, writer]);
    let engines = [
        "cloud-vlm".to_owned(),
        "local-ocr".to_owned(),
        "ariad-core".to_owned(),
    ];

    // Under Editable, cloud reader wins because metrics are better
    let plan_editable = plan(
        &caps,
        Format::Pdf,
        Format::Markdown,
        Profile::Editable,
        &engines,
    )
    .unwrap();
    assert_eq!(plan_editable.steps[0].engine, "cloud-vlm");

    // Under Private, cloud reader must be pruned and local reader must win
    let plan_private = plan(
        &caps,
        Format::Pdf,
        Format::Markdown,
        Profile::Private,
        &engines,
    )
    .unwrap();
    assert_eq!(plan_private.steps[0].engine, "local-ocr");
    // Cloud route cannot even appear as an alternative under Private
    assert!(plan_private.alternatives.is_empty());

    // If only cloud reader exists and Private profile is requested, returns NoRoute
    let cloud_only_caps = synthetic_caps(vec![
        edge(
            Format::Pdf,
            Format::AriadIrJson,
            "cloud-vlm",
            &[Runtime::Cloud],
            None,
        ),
        edge(
            Format::AriadIrJson,
            Format::Markdown,
            "ariad-core",
            &[Runtime::Local],
            None,
        ),
    ]);
    let err = plan(
        &cloud_only_caps,
        Format::Pdf,
        Format::Markdown,
        Profile::Private,
        &engines,
    )
    .unwrap_err();
    assert!(matches!(err, PlanError::NoRoute { .. }));
}

#[test]
fn deterministic_tie_breaking_on_engine_id_then_format_id() {
    // Two readers with exactly identical metrics
    let edge_a = edge(
        Format::Docx,
        Format::AriadIrJson,
        "engine-alpha",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.90,
            editability: 0.90,
            p50_ms: 500.0,
            peak_mem_mb: 64.0,
            samples: 10,
        }),
    );
    let edge_z = edge(
        Format::Docx,
        Format::AriadIrJson,
        "engine-zeta",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.90,
            editability: 0.90,
            p50_ms: 500.0,
            peak_mem_mb: 64.0,
            samples: 10,
        }),
    );
    let writer = edge(
        Format::AriadIrJson,
        Format::Markdown,
        "ariad-core",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 1.0,
            editability: 1.0,
            p50_ms: 10.0,
            peak_mem_mb: 16.0,
            samples: 10,
        }),
    );

    // Test with edge_z placed first in edges list to verify order independence
    let caps = synthetic_caps(vec![edge_z, edge_a, writer]);
    let engines = [
        "engine-zeta".to_owned(),
        "engine-alpha".to_owned(),
        "ariad-core".to_owned(),
    ];

    let p = plan(
        &caps,
        Format::Docx,
        Format::Markdown,
        Profile::Editable,
        &engines,
    )
    .unwrap();
    // "engine-alpha" < "engine-zeta" lexicographically
    assert_eq!(p.steps[0].engine, "engine-alpha");
    assert_eq!(p.alternatives.len(), 1);
    assert_eq!(p.alternatives[0].steps[0].engine, "engine-zeta");
}

#[test]
fn deterministic_tie_breaking_on_unmeasured_edges() {
    // Two unmeasured readers: tie break must use engine id
    let unmeasured_alpha = edge(
        Format::Docx,
        Format::AriadIrJson,
        "engine-alpha",
        &[Runtime::Local],
        None,
    );
    let unmeasured_zeta = edge(
        Format::Docx,
        Format::AriadIrJson,
        "engine-zeta",
        &[Runtime::Local],
        None,
    );
    let writer = edge(
        Format::AriadIrJson,
        Format::Markdown,
        "ariad-core",
        &[Runtime::Local],
        None,
    );

    // Provide zeta before alpha in capability edges to confirm ordering independence
    let caps = synthetic_caps(vec![unmeasured_zeta, unmeasured_alpha, writer.clone()]);
    let engines = [
        "engine-zeta".to_owned(),
        "engine-alpha".to_owned(),
        "ariad-core".to_owned(),
    ];

    let p = plan(
        &caps,
        Format::Docx,
        Format::Markdown,
        Profile::Editable,
        &engines,
    )
    .unwrap();
    assert_eq!(p.steps[0].engine, "engine-alpha");
    assert_eq!(p.alternatives.len(), 1);
    assert_eq!(p.alternatives[0].steps[0].engine, "engine-zeta");

    // Measured reader with worst possible metrics beats unmeasured reader with earlier alphabetical name under Profile::Fast
    let unmeasured_aaa = edge(
        Format::Docx,
        Format::AriadIrJson,
        "aaa-unmeasured",
        &[Runtime::Local],
        None,
    );
    let measured_zzz = edge(
        Format::Docx,
        Format::AriadIrJson,
        "zzz-measured",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.0,
            editability: 0.0,
            p50_ms: 5000.0,
            peak_mem_mb: 64.0,
            samples: 10,
        }),
    );
    let caps_mixed = synthetic_caps(vec![unmeasured_aaa, measured_zzz, writer]);
    let engines_mixed = [
        "aaa-unmeasured".to_owned(),
        "zzz-measured".to_owned(),
        "ariad-core".to_owned(),
    ];

    let p_mixed = plan(
        &caps_mixed,
        Format::Docx,
        Format::Markdown,
        Profile::Fast,
        &engines_mixed,
    )
    .unwrap();
    // zzz-measured wins despite later alphabetical name because worst measured cost (1.05) < pessimistic default cost (1.55)
    assert_eq!(p_mixed.steps[0].engine, "zzz-measured");
    assert_eq!(p_mixed.alternatives.len(), 1);
    assert_eq!(p_mixed.alternatives[0].steps[0].engine, "aaa-unmeasured");
}

#[test]
fn no_route_error_lists_reachable_target_formats() {
    let caps = synthetic_caps(vec![
        edge(
            Format::Markdown,
            Format::AriadIrJson,
            "ariad-core",
            &[Runtime::Local],
            None,
        ),
        edge(
            Format::AriadIrJson,
            Format::Html,
            "ariad-core",
            &[Runtime::Local],
            None,
        ),
        edge(
            Format::AriadIrJson,
            Format::Docx,
            "pandoc",
            &[Runtime::Local],
            None,
        ),
    ]);
    let engines = ["ariad-core".to_owned(), "pandoc".to_owned()];

    // Markdown can reach IR, Html, Docx, but Epub is unreachable
    let err = plan(
        &caps,
        Format::Markdown,
        Format::Epub,
        Profile::Editable,
        &engines,
    )
    .unwrap_err();
    match err {
        PlanError::NoRoute {
            from,
            to,
            reachable,
        } => {
            assert_eq!(from, Format::Markdown);
            assert_eq!(to, Format::Epub);
            assert!(reachable.contains(&Format::AriadIrJson));
            assert!(reachable.contains(&Format::Html));
            assert!(reachable.contains(&Format::Docx));
            assert!(!reachable.contains(&Format::Epub));
            assert!(!reachable.contains(&Format::Markdown));
        }
        other => panic!("expected NoRoute, got {other:?}"),
    }

    // Isolated format with zero edges
    let isolated_err = plan(
        &caps,
        Format::Png,
        Format::Markdown,
        Profile::Editable,
        &engines,
    )
    .unwrap_err();
    match isolated_err {
        PlanError::NoRoute {
            from,
            to,
            reachable,
        } => {
            assert_eq!(from, Format::Png);
            assert_eq!(to, Format::Markdown);
            assert!(reachable.is_empty());
        }
        other => panic!("expected NoRoute, got {other:?}"),
    }
}

#[test]
fn alternatives_search_returns_up_to_two_deduplicated_routes() {
    // 3 competing readers and 1 writer
    let r1 = edge(
        Format::Html,
        Format::AriadIrJson,
        "reader-1",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.95,
            editability: 0.95,
            p50_ms: 100.0,
            peak_mem_mb: 32.0,
            samples: 10,
        }),
    );
    let r2 = edge(
        Format::Html,
        Format::AriadIrJson,
        "reader-2",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.90,
            editability: 0.90,
            p50_ms: 120.0,
            peak_mem_mb: 32.0,
            samples: 10,
        }),
    );
    let r3 = edge(
        Format::Html,
        Format::AriadIrJson,
        "reader-3",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.85,
            editability: 0.85,
            p50_ms: 140.0,
            peak_mem_mb: 32.0,
            samples: 10,
        }),
    );
    let w1 = edge(
        Format::AriadIrJson,
        Format::Docx,
        "writer-1",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.95,
            editability: 0.95,
            p50_ms: 200.0,
            peak_mem_mb: 64.0,
            samples: 10,
        }),
    );

    let caps = synthetic_caps(vec![r1.clone(), r2.clone(), r3.clone(), w1.clone()]);
    let engines = [
        "reader-1".to_owned(),
        "reader-2".to_owned(),
        "reader-3".to_owned(),
        "writer-1".to_owned(),
    ];

    let p = plan(
        &caps,
        Format::Html,
        Format::Docx,
        Profile::Editable,
        &engines,
    )
    .unwrap();
    assert_eq!(p.steps[0].engine, "reader-1");
    // Best route has 2 edges [r1, w1]. Removing r1 finds r2. Removing w1 finds nothing.
    // So exactly 1 alternative is found.
    assert_eq!(p.alternatives.len(), 1);
    assert_eq!(p.alternatives[0].steps[0].engine, "reader-2");

    // Now add a second competing writer w2
    let w2 = edge(
        Format::AriadIrJson,
        Format::Docx,
        "writer-2",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.88,
            editability: 0.88,
            p50_ms: 220.0,
            peak_mem_mb: 64.0,
            samples: 10,
        }),
    );
    let caps_2w = synthetic_caps(vec![
        r1.clone(),
        r2.clone(),
        r3.clone(),
        w1.clone(),
        w2.clone(),
    ]);
    let engines_2w = [
        "reader-1".to_owned(),
        "reader-2".to_owned(),
        "reader-3".to_owned(),
        "writer-1".to_owned(),
        "writer-2".to_owned(),
    ];

    let p_2w = plan(
        &caps_2w,
        Format::Html,
        Format::Docx,
        Profile::Editable,
        &engines_2w,
    )
    .unwrap();
    assert_eq!(p_2w.steps[0].engine, "reader-1");
    assert_eq!(p_2w.steps[1].engine, "writer-1");
    // Removing r1 yields (r2, w1); removing w1 yields (r1, w2)
    assert_eq!(p_2w.alternatives.len(), 2);
    let alt_engines: Vec<(&str, &str)> = p_2w
        .alternatives
        .iter()
        .map(|alt| (alt.steps[0].engine.as_str(), alt.steps[1].engine.as_str()))
        .collect();
    assert!(alt_engines.contains(&("reader-2", "writer-1")));
    assert!(alt_engines.contains(&("reader-1", "writer-2")));
}

#[test]
fn alternatives_ranking_preserves_cost_and_step_order() {
    // Primary 3-hop route: Markdown -> Epub -> AriadIrJson -> Docx
    let hop0_primary = edge(
        Format::Markdown,
        Format::Epub,
        "hop0-primary",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.99,
            editability: 0.99,
            p50_ms: 10.0,
            peak_mem_mb: 16.0,
            samples: 10,
        }),
    );
    let hop1_primary = edge(
        Format::Epub,
        Format::AriadIrJson,
        "hop1-primary",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.99,
            editability: 0.99,
            p50_ms: 10.0,
            peak_mem_mb: 16.0,
            samples: 10,
        }),
    );
    let hop2_primary = edge(
        Format::AriadIrJson,
        Format::Docx,
        "hop2-primary",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.99,
            editability: 0.99,
            p50_ms: 10.0,
            peak_mem_mb: 16.0,
            samples: 10,
        }),
    );

    // Alternative 0 (when hop0_primary is excluded): expensive unmeasured edge for hop 0
    let alt0_slow = edge(
        Format::Markdown,
        Format::Epub,
        "alt0-slow",
        &[Runtime::Local],
        None,
    );

    // Alternative 1 (when hop1_primary is excluded): expensive unmeasured edge for hop 1
    let alt1_slow = edge(
        Format::Epub,
        Format::AriadIrJson,
        "alt1-slow",
        &[Runtime::Local],
        None,
    );

    // Alternative 2 (when hop2_primary, THE LAST EDGE, is excluded): cheap measured edge for hop 2
    let alt2_fast = edge(
        Format::AriadIrJson,
        Format::Docx,
        "alt2-fast",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: 0.98,
            editability: 0.98,
            p50_ms: 10.0,
            peak_mem_mb: 16.0,
            samples: 10,
        }),
    );

    let caps = synthetic_caps(vec![
        hop0_primary,
        hop1_primary,
        hop2_primary,
        alt0_slow,
        alt1_slow,
        alt2_fast,
    ]);
    let engines = [
        "hop0-primary".to_owned(),
        "hop1-primary".to_owned(),
        "hop2-primary".to_owned(),
        "alt0-slow".to_owned(),
        "alt1-slow".to_owned(),
        "alt2-fast".to_owned(),
    ];

    let p = plan(
        &caps,
        Format::Markdown,
        Format::Docx,
        Profile::Editable,
        &engines,
    )
    .unwrap();

    // Primary route must be 3 hops: hop0-primary -> hop1-primary -> hop2-primary
    assert_eq!(p.steps.len(), 3);
    assert_eq!(p.steps[0].engine, "hop0-primary");
    assert_eq!(p.steps[1].engine, "hop1-primary");
    assert_eq!(p.steps[2].engine, "hop2-primary");

    // Alternatives:
    // Excluding hop0 yields candidate 0 (alt0-slow, cost ~1.75).
    // Excluding hop1 yields candidate 1 (alt1-slow, cost ~1.75).
    // Excluding hop2 (last edge) yields candidate 2 (alt2-fast, cost ~0.27).
    // Sorting must place candidate 2 FIRST and retain it under truncate(2).
    assert_eq!(p.alternatives.len(), 2);
    assert_eq!(
        p.alternatives[0].steps[2].engine, "alt2-fast",
        "cheapest alternative produced by excluding the last edge must rank first"
    );
}

#[test]
fn capabilities_validation_catches_hostile_and_invalid_inputs() {
    // Empty edges
    let mut bad_caps = synthetic_caps(vec![]);
    assert_eq!(bad_caps.validate(), Err(CapabilitiesError::EmptyEdges));

    // Wrong version
    bad_caps.edges = vec![edge(
        Format::Markdown,
        Format::Html,
        "core",
        &[Runtime::Local],
        None,
    )];
    bad_caps.version = "ariad-capabilities/1".to_owned();
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::InvalidVersion { .. })
    ));
    bad_caps.version = "ariad-capabilities/0".to_owned();

    // Self-loop
    bad_caps.edges = vec![edge(
        Format::Markdown,
        Format::Markdown,
        "core",
        &[Runtime::Local],
        None,
    )];
    assert_eq!(
        bad_caps.validate(),
        Err(CapabilitiesError::SelfLoop {
            format: Format::Markdown
        })
    );

    // Duplicate edge
    bad_caps.edges = vec![
        edge(
            Format::Markdown,
            Format::Html,
            "core",
            &[Runtime::Local],
            None,
        ),
        edge(
            Format::Markdown,
            Format::Html,
            "core",
            &[Runtime::Local],
            None,
        ),
    ];
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::DuplicateEdge { .. })
    ));

    // Empty engine
    bad_caps.edges = vec![edge(
        Format::Markdown,
        Format::Html,
        "",
        &[Runtime::Local],
        None,
    )];
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::EmptyEngine { edge_index: 0 })
    ));

    // Empty runtime
    let mut edge_no_runtime = edge(Format::Markdown, Format::Html, "core", &[], None);
    edge_no_runtime.runtime = vec![];
    bad_caps.edges = vec![edge_no_runtime];
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::EmptyRuntime { edge_index: 0 })
    ));

    // Duplicate runtime
    let edge_dup_runtime = edge(
        Format::Markdown,
        Format::Html,
        "core",
        &[Runtime::Local, Runtime::Local],
        None,
    );
    bad_caps.edges = vec![edge_dup_runtime];
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::DuplicateRuntime { edge_index: 0 })
    ));

    // NaN fidelity
    bad_caps.edges = vec![edge(
        Format::Markdown,
        Format::Html,
        "core",
        &[Runtime::Local],
        Some(EdgeMetrics {
            fidelity: f64::NAN,
            editability: 0.9,
            p50_ms: 10.0,
            peak_mem_mb: 10.0,
            samples: 1,
        }),
    )];
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::InvalidMetrics { .. })
    ));

    // Out of range fidelity (> 1.0)
    bad_caps.edges[0].metrics = Some(EdgeMetrics {
        fidelity: 1.5,
        editability: 0.9,
        p50_ms: 10.0,
        peak_mem_mb: 10.0,
        samples: 1,
    });
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::InvalidMetrics { .. })
    ));

    // Negative p50_ms
    bad_caps.edges[0].metrics = Some(EdgeMetrics {
        fidelity: 0.9,
        editability: 0.9,
        p50_ms: -5.0,
        peak_mem_mb: 10.0,
        samples: 1,
    });
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::InvalidMetrics { .. })
    ));

    // Zero samples
    bad_caps.edges[0].metrics = Some(EdgeMetrics {
        fidelity: 0.9,
        editability: 0.9,
        p50_ms: 10.0,
        peak_mem_mb: 10.0,
        samples: 0,
    });
    assert!(matches!(
        bad_caps.validate(),
        Err(CapabilitiesError::InvalidMetrics { .. })
    ));
}

#[test]
fn profile_parsing_and_display() {
    assert_eq!("editable".parse::<Profile>(), Ok(Profile::Editable));
    assert_eq!("faithful".parse::<Profile>(), Ok(Profile::Faithful));
    assert_eq!("fast".parse::<Profile>(), Ok(Profile::Fast));
    assert_eq!("private".parse::<Profile>(), Ok(Profile::Private));

    // Strict case-sensitive parsing matching serde
    assert!(matches!(
        "EDITABLE".parse::<Profile>(),
        Err(ProfileParseError(s)) if s == "EDITABLE"
    ));

    assert_eq!(Profile::Editable.to_string(), "editable");
    assert_eq!(Profile::Faithful.to_string(), "faithful");
    assert_eq!(Profile::Fast.to_string(), "fast");
    assert_eq!(Profile::Private.to_string(), "private");

    assert!(matches!(
        "unknown".parse::<Profile>(),
        Err(ProfileParseError(s)) if s == "unknown"
    ));
}

#[test]
fn reachable_formats_helper_lists_targets() {
    let caps = embedded();
    let reachable = reachable_formats(
        caps,
        Format::Markdown,
        Profile::Editable,
        &["ariad-core".to_owned()],
    );
    assert!(reachable.contains(&Format::Html));
    assert!(reachable.contains(&Format::AriadIrJson));
    assert!(!reachable.contains(&Format::Docx));
    assert!(!reachable.contains(&Format::Epub));
}

#[test]
fn reachable_formats_does_not_stop_early_at_png() {
    let caps = synthetic_caps(vec![
        edge(
            Format::Markdown,
            Format::AriadIrJson,
            "core",
            &[Runtime::Local],
            Some(EdgeMetrics {
                fidelity: 1.0,
                editability: 1.0,
                p50_ms: 10.0,
                peak_mem_mb: 16.0,
                samples: 10,
            }),
        ),
        // Cheap PNG edge with engine name sorting first so it pops before HTML
        edge(
            Format::AriadIrJson,
            Format::Png,
            "aaa-png",
            &[Runtime::Local],
            Some(EdgeMetrics {
                fidelity: 1.0,
                editability: 1.0,
                p50_ms: 10.0,
                peak_mem_mb: 16.0,
                samples: 10,
            }),
        ),
        // More expensive HTML edge
        edge(
            Format::AriadIrJson,
            Format::Html,
            "zzz-html",
            &[Runtime::Local],
            None,
        ),
        // Docx reachable ONLY behind Html
        edge(Format::Html, Format::Docx, "core", &[Runtime::Local], None),
    ]);
    let engines = [
        "core".to_owned(),
        "aaa-png".to_owned(),
        "zzz-html".to_owned(),
    ];

    let reachable = reachable_formats(&caps, Format::Markdown, Profile::Editable, &engines);
    assert_eq!(reachable.len(), 4);
    assert!(reachable.contains(&Format::AriadIrJson));
    assert!(reachable.contains(&Format::Png));
    assert!(reachable.contains(&Format::Html));
    assert!(reachable.contains(&Format::Docx));
}
