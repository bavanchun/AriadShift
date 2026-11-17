#![cfg(feature = "codecs")]
//! Pruning of embedded `/Type1C` programs on the Gao highlights fixture, which
//! carries five name-keyed CFF fonts with WinAnsi encodings.

use ariad_core::limits::Limits;
use ariad_core::tools::params::{CompressParams, CompressPreset};
use ariad_pdf::{
    InputDoc, PdfDoc, ToolCtx,
    fonts::{SubsetOutcome, glyph_usage, subset_simple_cff},
    tools::{OutputPayload, compress},
};
use lopdf::{Document, Object, ObjectId, dictionary};

const GAO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/pdf/pd-en-gao-08-35-highlights.pdf"
);

fn gao() -> Vec<u8> {
    std::fs::read(GAO).unwrap()
}

/// Simple Type 1 font dictionaries whose descriptor embeds a `FontFile3`.
fn cff_fonts(pdf: &Document) -> Vec<(ObjectId, ObjectId)> {
    pdf.objects
        .iter()
        .filter_map(|(&id, object)| {
            let font = object.as_dict().ok()?;
            if font.get(b"Subtype").and_then(Object::as_name).ok()? != b"Type1" {
                return None;
            }
            let descriptor = pdf
                .get_dictionary(font.get(b"FontDescriptor").ok()?.as_reference().ok()?)
                .ok()?;
            let program = descriptor.get(b"FontFile3").ok()?.as_reference().ok()?;
            Some((id, program))
        })
        .collect()
}

/// Stored bytes of every `FontFile3` program referenced by a font.
fn program_bytes(pdf: &Document) -> usize {
    cff_fonts(pdf)
        .into_iter()
        .map(|(_, program)| match pdf.get_object(program) {
            Ok(Object::Stream(stream)) => stream.content.len(),
            _ => 0,
        })
        .sum()
}

fn compress_gao(bytes: Vec<u8>) -> Vec<u8> {
    let params = CompressParams {
        preset: Some(CompressPreset::Ebook),
        ..Default::default()
    };
    let (_, mut docs) = compress::run_inputs(
        &[InputDoc::new("gao.pdf", bytes)],
        &params,
        &ToolCtx::default(),
    )
    .unwrap();
    let OutputPayload::Bytes(output) = docs.remove(0).payload else {
        panic!("expected PDF")
    };
    output
}

fn text(bytes: &[u8]) -> String {
    let read = ariad_core::reader::pdf::read(bytes, &Limits::local()).unwrap();
    ariad_core::writer::markdown::write(&read.document).content
}

fn open(bytes: &[u8]) -> PdfDoc {
    ariad_pdf::document::open(bytes, &Limits::local()).unwrap()
}

#[test]
fn compress_prunes_the_gao_type1c_programs_and_keeps_the_text() {
    let input = gao();
    let before = program_bytes(&Document::load_mem(&input).unwrap());
    let output = compress_gao(input.clone());
    let after = program_bytes(&Document::load_mem(&output).unwrap());
    assert_eq!(cff_fonts(&Document::load_mem(&output).unwrap()).len(), 5);
    println!("FontFile3 bytes: {before} -> {after}");
    assert!(after * 100 < before * 70, "{before} -> {after}");
    assert_eq!(text(&input), text(&output));
}

fn subset_all(pdf: &mut PdfDoc) -> Vec<SubsetOutcome> {
    let limits = Limits::local();
    let usage = glyph_usage(pdf, &ToolCtx::default(), &limits).unwrap();
    let fonts = cff_fonts(pdf.inner());
    fonts
        .into_iter()
        .map(|(id, _)| subset_simple_cff(pdf, id, &usage, &limits).unwrap())
        .collect()
}

#[test]
fn every_gao_font_is_pruned_and_the_descriptor_charset_is_dropped() {
    let mut pdf = open(&gao());
    let outcomes = subset_all(&mut pdf);
    assert_eq!(outcomes.len(), 5);
    // One program is already tight: every glyph in it is drawn by the page text.
    let pruned = outcomes
        .iter()
        .filter(|o| matches!(o, SubsetOutcome::Subset { glyphs } if *glyphs > 1))
        .count();
    assert_eq!(pruned, 4, "{outcomes:?}");
    assert!(
        outcomes.contains(&SubsetOutcome::Skipped("no unused glyphs")),
        "{outcomes:?}"
    );
    let mut rewritten = 0;
    for (font, program) in cff_fonts(pdf.inner()) {
        let descriptor = descriptor_of(pdf.inner(), font);
        let descriptor = pdf.inner().get_dictionary(descriptor).unwrap();
        // The untouched font keeps its original descriptor, `/CharSet` included.
        if descriptor.has(b"CharSet") {
            continue;
        }
        rewritten += 1;
        let stream = pdf
            .inner()
            .get_object(program)
            .unwrap()
            .as_stream()
            .unwrap();
        assert_eq!(
            stream.dict.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Type1C"
        );
        assert!(descriptor.has(b"FontName"));
    }
    assert_eq!(rewritten, 4);
}

