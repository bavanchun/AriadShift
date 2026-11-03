//! Components for bounded redaction processing.

mod flat;
mod history;
pub mod matching;
mod metrics;
mod normalize;
mod page;
#[cfg(feature = "codecs")]
pub mod pixels;
pub mod scrub;
mod signatures;
pub mod stream;
mod text;
mod verify_text;

pub use crate::xobject::Matrix;
#[cfg(feature = "codecs")]
pub use pixels::{verify_rendered_pixels, verify_rendered_pixels_with_overlay};
pub use signatures::has_signatures;

use ariad_core::{
    limits::Limits,
    tools::params::{
        redact::{RedactParams, RedactionTarget},
        redact_internal::RedactGeometry,
    },
};

use crate::{PdfError, ToolCtx, tools::rotate::collect_page_ids};
use lopdf::Object;
#[cfg(feature = "codecs")]
use lopdf::{Document, ObjectId, Stream, content::Content, dictionary};
use matching::{MatchBudget, Matchers};
use std::collections::BTreeSet;

/// Checks the structural portion of the full-rewrite verification.
///
/// This is only one mandatory check. It does not establish that text, object
/// streams, pixels, masks or other carriers have been redacted.
pub fn verify_rewrite_history(
    bytes: &[u8],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    history::verify(bytes, limits, ctx)
}

/// Checks raw bytes for literal target representations; decoded streams and
/// all nonliteral targets still require their own independent checks.
pub fn verify_raw_literals(
    bytes: &[u8],
    params: &RedactParams,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    history::raw_literals(bytes, params, ctx)
}

/// Checks candidate raw bytes for accidental plaintext residue of matched target strings from the plan.
pub fn verify_raw_literals_for_plan(
    bytes: &[u8],
    params: &RedactParams,
    plan: &RedactionPlan,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    let mut texts = plan.matched_texts().to_vec();
    for target in &params.targets {
        if let RedactionTarget::Exact { text } = target
            && !texts.contains(text)
        {
            texts.push(text.clone());
        }
    }
    history::raw_literals_for_texts(bytes, params, &texts, ctx)
}

/// Counts matching reachable PDF strings and streams still needing inspection.
/// The counts contain neither target values nor matched text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectStringEvidence {
    /// Reachable object strings that contain a requested text match.
    pub matched_strings: usize,
    /// Reachable stream payloads that this string-only check did not inspect.
    pub uninspected_streams: usize,
}

