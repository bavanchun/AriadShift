//! Bounded content interpretation shared by watermark detection and removal.

use std::collections::{BTreeMap, HashMap, HashSet};

use ariad_core::{
    limits::Limits,
    reader::pdf::{MAX_PAGE_OPERATORS, PdfTextDecoder, ReadError},
    tools::params::WatermarkDetectParams,
};
use lopdf::{
    Dictionary, Document, Object, ObjectId, Stream,
    content::{Content, Operation},
};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::{
    PdfDoc, PdfError, ToolCtx,
    color::{page_ids, page_resources, resolve},
    document::DecodeBudget,
    tools::{outline::decode_pdf_object_string, watermark_identity::candidate_id},
    writer::{canonical_dictionary_hash, canonical_object_hash, sha256_chunks},
};

const MAX_FORM_VISITS: usize = 100_000;
const MAX_DRAWABLES: usize = 1_000_000;
const MAX_MARK_DEPTH: usize = 128;
const MAX_GROUP_COMPARISONS: usize = 2_000_000;
const MAX_CANDIDATES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix([f64; 6]);
impl Matrix {
    const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    fn then(self, other: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = other.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }
    fn point(self, x: f64, y: f64) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }
    fn rect(self, r: [f64; 4]) -> [f64; 4] {
        let points = [
            self.point(r[0], r[1]),
            self.point(r[0], r[3]),
            self.point(r[2], r[1]),
            self.point(r[2], r[3]),
        ];
        points.into_iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |a, p| {
                [
                    a[0].min(p[0]),
                    a[1].min(p[1]),
                    a[2].max(p[0]),
                    a[3].max(p[1]),
                ]
            },
        )
    }
    fn angle(self) -> f64 {
        self.0[1].atan2(self.0[0]).to_degrees()
    }
    fn similar(self, other: Self) -> bool {
        let a = self.0;
        let b = other.0;
        let scale = |m: [f64; 6]| [m[0].hypot(m[1]), m[2].hypot(m[3])];
        let sa = scale(a);
        let sb = scale(b);
        let radians = ((self.angle() - other.angle() + 180.0).rem_euclid(360.0) - 180.0)
            .to_radians()
            .abs();
        (a[4] - b[4]).abs() <= 2.0
            && (a[5] - b[5]).abs() <= 2.0
            && radians <= 0.01
            && (0..2).all(|i| (sa[i] - sb[i]).abs() <= 0.01 * sa[i].max(sb[i]).max(1e-9))
    }
}
fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(v) => Some(*v as f64),
        Object::Real(v) => Some(f64::from(*v)),
        _ => None,
    }
    .filter(|v| v.is_finite())
}
fn matrix(values: &[Object]) -> Option<Matrix> {
    if values.len() != 6 {
        return None;
    }
    Some(Matrix([
        number(&values[0])?,
        number(&values[1])?,
        number(&values[2])?,
        number(&values[3])?,
        number(&values[4])?,
        number(&values[5])?,
    ]))
}
fn dict<'a>(doc: &'a Document, object: &'a Object, depth: u16) -> Option<&'a Dictionary> {
    resolve(doc, object, depth)?.as_dict().ok()
}
fn resource<'a>(
    doc: &'a Document,
    resources: &'a Dictionary,
    category: &[u8],
    name: &[u8],
    depth: u16,
) -> Option<&'a Object> {
    dict(doc, resources.get(category).ok()?, depth)?
        .get(name)
        .ok()
}
fn rectangle(doc: &Document, object: &Object, depth: u16) -> Option<[f64; 4]> {
    let a = resolve(doc, object, depth)?.as_array().ok()?;
    if a.len() != 4 {
        return None;
    }
    Some([
        number(resolve(doc, &a[0], depth)?)?,
        number(resolve(doc, &a[1], depth)?)?,
        number(resolve(doc, &a[2], depth)?)?,
        number(resolve(doc, &a[3], depth)?)?,
    ])
}
fn name_matches(value: &str) -> bool {
    let normalized: String = value.nfc().flat_map(char::to_lowercase).collect();
    [
        "watermark",
        "draft",
        "confidential",
        "sample",
        "copy",
        "trial",
        "bản nháp",
        "mật",
        "mẫu",
    ]
    .iter()
    .any(|word| normalized.contains(word))
}

/// One operation span in a page stream or nested Form XObject.
#[derive(Clone, Debug)]
pub(super) struct Occurrence {
    pub page: u32,
    pub stream: ObjectId,
    pub slot: usize,
    /// Indices of enclosing `Do` operators, starting at the page content stream.
    pub path: Vec<usize>,
    pub start: usize,
    pub end: usize,
    pub annotation: Option<ObjectId>,
    pub own_stream: bool,
    pub ambiguous: bool,
}

