//! Scan page rebuilding preserves original image bytes and page geometry.
use ariad_core::limits::Limits;
use ariad_pdf::{
    ToolCtx, document,
    scan_pages::{ScanPageRegion, split_regions},
    tools::OutputPayload,
};
use std::path::PathBuf;

fn source() -> ariad_pdf::PdfDoc {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    document::open(&std::fs::read(path).unwrap(), &Limits::local()).unwrap()
}
fn images(pdf: &lopdf::Document) -> Vec<Vec<u8>> {
    pdf.objects
        .values()
        .filter_map(|object| {
            let stream = object.as_stream().ok()?;
            (stream.dict.get(b"Subtype").ok()?.as_name().ok()? == b"Image")
                .then(|| stream.content.clone())
        })
        .collect()
}
#[test]
fn duplicates_and_crops_without_reencoding_images() {
    let pdf = source();
    let original = images(pdf.inner());
    let output = split_regions(
        &pdf,
        &[
            ScanPageRegion {
                page: 2,
                rect: Some([0.0, 0.0, 0.5, 1.0]),
            },
            ScanPageRegion {
                page: 2,
                rect: Some([0.5, 0.0, 1.0, 1.0]),
            },
            ScanPageRegion {
                page: 1,
                rect: None,
            },
        ],
        &ToolCtx::default(),
    )
    .unwrap();
    let OutputPayload::Pdf(doc) = output.payload else {
        panic!("PDF output expected");
    };
    assert_eq!(doc.inner().get_pages().len(), 3);
    let derived = images(doc.inner());
    assert!(!derived.is_empty());
    assert!(derived.iter().all(|image| original.contains(image)));
    let pages = doc.inner().get_pages();
    let first = ariad_pdf::xobject::resolve_page_crop_box(doc.inner(), pages[&1]);
    let second = ariad_pdf::xobject::resolve_page_crop_box(doc.inner(), pages[&2]);
    assert!((first.right - second.left).abs() < 0.01);
    assert!((first.width() - second.width()).abs() < 0.01);
}
#[test]
fn rejects_invalid_regions_and_honors_page_limit() {
    let pdf = source();
    let ctx = ToolCtx::default();
    assert!(
        split_regions(
            &pdf,
            &[ScanPageRegion {
                page: 1,
                rect: Some([0.5, 0.0, 0.5, 1.0])
            }],
            &ctx
        )
        .is_err()
    );
    let one_page = ToolCtx::default().with_max_pages(Some(1));
    let regions = [ScanPageRegion {
        page: 2,
        rect: None,
    }; 2];
    assert!(matches!(
        split_regions(&pdf, &regions, &one_page),
        Err(ariad_pdf::PdfError::LimitExceeded(_))
    ));
}

#[test]
fn crops_rotated_pages_in_display_coordinates() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let page = pdf
        .inner_mut()
        .unwrap()
        .get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap();
    page.set(
        "CropBox",
        vec![10.into(), 20.into(), 310.into(), 420.into()],
    );
    page.set("Rotate", 90);
    page.set("UserUnit", 2);
    let regions = [
        ScanPageRegion {
            page: 2,
            rect: Some([0.0, 0.0, 0.5, 1.0]),
        },
        ScanPageRegion {
            page: 2,
            rect: Some([0.5, 0.0, 1.0, 1.0]),
        },
    ];
    let output = split_regions(&pdf, &regions, &ToolCtx::default()).unwrap();
    let OutputPayload::Pdf(doc) = output.payload else {
        panic!("PDF output expected");
    };
    let pages = doc.inner().get_pages();
    assert_eq!(
        ariad_pdf::xobject::resolve_page_crop_box(doc.inner(), pages[&1]).to_array(),
        [10.0, 20.0, 310.0, 220.0]
    );
    assert_eq!(
        ariad_pdf::xobject::resolve_page_crop_box(doc.inner(), pages[&2]).to_array(),
        [10.0, 220.0, 310.0, 420.0]
    );
    assert_eq!(
        ariad_pdf::xobject::resolve_page_rotation(doc.inner(), pages[&1]),
        90
    );
}
