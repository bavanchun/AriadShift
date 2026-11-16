//! Render-based check of what a redaction left of the raster content under its rectangles.
//!
//! The image-pixel verifier decodes images and decides where they sit with the same placement
//! scan the rewriter uses, so an error shared by both goes unnoticed. This check does not use
//! that scan to find leftovers: the host renders the original document and a probe of the
//! candidate (see [`super::rewrite_document_probe`]) with the PDFium engine, and PDFium decides
//! where every raster lands. The functions here pick the pages to render and compare the two
//! renders.
//!
//! What this does and does not give. PDFium, not our scan, places the pixels, and it also
//! sees content the scan cannot name (a tiling pattern, for example). Our rewriter still
//! builds the probe, and the comparison is blind to leftovers that render as pure black or
//! pure white (bilevel scans) and to any change smaller than a render pixel.
//!
//! Colour policy. The probe re-encodes cleared images to a device colour space, dropping the
//! original's ICC profile and `/Interpolate`, so the pixels it leaves alone can render with a
//! different colour. Two rules keep that from deciding the check either way. Inside a region
//! (the strict check) a pixel passes only if it shows a cleared colour or page white, however
//! far it moved from the original. Outside the regions, inside the footprint of a re-encoded
//! image (the drift zone), a changed pixel passes unless it became a cleared colour where the
//! original was bright. Everywhere else the original and the probe must agree within a small
//! tolerance. The drift zone never reaches the strict check.

use std::collections::BTreeMap;

use ariad_core::{limits::Limits, tools::params::redact_internal::RedactGeometry};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{PdfError, ToolCtx, tools::rotate::collect_page_ids, xobject::Matrix};

use super::{RedactionPlan, Region, pixels, redaction_refused, targeted_placements};

/// Seconds of the job's time budget one rendered page of the check is assumed to cost: two
/// engine renders (original and probe), their comparison and the engine's own validation.
const SECONDS_PER_PROBE_PAGE: u64 = 3;
/// The check may use at most this share of the time budget (one over the divisor); the text,
/// region render and object gates need the rest.
const PROBE_BUDGET_DIVISOR: u64 = 2;

/// Most pages the render check accepts under `limits`, or `None` when the job has no time
/// budget.
///
/// A larger job is refused instead of rendered, so the engine work and the memory of the
/// comparison stay bounded by the job's own budget (`timeout_s`) and not by a constant. The
/// cloud's 600 s budget allows 100 pages; a smaller plan budget allows proportionally fewer
/// (never fewer than one). The check also runs under the job deadline, which ends it with
/// `limit_exceeded` if the estimate was too generous.
#[must_use]
pub fn max_probe_pages(limits: &Limits) -> Option<usize> {
    limits.timeout_s.map(|seconds| {
        let pages = seconds / PROBE_BUDGET_DIVISOR / SECONDS_PER_PROBE_PAGE;
        usize::try_from(pages).unwrap_or(usize::MAX).max(1)
    })
}

/// Largest per-channel difference that still counts as "unchanged" between two renders.
const TOLERANCE: i16 = 8;
/// Gray levels that still count as the black a cleared image renders as, and as page white.
const BLACK_MAX: u8 = 16;
const WHITE_MIN: u8 = 239;
/// Neutral gray band a cleared CMYK image renders as. Clearing writes pure K (0, 0, 0, 255),
/// which PDFium's device CMYK conversion renders as a dark gray, not as RGB black. Accepted
/// only on pages where a cleared image is CMYK, and only for near-neutral pixels. A leftover of
/// exactly this level is invisible to the check, like any leftover that renders black.
const CMYK_CLEARED: std::ops::RangeInclusive<u8> = 24..=40;
const CMYK_CLEARED_SPREAD: u8 = 8;
/// Brightest channel an original pixel may have when the probe shows a cleared colour there,
/// inside the drift zone. See [`ProbePage::drift`].
const DRIFT_BRIGHT_MAX: u8 = 160;
/// Distance, in points, the drift zone reaches past the page footprint of a cleared image.
const DRIFT_PAD: f64 = 2.0;
/// Distance, in points, a region shrinks by before leftovers are looked for in it, so that
/// edge pixels shared with the surroundings never decide the check.
const ERODE: f64 = 1.0;

/// Outcome of comparing the renders of one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeVerdict {
    /// Nothing survives under the regions and nothing else changed.
    Agree,
    /// Original content survives, or content outside the allowed areas changed.
    Disagree,
    /// The comparison cannot decide (a region too thin to look inside, renders that do not
    /// match). Never read as agreement.
    Unavailable,
}

