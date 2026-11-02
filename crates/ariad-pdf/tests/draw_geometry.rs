//! Geometry against the rotated-page fixture.

use ariad_core::{limits::Limits, tools::params::Anchor};
use ariad_pdf::{document, draw::geometry::PageGeometry};
use std::path::Path;

#[test]
fn visual_coordinates_follow_the_rotated_crop_box() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/rotated-pages.pdf"),
    )
    .unwrap();
    let pdf = document::open(&bytes, &Limits::local()).unwrap();
    let pages = pdf.inner().get_pages();
    let normal = PageGeometry::from_page(pdf.inner(), pages[&1]).unwrap();
    let upside_down = PageGeometry::from_page(pdf.inner(), pages[&3]).unwrap();
    let quarter_turn = PageGeometry::from_page(pdf.inner(), pages[&4]).unwrap();
    assert!((normal.width_pt - upside_down.width_pt).abs() < 0.01);
    assert!((normal.width_pt - quarter_turn.height_pt).abs() < 0.01);
    assert!((normal.height_pt - quarter_turn.width_pt).abs() < 0.01);
    let (x, y) = quarter_turn
        .place(Anchor::BottomCenter, 100.0, 20.0, 24.0, 0.0, 0.0)
        .unwrap();
    assert!((x - (quarter_turn.width_pt - 100.0) / 2.0).abs() < 0.001);
    assert_eq!(y, 24.0);
    let (ux, uy) = quarter_turn.visual_to_user.transform_point(x, y);
    assert!((ux - 24.0).abs() < 0.001);
    assert!((uy - (normal.height_pt - x)).abs() < 0.001);
}
