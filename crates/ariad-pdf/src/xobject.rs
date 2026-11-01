//! Page-to-Form-XObject conversion, deep object copying, and 2D affine placement matrices
//! (`[a, b, c, d, e, f]`).
//!
//! Used by `pdf.scale`, `pdf.nup`, `pdf.booklet`, `pdf.overlay`, `pdf.divide_pages`,
//! `pdf.posterize`, `pdf.rotate_content`, and `pdf.single_page`.

use std::collections::{HashMap, HashSet, VecDeque};

use ariad_core::tools::params::scale::ScaleFit;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use crate::{
    document::{DecodeBudget, ToolCtx},
    error::PdfError,
};

/// Maximum `/Parent` chain depth traversed when resolving inherited `/Page` attributes.
const MAX_PAGE_TREE_DEPTH: usize = 64;

/// Maximum number of indirect `Object::Reference` hops followed when resolving scalar,
/// array, or dictionary values.
const MAX_REF_HOPS: usize = 8;

/// Maximum inline container nesting depth inside a single PDF object during deep copying.
const MAX_INLINE_COPY_DEPTH: usize = 64;

/// Default page box in PDF points (`US Letter`, `612 × 792 pt`).
pub const DEFAULT_MEDIA_BOX: PdfRect = PdfRect {
    left: 0.0,
    bottom: 0.0,
    right: 612.0,
    top: 792.0,
};

/// Axis-aligned rectangle `[left, bottom, right, top]` in PDF user-space points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfRect {
    /// Lower-left X coordinate (`llx`).
    pub left: f64,
    /// Lower-left Y coordinate (`lly`).
    pub bottom: f64,
    /// Upper-right X coordinate (`urx`).
    pub right: f64,
    /// Upper-right Y coordinate (`ury`).
    pub top: f64,
}

impl PdfRect {
    /// Constructs a normalized [`PdfRect`] (`left <= right`, `bottom <= top`).
    #[must_use]
    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        let (left, right) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        let (bottom, top) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        Self {
            left,
            bottom,
            right,
            top,
        }
    }

    /// Constructs a [`PdfRect`] starting at `(0, 0)` with `width` and `height`.
    #[must_use]
    pub const fn from_size(width: f64, height: f64) -> Self {
        Self::new(0.0, 0.0, width, height)
    }

    /// Constructs a normalized [`PdfRect`] from `[x0, y0, x1, y1]`.
    #[must_use]
    pub const fn from_array([x0, y0, x1, y1]: [f64; 4]) -> Self {
        Self::new(x0, y0, x1, y1)
    }

    /// Returns `[left, bottom, right, top]`.
    #[must_use]
    pub const fn to_array(self) -> [f64; 4] {
        [self.left, self.bottom, self.right, self.top]
    }

    /// Converts this rectangle into a 4-element `lopdf` array `[left, bottom, right, top]`.
    #[must_use]
    pub fn to_pdf_array(self) -> Vec<Object> {
        vec![
            Object::Real(self.left as f32),
            Object::Real(self.bottom as f32),
            Object::Real(self.right as f32),
            Object::Real(self.top as f32),
        ]
    }

    /// Returns the horizontal width (`right - left`, `>= 0.0`).
    #[must_use]
    pub const fn width(self) -> f64 {
        let w = self.right - self.left;
        if w > 0.0 { w } else { 0.0 }
    }

    /// Returns the vertical height (`top - bottom`, `>= 0.0`).
    #[must_use]
    pub const fn height(self) -> f64 {
        let h = self.top - self.bottom;
        if h > 0.0 { h } else { 0.0 }
    }

    /// Returns the intersection of `self` and `other`, or `self` if the intersection is empty.
    #[must_use]
    pub const fn intersect_or_self(self, other: Self) -> Self {
        let left = if self.left >= other.left {
            self.left
        } else {
            other.left
        };
        let bottom = if self.bottom >= other.bottom {
            self.bottom
        } else {
            other.bottom
        };
        let right = if self.right <= other.right {
            self.right
        } else {
            other.right
        };
        let top = if self.top <= other.top {
            self.top
        } else {
            other.top
        };
        if right > left && top > bottom {
            Self {
                left,
                bottom,
                right,
                top,
            }
        } else {
            self
        }
    }
}

/// 2D affine transformation matrix `[a, b, c, d, e, f]` for the PDF `cm` operator
/// (`x' = a*x + c*y + e`, `y' = b*x + d*y + f`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [f64; 6]);

impl Default for Matrix {
    fn default() -> Self {
        Self::identity()
    }
}

impl Matrix {
    /// Returns the identity matrix `[1, 0, 0, 1, 0, 0]`.
    #[must_use]
    pub const fn identity() -> Self {
        Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }

    /// Returns a translation matrix `[1, 0, 0, 1, tx, ty]`.
    #[must_use]
    pub const fn translate(tx: f64, ty: f64) -> Self {
        Self([1.0, 0.0, 0.0, 1.0, tx, ty])
    }

