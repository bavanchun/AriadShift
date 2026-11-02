//! Append isolated, marked streams without modifying original page content.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content, dictionary};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    document::DecodeBudget,
    tools::{metadata::catalog_object_id, nup::effective_version},
    xobject,
};

/// Key identifying streams created by the drawing layer.
pub const MARKER_KEY: &[u8] = b"AriadShift";
/// Marked watermark stream value.
pub const MARKER_WATERMARK: &[u8] = b"Watermark";
/// Marked pagination stream value.
pub const MARKER_PAGINATION: &[u8] = b"Pagination";
/// Marked background stream value.
pub const MARKER_BACKGROUND: &[u8] = b"Background";
/// Marked colour-filter stream value.
pub const MARKER_COLOR_FILTER: &[u8] = b"ColorFilter";
/// Marked plain-content stream value.
pub const MARKER_CONTENT: &[u8] = b"Content";
/// Isolation prefix stream value.
pub const MARKER_WRAP_BEGIN: &[u8] = b"WrapBegin";
/// Isolation suffix stream value.
pub const MARKER_WRAP_END: &[u8] = b"WrapEnd";
/// Name of the optional content group used for removable marks.
pub const WATERMARK_GROUP_NAME: &str = "AriadShift Watermark";

/// Semantic marking for a drawing stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Marking {
    Watermark,
    Header,
    Footer,
    Background,
    ColorFilter,
    Content,
}

impl Marking {
    fn marker(self) -> &'static [u8] {
        match self {
            Self::Watermark => MARKER_WATERMARK,
            Self::Header | Self::Footer => MARKER_PAGINATION,
            Self::Background => MARKER_BACKGROUND,
            Self::ColorFilter => MARKER_COLOR_FILTER,
            Self::Content => MARKER_CONTENT,
        }
    }
}

/// Placement relative to existing page content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerPosition {
    Under,
    Over,
}

fn resolve<'a>(doc: &'a Document, mut object: &'a Object) -> Result<&'a Object, PdfError> {
    for _ in 0..8 {
        match object {
            Object::Reference(id) => {
                object = doc.get_object(*id).map_err(|_| PdfError::Malformed)?
            }
            _ => return Ok(object),
        }
    }
    Err(PdfError::Malformed)
}
fn dict_clone(doc: &Document, value: &Object) -> Result<Dictionary, PdfError> {
    resolve(doc, value)?
        .as_dict()
        .cloned()
        .map_err(|_| PdfError::Malformed)
}
fn array_clone(doc: &Document, value: &Object) -> Result<Vec<Object>, PdfError> {
    resolve(doc, value)?
        .as_array()
        .cloned()
        .map_err(|_| PdfError::Malformed)
}

/// Pick an unused resource name in the given category, preserving existing entries.
pub fn insert_resource(
    doc: &Document,
    resources: &mut Dictionary,
    category: &[u8],
    prefix: &str,
    value: Object,
) -> Result<Vec<u8>, PdfError> {
    let mut entries = match resources.get(category) {
        Ok(v) => dict_clone(doc, v)?,
        Err(_) => Dictionary::new(),
    };
    for index in 1..=100_000 {
        let name = format!("AS{prefix}{index}").into_bytes();
        if !entries.has(&name) {
            entries.set(name.clone(), value);
            resources.set(category.to_vec(), Object::Dictionary(entries));
            return Ok(name);
        }
    }
    Err(PdfError::InvalidParams("too many page resources".into()))
}

fn contents(doc: &Document, page: ObjectId) -> Result<Vec<Object>, PdfError> {
    let page = doc.get_dictionary(page).map_err(|_| PdfError::Malformed)?;
    let Ok(value) = page.get(b"Contents") else {
        return Ok(Vec::new());
    };
    match resolve(doc, value)? {
        Object::Array(items) => Ok(items.clone()),
        Object::Stream(_) => Ok(vec![value.clone()]),
        _ => Err(PdfError::Malformed),
    }
}
fn marker<'a>(doc: &'a Document, value: &'a Object) -> Option<&'a [u8]> {
    resolve(doc, value)
        .ok()?
        .as_stream()
        .ok()?
        .dict
        .get(MARKER_KEY)
        .ok()?
        .as_name()
        .ok()
}
fn checked_balance(
    doc: &Document,
    items: &[Object],
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let mut bytes = Vec::new();
    for item in items {
        ctx.check()?;
        let stream = resolve(doc, item)?
            .as_stream()
            .map_err(|_| PdfError::Malformed)?;
        let decoded = if stream.dict.has(b"Filter") {
            budget.decode(stream)?.ok_or(PdfError::Malformed)?
        } else {
            budget.charge(stream.content.len())?;
            stream.content.clone()
        };
        bytes.extend_from_slice(&decoded);
        bytes.push(b'\n');
    }
    let content = Content::decode(&bytes).map_err(|_| PdfError::Malformed)?;
    let mut depth = 0usize;
    for op in content.operations {
        match op.operator.as_str() {
            "q" => {
                depth = depth.checked_add(1).ok_or(PdfError::Malformed)?;
                if depth > 100_000 {
                    return Err(PdfError::Malformed);
                }
            }
            "Q" => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    PdfError::InvalidParams("cannot isolate unbalanced page content".into())
                })?;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(PdfError::InvalidParams(
            "cannot isolate unbalanced page content".into(),
        ));
    }
    Ok(())
}

