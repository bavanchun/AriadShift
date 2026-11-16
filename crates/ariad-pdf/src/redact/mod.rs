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
use lopdf::{Document, ObjectId, Stream, content::Content};
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
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ariad_core::tools::params::redact_internal::RedactionCheckStatus, PdfError> {
    use ariad_core::tools::params::redact_internal::RedactionCheckStatus::{
        Fail, Pass, Unavailable,
    };
    if geometry.schema != "ariad-redact-geometry/1" {
        return Err(PdfError::Malformed);
    }
    // Evidence from another document must never vouch for these bytes: same error as `plan()`.
    if geometry.source_sha256 != crate::writer::sha256_chunks(&[bytes]) {
        return Err(PdfError::InvalidParams(
            "incomplete redaction geometry evidence".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    // Fail closed: a page the evidence does not cover would otherwise be skipped silently and
    // count as clean. The evidence must describe every page of these exact bytes, in order.
    let doc = crate::document::open_with_ctx(bytes, limits, ctx)?;
    let count = crate::tools::rotate::collect_page_ids(doc.inner(), ctx)?.len();
    if geometry.page_count != count
        || geometry.pages.len() != count
        || geometry
            .pages
            .iter()
            .enumerate()
            .any(|(index, page)| page.page as usize != index + 1)
    {
        return Err(PdfError::InvalidParams(
            "incomplete redaction geometry evidence".into(),
        ));
    }
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

/// Refusal raised when the image placement scan cannot account for every image on a page.
#[cfg(feature = "codecs")]
fn placement_scan_refused(reason: &str) -> PdfError {
    redaction_refused(&format!("image placement scan refused: {reason}"))
}

/// Safety refusal: the redaction cannot be carried out without risking a leak or damage.
///
/// Uses [`PdfError::NotImplemented`], the variant the uninspected-carrier refusal in
/// `flat.rs` already uses, so every redaction refusal keeps the same public contract
/// (`tool_missing`, exit 5 through `map_pdf_error`). The message is always prefixed
/// `redaction refused:`.
#[cfg(feature = "codecs")]
fn redaction_refused(reason: &str) -> PdfError {
    PdfError::NotImplemented(format!("redaction refused: {reason}"))
}

/// Reports whether a Flate-filtered stream inflates to a complete zlib stream.
///
/// The PDF reader tolerates corrupt or truncated Flate data and returns whatever it could
/// recover, which would let a scan see only part of the content. Only unfiltered streams and
/// a single Flate filter can be checked for completeness; any other filter, any filter chain
/// and an unreadable `/Filter` entry are refused.
#[cfg(feature = "codecs")]
fn flate_is_complete(stream: &Stream, ctx: &ToolCtx) -> Result<bool, PdfError> {
    if !stream.dict.has(b"Filter") {
        return Ok(true);
    }
    let Ok(filters) = stream.filters() else {
        return Ok(false);
    };
    if filters.is_empty() {
        return Ok(true);
    }
    if filters.len() != 1 || !(filters[0] == b"FlateDecode" || filters[0] == b"Fl") {
        return Ok(false);
    }
    let input = stream.content.as_slice();
    let mut inflater = flate2::Decompress::new(true);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        ctx.check()?;
        let (consumed, produced) = (inflater.total_in(), inflater.total_out());
        let offset = usize::try_from(consumed).map_err(|_| PdfError::Malformed)?;
        let status = inflater.decompress(
            input.get(offset..).ok_or(PdfError::Malformed)?,
            &mut buffer,
            flate2::FlushDecompress::Finish,
        );
        match status {
            Ok(flate2::Status::StreamEnd) => return Ok(true),
            Ok(_) if inflater.total_in() != consumed || inflater.total_out() != produced => {}
            _ => return Ok(false),
        }
    }
}

/// Lists every image XObject drawn directly by a page's content, with its placement matrix.
///
/// The scan fails closed: content that cannot be decoded or parsed, malformed transforms,
/// unresolvable or non-image XObjects and inline images are refused instead of being
/// reported as a page without images, so no caller can pass on an incomplete scan.
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
        _ => return Err(placement_scan_refused("page contents are not streams")),
    };
    let mut content_bytes = Vec::new();
    for obj in content_streams {
        ctx.check()?;
        let (_, derefed) = doc.dereference(obj).map_err(|_| PdfError::Malformed)?;
        let stream = derefed
            .as_stream()
            .map_err(|_| placement_scan_refused("page contents are not streams"))?;
        let decoded = budget
            .decode(stream)?
            .filter(|_| flate_is_complete(stream, ctx).unwrap_or(false))
            .ok_or_else(|| placement_scan_refused("undecodable page content"))?;
        content_bytes.extend_from_slice(&decoded);
        content_bytes.push(b'\n');
    }
    if content_bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(placements);
    }
    let parsed = Content::decode(&content_bytes)
        .map_err(|_| placement_scan_refused("unparsable page content"))?;
    let xobjects = match resources.get(b"XObject") {
        Ok(value) => Some(
            doc.dereference(value)
                .and_then(|(_, v)| v.as_dict())
                .map_err(|_| placement_scan_refused("malformed XObject resources"))?,
        ),
        Err(_) => None,
    };
    let mut ctm_stack = Vec::new();
    let mut ctm = Matrix::identity();
    for (index, op) in parsed.operations.iter().enumerate() {
        if index.is_multiple_of(1024) {
            ctx.check()?;
        }
        match op.operator.as_str() {
            "q" => ctm_stack.push(ctm),
            "Q" => {
                if let Some(prev) = ctm_stack.pop() {
                    ctm = prev;
                }
            }
            "cm" => {
                if op.operands.len() != 6 {
                    return Err(placement_scan_refused("malformed transform"));
                }
                let mut vals = [0.0; 6];
                for (slot, value) in vals.iter_mut().zip(&op.operands) {
                    *slot = match value {
                        Object::Integer(n) => *n as f64,
                        Object::Real(r) => f64::from(*r),
                        _ => return Err(placement_scan_refused("malformed transform")),
                    };
                }
                if !vals.iter().all(|x| x.is_finite()) {
                    return Err(placement_scan_refused("malformed transform"));
                }
                ctm = Matrix(vals).then(ctm);
            }
            "Do" => {
                let image_id = op
                    .operands
                    .first()
                    .and_then(|v| v.as_name().ok())
                    .and_then(|name| xobjects?.get(name).ok())
                    .and_then(|v| v.as_reference().ok())
                    .ok_or_else(|| placement_scan_refused("unresolvable XObject"))?;
                let is_image = doc
                    .get_object(image_id)
                    .and_then(Object::as_stream)
                    .map_err(|_| placement_scan_refused("unresolvable XObject"))?
                    .dict
                    .get(b"Subtype")
                    .is_ok_and(|s| matches!(s.as_name(), Ok(b"Image")));
                if !is_image {
                    return Err(placement_scan_refused("form XObject"));
                }
                placements.push((image_id, ctm));
            }
            "BI" => return Err(placement_scan_refused("inline image")),
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

/// Regions of `plan` on one-based page `page`; a whole-page region covers any page box.
#[cfg(feature = "codecs")]
fn regions_on_page(plan: &RedactionPlan, page: u32) -> Vec<[f64; 4]> {
    plan.regions()
        .iter()
        .filter_map(|r| match r {
            Region::Page { page: p } if *p == page => Some([0.0, 0.0, 100_000.0, 100_000.0]),
            Region::Rectangle { page: p, bounds } if *p == page => Some(*bounds),
            _ => None,
        })
        .collect()
}

/// The subset of `regions` overlapping the page-space bounding box of an image placement.
#[cfg(feature = "codecs")]
fn intersecting_regions(ctm: Matrix, regions: &[[f64; 4]]) -> Vec<[f64; 4]> {
    let bbox = image_placement_bbox(ctm);
    regions
        .iter()
        .copied()
        .filter(|r| bbox[0] < r[2] && bbox[2] > r[0] && bbox[1] < r[3] && bbox[3] > r[1])
        .collect()
}

/// Placements of one image on one page that overlap that page's redaction regions.
#[cfg(feature = "codecs")]
type PlacementGroups =
    std::collections::BTreeMap<(ObjectId, ObjectId), Vec<(Matrix, Vec<[f64; 4]>)>>;

/// Collects, for every page with regions, the image placements those regions touch.
#[cfg(feature = "codecs")]
fn targeted_placements(
    doc: &Document,
    plan: &RedactionPlan,
    budget: &mut crate::document::DecodeBudget,
    ctx: &ToolCtx,
) -> Result<PlacementGroups, PdfError> {
    let page_ids = collect_page_ids(doc, ctx)?;
    let mut groups = PlacementGroups::new();
    for (page_idx, page_id) in page_ids.iter().enumerate() {
        let regions = regions_on_page(plan, page_idx as u32 + 1);
        if regions.is_empty() {
            continue;
        }
        for (image_id, ctm) in collect_page_image_placements(doc, *page_id, budget, ctx)? {
            let intersecting = intersecting_regions(ctm, &regions);
            if !intersecting.is_empty() {
                groups
                    .entry((*page_id, image_id))
                    .or_default()
                    .push((ctm, intersecting));
            }
        }
    }
    Ok(groups)
}

/// Decoded images shared by several page rewrites, each decoded once under one budget.
#[cfg(feature = "codecs")]
struct ImageCache {
    images: std::collections::BTreeMap<ObjectId, crate::images::DecodedImage>,
    /// Page rewrites still needing each image; the last one takes the cached pixels.
    pending: std::collections::BTreeMap<ObjectId, usize>,
}

#[cfg(feature = "codecs")]
impl ImageCache {
    /// Returns a private, mutable copy of the image `id`, charging copies to `budget`.
    fn working_copy(
        &mut self,
        doc: &crate::PdfDoc,
        id: ObjectId,
        budget: &mut crate::document::DecodeBudget,
        limits: &Limits,
        ctx: &ToolCtx,
    ) -> Result<crate::images::DecodedImage, PdfError> {
        let pending = self.pending.get_mut(&id).ok_or(PdfError::Malformed)?;
        *pending = pending.checked_sub(1).ok_or(PdfError::Malformed)?;
        let last = *pending == 0;
        if let Some(cached) = self.images.get(&id) {
            if last {
                return self.images.remove(&id).ok_or(PdfError::Malformed);
            }
            let copy = cached.clone();
            budget.charge(copy.pixels.len())?;
            return Ok(copy);
        }
        let decoded = crate::images::decode_with_budget(
            doc,
            id,
            ctx,
            budget,
            limits.max_nesting_depth,
            false,
            false,
        )
        .map_err(|error| match error {
            crate::images::ImageError::Pdf(error @ PdfError::LimitExceeded(_)) => error,
            _ => PdfError::NotImplemented("image decode for redaction".into()),
        })?;
        if !last {
            budget.charge(decoded.pixels.len())?;
            self.images.insert(id, decoded.clone());
        }
        Ok(decoded)
    }
}

/// A cleared private copy of one image for one page, with freshly encoded masks.
#[cfg(feature = "codecs")]
struct ImageRewrite {
    page_id: ObjectId,
    image_id: ObjectId,
    /// The original soft mask object of `image_id`.
    original_soft_mask: Option<ObjectId>,
    image: Stream,
    soft_mask: Option<Stream>,
}

/// Clears image pixels beneath redaction regions.
///
/// Images with an explicit `/Mask` (stencil mask or colour key) are refused: re-encoding one
/// would need a valid 1-bit stencil, which is not produced. Each targeted page receives its
/// own cleared copy of an image and its soft mask, and only that
/// page's resources are repointed to it, so a shared image keeps its original pixels wherever
/// it is not redacted. Each image is decoded once, and decoding, copying and encoding draw
/// from one cumulative budget, as does pixel selection.
#[cfg(feature = "codecs")]
fn rewrite_image_pixels(
    doc: &mut crate::PdfDoc,
    plan: &RedactionPlan,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<usize, PdfError> {
    let mut budget = doc.decode_budget();
    let groups = targeted_placements(doc.inner(), plan, &mut budget, ctx)?;
    let mut cache = ImageCache {
        images: std::collections::BTreeMap::new(),
        pending: std::collections::BTreeMap::new(),
    };
    let mut masks = Vec::with_capacity(groups.len());
    for (_, image_id) in groups.keys() {
        let dict = &doc
            .inner()
            .get_object(*image_id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?
            .dict;
        if dict.has(b"Mask") {
            return Err(redaction_refused("image with an explicit /Mask"));
        }
        let soft = dict.get(b"SMask").ok().and_then(|v| v.as_reference().ok());
        for id in [Some(*image_id), soft].into_iter().flatten() {
            *cache.pending.entry(id).or_default() += 1;
        }
        masks.push(soft);
    }
    let mut work = 0u64;
    let mut rewrites = Vec::new();
    for (((page_id, image_id), placements), smask_id) in groups.into_iter().zip(masks) {
        ctx.check()?;
        let mut image = cache.working_copy(doc, image_id, &mut budget, limits, ctx)?;
        let mut soft_mask = smask_id
            .map(|id| cache.working_copy(doc, id, &mut budget, limits, ctx))
            .transpose()?;
        let mut cleared = 0usize;
        for (ctm, regions) in &placements {
            cleared = cleared.saturating_add(pixels::clear_pixels_with_work(
                &mut image,
                soft_mask.as_mut(),
                None,
                *ctm,
                regions,
                limits,
                ctx,
                &mut work,
            )?);
        }
        if cleared == 0 {
            continue;
        }
        let mut encode = |pixels: &crate::images::DecodedImage| {
            let stream = pixels::encode_lossless(pixels, ctx)?;
            budget.charge(stream.content.len())?;
            Ok::<_, PdfError>(stream)
        };
        rewrites.push(ImageRewrite {
            page_id,
            image_id,
            original_soft_mask: smask_id,
            soft_mask: soft_mask.as_ref().map(&mut encode).transpose()?,
            image: encode(&image)?,
        });
    }
    let modified = rewrites.len();
    let drawers = other_drawers(doc.inner(), &rewrites, doc.decode_budget(), ctx)?;
    let mut rewrite_counts = std::collections::BTreeMap::<ObjectId, usize>::new();
    for rewrite in &rewrites {
        *rewrite_counts.entry(rewrite.image_id).or_default() += 1;
    }
    let inner = doc.inner_mut()?;
    let mut originals: Vec<(Option<ObjectId>, Option<ObjectId>)> = Vec::new();
    let mut replacements = BTreeSet::new();
    for rewrite in rewrites {
        ctx.check()?;
        let mut image = rewrite.image;
        if let Some(stream) = rewrite.soft_mask {
            let id = inner.add_object(stream);
            replacements.insert(id);
            image.dict.set("SMask", Object::Reference(id));
        }
        let in_place = rewrite_counts.get(&rewrite.image_id) == Some(&1)
            && !drawers.contains(&rewrite.image_id);
        if in_place {
            // Nothing else draws the image, so its original pixels are overwritten in place.
            // The object now carries the new mask, so it is a replacement; its original mask
            // must become unreachable unless another stream still references it.
            replacements.insert(rewrite.image_id);
            inner
                .objects
                .insert(rewrite.image_id, Object::Stream(image));
            originals.push((None, rewrite.original_soft_mask));
        } else {
            // The original stays as another page's image and legitimately keeps its own mask,
            // so it is deliberately not a replacement.
            let copy_id = inner.add_object(image);
            replacements.insert(copy_id);
            repoint_page_image(inner, rewrite.page_id, rewrite.image_id, copy_id)?;
            originals.push((Some(rewrite.image_id), rewrite.original_soft_mask));
        }
    }
    ensure_originals_unreachable(inner, &originals, &replacements, &drawers, ctx)?;
    Ok(modified)
}

/// Refusal raised when a rewritten image's original object would still be saved.
#[cfg(feature = "codecs")]
fn original_image_retained() -> PdfError {
    redaction_refused("original image remains reachable")
}

/// Finds rewritten images that another page draws, and refuses when that cannot be decided.
///
/// A page that names a rewritten image directly in its effective `/XObject` resources is
/// scanned: it counts as a drawer only when its content really draws the image, and the
/// original is kept for it. A page that cannot be scanned (form XObject, inline image,
/// non-Flate or truncated content, exhausted budget, any scan error) is refused, as is any
/// route to a rewritten image other than a page's own direct resource entry, namely form
/// XObject resources, patterns, annotation appearance streams or any other reachable
/// object. The original is never kept on uncertainty.
#[cfg(feature = "codecs")]
fn other_drawers(
    doc: &Document,
    rewrites: &[ImageRewrite],
    mut budget: crate::document::DecodeBudget,
    ctx: &ToolCtx,
) -> Result<BTreeSet<ObjectId>, PdfError> {
    let mut drawers = BTreeSet::new();
    if rewrites.is_empty() {
        return Ok(drawers);
    }
    let watched: BTreeSet<ObjectId> = rewrites.iter().map(|r| r.image_id).collect();
    let mut walk = ReachWalk::default();
    for page_id in collect_page_ids(doc, ctx)? {
        ctx.check()?;
        let named = walk.page(doc, page_id, &watched, ctx)?;
        let candidates: BTreeSet<ObjectId> = named
            .into_iter()
            .filter(|id| {
                !rewrites
                    .iter()
                    .any(|r| r.image_id == *id && r.page_id == page_id)
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }
        match collect_page_image_placements(doc, page_id, &mut budget, ctx) {
            Ok(placements) => {
                drawers.extend(
                    placements
                        .iter()
                        .map(|(id, _)| *id)
                        .filter(|id| candidates.contains(id)),
                );
            }
            Err(_) => {
                ctx.check()?;
                // The reason stays a fixed string: it can reach HTTP clients.
                return Err(redaction_refused(
                    "another page naming a redacted image cannot be scanned",
                ));
            }
        }
    }
    Ok(drawers)
}

/// Object-graph walk shared by all pages, looking for rewritten images reached indirectly.
#[cfg(feature = "codecs")]
#[derive(Default)]
struct ReachWalk {
    visited: std::collections::HashSet<ObjectId>,
    steps: usize,
}

#[cfg(feature = "codecs")]
impl ReachWalk {
    /// Returns the watched images named directly in the page's effective `/XObject`
    /// resources, and refuses if a watched image is reachable from the page's other
    /// resources or annotations.
    ///
    /// Page tree nodes and page objects met on the way (annotation destinations) are not
    /// entered (so `/P` and `/Parent` links to them go nowhere). Reaching a watched image
    /// already refuses, so the visited set is shared across pages.
    fn page(
        &mut self,
        doc: &Document,
        page_id: ObjectId,
        watched: &BTreeSet<ObjectId>,
        ctx: &ToolCtx,
    ) -> Result<BTreeSet<ObjectId>, PdfError> {
        let unscannable = || redaction_refused("page resources cannot be scanned for images");
        let resources = crate::xobject::resolve_page_resources(doc, page_id);
        let mut named = BTreeSet::new();
        let mut stack: Vec<&Object> = Vec::new();
        for (key, value) in resources.iter() {
            if key != b"XObject" {
                stack.push(value);
                continue;
            }
            let (_, entries) = doc.dereference(value).map_err(|_| unscannable())?;
            let entries = entries.as_dict().map_err(|_| unscannable())?;
            for (_, entry) in entries.iter() {
                match entry.as_reference() {
                    Ok(id) if watched.contains(&id) => {
                        named.insert(id);
                    }
                    _ => stack.push(entry),
                }
            }
        }
        if let Ok(annots) = doc.get_dictionary(page_id).and_then(|d| d.get(b"Annots")) {
            stack.push(annots);
        }
        let mut pending: Vec<ObjectId> = Vec::new();
        while !stack.is_empty() || !pending.is_empty() {
            self.steps = self.steps.wrapping_add(1);
            if self.steps.is_multiple_of(128) {
                ctx.check()?;
            }
            if let Some(object) = stack.pop() {
                match object {
                    Object::Reference(id) => pending.push(*id),
                    Object::Array(items) => stack.extend(items.iter()),
                    Object::Dictionary(dict) => Self::push_dict(dict, &mut stack),
                    Object::Stream(stream) => Self::push_dict(&stream.dict, &mut stack),
                    _ => {}
                }
                continue;
            }
            let Some(id) = pending.pop() else { break };
            if watched.contains(&id) {
                return Err(redaction_refused(
                    "a redacted image is also reachable through a form XObject, annotation or other resource",
                ));
            }
            if !self.visited.insert(id) {
                continue;
            }
            let Some(object) = doc.objects.get(&id) else {
                continue;
            };
            let is_page_node = object.as_dict().ok().is_some_and(|d| {
                d.get(b"Type")
                    .is_ok_and(|t| matches!(t.as_name(), Ok(b"Page" | b"Pages")))
            });
            if !is_page_node {
                stack.push(object);
            }
        }
        Ok(named)
    }

    fn push_dict<'a>(dict: &'a lopdf::Dictionary, stack: &mut Vec<&'a Object>) {
        // No key is skipped: `/P` and `/Parent` may be resource names, and links to page tree
        // nodes are already cut by the page-type check in `page`.
        stack.extend(dict.iter().map(|(_, value)| value));
    }
}

/// Fails closed when an original image, or a mask only it used, would survive in the output.
///
/// Reachability is computed as the writer's garbage collection does. An original image may
/// remain only when another page really draws it, and an original mask only while another
/// reachable stream still references it.
#[cfg(feature = "codecs")]
fn ensure_originals_unreachable(
    doc: &Document,
    originals: &[(Option<ObjectId>, Option<ObjectId>)],
    replacements: &BTreeSet<ObjectId>,
    drawers: &std::collections::BTreeSet<ObjectId>,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    use std::collections::{HashSet, VecDeque};
    if originals.is_empty() {
        return Ok(());
    }
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    for (key, value) in doc.trailer.iter() {
        if key != b"Prev" && key != b"Encrypt" && key != b"XRefStm" {
            crate::gc::collect_references_in_object(value, &mut reachable, &mut queue);
        }
    }
    let mut steps = 0usize;
    while let Some(id) = queue.pop_front() {
        steps = steps.wrapping_add(1);
        if steps.is_multiple_of(128) {
            ctx.check()?;
        }
        if let Some(object) = doc.objects.get(&id) {
            crate::gc::collect_references_in_object(object, &mut reachable, &mut queue);
        }
    }
    for (image_id, soft_mask) in originals {
        if let Some(image_id) = image_id
            && reachable.contains(image_id)
            && !drawers.contains(image_id)
        {
            return Err(original_image_retained());
        }
        for mask in soft_mask.iter() {
            if !reachable.contains(mask) {
                continue;
            }
            let shared = reachable.iter().any(|id| {
                !replacements.contains(id)
                    && doc
                        .objects
                        .get(id)
                        .and_then(|o| o.as_stream().ok())
                        .is_some_and(|stream| {
                            [b"SMask".as_slice(), b"Mask"].iter().any(|key| {
                                stream
                                    .dict
                                    .get(key)
                                    .and_then(Object::as_reference)
                                    .is_ok_and(|r| r == *mask)
                            })
                        })
            });
            if !shared {
                return Err(original_image_retained());
            }
        }
    }
    Ok(())
}

/// Gives a page its own resource dictionary whose image entries for `from` point to `to`.
///
/// The page's effective (inherited and merged) resources are copied into the page, so sibling
/// pages sharing a resource dictionary or inheriting it from the page tree are not affected.
#[cfg(feature = "codecs")]
fn repoint_page_image(
    doc: &mut Document,
    page_id: ObjectId,
    from: ObjectId,
    to: ObjectId,
) -> Result<(), PdfError> {
    let mut resources = crate::xobject::resolve_page_resources(doc, page_id);
    let mut xobjects = resources
        .get(b"XObject")
        .and_then(|v| doc.dereference(v))
        .and_then(|(_, v)| v.as_dict())
        .cloned()
        .map_err(|_| PdfError::Malformed)?;
    let mut repointed = false;
    for (_, value) in xobjects.iter_mut() {
        if value.as_reference().is_ok_and(|id| id == from) {
            *value = Object::Reference(to);
            repointed = true;
        }
    }
    if !repointed {
        return Err(PdfError::Malformed);
    }
    resources.set("XObject", Object::Dictionary(xobjects));
    doc.get_object_mut(page_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| PdfError::Malformed)?
        .set("Resources", Object::Dictionary(resources));
    Ok(())
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
    let mut budget = doc.decode_budget();
    let mut image_placements = std::collections::BTreeMap::<ObjectId, Vec<_>>::new();
    for ((_, image_id), placements) in targeted_placements(doc.inner(), plan, &mut budget, ctx)? {
        image_placements
            .entry(image_id)
            .or_default()
            .extend(placements);
    }
    // One decode budget and one pixel work counter bound the whole check.
    let mut work = 0u64;
    let decode = |id: ObjectId, budget: &mut crate::document::DecodeBudget| {
        crate::images::decode_with_budget(
            &doc,
            id,
            ctx,
            budget,
            limits.max_nesting_depth,
            false,
            false,
        )
    };
    for (img_id, placements) in image_placements {
        ctx.check()?;
        let decoded = match decode(img_id, &mut budget) {
            Ok(img) => img,
            Err(crate::images::ImageError::Pdf(error @ PdfError::LimitExceeded(_))) => {
                return Err(error);
            }
            Err(_) => return Ok(Fail),
        };
        // The soft mask and an explicit `/Mask` stream are verified alike; an undecodable
        // mask fails the check.
        let mut mask_images = Vec::new();
        for key in [b"SMask".as_slice(), b"Mask"] {
            if let Ok(mask_id) = doc
                .inner()
                .get_object(img_id)
                .and_then(Object::as_stream)
                .and_then(|s| s.dict.get(key))
                .and_then(Object::as_reference)
            {
                match decode(mask_id, &mut budget) {
                    Ok(img) => mask_images.push(img),
                    Err(crate::images::ImageError::Pdf(error @ PdfError::LimitExceeded(_))) => {
                        return Err(error);
                    }
                    Err(_) => return Ok(Fail),
                }
            }
        }
        let channels = match decoded.color_space {
            crate::images::ColorSpace::Gray => 1usize,
            crate::images::ColorSpace::Rgb => 3,
            crate::images::ColorSpace::Cmyk => 4,
        };
        let stride = channels * usize::from(decoded.bits_per_component / 8);
        for (ctm, regions) in placements {
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
            for smask in &mask_images {
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

    /// Geometry for `pages` blank 100x100 pages with no text.
    fn blank_geometry(bytes: &[u8], pages: u32) -> RedactGeometry {
        RedactGeometry {
            schema: "ariad-redact-geometry/1".into(),
            source_sha256: crate::writer::sha256_chunks(&[bytes]),
            page_count: pages as usize,
            pages: (1..=pages)
                .map(|page| RedactGeometryPage {
                    page,
                    width: 100.0,
                    height: 100.0,
                    rotation: 0,
                    chars: Vec::new(),
                })
                .collect(),
        }
    }

    fn page_target_params(target: RedactionTarget) -> RedactParams {
        RedactParams {
            targets: vec![target],
            ..params()
        }
    }

    #[test]
    fn pdfium_text_check_refuses_incomplete_geometry_evidence() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        let bytes = source(false);
        let ctx = ToolCtx::new();
        let limits = Limits::local();
        let spatial = [
            page_target_params(RedactionTarget::Page { page: 1 }),
            page_target_params(rectangle(1, [0.0, 0.0, 50.0, 50.0])),
            page_target_params(RedactionTarget::Exact { text: "zzz".into() }),
        ];
        // The matching evidence is accepted, so each refusal below comes from the mutation.
        for request in &spatial {
            let ok = verify_pdfium_text(&bytes, request, &blank_geometry(&bytes, 1), &limits, &ctx);
            assert_eq!(ok.unwrap(), RedactionCheckStatus::Pass);
        }
        type Mutation = fn(&mut RedactGeometry);
        let mutations: [(&str, Mutation); 4] = [
            ("page missing", |g| g.pages.clear()),
            ("page_count too large", |g| g.page_count = 2),
            ("page_count too small", |g| g.page_count = 0),
            ("wrong page number", |g| g.pages[0].page = 2),
        ];
        for (name, mutate) in mutations {
            for request in &spatial {
                let mut geometry = blank_geometry(&bytes, 1);
                mutate(&mut geometry);
                let result = verify_pdfium_text(&bytes, request, &geometry, &limits, &ctx);
                assert!(
                    matches!(result, Err(PdfError::InvalidParams(_))),
                    "{name} must be refused, got {result:?}"
                );
            }
        }
    }

    #[test]
    fn pdfium_text_check_refuses_evidence_from_another_pdf() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        let pdf_a = source(false);
        let mut pdf_b = pdf_a.clone();
        pdf_b.extend_from_slice(b"\n%different bytes, same page count\n");
        assert_ne!(pdf_a, pdf_b);
        let ctx = ToolCtx::new();
        let limits = Limits::local();
        let request = page_target_params(RedactionTarget::Page { page: 1 });
        let evidence_a = blank_geometry(&pdf_a, 1);
        let ok = verify_pdfium_text(&pdf_a, &request, &evidence_a, &limits, &ctx);
        assert_eq!(ok.unwrap(), RedactionCheckStatus::Pass);
        let result = verify_pdfium_text(&pdf_b, &request, &evidence_a, &limits, &ctx);
        assert!(
            matches!(result, Err(PdfError::InvalidParams(_))),
            "evidence for another PDF must be refused, got {result:?}"
        );
    }

    fn rectangle(page: u32, rect: [f64; 4]) -> RedactionTarget {
        RedactionTarget::Rectangle {
            page,
            rect: ariad_core::tools::params::CropRect {
                left: rect[0],
                bottom: rect[1],
                right: rect[2],
                top: rect[3],
            },
        }
    }

    /// Adds an uncompressed 8-bit grey image XObject filled with `value`.
    fn gray_image(doc: &mut Document, width: u32, height: u32, value: u8) -> lopdf::ObjectId {
        doc.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => i64::from(width),
                "Height" => i64::from(height),
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
            },
            vec![value; (width * height) as usize],
        ))
    }

    /// Serializes a document whose page tree is `pages` with the given leaf pages.
    #[cfg(feature = "codecs")]
    fn finish(mut doc: Document, pages: lopdf::ObjectId, kids: &[lopdf::ObjectId]) -> Vec<u8> {
        finish_with(&mut doc, pages, kids, lopdf::Dictionary::new())
    }

    /// Like [`finish`], adding `inherited` entries such as `Resources` to the page tree node.
    fn finish_with(
        doc: &mut Document,
        pages: lopdf::ObjectId,
        kids: &[lopdf::ObjectId],
        inherited: lopdf::Dictionary,
    ) -> Vec<u8> {
        let mut node = dictionary! {
            "Type" => "Pages",
            "Kids" => kids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => kids.len() as i64,
        };
        node.extend(&inherited);
        doc.objects.insert(pages, Object::Dictionary(node));
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    fn rect_plan(
        bytes: &[u8],
        pages: u32,
        targets: Vec<RedactionTarget>,
    ) -> (RedactParams, RedactionPlan, RedactGeometry) {
        let geometry = blank_geometry(bytes, pages);
        let mut request = params();
        request.targets = targets;
        let planned = plan(
            bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        (request, planned, geometry)
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn image_pixel_check_refuses_undecodable_page_content() {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(b"q 100 0 0 100 0 0 cm /Im0 Do Q")
            .unwrap();
        let mut truncated = encoder.finish().unwrap();
        truncated.truncate(truncated.len() - 6);
        for encoded in [b"not a deflate stream".to_vec(), truncated] {
            let mut doc = Document::with_version("1.7");
            let pages = doc.new_object_id();
            let image = gray_image(&mut doc, 2, 2, 200);
            let contents = doc.add_object(lopdf::Stream::new(
                dictionary! { "Filter" => "FlateDecode" },
                encoded,
            ));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
                "Contents" => contents,
            });
            let bytes = finish(doc, pages, &[page]);
            let (_, planned, _) = rect_plan(&bytes, 1, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
            let err = verify_image_pixels(&bytes, &planned, &Limits::local(), &ToolCtx::new())
                .unwrap_err();
            assert!(
                err.to_string().contains("undecodable page content"),
                "{err}"
            );
        }
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn image_pixel_check_refuses_form_xobjects_and_inline_images() {
        for content in [
            &b"/Fm0 Do"[..],
            b"BI /W 1 /H 1 /BPC 8 /CS /G ID \x00 EI",
            b"/Missing Do",
        ] {
            let mut doc = Document::with_version("1.7");
            let pages = doc.new_object_id();
            let form = doc.add_object(lopdf::Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                },
                b"".to_vec(),
            ));
            let contents = doc.add_object(lopdf::Stream::new(dictionary! {}, content.to_vec()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Fm0" => form } },
                "Contents" => contents,
            });
            let bytes = finish(doc, pages, &[page]);
            let (_, planned, _) = rect_plan(&bytes, 1, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
            let err = verify_image_pixels(&bytes, &planned, &Limits::local(), &ToolCtx::new())
                .unwrap_err();
            assert!(
                err.to_string().contains("image placement scan refused"),
                "{err}"
            );
        }
    }

    /// The image object a page draws under the resource name `Im0`.
    #[cfg(feature = "codecs")]
    fn page_image(pdf: &crate::PdfDoc, page: lopdf::ObjectId) -> lopdf::ObjectId {
        let resources = crate::xobject::resolve_page_resources(pdf.inner(), page);
        let (_, xobjects) = pdf
            .inner()
            .dereference(resources.get(b"XObject").unwrap())
            .unwrap();
        xobjects
            .as_dict()
            .unwrap()
            .get(b"Im0")
            .unwrap()
            .as_reference()
            .unwrap()
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn redacting_one_page_leaves_a_shared_image_intact_on_other_pages() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        for inherited in [false, true] {
            let mut doc = Document::with_version("1.7");
            let pages = doc.new_object_id();
            let image = gray_image(&mut doc, 4, 4, 200);
            let resources =
                doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => image } });
            let mut kids = Vec::new();
            for _ in 0..2 {
                let contents = doc.add_object(lopdf::Stream::new(
                    dictionary! {},
                    b"q 100 0 0 100 0 0 cm /Im0 Do Q".to_vec(),
                ));
                let mut page = dictionary! {
                    "Type" => "Page",
                    "Parent" => pages,
                    "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                    "Contents" => contents,
                };
                if !inherited {
                    page.set("Resources", resources);
                }
                kids.push(doc.add_object(page));
            }
            let tree = if inherited {
                dictionary! { "Resources" => resources }
            } else {
                lopdf::Dictionary::new()
            };
            let bytes = finish_with(&mut doc, pages, &kids, tree);
            let (request, planned, geometry) =
                rect_plan(&bytes, 2, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
            let output = rewrite_document(
                &bytes,
                &request,
                &planned,
                &geometry,
                &Limits::local(),
                &ToolCtx::new(),
            )
            .unwrap();
            assert_eq!(
                verify_image_pixels(&output, &planned, &Limits::local(), &ToolCtx::new()).unwrap(),
                RedactionCheckStatus::Pass
            );
            let pdf = crate::document::open(&output, &Limits::local()).unwrap();
            let page_ids = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap();
            let (redacted, untouched) =
                (page_image(&pdf, page_ids[0]), page_image(&pdf, page_ids[1]));
            assert_ne!(redacted, untouched);
            let pixels = |id| {
                crate::images::decode(&pdf, id, &ToolCtx::new(), &Limits::local())
                    .unwrap()
                    .pixels
            };
            let redacted = pixels(redacted);
            assert!(redacted.contains(&0) && redacted.contains(&200));
            assert!(pixels(untouched).iter().all(|&p| p == 200));
        }
    }

    /// One page drawing each listed image at the given translations, scaled to 50x50.
    #[cfg(feature = "codecs")]
    fn placed_images(images: &[(u8, &[(i32, i32)])]) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let mut xobjects = lopdf::Dictionary::new();
        let mut content = String::new();
        for (index, (value, offsets)) in images.iter().enumerate() {
            let name = format!("Im{index}");
            xobjects.set(name.as_bytes(), gray_image(&mut doc, 64, 64, *value));
            for (x, y) in *offsets {
                content.push_str(&format!("q 50 0 0 50 {x} {y} cm /{name} Do Q\n"));
            }
        }
        let contents = doc.add_object(lopdf::Stream::new(dictionary! {}, content.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => dictionary! { "XObject" => xobjects },
            "Contents" => contents,
        });
        finish(doc, pages, &[page])
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn image_rewrite_decodes_each_image_once_under_one_budget() {
        let mut limits = Limits::local();
        // One 64x64 grey image costs about 8 KiB to decode (its inflated stream and its
        // unpacked pixels); this leaves room for one decode and its re-encoding, not two.
        limits.max_decompressed_bytes = 12_000;
        let rewrite = |bytes: &[u8]| {
            let (_, planned, _) = rect_plan(bytes, 1, vec![rectangle(1, [0.0, 0.0, 90.0, 90.0])]);
            let mut pdf = crate::document::open(bytes, &limits).unwrap();
            rewrite_image_pixels(&mut pdf, &planned, &limits, &ToolCtx::new())
        };
        let repeated = placed_images(&[(200, &[(0, 0), (10, 10), (20, 20)])]);
        assert_eq!(rewrite(&repeated).unwrap(), 1);
        let distinct = placed_images(&[(200, &[(0, 0)]), (100, &[(20, 20)])]);
        assert!(matches!(
            rewrite(&distinct).unwrap_err(),
            PdfError::LimitExceeded(_)
        ));
    }

    /// A one-page document drawing `content` with `resources`, plus extra page entries.
    fn single_page(
        doc: &mut Document,
        content: &[u8],
        resources: lopdf::Dictionary,
        extra: lopdf::Dictionary,
    ) -> Vec<u8> {
        let pages = doc.new_object_id();
        let contents = doc.add_object(lopdf::Stream::new(dictionary! {}, content.to_vec()));
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => resources,
            "Contents" => contents,
        };
        page.extend(&extra);
        let page = doc.add_object(page);
        finish_with(doc, pages, &[page], lopdf::Dictionary::new())
    }

    fn rewrite_rect(bytes: &[u8], overlay: Option<&str>) -> Result<Vec<u8>, PdfError> {
        let (mut request, _, _) = rect_plan(bytes, 1, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
        request.overlay_text = overlay.map(str::to_owned);
        let geometry = blank_geometry(bytes, 1);
        let planned = plan(
            bytes,
            &request,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )?;
        rewrite_document(
            bytes,
            &request,
            &planned,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
    }

    #[test]
    fn redacted_pages_lose_their_thumbnails() {
        let mut doc = Document::with_version("1.7");
        let thumb = gray_image(&mut doc, 2, 2, 7);
        let bytes = single_page(
            &mut doc,
            b"q Q",
            lopdf::Dictionary::new(),
            dictionary! { "Thumb" => thumb },
        );
        let output = rewrite_rect(&bytes, None).unwrap();
        let pdf = crate::document::open(&output, &Limits::local()).unwrap();
        let page = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap()[0];
        assert!(!pdf.inner().get_dictionary(page).unwrap().has(b"Thumb"));
    }

    #[test]
    fn rewrite_refuses_form_xobjects_and_extgstate_fonts() {
        let mut doc = Document::with_version("1.7");
        let form = doc.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            },
            Vec::new(),
        ));
        let bytes = single_page(
            &mut doc,
            b"/Fm0 Do",
            dictionary! { "XObject" => dictionary! { "Fm0" => form } },
            lopdf::Dictionary::new(),
        );
        let err = rewrite_rect(&bytes, None).unwrap_err();
        assert!(err.to_string().contains("form_xobject"), "{err}");

        let mut doc = Document::with_version("1.7");
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let bytes = single_page(
            &mut doc,
            b"/GS0 gs",
            dictionary! {
                "ExtGState" => dictionary! {
                    "GS0" => dictionary! { "Font" => vec![Object::Reference(font), 12.into()] },
                },
            },
            lopdf::Dictionary::new(),
        );
        let err = rewrite_rect(&bytes, None).unwrap_err();
        assert!(err.to_string().contains("ext_gstate_font"), "{err}");
    }

    #[test]
    fn object_graph_check_fails_on_undecodable_streams_and_utf16_xmp() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        let mut request = params();
        request.targets = vec![RedactionTarget::Exact {
            text: "secret".into(),
        }];
        let utf16: Vec<u8> = "<x:xmpmeta>secret</x:xmpmeta>"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        for (dict, content) in [
            (
                dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
                utf16,
            ),
            (dictionary! { "Filter" => "JPXDecode" }, b"opaque".to_vec()),
        ] {
            let mut doc = Document::with_version("1.7");
            let carrier = doc.add_object(lopdf::Stream::new(dict, content));
            let bytes = single_page(
                &mut doc,
                b"q Q",
                lopdf::Dictionary::new(),
                dictionary! { "Metadata" => carrier },
            );
            assert_eq!(
                verify_object_graph(&bytes, &request, &Limits::local(), &ToolCtx::new()).unwrap(),
                RedactionCheckStatus::Fail
            );
        }
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn overlay_on_inherited_resources_keeps_images_and_adds_its_font() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = gray_image(&mut doc, 4, 4, 200);
        let contents = doc.add_object(lopdf::Stream::new(
            dictionary! {},
            b"q 100 0 0 100 0 0 cm /Im0 Do Q".to_vec(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => contents,
        });
        let bytes = finish_with(
            &mut doc,
            pages,
            &[page],
            dictionary! { "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } } },
        );
        let output = rewrite_rect(&bytes, Some("REDACTED")).unwrap();
        let pdf = crate::document::open(&output, &Limits::local()).unwrap();
        let page = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap()[0];
        let resources = crate::xobject::resolve_page_resources(pdf.inner(), page);
        let (_, fonts) = pdf
            .inner()
            .dereference(resources.get(b"Font").unwrap())
            .unwrap();
        assert!(fonts.as_dict().unwrap().has(b"AriadRedactFont"));
        let image = crate::images::decode(
            &pdf,
            page_image(&pdf, page),
            &ToolCtx::new(),
            &Limits::local(),
        )
        .unwrap();
        assert!(image.pixels.contains(&0) && image.pixels.contains(&200));
        let (_, planned, _) = rect_plan(&bytes, 1, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
        assert_eq!(
            verify_image_pixels(&output, &planned, &Limits::local(), &ToolCtx::new()).unwrap(),
            RedactionCheckStatus::Pass
        );
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn soft_mask_is_made_opaque_beneath_the_region() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        let mut doc = Document::with_version("1.7");
        let mask = gray_image(&mut doc, 4, 4, 128);
        let image = gray_image(&mut doc, 4, 4, 200);
        doc.get_object_mut(image)
            .and_then(Object::as_stream_mut)
            .unwrap()
            .dict
            .set("SMask", mask);
        let bytes = single_page(
            &mut doc,
            b"q 100 0 0 100 0 0 cm /Im0 Do Q",
            dictionary! { "XObject" => dictionary! { "Im0" => image } },
            lopdf::Dictionary::new(),
        );
        let output = rewrite_rect(&bytes, None).unwrap();
        let (_, planned, _) = rect_plan(&bytes, 1, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
        assert_eq!(
            verify_image_pixels(&output, &planned, &Limits::local(), &ToolCtx::new()).unwrap(),
            RedactionCheckStatus::Pass
        );
        let pdf = crate::document::open(&output, &Limits::local()).unwrap();
        let page = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap()[0];
        let smask = pdf
            .inner()
            .get_object(page_image(&pdf, page))
            .and_then(Object::as_stream)
            .unwrap()
            .dict
            .get(b"SMask")
            .and_then(Object::as_reference)
            .unwrap();
        let smask = crate::images::decode(&pdf, smask, &ToolCtx::new(), &Limits::local()).unwrap();
        assert!(smask.pixels.contains(&255) && smask.pixels.contains(&128));
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

    /// Distinct 4x4 grey samples, so the original pixels are recognizable in any output.
    #[cfg(feature = "codecs")]
    const MARKER: [u8; 16] = [
        0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x17, 0x28, 0x39, 0x4A, 0x5B, 0x6C, 0x7D, 0x8E, 0x9F,
        0xA0,
    ];

    #[cfg(feature = "codecs")]
    fn marker_image(doc: &mut Document) -> lopdf::ObjectId {
        doc.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 4,
                "Height" => 4,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
            },
            MARKER.to_vec(),
        ))
    }

    /// Asserts that no byte of the saved file, and no decoded stream, holds the original pixels.
    #[cfg(feature = "codecs")]
    fn assert_original_pixels_absent(output: &[u8]) {
        assert!(
            !output.windows(MARKER.len()).any(|w| w == MARKER),
            "original pixels are present in the serialized bytes"
        );
        let parsed = Document::load_mem(output).unwrap();
        for object in parsed.objects.values() {
            if let Object::Stream(stream) = object {
                let decoded = stream
                    .decompressed_content()
                    .unwrap_or_else(|_| stream.content.clone());
                assert!(
                    !decoded.windows(MARKER.len()).any(|w| w == MARKER),
                    "original pixels survive in a decoded stream"
                );
            }
        }
    }

    #[cfg(feature = "codecs")]
    fn redact_pages(bytes: &[u8], pages: u32) -> Result<Vec<u8>, PdfError> {
        let (request, planned, geometry) =
            rect_plan(bytes, pages, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
        rewrite_document(
            bytes,
            &request,
            &planned,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
    }

    #[cfg(feature = "codecs")]
    fn drawing_page(doc: &mut Document, pages: lopdf::ObjectId, content: &[u8]) -> lopdf::ObjectId {
        let contents = doc.add_object(lopdf::Stream::new(dictionary! {}, content.to_vec()));
        doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => contents,
        })
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn single_page_with_inherited_resources_keeps_no_original_image() {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = marker_image(&mut doc);
        let page = drawing_page(&mut doc, pages, b"q 100 0 0 100 0 0 cm /Im0 Do Q");
        let bytes = finish_with(
            &mut doc,
            pages,
            &[page],
            dictionary! { "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } } },
        );
        assert!(bytes.windows(16).any(|w| w == MARKER));
        let output = redact_pages(&bytes, 1).unwrap();
        assert_original_pixels_absent(&output);
        let pdf = crate::document::open(&output, &Limits::local()).unwrap();
        let page = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap()[0];
        let decoded = crate::images::decode(
            &pdf,
            page_image(&pdf, page),
            &ToolCtx::new(),
            &Limits::local(),
        )
        .unwrap();
        assert!(decoded.pixels.contains(&0));
        assert!(decoded.pixels.contains(&0xA0) || decoded.pixels.contains(&0x9F));
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn shared_indirect_resources_keep_no_original_image_when_no_other_page_draws_it() {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = marker_image(&mut doc);
        let resources = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => image } });
        let mut kids = Vec::new();
        for content in [&b"q 100 0 0 100 0 0 cm /Im0 Do Q"[..], b"q Q"] {
            let page = drawing_page(&mut doc, pages, content);
            doc.get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .unwrap()
                .set("Resources", resources);
            kids.push(page);
        }
        let bytes = finish(doc, pages, &kids);
        let output = redact_pages(&bytes, 2).unwrap();
        assert_original_pixels_absent(&output);
        let pdf = crate::document::open(&output, &Limits::local()).unwrap();
        let page = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap()[0];
        let decoded = crate::images::decode(
            &pdf,
            page_image(&pdf, page),
            &ToolCtx::new(),
            &Limits::local(),
        )
        .unwrap();
        assert!(decoded.pixels.contains(&0));
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn shared_image_with_soft_mask_drawn_by_another_page_is_cloned_not_refused() {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let soft = gray_image(&mut doc, 4, 4, 128);
        let image = marker_image(&mut doc);
        doc.get_object_mut(image)
            .and_then(Object::as_stream_mut)
            .unwrap()
            .dict
            .set("SMask", soft);
        let resources = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => image } });
        let mut kids = Vec::new();
        for _ in 0..2 {
            let contents = doc.add_object(lopdf::Stream::new(
                dictionary! {},
                b"q 100 0 0 100 0 0 cm /Im0 Do Q".to_vec(),
            ));
            kids.push(doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Resources" => resources,
                "Contents" => contents,
            }));
        }
        let bytes = finish(doc, pages, &kids);
        let output = redact_pages(&bytes, 2).expect("another page drawing the image is not a leak");
        let pdf = crate::document::open(&output, &Limits::local()).unwrap();
        let page_ids = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap();
        let (redacted, untouched) = (page_image(&pdf, page_ids[0]), page_image(&pdf, page_ids[1]));
        assert_ne!(redacted, untouched);
        let stream = |id| {
            pdf.inner()
                .get_object(id)
                .unwrap()
                .as_stream()
                .unwrap()
                .clone()
        };
        let smask = |s: &lopdf::Stream| s.dict.get(b"SMask").unwrap().as_reference().unwrap();
        // The other page keeps the original image and its original soft mask.
        let kept = stream(untouched);
        let kept_content = kept
            .decompressed_content()
            .unwrap_or_else(|_| kept.content.clone());
        assert_eq!(kept_content, MARKER.to_vec());
        let kept_mask = stream(smask(&kept));
        let kept_mask_content = kept_mask
            .decompressed_content()
            .unwrap_or_else(|_| kept_mask.content.clone());
        assert!(kept_mask_content.iter().all(|&p| p == 128));
        // The redacted page carries a copy with no original pixels and its own mask.
        let copy = stream(redacted);
        let copy_content = copy
            .decompressed_content()
            .unwrap_or_else(|_| copy.content.clone());
        assert!(!copy_content.windows(MARKER.len()).any(|w| w == MARKER));
        assert_ne!(smask(&copy), smask(&kept));
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn original_image_still_reachable_after_rewrite_is_refused() {
        // Both pages are redacted and share inherited resources, so a per-page copy cannot
        // remove the original from the page tree; the rewrite must fail closed.
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = marker_image(&mut doc);
        let mut kids = Vec::new();
        for _ in 0..2 {
            kids.push(drawing_page(
                &mut doc,
                pages,
                b"q 100 0 0 100 0 0 cm /Im0 Do Q",
            ));
        }
        let bytes = finish_with(
            &mut doc,
            pages,
            &kids,
            dictionary! { "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } } },
        );
        let (request, planned, geometry) = rect_plan(
            &bytes,
            2,
            vec![
                rectangle(1, [0.0, 0.0, 50.0, 50.0]),
                rectangle(2, [0.0, 0.0, 50.0, 50.0]),
            ],
        );
        let err = rewrite_document(
            &bytes,
            &request,
            &planned,
            &geometry,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("original image"), "{err}");
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn in_place_rewrite_leaving_original_soft_mask_reachable_is_refused() {
        // One image with a soft mask, drawn only by the redacted page, so it is rewritten in
        // place with a fresh mask. The original mask object is also named by a page entry, so
        // it would still be saved with its unredacted alpha unless the run is refused.
        let build = |leak: bool| {
            let mut doc = Document::with_version("1.7");
            let pages = doc.new_object_id();
            let soft = gray_image(&mut doc, 4, 4, 128);
            let image = marker_image(&mut doc);
            doc.get_object_mut(image)
                .and_then(Object::as_stream_mut)
                .unwrap()
                .dict
                .set("SMask", soft);
            let page = drawing_page(&mut doc, pages, b"q 100 0 0 100 0 0 cm /Im0 Do Q");
            let page_dict = doc
                .get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .unwrap();
            page_dict.set(
                "Resources",
                dictionary! { "XObject" => dictionary! { "Im0" => image } },
            );
            if leak {
                page_dict.set("OriginalAlpha", soft);
            }
            finish(doc, pages, &[page])
        };
        // Control: without the extra reference the original mask is dropped and the run works.
        redact_pages(&build(false), 1).expect("an unreferenced original soft mask is dropped");
        let err = redact_pages(&build(true), 1).unwrap_err();
        assert!(
            err.to_string()
                .contains("redaction refused: original image remains reachable"),
            "{err}"
        );
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn flate_completeness_refuses_other_filters_and_bad_filter_entries() {
        let ctx = ToolCtx::new();
        let plain = |filter: Object, content: &[u8]| {
            lopdf::Stream::new(dictionary! { "Filter" => filter }, content.to_vec())
        };
        // Truncated or complete, any non-Flate filter is refused.
        for (name, data) in [
            ("LZWDecode", &b"\x80\x0b\x60"[..]),
            ("ASCII85Decode", b"87cURD]i,\"Ebo"),
            ("RunLengthDecode", b"\x05abc"),
            ("ASCIIHexDecode", b"48656c6c6f>"),
        ] {
            let stream = plain(Object::Name(name.as_bytes().to_vec()), data);
            assert!(!flate_is_complete(&stream, &ctx).unwrap(), "{name}");
        }
        let broken = plain(Object::Integer(7), b"x");
        assert!(!flate_is_complete(&broken, &ctx).unwrap());
        let unfiltered = lopdf::Stream::new(dictionary! {}, b"q Q".to_vec());
        assert!(flate_is_complete(&unfiltered, &ctx).unwrap());
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn truncated_non_flate_page_content_is_refused() {
        for (name, data) in [
            ("LZWDecode", &b"\x80\x0b\x60"[..]),
            ("ASCII85Decode", b"87cURD]i,\"Ebo"),
            ("RunLengthDecode", b"\x05abc"),
        ] {
            let mut doc = Document::with_version("1.7");
            let pages = doc.new_object_id();
            let image = gray_image(&mut doc, 2, 2, 200);
            let contents = doc.add_object(lopdf::Stream::new(
                dictionary! { "Filter" => name },
                data.to_vec(),
            ));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
                "Contents" => contents,
            });
            let bytes = finish(doc, pages, &[page]);
            let (_, planned, _) = rect_plan(&bytes, 1, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
            let err = verify_image_pixels(&bytes, &planned, &Limits::local(), &ToolCtx::new())
                .unwrap_err();
            assert!(
                err.to_string().contains("undecodable page content"),
                "{name}: {err}"
            );
        }
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn explicit_stencil_mask_cannot_be_verified_and_fails_closed() {
        use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
        // A real 1-bit stencil mask (`ImageMask true`) is not decodable as an image, so the
        // pixel check reports a failure instead of passing unverified data.
        let mut doc = Document::with_version("1.7");
        let stencil = doc.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image",
                "Width" => 4, "Height" => 4,
                "ImageMask" => true, "BitsPerComponent" => 1,
            },
            vec![0xFF; 4],
        ));
        let image = gray_image(&mut doc, 4, 4, 0);
        doc.get_object_mut(image)
            .and_then(Object::as_stream_mut)
            .unwrap()
            .dict
            .set("Mask", stencil);
        let bytes = single_page(
            &mut doc,
            b"q 100 0 0 100 0 0 cm /Im0 Do Q",
            dictionary! { "XObject" => dictionary! { "Im0" => image } },
            lopdf::Dictionary::new(),
        );
        let (_, planned, _) = rect_plan(&bytes, 1, vec![rectangle(1, [0.0, 0.0, 50.0, 50.0])]);
        assert_eq!(
            verify_image_pixels(&bytes, &planned, &Limits::local(), &ToolCtx::new()).unwrap(),
            RedactionCheckStatus::Fail
        );
    }

    /// A tiny form XObject whose own resources draw `image` as `Im0`.
    #[cfg(feature = "codecs")]
    fn form_drawing(doc: &mut Document, image: lopdf::ObjectId) -> lopdf::ObjectId {
        doc.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            },
            b"q 100 0 0 100 0 0 cm /Im0 Do Q".to_vec(),
        ))
    }

    /// Two pages: page 1 draws the marker image `Im0` from its own resources and is redacted;
    /// page 2 uses `page_two` resources, `content` and extra entries.
    #[cfg(feature = "codecs")]
    fn redact_with_other_page(
        build: impl FnOnce(&mut Document, lopdf::ObjectId) -> (lopdf::Dictionary, lopdf::Dictionary),
        content: &[u8],
        stream_dict: lopdf::Dictionary,
    ) -> (Vec<u8>, Result<Vec<u8>, PdfError>) {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = marker_image(&mut doc);
        let (resources, extra) = build(&mut doc, image);
        let first = drawing_page(&mut doc, pages, b"q 100 0 0 100 0 0 cm /Im0 Do Q");
        doc.get_object_mut(first)
            .and_then(Object::as_dict_mut)
            .unwrap()
            .set(
                "Resources",
                dictionary! { "XObject" => dictionary! { "Im0" => image } },
            );
        let contents = doc.add_object(lopdf::Stream::new(stream_dict, content.to_vec()));
        let mut second = dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => resources,
            "Contents" => contents,
        };
        second.extend(&extra);
        let second = doc.add_object(second);
        let bytes = finish(doc, pages, &[first, second]);
        let result = redact_pages(&bytes, 2);
        (bytes, result)
    }

    #[cfg(feature = "codecs")]
    fn assert_refused(result: Result<Vec<u8>, PdfError>) {
        let err = result.expect_err("the redaction must be refused");
        assert!(matches!(err, PdfError::NotImplemented(_)), "{err:?}");
        assert!(err.to_string().contains("redaction refused"), "{err}");
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn inherited_scan_and_footer_form_on_another_page_is_refused() {
        // The reviewer's repro: both pages inherit `Im0` and a footer form `Fm0` from the page
        // tree; page 1 is redacted and page 2 draws only `Fm0`.
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = marker_image(&mut doc);
        let footer = doc.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            },
            b"0 0 5 5 re f".to_vec(),
        ));
        let first = drawing_page(&mut doc, pages, b"q 100 0 0 100 0 0 cm /Im0 Do Q");
        let second = drawing_page(&mut doc, pages, b"/Fm0 Do");
        let bytes = finish_with(
            &mut doc,
            pages,
            &[first, second],
            dictionary! { "Resources" => dictionary! { "XObject" => dictionary! {
                "Im0" => image, "Fm0" => footer,
            } } },
        );
        assert_refused(redact_pages(&bytes, 2));
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn unscannable_other_pages_refuse_the_redaction() {
        // Non-Flate (truncated), inline-image and undecodable content on a page that names the
        // image in its resources.
        for (name, content, filter) in [
            ("lzw", &b"\x80\x0b\x60"[..], Some("LZWDecode")),
            ("ascii85", b"87cURD]i,\"Ebo", Some("ASCII85Decode")),
            ("inline", b"q BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI Q", None),
            ("flate", b"not zlib", Some("FlateDecode")),
        ] {
            let stream_dict = match filter {
                Some(f) => dictionary! { "Filter" => f },
                None => lopdf::Dictionary::new(),
            };
            let (_, result) = redact_with_other_page(
                |_, image| {
                    (
                        dictionary! { "XObject" => dictionary! { "Im0" => image } },
                        lopdf::Dictionary::new(),
                    )
                },
                content,
                stream_dict,
            );
            let err = result.expect_err(name);
            assert!(
                err.to_string().contains("redaction refused"),
                "{name}: {err}"
            );
        }
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn exhausted_budget_while_scanning_another_page_is_refused() {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = marker_image(&mut doc);
        let first = drawing_page(&mut doc, pages, b"q 100 0 0 100 0 0 cm /Im0 Do Q");
        let second = drawing_page(&mut doc, pages, b"q 100 0 0 100 0 0 cm /Im0 Do Q");
        let bytes = finish_with(
            &mut doc,
            pages,
            &[first, second],
            dictionary! { "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } } },
        );
        let pdf = crate::document::open(&bytes, &Limits::local()).unwrap();
        let first = collect_page_ids(pdf.inner(), &ToolCtx::new()).unwrap()[0];
        let rewrites = [ImageRewrite {
            page_id: first,
            image_id: page_image(&pdf, first),
            original_soft_mask: None,
            image: lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()),
            soft_mask: None,
        }];
        let mut tiny = Limits::local();
        tiny.max_decompressed_bytes = 1;
        let budget = crate::document::DecodeBudget::from_limits(bytes.len(), &tiny);
        let err = other_drawers(pdf.inner(), &rewrites, budget, &ToolCtx::new()).unwrap_err();
        assert!(err.to_string().contains("redaction refused"), "{err}");
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn image_drawn_through_another_pages_form_resources_is_refused() {
        let (_, result) = redact_with_other_page(
            |doc, image| {
                let form = form_drawing(doc, image);
                (
                    dictionary! { "XObject" => dictionary! { "Fm0" => form } },
                    lopdf::Dictionary::new(),
                )
            },
            b"/Fm0 Do",
            lopdf::Dictionary::new(),
        );
        assert_refused(result);
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn image_drawn_through_another_pages_annotation_appearance_is_refused() {
        let (_, result) = redact_with_other_page(
            |doc, image| {
                let form = form_drawing(doc, image);
                let annotation = doc.add_object(dictionary! {
                    "Type" => "Annot", "Subtype" => "Stamp",
                    "Rect" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                    "AP" => dictionary! { "N" => form },
                });
                (
                    lopdf::Dictionary::new(),
                    dictionary! { "Annots" => vec![Object::Reference(annotation)] },
                )
            },
            b"q Q",
            lopdf::Dictionary::new(),
        );
        assert_refused(result);
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn explicit_stencil_mask_is_refused_not_rewritten() {
        let mut doc = Document::with_version("1.7");
        let stencil = doc.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image",
                "Width" => 4, "Height" => 4,
                "ImageMask" => true, "BitsPerComponent" => 1,
            },
            vec![0xF0; 4],
        ));
        let image = marker_image(&mut doc);
        doc.get_object_mut(image)
            .and_then(Object::as_stream_mut)
            .unwrap()
            .dict
            .set("Mask", stencil);
        let bytes = single_page(
            &mut doc,
            b"q 100 0 0 100 0 0 cm /Im0 Do Q",
            dictionary! { "XObject" => dictionary! { "Im0" => image } },
            lopdf::Dictionary::new(),
        );
        assert_refused(redact_pages(&bytes, 1));
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn output_never_carries_an_eight_bit_explicit_mask() {
        // A soft mask is re-encoded as 8-bit gray; an explicit `/Mask` is never emitted.
        let mut doc = Document::with_version("1.7");
        let soft = gray_image(&mut doc, 4, 4, 128);
        let image = marker_image(&mut doc);
        doc.get_object_mut(image)
            .and_then(Object::as_stream_mut)
            .unwrap()
            .dict
            .set("SMask", soft);
        let bytes = single_page(
            &mut doc,
            b"q 100 0 0 100 0 0 cm /Im0 Do Q",
            dictionary! { "XObject" => dictionary! { "Im0" => image } },
            lopdf::Dictionary::new(),
        );
        let output = redact_pages(&bytes, 1).unwrap();
        let parsed = Document::load_mem(&output).unwrap();
        for object in parsed.objects.values() {
            if let Object::Stream(stream) = object {
                assert!(!stream.dict.has(b"Mask"), "an explicit /Mask was emitted");
            }
        }
    }
}
