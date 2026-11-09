//! PDF/A and PDF/UA compliance manipulation and inspection.

pub mod accessibility;
pub mod fixer;
pub mod fonts;
pub mod icc;
pub mod xmp;

pub use accessibility::{AccessibilityIssue, AccessibilityReport, inspect_accessibility};
pub use fixer::{FixerLevel, fix_pdfa_document};
pub use icc::SRGB_V4_ICC;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(rel)
    }

    #[test]
    fn fixer_fixes_non_compliant_pdf() {
        let fixture_path = fixture("fixtures/pdf/pdfa-non-compliant.pdf");
        let bytes = std::fs::read(&fixture_path).expect("read fixture");
        let mut doc = lopdf::Document::load_mem(&bytes).expect("load pdf");

        fix_pdfa_document(&mut doc, FixerLevel::A2b, 1_700_000_000).expect("fix pdfa-2b");

        let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let catalog = doc.get_object(catalog_id).unwrap().as_dict().unwrap();

        assert!(catalog.has(b"OutputIntents"), "must have OutputIntents");
        assert!(catalog.has(b"Metadata"), "must have Metadata");
        assert!(doc.trailer.has(b"ID"), "trailer must have /ID");

        let meta_id = catalog.get(b"Metadata").unwrap().as_reference().unwrap();
        let meta_stream = doc.get_object(meta_id).unwrap().as_stream().unwrap();
        let decompressed = meta_stream
            .decompressed_content()
            .unwrap_or_else(|_| meta_stream.content.clone());
        let xmp_str = std::str::from_utf8(&decompressed).unwrap();

        assert!(xmp_str.contains("<pdfaid:part>2</pdfaid:part>"));
        assert!(xmp_str.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
    }

    #[test]
    fn accessibility_report_on_sample() {
        let fixture_path = fixture("fixtures/pdf/pdfa-non-compliant.pdf");
        let bytes = std::fs::read(&fixture_path).expect("read fixture");
        let doc = lopdf::Document::load_mem(&bytes).expect("load pdf");

        let report = inspect_accessibility(&doc).expect("inspect");
        assert!(
            !report.compliant,
            "sample without title/display-doc-title is non-compliant"
        );
        assert!(report.is_tagged, "sample has tagged structure");
        assert!(report.has_struct_tree_root);
        assert_eq!(report.lang.as_deref(), Some("en"));
        assert!(!report.displays_doc_title);
    }

    #[test]
    fn fixer_refuses_encrypted_pdf() {
        let mut doc = lopdf::Document::new();
        doc.trailer.set(
            b"Encrypt",
            lopdf::Object::Dictionary(lopdf::Dictionary::new()),
        );

        let err = fix_pdfa_document(&mut doc, FixerLevel::A2b, 1_700_000_000);
        assert!(matches!(err, Err(crate::error::PdfError::Encrypted { .. })));
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use lopdf::{Document, Object, Stream, dictionary};

    fn sample() -> Document {
        Document::load_mem(include_bytes!(
            "../../../../fixtures/pdf/pdfa-non-compliant.pdf"
        ))
        .unwrap()
    }

    #[test]
    fn fixer_preserves_transparency_and_removes_nested_actions() {
        let mut doc = sample();
        let script = doc.add_object(dictionary! { "S" => "JavaScript", "JS" => Object::string_literal("app.alert('probe')") });
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let catalog = doc.get_object_mut(root).unwrap().as_dict_mut().unwrap();
        catalog.set("OpenAction", script);
        catalog.set("Names", dictionary! { "JavaScript" => dictionary! { "Names" => vec![Object::string_literal("probe"), script.into()] } });
        let page = doc.page_iter().next().unwrap();
        let group = dictionary! { "S" => "Transparency", "CS" => "DeviceRGB" };
        doc.get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Group", group.clone());
        fix_pdfa_document(&mut doc, FixerLevel::A2b, 1_700_000_000).unwrap();
        assert!(!doc.catalog().unwrap().has(b"OpenAction"));
        assert!(
            !doc.catalog()
                .unwrap()
                .get(b"Names")
                .unwrap()
                .as_dict()
                .unwrap()
                .has(b"JavaScript")
        );
        assert!(matches!(doc.get_object(script).unwrap(), Object::Null));
        assert_eq!(
            doc.get_dictionary(page).unwrap().get(b"Group").unwrap(),
            &Object::Dictionary(group)
        );
    }

    #[test]
    fn fixer_refuses_attachments_for_pdfa2_but_allows_pdfa3() {
        let mut doc = sample();
        let annotation =
            doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "FileAttachment" });
        let annotations = doc.add_object(vec![Object::Reference(annotation)]);
        let page = doc.page_iter().next().unwrap();
        doc.get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", annotations);
        for level in [FixerLevel::A2b, FixerLevel::A2u] {
            assert!(matches!(
                fix_pdfa_document(&mut doc.clone(), level, 1_700_000_000),
                Err(crate::PdfError::InvalidParams(_))
            ));
        }
        fix_pdfa_document(&mut doc, FixerLevel::A3b, 1_700_000_000).unwrap();
    }

    #[test]
    fn fixer_refuses_unknown_fonts_and_unknown_unicode_mappings() {
        let mut doc = sample();
        let font = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Symbol" },
        );
        assert!(fix_pdfa_document(&mut doc.clone(), FixerLevel::A2b, 1_700_000_000).is_err());
        doc.objects.remove(&font);
        for object in doc.objects.values_mut() {
            if let Ok(dict) = object.as_dict_mut()
                && dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Font")
            {
                dict.remove(b"ToUnicode");
            }
        }
        assert!(fix_pdfa_document(&mut doc, FixerLevel::A2u, 1_700_000_000).is_err());
    }

    #[test]
    fn fixer_refuses_incompatible_font_advances() {
        let mut doc = sample();
        doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding", "FirstChar" => 65_i64, "LastChar" => 65_i64,
            "Widths" => vec![Object::Integer(1234)] },
        );
        let error = fix_pdfa_document(&mut doc, FixerLevel::A2b, 1_700_000_000).unwrap_err();
        assert!(error.to_string().contains("font widths"));
    }

    #[test]
    fn accessibility_checks_inline_figures_and_xmp_only_title() {
        let mut doc = sample();
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let metadata = doc.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:d="http://purl.org/dc/elements/1.1/"><d:title><r:Alt><r:li xml:lang="x-default">Accessible &amp; title</r:li></r:Alt></d:title></r:Description></r:RDF></x:xmpmeta>"#.to_vec()));
        let info = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
        doc.get_object_mut(info)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"Title");
        let catalog = doc.get_object_mut(root).unwrap().as_dict_mut().unwrap();
        catalog.set("Metadata", metadata);
        catalog.set(
            "StructTreeRoot",
            dictionary! { "K" => dictionary! { "Type" => "StructElem", "S" => "Figure" } },
        );
        let report = inspect_accessibility(&doc).unwrap();
        assert!(report.has_title);
        assert_eq!(report.title.as_deref(), Some("Accessible & title"));
        assert!(report.figures_missing_alt > 0);
        assert!(!report.compliant);
    }
}
