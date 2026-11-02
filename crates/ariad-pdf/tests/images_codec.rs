#![cfg(feature = "codecs")]

use ariad_pdf::images::similarity::{similarity_png, ssim_gray};
use image::{GrayImage, ImageFormat};
use std::io::Cursor;

fn png(width: u32, height: u32, value: u8) -> Vec<u8> {
    let image = GrayImage::from_pixel(width, height, image::Luma([value]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn similarity_handles_partial_windows_and_distinct_images() {
    let a = vec![42; 81];
    let b = vec![200; 81];
    assert!((ssim_gray(&a, &a, 9, 9) - 1.0).abs() < 1e-12);
    assert!(ssim_gray(&a, &b, 9, 9) < 0.5);
    assert!(ssim_gray(&a[..80], &b, 9, 9).is_nan());
    assert!(ssim_gray(&[], &[], 0, 0).is_nan());
}

#[test]
fn png_similarity_checks_dimensions_and_invalid_input() {
    let a = png(9, 9, 42);
    assert!((similarity_png(&a, &a).unwrap() - 1.0).abs() < 1e-12);
    assert!(similarity_png(&a, &png(8, 9, 42)).is_err());
    assert!(similarity_png(&a, b"not png").is_err());
}