fn check_wrapped(
    doc: &Document,
    items: &[Object],
    begin: usize,
    end: usize,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    for (index, item) in items.iter().enumerate() {
        if index > begin && index < end {
            continue;
        }
        ctx.check()?;
        let stream = resolve(doc, item)?
            .as_stream()
            .map_err(|_| PdfError::Malformed)?;
        let data = if stream.dict.has(b"Filter") {
            budget.decode(stream)?.ok_or(PdfError::Malformed)?
        } else {
            budget.charge(stream.content.len())?;
            stream.content.clone()
        };
        let ops = Content::decode(&data)
            .map_err(|_| PdfError::Malformed)?
            .operations;
        if index == begin || index == end {
            let expected = if index == begin { "q" } else { "Q" };
            if ops.len() != 1 || ops[0].operator != expected || !ops[0].operands.is_empty() {
                return Err(PdfError::InvalidParams(
                    "invalid page isolation markers".into(),
                ));
            }
        } else {
            if ops.first().map(|v| v.operator.as_str()) != Some("q")
                || ops.last().map(|v| v.operator.as_str()) != Some("Q")
            {
                return Err(PdfError::InvalidParams(
                    "cannot isolate existing page content".into(),
                ));
            }
            let mut depth = 0usize;
            for (op_index, op) in ops.iter().enumerate() {
                match op.operator.as_str() {
                    "q" => depth = depth.checked_add(1).ok_or(PdfError::Malformed)?,
                    "Q" => depth = depth.checked_sub(1).ok_or(PdfError::Malformed)?,
                    _ => {}
                }
                if depth == 0 && op_index + 1 != ops.len() {
                    return Err(PdfError::InvalidParams(
                        "cannot isolate existing page content".into(),
                    ));
                }
            }
            if depth != 0 {
                return Err(PdfError::Malformed);
            }
        }
    }
    checked_balance(doc, &items[begin + 1..end], budget, ctx)
}
fn stream(doc: &mut Document, kind: &[u8], bytes: Vec<u8>) -> Object {
    let mut dict = Dictionary::new();
    dict.set(MARKER_KEY.to_vec(), Object::Name(kind.to_vec()));
    let mut stream = Stream::new(dict, bytes);
    let _ = stream.compress();
    Object::Reference(doc.add_object(Object::Stream(stream)))
}

fn watermark_group(doc: &mut Document) -> Result<ObjectId, PdfError> {
    let catalog_id = catalog_object_id(doc)?;
    let catalog = doc
        .get_dictionary(catalog_id)
        .map_err(|_| PdfError::Malformed)?;
    let mut props = match catalog.get(b"OCProperties") {
        Ok(v) => dict_clone(doc, v)?,
        Err(_) => Dictionary::new(),
    };
    let mut groups = match props.get(b"OCGs") {
        Ok(v) => array_clone(doc, v)?,
        Err(_) => Vec::new(),
    };
    for entry in &groups {
        let Ok(id) = entry.as_reference() else {
            continue;
        };
        let Ok(group) = dict_clone(doc, entry) else {
            continue;
        };
        let name = group.get(b"Name").ok().and_then(|v| v.as_str().ok());
        let creator = group
            .get(b"Usage")
            .ok()
            .and_then(|v| dict_clone(doc, v).ok())
            .and_then(|v| {
                v.get(b"CreatorInfo")
                    .ok()
                    .and_then(|v| dict_clone(doc, v).ok())
            });
        let is_ours = creator.as_ref().is_some_and(|v| {
            v.get(b"Creator").ok().and_then(|v| v.as_str().ok()) == Some(b"AriadShift")
                && v.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Artwork")
        });
        if name == Some(WATERMARK_GROUP_NAME.as_bytes()) && is_ours {
            return Ok(id);
        }
    }
    let group = dictionary! {
        "Type" => "OCG", "Name" => Object::string_literal(WATERMARK_GROUP_NAME),
        "Usage" => dictionary! { "CreatorInfo" => dictionary! { "Creator" => Object::string_literal("AriadShift"), "Subtype" => "Artwork" }, "PageElement" => dictionary! { "Subtype" => "FG" } },
    };
    let id = doc.add_object(group);
    groups.push(Object::Reference(id));
    props.set("OCGs", Object::Array(groups));
    let mut defaults = match props.get(b"D") {
        Ok(v) => dict_clone(doc, v)?,
        Err(_) => Dictionary::new(),
    };
    let mut order = match defaults.get(b"Order") {
        Ok(v) => array_clone(doc, v)?,
        Err(_) => Vec::new(),
    };
    order.push(Object::Reference(id));
    defaults.set("Order", Object::Array(order));
    props.set("D", Object::Dictionary(defaults));
    doc.get_dictionary_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .set("OCProperties", Object::Dictionary(props));
    Ok(id)
}

