//! Accessibility report generator for PDF/UA compliance inspection.

use lopdf::{Document, Object};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A detected accessibility violation or warning.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccessibilityIssue {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
}

/// Comprehensive accessibility report for PDF/UA-1 conformance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccessibilityReport {
    pub compliant: bool,
    pub is_tagged: bool,
    pub has_struct_tree_root: bool,
    pub has_lang: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    pub displays_doc_title: bool,
    pub has_title: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub figures_total: usize,
    pub figures_with_alt: usize,
    pub figures_missing_alt: usize,
    pub fonts_total: usize,
    pub fonts_with_tounicode: usize,
    pub fonts_missing_tounicode: usize,
    pub pages_with_tabs_s: usize,
    pub pages_total: usize,
    pub issues: Vec<AccessibilityIssue>,
}

/// Inspects a document for PDF/UA-1 accessibility requirements.
pub fn inspect_accessibility(
    doc: &Document,
) -> Result<AccessibilityReport, crate::error::PdfError> {
    let mut issues = Vec::new();

    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| crate::error::PdfError::Malformed)?;

    let catalog = doc
        .get_object(catalog_id)
        .and_then(Object::as_dict)
        .map_err(|_| crate::error::PdfError::Malformed)?;

    // 1. MarkInfo << /Marked true >>
    let is_marked = catalog
        .get(b"MarkInfo")
        .ok()
        .and_then(|o| match o {
            Object::Dictionary(d) => Some(d),
            Object::Reference(r) => doc.get_object(*r).ok().and_then(|obj| obj.as_dict().ok()),
            _ => None,
        })
        .and_then(|m| m.get(b"Marked").ok())
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);

    if !is_marked {
        issues.push(AccessibilityIssue {
            code: "missing_marked_info".into(),
            message: "The document catalog does not specify /MarkInfo << /Marked true >>.".into(),
            page: None,
        });
    }

    // 2. StructTreeRoot
    let has_struct_tree_root = catalog
        .get_deref(b"StructTreeRoot", doc)
        .and_then(Object::as_dict)
        .is_ok();
    if !has_struct_tree_root {
        issues.push(AccessibilityIssue {
            code: "missing_struct_tree_root".into(),
            message: "The document catalog does not define /StructTreeRoot for tagged structure."
                .into(),
            page: None,
        });
    }

    let is_tagged = is_marked && has_struct_tree_root;

    // 3. Document /Lang
    let lang = catalog
        .get_deref(b"Lang", doc)
        .ok()
        .and_then(crate::tools::outline::decode_pdf_object_string)
        .filter(|text| !text.trim().is_empty());

    let has_lang = lang.is_some();
    if !has_lang {
        issues.push(AccessibilityIssue {
            code: "missing_lang".into(),
            message: "The document catalog does not specify a natural language (/Lang).".into(),
            page: None,
        });
    }

    // 4. ViewerPreferences << /DisplayDocTitle true >>
    let displays_doc_title = catalog
        .get(b"ViewerPreferences")
        .ok()
        .and_then(|o| match o {
            Object::Dictionary(d) => Some(d),
            Object::Reference(r) => doc.get_object(*r).ok().and_then(|obj| obj.as_dict().ok()),
            _ => None,
        })
        .and_then(|vp| vp.get(b"DisplayDocTitle").ok())
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);

    if !displays_doc_title {
        issues.push(AccessibilityIssue {
            code: "missing_display_doc_title".into(),
            message: "The document does not set /ViewerPreferences << /DisplayDocTitle true >>."
                .into(),
            page: None,
        });
    }

    // 5. Document /Title
    let title = doc
        .trailer
        .get_deref(b"Info", doc)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|info| info.get_deref(b"Title", doc).ok())
        .and_then(crate::tools::outline::decode_pdf_object_string)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| xmp_title(doc));

    let has_title = title.is_some();
    if !has_title {
        issues.push(AccessibilityIssue {
            code: "missing_title".into(),
            message: "The document metadata does not specify a title.".into(),
            page: None,
        });
    }

    let mut objects = Vec::new();
    for object in doc.objects.values() {
        collect_objects(object, &mut objects);
    }

    // 6. Figure /Alt attributes
    let mut figures_total = 0;
    let mut figures_with_alt = 0;
    let mut figures_missing_alt = 0;

    for obj in &objects {
        let Ok(dict) = obj.as_dict() else {
            continue;
        };
        if dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"StructElem")
            || dict.get(b"S").and_then(Object::as_name).ok() != Some(b"Figure")
        {
            continue;
        }
        figures_total += 1;
        let has_alt = dict
            .get_deref(b"Alt", doc)
            .ok()
            .and_then(crate::tools::outline::decode_pdf_object_string)
            .is_some_and(|alt| !alt.trim().is_empty());
        if has_alt {
            figures_with_alt += 1;
        } else {
            figures_missing_alt += 1;
            issues.push(AccessibilityIssue {
                code: "figure_missing_alt".into(),
                message: "Figure structure element is missing an alternative description (/Alt)."
                    .into(),
                page: None,
            });
        }
    }

    // 7. Font /ToUnicode
    let mut fonts_total = 0;
    let mut fonts_with_tounicode = 0;
    let mut fonts_missing_tounicode = 0;

    for obj in &objects {
        let Ok(dict) = obj.as_dict() else {
            continue;
        };
        if dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"Font") {
            continue;
        }
        let subtype = dict.get(b"Subtype").and_then(Object::as_name).ok();
        // Descendant fonts inherit the Type0 font's Unicode mapping.
        if matches!(subtype, Some(b"CIDFontType0" | b"CIDFontType2")) {
            continue;
        }
        fonts_total += 1;
        if dict
            .get_deref(b"ToUnicode", doc)
            .and_then(Object::as_stream)
            .is_ok()
        {
            fonts_with_tounicode += 1;
        } else {
            fonts_missing_tounicode += 1;
            let name = dict
                .get(b"BaseFont")
                .and_then(Object::as_name)
                .map(String::from_utf8_lossy)
                .unwrap_or_default();
            issues.push(AccessibilityIssue {
                code: "font_missing_tounicode".into(),
                message: format!("Font '{name}' lacks a /ToUnicode mapping."),
                page: None,
            });
        }
    }

    // 8. Page tab order (/Tabs /S)
    let mut pages_total = 0;
    let mut pages_with_tabs_s = 0;

    for (page_idx, page_id) in doc.page_iter().enumerate() {
        pages_total += 1;
        let page_num = page_idx + 1;
        let has_tabs_s = doc
            .get_object(page_id)
            .and_then(Object::as_dict)
            .ok()
            .is_some_and(|page| page.get(b"Tabs").and_then(Object::as_name).ok() == Some(b"S"));
        if has_tabs_s {
            pages_with_tabs_s += 1;
        } else {
            issues.push(AccessibilityIssue {
                code: "page_missing_tabs_s".into(),
                message: format!(
                    "Page {page_num} does not specify structure tab order (/Tabs /S)."
                ),
                page: Some(page_num),
            });
        }
    }

    let compliant = is_tagged
        && has_lang
        && displays_doc_title
        && has_title
        && figures_missing_alt == 0
        && fonts_missing_tounicode == 0
        && pages_with_tabs_s == pages_total;

    Ok(AccessibilityReport {
        compliant,
        is_tagged,
        has_struct_tree_root,
        has_lang,
        lang,
        displays_doc_title,
        has_title,
        title,
        figures_total,
        figures_with_alt,
        figures_missing_alt,
        fonts_total,
        fonts_with_tounicode,
        fonts_missing_tounicode,
        pages_with_tabs_s,
        pages_total,
        issues,
    })
}

