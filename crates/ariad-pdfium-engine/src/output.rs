//! The engine context handed to every tool, and the helpers that emit events.
//!
//! A tool never writes to stdout itself. It reports through [`ToolContext`]:
//!
//! - [`ToolContext::progress`] emits a `progress` event,
//! - [`ToolContext::warn`] emits a `warning` event,
//! - [`ToolContext::artifact`] emits an `artifact` event for a finished file in the output
//!   directory,
//! - the returned [`ToolResult`] carries the metrics of the final successful `result` event,
//!   which [`crate::dispatch::handle`] emits after the tool returns.
//!
//! A tool that fails returns an [`EngineFailure`]; `serve_v2` turns it into the failed `result`.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    protocol::{ErrorCode, v2::EventV2},
    tools::{
        page_range::{DuplicatePolicy, PageRange},
        params::RasterFormat,
        specs::pdfium::output_formats,
    },
};
use ariad_engine_io::{EngineFailure, RunToolPaths, emit_v2};
use ariad_pdf::{PdfError, ToolCtx as PdfToolCtx, document as pdf_document};
use image::{
    ExtendedColorType, ImageEncoder, RgbaImage,
    codecs::{
        bmp::BmpEncoder, jpeg::JpegEncoder, png::PngEncoder, tiff::TiffEncoder, webp::WebPEncoder,
    },
};
use pdfium_render::prelude::{PdfBitmap, Pdfium, PdfiumError, PdfiumInternalError};
use serde_json::Value;
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

use crate::library;

/// Metrics reported on the final `result` event.
pub type Metrics = BTreeMap<String, Value>;

/// What a tool returns: the metrics of the successful `result`, or a typed failure.
pub type ToolResult = Result<Option<Metrics>, EngineFailure>;

/// Largest pixel count of one rendered image (16384 x 16384).
pub const MAX_IMAGE_PIXELS: u64 = 16_384 * 16_384;

/// Longest accepted artifact file name in bytes.
const MAX_FILE_NAME_BYTES: usize = 255;

/// A flag a tool polls (through [`ToolContext::check`]) to stop early.
///
/// The host cancels a job by terminating the engine process, so the flag exists for in-process
/// callers and tests that run a tool on a thread.
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// Creates an unset flag.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// The validated request of one `run_tool` job plus the channel to report on.
pub struct ToolContext<'a> {
    job: String,
    tool: String,
    paths: RunToolPaths,
    limits: Limits,
    deadline: Option<Instant>,
    cancel: CancelFlag,
    library_dir: Option<PathBuf>,
    artifacts: u32,
    out: &'a mut dyn Write,
}

impl<'a> ToolContext<'a> {
    /// Builds the context of one job. The deadline starts now.
    #[must_use]
    pub fn new(
        job: impl Into<String>,
        tool: impl Into<String>,
        paths: RunToolPaths,
        limits: Limits,
        out: &'a mut dyn Write,
    ) -> Self {
        let deadline = limits
            .timeout_s
            .and_then(|seconds| Instant::now().checked_add(Duration::from_secs(seconds)));
        Self {
            job: job.into(),
            tool: tool.into(),
            paths,
            limits,
            deadline,
            cancel: CancelFlag::new(),
            library_dir: None,
            artifacts: 0,
            out,
        }
    }

