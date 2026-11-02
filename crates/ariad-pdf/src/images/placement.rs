//! Image XObject placement scanning.

use super::ImageError;
use crate::{PdfDoc, PdfError, ToolCtx, document::DecodeBudget, xobject::Matrix};
use ariad_core::{limits::Limits, reader::pdf::ReadError};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content};
use std::collections::{HashMap, HashSet};

/// The effective resolution range observed for an image and its draw count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageUse {
    pub max_dpi_x: f64,
    pub max_dpi_y: f64,
    /// Lowest horizontal resolution across all painted placements.
    pub min_dpi_x: f64,
    /// Lowest vertical resolution across all painted placements.
    pub min_dpi_y: f64,
    pub uses: u64,
}

impl Default for ImageUse {
    fn default() -> Self {
        Self {
            max_dpi_x: 0.0,
            max_dpi_y: 0.0,
            min_dpi_x: f64::INFINITY,
            min_dpi_y: f64::INFINITY,
            uses: 0,
        }
    }
}

struct Scanner<'a> {
    doc: &'a Document,
    ctx: &'a ToolCtx,
    budget: DecodeBudget,
    max_depth: u16,
    active: HashSet<ObjectId>,
    visits: usize,
    operations: u32,
    max_operations: u32,
    images: HashMap<ObjectId, ImageUse>,
}
const MAX_VISITS: usize = 100_000;

fn deref<'a>(doc: &'a Document, mut obj: &'a Object, limit: u16) -> Option<&'a Object> {
    for _ in 0..=limit {
        match obj {
            Object::Reference(id) => obj = doc.get_object(*id).ok()?,
            _ => return Some(obj),
        }
    }
    None
}
fn dict<'a>(doc: &'a Document, obj: &'a Object, limit: u16) -> Option<&'a Dictionary> {
    deref(doc, obj, limit)?.as_dict().ok()
}
fn real(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(n) => Some(*n as f64),
        Object::Real(n) => Some(f64::from(*n)),
        _ => None,
    }
}
fn matrix(obj: Option<&Object>) -> Option<Matrix> {
    let values = obj?.as_array().ok()?;
    if values.len() != 6 {
        return None;
    }
    let matrix = Matrix([
        real(&values[0])?,
        real(&values[1])?,
        real(&values[2])?,
        real(&values[3])?,
        real(&values[4])?,
        real(&values[5])?,
    ]);
    matrix
        .0
        .iter()
        .all(|value| value.is_finite())
        .then_some(matrix)
}
fn rect(obj: Option<&Object>) -> Option<[f64; 4]> {
    let a = obj?.as_array().ok()?;
    if a.len() != 4 {
        return None;
    }
    let rect = [real(&a[0])?, real(&a[1])?, real(&a[2])?, real(&a[3])?];
    rect.iter().all(|value| value.is_finite()).then_some(rect)
}
fn stream_matrix(doc: &Document, dict: &Dictionary, depth: u16) -> Result<Matrix, ImageError> {
    match dict.get(b"Matrix") {
        Ok(value) => matrix(deref(doc, value, depth)).ok_or(ImageError::Malformed("form matrix")),
        Err(_) => Ok(Matrix::identity()),
    }
}

