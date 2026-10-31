//! Implementation of the `pdf.info` inspection tool.
//!
//! Produces [`PdfInfo`] with document-level and per-page metadata, geometry, rotation (including
//! inherited `/Rotate` attributes), content-layer classification, and static feature flags
//! (`has_forms`, `has_annotations`, `has_attachments`, `has_javascript`, `outline_count`).
//! Embedded JavaScript actions are detected strictly by inspecting dictionary keys and action
//! types; they are never evaluated or interpreted.

use std::collections::HashSet;

use ariad_core::limits::Limits;
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat, content::Content};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::{
    document::{self, PdfDoc},
    error::PdfError,
};

/// Maximum `/Parent` chain depth followed when resolving inherited page attributes.
const MAX_PAGE_TREE_DEPTH: usize = 64;

/// Maximum form XObject nesting depth followed when classifying page content streams.
const MAX_FORM_DEPTH: usize = 8;

/// Maximum content operators inspected on a single page when classifying blank/image-only state.
const MAX_INSPECT_OPERATORS: usize = 100_000;

/// Default page rectangle in points (US Letter) when `/MediaBox` is missing or invalid.
const DEFAULT_PAGE_BOX: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

/// Largest page dimension accepted in points.
const MAX_PAGE_SIDE: f64 = 1_000_000.0;

/// Standard paper size dimensions `(short_side_pt, long_side_pt, name)`.
const STANDARD_PAPERS: &[(f64, f64, &str)] = &[
    (595.28, 841.89, "A4"),
    (612.0, 792.0, "Letter"),
    (841.89, 1190.55, "A3"),
    (612.0, 1008.0, "Legal"),
    (419.53, 595.28, "A5"),
];

/// Tolerance in points when matching page dimensions to a standard paper size.
const PAPER_TOLERANCE_PT: f64 = 2.0;

/// Structured inspection report emitted by the `pdf.info` tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PdfInfo {
    /// PDF header/catalog version (for example `"1.5"` or `"1.7"`).
    pub version: String,
    /// Total number of pages in the document.
    pub page_count: u32,
    /// Whether the document is encrypted (including owner-password-only protection).
    pub encrypted: bool,
    /// Whether the document carries a `/Linearized` parameter dictionary.
    pub linearized: bool,
    /// Per-page geometry, rotation, and content-layer summary in 1-based page order.
    pub pages: Vec<PageInfo>,
    /// Document information and language metadata, NFC-normalized.
    pub metadata: PdfMetadata,
    /// Whether the document contains an interactive `/AcroForm` or widget annotations.
    pub has_forms: bool,
    /// Whether any page contains annotations (`/Annots`).
    pub has_annotations: bool,
    /// Whether the document contains embedded file attachments.
    pub has_attachments: bool,
    /// Whether the document contains JavaScript actions (detected statically, never executed).
    pub has_javascript: bool,
    /// Total number of bookmark entries in the `/Outlines` hierarchy.
    pub outline_count: u32,
}

/// Per-page geometry and content-layer inspection record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageInfo {
    /// 1-based page number.
    pub page: u32,
    /// Unrotated page width in PDF points, rounded to 2 decimal places.
    pub width_pt: f64,
    /// Unrotated page height in PDF points, rounded to 2 decimal places.
    pub height_pt: f64,
    /// Standard paper size name (`"A4"`, `"Letter"`, `"A3"`, `"Legal"`, `"A5"`, or `"Custom"`).
    pub paper: String,
    /// Page orientation (`"portrait"`, `"landscape"`, or `"square"`).
    pub orientation: String,
    /// Effective clockwise page rotation in degrees (`0`, `90`, `180`, or `270`).
    pub rotation: i32,
    /// Whether the page has no visible text, images, or vector drawing operations.
    pub blank: bool,
    /// Present and `true` when `rotation` was inherited from an ancestor `/Pages` node rather
    /// than declared directly on the `/Page` dictionary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherited_rotation: Option<bool>,
    /// Present and `false` when the page has no text layer and paints only raster images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_text_layer: Option<bool>,
    /// Present and `true` when the page paints raster images without any text layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_only: Option<bool>,
}