/// Append a self-contained drawing stream, materializing page resources.
///
/// `body` consists of valid PDF content operators and must be balanced on its own.
/// Resources required by `body` should already be inserted in `resources`.
pub fn append(
    pdf: &mut PdfDoc,
    page: ObjectId,
    mut resources: Dictionary,
    body: &[u8],
    marking: Marking,
    position: LayerPosition,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let mut budget = pdf.decode_budget();
    let doc = pdf.inner_mut()?;
    let mut items = contents(doc, page)?;
    if position == LayerPosition::Over {
        let begins: Vec<_> = items
            .iter()
            .enumerate()
            .filter_map(|(i, v)| (marker(doc, v) == Some(MARKER_WRAP_BEGIN)).then_some(i))
            .collect();
        let ends: Vec<_> = items
            .iter()
            .enumerate()
            .filter_map(|(i, v)| (marker(doc, v) == Some(MARKER_WRAP_END)).then_some(i))
            .collect();
        if begins.is_empty() && ends.is_empty() {
            checked_balance(doc, &items, &mut budget, ctx)?;
            let begin = stream(doc, MARKER_WRAP_BEGIN, b"q\n".to_vec());
            let end = stream(doc, MARKER_WRAP_END, b"Q\n".to_vec());
            items.insert(0, begin);
            items.push(end);
        } else if begins.len() != 1 || ends.len() != 1 || begins[0] >= ends[0] {
            return Err(PdfError::InvalidParams(
                "cannot isolate existing page content".into(),
            ));
        } else {
            check_wrapped(doc, &items, begins[0], ends[0], &mut budget, ctx)?;
        }
    }
    let mut bytes = b"q\n".to_vec();
    match marking {
        Marking::Watermark => {
            let ocg = watermark_group(doc)?;
            let name = insert_resource(
                doc,
                &mut resources,
                b"Properties",
                "oc",
                Object::Reference(ocg),
            )?;
            bytes.extend_from_slice(b"/OC /");
            bytes.extend_from_slice(&name);
            bytes.extend_from_slice(
                b" BDC\n/Artifact << /Type /Pagination /Subtype /Watermark >> BDC\n",
            );
        }
        Marking::Header => {
            bytes.extend_from_slice(b"/Artifact << /Type /Pagination /Subtype /Header >> BDC\n")
        }
        Marking::Footer => {
            bytes.extend_from_slice(b"/Artifact << /Type /Pagination /Subtype /Footer >> BDC\n")
        }
        Marking::Background | Marking::ColorFilter => bytes.extend_from_slice(b"/Artifact BMC\n"),
        Marking::Content => {}
    }
    bytes.extend_from_slice(body);
    bytes.push(b'\n');
    match marking {
        Marking::Watermark => bytes.extend_from_slice(b"EMC\nEMC\n"),
        Marking::Content => {}
        _ => bytes.extend_from_slice(b"EMC\n"),
    }
    bytes.extend_from_slice(b"Q\n");
    let added = stream(doc, marking.marker(), bytes);
    match position {
        LayerPosition::Under => items.insert(0, added),
        LayerPosition::Over => items.push(added),
    }
    let dict = doc
        .get_dictionary_mut(page)
        .map_err(|_| PdfError::Malformed)?;
    dict.set("Resources", Object::Dictionary(resources));
    dict.set("Contents", Object::Array(items));
    Ok(())
}

/// Raise the PDF version when a transparency or blend state is used.
pub fn require_transparency(doc: &mut Document) {
    if effective_version(doc).as_str() < "1.4" {
        doc.version = "1.4".into();
    }
}

/// Materialize inherited resources before adding drawing resources.
#[must_use]
pub fn page_resources(doc: &Document, page: ObjectId) -> Dictionary {
    xobject::resolve_page_resources(doc, page)
}