    /// Loads libpdfium from `dir` instead of the engine's own directory. Tests use this; the
    /// engine never reads the location from an environment variable.
    #[must_use]
    pub fn with_library_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.library_dir = Some(dir.into());
        self
    }

    /// Uses `cancel` as the cancellation flag polled by [`Self::check`].
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancelFlag) -> Self {
        self.cancel = cancel;
        self
    }

    /// Overrides the deadline polled by [`Self::check`].
    #[must_use]
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Job identifier from the request.
    #[must_use]
    pub fn job(&self) -> &str {
        &self.job
    }

    /// Tool identifier from the request (for example `pdf.render`).
    #[must_use]
    pub fn tool(&self) -> &str {
        &self.tool
    }

    /// Validated input files, all directly inside `<workspace>/in/`.
    #[must_use]
    pub fn inputs(&self) -> &[PathBuf] {
        &self.paths.inputs
    }

    /// The first input file; `run_tool` guarantees at least one.
    #[must_use]
    pub fn input(&self) -> &Path {
        &self.paths.inputs[0]
    }

    /// File stem of the first input, used to name artifacts (`document` when it has none).
    #[must_use]
    pub fn input_stem(&self) -> String {
        let raw = self.input().file_stem().map_or_else(
            || "document".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        sanitize_stem(&raw)
    }

    /// Directory every artifact must be written into.
    #[must_use]
    pub fn output_dir(&self) -> &Path {
        &self.paths.output_dir
    }

    /// Scratch directory for temporary files.
    #[must_use]
    pub fn work_dir(&self) -> &Path {
        &self.paths.work_dir
    }

    /// Resource limits of the job.
    #[must_use]
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Effective per-image pixel cap derived from [`MAX_IMAGE_PIXELS`] and [`Limits`].
    ///
    /// Each RGBA pixel occupies 4 bytes; when `max_asset_bytes`, `max_memory_mb`, or
    /// `max_decompressed_bytes` sets a tighter byte ceiling than `MAX_IMAGE_PIXELS * 4`,
    /// that tighter pixel bound is enforced.
    #[must_use]
    pub fn max_pixels(&self) -> u64 {
        let mut cap = MAX_IMAGE_PIXELS;
        if let Some(asset_bytes) = self.limits.max_asset_bytes {
            cap = cap.min(asset_bytes / 4);
        }
        if let Some(mem_mb) = self.limits.max_memory_mb {
            cap = cap.min((u64::from(mem_mb) * 1024 * 1024) / 4);
        }
        cap = cap.min(self.limits.max_decompressed_bytes / 4);
        cap
    }

    /// Fails when the job was cancelled or its time limit passed. Call it at least once per
    /// page and inside any loop whose length depends on the document.
    pub fn check(&self) -> Result<(), EngineFailure> {
        if self.cancel.is_cancelled() {
            return Err(EngineFailure::new(
                ErrorCode::EngineFailure,
                "The job was cancelled.",
            ));
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "The job exceeded its time limit.",
            ));
        }
        Ok(())
    }

    /// Fails when processing `pages` pages exceeds the job's page limit.
    pub fn check_page_count(&self, pages: usize) -> Result<(), EngineFailure> {
        match self.limits.max_pages {
            Some(max) if u64::try_from(pages).map_or(true, |pages| pages > u64::from(max)) => {
                Err(EngineFailure::limit_exceeded())
            }
            _ => Ok(()),
        }
    }

    /// Fails when a `width` x `height` image is empty, overflows PDFium `i32` coordinates,
    /// or exceeds [`Self::max_pixels`].
    pub fn check_pixels(&self, width: u64, height: u64) -> Result<(), EngineFailure> {
        if width == 0 || height == 0 {
            return Err(EngineFailure::new(
                ErrorCode::EngineFailure,
                "Rendered image dimensions must be at least 1x1 pixel.",
            ));
        }
        if width > i32::MAX as u64 || height > i32::MAX as u64 {
            return Err(EngineFailure::limit_exceeded());
        }
        let max_pixels = self.max_pixels();
        match width.checked_mul(height) {
            Some(pixels) if pixels <= max_pixels => Ok(()),
            _ => Err(EngineFailure::limit_exceeded()),
        }
    }

    /// Reads and pre-validates the first input PDF against [`Limits`] and encryption rules.
    ///
    /// Password-protected and owner-password-encrypted PDFs fail with a typed error pointing
    /// to `pdf.decrypt`. Corrupt inputs fail with a typed error, never a panic.
    pub fn load_validated_pdf_bytes(&mut self) -> Result<Vec<u8>, EngineFailure> {
        self.check()?;
        self.limits
            .validate()
            .map_err(|err| EngineFailure::owned(ErrorCode::LimitExceeded, err.to_string()))?;

        let input_path = self.input().to_path_buf();
        if let Some(max_input) = self.limits.max_input_bytes
            && let Ok(meta) = std::fs::metadata(&input_path)
            && meta.len() > max_input
        {
            return Err(EngineFailure::limit_exceeded());
        }

        let bytes = std::fs::read(&input_path).map_err(|_| EngineFailure::io())?;
        self.check()?;

        let mut pdf_ctx = PdfToolCtx::with_cancel(Arc::clone(&self.cancel.0))
            .with_max_pages(self.limits.max_pages);
        pdf_ctx.deadline = self.deadline;

        // Allow opening the document up to the global local page ceiling so `PageRange` can
        // select a smaller subset when `limits.max_pages` is tighter than the whole document;
        // `resolve_pages` then enforces `limits.max_pages` on the resolved page count.
        let open_limits = Limits {
            max_pages: self.limits.max_pages.map(|m| m.max(2_000)).or(Some(2_000)),
            ..self.limits.clone()
        };

        let mut doc = pdf_document::open_with_ctx(&bytes, &open_limits, &pdf_ctx)
            .map_err(map_pdf_open_error)?;

        if doc.repaired_xref() {
            self.warn(
                "repaired_xref",
                "cross-reference table was malformed and reconstructed from object offsets",
            )?;
            if let Ok(repaired) = doc.save_bytes() {
                return Ok(repaired);
            }
        }

        Ok(bytes)
    }

    /// Resolves `range` against `total_pages` with [`DuplicatePolicy::Reject`] and enforces
    /// `limits.max_pages` on the selected page count.
    pub fn resolve_pages(
        &self,
        range: &PageRange,
        total_pages: usize,
    ) -> Result<Vec<usize>, EngineFailure> {
        if total_pages == 0 {
            return Err(EngineFailure::new(
                ErrorCode::EngineFailure,
                "The input PDF contains no pages.",
            ));
        }
        let total_u32 = u32::try_from(total_pages).map_err(|_| EngineFailure::limit_exceeded())?;
        let pages: Vec<usize> = range
            .expand(total_u32, DuplicatePolicy::Reject)
            .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))?
            .into_iter()
            .map(|p| p as usize)
            .collect();
        self.check_page_count(pages.len())?;
        Ok(pages)
    }

    /// Loads libpdfium (from the directory given to [`Self::with_library_dir`], otherwise from
    /// the engine's own directory) and returns the handle.
    pub fn pdfium(&self) -> Result<Pdfium, EngineFailure> {
        match &self.library_dir {
            Some(dir) => library::bind_in(dir),
            None => library::bind_default(),
        }
    }

    /// Emits a `progress` event: `done` of `total` units of `stage` are finished.
    pub fn progress(&mut self, stage: &str, done: u64, total: u64) -> Result<(), EngineFailure> {
        self.emit(EventV2::Progress {
            stage: stage.to_owned(),
            done: Some(done),
            total: Some(total),
        })
    }

    /// Emits a `warning` event. `code` is a stable snake_case identifier; `message` never
    /// carries document content.
    pub fn warn(&mut self, code: &str, message: &str) -> Result<(), EngineFailure> {
        self.emit(EventV2::Warning {
            code: code.to_owned(),
            message: message.to_owned(),
        })
    }

    /// Returns the path of a new artifact file called `name` inside the output directory.
    ///
    /// `name` must be a plain file name: no separators, no `.` or `..`, at most 255 bytes.
    pub fn artifact_path(&self, name: &str) -> Result<PathBuf, EngineFailure> {
        let plain = !name.is_empty()
            && name.len() <= MAX_FILE_NAME_BYTES
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\', '\0']);
        if plain {
            Ok(self.paths.output_dir.join(name))
        } else {
            Err(EngineFailure::invalid_request())
        }
    }

    /// Emits an `artifact` event for a finished regular file directly inside the output
    /// directory.
    ///
    /// `format` must be one of the formats the tool declares in `specs::pdfium`, and the number
    /// of artifacts is bounded by `limits.max_archive_entries`.
    pub fn artifact(&mut self, path: &Path, format: &str) -> Result<(), EngineFailure> {
        if path.parent() != Some(self.paths.output_dir.as_path())
            || path.file_name().is_none()
            || !path.is_absolute()
        {
            return Err(EngineFailure::invalid_request());
        }
        if output_formats(&self.tool).is_some_and(|allowed| !allowed.contains(&format)) {
            return Err(EngineFailure::invalid_request());
        }
        let meta = std::fs::symlink_metadata(path).map_err(|_| EngineFailure::io())?;
        if !meta.is_file() {
            return Err(EngineFailure::invalid_request());
        }
        if self.artifacts >= self.limits.max_archive_entries {
            return Err(EngineFailure::limit_exceeded());
        }
        self.artifacts += 1;
        self.emit(EventV2::Artifact {
            path: path.to_string_lossy().into_owned(),
            format: format.to_owned(),
        })
    }

    /// Number of artifacts emitted so far.
    #[must_use]
    pub fn artifact_count(&self) -> u32 {
        self.artifacts
    }

    /// Emits the final successful `result` event. Called by dispatch, not by tools.
    pub(crate) fn finish(&mut self, metrics: Option<Metrics>) -> Result<(), EngineFailure> {
        self.emit(EventV2::Result {
            ok: true,
            metrics,
            error: None,
        })
    }

    fn emit(&mut self, event: EventV2) -> Result<(), EngineFailure> {
        emit_v2(&mut self.out, event).map_err(|_| EngineFailure::io())
    }
}