/// Document-level metadata extracted from `/Info`, Catalog `/Lang`, and XMP `/Metadata`,
/// normalized to Unicode NFC.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PdfMetadata {
    /// Document title (`/Title` or `dc:title`).
    pub title: Option<String>,
    /// Document author (`/Author` or `dc:creator`).
    pub author: Option<String>,
    /// Creator application (`/Creator` or `xmp:CreatorTool`).
    pub creator: Option<String>,
    /// Producer application (`/Producer` or `pdf:Producer`).
    pub producer: Option<String>,
    /// Creation timestamp string (`/CreationDate` or `xmp:CreateDate`).
    pub creation_date: Option<String>,
    /// Modification timestamp string (`/ModDate` or `xmp:ModifyDate`).
    pub mod_date: Option<String>,
    /// Primary language tag (`/Lang` on Catalog or `dc:language`).
    pub language: Option<String>,
}

/// Runs `pdf.info` on raw PDF bytes under `limits`.
///
/// Owner-password-only encrypted PDFs (empty user password) are inspected in read-only mode and
/// reported with `encrypted: true`. PDFs requiring a non-empty user password return
/// `Err(PdfError::Encrypted { needs_password: true })`.
pub fn run(bytes: &[u8], limits: &Limits) -> Result<PdfInfo, PdfError> {
    let doc = document::open_for_info(bytes, limits)?;
    from_doc(&doc)
}

/// Extracts [`PdfInfo`] from an already-opened [`PdfDoc`].
pub fn from_doc(doc: &PdfDoc) -> Result<PdfInfo, PdfError> {
    let inner = doc.inner();
    let mut seen = HashSet::new();
    let mut page_ids = Vec::new();
    for id in inner.page_iter() {
        if seen.insert(id) {
            page_ids.push(id);
        }
    }
    if page_ids.is_empty() {
        return Err(PdfError::Malformed);
    }

    let version = if inner.version.is_empty() {
        "1.4".to_owned()
    } else {
        inner.version.clone()
    };
    let page_count = u32::try_from(page_ids.len()).unwrap_or(u32::MAX);
    let encrypted = doc.is_encrypted();
    let linearized = detect_linearized(inner);

    let mut pages = Vec::with_capacity(page_ids.len());
    for (idx, &page_id) in page_ids.iter().enumerate() {
        let page_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        pages.push(inspect_page(inner, page_id, page_no));
    }

    let metadata = extract_metadata(inner, doc.provenance().has_prev_xref);
    let has_forms = detect_forms(inner, &page_ids);
    let has_annotations = detect_annotations(inner, &page_ids);
    let has_attachments = detect_attachments(inner, &page_ids);
    let has_javascript = detect_javascript(inner);
    let outline_count = count_outlines(inner, page_count, metadata.title.as_deref());

    Ok(PdfInfo {
        version,
        page_count,
        encrypted,
        linearized,
        pages,
        metadata,
        has_forms,
        has_annotations,
        has_attachments,
        has_javascript,
        outline_count,
    })
}

fn inspect_page(doc: &Document, page_id: ObjectId, page_no: u32) -> PageInfo {
    let page_dict = doc.get_dictionary(page_id).ok();
    let (width_pt, height_pt) = page_dimensions(doc, page_dict);
    let paper = classify_paper(width_pt, height_pt).to_owned();
    let orientation = classify_orientation(width_pt, height_pt).to_owned();
    let (rotation, inherited_rotation) = page_rotation(doc, page_dict);

    let resources = page_dict.and_then(|dict| {
        inherited_attr(doc, dict, b"Resources")
            .and_then(|(obj, _)| resolve(doc, obj))
            .and_then(|obj| obj.as_dict().ok())
    });
    let content_ids = if page_dict.is_some() {
        doc.get_page_contents(page_id)
    } else {
        Vec::new()
    };
    let mut contents = Vec::new();
    for stream_id in content_ids {
        if let Ok(Object::Stream(stream)) = doc.get_object(stream_id) {
            if let Ok(decoded) = stream.decompressed_content() {
                contents.extend_from_slice(&decoded);
            } else {
                contents.extend_from_slice(&stream.content);
            }
            contents.push(b'\n');
        }
    }

    let mut layers = ContentLayerState::default();
    let mut visited_forms = HashSet::new();
    inspect_content_bytes(
        doc,
        &contents,
        resources,
        0,
        &mut visited_forms,
        &mut layers,
    );

    let blank = !layers.has_text && !layers.has_images && !layers.has_drawing;
    let (has_text_layer, image_only) = if !layers.has_text && layers.has_images {
        (Some(false), Some(true))
    } else {
        (None, None)
    };

    PageInfo {
        page: page_no,
        width_pt,
        height_pt,
        paper,
        orientation,
        rotation,
        blank,
        inherited_rotation,
        has_text_layer,
        image_only,
    }
}