    /// Returns a scaling matrix `[sx, 0, 0, sy, 0, 0]`.
    #[must_use]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// Returns a counter-clockwise rotation matrix around `(0, 0)` by `angle_deg` degrees.
    ///
    /// Exact quarter-turn multiples (`0`, `90`, `180`, `270`, `-90`, etc.) use exact `0.0`, `1.0`,
    /// and `-1.0` coefficients to avoid floating-point trig rounding error.
    #[must_use]
    pub fn rotate_deg(angle_deg: f64) -> Self {
        let normalized = angle_deg.rem_euclid(360.0);
        let (sin, cos) = if (normalized - 0.0).abs() < 1e-9 || (normalized - 360.0).abs() < 1e-9 {
            (0.0, 1.0)
        } else if (normalized - 90.0).abs() < 1e-9 {
            (1.0, 0.0)
        } else if (normalized - 180.0).abs() < 1e-9 {
            (0.0, -1.0)
        } else if (normalized - 270.0).abs() < 1e-9 {
            (-1.0, 0.0)
        } else {
            normalized.to_radians().sin_cos()
        };
        Self([
            clean_zero(cos),
            clean_zero(sin),
            clean_zero(-sin),
            clean_zero(cos),
            0.0,
            0.0,
        ])
    }

    /// Composes `self` followed by `next` (`p' = (p * self) * next`).
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = next.0;
        Self([
            clean_zero(a1 * a2 + b1 * c2),
            clean_zero(a1 * b2 + b1 * d2),
            clean_zero(c1 * a2 + d1 * c2),
            clean_zero(c1 * b2 + d1 * d2),
            clean_zero(e1 * a2 + f1 * c2 + e2),
            clean_zero(e1 * b2 + f1 * d2 + f2),
        ])
    }

    /// Transforms a 2D point `(x, y)` through this matrix.
    #[must_use]
    pub fn transform_point(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (clean_zero(a * x + c * y + e), clean_zero(b * x + d * y + f))
    }

    /// Transforms all 4 corners of `rect` and returns the axis-aligned bounding [`PdfRect`].
    #[must_use]
    pub fn transform_rect_bbox(self, rect: PdfRect) -> PdfRect {
        let corners = [
            self.transform_point(rect.left, rect.bottom),
            self.transform_point(rect.right, rect.bottom),
            self.transform_point(rect.right, rect.top),
            self.transform_point(rect.left, rect.top),
        ];
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (x, y) in corners {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        PdfRect::new(min_x, min_y, max_x, max_y)
    }

    /// Converts the matrix coefficients into a 6-element `lopdf` array `[a, b, c, d, e, f]`.
    #[must_use]
    pub fn to_pdf_array(self) -> Vec<Object> {
        self.0.into_iter().map(|v| Object::Real(v as f32)).collect()
    }
}

fn clean_zero(val: f64) -> f64 {
    if val.abs() < 1e-12 { 0.0 } else { val }
}

/// Computes the 2D placement matrix that maps `source_box` (with clockwise page `/Rotate`
/// `rotation_deg` in `{0, 90, 180, 270}` and arbitrary `MediaBox` / `CropBox` offsets) into
/// `target_rect` according to `fit`.
#[must_use]
pub fn placement_matrix(
    source_box: PdfRect,
    rotation_deg: i32,
    target_rect: PdfRect,
    fit: ScaleFit,
) -> Matrix {
    let w = source_box.width().max(1e-6);
    let h = source_box.height().max(1e-6);
    let rot = rotation_deg.rem_euclid(360);

    // 1. Shift `source_box` origin `(left, bottom)` to `(0, 0)`.
    let to_origin = Matrix::translate(-source_box.left, -source_box.bottom);

    // 2. Apply the page's clockwise `/Rotate` attribute so `[0, 0, w, h]` becomes upright
    //    `[0, 0, visual_w, visual_h]`.
    let (upright, visual_w, visual_h) = match rot {
        90 => (
            Matrix::rotate_deg(-90.0).then(Matrix::translate(0.0, w)),
            h,
            w,
        ),
        180 => (
            Matrix::rotate_deg(180.0).then(Matrix::translate(w, h)),
            w,
            h,
        ),
        270 => (
            Matrix::rotate_deg(90.0).then(Matrix::translate(h, 0.0)),
            h,
            w,
        ),
        _ => (Matrix::identity(), w, h),
    };

    // 3. Scale and center `[0, 0, visual_w, visual_h]` inside `target_rect`.
    let target_w = target_rect.width().max(1e-6);
    let target_h = target_rect.height().max(1e-6);

    let (sx, sy) = match fit {
        ScaleFit::Contain => {
            let s = (target_w / visual_w).min(target_h / visual_h);
            (s, s)
        }
        ScaleFit::Cover => {
            let s = (target_w / visual_w).max(target_h / visual_h);
            (s, s)
        }
        ScaleFit::Stretch => (target_w / visual_w, target_h / visual_h),
    };

    let placed_w = visual_w * sx;
    let placed_h = visual_h * sy;
    let tx = target_rect.left + (target_w - placed_w) * 0.5;
    let ty = target_rect.bottom + (target_h - placed_h) * 0.5;

    to_origin
        .then(upright)
        .then(Matrix::scale(sx, sy))
        .then(Matrix::translate(tx, ty))
}

