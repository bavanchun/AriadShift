//! SVG image reader producing an IR figure asset for Typst rendering.

use sha2::{Digest, Sha256};

use crate::{
    format::Format,
    ir::{Asset, AssetRef, Block, Document, Inline, Metadata},
    limits::{Limits, LimitsError},
    reader::common::{ReadOutput, nfc},
};

/// Errors that can occur when reading SVG into IR.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("input exceeds byte limit (actual {actual}, limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("nesting too deep (limit {limit})")]
    NestingTooDeep { limit: u16 },
    #[error("XML parsing error: {0}")]
    Xml(String),
    #[error("invalid SVG: no <svg> root element found")]
    NoSvgElement,
}

/// Reads SVG bytes into an IR document containing a [`Block::Figure`].
pub fn read(bytes: &[u8], limits: &Limits) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    let byte_len = bytes.len() as u64;
    if let Some(max_bytes) = limits.max_input_bytes
        && byte_len > max_bytes
    {
        return Err(ReadError::InputTooLarge {
            actual: byte_len,
            limit: max_bytes,
        });
    }

    // Verify well-formed XML and detect <svg> tag while bounding nesting depth
    let mut xml_reader = quick_xml::Reader::from_reader(bytes);
    xml_reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut depth: u16 = 0;
    let mut has_svg = false;
    let mut in_title = false;
    let mut title_text = String::new();

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                depth = depth.saturating_add(1);
                if depth > limits.max_nesting_depth {
                    return Err(ReadError::NestingTooDeep {
                        limit: limits.max_nesting_depth,
                    });
                }
                let name = e.local_name();
                if name.as_ref().eq_ignore_ascii_case("svg") {
                    has_svg = true;
                } else if name.as_ref().eq_ignore_ascii_case("title") {
                    in_title = true;
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref().eq_ignore_ascii_case("svg") {
                    has_svg = true;
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                depth = depth.saturating_sub(1);
                let name = e.local_name();
                if name.as_ref().eq_ignore_ascii_case("title") {
                    in_title = false;
                }
            }
            Ok(quick_xml::events::Event::Text(ref e)) => {
                if in_title && title_text.is_empty() {
                    title_text = e.as_ref().to_string();
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(ReadError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    if !has_svg {
        return Err(ReadError::NoSvgElement);
    }

    let caption = if !title_text.trim().is_empty() {
        vec![Inline::Text {
            text: nfc(title_text.trim()),
        }]
    } else {
        Vec::new()
    };

    let id = hex::encode(Sha256::digest(bytes));
    let asset = Asset {
        font: None,
        media_type: "image/svg+xml".to_owned(),
        bytes: bytes.to_vec(),
    };

    let mut document = Document {
        meta: Metadata {
            title: (!title_text.trim().is_empty()).then(|| nfc(title_text.trim())),
            source_format: Some(Format::Svg),
            ..Metadata::default()
        },
        body: vec![Block::Figure {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            asset: AssetRef::Asset { id: id.clone() },
            caption,
        }],
        ..Document::default()
    };
    document.assets.insert(id, asset);

    Ok(ReadOutput {
        document: document.finalize_import(),
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_svg_with_title() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <title>Sample SVG Diagram</title>
            <circle cx="50" cy="50" r="40" fill="red" />
        </svg>"#;

        let out = read(svg, &Limits::local()).unwrap();
        assert_eq!(out.document.meta.source_format, Some(Format::Svg));
        assert_eq!(
            out.document.meta.title.as_deref(),
            Some("Sample SVG Diagram")
        );
        assert_eq!(out.document.body.len(), 1);
        assert_eq!(out.document.assets.len(), 1);

        if let Block::Figure { asset, caption, .. } = &out.document.body[0] {
            assert!(matches!(asset, AssetRef::Asset { .. }));
            assert_eq!(
                caption[0],
                Inline::Text {
                    text: "Sample SVG Diagram".to_owned()
                }
            );
        } else {
            panic!("expected Figure block");
        }
    }

    #[test]
    fn rejects_non_svg_xml() {
        let xml = b"<html><body>Hello</body></html>";
        let err = read(xml, &Limits::local()).unwrap_err();
        assert!(matches!(err, ReadError::NoSvgElement));
    }

    #[test]
    fn rejects_malformed_xml() {
        let xml = b"<svg><circle></svg>";
        let err = read(xml, &Limits::local()).unwrap_err();
        assert!(matches!(err, ReadError::Xml(_)));
    }

    #[test]
    fn enforces_nesting_depth() {
        let mut deep_svg = Vec::new();
        deep_svg.extend_from_slice(b"<svg xmlns=\"http://www.w3.org/2000/svg\">");
        for _ in 0..10 {
            deep_svg.extend_from_slice(b"<g>");
        }
        for _ in 0..10 {
            deep_svg.extend_from_slice(b"</g>");
        }
        deep_svg.extend_from_slice(b"</svg>");

        let limits = Limits {
            max_nesting_depth: 5,
            ..Limits::local()
        };
        let err = read(&deep_svg, &limits).unwrap_err();
        assert!(matches!(err, ReadError::NestingTooDeep { .. }));
    }
}
