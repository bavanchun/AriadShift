//! In-place removal of decoded image pixels beneath page-space redaction regions.

use ariad_core::limits::Limits;
use lopdf::{Stream, dictionary};

use crate::{
    PdfError, ToolCtx,
    images::{ColorSpace, DecodedImage},
    xobject::Matrix,
};

const MAX_PIXEL_WORK: u64 = 256 * 1024 * 1024;

/// Encode an unmasked decoded image as a new lossless image stream.
///
/// A fresh dictionary avoids copying alternate images, metadata, external-file
/// references or prior encoded image data into the replacement. Masked images
/// need a separate mask rewrite and are deliberately rejected here.
pub fn encode_unmasked_image(image: &DecodedImage, ctx: &ToolCtx) -> Result<Stream, PdfError> {
    ctx.check()?;
    validate(image)?;
    if image.has_mask || image.has_soft_mask {
        return Err(PdfError::NotImplemented("image mask encoding".into()));
    }
    let (encoded, parameters) = crate::images::encode_flate(image, 6)
        .map_err(|_| PdfError::NotImplemented("image encoding".into()))?;
    ctx.check()?;
    let color_space = match image.color_space {
        ColorSpace::Gray => "DeviceGray",
        ColorSpace::Rgb => "DeviceRGB",
        ColorSpace::Cmyk => "DeviceCMYK",
    };
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => i64::from(image.width),
        "Height" => i64::from(image.height),
        "ColorSpace" => color_space,
        "BitsPerComponent" => i64::from(image.bits_per_component),
        "Filter" => "FlateDecode",
        "DecodeParms" => parameters,
    };
    Ok(Stream::new(dict, encoded))
}

/// Checks pixels of a freshly rendered page against the requested replacement color.
///
/// `image_to_page` maps the unit image square to original page coordinates. All
/// pixels touched by a region must match; this intentionally fails closed when
/// antialiased boundaries cannot demonstrate complete coverage. The host must
/// obtain the raster from its sandboxed renderer and perform the other mandatory
/// checks independently.
pub fn verify_rendered_pixels(
    rendered: &image::RgbaImage,
    image_to_page: Matrix,
    regions: &[[f64; 4]],
    color: [u8; 3],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    verify_rendered_pixels_with_overlay(rendered, image_to_page, regions, color, false, limits, ctx)
}

/// Checks pixels of a freshly rendered page against the requested replacement color
/// or full opacity when overlay text is present.
pub fn verify_rendered_pixels_with_overlay(
    rendered: &image::RgbaImage,
    image_to_page: Matrix,
    regions: &[[f64; 4]],
    color: [u8; 3],
    has_overlay: bool,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    if rendered.width() == 0 || rendered.height() == 0 || regions.is_empty() {
        return Err(PdfError::InvalidParams(
            "missing rendered redaction evidence".into(),
        ));
    }
    let mut work = 0;
    let selected = selection(
        rendered.width(),
        rendered.height(),
        image_to_page,
        regions,
        limits,
        ctx,
        &mut work,
        true,
    )?;
    if !selected.iter().any(|hit| *hit) {
        return Err(PdfError::NotImplemented(
            "subpixel redaction evidence".into(),
        ));
    }
    let mut matching_background = 0usize;
    let mut total_selected = 0usize;
    for (index, (pixel, hit)) in rendered.pixels().zip(selected).enumerate() {
        if index.is_multiple_of(4096) {
            ctx.check()?;
        }
        if hit {
            total_selected += 1;
            if has_overlay {
                if pixel.0[3] != 255 {
                    return Ok(false);
                }
                if pixel.0[0] < color[0].saturating_sub(2)
                    || pixel.0[1] < color[1].saturating_sub(2)
                    || pixel.0[2] < color[2].saturating_sub(2)
                {
                    return Ok(false);
                }
                if pixel.0 == [color[0], color[1], color[2], 255] {
                    matching_background += 1;
                }
            } else if pixel.0 != [color[0], color[1], color[2], 255] {
                return Ok(false);
            }
        }
    }
    if has_overlay && total_selected > 0 && matching_background * 10 < total_selected * 4 {
        return Ok(false);
    }
    Ok(true)
}