/// Rewrite the fixture, then return the reason the first CFF font was skipped.
fn skip_reason(
    edit: impl FnOnce(&mut Document, ObjectId, ObjectId),
) -> (&'static str, usize, usize) {
    let mut source = Document::load_mem(&gao()).unwrap();
    let (font, program) = cff_fonts(&source).into_iter().next().unwrap();
    edit(&mut source, font, program);
    let mut bytes = Vec::new();
    source.save_to(&mut bytes).unwrap();
    let mut pdf = open(&bytes);
    let limits = Limits::local();
    let usage = glyph_usage(&pdf, &ToolCtx::default(), &limits).unwrap();
    let before = program_bytes(pdf.inner());
    let outcome = subset_simple_cff(&mut pdf, font, &usage, &limits).unwrap();
    let SubsetOutcome::Skipped(reason) = outcome else {
        panic!("expected a skip, got {outcome:?}")
    };
    (reason, before, program_bytes(pdf.inner()))
}

fn descriptor_of(pdf: &Document, font: ObjectId) -> ObjectId {
    pdf.get_dictionary(font)
        .unwrap()
        .get(b"FontDescriptor")
        .unwrap()
        .as_reference()
        .unwrap()
}

#[test]
fn opentype_wrapped_programs_are_left_alone() {
    let (reason, before, after) = skip_reason(|pdf, _, program| {
        let Object::Stream(stream) = pdf.get_object_mut(program).unwrap() else {
            panic!()
        };
        stream.dict.set("Subtype", "OpenType");
    });
    assert_eq!(reason, "font program is not Type1C");
    assert_eq!(before, after);
}

#[test]
fn cid_keyed_subtype_is_left_alone() {
    let (reason, ..) = skip_reason(|pdf, _, program| {
        let Object::Stream(stream) = pdf.get_object_mut(program).unwrap() else {
            panic!()
        };
        stream.dict.set("Subtype", "CIDFontType0C");
    });
    assert_eq!(reason, "font program is not Type1C");
}

#[test]
fn corrupt_programs_are_left_alone() {
    let (reason, before, after) = skip_reason(|pdf, _, program| {
        let Object::Stream(stream) = pdf.get_object_mut(program).unwrap() else {
            panic!()
        };
        stream.dict.remove(b"Filter");
        stream.dict.remove(b"DecodeParms");
        stream.set_plain_content(vec![1, 0, 4, 4, 0, 1, 1, 1, 0, 0, 0, 0]);
    });
    assert_eq!(reason, "unsupported CFF program");
    assert_eq!(before, after);
}

#[test]
fn unknown_base_encodings_are_left_alone() {
    let (reason, ..) = skip_reason(|pdf, font, _| {
        pdf.get_object_mut(font)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Encoding", "SymbolEncoding");
    });
    assert_eq!(reason, "unsupported font encoding");
}

#[test]
fn malformed_differences_are_left_alone() {
    let (reason, ..) = skip_reason(|pdf, font, _| {
        let encoding =
            dictionary! {"Type" => "Encoding", "Differences" => vec![Object::Name(b"A".to_vec())]};
        pdf.get_object_mut(font)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Encoding", encoding);
    });
    assert_eq!(reason, "invalid differences");
}

#[test]
fn ambiguous_glyph_scan_leaves_every_font_alone() {
    let (reason, before, after) = skip_reason(|pdf, _, _| {
        let page = pdf.get_pages().values().next().copied().unwrap();
        let content = pdf.add_object(lopdf::Stream::new(
            dictionary! {},
            b"BT /Unknown 9 Tf (x) Tj ET".to_vec(),
        ));
        let page = pdf.get_object_mut(page).unwrap().as_dict_mut().unwrap();
        let mut list = match page.get(b"Contents").unwrap() {
            Object::Array(items) => items.clone(),
            other => vec![other.clone()],
        };
        list.push(Object::Reference(content));
        page.set("Contents", list);
    });
    assert_eq!(reason, "ambiguous glyph scan");
    assert_eq!(before, after);
}

#[test]
fn fonts_listed_in_form_default_resources_are_left_alone() {
    let (reason, ..) = skip_reason(|pdf, font, _| {
        let form = pdf
            .add_object(dictionary! {"DR" => dictionary! {"Font" => dictionary! {"Fx" => font}}});
        let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
        pdf.get_object_mut(root)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("AcroForm", form);
    });
    assert_eq!(reason, "form default resource");
}

#[test]
fn shared_programs_and_descriptors_are_left_alone() {
    let (reason, ..) = skip_reason(|pdf, font, _| {
        let descriptor = descriptor_of(pdf, font);
        let twin =
            dictionary! {"Type" => "Font", "Subtype" => "Type1", "FontDescriptor" => descriptor};
        pdf.add_object(twin);
    });
    assert_eq!(reason, "shared font program");
}

#[test]
fn pruned_output_is_a_valid_pdf_with_the_same_page_count() {
    let output = compress_gao(gao());
    let pdf = Document::load_mem(&output).unwrap();
    assert_eq!(
        pdf.get_pages().len(),
        Document::load_mem(&gao()).unwrap().get_pages().len()
    );
}
