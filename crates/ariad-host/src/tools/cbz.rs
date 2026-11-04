//! Host composite tool for `pdf.to_cbz`.
//!
//! Renders PDF pages to images via `pdf.render` (using the PDFium engine pack),
//! then packages the images into a `.cbz` archive with natural-ordered filenames.

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    tools::{
        PageRange, ToolSpec,
        params::{Background, Combine, ImageFormat, RasterFormat, RenderParams, ToCbzParams},
    },
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    runner::EngineCommand,
    tools::{
        engine,
        inprocess::RawToolExecution,
        run::{ToolError, ToolEvent},
        secrets::SecretScrubber,
    },
    workspace::Workspace,
};

/// Preflights `pdf.to_cbz` by verifying that the `pdfium` engine is available.
pub fn preflight(
    engine_program: Option<&Path>,
    engine_command: Option<&EngineCommand>,
) -> Result<(), ToolError> {
    let _ = engine::resolve_engine_command("pdfium", engine_program, engine_command)?;
    Ok(())
}

/// Executes `pdf.to_cbz` as a host composite tool.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    _spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    params_value: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    let params: ToCbzParams = serde_json::from_value(params_value.clone())
        .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
    params.validate().map_err(ToolError::InvalidParams)?;

    let render_spec = *ariad_core::tools::find("pdf.render")
        .ok_or_else(|| ToolError::UnknownTool("pdf.render".into()))?;

    let raster_format = match params.format.unwrap_or(ImageFormat::Png) {
        ImageFormat::Png => RasterFormat::Png,
        ImageFormat::Jpeg => RasterFormat::Jpeg,
        ImageFormat::Webp => RasterFormat::Webp,
    };

    let render_params = RenderParams {
        pages: params.pages.unwrap_or_else(PageRange::all),
        dpi: params.dpi.unwrap_or(150.0).round() as u32,
        format: raster_format,
        quality: params.quality,
        background: Background::White,
        annotations: true,
        combine: Combine::None,
    };

    let render_params_value =
        serde_json::to_value(render_params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

    // Execute pdf.render using pdfium engine
    let render_exec = engine::execute(
        &render_spec,
        "pdfium",
        staged_inputs,
        &render_params_value,
        false,
        workspace,
        limits,
        scrubber,
        engine_program,
        engine_command_override,
        cancel.clone(),
        on_event,
    )?;

    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    // Collect image files (either individual artifacts or from an emitted zip)
    let ext = raster_format.as_str();
    let mut image_entries: Vec<(String, Vec<u8>)> = Vec::new();

    for (artifact_path, format) in &render_exec.artifacts {
        if format == "zip" {
            let file = workspace.open_artifact(artifact_path).map_err(|_| {
                ToolError::EngineFailure("could not open render zip artifact".into())
            })?;
            let mut archive = ZipArchive::new(file)
                .map_err(|_| ToolError::EngineFailure("invalid zip artifact from render".into()))?;
            for i in 0..archive.len() {
                let mut zip_file = archive.by_index(i).map_err(|_| {
                    ToolError::EngineFailure("could not read entry from render zip".into())
                })?;
                let name = zip_file
                    .name()
                    .map_err(|_| ToolError::EngineFailure("invalid filename in render zip".into()))?
                    .to_string();
                if !zip_file.is_dir() {
                    let mut data = Vec::new();
                    zip_file.read_to_end(&mut data).map_err(|_| {
                        ToolError::EngineFailure("could not read data from render zip".into())
                    })?;
                    image_entries.push((name, data));
                }
            }
        } else {
            let mut file = workspace
                .open_artifact(artifact_path)
                .map_err(|_| ToolError::EngineFailure("could not open render artifact".into()))?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)
                .map_err(|_| ToolError::EngineFailure("could not read render artifact".into()))?;
            let filename = artifact_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("page.{ext}"));
            image_entries.push((filename, data));
        }
    }

    if image_entries.is_empty() {
        return Err(ToolError::EngineFailure(
            "rendering produced no images for CBZ".into(),
        ));
    }

    // Sort image entries in natural sort order
    image_entries.sort_by(|(a, _), (b, _)| ariad_pdf::cbz::natural_cmp(a, b));

    // Create the output CBZ archive in workspace output_dir
    let cbz_rel_path = PathBuf::from("output.cbz");
    let cbz_abs_path = workspace.output_dir().join(&cbz_rel_path);

    let file = File::create(&cbz_abs_path)
        .map_err(|e| ToolError::OutputIo(format!("could not create cbz output: {e}")))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let num_pages = image_entries.len();
    let width = if num_pages >= 1000 { 4 } else { 3 };

    for (idx, (_orig_name, data)) in image_entries.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(ToolError::Interrupted);
        }
        let page_num = idx + 1;
        let entry_name = format!("page-{page_num:0width$}.{ext}");
        zip.start_file(entry_name, options)
            .map_err(|e| ToolError::OutputIo(format!("could not write cbz entry: {e}")))?;
        zip.write_all(&data)
            .map_err(|e| ToolError::OutputIo(format!("could not write cbz data: {e}")))?;
    }

    zip.finish()
        .map_err(|e| ToolError::OutputIo(format!("could not finalize cbz: {e}")))?;

    Ok(RawToolExecution {
        artifacts: vec![(cbz_abs_path, "cbz".to_string())],
        warnings: render_exec.warnings,
        metrics: render_exec.metrics,
        inline_json: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_checks_pdfium_availability() {
        // Preflight with an explicit override command must always succeed regardless of environment
        let dummy = EngineCommand::builtin(Path::new("/bin/echo"), "pdfium");
        assert!(preflight(None, Some(&dummy)).is_ok());

        // Preflight without override must strictly assert based on environment configuration
        let res = preflight(None, None);
        match std::env::var_os("ASHIFT_PACK_PDFIUM") {
            Some(pack) if Path::new(&pack).join("pack.toml").is_file() => {
                assert!(
                    res.is_ok(),
                    "preflight must succeed when PDFium pack is configured"
                );
            }
            _ => {
                assert!(
                    res.is_err(),
                    "preflight must fail when PDFium pack is not configured"
                );
            }
        }
    }
}