/// Computes the matrix and resulting page box for rotating page content by an arbitrary clockwise
/// angle `angle_deg` (`pdf.rotate_content`).
///
/// When `expand == true`, the returned [`PdfRect`] starts at `(0, 0)` and is enlarged to fit all
/// four rotated corners without clipping.
#[must_use]
pub fn content_rotation_matrix(
    source_box: PdfRect,
    angle_deg: f64,
    expand: bool,
) -> (Matrix, PdfRect) {
    let cx = (source_box.left + source_box.right) * 0.5;
    let cy = (source_box.bottom + source_box.top) * 0.5;

    // Clockwise rotation by `angle_deg` corresponds to counter-clockwise `-angle_deg` in PDF user space.
    let centered_rot = Matrix::translate(-cx, -cy).then(Matrix::rotate_deg(-angle_deg));
    if expand {
        let rotated_bbox = centered_rot.transform_rect_bbox(source_box);
        let new_w = rotated_bbox.width();
        let new_h = rotated_bbox.height();
        let shift = Matrix::translate(new_w * 0.5, new_h * 0.5);
        (centered_rot.then(shift), PdfRect::from_size(new_w, new_h))
    } else {
        let restore = Matrix::translate(cx, cy);
        (centered_rot.then(restore), source_box)
    }
}

/// Renders a PDF content stream snippet (`q a b c d e f cm /<name> Do Q\n`) that places a Form
/// XObject with `matrix`.
#[must_use]
pub fn render_place_xobject(xobject_name: &[u8], matrix: Matrix) -> Vec<u8> {
    let [a, b, c, d, e, f] = matrix.0;
    let name_str = String::from_utf8_lossy(xobject_name);
    let clean_name = name_str.trim_start_matches('/');
    format!(
        "q {} {} {} {} {} {} cm /{clean_name} Do Q\n",
        fmt_num(a),
        fmt_num(b),
        fmt_num(c),
        fmt_num(d),
        fmt_num(e),
        fmt_num(f),
    )
    .into_bytes()
}