/// One page the render check must cover.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbePage {
    /// One-based page number.
    pub page: u32,
    /// Rectangle regions on the page, in page points.
    pub regions: Vec<[f64; 4]>,
    /// Areas where the redaction may legitimately change the render: the regions and the
    /// pixel cells of the images they clear, the characters it removes and the annotations on
    /// the page.
    pub allowed: Vec<[f64; 4]>,
    /// Page-point rectangles of the cleared CMYK image placements that intersect a region.
    /// Only inside them may a cleared pixel render as the [`CMYK_CLEARED`] gray instead of
    /// black; anywhere else on the page that gray is a leftover (dark text over RGB or vector
    /// content, for example).
    pub cmyk_footprints: Vec<[f64; 4]>,
    /// The drift zone: bounding boxes, in page points, of the images the rewrite re-encoded.
    ///
    /// Re-encoding writes a fresh device-colour image without the original's ICC profile or
    /// `/Interpolate`, so even the pixels the redaction left alone render with a different
    /// colour or edge. Outside the allowed areas, inside this zone, a change is therefore not
    /// by itself over-clearing; only a pixel that turned into a cleared colour where the
    /// original was bright is (see [`DRIFT_BRIGHT_MAX`]). The zone never reaches the in-region
    /// check, which accepts only cleared colours whatever the drift, and a placement of the
    /// same image away from the regions is outside the zone and compared strictly.
    pub drift: Vec<[f64; 4]>,
}

fn dilate(rect: [f64; 4], by: f64) -> [f64; 4] {
    [rect[0] - by, rect[1] - by, rect[2] + by, rect[3] + by]
}

