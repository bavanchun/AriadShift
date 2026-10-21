use std::collections::BTreeSet;

use ariad_core::{
    format::Format,
    planner::{self, Profile},
};
use ariad_host::convert::{DocumentFormat, executor_edges, route_from_plan};

#[test]
fn every_executor_edge_appears_in_embedded_capabilities_and_vice_versa() {
    let caps = planner::embedded();

    let caps_edges: BTreeSet<(&'static str, &'static str, String)> = caps
        .edges
        .iter()
        .map(|e| (e.from.id(), e.to.id(), e.engine.clone()))
        .collect();

    let exec_edges: BTreeSet<(&'static str, &'static str, String)> = executor_edges()
        .into_iter()
        .map(|(from, to, engine)| (from.id(), to.id(), engine.to_owned()))
        .collect();

    assert_eq!(
        caps_edges, exec_edges,
        "mismatch between capabilities.json edges and host executor edges"
    );
    assert_eq!(
        caps.edges.len(),
        8,
        "expected exactly 8 capability edges in initial bootstrap capabilities"
    );
}

#[test]
fn every_non_identical_format_pair_can_be_planned_and_resolved_to_executor_route() {
    let caps = planner::embedded();
    let doc_formats = [
        DocumentFormat::Markdown,
        DocumentFormat::Html,
        DocumentFormat::Docx,
        DocumentFormat::Epub,
    ];
    let engines = [String::from("ariad-core"), String::from("pandoc")];

    for &from in &doc_formats {
        for &to in &doc_formats {
            if from == to {
                continue;
            }
            let plan = planner::plan(
                caps,
                Format::from(from),
                Format::from(to),
                Profile::Editable,
                &engines,
            )
            .expect("planning must succeed for supported document formats");

            assert_eq!(
                plan.steps.len(),
                2,
                "planned route from {from:?} to {to:?} must consist of exactly 2 steps (reader + writer)"
            );

            let route = route_from_plan(&plan)
                .expect("executor must be able to construct an execution route from plan");
            assert_eq!(route.input_format, from);
            assert_eq!(route.output_format, to);
        }
    }
}

#[test]
fn reader_and_writer_edge_all_arrays_are_exhaustive() {
    for &edge in ariad_host::convert::ReaderEdge::ALL {
        match edge {
            ariad_host::convert::ReaderEdge::NativeMarkdown => {}
            ariad_host::convert::ReaderEdge::NativeHtml => {}
            ariad_host::convert::ReaderEdge::PandocDocx => {}
            ariad_host::convert::ReaderEdge::PandocEpub => {}
        }
    }
    assert!(
        ariad_host::convert::ReaderEdge::ALL
            .contains(&ariad_host::convert::ReaderEdge::NativeMarkdown)
    );
    assert!(
        ariad_host::convert::ReaderEdge::ALL.contains(&ariad_host::convert::ReaderEdge::NativeHtml)
    );
    assert!(
        ariad_host::convert::ReaderEdge::ALL.contains(&ariad_host::convert::ReaderEdge::PandocDocx)
    );
    assert!(
        ariad_host::convert::ReaderEdge::ALL.contains(&ariad_host::convert::ReaderEdge::PandocEpub)
    );

    for &edge in ariad_host::convert::WriterEdge::ALL {
        match edge {
            ariad_host::convert::WriterEdge::NativeMarkdown => {}
            ariad_host::convert::WriterEdge::NativeHtml => {}
            ariad_host::convert::WriterEdge::PandocDocx => {}
            ariad_host::convert::WriterEdge::PandocEpub => {}
        }
    }
    assert!(
        ariad_host::convert::WriterEdge::ALL
            .contains(&ariad_host::convert::WriterEdge::NativeMarkdown)
    );
    assert!(
        ariad_host::convert::WriterEdge::ALL.contains(&ariad_host::convert::WriterEdge::NativeHtml)
    );
    assert!(
        ariad_host::convert::WriterEdge::ALL.contains(&ariad_host::convert::WriterEdge::PandocDocx)
    );
    assert!(
        ariad_host::convert::WriterEdge::ALL.contains(&ariad_host::convert::WriterEdge::PandocEpub)
    );
}
