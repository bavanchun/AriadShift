//! CBZ (Comic Book ZIP) to PDF converter.
//!
//! Extracts images from a CBZ archive in natural sort order and packages them into
//! a PDF document using [`crate::image_to_pdf`].

use std::io::{Cursor, Read};

use ariad_core::{limits::Limits, reader::pdf::ReadError, tools::params::ImageToPdfParams};

use crate::{document::ToolCtx, error::PdfError, image_to_pdf};

/// Supported image extensions in comic book archives.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

/// Maximum supported images in a comic book archive.
const MAX_CBZ_IMAGES: usize = 500;

/// Natural comparison for strings with numbers (e.g. `page2.png` < `page10.png`).
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();

    while let (Some(&ac), Some(&bc)) = (a_chars.peek(), b_chars.peek()) {
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let mut a_num: u64 = 0;
            while let Some(&d) = a_chars.peek() {
                if let Some(digit) = d.to_digit(10) {
                    a_chars.next();
                    a_num = a_num.saturating_mul(10).saturating_add(digit as u64);
                } else {
                    break;
                }
            }
            let mut b_num: u64 = 0;
            while let Some(&d) = b_chars.peek() {
                if let Some(digit) = d.to_digit(10) {
                    b_chars.next();
                    b_num = b_num.saturating_mul(10).saturating_add(digit as u64);
                } else {
                    break;
                }
            }
            match a_num.cmp(&b_num) {
                std::cmp::Ordering::Equal => continue,
                ord => return ord,
            }
        } else {
            let ac_low = ac.to_ascii_lowercase();
            let bc_low = bc.to_ascii_lowercase();
            match ac_low.cmp(&bc_low) {
                std::cmp::Ordering::Equal => {
                    a_chars.next();
                    b_chars.next();
                }
                ord => return ord,
            }
        }
    }

    match a_chars.peek().is_some().cmp(&b_chars.peek().is_some()) {
        std::cmp::Ordering::Equal => a.cmp(b),
        ord => ord,
    }
}