/// Name of the artifact for a 1-based `page`: `<stem>.page-001.<ext>` (three digits, more when
/// the number needs them), as documented for `pdf.render` in `docs/tools.md`.
#[must_use]
pub fn page_artifact_name(stem: &str, page: u32, ext: &str) -> String {
    format!("{stem}.page-{page:03}.{ext}")
}

/// Sanitizes an input file stem so generated artifact filenames never contain path separators,
/// control characters, or exceed safe filename length bounds.
#[must_use]
pub fn sanitize_stem(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(180));
    for ch in raw.chars() {
        if out.len() >= 180 {
            break;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('.');
    if trimmed.is_empty() {
        "document".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Maps an [`ariad_pdf::PdfError`] from bounded pre-validation into a typed [`EngineFailure`].
#[must_use]
pub fn map_pdf_open_error(err: PdfError) -> EngineFailure {
    if err.is_cancelled() {
        return EngineFailure::new(ErrorCode::EngineFailure, "The job was cancelled.");
    }
    if err.is_timed_out() {
        return EngineFailure::new(ErrorCode::LimitExceeded, "The job exceeded its time limit.");
    }
    match err {
        PdfError::Encrypted { .. } => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The input PDF is encrypted; decrypt it with 'pdf.decrypt' first.",
        ),
        PdfError::LimitExceeded(read_err) => {
            EngineFailure::owned(ErrorCode::LimitExceeded, read_err.to_string())
        }
        PdfError::Malformed => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The input file is not a valid or readable PDF.",
        ),
        PdfError::InvalidParams(message) => {
            EngineFailure::owned(ErrorCode::InvalidRequest, message)
        }
        PdfError::NotImplemented(message) => {
            EngineFailure::owned(ErrorCode::EngineFailure, message)
        }
    }
}

/// Maps a [`PdfiumError`] from loading a PDF document into a typed [`EngineFailure`].
#[must_use]
pub fn map_pdfium_open_error(err: PdfiumError) -> EngineFailure {
    match err {
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::PasswordError | PdfiumInternalError::SecurityError,
        ) => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The input PDF is encrypted; decrypt it with 'pdf.decrypt' first.",
        ),
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::FormatError | PdfiumInternalError::PageError,
        ) => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The input file is not a valid or readable PDF.",
        ),
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FileError)
        | PdfiumError::IoError(_) => EngineFailure::io(),
        other => EngineFailure::owned(
            ErrorCode::EngineFailure,
            format!("PDFium failed to open the input PDF: {other}"),
        ),
    }
}

