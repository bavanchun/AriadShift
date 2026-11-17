//! Fuzzes `ariad_pdf::fonts::subset_simple_cff` through a PDF wrapper: the fuzz bytes are
//! embedded as the `/FontFile3 /Type1C` program of a simple Type 1 font that a page draws text with.
//!
//! Invariants checked on every input:
//! - The pruner never panics; the 2 s tool-context deadline and the per-font work budget keep
//!   crafted subroutine nesting from running for long.
//! - A font reported as subset has a plain replacement program that is strictly smaller than the
//!   input and still starts with the CFF major version 1.
//! - A skipped font keeps its original program byte for byte.
#![no_main]

use std::time::{Duration, Instant};

use ariad_core::limits::Limits;
use ariad_pdf::{
    ToolCtx, document,
    fonts::{SubsetOutcome, glyph_usage, subset_simple_cff},
};
use libfuzzer_sys::fuzz_target;
use lopdf::{Document, Object, Stream, dictionary};

const MAX_FUZZ_BYTES: usize = 256 * 1024;

fn wrapper(program: &[u8]) -> Option<(Vec<u8>, lopdf::ObjectId)> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let file = doc.add_object(Stream::new(
        dictionary! {"Subtype" => "Type1C"},
        program.to_vec(),
    ));
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "AAAAAA+Fuzz",
        "Flags" => 32,
        "FontFile3" => file,
    });
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "AAAAAA+Fuzz",
        "FirstChar" => 65,
        "LastChar" => 67,
        "Widths" => vec![Object::Integer(500); 3],
        "Encoding" => "WinAnsiEncoding",
        "FontDescriptor" => descriptor,
    });
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 10 10 Td (AB) Tj ET".to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}},
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).ok()?;
    Some((bytes, font))
}

fn program_of(pdf: &Document, font: lopdf::ObjectId) -> Option<Vec<u8>> {
    let font = pdf.get_dictionary(font).ok()?;
    let descriptor = pdf.get_dictionary(font.get(b"FontDescriptor").ok()?.as_reference().ok()?);
    let file = descriptor
        .ok()?
        .get(b"FontFile3")
        .ok()?
        .as_reference()
        .ok()?;
    match pdf.get_object(file).ok()? {
        Object::Stream(stream) => Some(stream.content.clone()),
        _ => None,
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_BYTES {
        return;
    }
    let limits = Limits {
        max_input_bytes: Some(2 * MAX_FUZZ_BYTES as u64),
        max_pages: Some(4),
        max_decompressed_bytes: 4 * 1024 * 1024,
        max_nesting_depth: 16,
        ..Limits::local()
    };
    let Some((bytes, font)) = wrapper(data) else {
        return;
    };
    let Ok(mut pdf) = document::open(&bytes, &limits) else {
        return;
    };
    let ctx = ToolCtx::with_deadline(Instant::now() + Duration::from_secs(2));
    let Ok(usage) = glyph_usage(&pdf, &ctx, &limits) else {
        return;
    };
    match subset_simple_cff(&mut pdf, font, &usage, &limits, &ctx) {
        Ok(SubsetOutcome::Subset { .. }) => {
            let program = program_of(pdf.inner(), font).expect("subset font keeps a program");
            assert!(program.len() < data.len(), "subset program must be smaller");
            assert_eq!(program.first(), Some(&1), "CFF major version");
        }
        Ok(SubsetOutcome::Skipped(_)) => {
            assert_eq!(
                program_of(pdf.inner(), font).as_deref(),
                Some(data),
                "a skipped font keeps its program"
            );
        }
        // Only the tool context may stop the work early.
        Err(_) => {}
    }
});