/// Replaces affected decoded pixels and mask samples. Mask inputs must already be normalized
/// as opacity (zero transparent, maximum opaque); callers must preserve that interpretation
/// when encoding fresh streams and account for every use of a shared image.
pub fn clear_pixels(
    image: &mut DecodedImage,
    soft_mask: Option<&mut DecodedImage>,
    explicit_mask: Option<&mut DecodedImage>,
    image_to_page: Matrix,
    regions: &[[f64; 4]],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<usize, PdfError> {
    ctx.check()?;
    if image.has_mask != explicit_mask.is_some() || image.has_soft_mask != soft_mask.is_some() {
        return Err(PdfError::NotImplemented("image mask redaction".into()));
    }
    validate(image)?;
    for mask in soft_mask.iter().chain(explicit_mask.iter()) {
        validate(mask)?;
        if mask.color_space != ColorSpace::Gray || mask.has_mask || mask.has_soft_mask {
            return Err(PdfError::NotImplemented(
                "nested image mask redaction".into(),
            ));
        }
    }
    let mut work = 0u64;
    let selected = selection(
        image.width,
        image.height,
        image_to_page,
        regions,
        limits,
        ctx,
        &mut work,
        false,
    )?;
    let mask_selection = soft_mask
        .as_ref()
        .map(|mask| {
            selection(
                mask.width,
                mask.height,
                image_to_page,
                regions,
                limits,
                ctx,
                &mut work,
                false,
            )
        })
        .transpose()?;
    let explicit_selection = explicit_mask
        .as_ref()
        .map(|mask| {
            selection(
                mask.width,
                mask.height,
                image_to_page,
                regions,
                limits,
                ctx,
                &mut work,
                false,
            )
        })
        .transpose()?;
    ctx.check()?;
    let count = selected.iter().filter(|selected| **selected).count();
    paint(image, &selected, false);
    if let (Some(mask), Some(selected)) = (soft_mask, mask_selection) {
        paint(mask, &selected, true);
    }
    if let (Some(mask), Some(selected)) = (explicit_mask, explicit_selection) {
        paint(mask, &selected, true);
    }
    Ok(count)
}

fn validate(image: &DecodedImage) -> Result<(), PdfError> {
    let channels = match image.color_space {
        ColorSpace::Gray => 1usize,
        ColorSpace::Rgb => 3,
        ColorSpace::Cmyk => 4,
    };
    if image.width == 0 || image.height == 0 || !matches!(image.bits_per_component, 8 | 16) {
        return Err(PdfError::Malformed);
    }
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|n| n.checked_mul(channels))
        .and_then(|n| n.checked_mul(usize::from(image.bits_per_component / 8)))
        .ok_or(PdfError::Malformed)?;
    if expected != image.pixels.len() {
        return Err(PdfError::Malformed);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn selection(
    width: u32,
    height: u32,
    transform: Matrix,
    regions: &[[f64; 4]],
    limits: &Limits,
    ctx: &ToolCtx,
    work: &mut u64,
    require_each_region: bool,
) -> Result<Vec<bool>, PdfError> {
    let [a, b, c, d, e, f] = transform.0;
    if !transform
        .0
        .iter()
        .all(|n| n.is_finite() && n.abs() <= 1_000_000_000.0)
    {
        return Err(PdfError::Malformed);
    }
    let determinant = a * d - b * c;
    let scale = (a.abs() + b.abs() + c.abs() + d.abs()).max(1.0);
    if !determinant.is_finite() || determinant.abs() <= 1e-12 * scale * scale {
        return Err(PdfError::NotImplemented("singular image transform".into()));
    }
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or(PdfError::Malformed)?;
    if pixel_count as u64 > limits.max_decompressed_bytes {
        return Err(PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::DecompressedTooLarge {
                limit: limits.max_decompressed_bytes,
            },
        ));
    }
    let mut selected = vec![false; pixel_count];
    for &[left, bottom, right, top] in regions {
        ctx.check()?;
        let mut region_hit = false;
        if ![left, bottom, right, top].iter().all(|n| n.is_finite())
            || left >= right
            || bottom >= top
        {
            return Err(PdfError::InvalidParams(
                "invalid image redaction region".into(),
            ));
        }
        let mut polygon = [(0.0, 0.0); 4];
        for (point, (x, y)) in
            polygon
                .iter_mut()
                .zip([(left, bottom), (right, bottom), (right, top), (left, top)])
        {
            let dx = x - e;
            let dy = y - f;
            let u = (d * dx - c * dy) / determinant;
            let v = (-b * dx + a * dy) / determinant;
            *point = (u * f64::from(width), (1.0 - v) * f64::from(height));
            if !point.0.is_finite()
                || !point.1.is_finite()
                || point.0.abs() > 1e12
                || point.1.abs() > 1e12
            {
                return Err(PdfError::Malformed);
            }
            if require_each_region
                && (point.0 < 0.0
                    || point.0 > f64::from(width)
                    || point.1 < 0.0
                    || point.1 > f64::from(height))
            {
                return Err(PdfError::NotImplemented(
                    "redaction region outside rendered evidence".into(),
                ));
            }
        }
        let edge1 = (polygon[1].0 - polygon[0].0, polygon[1].1 - polygon[0].1);
        let edge2 = (polygon[3].0 - polygon[0].0, polygon[3].1 - polygon[0].1);
        let area = edge1.0 * edge2.1 - edge1.1 * edge2.0;
        if !area.is_finite() || area.abs() < 1e-12 {
            return Err(PdfError::NotImplemented("image redaction geometry".into()));
        }
        let min_x = polygon
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .clamp(0.0, f64::from(width)) as u32;
        let max_x = polygon
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .clamp(0.0, f64::from(width)) as u32;
        let min_y = polygon
            .iter()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .clamp(0.0, f64::from(height)) as u32;
        let max_y = polygon
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .clamp(0.0, f64::from(height)) as u32;
        for y in min_y..max_y {
            for x in min_x..max_x {
                *work = work.checked_add(1).ok_or(PdfError::Malformed)?;
                if *work > MAX_PIXEL_WORK {
                    return Err(PdfError::LimitExceeded(
                        ariad_core::reader::pdf::ReadError::TooManyObjects {
                            limit: MAX_PIXEL_WORK as usize,
                        },
                    ));
                }
                if (*work).is_multiple_of(4096) {
                    ctx.check()?;
                }
                if intersects(&polygon, f64::from(x), f64::from(y)) {
                    selected[y as usize * width as usize + x as usize] = true;
                    region_hit = true;
                }
            }
        }
        if require_each_region && !region_hit {
            return Err(PdfError::NotImplemented(
                "redaction region outside rendered evidence".into(),
            ));
        }
    }
    Ok(selected)
}

/// Separating-axis test for a transformed region and an open-area pixel square.
fn intersects(polygon: &[(f64, f64); 4], x: f64, y: f64) -> bool {
    let square = [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)];
    let mut axes = [
        (1.0, 0.0),
        (0.0, 1.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (0.0, 0.0),
        (0.0, 0.0),
    ];
    for i in 0..4 {
        let edge = (
            polygon[(i + 1) % 4].0 - polygon[i].0,
            polygon[(i + 1) % 4].1 - polygon[i].1,
        );
        axes[i + 2] = (-edge.1, edge.0);
    }
    for (ax, ay) in axes {
        let project = |p: &(f64, f64)| p.0 * ax + p.1 * ay;
        let poly_min = polygon.iter().map(project).fold(f64::INFINITY, f64::min);
        let poly_max = polygon
            .iter()
            .map(project)
            .fold(f64::NEG_INFINITY, f64::max);
        let square_min = square.iter().map(project).fold(f64::INFINITY, f64::min);
        let square_max = square.iter().map(project).fold(f64::NEG_INFINITY, f64::max);
        if poly_max <= square_min || square_max <= poly_min {
            return false;
        }
    }
    true
}

fn paint(image: &mut DecodedImage, selected: &[bool], opaque: bool) {
    let channels = match image.color_space {
        ColorSpace::Gray => 1usize,
        ColorSpace::Rgb => 3,
        ColorSpace::Cmyk => 4,
    };
    let component_bytes = usize::from(image.bits_per_component / 8);
    let stride = channels * component_bytes;
    for (pixel, selected) in image.pixels.chunks_exact_mut(stride).zip(selected) {
        if !*selected {
            continue;
        }
        pixel.fill(if opaque { 255 } else { 0 });
        if !opaque && image.color_space == ColorSpace::Cmyk {
            pixel[stride - component_bytes..].fill(255);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, color_space: ColorSpace, value: u8) -> DecodedImage {
        let channels = if color_space == ColorSpace::Rgb { 3 } else { 1 };
        DecodedImage {
            width,
            height,
            color_space,
            bits_per_component: 8,
            pixels: vec![value; width as usize * height as usize * channels],
            has_soft_mask: false,
            has_mask: false,
        }
    }

    #[test]
    fn removes_only_intersecting_pixels_and_updates_soft_mask() {
        let mut base = image(4, 4, ColorSpace::Rgb, 130);
        base.has_soft_mask = true;
        let mut mask = image(2, 2, ColorSpace::Gray, 0);
        let count = clear_pixels(
            &mut base,
            Some(&mut mask),
            None,
            Matrix::scale(4.0, 4.0),
            &[[1.0, 1.0, 3.0, 3.0]],
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(count, 4);
        assert_eq!(
            base.pixels
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|p| **p == [0, 0, 0])
                .count(),
            4
        );
        assert!(mask.pixels.iter().all(|value| *value == 255));
        assert_eq!(&base.pixels[..3], &[130, 130, 130]);
    }

    #[test]
    fn supports_rotated_images_and_leaves_data_unchanged_on_failure() {
        let mut base = image(4, 4, ColorSpace::Gray, 200);
        let count = clear_pixels(
            &mut base,
            None,
            None,
            Matrix([0.0, 4.0, -4.0, 0.0, 4.0, 0.0]),
            &[[0.0, 0.0, 2.0, 2.0]],
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(count, 4);
        let before = base.pixels.clone();
        assert!(
            clear_pixels(
                &mut base,
                None,
                None,
                Matrix::scale(0.0, 1.0),
                &[[0.0, 0.0, 1.0, 1.0]],
                &Limits::local(),
                &ToolCtx::new()
            )
            .is_err()
        );
        assert_eq!(base.pixels, before);
    }

    #[test]
    fn rewrites_explicit_mask_and_cmyk_samples() {
        let mut base = DecodedImage {
            width: 2,
            height: 1,
            color_space: ColorSpace::Cmyk,
            bits_per_component: 16,
            pixels: vec![42; 16],
            has_soft_mask: false,
            has_mask: true,
        };
        let mut mask = image(2, 1, ColorSpace::Gray, 0);
        assert!(
            clear_pixels(
                &mut base,
                None,
                None,
                Matrix::scale(2.0, 1.0),
                &[[0.0, 0.0, 1.0, 1.0]],
                &Limits::local(),
                &ToolCtx::new()
            )
            .is_err()
        );
        assert_eq!(base.pixels, vec![42; 16]);
        let count = clear_pixels(
            &mut base,
            None,
            Some(&mut mask),
            Matrix::scale(2.0, 1.0),
            &[[0.0, 0.0, 1.0, 1.0]],
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(&base.pixels[..8], &[0, 0, 0, 0, 0, 0, 255, 255]);
        assert_eq!(&base.pixels[8..], &[42; 8]);
        assert_eq!(mask.pixels, [255, 0]);
    }

    #[test]
    fn verifies_every_requested_region_in_the_rendered_raster() {
        let mut image = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        for y in 1..3 {
            for x in 1..3 {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        let transform = Matrix::scale(4.0, 4.0);
        let regions = [[1.0, 1.0, 3.0, 3.0]];
        assert!(
            verify_rendered_pixels(
                &image,
                transform,
                &regions,
                [0, 0, 0],
                &Limits::local(),
                &ToolCtx::new()
            )
            .unwrap()
        );
        image.put_pixel(1, 1, image::Rgba([1, 0, 0, 255]));
        assert!(
            !verify_rendered_pixels(
                &image,
                transform,
                &regions,
                [0, 0, 0],
                &Limits::local(),
                &ToolCtx::new()
            )
            .unwrap()
        );
        assert!(
            verify_rendered_pixels(
                &image,
                transform,
                &[[1.0, 1.0, 3.0, 3.0], [10.0, 10.0, 11.0, 11.0]],
                [0, 0, 0],
                &Limits::local(),
                &ToolCtx::new()
            )
            .is_err()
        );
    }

    #[test]
    fn fresh_lossless_stream_round_trips_redacted_pixels() {
        use lopdf::{Document, Object, dictionary};

        let mut pixels = image(2, 2, ColorSpace::Rgb, 130);
        clear_pixels(
            &mut pixels,
            None,
            None,
            Matrix::scale(2.0, 2.0),
            &[[0.0, 0.0, 1.0, 1.0]],
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let stream = encode_unmasked_image(&pixels, &ToolCtx::new()).unwrap();
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let image_id = doc.add_object(stream);
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
            }),
        );
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let pdf = crate::document::open(&bytes, &Limits::local()).unwrap();
        let decoded =
            crate::images::decode(&pdf, image_id, &ToolCtx::new(), &Limits::local()).unwrap();
        assert_eq!(decoded.pixels, pixels.pixels);
        pixels.has_mask = true;
        assert!(encode_unmasked_image(&pixels, &ToolCtx::new()).is_err());
    }
}
