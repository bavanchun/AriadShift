//! CBZ (Comic Book ZIP) to PDF converter.
//!
//! Extracts images from a CBZ archive in natural sort order and packages them into
//! a PDF document using [`crate::image_to_pdf`].

use std::io::{Cursor, Read};

use ariad_core::{limits::Limits, reader::pdf::ReadError, tools::params::ImageToPdfParams};

use crate::{document::ToolCtx, error::PdfError, image_to_pdf};

/// Supported image extensions in comic book archives.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

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

    let mut image_names: Vec<String> = Vec::new();
    for name in archive.file_names().flatten() {
        if is_image_path(&name) {
            image_names.push(name.into_owned());
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
        total_decompressed = total_decompressed.saturating_add(uncompressed_size);

        if total_decompressed > limits.max_decompressed_bytes {
            return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
                limit: limits.max_decompressed_bytes,
            }));
        }

        let mut buf = Vec::with_capacity(uncompressed_size as usize);
        file.read_to_end(&mut buf)
            .map_err(|_| PdfError::Malformed)?;
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
        // Minimal 1x1 8-bit grayscale PNG:
        // Signature (8 bytes) + IHDR (25 bytes) + IDAT (11 bytes) + IEND (12 bytes)
        let minimal_png: [u8; 67] = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // signature
            0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR length & type
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
            0x08, 0x00, 0x00, 0x00, 0x00, 0x3a, 0x7e, 0x9b, 0x55, // 8-bit gray + crc
            0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, // IDAT length & type
            0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x73, 0x75, 0x01,
            0x18, // crc
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82, // IEND
        ];

        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("page2.png", options).unwrap();
            zip.write_all(&minimal_png).unwrap();
            zip.start_file("page1.png", options).unwrap();
            zip.write_all(&minimal_png).unwrap();
            zip.finish().unwrap();
        }

        let zip_bytes = zip_buf.into_inner();
        let pdf = cbz_to_pdf(&zip_bytes, &Limits::local(), &ToolCtx::default()).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }
}