/// Candidate metadata returned to the caller; occurrence details stay private.
#[derive(Clone, Debug, Serialize)]
pub(super) struct Candidate {
    pub id: String,
    pub kind: &'static str,
    pub origin: &'static str,
    pub summary: String,
    pub pages: Vec<u32>,
    pub bbox: [f64; 4],
    pub rotate: i32,
    pub preview: Preview,
    pub confidence: f32,
    pub signals: Vec<&'static str>,
    #[serde(skip)]
    pub occurrences: Vec<Occurrence>,
    #[serde(skip)]
    pub ocg: Option<ObjectId>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Preview {
    pub page: u32,
    pub bbox: [f64; 4],
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Detection {
    pub page_count: u32,
    pub candidates: Vec<Candidate>,
    #[serde(skip_serializing_if = "is_zero")]
    pub unsupported_images: usize,
}
fn is_zero(value: &usize) -> bool {
    *value == 0
}

// A Form without nested XObjects only depends on resource entries named by its own operators.
// Keep the whole dictionary when indirect operands or nested XObjects make that proof incomplete.
fn used_form_resources(
    doc: &Document,
    resources: &Dictionary,
    content: &Content,
    limit: u16,
) -> Dictionary {
    let mut names = HashSet::new();
    for op in &content.operations {
        if matches!(op.operator.as_str(), "BI" | "ID" | "EI") {
            return resources.clone();
        }
        let mut stack: Vec<(&Object, usize)> = op.operands.iter().map(|value| (value, 0)).collect();
        while let Some((value, depth)) = stack.pop() {
            if depth > 64 {
                return resources.clone();
            }
            match value {
                Object::Name(name) => {
                    names.insert(name.clone());
                    if names.len() > 100_000 {
                        return resources.clone();
                    }
                }
                Object::Array(items) => stack.extend(items.iter().map(|v| (v, depth + 1))),
                Object::Dictionary(items) => {
                    stack.extend(items.iter().map(|(_, v)| (v, depth + 1)))
                }
                Object::Reference(_) | Object::Stream(_) => return resources.clone(),
                _ => {}
            }
        }
    }
    let mut selected = Dictionary::new();
    for (category, value) in resources.iter() {
        if category == b"ProcSet" {
            continue;
        }
        let Some(entries) = dict(doc, value, limit) else {
            return resources.clone();
        };
        let mut used = Dictionary::new();
        for (name, value) in entries.iter() {
            if names.contains(name) {
                used.set(name.clone(), value.clone());
            }
        }
        if !used.is_empty() {
            selected.set(category.clone(), used);
        }
    }
    selected
}

#[derive(Clone)]
struct Graphics {
    ctm: Matrix,
    alpha: f64,
    text: Matrix,
    line: Matrix,
    font: Option<std::rc::Rc<Object>>,
    size: f64,
    leading: f64,
}
impl Default for Graphics {
    fn default() -> Self {
        Self {
            ctm: Matrix::IDENTITY,
            alpha: 1.0,
            text: Matrix::IDENTITY,
            line: Matrix::IDENTITY,
            font: None,
            size: 0.0,
            leading: 0.0,
        }
    }
}

#[derive(Clone)]
struct Drawable {
    kind: &'static str,
    key: Vec<u8>,
    context: [u8; 32],
    occurrence: Occurrence,
    bbox: [f64; 4],
    ctm: Matrix,
    alpha: f64,
    size: f64,
    label: Option<String>,
}

#[derive(Clone)]
struct Mark {
    kind: Option<&'static str>,
    key: Vec<u8>,
    label: Option<String>,
    own: bool,
    start: usize,
    root_stream: ObjectId,
    slot: usize,
    path: Vec<usize>,
    bbox: Option<[f64; 4]>,
    alpha: f64,
}
fn union(a: Option<[f64; 4]>, b: [f64; 4]) -> [f64; 4] {
    a.map_or(b, |a| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    })
}
fn properties<'a>(
    doc: &'a Document,
    resources: &'a Dictionary,
    name: &[u8],
    depth: u16,
) -> Option<&'a Object> {
    let entries = dict(doc, resources.get(b"Properties").ok()?, depth)?;
    entries.get(name).ok()
}
fn object_id_bytes(id: ObjectId) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(6);
    bytes.extend_from_slice(&id.0.to_be_bytes());
    bytes.extend_from_slice(&id.1.to_be_bytes());
    bytes
}
fn ocg_info(doc: &Document, object: &Object, limit: u16) -> Option<(Vec<u8>, String, bool)> {
    ocg_info_inner(doc, object, limit, &mut HashSet::new())
}
fn ocg_info_inner(
    doc: &Document,
    object: &Object,
    limit: u16,
    visited: &mut HashSet<ObjectId>,
) -> Option<(Vec<u8>, String, bool)> {
    let id = object.as_reference().ok()?;
    if visited.len() >= usize::from(limit.min(64)) || !visited.insert(id) {
        return None;
    }
    let group = dict(doc, object, limit)?;
    if group
        .get(b"Type")
        .ok()
        .and_then(|v| resolve(doc, v, limit))
        .and_then(|v| v.as_name().ok())
        == Some(b"OCMD")
    {
        let entry = group.get(b"OCGs").ok()?;
        let groups = resolve(doc, entry, limit)?;
        if let Ok(entries) = groups.as_array() {
            if entries.len() != 1 {
                return None;
            }
            return ocg_info_inner(doc, &entries[0], limit, visited);
        }
        return ocg_info_inner(doc, entry, limit, visited);
    }
    let name = group
        .get(b"Name")
        .ok()
        .and_then(|v| resolve(doc, v, limit))
        .and_then(decode_pdf_object_string)
        .unwrap_or_default();
    let usage = group.get(b"Usage").ok().and_then(|v| dict(doc, v, limit));
    let page_mark = usage
        .and_then(|d| d.get(b"PageElement").ok())
        .and_then(|v| dict(doc, v, limit))
        .and_then(|d| d.get(b"Subtype").ok())
        .and_then(|v| resolve(doc, v, limit))
        .and_then(|v| v.as_name().ok())
        == Some(b"Watermark");
    if !name_matches(&name) && !page_mark {
        return None;
    }
    let creator = usage
        .and_then(|d| d.get(b"CreatorInfo").ok())
        .and_then(|v| dict(doc, v, limit));
    let own = name == crate::draw::layer::WATERMARK_GROUP_NAME
        && creator
            .and_then(|d| d.get(b"Creator").ok())
            .and_then(|v| resolve(doc, v, limit))
            .and_then(decode_pdf_object_string)
            .as_deref()
            == Some("AriadShift")
        && creator
            .and_then(|d| d.get(b"Subtype").ok())
            .and_then(|v| resolve(doc, v, limit))
            .and_then(|v| v.as_name().ok())
            == Some(b"Artwork");
    Some((object_id_bytes(id), name, own))
}
fn marker(
    doc: &Document,
    resources: &Dictionary,
    op: &Operation,
    limit: u16,
) -> Option<(&'static str, Vec<u8>, Option<String>, bool)> {
    if op.operator != "BDC" {
        return None;
    }
    let tag = op.operands.first()?.as_name().ok()?;
    let mut property = op.operands.get(1)?;
    if let Ok(name) = property.as_name() {
        property = properties(doc, resources, name, limit)?;
    }
    if tag == b"OC" {
        let (key, label, own) = ocg_info(doc, property, limit)?;
        Some(("ocg", key, Some(label), own))
    } else if tag == b"Artifact" {
        let d = dict(doc, property, limit)?;
        let subtype = d
            .get(b"Subtype")
            .ok()
            .and_then(|v| resolve(doc, v, limit))
            .and_then(|v| v.as_name().ok());
        (subtype == Some(b"Watermark")).then_some(("artifact", Vec::new(), None, false))
    } else {
        None
    }
}