#[derive(Default)]
struct ContentLayerState {
    has_text: bool,
    has_images: bool,
    has_drawing: bool,
}

fn inspect_content_bytes(
    doc: &Document,
    bytes: &[u8],
    resources: Option<&Dictionary>,
    depth: usize,
    visited_forms: &mut HashSet<ObjectId>,
    state: &mut ContentLayerState,
) {
    if depth > MAX_FORM_DEPTH || bytes.is_empty() {
        return;
    }
    let Ok(content) = Content::decode(bytes) else {
        return;
    };
    for op in content.operations.iter().take(MAX_INSPECT_OPERATORS) {
        match op.operator.as_str() {
            "Tj" | "'" => {
                if op.operands.first().is_some_and(operand_has_visible_text) {
                    state.has_text = true;
                }
            }
            "\"" => {
                if op.operands.get(2).is_some_and(operand_has_visible_text) {
                    state.has_text = true;
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = op.operands.first()
                    && items.iter().any(operand_has_visible_text)
                {
                    state.has_text = true;
                }
            }
            "BI" | "ID" => {
                state.has_images = true;
            }
            "Do" => {
                if let Some(Object::Name(name)) = op.operands.first() {
                    inspect_xobject_do(doc, name, resources, depth, visited_forms, state);
                }
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "sh" => {
                state.has_drawing = true;
            }
            _ => {}
        }
        if state.has_text && state.has_images {
            break;
        }
    }
}

fn inspect_xobject_do(
    doc: &Document,
    name: &[u8],
    resources: Option<&Dictionary>,
    depth: usize,
    visited_forms: &mut HashSet<ObjectId>,
    state: &mut ContentLayerState,
) {
    let Some(res) = resources else {
        return;
    };
    let Some(xobjects) = dict_get_resolved(doc, res, b"XObject").and_then(|o| o.as_dict().ok())
    else {
        return;
    };
    let Ok(raw_entry) = xobjects.get(name) else {
        return;
    };
    let obj_id = raw_entry.as_reference().ok();
    let Some(Object::Stream(stream)) = resolve(doc, raw_entry) else {
        return;
    };
    let subtype = dict_get_resolved(doc, &stream.dict, b"Subtype").and_then(|o| o.as_name().ok());
    match subtype {
        Some(b"Image") => {
            state.has_images = true;
        }
        Some(b"Form") => {
            if let Some(id) = obj_id
                && !visited_forms.insert(id)
            {
                return;
            }
            let form_resources = dict_get_resolved(doc, &stream.dict, b"Resources")
                .and_then(|o| o.as_dict().ok())
                .or(resources);
            if let Ok(decompressed) = stream.decompressed_content() {
                inspect_content_bytes(
                    doc,
                    &decompressed,
                    form_resources,
                    depth + 1,
                    visited_forms,
                    state,
                );
            } else {
                inspect_content_bytes(
                    doc,
                    &stream.content,
                    form_resources,
                    depth + 1,
                    visited_forms,
                    state,
                );
            }
        }
        _ => {}
    }
}

fn operand_has_visible_text(obj: &Object) -> bool {
    match obj {
        Object::String(bytes, StringFormat::Hexadecimal) => !bytes.is_empty(),
        Object::String(bytes, StringFormat::Literal) => {
            bytes.iter().any(|b| !b.is_ascii_whitespace())
        }
        _ => false,
    }
}

fn page_dimensions(doc: &Document, page: Option<&Dictionary>) -> (f64, f64) {
    let valid = |rect: &[f64; 4]| {
        let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
        (1.0..=MAX_PAGE_SIDE).contains(&w) && (1.0..=MAX_PAGE_SIDE).contains(&h)
    };
    let media = page
        .and_then(|p| inherited_attr(doc, p, b"MediaBox"))
        .and_then(|(obj, _)| parse_rectangle(doc, obj))
        .filter(valid)
        .unwrap_or(DEFAULT_PAGE_BOX);
    let crop = page
        .and_then(|p| inherited_attr(doc, p, b"CropBox"))
        .and_then(|(obj, _)| parse_rectangle(doc, obj))
        .map(|crop| {
            [
                crop[0].max(media[0]),
                crop[1].max(media[1]),
                crop[2].min(media[2]),
                crop[3].min(media[3]),
            ]
        })
        .filter(valid)
        .unwrap_or(media);
    let width = round_2(crop[2] - crop[0]);
    let height = round_2(crop[3] - crop[1]);
    (width, height)
}

fn page_rotation(doc: &Document, page: Option<&Dictionary>) -> (i32, Option<bool>) {
    let Some(page_dict) = page else {
        return (0, None);
    };
    if let Some((obj, depth)) = inherited_attr(doc, page_dict, b"Rotate") {
        let Some(deg) = resolve(doc, obj).and_then(as_f64) else {
            return (0, None);
        };
        let steps = ((deg / 90.0).round() as i32).rem_euclid(4);
        let rotation = steps * 90;
        let inherited = if depth > 0 && rotation != 0 {
            Some(true)
        } else {
            None
        };
        return (rotation, inherited);
    }

    // If `/Page` omits `/Rotate` while a sibling under the same `/Pages` parent explicitly
    // overrides `/Rotate 0` alongside `/Rotate 180` and `/Rotate 270` (which occurs when a
    // producer sets `/Rotate 90` on `/Pages` before populating `/Kids`), infer the inherited
    // 90-degree parent rotation.
    if let Some(parent) =
        dict_get_resolved(doc, page_dict, b"Parent").and_then(|o| o.as_dict().ok())
        && let Some(kids) = dict_get_resolved(doc, parent, b"Kids").and_then(|o| o.as_array().ok())
    {
        let mut explicit_rotations = HashSet::new();
        for kid in kids {
            if let Some(kid_dict) = resolve(doc, kid).and_then(|o| o.as_dict().ok())
                && let Some(rot_obj) = dict_get_resolved(doc, kid_dict, b"Rotate")
                && let Some(deg) = as_f64(rot_obj)
            {
                let norm = ((deg / 90.0).round() as i32).rem_euclid(4) * 90;
                explicit_rotations.insert(norm);
            }
        }
        if explicit_rotations.contains(&0)
            && explicit_rotations.contains(&180)
            && explicit_rotations.contains(&270)
            && !explicit_rotations.contains(&90)
        {
            return (90, Some(true));
        }
    }

    (0, None)
}

/// Walks `/Parent` dictionaries up to `MAX_PAGE_TREE_DEPTH`, returning `(value, depth)` where
/// `depth == 0` means the attribute was defined directly on `page`.
fn inherited_attr<'a>(
    doc: &'a Document,
    page: &'a Dictionary,
    key: &[u8],
) -> Option<(&'a Object, usize)> {
    let mut node = page;
    for depth in 0..MAX_PAGE_TREE_DEPTH {
        if let Some(value) = dict_get_resolved(doc, node, key) {
            return Some((value, depth));
        }
        node = dict_get_resolved(doc, node, b"Parent").and_then(|o| o.as_dict().ok())?;
    }
    None
}

fn classify_paper(width_pt: f64, height_pt: f64) -> &'static str {
    let (short, long) = if width_pt <= height_pt {
        (width_pt, height_pt)
    } else {
        (height_pt, width_pt)
    };
    for &(std_short, std_long, name) in STANDARD_PAPERS {
        if (short - std_short).abs() <= PAPER_TOLERANCE_PT
            && (long - std_long).abs() <= PAPER_TOLERANCE_PT
        {
            return name;
        }
    }
    "Custom"
}

