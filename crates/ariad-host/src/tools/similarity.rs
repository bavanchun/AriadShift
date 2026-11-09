//! Best-effort rendered similarity measurement for compressed documents.

use crate::{
    tools::{engine, secrets::SecretScrubber},
    workspace::Workspace,
};
use ariad_core::{limits::Limits, tools::registry};
use ariad_pdf::images::similarity::similarity_png;
use serde_json::json;
use std::{fs, io::Read, path::Path};
use tokio_util::sync::CancellationToken;

/// Similarity could not be measured.
#[derive(Clone, Copy, Debug)]
pub struct SimilarityError;

/// Minimum similarity accepted for a compressed document.
pub const SIMILARITY_FLOOR: f64 = 0.98;

/// Render corresponding pages through the sandboxed PDFium engine and compare them.
pub fn measure(
    input: &Path,
    output: &Path,
    limits: &Limits,
    program: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<f64, SimilarityError> {
    let input_cap = limits.max_decompressed_bytes.min(256 * 1024 * 1024);
    let mut input_bytes = Vec::new();
    fs::File::open(input)
        .map_err(|_| SimilarityError)?
        .take(input_cap.saturating_add(1))
        .read_to_end(&mut input_bytes)
        .map_err(|_| SimilarityError)?;
    if input_bytes.len() as u64 > input_cap {
        return Err(SimilarityError);
    }
    let pdf = ariad_pdf::document::open(&input_bytes, limits).map_err(|_| SimilarityError)?;
    let count = pdf.inner().get_pages().len();
    drop(pdf);
    drop(input_bytes);
    if count == 0 {
        return Err(SimilarityError);
    }
    let mut pages = vec![1, count.div_ceil(2), count];
    pages.sort_unstable();
    pages.dedup();
    let selector = pages
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let first = render(input, &selector, limits, program, cancel)?;
    let second = render(output, &selector, limits, program, cancel)?;
    if first.len() != pages.len() || first.len() != second.len() {
        return Err(SimilarityError);
    }
    let mut minimum = 1.0_f64;
    for (a, b) in first.iter().zip(&second) {
        let memory_cap = limits.max_memory_mb.map_or(256 * 1024 * 1024_u64, |mb| {
            (u64::from(mb) * 1024 * 1024 / 2).min(256 * 1024 * 1024)
        });
        for png in [a, b] {
            if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" {
                return Err(SimilarityError);
            }
            let width = u32::from_be_bytes(png[16..20].try_into().map_err(|_| SimilarityError)?);
            let height = u32::from_be_bytes(png[20..24].try_into().map_err(|_| SimilarityError)?);
            if u64::from(width)
                .saturating_mul(u64::from(height))
                .saturating_mul(12)
                > memory_cap
            {
                return Err(SimilarityError);
            }
        }
        let score = similarity_png(a, b).map_err(|_| SimilarityError)?;
        if !score.is_finite() {
            return Err(SimilarityError);
        }
        minimum = minimum.min(score);
    }
    Ok(minimum)
}

fn render(
    input: &Path,
    pages: &str,
    limits: &Limits,
    program: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<Vec<Vec<u8>>, SimilarityError> {
    let workspace = Workspace::new().map_err(|_| SimilarityError)?;
    let staged = workspace.input_dir().join("document.pdf");
    fs::copy(input, &staged).map_err(|_| SimilarityError)?;
    let spec = registry::find("pdf.render").ok_or(SimilarityError)?;
    let params = json!({"pages": pages, "dpi": 96, "format": "png", "background": "white", "annotations": true, "combine": "none"});
    let scrubber = SecretScrubber::default();
    let result = engine::execute(
        spec,
        "pdfium",
        &[(staged, "pdf".into())],
        &params,
        false,
        &workspace,
        limits,
        &scrubber,
        &crate::tools::secrets::SecretBag::new(),
        program,
        None,
        cancel.clone(),
        &mut |_| {},
    )
    .map_err(|_| SimilarityError)?;
    let mut data = Vec::new();
    let cap = limits
        .max_memory_mb
        .map_or(32 * 1024 * 1024_u64, |mb| {
            (u64::from(mb) * 1024 * 1024 / 16).min(32 * 1024 * 1024)
        })
        .min(limits.max_decompressed_bytes / 8);
    let mut total = 0_u64;
    for (path, format) in result.artifacts {
        if format != "png" {
            return Err(SimilarityError);
        }
        let mut bytes = Vec::new();
        workspace
            .open_artifact(&path)
            .map_err(|_| SimilarityError)?
            .take(cap.saturating_sub(total).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| SimilarityError)?;
        total = total.saturating_add(bytes.len() as u64);
        if total > cap {
            return Err(SimilarityError);
        }
        data.push(bytes);
    }
    Ok(data)
}