struct Scanner<'a> {
    pdf: &'a PdfDoc,
    doc: &'a Document,
    limits: &'a Limits,
    ctx: &'a ToolCtx,
    budget: DecodeBudget,
    decoder: PdfTextDecoder<'a>,
    active_forms: HashSet<ObjectId>,
    form_visits: usize,
    drawables: Vec<Drawable>,
    image_cache: HashMap<ObjectId, Option<Vec<u8>>>,
    unsupported_images: usize,
}
impl<'a> Scanner<'a> {
    fn nested_context(
        &mut self,
        content: &Content,
        resources: &Dictionary,
        depth: u16,
        active: &mut HashSet<ObjectId>,
    ) -> Result<[u8; 32], PdfError> {
        if depth > self.limits.max_nesting_depth.min(64) {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth.min(64),
            }));
        }
        let estimate = crate::color::targeted_document::dictionary_size(
            resources,
            0,
            self.limits.max_nesting_depth,
        )?;
        self.budget.charge(estimate.saturating_mul(8))?;
        let referenced =
            used_form_resources(self.doc, resources, content, self.limits.max_nesting_depth);
        let mut parts = vec![canonical_dictionary_hash(&referenced)];
        for op in &content.operations {
            if op.operator != "Do" {
                continue;
            }
            self.ctx.check()?;
            let Some(name) = op.operands.first().and_then(|v| v.as_name().ok()) else {
                return Err(PdfError::Malformed);
            };
            let Some(id) = resource(
                self.doc,
                resources,
                b"XObject",
                name,
                self.limits.max_nesting_depth,
            )
            .and_then(|v| v.as_reference().ok()) else {
                return Err(PdfError::Malformed);
            };
            let stream = self
                .doc
                .get_object(id)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?;
            if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
                continue;
            }
            if !active.insert(id) {
                return Err(PdfError::Malformed);
            }
            let estimate = crate::color::targeted_document::dictionary_size(
                &stream.dict,
                0,
                self.limits.max_nesting_depth,
            )?;
            self.budget.charge(
                stream
                    .content
                    .len()
                    .saturating_add(estimate.saturating_mul(8)),
            )?;
            let stream = stream.clone();
            let bytes = self.decode(&stream)?;
            let nested = Content::decode(&bytes).map_err(|_| PdfError::Malformed)?;
            let nested_resources = match stream.dict.get(b"Resources") {
                Ok(value) => dict(self.doc, value, self.limits.max_nesting_depth)
                    .cloned()
                    .ok_or(PdfError::Malformed)?,
                Err(_) => resources.clone(),
            };
            parts.push(self.nested_context(&nested, &nested_resources, depth + 1, active)?);
            active.remove(&id);
        }
        let chunks: Vec<&[u8]> = parts.iter().map(|part| part.as_slice()).collect();
        Ok(sha256_chunks(&chunks))
    }

    fn decode(&mut self, stream: &Stream) -> Result<Vec<u8>, PdfError> {
        self.ctx.check()?;
        if stream.dict.has(b"Filter") {
            self.budget.decode(stream)?.ok_or(PdfError::Malformed)
        } else {
            self.budget.charge(stream.content.len())?;
            Ok(stream.content.clone())
        }
    }
    fn emit(&mut self, drawable: Drawable) -> Result<(), PdfError> {
        if drawable
            .bbox
            .iter()
            .chain(drawable.ctm.0.iter())
            .any(|value| !value.is_finite())
            || !drawable.alpha.is_finite()
            || !drawable.size.is_finite()
        {
            return Err(PdfError::Malformed);
        }
        if self.drawables.len() >= MAX_DRAWABLES {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: MAX_DRAWABLES,
            }));
        }
        let bytes = std::mem::size_of::<Drawable>()
            .saturating_add(drawable.key.len())
            .saturating_add(
                drawable
                    .occurrence
                    .path
                    .len()
                    .saturating_mul(std::mem::size_of::<usize>()),
            )
            .saturating_add(drawable.label.as_ref().map_or(0, String::len));
        self.budget.charge(bytes)?;
        self.drawables.push(drawable);
        Ok(())
    }
    fn text(
        &mut self,
        op: &Operation,
        state: &Graphics,
    ) -> Result<(Vec<u8>, Option<String>, bool, usize), PdfError> {
        let value = match op.operator.as_str() {
            "'" => op.operands.first(),
            "\"" => op.operands.get(2),
            _ => op.operands.first(),
        };
        let mut raw = Vec::new();
        if let Some(Object::String(bytes, _)) = value {
            self.budget.charge(bytes.len())?;
            raw.extend_from_slice(bytes);
        }
        if let Some(Object::Array(values)) = value {
            for value in values {
                if let Object::String(bytes, _) = value {
                    self.budget.charge(bytes.len())?;
                    raw.extend_from_slice(bytes);
                }
            }
        }
        let has_to_unicode = state
            .font
            .as_ref()
            .and_then(|font| dict(self.doc, font, self.limits.max_nesting_depth))
            .is_some_and(|font| font.has(b"ToUnicode"));
        let decoded = if let Some(font) = &state.font {
            self.decoder.decode(font, &raw)?
        } else {
            None
        };
        let glyphs = decoded.as_ref().map_or(raw.len(), |v| v.chars().count());
        Ok((raw, decoded, has_to_unicode, glyphs))
    }
    #[allow(clippy::too_many_arguments)]
    fn content(
        &mut self,
        page: u32,
        root: ObjectId,
        slot: usize,
        path: &[usize],
        content: &Content,
        resources: &Dictionary,
        state: &mut Graphics,
        page_box: [f64; 4],
        own_stream: bool,
        outer_marker: bool,
        depth: u16,
        page_ops: &mut u64,
        saved: &mut Vec<Graphics>,
        marked: &mut Vec<Mark>,
    ) -> Result<(), PdfError> {
        if depth > self.limits.max_nesting_depth.min(64) {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth.min(64),
            }));
        }
        *page_ops = page_ops.saturating_add(content.operations.len() as u64);
        if *page_ops > MAX_PAGE_OPERATORS {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: MAX_PAGE_OPERATORS as usize,
            }));
        }
        for (index, op) in content.operations.iter().enumerate() {
            if index % 1024 == 0 {
                self.ctx.check()?;
            }
            let marked_outer = outer_marker || marked.iter().any(|m| m.kind.is_some());
            match op.operator.as_str() {
                "q" => {
                    if saved.len() >= 4096 {
                        return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                            limit: 4096,
                        }));
                    }
                    saved.push(state.clone());
                }
                "Q" => {
                    let text = state.text;
                    let line = state.line;
                    *state = saved.pop().ok_or(PdfError::Malformed)?;
                    state.text = text;
                    state.line = line;
                }
                "cm" => {
                    if let Some(m) = matrix(&op.operands) {
                        state.ctm = state.ctm.then(m);
                    }
                }
                "gs" => {
                    if let Some(name) = op.operands.first().and_then(|v| v.as_name().ok())
                        && let Some(gs) = resource(
                            self.doc,
                            resources,
                            b"ExtGState",
                            name,
                            self.limits.max_nesting_depth,
                        )
                        && let Some(gs) = dict(self.doc, gs, self.limits.max_nesting_depth)
                        && let Some(alpha) = gs.get(b"ca").ok().and_then(number)
                    {
                        state.alpha = alpha.clamp(0.0, 1.0);
                    }
                }
                "BT" => {
                    state.text = Matrix::IDENTITY;
                    state.line = Matrix::IDENTITY;
                }
                "Tf" => {
                    if let (Some(name), Some(size)) = (
                        op.operands.first().and_then(|v| v.as_name().ok()),
                        op.operands.get(1).and_then(number),
                    ) {
                        let font = resource(
                            self.doc,
                            resources,
                            b"Font",
                            name,
                            self.limits.max_nesting_depth,
                        );
                        if let Some(font) = font {
                            self.budget
                                .charge(crate::color::targeted_document::resource_size(
                                    font,
                                    0,
                                    self.limits.max_nesting_depth,
                                )?)?;
                            state.font = Some(std::rc::Rc::new(font.clone()));
                        } else {
                            state.font = None;
                        }
                        state.size = size;
                    }
                }
                "Tm" => {
                    if let Some(m) = matrix(&op.operands) {
                        state.text = m;
                        state.line = m;
                    }
                }
                "Td" | "TD" => {
                    if let (Some(x), Some(y)) = (
                        op.operands.first().and_then(number),
                        op.operands.get(1).and_then(number),
                    ) {
                        if op.operator == "TD" {
                            state.leading = -y;
                        }
                        state.line = state.line.then(Matrix([1.0, 0.0, 0.0, 1.0, x, y]));
                        state.text = state.line;
                    }
                }
                "TL" => {
                    if let Some(value) = op.operands.first().and_then(number) {
                        state.leading = value;
                    }
                }
                "T*" => {
                    state.line = state
                        .line
                        .then(Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -state.leading]));
                    state.text = state.line;
                }
                "BDC" | "BMC" => {
                    if marked.len() >= MAX_MARK_DEPTH {
                        return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                            limit: MAX_MARK_DEPTH as u16,
                        }));
                    }
                    let info = if marked_outer {
                        None
                    } else {
                        marker(self.doc, resources, op, self.limits.max_nesting_depth)
                    };
                    let (kind, key, label, own) = info.map_or(
                        (None, Vec::new(), None, false),
                        |(kind, key, label, own)| (Some(kind), key, label, own),
                    );
                    marked.push(Mark {
                        kind,
                        key,
                        label,
                        own: own && own_stream && path.is_empty(),
                        start: index,
                        root_stream: root,
                        slot,
                        path: path.to_vec(),
                        bbox: None,
                        alpha: state.alpha,
                    });
                }
                "EMC" => {
                    let mark = marked.pop().ok_or(PdfError::Malformed)?;
                    if let Some(kind) = mark.kind {
                        let same_stream = mark.root_stream == root
                            && mark.slot == slot
                            && mark.path == path
                            && mark.start <= index;
                        let key = if kind == "artifact" && same_stream {
                            let encoded = Content {
                                operations: content.operations[mark.start..=index].to_vec(),
                            }
                            .encode()
                            .map_err(|_| PdfError::Malformed)?;
                            self.budget.charge(encoded.len())?;
                            sha256_chunks(&[&encoded]).to_vec()
                        } else if kind == "artifact" {
                            sha256_chunks(&[
                                &object_id_bytes(mark.root_stream),
                                &mark.start.to_be_bytes(),
                            ])
                            .to_vec()
                        } else {
                            mark.key
                        };
                        self.emit(Drawable {
                            kind,
                            key,
                            context: [0; 32],
                            occurrence: Occurrence {
                                page,
                                stream: mark.root_stream,
                                slot: mark.slot,
                                path: mark.path,
                                start: mark.start,
                                end: index + 1,
                                annotation: None,
                                own_stream: mark.own && mark.root_stream == root,
                                ambiguous: !same_stream,
                            },
                            bbox: mark.bbox.unwrap_or(page_box),
                            ctm: state.ctm,
                            alpha: mark.alpha,
                            size: state.size,
                            label: mark.label,
                        })?;
                    }
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    if op.operator == "'" || op.operator == "\"" {
                        state.line =
                            state
                                .line
                                .then(Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -state.leading]));
                        state.text = state.line;
                    }
                    let (raw, decoded, has_to_unicode, glyphs) = self.text(op, state)?;
                    let transformed = state.ctm.then(state.text);
                    let width = state.size.abs() * (glyphs as f64) * 0.5;
                    let bbox =
                        transformed.rect([0.0, -state.size.abs() * 0.2, width, state.size.abs()]);
                    for mark in marked.iter_mut() {
                        if mark.kind.is_some() {
                            mark.bbox = Some(union(mark.bbox, bbox));
                        }
                    }
                    if !marked_outer && !raw.is_empty() {
                        let key = decoded.as_ref().filter(|_| has_to_unicode).map_or_else(
                            || sha256_chunks(&[b"raw", &raw]).to_vec(),
                            |text| sha256_chunks(&[b"unicode", text.as_bytes()]).to_vec(),
                        );
                        self.emit(Drawable {
                            kind: "repeated_text",
                            key,
                            context: [0; 32],
                            occurrence: Occurrence {
                                page,
                                stream: root,
                                slot,
                                path: path.to_vec(),
                                start: index,
                                end: index + 1,
                                annotation: None,
                                own_stream: false,
                                ambiguous: false,
                            },
                            bbox,
                            ctm: transformed,
                            alpha: state.alpha,
                            size: state.size.abs() * transformed.0[2].hypot(transformed.0[3]),
                            label: decoded,
                        })?;
                    }
                    state.text = state.text.then(Matrix([1.0, 0.0, 0.0, 1.0, width, 0.0]));
                }
                "Do" => {
                    if let Some(name) = op.operands.first().and_then(|v| v.as_name().ok())
                        && let Some(object) = resource(
                            self.doc,
                            resources,
                            b"XObject",
                            name,
                            self.limits.max_nesting_depth,
                        )
                        && let Ok(id) = object.as_reference()
                    {
                        let Some(source) = self
                            .doc
                            .get_object(id)
                            .ok()
                            .and_then(|v| v.as_stream().ok())
                        else {
                            continue;
                        };
                        let is_cached_image = source
                            .dict
                            .get(b"Subtype")
                            .ok()
                            .and_then(|v| v.as_name().ok())
                            == Some(b"Image")
                            && self.image_cache.contains_key(&id);
                        let dict_size = crate::color::targeted_document::dictionary_size(
                            &source.dict,
                            0,
                            self.limits.max_nesting_depth,
                        )?;
                        self.budget.charge(
                            (if is_cached_image {
                                0
                            } else {
                                source.content.len()
                            })
                            .saturating_add(dict_size.saturating_mul(8)),
                        )?;
                        let stream = if is_cached_image {
                            Stream::new(source.dict.clone(), Vec::new())
                        } else {
                            source.clone()
                        };
                        let subtype = stream
                            .dict
                            .get(b"Subtype")
                            .ok()
                            .and_then(|v| v.as_name().ok())
                            .unwrap_or_default();
                        let bbox = stream
                            .dict
                            .get(b"BBox")
                            .ok()
                            .and_then(|v| rectangle(self.doc, v, self.limits.max_nesting_depth))
                            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
                        let local = stream
                            .dict
                            .get(b"Matrix")
                            .ok()
                            .and_then(|v| v.as_array().ok())
                            .and_then(|v| matrix(v))
                            .unwrap_or(Matrix::IDENTITY);
                        let transformed = state.ctm.then(local);
                        let bbox = transformed.rect(bbox);
                        for mark in marked.iter_mut() {
                            if mark.kind.is_some() {
                                mark.bbox = Some(union(mark.bbox, bbox));
                            }
                        }
                        if subtype == b"Form" {
                            self.form_visits += 1;
                            if self.form_visits > MAX_FORM_VISITS {
                                return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                                    limit: MAX_FORM_VISITS,
                                }));
                            }
                            if !self.active_forms.insert(id) {
                                return Err(PdfError::Malformed);
                            }
                            let data = self.decode(&stream)?;
                            let inherited_size = crate::color::targeted_document::dictionary_size(
                                resources,
                                0,
                                self.limits.max_nesting_depth,
                            )?;
                            self.budget.charge(inherited_size.saturating_mul(8))?;
                            let form_resources = stream
                                .dict
                                .get(b"Resources")
                                .ok()
                                .and_then(|v| dict(self.doc, v, self.limits.max_nesting_depth))
                                .cloned()
                                .unwrap_or_else(|| resources.clone());
                            let form_content =
                                Content::decode(&data).map_err(|_| PdfError::Malformed)?;
                            let referenced = used_form_resources(
                                self.doc,
                                &form_resources,
                                &form_content,
                                self.limits.max_nesting_depth,
                            );
                            let estimate = crate::color::targeted_document::dictionary_size(
                                &referenced,
                                0,
                                self.limits.max_nesting_depth,
                            )?;
                            self.budget.charge(estimate.saturating_mul(8))?;
                            let mut font_set = false;
                            let mut font_may_restore = false;
                            // A nested Form may use the font selected by its caller without a Tf.
                            let mut uses_inherited_font =
                                form_content.operations.iter().any(|op| op.operator == "Do");
                            for op in &form_content.operations {
                                match op.operator.as_str() {
                                    "q" | "Q" => font_may_restore = true,
                                    "Tf" => font_set = true,
                                    "Tj" | "TJ" | "'" | "\"" if !font_set || font_may_restore => {
                                        uses_inherited_font = true
                                    }
                                    _ => {}
                                }
                            }
                            let mut hashes = vec![canonical_dictionary_hash(&referenced)];
                            if form_content.operations.iter().any(|op| op.operator == "Do") {
                                hashes.push(self.nested_context(
                                    &form_content,
                                    &form_resources,
                                    0,
                                    &mut HashSet::from([id]),
                                )?);
                            }
                            let font = if uses_inherited_font {
                                state.font.as_deref()
                            } else {
                                None
                            };
                            for object in [
                                font,
                                stream.dict.get(b"BBox").ok(),
                                stream.dict.get(b"Matrix").ok(),
                                stream.dict.get(b"Group").ok(),
                                stream.dict.get(b"OC").ok(),
                            ] {
                                if let Some(object) = object {
                                    let size = crate::color::targeted_document::resource_size(
                                        object,
                                        0,
                                        self.limits.max_nesting_depth,
                                    )?;
                                    self.budget.charge(size.saturating_mul(8))?;
                                    hashes.push(canonical_object_hash(object));
                                } else {
                                    hashes.push(sha256_chunks(&[b"missing"]));
                                }
                            }
                            let chunks: Vec<&[u8]> =
                                hashes.iter().map(|hash| hash.as_slice()).collect();
                            let context = sha256_chunks(&chunks);
                            let parent_index = self.drawables.len();
                            if !marked_outer {
                                self.emit(Drawable {
                                    kind: "repeated_xobject",
                                    key: sha256_chunks(&[subtype, &data]).to_vec(),
                                    context,
                                    occurrence: Occurrence {
                                        page,
                                        stream: root,
                                        slot,
                                        path: path.to_vec(),
                                        start: index,
                                        end: index + 1,
                                        annotation: None,
                                        own_stream: false,
                                        ambiguous: false,
                                    },
                                    bbox,
                                    ctm: transformed,
                                    alpha: state.alpha,
                                    size: 0.0,
                                    label: None,
                                })?;
                            }
                            let mut form_state = state.clone();
                            form_state.ctm = transformed;
                            let mut child_path = path.to_vec();
                            child_path.push(index);
                            let mut form_saved = Vec::new();
                            let mut form_marked = Vec::new();
                            let result = self.content(
                                page,
                                root,
                                slot,
                                &child_path,
                                &form_content,
                                &form_resources,
                                &mut form_state,
                                page_box,
                                false,
                                marked_outer,
                                depth.saturating_add(1),
                                page_ops,
                                &mut form_saved,
                                &mut form_marked,
                            );
                            self.active_forms.remove(&id);
                            result?;
                            if !form_saved.is_empty() || !form_marked.is_empty() {
                                return Err(PdfError::Malformed);
                            }
                            if !marked_outer {
                                let nested = &self.drawables[parent_index + 1..];
                                let size = nested.iter().map(|v| v.size).fold(0.0, f64::max);
                                let alpha = (!nested.is_empty()
                                    && nested.iter().all(|v| v.alpha < 0.99))
                                .then(|| {
                                    nested.iter().map(|v| v.alpha).fold(state.alpha, f64::min)
                                });
                                let labels: Vec<_> = nested
                                    .iter()
                                    .filter_map(|v| v.label.as_ref())
                                    .filter(|v| name_matches(v))
                                    .collect();
                                let label = (!labels.is_empty() && labels.len() * 2 > nested.len())
                                    .then(|| labels[0].clone());
                                if let Some(alpha) = alpha {
                                    self.drawables[parent_index].alpha = alpha;
                                }
                                self.drawables[parent_index].label = label;
                                self.drawables[parent_index].size = size;
                            }
                        } else if subtype == b"Image" && !marked_outer {
                            let key = if let Some(cached) = self.image_cache.get(&id) {
                                cached.clone()
                            } else {
                                match crate::images::decode_with_budget(
                                    self.pdf,
                                    id,
                                    self.ctx,
                                    &mut self.budget,
                                    self.limits.max_nesting_depth,
                                    false,
                                    false,
                                ) {
                                    Ok(image) => {
                                        // Masks and dimensions are part of identity so distinct visible images
                                        // cannot collapse into one candidate merely because pixels match.
                                        let mask = stream
                                            .dict
                                            .get(b"SMask")
                                            .ok()
                                            .map(canonical_object_hash)
                                            .unwrap_or([0; 32]);
                                        let explicit_mask = stream
                                            .dict
                                            .get(b"Mask")
                                            .ok()
                                            .map(canonical_object_hash)
                                            .unwrap_or([0; 32]);
                                        let space = format!("{:?}", image.color_space);
                                        let key = sha256_chunks(&[
                                            subtype,
                                            &image.width.to_be_bytes(),
                                            &image.height.to_be_bytes(),
                                            &[image.bits_per_component],
                                            space.as_bytes(),
                                            &mask,
                                            &explicit_mask,
                                            &image.pixels,
                                        ])
                                        .to_vec();
                                        self.image_cache.insert(id, Some(key.clone()));
                                        Some(key)
                                    }
                                    Err(crate::images::ImageError::Pdf(error)) => {
                                        return Err(error);
                                    }
                                    Err(_) => {
                                        self.image_cache.insert(id, None);
                                        self.unsupported_images += 1;
                                        None
                                    }
                                }
                            };
                            if let Some(key) = key {
                                self.emit(Drawable {
                                    kind: "repeated_xobject",
                                    key,
                                    context: [0; 32],
                                    occurrence: Occurrence {
                                        page,
                                        stream: root,
                                        slot,
                                        path: path.to_vec(),
                                        start: index,
                                        end: index + 1,
                                        annotation: None,
                                        own_stream: false,
                                        ambiguous: false,
                                    },
                                    bbox,
                                    ctm: transformed,
                                    alpha: state.alpha,
                                    size: 0.0,
                                    label: None,
                                })?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Page {
    bbox: [f64; 4],
    rotate: i32,
}

fn occurrence_identity(drawable: &Drawable) -> Vec<u8> {
    let mut identity = Vec::new();
    identity.extend_from_slice(drawable.kind.as_bytes());
    identity.push(0);
    identity.extend_from_slice(&(drawable.key.len() as u64).to_be_bytes());
    identity.extend_from_slice(&drawable.key);
    if matches!(
        drawable.kind,
        "repeated_text" | "repeated_xobject" | "annotation"
    ) {
        identity.extend_from_slice(&drawable.occurrence.page.to_be_bytes());
        identity.extend_from_slice(&object_id_bytes(drawable.occurrence.stream));
        identity.extend_from_slice(&(drawable.occurrence.slot as u64).to_be_bytes());
        for part in &drawable.occurrence.path {
            identity.extend_from_slice(&(*part as u64).to_be_bytes());
        }
        identity.extend_from_slice(&(drawable.occurrence.start as u64).to_be_bytes());
    }
    identity
}
fn centered(bbox: [f64; 4], page: [f64; 4]) -> bool {
    let width = (page[2] - page[0]).abs();
    let height = (page[3] - page[1]).abs();
    ((bbox[0] + bbox[2] - page[0] - page[2]) * 0.5).abs() <= width * 0.2
        && ((bbox[1] + bbox[3] - page[1] - page[3]) * 0.5).abs() <= height * 0.2
}
fn make_candidate(
    group: Vec<Drawable>,
    pages: &BTreeMap<u32, Page>,
    input_hash: &[u8; 32],
    query: &WatermarkDetectParams,
) -> Result<Candidate, PdfError> {
    let first = &group[0];
    let mut page_numbers: Vec<u32> = group.iter().map(|d| d.occurrence.page).collect();
    page_numbers.sort_unstable();
    page_numbers.dedup();
    let low_alpha = group.iter().any(|d| d.alpha < 0.6);
    let rotated = group.iter().any(|d| d.ctm.angle().abs() >= 10.0);
    let large = group.iter().any(|d| d.size >= 24.0);
    let central = group.iter().any(|d| {
        pages
            .get(&d.occurrence.page)
            .is_some_and(|p| centered(d.bbox, p.bbox))
    });
    let lexical = group
        .iter()
        .filter_map(|d| d.label.as_deref())
        .any(name_matches);
    let mut signals = Vec::new();
    if low_alpha {
        signals.push("low_alpha");
    }
    if rotated {
        signals.push("rotated");
    }
    if large {
        signals.push("large_text");
    }
    if central {
        signals.push("page_center");
    }
    if lexical {
        signals.push("watermark_word");
    }
    // Explicit PDF watermark markers and annotations are strong evidence. Repetition starts at
    // 0.45; transparency adds 0.20, diagonal rotation 0.15, central position 0.15, and a
    // watermark word 0.20. A large text run adds 0.05. The result is capped at 0.99;
    // opaque, unrotated content away from the page centre is capped at 0.60.
    let mut confidence = if matches!(first.kind, "ocg" | "artifact" | "annotation") {
        0.98
    } else {
        (0.45_f32
            + if low_alpha { 0.20 } else { 0.0 }
            + if rotated { 0.15 } else { 0.0 }
            + if central { 0.15 } else { 0.0 }
            + if lexical { 0.20 } else { 0.0 }
            + if large { 0.05 } else { 0.0 })
        .min(0.99)
    };
    if !matches!(first.kind, "ocg" | "artifact" | "annotation")
        && !low_alpha
        && !rotated
        && !central
    {
        confidence = confidence.min(0.6);
    }
    let bbox = group
        .iter()
        .fold(None, |a, d| Some(union(a, d.bbox)))
        .unwrap_or(first.bbox);
    if bbox.iter().any(|v| !v.is_finite()) {
        return Err(PdfError::Malformed);
    }
    let summary = first
        .label
        .as_ref()
        .map(|s| s.chars().take(80).collect())
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| first.kind.replace('_', " "));
    let ocg = if first.kind == "ocg" && first.key.len() == 6 {
        Some((
            u32::from_be_bytes(
                first.key[0..4]
                    .try_into()
                    .map_err(|_| PdfError::Malformed)?,
            ),
            u16::from_be_bytes(
                first.key[4..6]
                    .try_into()
                    .map_err(|_| PdfError::Malformed)?,
            ),
        ))
    } else {
        None
    };
    Ok(Candidate {
        ocg,
        id: candidate_id(input_hash, query, &occurrence_identity(first))?,
        kind: first.kind,
        origin: if first.kind == "ocg" && group.iter().all(|d| d.occurrence.own_stream) {
            "ariadshift"
        } else {
            "other"
        },
        summary,
        pages: page_numbers,
        bbox,
        rotate: pages.get(&first.occurrence.page).map_or(0, |p| p.rotate),
        preview: Preview {
            page: first.occurrence.page,
            bbox: first.bbox,
        },
        confidence,
        signals,
        occurrences: group.into_iter().map(|d| d.occurrence).collect(),
    })
}
fn repeated_groups(
    items: Vec<Drawable>,
    selected_count: usize,
    ratio: f32,
    ctx: &ToolCtx,
) -> Result<Vec<Vec<Drawable>>, PdfError> {
    type Clusters = BTreeMap<(Vec<u8>, [u8; 32]), Vec<Vec<Drawable>>>;
    let mut keyed: Clusters = BTreeMap::new();
    let mut comparisons = 0_usize;
    for item in items {
        let clusters = keyed.entry((item.key.clone(), item.context)).or_default();
        comparisons = comparisons.saturating_add(clusters.len());
        if comparisons > MAX_GROUP_COMPARISONS {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: MAX_GROUP_COMPARISONS,
            }));
        }
        if comparisons.is_multiple_of(1024) {
            ctx.check()?;
        }
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|cluster| cluster[0].ctm.similar(item.ctm))
        {
            cluster.push(item);
        } else {
            clusters.push(vec![item]);
        }
    }
    Ok(keyed
        .into_values()
        .flatten()
        .filter(|cluster| {
            let pages: HashSet<u32> = cluster.iter().map(|d| d.occurrence.page).collect();
            (pages.len() >= 2 && (pages.len() as f64) >= f64::from(ratio) * (selected_count as f64))
                || (cluster[0].kind == "repeated_text"
                    && cluster.iter().any(|d| d.alpha < 0.6 && d.size >= 24.0))
        })
        .collect())
}

