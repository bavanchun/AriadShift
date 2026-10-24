use std::collections::BTreeSet;

use ariad_core::{
    format::Format,
    planner::{self, Profile},
};
use ariad_host::convert::{executor_edges, route_from_plan};

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
        exec_edges.len(),
        "duplicate capability edges"
    );
}

#[test]
fn every_plannable_format_pair_resolves_to_an_executor_route() {
    let caps = planner::embedded();
    let engines = caps.registered_engines();
    let mut planned = 0;

    for &from in Format::ALL {
        for &to in Format::ALL {
            if from == to || !from.is_user_facing() || !to.is_user_facing() {
                continue;
            }
            let Ok(plan) = planner::plan(caps, from, to, Profile::Editable, &engines) else {
                continue;
            };
            planned += 1;
            let route = route_from_plan(&plan).unwrap_or_else(|| {
                panic!("planned route {from:?} -> {to:?} has no executor route: {plan:?}")
            });
            assert_eq!(route.input_format, from);
            assert_eq!(route.output_format, to);
            for alternative in &plan.alternatives {
                assert!(
                    route_from_plan(alternative).is_some(),
                    "alternative for {from:?} -> {to:?} has no executor route"
                );
            }
        }
    }
    assert!(planned > 12, "only {planned} format pairs are plannable");
}

#[test]
fn the_planner_chains_office_pdf_and_ir_routes() {
    let caps = planner::embedded();
    let engines = caps.registered_engines();
    let route = |from, to| -> Vec<String> {
        planner::plan(caps, from, to, Profile::Editable, &engines)
            .unwrap_or_else(|error| panic!("{from:?} -> {to:?}: {error}"))
            .steps
            .iter()
            .map(|step| format!("{}-{}->{}", step.from.id(), step.engine, step.to.id()))
            .collect()
    };

    assert_eq!(
        route(Format::Doc, Format::Markdown),
        [
            "doc-libreoffice->docx",
            "docx-pandoc->ariad-ir+json",
            "ariad-ir+json-ariad-core->markdown"
        ]
    );
    assert_eq!(
        route(Format::Markdown, Format::Pdf),
        [
            "markdown-ariad-core->ariad-ir+json",
            "ariad-ir+json-typst->pdf"
        ]
    );
    assert_eq!(
        route(Format::Pdf, Format::Markdown),
        [
            "pdf-pdf->ariad-ir+json",
            "ariad-ir+json-ariad-core->markdown"
        ]
    );
    assert_eq!(route(Format::Pptx, Format::Pdf), ["pptx-libreoffice->pdf"]);
    assert_eq!(route(Format::Docx, Format::Pdf), ["docx-libreoffice->pdf"]);
    assert_eq!(
        route(Format::Markdown, Format::AriadIrJson),
        ["markdown-ariad-core->ariad-ir+json"]
    );
    assert_eq!(
        route(Format::AriadIrJson, Format::Docx),
        ["ariad-ir+json-pandoc->docx"]
    );

    // Without LibreOffice, DOCX still reaches PDF through IR and Typst.
    let without_office: Vec<String> = engines
        .iter()
        .filter(|engine| *engine != "libreoffice")
        .cloned()
        .collect();
    let fallback = planner::plan(
        caps,
        Format::Docx,
        Format::Pdf,
        Profile::Editable,
        &without_office,
    )
    .expect("docx reaches pdf without LibreOffice");
    assert_eq!(
        fallback.steps.last().map(|s| s.engine.as_str()),
        Some("typst")
    );
    let slides_fallback = planner::plan(
        caps,
        Format::Pptx,
        Format::Pdf,
        Profile::Editable,
        &without_office,
    )
    .expect("Docling and Typst can convert slides without LibreOffice");
    assert_eq!(slides_fallback.steps[0].engine, "docling");
    assert_eq!(slides_fallback.steps.last().unwrap().engine, "typst");
}

#[test]
fn docling_inputs_plan_through_extraction_mapping_and_a_writer() {
    let caps = planner::embedded();
    let engines = [
        String::from("ariad-core"),
        String::from("pandoc"),
        String::from("docling"),
    ];
    let inputs = [
        Format::Pdf,
        Format::Pptx,
        Format::Xlsx,
        Format::Png,
        Format::Jpeg,
        Format::Tiff,
        Format::Webp,
    ];
    let outputs = [Format::Markdown, Format::Html, Format::Docx, Format::Epub];
    for &from in &inputs {
        for &to in &outputs {
            let plan = planner::plan(caps, from, to, Profile::Editable, &engines)
                .expect("Docling inputs reach every writer");
            assert_eq!(plan.steps.len(), 3, "{from:?} -> {to:?}");
            assert_eq!(plan.steps[0].engine, "docling");
            assert_eq!(plan.steps[1].from, Format::DoclingJson);
            let route = route_from_plan(&plan).expect("three-step plans resolve to a route");
            assert_eq!(route.input_format, from);
            assert_eq!(route.output_format, to);
            assert_eq!(route.steps[0].engine, "docling");
        }
    }

    let plan = planner::plan(
        caps,
        Format::DoclingJson,
        Format::Markdown,
        Profile::Editable,
        &engines[..1],
    )
    .expect("DoclingDocument JSON converts without any engine");
    let route = route_from_plan(&plan).expect("mapper route");
    assert_eq!(route.input_format, Format::DoclingJson);
    assert!(route.steps.iter().all(|step| step.engine != "docling"));

    let docx = planner::plan(
        caps,
        Format::Docx,
        Format::Markdown,
        Profile::Editable,
        &engines,
    )
    .expect("DOCX still converts");
    assert_eq!(
        docx.steps[0].engine, "pandoc",
        "measured Pandoc edges win over the unmeasured Docling route"
    );
}
