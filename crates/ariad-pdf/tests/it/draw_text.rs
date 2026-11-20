#![cfg(feature = "codecs")]

//! Text subset and Unicode extraction tests.

use ariad_core::{limits::Limits, tools::params::FontFace};
use ariad_pdf::{
    ToolCtx, document,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        text::{self, TextAlign, TextLayout, TextStyle},
    },
    writer,
};
use serde_json::json;
use std::path::Path;

fn collect_text(value: &serde_json::Value, text: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|v| v.as_str()) == Some("text")
                && let Some(value) = map.get("text").and_then(|v| v.as_str())
            {
                text.push_str(value);
            }
            for value in map.values() {
                collect_text(value, text);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_text(item, text);
            }
        }
        _ => {}
    }
}

#[test]
fn vietnamese_text_round_trips_on_rotated_and_unrotated_pages() {
    let message = "Bản nháp — Tài liệu mật: Đường Hưng Đạo, số 1, ₫";
    for fixture in ["multipage-20.pdf", "rotated-pages.pdf"] {
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf-tools")
                .join(fixture),
        )
        .unwrap();
        let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
        let page = if fixture == "rotated-pages.pdf" {
            *pdf.inner().get_pages().get(&3).unwrap()
        } else {
            *pdf.inner().get_pages().values().next().unwrap()
        };
        let geometry = PageGeometry::from_page(pdf.inner(), page).unwrap();
        let mut plan = FontPlan::new(FontFace::Serif);
        let layout = TextLayout::prepare(&mut plan, message, 12.0, None, 1.2).unwrap();
        let font = plan.embed(pdf.inner_mut().unwrap()).unwrap();
        let style = TextStyle {
            size_pt: 12.0,
            color: serde_json::from_value(json!("#000000")).unwrap(),
            opacity: serde_json::from_value(json!(1.0)).unwrap(),
            stroke: None,
            align: TextAlign::Left,
            rotation_deg: 0.0,
        };
        text::draw(
            &mut pdf,
            page,
            &geometry,
            &font,
            &layout,
            &style,
            30.0,
            30.0,
            Marking::Content,
            LayerPosition::Over,
            &ToolCtx::new(),
        )
        .unwrap();
        let output = writer::save(&mut pdf, &ToolCtx::new()).unwrap();
        let result = ariad_core::reader::pdf::read(&output, &Limits::local()).unwrap();
        let mut extracted = String::new();
        collect_text(
            &serde_json::to_value(result.document.body).unwrap(),
            &mut extracted,
        );
        assert!(extracted.contains(message), "{fixture}: {extracted:?}");
    }
}

#[test]
fn multiple_pages_share_a_subset_and_serialize_deterministically() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let render = || {
        let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
        let pages: Vec<_> = pdf.inner().get_pages().values().take(2).copied().collect();
        let mut plan = FontPlan::new(FontFace::SerifBold);
        let first = TextLayout::prepare(&mut plan, "Trang một", 14.0, None, 1.2).unwrap();
        let second = TextLayout::prepare(&mut plan, "Trang hai", 14.0, None, 1.2).unwrap();
        let font = plan.embed(pdf.inner_mut().unwrap()).unwrap();
        let style = TextStyle {
            size_pt: 14.0,
            color: serde_json::from_value(json!("#b00020")).unwrap(),
            opacity: serde_json::from_value(json!(0.5)).unwrap(),
            stroke: None,
            align: TextAlign::Center,
            rotation_deg: 20.0,
        };
        for (page, layout) in pages.into_iter().zip([first, second]) {
            let geometry = PageGeometry::from_page(pdf.inner(), page).unwrap();
            text::draw(
                &mut pdf,
                page,
                &geometry,
                &font,
                &layout,
                &style,
                30.0,
                30.0,
                Marking::Watermark,
                LayerPosition::Over,
                &ToolCtx::new(),
            )
            .unwrap();
        }
        writer::save(&mut pdf, &ToolCtx::new()).unwrap()
    };
    assert_eq!(render(), render());
}

#[test]
fn shared_glyph_characters_keep_distinct_unicode_values() {
    let message = "עﬠ";
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let page = *pdf.inner().get_pages().values().next().unwrap();
    let geometry = PageGeometry::from_page(pdf.inner(), page).unwrap();
    let mut plan = FontPlan::new(FontFace::Serif);
    let layout = TextLayout::prepare(&mut plan, message, 20.0, None, 1.2).unwrap();
    let font = plan.embed(pdf.inner_mut().unwrap()).unwrap();
    assert!(
        pdf.inner()
            .get_dictionary(font.id)
            .unwrap()
            .get(b"Encoding")
            .unwrap()
            .as_reference()
            .is_ok()
    );
    let style = TextStyle {
        size_pt: 20.0,
        color: serde_json::from_value(json!("#000000")).unwrap(),
        opacity: serde_json::from_value(json!(1.0)).unwrap(),
        stroke: None,
        align: TextAlign::Left,
        rotation_deg: 0.0,
    };
    text::draw(
        &mut pdf,
        page,
        &geometry,
        &font,
        &layout,
        &style,
        30.0,
        30.0,
        Marking::Content,
        LayerPosition::Over,
        &ToolCtx::new(),
    )
    .unwrap();
    let output = writer::save(&mut pdf, &ToolCtx::new()).unwrap();
    let result = ariad_core::reader::pdf::read(&output, &Limits::local()).unwrap();
    let mut extracted = String::new();
    collect_text(
        &serde_json::to_value(result.document.body).unwrap(),
        &mut extracted,
    );
    assert!(extracted.contains(message), "{extracted:?}");
}