/// Scans selected pages and returns deterministic candidates bound to the original input hash.
pub(super) fn scan(
    pdf: &PdfDoc,
    input_hash: &[u8; 32],
    input_len: usize,
    params: &WatermarkDetectParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Detection, PdfError> {
    params
        .validate()
        .map_err(|error| PdfError::InvalidParams(error.into()))?;
    let doc = pdf.inner();
    let page_ids = page_ids(doc, limits, ctx)?;
    let selected = crate::recolor::selected_pages(pdf, &params.pages, limits, ctx)?;
    let mut scanner = Scanner {
        pdf,
        doc,
        limits,
        ctx,
        budget: pdf.decode_budget(),
        decoder: PdfTextDecoder::new(doc, input_len, limits),
        active_forms: HashSet::new(),
        form_visits: 0,
        drawables: Vec::new(),
        image_cache: HashMap::new(),
        unsupported_images: 0,
    };
    let mut pages = BTreeMap::new();
    for number in &selected {
        ctx.check()?;
        let id = page_ids[(*number - 1) as usize];
        let crop = crate::xobject::resolve_page_crop_box(doc, id);
        let page_box = [crop.left, crop.bottom, crop.right, crop.top];
        let rotate = crate::xobject::resolve_page_rotation(doc, id);
        pages.insert(
            *number,
            Page {
                bbox: page_box,
                rotate,
            },
        );
        let resources = page_resources(doc, id, limits)?;
        let mut state = Graphics::default();
        let mut saved = Vec::new();
        let mut marked = Vec::new();
        let mut page_ops = 0;
        let streams = super::watermark_remove::contents(doc, id, limits.max_nesting_depth)?;
        if streams.len() > MAX_DRAWABLES {
            return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                limit: MAX_DRAWABLES,
            }));
        }
        for (slot, root) in streams.into_iter().enumerate() {
            let stream = doc
                .get_object(root)
                .and_then(Object::as_stream)
                .map_err(|_| PdfError::Malformed)?;
            let own = stream
                .dict
                .get(b"AriadShift")
                .and_then(Object::as_name)
                .ok()
                == Some(crate::draw::layer::MARKER_WATERMARK);
            let data = scanner.decode(stream)?;
            let content = Content::decode(&data).map_err(|_| PdfError::Malformed)?;
            scanner.content(
                *number,
                root,
                slot,
                &[],
                &content,
                &resources,
                &mut state,
                page_box,
                own,
                false,
                0,
                &mut page_ops,
                &mut saved,
                &mut marked,
            )?;
        }
        if !saved.is_empty() || !marked.is_empty() {
            return Err(PdfError::Malformed);
        }
        let page = doc.get_dictionary(id).map_err(|_| PdfError::Malformed)?;
        if let Some(annots) = page
            .get(b"Annots")
            .ok()
            .and_then(|o| resolve(doc, o, limits.max_nesting_depth))
            .and_then(|o| o.as_array().ok())
        {
            if annots.len() > MAX_DRAWABLES {
                return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                    limit: MAX_DRAWABLES,
                }));
            }
            for (index, annotation) in annots.iter().enumerate() {
                let Some(d) = dict(doc, annotation, limits.max_nesting_depth) else {
                    continue;
                };
                if d.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Watermark") {
                    continue;
                }
                let bbox = d
                    .get(b"Rect")
                    .ok()
                    .and_then(|v| rectangle(doc, v, limits.max_nesting_depth))
                    .unwrap_or(page_box);
                let key = annotation
                    .as_reference()
                    .map(object_id_bytes)
                    .unwrap_or_else(|_| (index as u64).to_be_bytes().to_vec());
                scanner.emit(Drawable {
                    kind: "annotation",
                    key,
                    context: [0; 32],
                    occurrence: Occurrence {
                        page: *number,
                        stream: id,
                        slot: 0,
                        path: Vec::new(),
                        start: index,
                        end: index + 1,
                        annotation: annotation.as_reference().ok(),
                        own_stream: false,
                        ambiguous: false,
                    },
                    bbox,
                    ctm: Matrix::IDENTITY,
                    alpha: 1.0,
                    size: 0.0,
                    label: None,
                })?;
            }
        }
    }
    let mut explicit: BTreeMap<(&'static str, Vec<u8>), Vec<Drawable>> = BTreeMap::new();
    let mut xobjects = Vec::new();
    let mut text = Vec::new();
    for drawable in scanner.drawables {
        match drawable.kind {
            "repeated_xobject" => xobjects.push(drawable),
            "repeated_text" => text.push(drawable),
            _ => explicit
                .entry((drawable.kind, drawable.key.clone()))
                .or_default()
                .push(drawable),
        }
    }
    let mut xobject_groups = repeated_groups(xobjects, selected.len(), params.min_page_ratio, ctx)?;
    let parent_paths: HashSet<_> = xobject_groups
        .iter()
        .flatten()
        .map(|parent| {
            (
                parent.occurrence.page,
                parent.occurrence.stream,
                parent.occurrence.slot,
                parent.occurrence.path.clone(),
                parent.occurrence.start,
            )
        })
        .collect();
    let is_nested = |item: &Drawable| {
        (0..item.occurrence.path.len()).any(|index| {
            parent_paths.contains(&(
                item.occurrence.page,
                item.occurrence.stream,
                item.occurrence.slot,
                item.occurrence.path[..index].to_vec(),
                item.occurrence.path[index],
            ))
        })
    };
    text.retain(|item| !is_nested(item));
    for group in &mut xobject_groups {
        group.retain(|item| !is_nested(item));
    }
    xobject_groups.retain(|group| !group.is_empty());
    for group in explicit.values_mut() {
        group.retain(|item| !is_nested(item));
    }
    explicit.retain(|_, group| !group.is_empty());
    let mut groups: Vec<Vec<Drawable>> = explicit.into_values().collect();
    groups.extend(xobject_groups);
    groups.extend(repeated_groups(
        text,
        selected.len(),
        params.min_page_ratio,
        ctx,
    )?);
    if groups.len() > MAX_CANDIDATES {
        return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
            limit: MAX_CANDIDATES,
        }));
    }
    scanner.budget.charge(
        groups.len().saturating_mul(
            params
                .pages
                .as_str()
                .len()
                .saturating_mul(2)
                .saturating_add(512),
        ),
    )?;
    let mut candidates = groups
        .into_iter()
        .map(|g| make_candidate(g, &pages, input_hash, params))
        .collect::<Result<Vec<_>, _>>()?;
    candidates.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut ids = HashSet::new();
    if candidates.iter().any(|c| !ids.insert(&c.id)) {
        return Err(PdfError::Malformed);
    }
    Ok(Detection {
        page_count: page_ids.len() as u32,
        candidates,
        unsupported_images: scanner.unsupported_images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_corpus_watermark_kinds() {
        for (name, kind) in [
            ("watermark-layer.pdf", "ocg"),
            ("watermark-artifact.pdf", "artifact"),
            ("watermark-repeated-xobject.pdf", "repeated_xobject"),
        ] {
            let bytes = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/pdf-tools")
                    .join(name),
            )
            .unwrap();
            let limits = Limits::local();
            let ctx = ToolCtx::new();
            let pdf = crate::document::open(&bytes, &limits).unwrap();
            let result = scan(
                &pdf,
                &sha256_chunks(&[&bytes]),
                bytes.len(),
                &WatermarkDetectParams::default(),
                &limits,
                &ctx,
            )
            .unwrap();
            let candidate = result
                .candidates
                .iter()
                .find(|c| c.kind == kind)
                .unwrap_or_else(|| panic!("{name}: {result:?}"));
            assert!(candidate.confidence >= 0.9, "{name}: {candidate:?}");
            assert_eq!(candidate.origin, "other");
            assert_eq!(candidate.pages, vec![1, 2, 3]);
        }
    }
}