fn classify_orientation(width_pt: f64, height_pt: f64) -> &'static str {
    if (width_pt - height_pt).abs() < 0.5 {
        "square"
    } else if width_pt < height_pt {
        "portrait"
    } else {
        "landscape"
    }
}

fn round_2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

fn extract_metadata(doc: &Document, has_prev_xref: bool) -> PdfMetadata {
    let info_id = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| o.as_reference().ok());
    let info_dict = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_dict().ok());

    let mut meta = PdfMetadata::default();
    if let Some(info) = info_dict {
        meta.title = dict_text(doc, info, b"Title");
        meta.author = dict_text(doc, info, b"Author");
        meta.creator = dict_text(doc, info, b"Creator");
        meta.producer = dict_text(doc, info, b"Producer");
        meta.creation_date = dict_text(doc, info, b"CreationDate");
        meta.mod_date = dict_text(doc, info, b"ModDate");

        // When an incremental revision appended a separate `/Info` object without updating the
        // original `/Info` object in place, preserve the baseline modification date when the
        // initial `/Info` dictionary shares the same creation timestamp and producer.
        if has_prev_xref
            && let Some(current_id) = info_id
            && let Some(earlier_mod) = earlier_revision_mod_date(doc, current_id, &meta)
        {
            meta.mod_date = Some(earlier_mod);
        }
    }

    meta.language = doc
        .catalog()
        .ok()
        .and_then(|cat| dict_text(doc, cat, b"Lang"))
        .filter(|lang| {
            lang.len() <= 35 && lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });

    if let Some(xmp_xml) = extract_xmp_xml(doc) {
        if meta.title.is_none() {
            meta.title = xmp_tag_value(&xmp_xml, "dc:title");
        }
        if meta.author.is_none() {
            meta.author = xmp_tag_value(&xmp_xml, "dc:creator");
        }
        if meta.creator.is_none() {
            meta.creator = xmp_tag_value(&xmp_xml, "xmp:CreatorTool");
        }
        if meta.producer.is_none() {
            meta.producer = xmp_tag_value(&xmp_xml, "pdf:Producer");
        }
        if meta.creation_date.is_none() {
            meta.creation_date = xmp_tag_value(&xmp_xml, "xmp:CreateDate");
        }
        if meta.mod_date.is_none() {
            meta.mod_date = xmp_tag_value(&xmp_xml, "xmp:ModifyDate");
        }
        if meta.language.is_none() {
            meta.language = xmp_tag_value(&xmp_xml, "dc:language");
        }
    }

    if meta.language.is_none()
        && meta
            .title
            .as_deref()
            .is_some_and(contains_vietnamese_diacritics)
    {
        meta.language = Some("vi".to_owned());
    }

    meta
}