fn intersects(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Whether a region is too thin to look inside once [`ERODE`] is set aside on each side.
fn is_thin(r: &[f64; 4]) -> bool {
    !(r[2] - r[0] > 2.0 * ERODE && r[3] - r[1] > 2.0 * ERODE)
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

/// Whether the page's effective resources can paint raster content at all.
fn may_paint_raster(doc: &Document, resources: &Dictionary) -> bool {
    [b"XObject".as_slice(), b"Pattern", b"Shading"]
        .iter()
        .any(|key| {
            resources.get(key).ok().is_some_and(|value| {
                match doc.dereference(value) {
                    Ok((_, Object::Dictionary(dict))) => !dict.is_empty(),
                    // An entry that cannot be read is treated as raster: never skip on doubt.
                    _ => true,
                }
            })
        })
}

/// `/Rect` of every annotation on the page, in page points.
fn annotation_rects(doc: &Document, page: ObjectId) -> Vec<[f64; 4]> {
    let Some(annots) = doc
        .get_dictionary(page)
        .ok()
        .and_then(|dict| dict.get(b"Annots").ok())
        .and_then(|value| doc.dereference(value).ok())
        .and_then(|(_, value)| value.as_array().ok())
    else {
        return Vec::new();
    };
    annots
        .iter()
        .filter_map(|item| doc.dereference(item).ok())
        .filter_map(|(_, annot)| annot.as_dict().ok())
        .filter_map(|dict| dict.get(b"Rect").ok())
        .filter_map(|value| doc.dereference(value).ok())
        .filter_map(|(_, value)| value.as_array().ok())
        .filter_map(|rect| {
            let values: Vec<f64> = rect.iter().filter_map(number).collect();
            (values.len() == 4).then(|| {
                [
                    values[0].min(values[2]),
                    values[1].min(values[3]),
                    values[0].max(values[2]),
                    values[1].max(values[3]),
                ]
            })
        })
        .collect()
}

/// Chooses the pages the render check covers and the areas it lets change.
///
/// A page is covered when it has rectangle regions, no whole-page region (that page is
/// replaced outright) and resources that can paint raster content. Which pages hold a raster
/// comes from the page resources, not from the image placement scan, so a raster the scan
/// misses still gets its page rendered. `candidate` supplies the pixel cell size of the
/// images the rewrite cleared, the only place the scan is used: to size a margin, never to
/// find leftovers.
///
/// Refuses a job with more pages than [`max_probe_pages`] allows.
pub fn probe_pages(
    input: &[u8],
    candidate: &[u8],
    plan: &RedactionPlan,
    geometry: &RedactGeometry,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<ProbePage>, PdfError> {
    let source = crate::document::open_with_ctx(input, limits, ctx)?;
    let page_ids = collect_page_ids(source.inner(), ctx)?;
    let mut whole = std::collections::BTreeSet::new();
    let mut rectangles: BTreeMap<u32, Vec<[f64; 4]>> = BTreeMap::new();
    for region in plan.regions() {
        match region {
            Region::Page { page } => {
                whole.insert(*page);
            }
            Region::Rectangle { page, bounds } => {
                rectangles.entry(*page).or_default().push(*bounds)
            }
        }
    }
    let mut pages = Vec::new();
    for (page, regions) in rectangles {
        ctx.check()?;
        if whole.contains(&page) {
            continue;
        }
        let id = page
            .checked_sub(1)
            .and_then(|index| page_ids.get(index as usize))
            .copied()
            .ok_or(PdfError::Malformed)?;
        let resources = crate::xobject::resolve_page_resources(source.inner(), id);
        if !may_paint_raster(source.inner(), &resources) {
            continue;
        }
        // Checked on the region as given, before the margin widens it: a region this thin
        // leaves nothing to look inside once its edge pixels are set aside.
        if regions.iter().any(is_thin) {
            return Err(redaction_refused(
                "region too thin to verify image pixels by rendering",
            ));
        }
        let mut allowed = Vec::new();
        for selected in plan.selected_characters(page).unwrap_or_default() {
            let bbox = geometry
                .pages
                .get(page as usize - 1)
                .and_then(|p| p.chars.get(*selected))
                .and_then(|c| c.bbox);
            if let Some(bbox) = bbox {
                allowed.push(dilate(bbox, 1.0));
            }
        }
        allowed.extend(
            annotation_rects(source.inner(), id)
                .into_iter()
                .map(|r| dilate(r, 1.0)),
        );
        pages.push(ProbePage {
            page,
            regions,
            allowed,
            cmyk_footprints: Vec::new(),
            drift: Vec::new(),
        });
    }
    if max_probe_pages(limits).is_some_and(|max| pages.len() > max) {
        return Err(redaction_refused(
            "too many pages to verify image pixels by rendering",
        ));
    }
    // Cleared images lose whole pixels, so their change reaches one pixel cell past a region.
    let cleared = cleared_images(candidate, plan, limits, ctx)?;
    for probe in &mut pages {
        let info = cleared.get(&probe.page).cloned().unwrap_or_default();
        probe.cmyk_footprints = info
            .cmyk_footprints
            .iter()
            .copied()
            .filter(|f| probe.regions.iter().any(|r| intersects(f, r)))
            .collect();
        probe.drift = info
            .footprints
            .iter()
            .map(|r| dilate(*r, DRIFT_PAD))
            .collect();
        let margin = info.margin;
        let regions: Vec<[f64; 4]> = probe
            .regions
            .iter()
            .map(|r| dilate(*r, margin + 1.0))
            .collect();
        probe.allowed.extend(regions);
    }
    Ok(pages)
}

/// What the rewrite did to the images of one page.
#[derive(Clone, Default)]
struct ClearedImages {
    /// Largest pixel cell, in page points.
    margin: f64,
    /// Bounding box of every placement of a cleared CMYK image, in page points.
    cmyk_footprints: Vec<[f64; 4]>,
    /// Bounding box of every placement of a cleared image, in page points.
    footprints: Vec<[f64; 4]>,
}

/// The images the rewrite cleared, per one-based page number. The candidate is a rewritten and
/// saved file, so its object ids need not match the original's: pages are matched by number.
fn cleared_images(
    candidate: &[u8],
    plan: &RedactionPlan,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<BTreeMap<u32, ClearedImages>, PdfError> {
    let doc = crate::document::open_with_ctx(candidate, limits, ctx)?;
    let page_numbers: BTreeMap<ObjectId, u32> = collect_page_ids(doc.inner(), ctx)?
        .into_iter()
        .zip(1u32..)
        .collect();
    let mut budget = doc.decode_budget();
    let mut margins = BTreeMap::new();
    for ((page, image), placements) in targeted_placements(doc.inner(), plan, &mut budget, ctx)? {
        let dict = &doc
            .inner()
            .get_object(image)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?
            .dict;
        let dimension = |key: &[u8]| {
            dict.get(key)
                .ok()
                .and_then(number)
                .filter(|v| *v >= 1.0)
                .ok_or(PdfError::Malformed)
        };
        let (width, height) = (dimension(b"Width")?, dimension(b"Height")?);
        let cmyk = dict
            .get(b"ColorSpace")
            .ok()
            .and_then(|v| v.as_name().ok())
            .is_some_and(|name| name == b"DeviceCMYK");
        for (ctm, _) in placements {
            let [a, b, c, d, _, _] = ctm.0;
            let cell = (a.hypot(b) / width).max(c.hypot(d) / height);
            if !cell.is_finite() {
                return Err(PdfError::Malformed);
            }
            let number = *page_numbers.get(&page).ok_or(PdfError::Malformed)?;
            let slot = margins.entry(number).or_insert_with(ClearedImages::default);
            slot.margin = slot.margin.max(cell);
            let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(x, y)| {
                (
                    ctm.0[0] * x + ctm.0[2] * y + ctm.0[4],
                    ctm.0[1] * x + ctm.0[3] * y + ctm.0[5],
                )
            });
            let xs = corners.map(|c| c.0);
            let ys = corners.map(|c| c.1);
            let min = |v: [f64; 4]| v.into_iter().fold(f64::INFINITY, f64::min);
            let max = |v: [f64; 4]| v.into_iter().fold(f64::NEG_INFINITY, f64::max);
            let footprint = [min(xs), min(ys), max(xs), max(ys)];
            if cmyk {
                slot.cmyk_footprints.push(footprint);
            }
            slot.footprints.push(footprint);
        }
    }
    Ok(margins)
}

fn channel_distance(a: &image::Rgba<u8>, b: &image::Rgba<u8>) -> i16 {
    (0..3)
        .map(|i| (i16::from(a.0[i]) - i16::from(b.0[i])).abs())
        .max()
        .unwrap_or(0)
}

/// Compares the render of the original page with the render of the probe.
///
/// A region thinner than twice [`ERODE`] in either direction leaves nothing to look inside
/// once its edge pixels are set aside, so the page is [`ProbeVerdict::Unavailable`], never
/// silently skipped.
///
/// `image_to_page` maps the unit raster square to page points, like the matrix of
/// [`pixels::verify_rendered_pixels`]. Returns `false`, never an error, for a disagreement:
/// - a pixel well inside a region that is neither black (what a cleared image renders as) nor
///   white (page background) is original content that survived, whether or not its colour
///   moved: re-encoding an ICC or CMYK image to a device colour space shifts colours, so a
///   missed cell can render differently from the original and still be a leftover;
/// - a pixel outside every allowed area that differs from the original shows that the
///   redaction changed more than it was asked to (over-clearing).
pub fn verify_probe_render(
    original: &image::RgbaImage,
    probe: &image::RgbaImage,
    image_to_page: Matrix,
    page: &ProbePage,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ProbeVerdict, PdfError> {
    if original.dimensions() != probe.dimensions() || page.regions.is_empty() {
        return Ok(ProbeVerdict::Disagree);
    }
    if page.regions.iter().any(is_thin) {
        return Ok(ProbeVerdict::Unavailable);
    }
    let (width, height) = original.dimensions();
    let mut work = 0;
    let mut mask = |regions: &[[f64; 4]]| {
        pixels::selection(
            width,
            height,
            image_to_page,
            regions,
            limits,
            ctx,
            &mut work,
            false,
        )
    };
    let eroded: Vec<[f64; 4]> = page
        .regions
        .iter()
        .map(|r| [r[0] + ERODE, r[1] + ERODE, r[2] - ERODE, r[3] - ERODE])
        .filter(|r| r[0] < r[2] && r[1] < r[3])
        .collect();
    let inside = if eroded.is_empty() {
        vec![false; width as usize * height as usize]
    } else {
        mask(&eroded)?
    };
    let allowed = mask(&page.allowed)?;
    let cmyk = if page.cmyk_footprints.is_empty() {
        vec![false; width as usize * height as usize]
    } else {
        mask(&page.cmyk_footprints)?
    };
    let drift = if page.drift.is_empty() {
        vec![false; width as usize * height as usize]
    } else {
        mask(&page.drift)?
    };
    for (index, (o, p)) in original.pixels().zip(probe.pixels()).enumerate() {
        if index.is_multiple_of(4096) {
            ctx.check()?;
        }
        let unchanged = channel_distance(o, p) <= TOLERANCE;
        let level = p.0[0].max(p.0[1]).max(p.0[2]);
        let low = p.0[0].min(p.0[1]).min(p.0[2]);
        let cleared = level <= BLACK_MAX
            || (cmyk[index]
                && CMYK_CLEARED.contains(&low)
                && CMYK_CLEARED.contains(&level)
                && level - low <= CMYK_CLEARED_SPREAD);
        if !allowed[index] && !unchanged {
            let drifted =
                drift[index] && !(cleared && o.0[..3].iter().any(|v| *v > DRIFT_BRIGHT_MAX));
            if !drifted {
                return Ok(ProbeVerdict::Disagree);
            }
        }
        if inside[index] {
            let cleared_or_blank = cleared || low >= WHITE_MIN;
            // Never "changed": a leftover that re-encoding moved to another colour is still
            // original content. Only the cleared colour or page white is accepted.
            if !cleared_or_blank {
                return Ok(ProbeVerdict::Disagree);
            }
        }
    }
    Ok(ProbeVerdict::Agree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// A 100x100 page rendered at 1 px per point (row 0 is the top of the page).
    fn matrix() -> Matrix {
        Matrix([100.0, 0.0, 0.0, 100.0, 0.0, 0.0])
    }

    fn page(allowed: Vec<[f64; 4]>) -> ProbePage {
        ProbePage {
            page: 1,
            // Page rows 40..80 and columns 20..60.
            regions: vec![[20.0, 20.0, 60.0, 60.0]],
            allowed,
            cmyk_footprints: Vec::new(),
            drift: Vec::new(),
        }
    }

    fn paint(image: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, level: u8) {
        for yy in y..y + h {
            for xx in x..x + w {
                image.put_pixel(xx, yy, Rgba([level, level, level, 255]));
            }
        }
    }

    fn check(original: &RgbaImage, probe: &RgbaImage, allowed: Vec<[f64; 4]>) -> bool {
        verify_probe_render(
            original,
            probe,
            matrix(),
            &page(allowed),
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap()
            == ProbeVerdict::Agree
    }

    fn white() -> RgbaImage {
        RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]))
    }

    #[test]
    fn cleared_region_passes_and_surviving_content_fails() {
        let mut original = white();
        paint(&mut original, 25, 45, 30, 30, 128); // image under the region
        // Cleared: the image area under the region renders black.
        let mut cleared = original.clone();
        paint(&mut cleared, 20, 40, 40, 40, 0);
        let allowed = vec![[20.0, 20.0, 60.0, 60.0]];
        assert!(check(&original, &cleared, allowed.clone()));
        // Nothing was cleared: the mid-tone survives under the region.
        assert!(!check(&original, &original, allowed.clone()));
        // Part of it was cleared; the rest survives.
        let mut partial = original.clone();
        paint(&mut partial, 25, 45, 10, 30, 0);
        assert!(!check(&original, &partial, allowed));
    }

    #[test]
    fn drifted_leftover_inside_a_region_fails() {
        // A leftover cell whose colour moved by more than the tolerance (an ICC or CMYK image
        // re-encoded to a device colour space renders differently) is still original content:
        // inside a region only the cleared colour counts, never "changed".
        let mut original = white();
        paint(&mut original, 25, 45, 30, 30, 128);
        let mut cleared = original.clone();
        paint(&mut cleared, 20, 40, 40, 40, 0);
        let allowed = vec![[20.0, 20.0, 60.0, 60.0]];
        assert!(check(&original, &cleared, allowed.clone()));
        for drifted in [100u8, 60, 40, 200] {
            let mut leftover = cleared.clone();
            paint(&mut leftover, 30, 50, 6, 6, drifted);
            assert!(
                !check(&original, &leftover, allowed.clone()),
                "a leftover cell rendered as {drifted} must fail"
            );
        }
    }

    #[test]
    fn cleared_cmyk_renders_as_a_neutral_dark_gray() {
        let mut original = white();
        paint(&mut original, 25, 45, 30, 30, 128);
        let region = [[20.0, 20.0, 60.0, 60.0]];
        let run = |cmyk_cleared: bool, level: [u8; 3]| {
            let footprints = if cmyk_cleared {
                vec![[0.0, 0.0, 100.0, 100.0]]
            } else {
                Vec::new()
            };
            let mut probe = original.clone();
            for y in 40..80 {
                for x in 20..60 {
                    probe.put_pixel(x, y, Rgba([level[0], level[1], level[2], 255]));
                }
            }
            let page = ProbePage {
                page: 1,
                regions: region.to_vec(),
                allowed: region.to_vec(),
                cmyk_footprints: footprints,
                drift: Vec::new(),
            };
            verify_probe_render(
                &original,
                &probe,
                matrix(),
                &page,
                &Limits::local(),
                &ToolCtx::new(),
            )
            .unwrap()
                == ProbeVerdict::Agree
        };
        // Pure K renders as about 32 gray: accepted only where a cleared image is CMYK.
        assert!(run(true, [32, 32, 32]));
        assert!(!run(false, [32, 32, 32]));
        // Not neutral, or outside the band: still a leftover.
        assert!(!run(true, [32, 32, 60]));
        assert!(!run(true, [60, 60, 60]));
        assert!(!run(true, [128, 128, 128]));
        assert!(run(true, [0, 0, 0]));
    }

    #[test]
    fn cmyk_gray_is_accepted_only_inside_a_cmyk_footprint() {
        // A cleared CMYK image covers columns 20..40 and a non-CMYK area is columns 40..60,
        // both under the region. A neutral 32 gray over the non-CMYK area is a leftover.
        let mut original = white();
        paint(&mut original, 25, 45, 30, 30, 128);
        let run = |gray_columns: std::ops::Range<u32>| {
            let mut probe = original.clone();
            paint(&mut probe, 20, 40, 40, 40, 0);
            for y in 40..80 {
                for x in gray_columns.clone() {
                    probe.put_pixel(x, y, Rgba([32, 32, 32, 255]));
                }
            }
            let page = ProbePage {
                page: 1,
                regions: vec![[20.0, 20.0, 60.0, 60.0]],
                allowed: vec![[20.0, 20.0, 60.0, 60.0]],
                cmyk_footprints: vec![[20.0, 0.0, 40.0, 100.0]],
                drift: Vec::new(),
            };
            verify_probe_render(
                &original,
                &probe,
                matrix(),
                &page,
                &Limits::local(),
                &ToolCtx::new(),
            )
            .unwrap()
        };
        assert_eq!(run(20..40), ProbeVerdict::Agree);
        assert_eq!(run(40..60), ProbeVerdict::Disagree);
        assert_eq!(run(20..60), ProbeVerdict::Disagree);
    }

    /// A probe page whose drift zone is the whole page.
    fn check_with_drift(original: &RgbaImage, probe: &RgbaImage, drift: bool) -> bool {
        let mut page = page(vec![[20.0, 20.0, 60.0, 60.0]]);
        if drift {
            page.drift = vec![[0.0, 0.0, 100.0, 100.0]];
        }
        verify_probe_render(
            original,
            probe,
            matrix(),
            &page,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap()
            == ProbeVerdict::Agree
    }

    #[test]
    fn colour_drift_is_tolerated_only_inside_the_drift_zone() {
        let mut original = white();
        paint(&mut original, 0, 0, 100, 100, 128);
        let mut cleared = original.clone();
        paint(&mut cleared, 20, 40, 40, 40, 0);
        // Re-encoding shifted the colour of the untouched part of the image.
        let mut drifted = cleared.clone();
        for y in 0..100 {
            for x in 0..100 {
                if !(20..60).contains(&x) || !(40..80).contains(&y) {
                    drifted.put_pixel(x, y, Rgba([100, 100, 100, 255]));
                }
            }
        }
        assert!(!check_with_drift(&original, &drifted, false));
        assert!(check_with_drift(&original, &drifted, true));

        // The zone does not excuse a pixel that turned into a cleared colour where the
        // original was bright: that is over-clearing, not drift.
        let mut bright = white();
        paint(&mut bright, 0, 0, 100, 100, 220);
        let mut cleared_bright = bright.clone();
        paint(&mut cleared_bright, 20, 40, 40, 40, 0);
        assert!(check_with_drift(&bright, &cleared_bright, true));
        let mut over = cleared_bright.clone();
        paint(&mut over, 70, 5, 10, 10, 0);
        assert!(!check_with_drift(&bright, &over, true));

        // And it never opens the in-region check: a drifted leftover still fails.
        let mut leftover = drifted.clone();
        paint(&mut leftover, 30, 50, 6, 6, 100);
        assert!(!check_with_drift(&original, &leftover, true));
    }

    #[test]
    fn a_region_too_thin_to_look_inside_is_unavailable_not_skipped() {
        let mut original = white();
        paint(&mut original, 25, 45, 30, 30, 128);
        let run = |region: [f64; 4]| {
            let page = ProbePage {
                page: 1,
                regions: vec![region],
                allowed: vec![region],
                cmyk_footprints: Vec::new(),
                drift: Vec::new(),
            };
            // The original itself as the probe: everything survives.
            verify_probe_render(
                &original,
                &original,
                matrix(),
                &page,
                &Limits::local(),
                &ToolCtx::new(),
            )
            .unwrap()
        };
        assert_eq!(run([20.0, 20.0, 21.5, 60.0]), ProbeVerdict::Unavailable);
        assert_eq!(run([20.0, 20.0, 60.0, 21.9]), ProbeVerdict::Unavailable);
        assert_eq!(run([20.0, 20.0, 60.0, 60.0]), ProbeVerdict::Disagree);
    }

    #[test]
    fn content_outside_the_regions_must_not_change() {
        let mut original = white();
        paint(&mut original, 0, 0, 100, 100, 128); // the image spans the whole page
        let mut cleared = original.clone();
        paint(&mut cleared, 20, 40, 40, 40, 0);
        let allowed = vec![[20.0, 20.0, 60.0, 60.0]];
        assert!(check(&original, &cleared, allowed.clone()));
        // Over-clearing: the whole image was blanked, not just the region.
        let mut everything = original.clone();
        paint(&mut everything, 0, 0, 100, 100, 0);
        assert!(!check(&original, &everything, allowed));
        // Clearing whole pixel cells of a coarse image legitimately reaches past the region
        // by one cell, which the allowed area carries.
        let mut cells = original.clone();
        paint(&mut cells, 10, 30, 60, 60, 0);
        assert!(!check(&original, &cells, vec![[20.0, 20.0, 60.0, 60.0]]));
        assert!(check(&original, &cells, vec![[10.0, 10.0, 70.0, 70.0]]));
    }

    #[test]
    fn mismatched_renders_and_missing_regions_disagree() {
        let original = white();
        let other = RgbaImage::from_pixel(50, 50, Rgba([255, 255, 255, 255]));
        assert!(!check(&original, &other, vec![]));
    }

    /// One page that draws a grey image under `[0, 0, 50, 50]` and text, redacted with an
    /// overlay so that the cover has an opaque fill and text.
    fn covered_document() -> Vec<u8> {
        use lopdf::{Document, Stream, dictionary};
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 4, "Height" => 4,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            vec![200; 16],
        ));
        let contents = doc.add_object(Stream::new(
            dictionary! {},
            b"q 40 0 0 40 10 10 cm /Im0 Do Q".to_vec(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            "Contents" => contents,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1
            }),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn probe_differs_from_the_output_only_by_the_cover() {
        use ariad_core::tools::params::{
            CropRect,
            redact::{RedactParams, RedactionScope, RedactionTarget},
            redact_internal::{RedactGeometry, RedactGeometryPage},
        };
        let bytes = covered_document();
        let geometry = RedactGeometry {
            schema: "ariad-redact-geometry/1".into(),
            source_sha256: crate::writer::sha256_chunks(&[&bytes]),
            page_count: 1,
            pages: vec![RedactGeometryPage {
                page: 1,
                width: 100.0,
                height: 100.0,
                rotation: 0,
                chars: Vec::new(),
            }],
        };
        let params = RedactParams {
            targets: vec![RedactionTarget::Rectangle {
                page: 1,
                rect: CropRect {
                    left: 0.0,
                    bottom: 0.0,
                    right: 30.0,
                    top: 30.0,
                },
            }],
            color: None,
            overlay_text: Some("X".into()),
            scope: RedactionScope::default(),
            invalidate_signatures: false,
        };
        let (limits, ctx) = (Limits::local(), ToolCtx::new());
        let plan = super::super::plan(&bytes, &params, &geometry, &limits, &ctx).unwrap();
        let (shipped, _) = super::super::rewrite_document_with_evidence(
            &bytes, &params, &plan, &geometry, &limits, &ctx,
        )
        .unwrap();
        let probe =
            super::super::rewrite_document_probe(&bytes, &params, &plan, &geometry, &limits, &ctx)
                .unwrap();
        assert_ne!(shipped, probe);

        let shipped = crate::document::open(&shipped, &limits).unwrap();
        let probe = crate::document::open(&probe, &limits).unwrap();
        let page_of = |doc: &crate::PdfDoc| collect_page_ids(doc.inner(), &ctx).unwrap()[0];
        let content_of = |doc: &crate::PdfDoc| {
            let page = doc.inner().get_dictionary(page_of(doc)).unwrap();
            let id = page.get(b"Contents").unwrap().as_reference().unwrap();
            (id, doc.inner().get_page_content(page_of(doc)))
        };
        let (shipped_id, shipped_content) = content_of(&shipped);
        let (probe_id, probe_content) = content_of(&probe);
        assert_eq!(shipped_id, probe_id);
        // Every other object is identical, the embedded overlay font included.
        // The writer stamps fresh dates and a random /ID on every save; those two carriers (the
        // Info dictionary and the cross-reference stream) are the only other difference.
        let comparable = |doc: &crate::PdfDoc, content: ObjectId| {
            let info = doc
                .inner()
                .trailer
                .get(b"Info")
                .ok()
                .and_then(|v| v.as_reference().ok());
            let mut objects = doc.inner().objects.clone();
            objects.retain(|id, object| {
                let is_xref = object.as_stream().is_ok_and(|s| {
                    s.dict
                        .get(b"Type")
                        .is_ok_and(|t| t.as_name().is_ok_and(|n| n == b"XRef"))
                });
                *id != content && Some(*id) != info && !is_xref
            });
            objects
        };
        assert_eq!(
            comparable(&shipped, shipped_id),
            comparable(&probe, probe_id)
        );
        // The page content is the probe's followed by the cover and nothing else.
        let shipped_ops = lopdf::content::Content::decode(&shipped_content)
            .unwrap()
            .operations;
        let probe_ops = lopdf::content::Content::decode(&probe_content)
            .unwrap()
            .operations;
        let debug = |ops: &[lopdf::content::Operation]| -> Vec<String> {
            ops.iter().map(|op| format!("{op:?}")).collect()
        };
        assert!(debug(&shipped_ops).starts_with(&debug(&probe_ops)));
        let cover = &shipped_ops[probe_ops.len()..];
        let names: Vec<&str> = cover.iter().map(|op| op.operator.as_str()).collect();
        assert_eq!(
            names,
            [
                "q", "rg", "re", "f", "BT", "rg", "Tf", "Td", "Tj", "ET", "Q"
            ],
            "the cover is one opaque fill with its overlay text"
        );
    }

    /// `count` pages, each drawing a gray image under `[10, 10, 50, 50]`.
    fn image_pages_document(count: usize) -> Vec<u8> {
        use lopdf::{Document, Stream, dictionary};
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 4, "Height" => 4,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            vec![200; 16],
        ));
        let mut kids = Vec::new();
        for _ in 0..count {
            let contents = doc.add_object(Stream::new(
                dictionary! {},
                b"q 40 0 0 40 10 10 cm /Im0 Do Q".to_vec(),
            ));
            kids.push(Object::Reference(doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
                "Contents" => contents,
            })));
        }
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => count as i64
            }),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn page_cap_follows_the_time_budget_in_the_limits() {
        use ariad_core::tools::params::{
            CropRect,
            redact::{RedactParams, RedactionScope, RedactionTarget},
            redact_internal::{RedactGeometry, RedactGeometryPage},
        };
        assert_eq!(max_probe_pages(&Limits::local()), None);
        assert_eq!(max_probe_pages(&Limits::cloud()), Some(100));
        let mut tiny = Limits::local();
        tiny.timeout_s = Some(1);
        assert_eq!(max_probe_pages(&tiny), Some(1));

        let count = 5;
        let bytes = image_pages_document(count);
        let geometry = RedactGeometry {
            schema: "ariad-redact-geometry/1".into(),
            source_sha256: crate::writer::sha256_chunks(&[&bytes]),
            page_count: count,
            pages: (1..=count as u32)
                .map(|page| RedactGeometryPage {
                    page,
                    width: 100.0,
                    height: 100.0,
                    rotation: 0,
                    chars: Vec::new(),
                })
                .collect(),
        };
        let params = RedactParams {
            targets: (1..=count as u32)
                .map(|page| RedactionTarget::Rectangle {
                    page,
                    rect: CropRect {
                        left: 0.0,
                        bottom: 0.0,
                        right: 30.0,
                        top: 30.0,
                    },
                })
                .collect(),
            color: None,
            overlay_text: None,
            scope: RedactionScope::default(),
            invalidate_signatures: false,
        };
        let ctx = ToolCtx::new();
        let run = |timeout_s: Option<u64>| {
            let mut limits = Limits::local();
            limits.timeout_s = timeout_s;
            let plan = super::super::plan(&bytes, &params, &geometry, &limits, &ctx).unwrap();
            probe_pages(&bytes, &bytes, &plan, &geometry, &limits, &ctx)
        };
        // 30 s allows 5 pages, 29 s allows 4: one page over the cap is refused.
        assert_eq!(run(Some(30)).unwrap().len(), count);
        let error = run(Some(29)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "redaction refused: too many pages to verify image pixels by rendering"
        );
        assert_eq!(run(None).unwrap().len(), count);
    }

    #[test]
    fn a_thin_region_is_refused_by_probe_pages_before_it_is_widened() {
        use ariad_core::tools::params::{
            CropRect,
            redact::{RedactParams, RedactionScope, RedactionTarget},
            redact_internal::{RedactGeometry, RedactGeometryPage},
        };
        let bytes = image_pages_document(1);
        let geometry = RedactGeometry {
            schema: "ariad-redact-geometry/1".into(),
            source_sha256: crate::writer::sha256_chunks(&[&bytes]),
            page_count: 1,
            pages: vec![RedactGeometryPage {
                page: 1,
                width: 100.0,
                height: 100.0,
                rotation: 0,
                chars: Vec::new(),
            }],
        };
        let (limits, ctx) = (Limits::local(), ToolCtx::new());
        let run = |right: f64| {
            let params = RedactParams {
                targets: vec![RedactionTarget::Rectangle {
                    page: 1,
                    rect: CropRect {
                        left: 10.0,
                        bottom: 10.0,
                        right,
                        top: 40.0,
                    },
                }],
                color: None,
                overlay_text: None,
                scope: RedactionScope::default(),
                invalidate_signatures: false,
            };
            let plan = super::super::plan(&bytes, &params, &geometry, &limits, &ctx).unwrap();
            probe_pages(&bytes, &bytes, &plan, &geometry, &limits, &ctx)
        };
        // 1.5 pt wide: widened by the pixel margin it would pass; as given it is too thin.
        let error = run(11.5).unwrap_err();
        assert_eq!(
            error.to_string(),
            "redaction refused: region too thin to verify image pixels by rendering"
        );
        assert_eq!(run(40.0).unwrap().len(), 1);
    }
}