fn fmt_num(v: f64) -> String {
    let clean = clean_zero(v);
    if (clean - clean.round()).abs() < 1e-9 {
        format!("{}", clean.round() as i64)
    } else {
        let s = format!("{clean:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// Resolves an inherited box attribute (`b"MediaBox"`, `b"CropBox"`, etc.) by walking up the
/// `/Parent` chain of `page_id`.
#[must_use]
pub fn resolve_page_box(doc: &Document, page_id: ObjectId, key: &[u8]) -> Option<PdfRect> {
    let mut current = Some(page_id);
    let mut visited = HashSet::new();
    for _ in 0..MAX_PAGE_TREE_DEPTH {
        let id = current?;
        if !visited.insert(id) {
            break;
        }
        let dict = doc.objects.get(&id)?.as_dict().ok()?;
        if let Ok(obj) = dict.get(key)
            && let Some(rect) = parse_rect_object(doc, obj)
        {
            return Some(rect);
        }
        current = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    None
}

/// Resolves the effective `/MediaBox` of `page_id` (falling back to [`DEFAULT_MEDIA_BOX`]).
#[must_use]
pub fn resolve_page_media_box(doc: &Document, page_id: ObjectId) -> PdfRect {
    resolve_page_box(doc, page_id, b"MediaBox").unwrap_or(DEFAULT_MEDIA_BOX)
}

/// Resolves the effective visible box of `page_id` (`/CropBox` intersected with `/MediaBox` when
/// `/CropBox` is present, otherwise `/MediaBox`).
#[must_use]
pub fn resolve_page_crop_box(doc: &Document, page_id: ObjectId) -> PdfRect {
    let media = resolve_page_media_box(doc, page_id);
    match resolve_page_box(doc, page_id, b"CropBox") {
        Some(crop) => crop.intersect_or_self(media),
        None => media,
    }
}

/// Resolves the effective clockwise `/Rotate` angle (`0`, `90`, `180`, or `270`) of `page_id`,
/// including values inherited from ancestor `/Pages` nodes.
#[must_use]
pub fn resolve_page_rotation(doc: &Document, page_id: ObjectId) -> i32 {
    let mut current = Some(page_id);
    let mut visited = HashSet::new();
    for _ in 0..MAX_PAGE_TREE_DEPTH {
        let Some(id) = current else {
            break;
        };
        if !visited.insert(id) {
            break;
        }
        let Some(obj) = doc.objects.get(&id) else {
            break;
        };
        let Ok(dict) = obj.as_dict() else {
            break;
        };
        if let Ok(rot_obj) = dict.get(b"Rotate")
            && let Some(angle) = resolve_integer(doc, rot_obj)
        {
            let norm = (angle as i32).rem_euclid(360);
            if matches!(norm, 0 | 90 | 180 | 270) {
                return norm;
            }
        }
        current = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    0
}

/// Resolves the merged `/Resources` dictionary for `page_id`, inheriting entries from ancestor
/// `/Pages` nodes when not overridden on the `/Page` itself.
#[must_use]
pub fn resolve_page_resources(doc: &Document, page_id: ObjectId) -> Dictionary {
    let mut chain = Vec::new();
    let mut current = Some(page_id);
    let mut visited = HashSet::new();

    for _ in 0..MAX_PAGE_TREE_DEPTH {
        let Some(id) = current else {
            break;
        };
        if !visited.insert(id) {
            break;
        }
        let Some(Ok(dict)) = doc.objects.get(&id).map(Object::as_dict) else {
            break;
        };
        if let Ok(res_obj) = dict.get(b"Resources")
            && let Some(res_dict) = deref_dict(doc, res_obj)
        {
            chain.push(res_dict.clone());
        }
        current = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }

    // Merge from ancestor down to the leaf `/Page` so leaf entries win.
    let mut merged = Dictionary::new();
    for res in chain.into_iter().rev() {
        for (key, value) in res.iter() {
            if let (Ok(existing), Some(incoming_sub)) = (merged.get(key), deref_dict(doc, value))
                && let Some(existing_sub) = deref_dict(doc, existing)
            {
                let mut combined = existing_sub.clone();
                for (sub_k, sub_v) in incoming_sub.iter() {
                    combined.set(sub_k.clone(), sub_v.clone());
                }
                merged.set(key.clone(), Object::Dictionary(combined));
                continue;
            }
            merged.set(key.clone(), value.clone());
        }
    }
    merged
}

/// Memoizing deep-cloner that copies indirect objects from a source [`Document`] into a target
/// [`Document`] while preserving object graph sharing and cyclic references.
#[derive(Clone, Debug, Default)]
pub struct ObjectCopier {
    mapped: HashMap<ObjectId, ObjectId>,
    pending: VecDeque<(ObjectId, ObjectId)>,
}

impl ObjectCopier {
    /// Creates a new empty [`ObjectCopier`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Deep-copies `object` (and all indirect objects it references in `source_doc`) into
    /// `target_doc`.
    pub fn copy_object(
        &mut self,
        source_doc: &Document,
        target_doc: &mut Document,
        object: &Object,
        ctx: &ToolCtx,
    ) -> Result<Object, PdfError> {
        self.copy_object_with_budget(source_doc, target_doc, object, None, ctx)
    }

    /// Deep-copies a [`Dictionary`] from `source_doc` into `target_doc`.
    pub fn copy_dictionary(
        &mut self,
        source_doc: &Document,
        target_doc: &mut Document,
        dict: &Dictionary,
        ctx: &ToolCtx,
    ) -> Result<Dictionary, PdfError> {
        self.copy_dictionary_with_budget(source_doc, target_doc, dict, None, ctx)
    }

    /// Deep-copies a [`Dictionary`] from `source_doc` into `target_doc`, charging any copied
    /// [`Stream`] objects against `budget` when provided.
    pub(crate) fn copy_dictionary_with_budget(
        &mut self,
        source_doc: &Document,
        target_doc: &mut Document,
        dict: &Dictionary,
        mut budget: Option<&mut DecodeBudget>,
        ctx: &ToolCtx,
    ) -> Result<Dictionary, PdfError> {
        let copied =
            self.copy_dictionary_inline(target_doc, dict, budget.as_deref_mut(), 0, ctx)?;
        self.flush_pending(source_doc, target_doc, budget, ctx)?;
        Ok(copied)
    }

    /// Copies the indirect object `src_id` from `source_doc` into `target_doc`, returning its new
    /// [`ObjectId`] in `target_doc`.
    pub fn copy_id(
        &mut self,
        source_doc: &Document,
        target_doc: &mut Document,
        src_id: ObjectId,
        ctx: &ToolCtx,
    ) -> Result<ObjectId, PdfError> {
        let dst_id = self.enqueue_id(target_doc, src_id, ctx)?;
        self.flush_pending(source_doc, target_doc, None, ctx)?;
        Ok(dst_id)
    }

    fn copy_object_with_budget(
        &mut self,
        source_doc: &Document,
        target_doc: &mut Document,
        object: &Object,
        mut budget: Option<&mut DecodeBudget>,
        ctx: &ToolCtx,
    ) -> Result<Object, PdfError> {
        let copied = self.copy_object_inline(target_doc, object, budget.as_deref_mut(), 0, ctx)?;
        self.flush_pending(source_doc, target_doc, budget, ctx)?;
        Ok(copied)
    }

    fn enqueue_id(
        &mut self,
        target_doc: &mut Document,
        src_id: ObjectId,
        ctx: &ToolCtx,
    ) -> Result<ObjectId, PdfError> {
        if let Some(&existing) = self.mapped.get(&src_id) {
            return Ok(existing);
        }
        ctx.check()?;
        let dst_id = target_doc.new_object_id();
        self.mapped.insert(src_id, dst_id);
        target_doc.objects.insert(dst_id, Object::Null);
        self.pending.push_back((src_id, dst_id));
        Ok(dst_id)
    }

    fn flush_pending(
        &mut self,
        source_doc: &Document,
        target_doc: &mut Document,
        mut budget: Option<&mut DecodeBudget>,
        ctx: &ToolCtx,
    ) -> Result<(), PdfError> {
        while let Some((src_id, dst_id)) = self.pending.pop_front() {
            ctx.check()?;
            if let Some(src_obj) = source_doc.objects.get(&src_id) {
                let copied =
                    self.copy_object_inline(target_doc, src_obj, budget.as_deref_mut(), 0, ctx)?;
                target_doc.objects.insert(dst_id, copied);
            } else {
                target_doc.objects.insert(dst_id, Object::Null);
            }
        }
        Ok(())
    }

    fn copy_object_inline(
        &mut self,
        target_doc: &mut Document,
        object: &Object,
        mut budget: Option<&mut DecodeBudget>,
        depth: usize,
        ctx: &ToolCtx,
    ) -> Result<Object, PdfError> {
        ctx.check()?;
        if depth > MAX_INLINE_COPY_DEPTH {
            return Err(PdfError::Malformed);
        }
        match object {
            Object::Reference(src_id) => {
                let dst_id = self.enqueue_id(target_doc, *src_id, ctx)?;
                Ok(Object::Reference(dst_id))
            }
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.copy_object_inline(
                        target_doc,
                        item,
                        budget.as_deref_mut(),
                        depth + 1,
                        ctx,
                    )?);
                }
                Ok(Object::Array(out))
            }
            Object::Dictionary(dict) => {
                let copied = self.copy_dictionary_inline(
                    target_doc,
                    dict,
                    budget.as_deref_mut(),
                    depth + 1,
                    ctx,
                )?;
                Ok(Object::Dictionary(copied))
            }
            Object::Stream(stream) => {
                if let Some(b) = budget.as_deref_mut()
                    && b.decode(stream)?.is_none()
                {
                    b.charge(stream.content.len())?;
                }
                let dict =
                    self.copy_dictionary_inline(target_doc, &stream.dict, budget, depth + 1, ctx)?;
                Ok(Object::Stream(Stream::new(dict, stream.content.clone())))
            }
            other => Ok(other.clone()),
        }
    }

    fn copy_dictionary_inline(
        &mut self,
        target_doc: &mut Document,
        dict: &Dictionary,
        mut budget: Option<&mut DecodeBudget>,
        depth: usize,
        ctx: &ToolCtx,
    ) -> Result<Dictionary, PdfError> {
        if depth > MAX_INLINE_COPY_DEPTH {
            return Err(PdfError::Malformed);
        }
        let mut out = Dictionary::new();
        for (k, v) in dict.iter() {
            if k == b"Parent" {
                continue;
            }
            out.set(
                k.clone(),
                self.copy_object_inline(target_doc, v, budget.as_deref_mut(), depth + 1, ctx)?,
            );
        }
        Ok(out)
    }
}

/// Metadata and object ID of a Form XObject created from a source PDF page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormXObject {
    /// Object ID of the `/Subtype /Form` stream in the target [`Document`].
    pub id: ObjectId,
    /// Form `/BBox` (equal to the resolved visible `crop_box` of the source page).
    pub bbox: PdfRect,
    /// Resolved clockwise `/Rotate` angle (`0`, `90`, `180`, or `270`) of the source page.
    pub rotation: i32,
    /// Resolved `/MediaBox` of the source page.
    pub media_box: PdfRect,
    /// Resolved visible `/CropBox` of the source page.
    pub crop_box: PdfRect,
}

/// Converts `page_id` from `source_doc` (resolving inherited `/Resources`, `/Rotate`, `/MediaBox`,
/// and `/CropBox`) into a `/Subtype /Form` XObject inside `target_doc`, charging decoded content
/// and copied resource streams against `budget`.
#[allow(private_interfaces)]
pub fn page_to_form_xobject(
    source_doc: &Document,
    page_id: ObjectId,
    target_doc: &mut Document,
    copier: &mut ObjectCopier,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<FormXObject, PdfError> {
    ctx.check()?;
    let media_box = resolve_page_media_box(source_doc, page_id);
    let crop_box = resolve_page_crop_box(source_doc, page_id);
    let rotation = resolve_page_rotation(source_doc, page_id);
    let resolved_resources = resolve_page_resources(source_doc, page_id);
    let copied_resources = copier.copy_dictionary_with_budget(
        source_doc,
        target_doc,
        &resolved_resources,
        Some(budget),
        ctx,
    )?;

    let content_bytes = collect_page_content_bytes(source_doc, page_id, budget, ctx)?;
    let form_dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "FormType" => 1,
        "BBox" => crop_box.to_pdf_array(),
        "Resources" => Object::Dictionary(copied_resources),
    };
    let mut stream = Stream::new(form_dict, content_bytes);
    let _ = stream.compress();
    let id = target_doc.add_object(Object::Stream(stream));

    Ok(FormXObject {
        id,
        bbox: crop_box,
        rotation,
        media_box,
        crop_box,
    })
}

/// Converts `page_id` into a `/Subtype /Form` XObject within the same [`Document`], decoding each
/// content stream once through `budget`.
#[allow(private_interfaces)]
pub fn page_to_form_xobject_in_place(
    doc: &mut Document,
    page_id: ObjectId,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<FormXObject, PdfError> {
    ctx.check()?;
    let media_box = resolve_page_media_box(doc, page_id);
    let crop_box = resolve_page_crop_box(doc, page_id);
    let rotation = resolve_page_rotation(doc, page_id);
    let resolved_resources = resolve_page_resources(doc, page_id);
    let content_bytes = collect_page_content_bytes(doc, page_id, budget, ctx)?;

    let form_dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "FormType" => 1,
        "BBox" => crop_box.to_pdf_array(),
        "Resources" => Object::Dictionary(resolved_resources),
    };
    let mut stream = Stream::new(form_dict, content_bytes);
    let _ = stream.compress();
    let id = doc.add_object(Object::Stream(stream));

    Ok(FormXObject {
        id,
        bbox: crop_box,
        rotation,
        media_box,
        crop_box,
    })
}

fn collect_page_content_bytes(
    doc: &Document,
    page_id: ObjectId,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    ctx.check()?;
    let Some(Ok(page_dict)) = doc.objects.get(&page_id).map(Object::as_dict) else {
        return Err(PdfError::Malformed);
    };
    let Ok(contents_obj) = page_dict.get(b"Contents") else {
        return Ok(Vec::new());
    };

    let mut combined = Vec::new();
    match resolve_object_bounded(doc, contents_obj) {
        Some(Object::Stream(stream)) => {
            append_decoded_stream(&mut combined, stream, budget)?;
        }
        Some(Object::Array(arr)) => {
            for item in arr {
                ctx.check()?;
                if let Some(Object::Stream(stream)) = resolve_object_bounded(doc, item) {
                    append_decoded_stream(&mut combined, stream, budget)?;
                }
            }
        }
        _ => {}
    }
    Ok(combined)
}

fn append_decoded_stream(
    combined: &mut Vec<u8>,
    stream: &Stream,
    budget: &mut DecodeBudget,
) -> Result<(), PdfError> {
    let Some(decoded) = budget.decode(stream)? else {
        return Err(PdfError::Malformed);
    };
    if !combined.is_empty() && !combined.ends_with(b"\n") {
        combined.push(b'\n');
    }
    combined.extend_from_slice(&decoded);
    Ok(())
}

fn resolve_object_bounded<'a>(doc: &'a Document, mut obj: &'a Object) -> Option<&'a Object> {
    for _ in 0..MAX_REF_HOPS {
        match obj {
            Object::Reference(id) => {
                obj = doc.objects.get(id)?;
            }
            other => return Some(other),
        }
    }
    match obj {
        Object::Reference(_) => None,
        other => Some(other),
    }
}

fn parse_rect_object(doc: &Document, obj: &Object) -> Option<PdfRect> {
    let arr = resolve_object_bounded(doc, obj)?.as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }
    let x0 = resolve_number(doc, &arr[0])?;
    let y0 = resolve_number(doc, &arr[1])?;
    let x1 = resolve_number(doc, &arr[2])?;
    let y1 = resolve_number(doc, &arr[3])?;
    let rect = PdfRect::new(x0, y0, x1, y1);
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    Some(rect)
}