/// Converts a rendered [`PdfBitmap`] into an [`RgbaImage`].
pub fn bitmap_to_rgba(bitmap: &PdfBitmap<'_>) -> Result<RgbaImage, EngineFailure> {
    let width = u32::try_from(bitmap.width()).map_err(|_| EngineFailure::limit_exceeded())?;
    let height = u32::try_from(bitmap.height()).map_err(|_| EngineFailure::limit_exceeded())?;
    let bytes = bitmap.as_rgba_bytes();
    RgbaImage::from_raw(width, height, bytes).ok_or_else(|| {
        EngineFailure::new(
            ErrorCode::EngineFailure,
            "PDFium returned a bitmap buffer whose size did not match its dimensions.",
        )
    })
}

/// Encodes an [`RgbaImage`] into the requested [`RasterFormat`].
pub fn encode_raster_image(
    image: &RgbaImage,
    format: RasterFormat,
    quality: Option<u8>,
) -> Result<Vec<u8>, EngineFailure> {
    let mut buf = Vec::new();
    let res = match format {
        RasterFormat::Png => PngEncoder::new(&mut buf).write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        ),
        RasterFormat::Jpeg => {
            let rgb = image::ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
                let [r, g, b, _] = image.get_pixel(x, y).0;
                image::Rgb([r, g, b])
            });
            let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality.unwrap_or(85));
            encoder.encode_image(&rgb)
        }
        RasterFormat::Webp => {
            let encoder = WebPEncoder::new_lossless(&mut buf);
            encoder.write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
            )
        }
        RasterFormat::Bmp => {
            let encoder = BmpEncoder::new(&mut buf);
            encoder.write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
            )
        }
        RasterFormat::Tiff => {
            let encoder = TiffEncoder::new(Cursor::new(&mut buf));
            encoder.write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
            )
        }
    };
    res.map_err(|err| {
        EngineFailure::owned(
            ErrorCode::EngineFailure,
            format!("Failed to encode {} image: {err}", format.as_str()),
        )
    })?;
    Ok(buf)
}