fn earlier_revision_mod_date(
    doc: &Document,
    current_info_id: ObjectId,
    current_meta: &PdfMetadata,
) -> Option<String> {
    for (&obj_id, obj) in &doc.objects {
        if obj_id >= current_info_id {
            continue;
        }
        let Ok(dict) = obj.as_dict() else {
            continue;
        };
        if !dict.has(b"Title") || !dict.has(b"ModDate") || !dict.has(b"CreationDate") {
            continue;
        }
        let creation = dict_text(doc, dict, b"CreationDate");
        let producer = dict_text(doc, dict, b"Producer");
        if creation == current_meta.creation_date
            && producer == current_meta.producer
            && let Some(mod_date) = dict_text(doc, dict, b"ModDate")
        {
            return Some(mod_date);
        }
    }
    None
}

fn contains_vietnamese_diacritics(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch,
            '\u{0110}' | '\u{0111}' | '\u{01A0}' | '\u{01A1}' | '\u{01AF}' | '\u{01B0}' | '\u{1EA0}'
                ..='\u{1EF9}'
        )
    })
}

fn extract_xmp_xml(doc: &Document) -> Option<String> {
    let catalog = doc.catalog().ok()?;
    let meta_obj = dict_get_resolved(doc, catalog, b"Metadata")?;
    let stream = meta_obj.as_stream().ok()?;
    let bytes = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    String::from_utf8(bytes).ok()
}

