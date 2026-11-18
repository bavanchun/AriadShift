#![no_main]

use std::time::{Duration, Instant};

use ariad_core::{
    limits::Limits,
    tools::params::{
        CropRect,
        redact::{RedactParams, RedactVerifyParams, RedactionScope, RedactionTarget},
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
        verify_image_pixels, verify_object_graph,
    },
};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};

const FIXTURE_BYTES: &[u8] = include_bytes!("../../fixtures/pdf-tools/rotated-pages.pdf");

/// Two pages sharing one image through resources inherited from the page tree, so mutated
/// inputs reach the image rewrite and its clone-and-repoint path.
fn image_fixture() -> Vec<u8> {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 8,
            "Height" => 8,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        (0..64u8).map(|n| n.wrapping_mul(4)).collect::<Vec<u8>>(),
    ));
    let mut kids = Vec::new();
    for content in ["q 612 0 0 792 0 0 cm /Im0 Do Q", "q 100 0 0 100 0 0 cm /Im0 Do Q"] {
        let contents = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => contents,
        })));
    }
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => 2,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
        }),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialize the image fixture");
    bytes
}

/// Applies `(offset, value)` triples from `data` to a copy of a fixture, or uses `data` as
/// the whole document when it already looks like one. The first byte picks the fixture: the
/// rotated-pages document, or the image fixture when it is odd.
fn mutate(data: &[u8]) -> Vec<u8> {
    if data.starts_with(b"%PDF-") {
        return data.to_vec();
    }
    let (selector, data) = data.split_first().unwrap_or((&0, &[]));
    let mut bytes = if selector & 1 == 1 {
        image_fixture()
    } else {
        FIXTURE_BYTES.to_vec()
    };
    for &[low, high, value] in data.as_chunks::<3>().0 {
        let position = usize::from(u16::from_le_bytes([low, high]));
        let offset = (position * bytes.len()) >> 16;
        bytes[offset] = value;
    }
    bytes
}

/// Geometry evidence bound to `bytes`, so planning and rewriting accept the document.
fn geometry(bytes: &[u8], limits: &Limits) -> Option<RedactGeometry> {
    let count = ariad_pdf::document::open(bytes, limits)
        .ok()?
        .inner()
        .get_pages()
        .len();
    Some(RedactGeometry {
        schema: "ariad-redact-geometry/1".into(),
        source_sha256: Sha256::digest(bytes).into(),
        page_count: count,
        pages: (1..=count as u32)
            .map(|page| RedactGeometryPage {
                page,
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
            })
            .collect(),
    })
}

/// Rectangle, text and whole-document scope used for mutated documents.
fn default_params() -> RedactParams {
    RedactParams {
        targets: vec![
            RedactionTarget::Rectangle {
                page: 1,
                rect: CropRect {
                    left: 50.0,
                    bottom: 50.0,
                    right: 300.0,
                    top: 400.0,
                },
            },
            RedactionTarget::Exact { text: "A".into() },
        ],
        color: None,
        overlay_text: None,
        scope: RedactionScope::default(),
        invalidate_signatures: true,
    }
}

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

    let (params, document) = match serde_json::from_slice::<RedactParams>(data) {
        Ok(params) => {
            if params.validate().is_ok()
                && let Ok(matchers) = Matchers::new(&params.targets)
                && let Ok(mut doc) = ariad_pdf::document::open_for_info(FIXTURE_BYTES, &limits)
            {
                let mut budget = MatchBudget::default();
                budget.set_context(&ctx);
                let _ = scrub_object_strings(&mut doc, &matchers, &mut budget, &ctx);
            }
            (params, FIXTURE_BYTES.to_vec())
        }
        Err(_) => (default_params(), mutate(data)),
    };

    // Exercise plan, rewrite_document and the verifiers on the candidate
    if params.validate().is_ok()
        && let Some(geometry) = geometry(&document, &limits)
        && let Ok(plan) = plan(&document, &params, &geometry, &limits, &ctx)
        && let Ok(candidate) = rewrite_document(&document, &params, &plan, &geometry, &limits, &ctx)
    {
        let _ = verify_image_pixels(&candidate, &plan, &limits, &ctx);
        let _ = verify_object_graph(&candidate, &params, &limits, &ctx);
    }

    if let Ok(verify_params) = serde_json::from_slice::<RedactVerifyParams>(data) {
        let _ = verify_params.validate();
    }
});
