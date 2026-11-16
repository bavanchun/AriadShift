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

use std::collections::BTreeMap;

use ariad_core::{limits::Limits, tools::params::redact_internal::RedactGeometry};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{PdfError, ToolCtx, tools::rotate::collect_page_ids, xobject::Matrix};

use super::{RedactionPlan, Region, pixels, redaction_refused, targeted_placements};

/// Most pages the render check accepts. A larger job is refused instead of rendered, which
/// bounds the engine work and the memory of the comparison.
pub const MAX_PROBE_PAGES: usize = 64;

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
/// Distance, in points, a region shrinks by before leftovers are looked for in it, so that
/// edge pixels shared with the surroundings never decide the check.
const ERODE: f64 = 1.0;

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
    /// Whether an image the rewrite cleared on this page is CMYK, so that cleared pixels may
    /// render as the [`CMYK_CLEARED`] gray instead of black.
    pub cmyk_cleared: bool,
}

fn dilate(rect: [f64; 4], by: f64) -> [f64; 4] {
    [rect[0] - by, rect[1] - by, rect[2] + by, rect[3] + by]
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
/// Refuses a job with more than [`MAX_PROBE_PAGES`] pages.
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
            cmyk_cleared: false,
        });
    }
    if pages.len() > MAX_PROBE_PAGES {
        return Err(redaction_refused(
            "too many pages to verify image pixels by rendering",
        ));
    }
    // Cleared images lose whole pixels, so their change reaches one pixel cell past a region.
    let cleared = cleared_images(candidate, plan, limits, ctx)?;
    for probe in &mut pages {
        let info = cleared.get(&probe.page).copied().unwrap_or_default();
        probe.cmyk_cleared = info.cmyk;
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
#[derive(Clone, Copy, Default)]
struct ClearedImages {
    /// Largest pixel cell, in page points.
    margin: f64,
    /// Whether any cleared image is CMYK.
    cmyk: bool,
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
            slot.cmyk |= cmyk;
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
) -> Result<bool, PdfError> {
    if original.dimensions() != probe.dimensions() || page.regions.is_empty() {
        return Ok(false);
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
    for (index, (o, p)) in original.pixels().zip(probe.pixels()).enumerate() {
        if index.is_multiple_of(4096) {
            ctx.check()?;
        }
        let unchanged = channel_distance(o, p) <= TOLERANCE;
        if !allowed[index] && !unchanged {
            return Ok(false);
        }
        if inside[index] {
            let level = p.0[0].max(p.0[1]).max(p.0[2]);
            let low = p.0[0].min(p.0[1]).min(p.0[2]);
            let cleared_or_blank = level <= BLACK_MAX
                || low >= WHITE_MIN
                || (page.cmyk_cleared
                    && CMYK_CLEARED.contains(&low)
                    && CMYK_CLEARED.contains(&level)
                    && level - low <= CMYK_CLEARED_SPREAD);
            // Never "changed": a leftover that re-encoding moved to another colour is still
            // original content. Only the cleared colour or page white is accepted.
            if !cleared_or_blank {
                return Ok(false);
            }
        }
    }
    Ok(true)
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
            cmyk_cleared: false,
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
                cmyk_cleared,
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
}