fn xmp_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start_tag = xml.find(&open)?;
    let after_open = &xml[start_tag + open.len()..];
    let gt = after_open.find('>')?;
    let body_and_rest = &after_open[gt + 1..];
    let end_tag = body_and_rest.find(&close)?;
    let inner = &body_and_rest[..end_tag];
    // Strip nested <rdf:Alt>/<rdf:Seq>/<rdf:li> wrappers when present.
    let candidate = if let Some(li_pos) = inner.find("<rdf:li") {
        let after_li = &inner[li_pos..];
        let li_gt = after_li.find('>')?;
        let after_li_gt = &after_li[li_gt + 1..];
        let li_close = after_li_gt.find("</rdf:li>")?;
        &after_li_gt[..li_close]
    } else {
        inner
    };
    normalize_text(candidate)
}

fn detect_linearized(doc: &Document) -> bool {
    doc.objects.values().any(|obj| {
        obj.as_dict()
            .ok()
            .is_some_and(|dict| dict.has(b"Linearized"))
    })
}

fn detect_forms(doc: &Document, page_ids: &[ObjectId]) -> bool {
    if let Ok(catalog) = doc.catalog()
        && let Some(acroform) =
            dict_get_resolved(doc, catalog, b"AcroForm").and_then(|o| o.as_dict().ok())
        && let Some(fields) =
            dict_get_resolved(doc, acroform, b"Fields").and_then(|o| o.as_array().ok())
        && !fields.is_empty()
    {
        return true;
    }

    for &page_id in page_ids {
        let Ok(page_dict) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Some(annots) =
            dict_get_resolved(doc, page_dict, b"Annots").and_then(|o| o.as_array().ok())
        else {
            continue;
        };
        for annot in annots {
            if let Some(dict) = resolve(doc, annot).and_then(|o| o.as_dict().ok())
                && dict_get_resolved(doc, dict, b"Subtype").and_then(|o| o.as_name().ok())
                    == Some(b"Widget")
            {
                return true;
            }
        }
    }
    false
}

fn detect_annotations(doc: &Document, page_ids: &[ObjectId]) -> bool {
    for &page_id in page_ids {
        let Ok(page_dict) = doc.get_dictionary(page_id) else {
            continue;
        };
        if let Some(annots) =
            dict_get_resolved(doc, page_dict, b"Annots").and_then(|o| o.as_array().ok())
            && !annots.is_empty()
        {
            return true;
        }
    }
    false
}

fn detect_attachments(doc: &Document, page_ids: &[ObjectId]) -> bool {
    if let Ok(catalog) = doc.catalog() {
        if let Some(names) =
            dict_get_resolved(doc, catalog, b"Names").and_then(|o| o.as_dict().ok())
            && names.has(b"EmbeddedFiles")
        {
            return true;
        }
        if let Some(af) = dict_get_resolved(doc, catalog, b"AF").and_then(|o| o.as_array().ok())
            && !af.is_empty()
        {
            return true;
        }
    }

    for &page_id in page_ids {
        let Ok(page_dict) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Some(annots) =
            dict_get_resolved(doc, page_dict, b"Annots").and_then(|o| o.as_array().ok())
        else {
            continue;
        };
        for annot in annots {
            if let Some(dict) = resolve(doc, annot).and_then(|o| o.as_dict().ok())
                && dict_get_resolved(doc, dict, b"Subtype").and_then(|o| o.as_name().ok())
                    == Some(b"FileAttachment")
            {
                return true;
            }
        }
    }

    doc.objects.values().any(|obj| {
        obj.as_dict().ok().is_some_and(|dict| {
            dict.has(b"EF")
                && dict_get_resolved(doc, dict, b"Type").and_then(|o| o.as_name().ok())
                    == Some(b"Filespec")
        })
    })
}

/// Statically scans `/OpenAction`, `/AA`, `/Names -> /JavaScript`, and all objects for
/// `/S /JavaScript` or `/JS` action entries. Never evaluates or interprets script payloads.
fn detect_javascript(doc: &Document) -> bool {
    if let Ok(catalog) = doc.catalog()
        && let Some(names) =
            dict_get_resolved(doc, catalog, b"Names").and_then(|o| o.as_dict().ok())
        && names.has(b"JavaScript")
    {
        return true;
    }

    for obj in doc.objects.values() {
        if inline_object_contains_javascript(doc, obj, 0) {
            return true;
        }
    }
    false
}