/// Checks whether a ZIP entry path is a supported image file.
fn is_image_path(path: &str) -> bool {
    let clean = path.replace('\\', "/");
    let file_name = clean.rsplit('/').next().unwrap_or(&clean);
    if file_name.starts_with('.') || clean.starts_with("__MACOSX/") || clean.is_empty() {
        return false;
    }
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// Converts a CBZ byte buffer into a PDF document.
pub fn cbz_to_pdf(bytes: &[u8], limits: &Limits, ctx: &ToolCtx) -> Result<Vec<u8>, PdfError> {
    ctx.check()?;
    limits.validate().map_err(ReadError::from)?;

    if let Some(max_input) = limits.max_input_bytes
        && bytes.len() as u64 > max_input
    {
        return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: max_input,
        }));
    }

    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|_| PdfError::Malformed)?;

    if archive.len() as u32 > limits.max_archive_entries {
        return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
            limit: limits.max_archive_entries as usize,
        }));
    }

    if archive.has_duplicate_names() {
        return Err(PdfError::Malformed);
    }

    let mut image_names: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|_| PdfError::Malformed)?;
        let name = file.name().map_err(|_| PdfError::Malformed)?;
        if is_image_path(&name) {
            let owned = name.into_owned();
            if !seen.insert(owned.clone()) {
                return Err(PdfError::Malformed);
            }
            if image_names.len() >= MAX_CBZ_IMAGES {
                return Err(PdfError::LimitExceeded(ReadError::TooManyObjects {
                    limit: MAX_CBZ_IMAGES,
                }));
            }
            image_names.push(owned);
        }
    }

    if image_names.is_empty() {
        return Err(PdfError::InvalidParams(
            "CBZ archive contains no supported images".to_owned(),
        ));
    }

    image_names.sort_by(|a, b| natural_cmp(a, b));

    let mut total_decompressed: u64 = 0;
    let mut images = Vec::with_capacity(image_names.len());

    for name in image_names {
        ctx.check()?;
        let mut file = archive.by_name(&name).map_err(|_| PdfError::Malformed)?;
        let uncompressed_size = file.size();
        let remaining_budget = limits
            .max_decompressed_bytes
            .saturating_sub(total_decompressed);
        let mut take = (&mut file).take(remaining_budget.saturating_add(1));
        let alloc_hint = (uncompressed_size as usize).min(64 * 1024);
        let mut buf = Vec::with_capacity(alloc_hint);
        take.read_to_end(&mut buf)
            .map_err(|_| PdfError::Malformed)?;
        total_decompressed = total_decompressed.saturating_add(buf.len() as u64);

        if total_decompressed > limits.max_decompressed_bytes {
            return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
                limit: limits.max_decompressed_bytes,
            }));
        }

        images.push(buf);
    }

    let params = ImageToPdfParams::default();
    image_to_pdf::run_with_ctx(images, &params, limits, ctx)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn natural_cmp_sorts_correctly() {
        let mut pages = vec![
            "page10.png",
            "page2.png",
            "page1.png",
            "page01.png",
            "page20.png",
            "Page3.png",
        ];
        pages.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            pages,
            vec![
                "page01.png",
                "page1.png",
                "page2.png",
                "Page3.png",
                "page10.png",
                "page20.png",
            ]
        );
    }

    #[test]
    fn filters_non_images_and_hidden_files() {
        assert!(is_image_path("chapter1/page01.jpg"));
        assert!(is_image_path("page01.PNG"));
        assert!(is_image_path("page01.webp"));
        assert!(!is_image_path("info.txt"));
        assert!(!is_image_path(".DS_Store"));
        assert!(!is_image_path("__MACOSX/._page01.jpg"));
    }

    #[test]
    fn converts_simple_cbz_to_pdf() {
        // Minimal 10x20 PNG
        let png_10x20: [u8; 69] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x14, 0x08, 0x00, 0x00, 0x00,
            0x00, 0x91, 0x85, 0xf3, 0x8a, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x60, 0x18, 0xbe, 0x00, 0x00, 0x00, 0xdc, 0x00, 0x01, 0xf1, 0xc4, 0xae,
            0xe0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        // Minimal 30x40 PNG
        let png_30x40: [u8; 75] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x00, 0x28, 0x08, 0x00, 0x00, 0x00,
            0x00, 0xcc, 0x78, 0x15, 0xce, 0x00, 0x00, 0x00, 0x12, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x60, 0x18, 0x05, 0xa3, 0x60, 0x14, 0x8c, 0x82, 0xe1, 0x05, 0x00, 0x04,
            0xd8, 0x00, 0x01, 0x5f, 0x45, 0x6a, 0x85, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
            0x44, 0xae, 0x42, 0x60, 0x82,
        ];

        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            // Insert out-of-order: page2 (30x40) before page1 (10x20)
            zip.start_file("page2.png", options).unwrap();
            zip.write_all(&png_30x40).unwrap();
            zip.start_file("page1.png", options).unwrap();
            zip.write_all(&png_10x20).unwrap();
            zip.finish().unwrap();
        }

        let zip_bytes = zip_buf.into_inner();
        let pdf = cbz_to_pdf(&zip_bytes, &Limits::local(), &ToolCtx::default()).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));

        // Verify natural ordering resulted in page1 (10x20) first, page2 (30x40) second
        let doc = lopdf::Document::load_mem(&pdf).expect("valid PDF");
        let pages = doc.get_pages();
        assert_eq!(pages.len(), 2);
        let page1 = doc.get_object(pages[&1]).unwrap().as_dict().unwrap();
        let page2 = doc.get_object(pages[&2]).unwrap().as_dict().unwrap();
        let mbox1 = page1.get(b"MediaBox").unwrap().as_array().unwrap();
        let mbox2 = page2.get(b"MediaBox").unwrap().as_array().unwrap();

        let to_f = |o: &lopdf::Object| match o {
            lopdf::Object::Integer(i) => *i as f64,
            lopdf::Object::Real(r) => *r as f64,
            _ => panic!("Expected number in MediaBox"),
        };
        assert_eq!((to_f(&mbox1[2]), to_f(&mbox1[3])), (10.0, 20.0));
        assert_eq!((to_f(&mbox2[2]), to_f(&mbox2[3])), (30.0, 40.0));
    }

    #[test]
    fn rejects_cbz_exceeding_max_images() {
        let png_1x1: [u8; 69] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x14, 0x08, 0x00, 0x00, 0x00,
            0x00, 0x91, 0x85, 0xf3, 0x8a, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x60, 0x18, 0xbe, 0x00, 0x00, 0x00, 0xdc, 0x00, 0x01, 0xf1, 0xc4, 0xae,
            0xe0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for i in 0..=MAX_CBZ_IMAGES {
                zip.start_file(format!("page_{i:04}.png"), options).unwrap();
                zip.write_all(&png_1x1).unwrap();
            }
            zip.finish().unwrap();
        }
        let zip_bytes = zip_buf.into_inner();
        let mut limits = Limits::local();
        limits.max_archive_entries = 1000;
        let err = cbz_to_pdf(&zip_bytes, &limits, &ToolCtx::default()).unwrap_err();
        match err {
            PdfError::LimitExceeded(ReadError::TooManyObjects { limit }) => {
                assert_eq!(limit, MAX_CBZ_IMAGES);
            }
            other => panic!("expected TooManyObjects error, got: {other:?}"),
        }
    }

    #[test]
    fn rejects_cbz_with_duplicate_image_names() {
        let png: [u8; 69] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x14, 0x08, 0x00, 0x00, 0x00,
            0x00, 0x91, 0x85, 0xf3, 0x8a, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x60, 0x18, 0xbe, 0x00, 0x00, 0x00, 0xdc, 0x00, 0x01, 0xf1, 0xc4, 0xae,
            0xe0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("page01.png", options).unwrap();
            zip.write_all(&png).unwrap();
            zip.start_file("page02.png", options).unwrap();
            zip.write_all(&png).unwrap();
            zip.finish().unwrap();
        }
        let mut zip_bytes = zip_buf.into_inner();
        // Replace "page02.png" with "page01.png" in zip headers to create duplicate entry names
        let p2 = b"page02.png";
        let p1 = b"page01.png";
        let mut count = 0;
        for i in 0..=zip_bytes.len().saturating_sub(p2.len()) {
            if &zip_bytes[i..i + p2.len()] == p2 {
                zip_bytes[i..i + p2.len()].copy_from_slice(p1);
                count += 1;
            }
        }
        assert_eq!(
            count, 2,
            "must replace both local header and central directory entry"
        );
        let err = cbz_to_pdf(&zip_bytes, &Limits::local(), &ToolCtx::default()).unwrap_err();
        assert!(matches!(err, PdfError::Malformed));
    }

    #[test]
    fn rejects_cbz_exceeding_byte_budget() {
        let png: [u8; 69] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x14, 0x08, 0x00, 0x00, 0x00,
            0x00, 0x91, 0x85, 0xf3, 0x8a, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x60, 0x18, 0xbe, 0x00, 0x00, 0x00, 0xdc, 0x00, 0x01, 0xf1, 0xc4, 0xae,
            0xe0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("page01.png", options).unwrap();
            zip.write_all(&png).unwrap();
            zip.finish().unwrap();
        }
        let zip_bytes = zip_buf.into_inner();
        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 40;
        let err = cbz_to_pdf(&zip_bytes, &limits, &ToolCtx::default()).unwrap_err();
        match err {
            PdfError::LimitExceeded(ReadError::DecompressedTooLarge { limit }) => {
                assert_eq!(limit, 40);
            }
            other => panic!("expected DecompressedTooLarge error, got: {other:?}"),
        }
    }
}