/// Writes `entries` (`(file_name, bytes)`) into a deterministic Deflate-compressed `.zip` file.
pub fn write_zip_archive(
    zip_path: &Path,
    entries: &[(String, Vec<u8>)],
) -> Result<(), EngineFailure> {
    let file = File::create(zip_path).map_err(|_| EngineFailure::io())?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());
    for (name, bytes) in entries {
        writer
            .start_file(name, options)
            .map_err(|err| EngineFailure::owned(ErrorCode::EngineFailure, err.to_string()))?;
        writer.write_all(bytes).map_err(|_| EngineFailure::io())?;
    }
    writer
        .finish()
        .map_err(|err| EngineFailure::owned(ErrorCode::EngineFailure, err.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;

    struct Workspace {
        _dir: tempfile::TempDir,
        paths: RunToolPaths,
    }

    fn workspace() -> Workspace {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical root");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(root.join(sub)).expect("create workspace dir");
        }
        fs::write(root.join("in").join("doc.pdf"), b"%PDF-1.7").expect("write input");
        let paths = RunToolPaths {
            workspace: root.clone(),
            inputs: vec![root.join("in").join("doc.pdf")],
            output_dir: root.join("out"),
            work_dir: root.join("tmp"),
        };
        Workspace { _dir: dir, paths }
    }

    fn events(bytes: &[u8]) -> Vec<serde_json::Value> {
        bytes
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).expect("event json"))
            .collect()
    }

    #[test]
    fn emits_progress_warning_artifact_and_result_in_order() {
        let ws = workspace();
        let mut sink = Vec::new();
        let artifact = ws.paths.output_dir.join("doc.page-001.png");
        fs::write(&artifact, b"png").expect("write artifact");
        {
            let mut ctx = ToolContext::new(
                "job-1",
                "pdf.render",
                ws.paths.clone(),
                Limits::local(),
                &mut sink,
            );
            ctx.progress("render", 1, 2).expect("progress");
            ctx.warn("page_skipped", "Page 2 could not be rendered.")
                .expect("warning");
            ctx.artifact(&artifact, "png").expect("artifact");
            assert_eq!(ctx.artifact_count(), 1);
            ctx.finish(Some(Metrics::from([("pages".to_owned(), json!(1))])))
                .expect("result");
        }
        let events = events(&sink);
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            json!({"type":"progress","stage":"render","done":1,"total":2})
        );
        assert_eq!(events[1]["type"], "warning");
        assert_eq!(events[1]["code"], "page_skipped");
        assert_eq!(events[2]["type"], "artifact");
        assert_eq!(events[2]["format"], "png");
        assert_eq!(
            events[3],
            json!({"type":"result","ok":true,"metrics":{"pages":1}})
        );
    }

    #[test]
    fn artifact_rejects_paths_outside_the_output_directory_and_undeclared_formats() {
        let ws = workspace();
        let mut sink = Vec::new();
        let inside = ws.paths.output_dir.join("doc.page-001.png");
        fs::write(&inside, b"png").expect("write artifact");
        let outside = ws.paths.work_dir.join("doc.page-001.png");
        fs::write(&outside, b"png").expect("write outside file");
        let mut ctx = ToolContext::new(
            "job-1",
            "pdf.render",
            ws.paths.clone(),
            Limits::local(),
            &mut sink,
        );
        assert!(ctx.artifact(&outside, "png").is_err());
        assert!(ctx.artifact(&inside, "gif").is_err());
        assert!(
            ctx.artifact(&ws.paths.output_dir.join("missing.png"), "png")
                .is_err()
        );
        assert!(ctx.artifact(&ws.paths.output_dir, "png").is_err());
        assert_eq!(ctx.artifact_count(), 0);
    }

    #[test]
    fn artifact_count_is_bounded_by_the_archive_entry_limit() {
        let ws = workspace();
        let mut sink = Vec::new();
        let artifact = ws.paths.output_dir.join("a.png");
        fs::write(&artifact, b"png").expect("write artifact");
        let limits = Limits {
            max_archive_entries: 1,
            ..Limits::local()
        };
        let mut ctx = ToolContext::new("job-1", "pdf.render", ws.paths.clone(), limits, &mut sink);
        ctx.artifact(&artifact, "png").expect("first artifact");
        assert_eq!(
            ctx.artifact(&artifact, "png").unwrap_err().code,
            ErrorCode::LimitExceeded
        );
    }

    #[test]
    fn artifact_path_accepts_only_plain_file_names() {
        let ws = workspace();
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "j",
            "pdf.render",
            ws.paths.clone(),
            Limits::local(),
            &mut sink,
        );
        assert_eq!(
            ctx.artifact_path("doc.page-001.png").expect("plain name"),
            ws.paths.output_dir.join("doc.page-001.png")
        );
        for name in ["", ".", "..", "../x.png", "a/b.png", "a\\b.png", "a\0b"] {
            assert!(ctx.artifact_path(name).is_err(), "{name:?}");
        }
        assert!(ctx.artifact_path(&"a".repeat(256)).is_err());
    }

    #[test]
    fn check_reports_cancellation_and_timeout() {
        let ws = workspace();
        let mut sink = Vec::new();
        let cancel = CancelFlag::new();
        let ctx = ToolContext::new(
            "j",
            "pdf.render",
            ws.paths.clone(),
            Limits::local(),
            &mut sink,
        )
        .with_cancel(cancel.clone());
        ctx.check().expect("running job passes");
        cancel.cancel();
        assert_eq!(ctx.check().unwrap_err().code, ErrorCode::EngineFailure);

        let mut sink = Vec::new();
        let expired = Limits {
            timeout_s: Some(1),
            ..Limits::local()
        };
        let mut ctx = ToolContext::new("j", "pdf.render", ws.paths.clone(), expired, &mut sink);
        ctx.deadline = Instant::now().checked_sub(Duration::from_secs(1));
        assert_eq!(ctx.check().unwrap_err().code, ErrorCode::LimitExceeded);
    }

    #[test]
    fn page_and_pixel_limits_are_enforced() {
        let ws = workspace();
        let mut sink = Vec::new();
        let limits = Limits {
            max_pages: Some(10),
            ..Limits::local()
        };
        let ctx = ToolContext::new("j", "pdf.render", ws.paths.clone(), limits, &mut sink);
        ctx.check_page_count(10).expect("at the limit");
        assert!(ctx.check_page_count(11).is_err());
        ctx.check_pixels(16_384, 16_384).expect("at the limit");
        assert!(ctx.check_pixels(16_385, 16_384).is_err());
        assert!(ctx.check_pixels(u64::MAX, 2).is_err());
    }

    #[test]
    fn page_artifact_names_are_zero_padded() {
        assert_eq!(page_artifact_name("doc", 1, "png"), "doc.page-001.png");
        assert_eq!(
            page_artifact_name("doc", 1234, "jpeg"),
            "doc.page-1234.jpeg"
        );
    }
}