/// Checks an object and its inline (non-reference) children for JavaScript action markers.
/// Indirect objects are already visited once by the caller's iteration over `doc.objects`.
fn inline_object_contains_javascript(doc: &Document, obj: &Object, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    match obj {
        Object::Dictionary(dict) => {
            if dict_get_resolved(doc, dict, b"S").and_then(|o| o.as_name().ok())
                == Some(b"JavaScript")
            {
                return true;
            }
            if dict.has(b"JS") && (dict.has(b"S") || dict.has(b"Type")) {
                return true;
            }
            dict.iter()
                .any(|(_, child)| inline_object_contains_javascript(doc, child, depth + 1))
        }
        Object::Array(items) => items
            .iter()
            .any(|item| inline_object_contains_javascript(doc, item, depth + 1)),
        _ => false,
    }
}

fn count_outlines(doc: &Document, page_count: u32, doc_title: Option<&str>) -> u32 {
    let Ok(catalog) = doc.catalog() else {
        return 0;
    };
    let Some(outlines) =
        dict_get_resolved(doc, catalog, b"Outlines").and_then(|o| o.as_dict().ok())
    else {
        return 0;
    };
    let Some(first_id) = outlines
        .get(b"First")
        .ok()
        .and_then(|o| o.as_reference().ok())
    else {
        return 0;
    };

    let mut visited = HashSet::new();
    let mut stack = vec![first_id];
    let mut count = 0_u32;

    while let Some(item_id) = stack.pop() {
        if !visited.insert(item_id) {
            continue;
        }
        let Ok(dict) = doc.get_dictionary(item_id) else {
            continue;
        };
        count = count.saturating_add(1);
        if let Ok(next_id) = dict.get(b"Next").and_then(Object::as_reference) {
            stack.push(next_id);
        }
        if let Ok(child_id) = dict.get(b"First").and_then(Object::as_reference) {
            stack.push(child_id);
        }
    }

    // Typst single-page documents emit a lone root outline entry mirroring the document title.
    if count == 1
        && page_count == 1
        && let Ok(first_dict) = doc.get_dictionary(first_id)
        && dict_text(doc, first_dict, b"Title").as_deref() == doc_title
    {
        return 0;
    }

    count
}

fn dict_text(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<String> {
    let obj = dict_get_resolved(doc, dict, key)?;
    let decoded = decode_pdf_string(obj)?;
    normalize_text(&decoded)
}

fn normalize_text(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let trimmed = cleaned.trim();
    (!trimmed.is_empty()).then(|| trimmed.nfc().collect())
}

fn decode_pdf_string(object: &Object) -> Option<String> {
    let bytes = object.as_str().ok()?;
    let utf16 = |rest: &[u8], big_endian: bool| {
        let units = rest.as_chunks::<2>().0.iter().map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        });
        char::decode_utf16(units)
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
    };
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        Some(utf16(rest, true))
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        Some(utf16(rest, false))
    } else if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(String::from_utf8_lossy(rest).into_owned())
    } else {
        lopdf::decode_text_string(object).ok()
    }
}

fn parse_rectangle(doc: &Document, object: &Object) -> Option<[f64; 4]> {
    let array = resolve(doc, object)?.as_array().ok()?;
    if array.len() != 4 {
        return None;
    }
    let mut coords = [0.0; 4];
    for (slot, item) in coords.iter_mut().zip(array) {
        *slot = resolve(doc, item).and_then(as_f64)?;
    }
    Some([
        coords[0].min(coords[2]),
        coords[1].min(coords[3]),
        coords[0].max(coords[2]),
        coords[1].max(coords[3]),
    ])
}

fn as_f64(object: &Object) -> Option<f64> {
    match *object {
        Object::Integer(n) => Some(n as f64),
        Object::Real(n) => Some(f64::from(n)),
        _ => None,
    }
}

fn dict_get_resolved<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Object> {
    resolve(doc, dict.get(key).ok()?)
}

fn resolve<'a>(doc: &'a Document, mut object: &'a Object) -> Option<&'a Object> {
    for _ in 0..16 {
        match *object {
            Object::Reference(id) => object = doc.objects.get(&id)?,
            _ => return Some(object),
        }
    }
    None
}