fn collect_objects<'a>(object: &'a Object, objects: &mut Vec<&'a Object>) {
    objects.push(object);
    match object {
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter() {
                collect_objects(value, objects);
            }
        }
        Object::Array(array) => {
            for value in array {
                collect_objects(value, objects);
            }
        }
        _ => {}
    }
}

fn xmp_title(doc: &Document) -> Option<String> {
    use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};
    let catalog = doc.catalog().ok()?;
    let stream = catalog
        .get_deref(b"Metadata", doc)
        .and_then(Object::as_stream)
        .ok()?;
    let bytes = super::xmp::metadata_bytes(stream).ok()?;
    let xml = std::str::from_utf8(&bytes).ok()?;
    let mut reader = NsReader::from_str(xml);
    let mut depth = 0;
    let mut in_title = false;
    let mut title = String::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(start) => {
                let (ns, local) = reader.resolver().resolve_element(start.name());
                if matches!(ns, ResolveResult::Bound(ns) if ns.0 == "http://purl.org/dc/elements/1.1/")
                    && local.as_ref() == "title"
                {
                    in_title = true;
                }
                if in_title {
                    depth += 1;
                }
            }
            Event::Text(text) if in_title => {
                title.push_str(&quick_xml::escape::unescape(text.as_ref()).ok()?)
            }
            Event::GeneralRef(reference) if in_title => {
                let escaped = format!("&{};", reference.as_ref());
                title.push_str(&quick_xml::escape::unescape(&escaped).ok()?);
            }
            Event::CData(text) if in_title => title.push_str(text.as_ref()),
            Event::End(_) if in_title => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (!title.trim().is_empty()).then(|| title.trim().to_owned())
}