fn resolve_number(doc: &Document, obj: &Object) -> Option<f64> {
    match resolve_object_bounded(doc, obj)? {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

fn resolve_integer(doc: &Document, obj: &Object) -> Option<i64> {
    match resolve_object_bounded(doc, obj)? {
        Object::Integer(i) => Some(*i),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}

fn deref_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    resolve_object_bounded(doc, obj)?.as_dict().ok()
}

#[cfg(test)]
mod tests {
    use ariad_core::limits::Limits;

    use super::*;

    fn test_budget(limit: u64) -> DecodeBudget {
        DecodeBudget::from_limits(
            1024,
            &Limits {
                max_decompressed_bytes: limit,
                ..Limits::local()
            },
        )
    }

    fn assert_rect_approx_eq(actual: PdfRect, expected: PdfRect) {
        assert!(
            (actual.left - expected.left).abs() < 1e-4
                && (actual.bottom - expected.bottom).abs() < 1e-4
                && (actual.right - expected.right).abs() < 1e-4
                && (actual.top - expected.top).abs() < 1e-4,
            "rect mismatch: actual={actual:?}, expected={expected:?}"
        );
    }

    #[test]
    fn placement_matrices_handle_rotations_0_90_180_270_with_media_and_crop_offsets() {
        // Non-zero offset CropBox: origin at (36, 50), width = 200, height = 400.
        let crop_box = PdfRect::new(36.0, 50.0, 236.0, 450.0);
        let target = PdfRect::new(10.0, 20.0, 410.0, 820.0); // width = 400, height = 800

        // 1. Stretch fit maps `crop_box` onto exact `target` for every quarter-turn rotation.
        for rot in [0, 90, 180, 270] {
            let m = placement_matrix(crop_box, rot, target, ScaleFit::Stretch);
            let mapped = m.transform_rect_bbox(crop_box);
            assert_rect_approx_eq(mapped, target);
        }

        // 2. Specific corner verification under each rotation:
        // Rotation 0: bottom-left (36, 50) -> target bottom-left (10, 20); top-right (236, 450) -> (410, 820)
        let m0 = placement_matrix(crop_box, 0, target, ScaleFit::Contain);
        let (x0, y0) = m0.transform_point(36.0, 50.0);
        let (x1, y1) = m0.transform_point(236.0, 450.0);
        assert!((x0 - 10.0).abs() < 1e-4 && (y0 - 20.0).abs() < 1e-4);
        assert!((x1 - 410.0).abs() < 1e-4 && (y1 - 820.0).abs() < 1e-4);

        // Rotation 90 (clockwise): visual size is 400 x 200 (2:1 landscape).
        // Placed into `target` (400 x 800) with `Contain`: scale = min(400/400, 800/200) = 1.0,
        // centered vertically at y in [320, 520], x in [10, 410].
        // Source top-left (36, 450) rotates 90 deg CW to top-right (410, 520).
        // Source bottom-right (236, 50) rotates 90 deg CW to bottom-left (10, 320).
        let m90 = placement_matrix(crop_box, 90, target, ScaleFit::Contain);
        assert_rect_approx_eq(
            m90.transform_rect_bbox(crop_box),
            PdfRect::new(10.0, 320.0, 410.0, 520.0),
        );
        let (tl_x, tl_y) = m90.transform_point(36.0, 450.0);
        assert!((tl_x - 410.0).abs() < 1e-4 && (tl_y - 520.0).abs() < 1e-4);
        let (br_x, br_y) = m90.transform_point(236.0, 50.0);
        assert!((br_x - 10.0).abs() < 1e-4 && (br_y - 320.0).abs() < 1e-4);

        // Rotation 180: source bottom-left (36, 50) maps to target top-right (410, 820).
        let m180 = placement_matrix(crop_box, 180, target, ScaleFit::Contain);
        let (bl_x, bl_y) = m180.transform_point(36.0, 50.0);
        assert!((bl_x - 410.0).abs() < 1e-4 && (bl_y - 820.0).abs() < 1e-4);

        // Rotation 270 (90 CCW): source top-left (36, 450) maps to bottom-left (10, 320).
        let m270 = placement_matrix(crop_box, 270, target, ScaleFit::Contain);
        assert_rect_approx_eq(
            m270.transform_rect_bbox(crop_box),
            PdfRect::new(10.0, 320.0, 410.0, 520.0),
        );
        let (tl270_x, tl270_y) = m270.transform_point(36.0, 450.0);
        assert!((tl270_x - 10.0).abs() < 1e-4 && (tl270_y - 320.0).abs() < 1e-4);
    }

    #[test]
    fn page_to_form_xobject_inherits_parent_resources_rotation_and_crop_box() {
        let mut src_doc = Document::with_version("1.5");
        let pages_id = src_doc.new_object_id();
        let font_id = src_doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let parent_resources = src_doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id = src_doc.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 72 720 Td (Inherited) Tj ET".to_vec(),
        ));
        let page_id = src_doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "CropBox" => vec![10.into(), 20.into(), 310.into(), 420.into()],
            "Contents" => content_id,
        });
        src_doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => parent_resources,
                "Rotate" => 90,
            }),
        );

        let mut dst_doc = Document::with_version("1.5");
        let mut copier = ObjectCopier::new();
        let mut budget = test_budget(1024 * 1024);
        let form = page_to_form_xobject(
            &src_doc,
            page_id,
            &mut dst_doc,
            &mut copier,
            &mut budget,
            &ToolCtx::default(),
        )
        .expect("convert page to form xobject");

        assert_eq!(form.rotation, 90);
        assert_rect_approx_eq(form.media_box, PdfRect::new(0.0, 0.0, 612.0, 792.0));
        assert_rect_approx_eq(form.crop_box, PdfRect::new(10.0, 20.0, 310.0, 420.0));
        assert_rect_approx_eq(form.bbox, PdfRect::new(10.0, 20.0, 310.0, 420.0));

        let form_stream = dst_doc
            .get_object(form.id)
            .unwrap()
            .as_stream()
            .expect("form object is stream");
        assert!(form_stream.dict.has_type(b"XObject"));
        assert_eq!(
            form_stream.dict.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Form"
        );
        let res = form_stream
            .dict
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap();
        assert!(
            res.get(b"Font").is_ok(),
            "inherited Font resource must be copied"
        );
    }

    #[test]
    fn cyclic_and_deep_indirect_references_in_boxes_and_rotation_terminate_safely() {
        let mut doc = Document::with_version("1.5");

        // Self-referential cycle `10 0 obj 10 0 R` inside MediaBox, CropBox, and Rotate.
        let cycle_id = doc.new_object_id();
        doc.objects.insert(cycle_id, Object::Reference(cycle_id));

        let cyclic_page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![
                0.into(),
                0.into(),
                Object::Reference(cycle_id),
                792.into(),
            ],
            "CropBox" => Object::Reference(cycle_id),
            "Rotate" => Object::Reference(cycle_id),
        });

        assert_rect_approx_eq(
            resolve_page_media_box(&doc, cyclic_page_id),
            DEFAULT_MEDIA_BOX,
        );
        assert_rect_approx_eq(
            resolve_page_crop_box(&doc, cyclic_page_id),
            DEFAULT_MEDIA_BOX,
        );
        assert_eq!(resolve_page_rotation(&doc, cyclic_page_id), 0);

        // 8-hop chain succeeds (`MAX_REF_HOPS == 8`), while a 9-hop chain falls back safely.
        let mut hop8_id = doc.add_object(Object::Integer(500));
        for _ in 0..7 {
            hop8_id = doc.add_object(Object::Reference(hop8_id));
        }
        let hop9_id = doc.add_object(Object::Reference(hop8_id));

        let mut rot8_id = doc.add_object(Object::Integer(180));
        for _ in 0..7 {
            rot8_id = doc.add_object(Object::Reference(rot8_id));
        }
        let rot9_id = doc.add_object(Object::Reference(rot8_id));

        let page_8_hops = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), Object::Reference(hop8_id), 700.into()],
            "Rotate" => Object::Reference(rot8_id),
        });
        assert_rect_approx_eq(
            resolve_page_media_box(&doc, page_8_hops),
            PdfRect::new(0.0, 0.0, 500.0, 700.0),
        );
        assert_eq!(resolve_page_rotation(&doc, page_8_hops), 180);

        let page_9_hops = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), Object::Reference(hop9_id), 700.into()],
            "Rotate" => Object::Reference(rot9_id),
        });
        assert_rect_approx_eq(resolve_page_media_box(&doc, page_9_hops), DEFAULT_MEDIA_BOX);
        assert_eq!(resolve_page_rotation(&doc, page_9_hops), 0);
    }

    #[test]
    fn object_copier_handles_deep_chains_cycles_and_charges_resource_stream_budget() {
        let mut src_doc = Document::with_version("1.5");
        let chain_len = 5_000_usize;
        let first_id = src_doc.new_object_id();
        let mut prev_id = first_id;
        for idx in 1..chain_len {
            let next_id = src_doc.new_object_id();
            src_doc.objects.insert(
                prev_id,
                Object::Dictionary(dictionary! {
                    "Step" => idx as i64,
                    "Next" => Object::Reference(next_id),
                }),
            );
            prev_id = next_id;
        }
        // Close the 5,000-node chain into a cycle back to `first_id`.
        src_doc.objects.insert(
            prev_id,
            Object::Dictionary(dictionary! {
                "Step" => chain_len as i64,
                "Next" => Object::Reference(first_id),
            }),
        );

        let mut dst_doc = Document::with_version("1.5");
        let mut copier = ObjectCopier::new();
        let copied_first = copier
            .copy_id(&src_doc, &mut dst_doc, first_id, &ToolCtx::default())
            .expect("copy 5000-node cyclic indirect chain iteratively without stack overflow");
        assert_eq!(dst_doc.objects.len(), chain_len);

        // Verify the back-edge points back to `copied_first`.
        let copied_last = *copier.mapped.get(&prev_id).expect("last node mapped");
        let last_dict = dst_doc.get_dictionary(copied_last).expect("last dict");
        assert_eq!(
            last_dict.get(b"Next").unwrap().as_reference().unwrap(),
            copied_first
        );

        // Verify resource streams copied via `page_to_form_xobject` charge `DecodeBudget`.
        let mut res_doc = Document::with_version("1.5");
        let mut big_font_stream = Stream::new(dictionary! {}, vec![b'A'; 512]);
        let _ = big_font_stream.compress();
        let font_stream_id = res_doc.add_object(Object::Stream(big_font_stream));
        let page_id = res_doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! {
                    "F1" => Object::Reference(font_stream_id),
                },
            },
        });

        let mut target_doc = Document::with_version("1.5");
        let mut tiny_budget = test_budget(100);
        let err = page_to_form_xobject(
            &res_doc,
            page_id,
            &mut target_doc,
            &mut ObjectCopier::new(),
            &mut tiny_budget,
            &ToolCtx::default(),
        )
        .unwrap_err();
        assert!(
            matches!(err, PdfError::LimitExceeded(_)),
            "expected LimitExceeded when copied resource stream exceeds DecodeBudget, got {err:?}"
        );
    }

    #[test]
    fn page_to_form_xobject_decodes_each_stream_once_handles_inline_streams_and_rejects_corrupt_filters()
     {
        let payload = b"BT /F1 12 Tf 72 720 Td (Single decode budget check!) Tj ET\n".to_vec();
        assert_eq!(payload.len(), 59);

        let mut doc = Document::with_version("1.5");
        let mut compressed = Stream::new(dictionary! {}, payload.clone());
        let _ = compressed.compress();
        let content_id = doc.add_object(Object::Stream(compressed));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => Object::Reference(content_id),
        });

        // A 100-byte budget accommodates a single 59-byte decode (59 <= 100) but would fail
        // if decoded twice (118 > 100).
        let mut budget = test_budget(100);
        let mut dst_doc = Document::with_version("1.5");
        let form = page_to_form_xobject(
            &doc,
            page_id,
            &mut dst_doc,
            &mut ObjectCopier::new(),
            &mut budget,
            &ToolCtx::default(),
        )
        .expect("single decode within 100-byte budget must succeed");
        // Exactly 41 bytes of budget remain (`100 - 59 == 41`).
        budget
            .charge(41)
            .expect("charging remaining 41 bytes must succeed");
        assert!(matches!(budget.charge(1), Err(PdfError::LimitExceeded(_))));
        let form_bytes = dst_doc
            .get_object(form.id)
            .unwrap()
            .as_stream()
            .unwrap()
            .decompressed_content()
            .unwrap();
        assert_eq!(form_bytes, payload);

        // Inline Object::Stream inside `/Contents` array is included alongside indirect streams.
        let inline_part = Stream::new(dictionary! {}, b"q 1 0 0 1 0 0 cm".to_vec());
        let indirect_id =
            doc.add_object(Object::Stream(Stream::new(dictionary! {}, b"Q".to_vec())));
        let inline_page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => Object::Array(vec![
                Object::Stream(inline_part),
                Object::Reference(indirect_id),
            ]),
        });
        let mut inline_budget = test_budget(1024);
        let inline_form = page_to_form_xobject_in_place(
            &mut doc,
            inline_page_id,
            &mut inline_budget,
            &ToolCtx::default(),
        )
        .expect("convert page with inline stream in Contents array");
        let combined = doc
            .get_object(inline_form.id)
            .unwrap()
            .as_stream()
            .unwrap()
            .decompressed_content()
            .unwrap();
        assert_eq!(combined, b"q 1 0 0 1 0 0 cm\nQ");

        // Corrupt filtered stream (`budget.decode` -> `Ok(None)`) returns `PdfError::Malformed`
        // instead of splicing raw compressed bytes.
        let corrupt_stream = Stream::new(
            dictionary! { "Filter" => "BrotliDecode" },
            b"not-valid-brotli-stream".to_vec(),
        );
        let corrupt_id = doc.add_object(Object::Stream(corrupt_stream));
        let corrupt_page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => Object::Reference(corrupt_id),
        });
        let err = page_to_form_xobject_in_place(
            &mut doc,
            corrupt_page_id,
            &mut test_budget(1024),
            &ToolCtx::default(),
        )
        .unwrap_err();
        assert!(
            matches!(err, PdfError::Malformed),
            "expected Malformed for corrupt FlateDecode stream, got {err:?}"
        );
    }
}