impl Scanner<'_> {
    fn appearance(
        &mut self,
        id: ObjectId,
        annotation: &Dictionary,
        resources: &Dictionary,
        page_ctm: Matrix,
    ) -> Result<(), ImageError> {
        let stream = self
            .doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| ImageError::Malformed("appearance stream"))?;
        let bbox =
            rect(stream.dict.get(b"BBox").ok()).ok_or(ImageError::Malformed("appearance box"))?;
        let target = rect(annotation.get(b"Rect").ok())
            .ok_or(ImageError::Malformed("annotation rectangle"))?;
        let transform = stream_matrix(self.doc, &stream.dict, self.max_depth)?;
        let points = [
            transform.transform_point(bbox[0], bbox[1]),
            transform.transform_point(bbox[0], bbox[3]),
            transform.transform_point(bbox[2], bbox[1]),
            transform.transform_point(bbox[2], bbox[3]),
        ];
        let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        if max_x <= min_x || max_y <= min_y {
            return Err(ImageError::Malformed("empty appearance box"));
        }
        let sx = (target[2] - target[0]) / (max_x - min_x);
        let sy = (target[3] - target[1]) / (max_y - min_y);
        let map = Matrix::translate(-min_x, -min_y)
            .then(Matrix::scale(sx, sy))
            .then(Matrix::translate(target[0], target[1]))
            .then(page_ctm);
        self.enter(id, resources, map, 1)
    }
    fn stream(&mut self, stream: &Stream) -> Result<Vec<u8>, ImageError> {
        self.ctx.check()?;
        if stream.dict.has(b"Filter") {
            self.budget
                .decode(stream)?
                .ok_or(ImageError::Malformed("content stream"))
        } else {
            self.budget.charge(stream.content.len())?;
            Ok(stream.content.clone())
        }
    }
    fn enter(
        &mut self,
        id: ObjectId,
        resources: &Dictionary,
        ctm: Matrix,
        depth: u16,
    ) -> Result<(), ImageError> {
        if depth > self.max_depth {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: self.max_depth,
            })
            .into());
        }
        self.visits = self.visits.saturating_add(1);
        if self.visits > MAX_VISITS {
            return Err(
                PdfError::LimitExceeded(ReadError::TooManyObjects { limit: MAX_VISITS }).into(),
            );
        }
        if !self.active.insert(id) {
            return Err(ImageError::Malformed("recursive form"));
        }
        let result = (|| {
            let stream = self
                .doc
                .get_object(id)
                .and_then(Object::as_stream)
                .map_err(|_| ImageError::Malformed("form stream"))?;
            let resources = stream
                .dict
                .get(b"Resources")
                .ok()
                .and_then(|v| dict(self.doc, v, self.max_depth))
                .unwrap_or(resources);
            let ctm = stream_matrix(self.doc, &stream.dict, self.max_depth)?.then(ctm);
            if ctm.0.iter().any(|value| !value.is_finite()) {
                return Err(ImageError::Malformed("transformation matrix"));
            }
            let bytes = self.stream(stream)?;
            self.content(&bytes, resources, ctm, depth)
        })();
        self.active.remove(&id);
        result
    }
    fn content(
        &mut self,
        bytes: &[u8],
        resources: &Dictionary,
        mut ctm: Matrix,
        depth: u16,
    ) -> Result<(), ImageError> {
        self.ctx.check()?;
        let content =
            Content::decode(bytes).map_err(|_| ImageError::Malformed("content operations"))?;
        let mut stack = Vec::new();
        for op in content.operations {
            self.operations = self.operations.saturating_add(1);
            if self.operations > self.max_operations {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: self.max_operations,
                })
                .into());
            }
            if self.operations.is_multiple_of(128) {
                self.ctx.check()?;
            }
            match op.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => {
                    ctm = stack
                        .pop()
                        .ok_or(ImageError::Malformed("graphics state stack"))?;
                }
                "cm" => {
                    let m = matrix(Some(&Object::Array(op.operands)))
                        .ok_or(ImageError::Malformed("transformation matrix"))?;
                    ctm = m.then(ctm);
                    if ctm.0.iter().any(|value| !value.is_finite()) {
                        return Err(ImageError::Malformed("transformation matrix"));
                    }
                }
                "Do" => {
                    let name = op
                        .operands
                        .first()
                        .and_then(|v| v.as_name().ok())
                        .ok_or(ImageError::Malformed("XObject name"))?;
                    let obj = resources
                        .get(b"XObject")
                        .ok()
                        .and_then(|v| dict(self.doc, v, self.max_depth))
                        .and_then(|d| d.get(name).ok())
                        .ok_or(ImageError::Malformed("XObject resource"))?;
                    let id = obj
                        .as_reference()
                        .map_err(|_| ImageError::Unsupported("direct XObject"))?;
                    let stream = self
                        .doc
                        .get_object(id)
                        .and_then(Object::as_stream)
                        .map_err(|_| ImageError::Malformed("XObject stream"))?;
                    match stream
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .unwrap_or_default()
                    {
                        b"Image" => {
                            let (Ok(w), Ok(h)) = (
                                stream.dict.get(b"Width").and_then(Object::as_i64),
                                stream.dict.get(b"Height").and_then(Object::as_i64),
                            ) else {
                                return Err(ImageError::Malformed("image dimension"));
                            };
                            if !(1..=65535).contains(&w) || !(1..=65535).contains(&h) {
                                return Err(ImageError::Malformed("image dimension"));
                            }
                            let [a, b, c, d, _, _] = ctm.0;
                            let (sx, sy) = (a.hypot(b), c.hypot(d));
                            let entry = self.images.entry(id).or_default();
                            let dpi_x = if sx == 0.0 {
                                f64::INFINITY
                            } else {
                                w as f64 * 72.0 / sx
                            };
                            let dpi_y = if sy == 0.0 {
                                f64::INFINITY
                            } else {
                                h as f64 * 72.0 / sy
                            };
                            entry.max_dpi_x = entry.max_dpi_x.max(dpi_x);
                            entry.max_dpi_y = entry.max_dpi_y.max(dpi_y);
                            entry.min_dpi_x = entry.min_dpi_x.min(dpi_x);
                            entry.min_dpi_y = entry.min_dpi_y.min(dpi_y);
                            entry.uses = entry.uses.saturating_add(1);
                        }
                        b"Form" => self.enter(id, resources, ctm, depth.saturating_add(1))?,
                        _ => return Err(ImageError::Unsupported("XObject subtype")),
                    }
                }
                "scn" | "SCN" => {
                    let Some(name) = op.operands.last().and_then(|v| v.as_name().ok()) else {
                        continue;
                    };
                    let id = resources
                        .get(b"Pattern")
                        .ok()
                        .and_then(|v| dict(self.doc, v, self.max_depth))
                        .and_then(|d| d.get(name).ok())
                        .and_then(|v| v.as_reference().ok())
                        .ok_or(ImageError::Malformed("pattern resource"))?;
                    match self
                        .doc
                        .get_object(id)
                        .map_err(|_| ImageError::Malformed("pattern resource"))?
                    {
                        Object::Stream(stream)
                            if stream
                                .dict
                                .get(b"PatternType")
                                .and_then(Object::as_i64)
                                .ok()
                                == Some(1) =>
                        {
                            self.enter(id, resources, ctm, depth.saturating_add(1))?
                        }
                        Object::Dictionary(dict)
                            if dict.get(b"PatternType").and_then(Object::as_i64).ok()
                                == Some(2) => {}
                        _ => return Err(ImageError::Unsupported("pattern type")),
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Find image draws in page streams, forms, tiling patterns and annotation appearances.
pub fn placements(
    doc: &PdfDoc,
    ctx: &ToolCtx,
    limits: &Limits,
) -> Result<HashMap<ObjectId, ImageUse>, ImageError> {
    let mut scan = Scanner {
        doc: doc.inner(),
        ctx,
        budget: doc.decode_budget(),
        max_depth: limits.max_nesting_depth,
        active: HashSet::new(),
        visits: 0,
        operations: 0,
        max_operations: limits.max_blocks,
        images: HashMap::new(),
    };
    for page_id in crate::color::page_ids(doc.inner(), limits, ctx)? {
        ctx.check()?;
        let resources = crate::color::page_resources(doc.inner(), page_id, limits)?;
        let mut page = Vec::new();
        for id in doc.inner().get_page_contents(page_id) {
            let stream = doc
                .inner()
                .get_object(id)
                .and_then(Object::as_stream)
                .map_err(|_| ImageError::Malformed("page content"))?;
            page.extend_from_slice(&scan.stream(stream)?);
            page.push(b'\n');
        }
        let page_dict = doc
            .inner()
            .get_object(page_id)
            .and_then(Object::as_dict)
            .map_err(|_| ImageError::Malformed("page"))?;
        let user_unit = match page_dict.get(b"UserUnit") {
            Ok(value) => deref(doc.inner(), value, limits.max_nesting_depth)
                .and_then(real)
                .filter(|n| n.is_finite() && *n > 0.0)
                .ok_or(ImageError::Malformed("page user unit"))?,
            Err(_) => 1.0,
        };
        scan.content(&page, &resources, Matrix::scale(user_unit, user_unit), 0)?;
        if let Ok(annots) = page_dict.get(b"Annots") {
            let annots = deref(doc.inner(), annots, limits.max_nesting_depth)
                .and_then(|o| o.as_array().ok())
                .ok_or(ImageError::Malformed("annotations"))?;
            for annot in annots {
                let annotation = dict(doc.inner(), annot, limits.max_nesting_depth)
                    .ok_or(ImageError::Malformed("annotation"))?;
                let Ok(appearance) = annotation.get(b"AP") else {
                    continue;
                };
                let ap = dict(doc.inner(), appearance, limits.max_nesting_depth)
                    .ok_or(ImageError::Malformed("appearance dictionary"))?;
                for key in [b"N".as_slice(), b"R", b"D"] {
                    let Some(ap_obj) = ap.get(key).ok() else {
                        continue;
                    };
                    if let Ok(id) = ap_obj.as_reference()
                        && doc
                            .inner()
                            .get_object(id)
                            .is_ok_and(|o| matches!(o, Object::Stream(_)))
                    {
                        scan.appearance(
                            id,
                            annotation,
                            &resources,
                            Matrix::scale(user_unit, user_unit),
                        )?;
                        continue;
                    }
                    if let Some(states) = dict(doc.inner(), ap_obj, limits.max_nesting_depth) {
                        for (_, v) in states {
                            let id = v
                                .as_reference()
                                .map_err(|_| ImageError::Malformed("appearance state"))?;
                            scan.appearance(
                                id,
                                annotation,
                                &resources,
                                Matrix::scale(user_unit, user_unit),
                            )?;
                        }
                    } else {
                        return Err(ImageError::Malformed("appearance stream"));
                    }
                }
            }
        }
    }
    Ok(scan.images)
}