/// Examines reachable object strings without claiming that streams are safe.
pub fn inspect_object_strings(
    bytes: &[u8],
    params: &RedactParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ObjectStringEvidence, PdfError> {
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let mut pdf = crate::document::open_with_ctx(bytes, limits, ctx)?;
    // The canonical writer's binary content-derived file ID is not a text string.
    pdf.inner_mut()?.trailer.remove(b"ID");
    let matcher = Matchers::new(&params.targets).map_err(|e| PdfError::InvalidParams(e.into()))?;
    let stats =
        scrub::scrub_object_strings(&mut pdf, &matcher, &mut MatchBudget::with_context(ctx), ctx)?;
    Ok(ObjectStringEvidence {
        matched_strings: stats.scrubbed,
        uninspected_streams: stats.uninspected_streams,
    })
}

/// Independently checks the core PDF reader's extracted text against text targets.
/// This does not check pixels, hidden content, object streams, or geometry.
pub fn verify_core_text(
    bytes: &[u8],
    params: &RedactParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    verify_text::core_text(bytes, params, limits, ctx)
}

/// Checks text targets against fresh private PDFium evidence for the candidate.
/// Pixel, object-graph and geometric checks remain separate mandatory checks.
pub fn verify_pdfium_text(
    bytes: &[u8],
    params: &RedactParams,
    geometry: &RedactGeometry,
    _limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    use ariad_core::tools::params::redact_internal::RedactionCheckStatus::{
        Fail, Pass, Unavailable,
    };
    if geometry.schema != "ariad-redact-geometry/1" {
        return Err(PdfError::Malformed);
    }
    let computed_sha256 = crate::writer::sha256_chunks(&[bytes]);
    if geometry.source_sha256 != computed_sha256 {
        return Err(PdfError::Malformed);
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    if !params.scope.text {
        return Ok(Unavailable);
    }
    for target in &params.targets {
        match target {
            RedactionTarget::Page { page } => {
                if let Some(pg) = geometry.pages.iter().find(|p| p.page == *page) {
                    for ch in &pg.chars {
                        if !ch.generated {
                            return Ok(Fail);
                        }
                    }
                }
            }
            RedactionTarget::Rectangle { page, rect } => {
                if let Some(pg) = geometry.pages.iter().find(|p| p.page == *page) {
                    let r = [rect.left, rect.bottom, rect.right, rect.top];
                    let mut glyphs_in_rect = Vec::new();
                    for ch in &pg.chars {
                        if !ch.generated
                            && let Some(bbox) = ch.bbox
                            && bbox[0] < r[2]
                            && bbox[2] > r[0]
                            && bbox[1] < r[3]
                            && bbox[3] > r[1]
                        {
                            glyphs_in_rect.push(ch.value);
                        }
                    }
                    if !glyphs_in_rect.is_empty() {
                        if let Some(ref overlay) = params.overlay_text {
                            let text_in_rect: String = glyphs_in_rect.into_iter().collect();
                            if text_in_rect != *overlay {
                                return Ok(Fail);
                            }
                        } else {
                            return Ok(Fail);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let has_text_targets = params.targets.iter().any(|target| {
        matches!(
            target,
            RedactionTarget::Exact { .. }
                | RedactionTarget::Regex { .. }
                | RedactionTarget::Preset { .. }
        )
    });
    if has_text_targets {
        let matchers =
            Matchers::new(&params.targets).map_err(|e| PdfError::InvalidParams(e.into()))?;
        let mut budget = MatchBudget::with_context(ctx);
        for page in &geometry.pages {
            ctx.check()?;
            let mut text = String::new();
            for ch in &page.chars {
                text.push(ch.value);
            }
            if let Ok(found) = matchers.find(&text, &mut budget) {
                if !found.is_empty() {
                    return Ok(Fail);
                }
            } else {
                return Ok(Fail);
            }
        }
    }
    Ok(Pass)
}

/// A source page and the geometry to be removed by the rewrite stage.
#[derive(Clone, Debug, PartialEq)]
pub enum Region {
    /// Every object on a page is targeted.
    Page { page: u32 },
    /// PDF-point rectangle in the coordinate system recorded by the evidence stage.
    Rectangle { page: u32, bounds: [f64; 4] },
}

/// Private rewrite input. It does not contain target values or extracted text.
#[derive(Clone)]
pub struct RedactionPlan {
    source_sha256: [u8; 32],
    params_sha256: [u8; 32],
    geometry_sha256: [u8; 32],
    match_budget: MatchBudget,
    selected_characters: Vec<Vec<usize>>,
    /// Regions to be removed from pages.
    regions: Vec<Region>,
    /// Number of text occurrences, including overlaps.
    matches: usize,
    /// Whether a signed input was explicitly accepted for invalidation.
    invalidates_signatures: bool,
    /// Whether all requested carrier scopes are included.
    complete_scope: bool,
    matched_texts: Vec<String>,
}

impl std::fmt::Debug for RedactionPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedactionPlan")
            .field("regions", &self.regions)
            .field("matches", &self.matches)
            .field("invalidates_signatures", &self.invalidates_signatures)
            .field("complete_scope", &self.complete_scope)
            .finish()
    }
}

impl RedactionPlan {
    /// PDFium character indices selected on a one-based page.
    #[must_use]
    pub fn selected_characters(&self, page: u32) -> Option<&[usize]> {
        page.checked_sub(1)
            .and_then(|index| self.selected_characters.get(index as usize))
            .map(Vec::as_slice)
    }
    /// Regions to be removed from pages.
    #[must_use]
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }
    /// Number of matched occurrences.
    #[must_use]
    pub const fn match_count(&self) -> usize {
        self.matches
    }
    /// Matched target text strings, for verification against raw candidate bytes.
    #[must_use]
    pub fn matched_texts(&self) -> &[String] {
        &self.matched_texts
    }
    /// Whether this operation explicitly invalidates signatures.
    #[must_use]
    pub const fn invalidates_signatures(&self) -> bool {
        self.invalidates_signatures
    }
    /// Whether every carrier scope was selected.
    #[must_use]
    pub const fn complete_scope(&self) -> bool {
        self.complete_scope
    }
}

fn params_digest(params: &RedactParams) -> Result<[u8; 32], PdfError> {
    let bytes = serde_json::to_vec(params)
        .map_err(|_| PdfError::InvalidParams("invalid redaction parameters".into()))?;
    Ok(crate::writer::sha256_chunks(&[&bytes]))
}

fn geometry_digest(geometry: &RedactGeometry) -> Result<[u8; 32], PdfError> {
    let bytes = serde_json::to_vec(geometry)
        .map_err(|_| PdfError::InvalidParams("invalid redaction geometry evidence".into()))?;
    Ok(crate::writer::sha256_chunks(&[&bytes]))
}

/// Validates input and translates private character evidence into rewrite regions.
///
/// This prepares a rewrite only; it does not remove or verify any document content.
pub fn plan(
    bytes: &[u8],
    params: &RedactParams,
    geometry: &RedactGeometry,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<RedactionPlan, PdfError> {
    params
        .validate()
        .map_err(|error| PdfError::InvalidParams(error.to_string()))?;
    let doc = crate::document::open_with_ctx(bytes, limits, ctx)?;
    let signed = signatures::has_signatures(doc.inner(), ctx)?;
    if signed && !params.invalidate_signatures {
        return Err(PdfError::InvalidParams(
            "redaction would invalidate existing signatures; explicit confirmation is required"
                .into(),
        ));
    }
    let count = crate::tools::rotate::collect_page_ids(doc.inner(), ctx)?.len();
    if geometry.source_sha256 != crate::writer::sha256_chunks(&[bytes])
        || geometry.schema != "ariad-redact-geometry/1"
        || geometry.page_count != count
        || geometry.pages.len() != count
    {
        return Err(PdfError::InvalidParams(
            "incomplete redaction geometry evidence".into(),
        ));
    }
    let matcher =
        Matchers::new(&params.targets).map_err(|error| PdfError::InvalidParams(error.into()))?;
    let mut evidence_chars = 0usize;
    let mut plan = RedactionPlan {
        source_sha256: geometry.source_sha256,
        params_sha256: params_digest(params)?,
        geometry_sha256: [0; 32],
        match_budget: MatchBudget::with_context(ctx),
        selected_characters: Vec::new(),
        regions: Vec::new(),
        matches: 0,
        invalidates_signatures: signed,
        complete_scope: params.scope.is_complete(),
        matched_texts: Vec::new(),
    };
    let mut selected: Vec<BTreeSet<usize>> = (0..count).map(|_| BTreeSet::new()).collect();
    for (position, page) in geometry.pages.iter().enumerate() {
        ctx.check()?;
        if page.page as usize != position + 1
            || !page.width.is_finite()
            || !page.height.is_finite()
            || page.width <= 0.0
            || page.height <= 0.0
            || !matches!(page.rotation, 0 | 90 | 180 | 270)
        {
            return Err(PdfError::InvalidParams(
                "invalid redaction geometry evidence".into(),
            ));
        }
        let mut text = String::new();
        evidence_chars = evidence_chars
            .checked_add(page.chars.len())
            .ok_or_else(|| {
                PdfError::InvalidParams("redaction character evidence limit exceeded".into())
            })?;
        if evidence_chars > 1_000_000 {
            return Err(PdfError::InvalidParams(
                "redaction character evidence limit exceeded".into(),
            ));
        }
        let mut offsets = Vec::with_capacity(page.chars.len());
        for (index, character) in page.chars.iter().enumerate() {
            if character.index != index || character.value == '\0' {
                return Err(PdfError::InvalidParams(
                    "invalid redaction character evidence".into(),
                ));
            }
            if character
                .origin
                .is_some_and(|point| !point.iter().all(|value| value.is_finite()))
                || (character.origin.is_none()
                    && !character.generated
                    && !character.value.is_whitespace())
            {
                return Err(PdfError::InvalidParams(
                    "invalid redaction character origin".into(),
                ));
            }
            if let Some([left, bottom, right, top]) = character.bbox {
                if ![left, bottom, right, top].iter().all(|v| v.is_finite())
                    || left > right
                    || bottom > top
                {
                    return Err(PdfError::InvalidParams(
                        "invalid redaction character geometry".into(),
                    ));
                }
            } else if !character.generated && !character.value.is_whitespace() {
                return Err(PdfError::InvalidParams(
                    "missing redaction character geometry".into(),
                ));
            }
            offsets.push(text.len());
            text.push(character.value);
        }
        let found = matcher.find(&text, &mut plan.match_budget);
        ctx.check()?;
        let matches = found.map_err(|error| PdfError::InvalidParams(error.into()))?;
        plan.matches += matches.len();
        for (match_idx, found) in matches.iter().enumerate() {
            if match_idx.is_multiple_of(128) {
                ctx.check()?;
            }
            if let Some(matched_slice) = text.get(found.start..found.end)
                && !matched_slice.is_empty()
                && !plan.matched_texts.iter().any(|s| s == matched_slice)
            {
                plan.matched_texts.push(matched_slice.to_owned());
            }
            let first = offsets.partition_point(|offset| *offset < found.start);
            let last = offsets.partition_point(|offset| *offset < found.end);
            let mut mapped = false;
            for character in &page.chars[first..last] {
                if !character.generated {
                    selected[position].insert(character.index);
                    if character.bbox.is_some() {
                        mapped = true;
                    }
                }
            }
            if !mapped {
                return Err(PdfError::InvalidParams(
                    "a text match has no removable page geometry".into(),
                ));
            }
        }
        let mut char_boxes: Vec<[f64; 4]> = selected[position]
            .iter()
            .filter_map(|&idx| {
                let c = &page.chars[idx];
                if !c.generated { c.bbox } else { None }
            })
            .collect();
        char_boxes.sort_by(|a, b| {
            a[1].partial_cmp(&b[1])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal))
        });
        let mut merged: Vec<[f64; 4]> = Vec::new();
        for b in char_boxes {
            if let Some(last) = merged.last_mut()
                && (last[1] - b[1]).abs() <= 2.0
                && (last[3] - b[3]).abs() <= 2.0
                && b[0] <= last[2] + 5.0
            {
                last[0] = last[0].min(b[0]);
                last[1] = last[1].min(b[1]);
                last[2] = last[2].max(b[2]);
                last[3] = last[3].max(b[3]);
                continue;
            }
            merged.push(b);
        }
        for bounds in merged {
            plan.regions.push(Region::Rectangle {
                page: page.page,
                bounds,
            });
        }
    }
    for target in &params.targets {
        match target {
            RedactionTarget::Page { page } => {
                if *page as usize > count {
                    return Err(PdfError::InvalidParams(
                        "redaction page is outside the document".into(),
                    ));
                }
                for character in &geometry.pages[*page as usize - 1].chars {
                    if !character.generated {
                        selected[*page as usize - 1].insert(character.index);
                    }
                }
                plan.regions.push(Region::Page { page: *page });
            }
            RedactionTarget::Rectangle { page, rect } => {
                if *page as usize > count {
                    return Err(PdfError::InvalidParams(
                        "redaction page is outside the document".into(),
                    ));
                }
                for character in &geometry.pages[*page as usize - 1].chars {
                    if character.generated {
                        continue;
                    }
                    let hit = if let Some([left, bottom, right, top]) = character.bbox {
                        if left < right && bottom < top {
                            left < rect.right
                                && right > rect.left
                                && bottom < rect.top
                                && top > rect.bottom
                        } else if let Some([x, y]) = character.origin {
                            x >= rect.left && x <= rect.right && y >= rect.bottom && y <= rect.top
                        } else {
                            return Err(PdfError::NotImplemented(
                                "character geometry for rectangle redaction".into(),
                            ));
                        }
                    } else if let Some([x, y]) = character.origin {
                        x >= rect.left && x <= rect.right && y >= rect.bottom && y <= rect.top
                    } else {
                        return Err(PdfError::NotImplemented(
                            "character geometry for rectangle redaction".into(),
                        ));
                    };
                    if hit {
                        selected[*page as usize - 1].insert(character.index);
                    }
                }
                plan.regions.push(Region::Rectangle {
                    page: *page,
                    bounds: [rect.left, rect.bottom, rect.right, rect.top],
                });
            }
            _ => {}
        }
    }
    if plan.regions.len() > 100_000 {
        return Err(PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::TooManyObjects { limit: 100_000 },
        ));
    }
    plan.selected_characters = selected
        .into_iter()
        .map(|indices| indices.into_iter().collect())
        .collect();
    plan.geometry_sha256 = geometry_digest(geometry)?;
    ctx.check()?;
    Ok(plan)
}

/// Rewrites content streams, removes annotations and opaque carriers, and scrubs matching
/// object strings across the whole document.
///
/// Returns private intermediate candidate bytes. These must undergo all mandatory
/// verification gates before atomic publication.
pub fn rewrite_document(
    bytes: &[u8],
    params: &RedactParams,
    plan: &RedactionPlan,
    geometry: &RedactGeometry,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    rewrite_document_with_evidence(bytes, params, plan, geometry, limits, ctx).map(|(b, _)| b)
}

/// Rewrites content streams, removes annotations and opaque carriers, scrubs matching
/// object strings, and records modifications across carriers.
pub fn rewrite_document_with_evidence(
    bytes: &[u8],
    params: &RedactParams,
    plan: &RedactionPlan,
    geometry: &RedactGeometry,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<
    (
        Vec<u8>,
        ariad_core::tools::params::redact::RedactionCarrierEvidence,
    ),
    PdfError,
> {
    use std::collections::BTreeSet;
    if plan.source_sha256 != crate::writer::sha256_chunks(&[bytes])
        || plan.params_sha256 != params_digest(params)?
        || plan.geometry_sha256 != geometry_digest(geometry)?
    {
        return Err(PdfError::InvalidParams(
            "redaction evidence does not match the request".into(),
        ));
    }
    let mut doc = crate::document::open_with_ctx(bytes, limits, ctx)?;
    if signatures::has_signatures(doc.inner(), ctx)? && !params.invalidate_signatures {
        return Err(PdfError::InvalidParams(
            "redaction would invalidate existing signatures; explicit confirmation is required"
                .into(),
        ));
    }

    let mut carrier_evidence = ariad_core::tools::params::redact::RedactionCarrierEvidence {
        text_removed: plan.match_count(),
        images_modified: 0,
        vectors_inspected: 0,
        annotations_removed: 0,
        metadata_scrubbed: 0,
        attachments_removed: 0,
    };

    let mut whole_pages = BTreeSet::new();
    let mut has_rectangles = false;
    for region in &plan.regions {
        match region {
            Region::Page { page } => {
                whole_pages.insert(*page);
            }
            Region::Rectangle { .. } => {
                has_rectangles = true;
            }
        }
    }

    if !whole_pages.is_empty() {
        let color = params
            .color
            .as_ref()
            .map_or("#000000", |color| color.as_str());
        let mut rgb = [0u8; 3];
        for (index, component) in rgb.iter_mut().enumerate() {
            *component = u8::from_str_radix(&color[1 + index * 2..3 + index * 2], 16)
                .map_err(|_| PdfError::Malformed)?;
        }
        carrier_evidence.annotations_removed +=
            page::clear_pages(&mut doc, &whole_pages, rgb, ctx)?;
    }

    if has_rectangles {
        let (annots_removed, vectors_inspected) =
            flat::rewrite(&mut doc, bytes.len(), params, plan, geometry, limits, ctx)?;
        carrier_evidence.annotations_removed += annots_removed;
        carrier_evidence.vectors_inspected += vectors_inspected;
    }

    if params.scope.is_complete() || params.scope.images {
        #[cfg(feature = "codecs")]
        {
            carrier_evidence.images_modified = rewrite_image_pixels(&mut doc, plan, limits, ctx)?;
        }
    }

    let doc_inner = doc.inner_mut()?;
    let affected_page_nums: BTreeSet<u32> = plan
        .regions()
        .iter()
        .map(|r| match r {
            Region::Page { page } => *page,
            Region::Rectangle { page, .. } => *page,
        })
        .collect();
    if let Ok(p_ids) = collect_page_ids(doc_inner, ctx) {
        for pnum in affected_page_nums {
            if let Some(pid) = pnum.checked_sub(1).and_then(|idx| p_ids.get(idx as usize))
                && let Ok(pdict) = doc_inner.get_object_mut(*pid).and_then(Object::as_dict_mut)
            {
                pdict.remove(b"Thumb");
            }
        }
    }

    if params.scope.is_complete() || params.scope.attachments {
        carrier_evidence.attachments_removed = scrub::remove_opaque_carriers(&mut doc, ctx)?;
    }
    doc.inner_mut()?.trailer.remove(b"ID");

    if params.scope.is_complete()
        || params.scope.metadata
        || params.targets.iter().any(|target| {
            matches!(
                target,
                RedactionTarget::Exact { .. }
                    | RedactionTarget::Regex { .. }
                    | RedactionTarget::Preset { .. }
            )
        })
    {
        let matchers = Matchers::new(&params.targets)
            .map_err(|error| PdfError::InvalidParams(error.into()))?;
        let mut budget = plan.match_budget.clone();
        budget.set_context(ctx);
        let stats = scrub::scrub_object_strings(&mut doc, &matchers, &mut budget, ctx)?;
        carrier_evidence.metadata_scrubbed = stats.scrubbed;
    }

    let output_bytes = crate::writer::save(&mut doc, ctx)?;
    Ok((output_bytes, carrier_evidence))
}

#[cfg(feature = "codecs")]
fn collect_page_image_placements(
    doc: &Document,
    page_id: ObjectId,
    budget: &mut crate::document::DecodeBudget,
    ctx: &ToolCtx,
) -> Result<Vec<(ObjectId, Matrix)>, PdfError> {
    let mut placements = Vec::new();
    let page_dict = doc
        .get_dictionary(page_id)
        .map_err(|_| PdfError::Malformed)?;
    let resources = crate::xobject::resolve_page_resources(doc, page_id);
    let Ok(contents_val) = page_dict.get(b"Contents") else {
        return Ok(placements);
    };
    let (_, contents_val) = doc
        .dereference(contents_val)
        .map_err(|_| PdfError::Malformed)?;
    let content_streams: Vec<&Object> = match contents_val {
        Object::Array(arr) => arr.iter().collect(),
        Object::Stream(_) => vec![contents_val],
        _ => return Ok(placements),
    };
    let mut content_bytes = Vec::new();
    for obj in content_streams {
        ctx.check()?;
        let (_, derefed) = doc.dereference(obj).map_err(|_| PdfError::Malformed)?;
        if let Ok(stream) = derefed.as_stream()
            && let Ok(Some(decoded)) = budget.decode(stream)
        {
            content_bytes.extend_from_slice(&decoded);
            content_bytes.push(b'\n');
        }
    }
    if content_bytes.is_empty() {
        return Ok(placements);
    }
    let parsed = match Content::decode(&content_bytes) {
        Ok(c) => c,
        Err(_) => return Ok(placements),
    };
    let mut ctm_stack = Vec::new();
    let mut ctm = Matrix::identity();
    for op in parsed.operations {
        match op.operator.as_str() {
            "q" => ctm_stack.push(ctm),
            "Q" => {
                if let Some(prev) = ctm_stack.pop() {
                    ctm = prev;
                }
            }
            "cm" => {
                if op.operands.len() == 6 {
                    let mut vals = [0.0; 6];
                    let mut ok = true;
                    for (i, v) in op.operands.iter().enumerate() {
                        match v {
                            Object::Integer(n) => vals[i] = *n as f64,
                            Object::Real(r) => vals[i] = f64::from(*r),
                            _ => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok && vals.iter().all(|x| x.is_finite()) {
                        ctm = Matrix(vals).then(ctm);
                    }
                }
            }
            "Do" => {
                if let Some(name) = op.operands.first().and_then(|v| v.as_name().ok())
                    && let Ok(xobjs) = resources
                        .get(b"XObject")
                        .and_then(|v| doc.dereference(v))
                        .and_then(|(_, v)| v.as_dict())
                    && let Ok(xobj_ref) = xobjs.get(name)
                    && let Ok(id) = xobj_ref.as_reference()
                    && let Ok(xobj_stream) = doc.get_object(id).and_then(Object::as_stream)
                    && xobj_stream
                        .dict
                        .get(b"Subtype")
                        .is_ok_and(|s| matches!(s.as_name(), Ok(b"Image")))
                {
                    placements.push((id, ctm));
                }
            }
            _ => {}
        }
    }
    Ok(placements)
}

#[cfg(feature = "codecs")]
fn image_placement_bbox(ctm: Matrix) -> [f64; 4] {
    let [a, b, c, d, e, f] = ctm.0;
    let p0 = (e, f);
    let p1 = (a + e, b + f);
    let p2 = (c + e, d + f);
    let p3 = (a + c + e, b + d + f);
    let min_x = p0.0.min(p1.0).min(p2.0).min(p3.0);
    let max_x = p0.0.max(p1.0).max(p2.0).max(p3.0);
    let min_y = p0.1.min(p1.1).min(p2.1).min(p3.1);
    let max_y = p0.1.max(p1.1).max(p2.1).max(p3.1);
    [min_x, min_y, max_x, max_y]
}

#[cfg(feature = "codecs")]
fn rewrite_image_pixels(
    doc: &mut crate::PdfDoc,
    plan: &RedactionPlan,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<usize, PdfError> {
    let page_ids = collect_page_ids(doc.inner(), ctx)?;
    let mut decode_budget = doc.decode_budget();
    let mut modified_count = 0usize;
    for (page_idx, page_id) in page_ids.iter().enumerate() {
        let page_num = page_idx as u32 + 1;
        let mut regions_on_page: Vec<[f64; 4]> = Vec::new();
        for r in plan.regions() {
            match r {
                Region::Page { page } if *page == page_num => {
                    regions_on_page.push([0.0, 0.0, 100_000.0, 100_000.0]);
                }
                Region::Rectangle { page, bounds } if *page == page_num => {
                    regions_on_page.push(*bounds);
                }
                _ => {}
            }
        }
        if regions_on_page.is_empty() {
            continue;
        }
        let placements =
            collect_page_image_placements(doc.inner(), *page_id, &mut decode_budget, ctx)?;
        for (img_id, ctm) in placements {
            ctx.check()?;
            let img_bbox = image_placement_bbox(ctm);
            let intersects = regions_on_page.iter().any(|r| {
                img_bbox[0] < r[2] && img_bbox[2] > r[0] && img_bbox[1] < r[3] && img_bbox[3] > r[1]
            });
            if !intersects {
                continue;
            }
            let mut decoded_img = crate::images::decode(doc, img_id, ctx, limits)
                .map_err(|_| PdfError::NotImplemented("image decode for redaction".into()))?;
            let smask_id = doc
                .inner()
                .get_object(img_id)
                .and_then(Object::as_stream)
                .ok()
                .and_then(|s| s.dict.get(b"SMask").ok())
                .and_then(|v| v.as_reference().ok());
            let mut smask_img = if let Some(sid) = smask_id {
                Some(crate::images::decode(doc, sid, ctx, limits).map_err(|_| {
                    PdfError::NotImplemented("image smask decode for redaction".into())
                })?)
            } else {
                None
            };
            let mask_id = doc
                .inner()
                .get_object(img_id)
                .and_then(Object::as_stream)
                .ok()
                .and_then(|s| s.dict.get(b"Mask").ok())
                .and_then(|v| v.as_reference().ok());
            let mut mask_img = if let Some(mid) = mask_id {
                Some(crate::images::decode(doc, mid, ctx, limits).map_err(|_| {
                    PdfError::NotImplemented("image mask decode for redaction".into())
                })?)
            } else {
                None
            };
            let cleared = pixels::clear_pixels(
                &mut decoded_img,
                smask_img.as_mut(),
                mask_img.as_mut(),
                ctm,
                &regions_on_page,
                limits,
                ctx,
            )?;
            if cleared > 0 {
                modified_count += 1;
                if let (Some(sid), Some(simg)) = (smask_id, smask_img) {
                    let new_smask_stream = pixels::encode_unmasked_image(&simg, ctx)?;
                    doc.inner_mut()?.set_object(sid, new_smask_stream);
                }
                if let (Some(mid), Some(mimg)) = (mask_id, mask_img) {
                    let new_mask_stream = pixels::encode_unmasked_image(&mimg, ctx)?;
                    doc.inner_mut()?.set_object(mid, new_mask_stream);
                }
                let (encoded, parameters) = crate::images::encode_flate(&decoded_img, 6)
                    .map_err(|_| PdfError::NotImplemented("image encoding".into()))?;
                let color_space = match decoded_img.color_space {
                    crate::images::ColorSpace::Gray => "DeviceGray",
                    crate::images::ColorSpace::Rgb => "DeviceRGB",
                    crate::images::ColorSpace::Cmyk => "DeviceCMYK",
                };
                let mut dict = lopdf::dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => i64::from(decoded_img.width),
                    "Height" => i64::from(decoded_img.height),
                    "ColorSpace" => color_space,
                    "BitsPerComponent" => i64::from(decoded_img.bits_per_component),
                    "Filter" => "FlateDecode",
                    "DecodeParms" => parameters,
                };
                if let Some(sid) = smask_id {
                    dict.set("SMask", Object::Reference(sid));
                }
                if let Some(mid) = mask_id {
                    dict.set("Mask", Object::Reference(mid));
                }
                doc.inner_mut()?
                    .set_object(img_id, Stream::new(dict, encoded));
            }
        }
    }
    Ok(modified_count)
}

/// Whole-object-graph scan across all reachable objects and decoded streams.
pub fn verify_object_graph(
    bytes: &[u8],
    params: &RedactParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    scrub::verify_object_graph(bytes, params, limits, ctx)
}

/// Verifies that image XObjects on affected pages do not retain non-redacted pixels in redaction regions.
#[cfg(feature = "codecs")]
pub fn verify_image_pixels(
    bytes: &[u8],
    plan: &RedactionPlan,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    use ariad_core::tools::params::redact_internal::RedactionCheckStatus::{
        Fail, Pass, Unavailable,
    };
    if !plan.complete_scope() {
        return Ok(Unavailable);
    }
    let doc = crate::document::open_with_ctx(bytes, limits, ctx)?;
    let mut decode_budget = doc.decode_budget();
    let page_ids = collect_page_ids(doc.inner(), ctx)?;
    #[allow(clippy::type_complexity)]
    let mut image_placements: std::collections::BTreeMap<
        ObjectId,
        Vec<(Matrix, Vec<[f64; 4]>)>,
    > = std::collections::BTreeMap::new();
    for (page_idx, page_id) in page_ids.iter().enumerate() {
        let page_num = page_idx as u32 + 1;
        let regions_for_page: Vec<[f64; 4]> = plan
            .regions()
            .iter()
            .filter_map(|r| match r {
                Region::Page { page } if *page == page_num => {
                    Some([0.0, 0.0, 100_000.0, 100_000.0])
                }
                Region::Rectangle { page, bounds } if *page == page_num => Some(*bounds),
                _ => None,
            })
            .collect();
        if regions_for_page.is_empty() {
            continue;
        }
        let placements =
            collect_page_image_placements(doc.inner(), *page_id, &mut decode_budget, ctx)?;
        for (img_id, ctm) in placements {
            let img_bbox = image_placement_bbox(ctm);
            let intersecting: Vec<[f64; 4]> = regions_for_page
                .iter()
                .copied()
                .filter(|r| {
                    img_bbox[0] < r[2]
                        && img_bbox[2] > r[0]
                        && img_bbox[1] < r[3]
                        && img_bbox[3] > r[1]
                })
                .collect();
            if !intersecting.is_empty() {
                image_placements
                    .entry(img_id)
                    .or_default()
                    .push((ctm, intersecting));
            }
        }
    }
    for (img_id, placements) in image_placements {
        ctx.check()?;
        let decoded = match crate::images::decode(&doc, img_id, ctx, limits) {
            Ok(img) => img,
            Err(_) => return Ok(Fail),
        };
        let smask_decoded =
            if let Ok(s) = doc.inner().get_object(img_id).and_then(Object::as_stream) {
                if let Ok(smask_id) = s.dict.get(b"SMask").and_then(Object::as_reference) {
                    crate::images::decode(&doc, smask_id, ctx, limits).ok()
                } else {
                    None
                }
            } else {
                None
            };
        let channels = match decoded.color_space {
            crate::images::ColorSpace::Gray => 1usize,
            crate::images::ColorSpace::Rgb => 3,
            crate::images::ColorSpace::Cmyk => 4,
        };
        let stride = channels * usize::from(decoded.bits_per_component / 8);
        for (ctm, regions) in placements {
            let mut work = 0u64;
            let sel = match pixels::selection(
                decoded.width,
                decoded.height,
                ctm,
                &regions,
                limits,
                ctx,
                &mut work,
                false,
            ) {
                Ok(s) => s,
                Err(_) => return Ok(Fail),
            };
            if let Some(ref smask) = smask_decoded {
                let mask_sel = match pixels::selection(
                    smask.width,
                    smask.height,
                    ctm,
                    &regions,
                    limits,
                    ctx,
                    &mut work,
                    false,
                ) {
                    Ok(s) => s,
                    Err(_) => return Ok(Fail),
                };
                let mask_stride = usize::from(smask.bits_per_component / 8);
                for (pixel, hit) in smask.pixels.chunks_exact(mask_stride).zip(mask_sel.iter()) {
                    if *hit && pixel.iter().any(|&b| b != 255) {
                        return Ok(Fail);
                    }
                }
            }
            for (pixel, hit) in decoded.pixels.chunks_exact(stride).zip(sel.iter()) {
                if *hit {
                    if decoded.color_space == crate::images::ColorSpace::Cmyk {
                        if pixel != [0, 0, 0, 255] && pixel != [0, 0, 0, 0] {
                            return Ok(Fail);
                        }
                    } else if pixel.iter().any(|&b| b != 0) {
                        return Ok(Fail);
                    }
                }
            }
        }
    }
    Ok(Pass)
}

#[cfg(not(feature = "codecs"))]
pub fn verify_image_pixels(
    _bytes: &[u8],
    _plan: &RedactionPlan,
    _limits: &Limits,
    _ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    Ok(ariad_core::tools::params::redact_internal::RedactionCheckStatus::Unavailable)
}

/// Rewrites whole-page content and scrubs matching object strings before verification.
///
/// These bytes are an intermediate only and must not be published as redacted output.
/// Other region kinds are rejected until their content rewrite is available.
pub fn rewrite_page_content(
    bytes: &[u8],
    params: &RedactParams,
    plan: &RedactionPlan,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    use std::collections::BTreeSet;
    if plan.source_sha256 != crate::writer::sha256_chunks(&[bytes])
        || plan.params_sha256 != params_digest(params)?
    {
        return Err(PdfError::InvalidParams(
            "redaction plan does not match the input".into(),
        ));
    }
    if params.overlay_text.is_some() {
        return Err(PdfError::NotImplemented("redaction overlay text".into()));
    }
    let mut pages = BTreeSet::new();
    for region in &plan.regions {
        match region {
            Region::Page { page } => {
                pages.insert(*page);
            }
            Region::Rectangle { .. } => {
                return Err(PdfError::NotImplemented(
                    "redaction rectangle rewrite".into(),
                ));
            }
        }
    }
    let mut doc = crate::document::open_with_ctx(bytes, limits, ctx)?;
    if signatures::has_signatures(doc.inner(), ctx)? && !params.invalidate_signatures {
        return Err(PdfError::InvalidParams(
            "redaction would invalidate existing signatures; explicit confirmation is required"
                .into(),
        ));
    }
    let color = params
        .color
        .as_ref()
        .map_or("#000000", |color| color.as_str());
    let mut rgb = [0u8; 3];
    for (index, component) in rgb.iter_mut().enumerate() {
        *component = u8::from_str_radix(&color[1 + index * 2..3 + index * 2], 16)
            .map_err(|_| PdfError::Malformed)?;
    }
    page::clear_pages(&mut doc, &pages, rgb, ctx)?;
    if params.scope.is_complete() {
        // Embedded files and XFA are opaque to text matching. Dropping references
        // lets the full writer collect the unreachable payloads.
        scrub::remove_opaque_carriers(&mut doc, ctx)?;
    }
    if params.scope.is_complete()
        && params.targets.iter().any(|target| {
            matches!(
                target,
                RedactionTarget::Exact { .. }
                    | RedactionTarget::Regex { .. }
                    | RedactionTarget::Preset { .. }
            )
        })
    {
        // The canonical writer discards the old ID; it is binary data, not a PDF text string.
        doc.inner_mut()?.trailer.remove(b"ID");
        let matchers = Matchers::new(&params.targets)
            .map_err(|error| PdfError::InvalidParams(error.into()))?;
        let mut budget = plan.match_budget.clone();
        budget.set_context(ctx);
        let _stats = scrub::scrub_object_strings(&mut doc, &matchers, &mut budget, ctx)?;
        // Stream payloads remain unchecked. The caller must run all verification stages.
    }
    crate::writer::save(&mut doc, ctx)
}

/// Rewrites correlated text in flat page streams for subsequent graph and visual verification.
///
/// The returned bytes are private intermediate data. Unsupported page content fails closed;
/// no caller may publish them without completing all mandatory verification checks.
pub fn rewrite_flat_text_content(
    bytes: &[u8],
    params: &RedactParams,
    plan: &RedactionPlan,
    geometry: &RedactGeometry,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    if plan.source_sha256 != crate::writer::sha256_chunks(&[bytes])
        || plan.params_sha256 != params_digest(params)?
        || plan.geometry_sha256 != geometry_digest(geometry)?
    {
        return Err(PdfError::InvalidParams(
            "redaction evidence does not match the request".into(),
        ));
    }
    if !params.scope.is_complete()
        || params.overlay_text.is_some()
        || plan
            .regions
            .iter()
            .any(|r| matches!(r, Region::Page { .. }))
    {
        return Err(PdfError::NotImplemented(
            "redaction request for flat text rewrite".into(),
        ));
    }
    let mut doc = crate::document::open_with_ctx(bytes, limits, ctx)?;
    if signatures::has_signatures(doc.inner(), ctx)? && !params.invalidate_signatures {
        return Err(PdfError::InvalidParams(
            "redaction would invalidate existing signatures; explicit confirmation is required"
                .into(),
        ));
    }
    flat::rewrite(&mut doc, bytes.len(), params, plan, geometry, limits, ctx)?;
    scrub::remove_opaque_carriers(&mut doc, ctx)?;
    doc.inner_mut()?.trailer.remove(b"ID");
    let matchers =
        Matchers::new(&params.targets).map_err(|error| PdfError::InvalidParams(error.into()))?;
    let mut budget = plan.match_budget.clone();
    budget.set_context(ctx);
    let _stats = scrub::scrub_object_strings(&mut doc, &matchers, &mut budget, ctx)?;
    crate::writer::save(&mut doc, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::tools::params::{
        redact::RedactionScope,
        redact_internal::{RedactGeometryChar, RedactGeometryPage},
    };
    use lopdf::{Document, Object, dictionary};

    fn source(signed: bool) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()] });
        doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
        let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages, "Title" => lopdf::text_string("secret metadata") };
        if signed {
            let sig = doc.add_object(dictionary! { "Type" => "Sig", "ByteRange" => vec![0.into(), 1.into(), 2.into(), 3.into()] });
            catalog.set("Signature", sig);
        }
        let root = doc.add_object(catalog);
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    fn evidence(bytes: &[u8]) -> RedactGeometry {
        let chars = "a\u{306}\u{301}"
            .chars()
            .enumerate()
            .map(|(index, value)| RedactGeometryChar {
                value,
                index,
                generated: false,
                origin: Some([index as f64, 1.0]),
                bbox: Some([index as f64, 1.0, index as f64 + 1.0, 2.0]),
            })
            .collect();
        RedactGeometry {
            schema: "ariad-redact-geometry/1".into(),
            source_sha256: crate::writer::sha256_chunks(&[bytes]),
            page_count: 1,
            pages: vec![RedactGeometryPage {
                page: 1,
                width: 100.0,
                height: 100.0,
                rotation: 0,
                chars,
            }],
        }
    }

    fn params() -> RedactParams {
        RedactParams {
            targets: vec![RedactionTarget::Exact { text: "ắ".into() }],
            color: None,
            overlay_text: None,
            scope: RedactionScope::default(),
            invalidate_signatures: false,
        }
    }

    #[test]
    fn maps_all_combining_glyphs_and_refuses_mismatched_evidence() {
        let bytes = source(false);
        let geometry = evidence(&bytes);
        let planned = plan(
            &bytes,
            &params(),
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(planned.matches, 1);
        assert_eq!(planned.regions.len(), 1);
        assert_eq!(planned.selected_characters(1), Some(&[0, 1, 2][..]));
        let mut stale = geometry;
        stale.source_sha256[0] ^= 1;
        assert!(plan(&bytes, &params(), &stale, &Limits::local(), &ToolCtx::new()).is_err());
    }

    #[test]
    fn requires_explicit_signature_invalidation() {
        let bytes = source(true);
        let geometry = evidence(&bytes);
        let mut request = params();
        assert!(
            plan(
                &bytes,
                &request,
                &geometry,
                &Limits::local(),
                &ToolCtx::new()
            )
            .is_err()
        );
        request.invalidate_signatures = true;
        assert!(
            plan(
                &bytes,
                &request,
                &geometry,
                &Limits::local(),
                &ToolCtx::new()
            )
            .unwrap()
            .invalidates_signatures
        );
    }

    #[test]
    fn page_rewrite_accepts_only_a_plan_bound_to_its_source() {
        let bytes = source(false);
        let geometry = evidence(&bytes);
        let mut request = params();
        request.targets = vec![RedactionTarget::Page { page: 1 }];
        let plan = plan(
            &bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let output =
            rewrite_page_content(&bytes, &request, &plan, &Limits::local(), &ToolCtx::new())
                .unwrap();
        assert!(crate::document::open(&output, &Limits::local()).is_ok());
        let mut changed_request = request.clone();
        changed_request.scope.metadata = false;
        assert!(
            rewrite_page_content(
                &bytes,
                &changed_request,
                &plan,
                &Limits::local(),
                &ToolCtx::new()
            )
            .is_err()
        );
        let mut wrong = bytes;
        wrong.push(b' ');
        assert!(
            rewrite_page_content(&wrong, &request, &plan, &Limits::local(), &ToolCtx::new())
                .is_err()
        );
    }

    #[test]
    fn intermediate_scrubs_matching_graph_strings() {
        let bytes = source(false);
        let geometry = evidence(&bytes);
        let mut request = params();
        request.targets = vec![RedactionTarget::Exact {
            text: "secret".into(),
        }];
        let before =
            inspect_object_strings(&bytes, &request, &Limits::local(), &ToolCtx::new()).unwrap();
        assert_eq!(before.matched_strings, 1);
        let plan = plan(
            &bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let output =
            rewrite_page_content(&bytes, &request, &plan, &Limits::local(), &ToolCtx::new())
                .unwrap();
        let after =
            inspect_object_strings(&output, &request, &Limits::local(), &ToolCtx::new()).unwrap();
        assert_eq!(after.matched_strings, 0);
        assert_eq!(after.uninspected_streams, 0);
        let doc = crate::document::open(&output, &Limits::local()).unwrap();
        assert!(
            doc.inner()
                .catalog()
                .unwrap()
                .get(b"Title")
                .unwrap()
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rewrite_document_handles_mixed_targets_and_verifies_graph() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        let bytes = source(false);
        let geometry = evidence(&bytes);
        let mut request = params();
        request.targets = vec![
            RedactionTarget::Page { page: 1 },
            RedactionTarget::Exact {
                text: "secret".into(),
            },
        ];
        request.overlay_text = Some("REDACTED".into());
        let plan = plan(
            &bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let output = rewrite_document(
            &bytes,
            &request,
            &plan,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert!(crate::document::open(&output, &Limits::local()).is_ok());

        // Verify graph passes
        let check =
            verify_object_graph(&output, &request, &Limits::local(), &ToolCtx::new()).unwrap();
        assert_eq!(check, RedactionCheckStatus::Pass);

        // Verification on original bytes before rewrite should fail
        let dirty_check =
            verify_object_graph(&bytes, &request, &Limits::local(), &ToolCtx::new()).unwrap();
        assert_eq!(dirty_check, RedactionCheckStatus::Fail);
    }

    #[test]
    fn uninspected_carrier_inline_image_refused() {
        use ariad_core::tools::params::CropRect;
        use lopdf::Stream;
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let contents = doc.add_object(Stream::new(
            dictionary! {},
            b"BI /W 1 /H 1 /BPC 8 /CS /G ID \x00 EI".to_vec(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => contents,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page)],
                "Count" => 1,
            }),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let geometry = evidence(&bytes);
        let mut request = params();
        request.targets = vec![RedactionTarget::Rectangle {
            page: 1,
            rect: CropRect {
                left: 10.0,
                bottom: 10.0,
                right: 50.0,
                top: 50.0,
            },
        }];
        let plan = plan(
            &bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let err = rewrite_document(
            &bytes,
            &request,
            &plan,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("uninspected carrier refused: inline_image")
        );
    }

    #[test]
    fn uninspected_carrier_shading_refused() {
        use ariad_core::tools::params::CropRect;
        use lopdf::Stream;
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let contents = doc.add_object(Stream::new(dictionary! {}, b"/Sh1 sh".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => contents,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page)],
                "Count" => 1,
            }),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let geometry = evidence(&bytes);
        let mut request = params();
        request.targets = vec![RedactionTarget::Rectangle {
            page: 1,
            rect: CropRect {
                left: 10.0,
                bottom: 10.0,
                right: 50.0,
                top: 50.0,
            },
        }];
        let plan = plan(
            &bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let err = rewrite_document(
            &bytes,
            &request,
            &plan,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("uninspected carrier refused: shading")
        );
    }

    #[test]
    fn uninspected_carrier_vector_path_under_region_refused() {
        use ariad_core::tools::params::CropRect;
        use lopdf::Stream;
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let contents = doc.add_object(Stream::new(dictionary! {}, b"20 20 30 30 re f".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => contents,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page)],
                "Count" => 1,
            }),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let geometry = evidence(&bytes);
        let mut request = params();
        request.targets = vec![RedactionTarget::Rectangle {
            page: 1,
            rect: CropRect {
                left: 10.0,
                bottom: 10.0,
                right: 50.0,
                top: 50.0,
            },
        }];
        let plan = plan(
            &bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let err = rewrite_document(
            &bytes,
            &request,
            &plan,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("uninspected carrier refused: vector_path")
        );
    }
}
