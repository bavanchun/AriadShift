#![no_main]

use std::time::{Duration, Instant};

use ariad_core::{
    limits::Limits,
    tools::params::{
        redact::{RedactParams, RedactVerifyParams},
        redact_internal::{RedactGeometry, RedactGeometryChar, RedactGeometryPage},
    },
};
use ariad_pdf::{
    ToolCtx,
    redact::{
        matching::{MatchBudget, Matchers},
        plan, rewrite_document,
        scrub::scrub_object_strings,
        stream::trace,
    },
};
use libfuzzer_sys::fuzz_target;

const FIXTURE_BYTES: &[u8] = include_bytes!("../../fixtures/pdf-tools/rotated-pages.pdf");

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > 8192 {
        return;
    }
    let ctx = ToolCtx::with_deadline(Instant::now() + Duration::from_millis(200));
    let limits = Limits {
        max_pages: Some(4),
        max_blocks: 64,
        max_nesting_depth: 16,
        max_decompressed_bytes: 256 * 1024,
        ..Limits::local()
    };

    // Exercise stream tracing on arbitrary input
    let empty_doc = lopdf::Document::new();
    let empty_res = lopdf::Dictionary::new();
    let _ = trace(&empty_doc, &empty_res, data, data.len(), &limits, &ctx);

    if let Ok(params) = serde_json::from_slice::<RedactParams>(data) {
        if params.validate().is_ok() {
            // Exercise Matchers::new
            if let Ok(matchers) = Matchers::new(&params.targets) {
                // Exercise scrub_object_strings
                if let Ok(mut doc) = ariad_pdf::document::open_for_info(FIXTURE_BYTES, &limits) {
                    let mut budget = MatchBudget::default();
                    budget.set_context(&ctx);
                    let _ = scrub_object_strings(&mut doc, &matchers, &mut budget, &ctx);
                }
            }

            // Exercise plan and rewrite_document
            let dummy_geometry = RedactGeometry {
                schema: "ariad-redact-geometry/1".into(),
                source_sha256: [0u8; 32],
                page_count: 1,
                pages: vec![RedactGeometryPage {
                    page: 1,
                    width: 612.0,
                    height: 792.0,
                    rotation: 0,
                    chars: vec![RedactGeometryChar {
                        value: 'A',
                        index: 0,
                        generated: false,
                        origin: Some([72.0, 700.0]),
                        bbox: Some([72.0, 700.0, 80.0, 712.0]),
                    }],
                }],
            };

            if let Ok(plan) = plan(FIXTURE_BYTES, &params, &dummy_geometry, &limits, &ctx) {
                let _ = rewrite_document(
                    FIXTURE_BYTES,
                    &params,
                    &plan,
                    &dummy_geometry,
                    &limits,
                    &ctx,
                );
            }
        }
    }

    if let Ok(verify_params) = serde_json::from_slice::<RedactVerifyParams>(data) {
        let _ = verify_params.validate();
    }
});
