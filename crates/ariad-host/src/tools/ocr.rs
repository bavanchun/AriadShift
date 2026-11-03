//! Bounded per-page OCR orchestration across the sandboxed PDFium and Tesseract engines.
use crate::{
    convert::{ConvertError, ConvertWarning, read_bounded_file},
    runner::EngineCommand,
    tools::{
        engine,
        inprocess::{self, CancelBridge, RawToolExecution},
        ocr_text,
        run::{ToolError, ToolEvent},
        secrets::SecretScrubber,
    },
    workspace::Workspace,
};
use ariad_core::{
    limits::Limits,
    ocr::{MAX_OCR_JSON_BYTES, OcrOrientation, OcrPage},
    tools::{
        ToolSpec,
        page_range::DuplicatePolicy,
        params::image_ocr::ImageOcrOutput,
        params::ocr::{OcrMode, OcrOutput},
        params::{AutoRotateParams, ImageOcrParams, OcrParams},
        specs::ocr_scan,
    },
};
use ariad_pdf::{
    PdfDoc, ToolCtx,
    tools::{OutputDoc, OutputPayload},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

struct Job<'a> {
    workspace: &'a Workspace,
    limits: &'a Limits,
    scrubber: &'a SecretScrubber,
    engine_program: Option<&'a Path>,
    cancel: &'a CancellationToken,
    deadline: Instant,
    ctx: ToolCtx,
    _bridge: CancelBridge,
    warnings: Vec<ConvertWarning>,
}

struct Sidecar<'a> {
    bytes: &'a mut Vec<u8>,
    cap: usize,
}

fn ocr_timeout(pages: usize, explicit: Option<u64>) -> u64 {
    let scaled = 600u64.saturating_add((pages as u64).saturating_mul(15));
    scaled.min(explicit.unwrap_or(14_400))
}

impl Write for Sidecar<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.len() > self.cap.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("OCR sidecar exceeds its limit"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Job<'_> {
    fn read_artifact(&self, path: &Path, cap: u64) -> Result<Vec<u8>, ToolError> {
        let file = self
            .workspace
            .open_artifact(path)
            .map_err(|_| ToolError::EngineFailure("could not open engine artifact".into()))?;
        let mut bytes = Vec::new();
        file.take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| ToolError::EngineFailure("could not read engine artifact".into()))?;
        if bytes.len() as u64 > cap {
            return Err(ToolError::LimitExceeded);
        }
        Ok(bytes)
    }
    fn check(&self) -> Result<(), ToolError> {
        self.ctx
            .check()
            .map_err(|e| inprocess::map_pdf_error(e, self.scrubber))
    }

    fn invoke(
        &mut self,
        spec: &ToolSpec,
        inputs: &[(PathBuf, String)],
        params: Value,
        events: &mut dyn FnMut(ToolEvent),
    ) -> Result<RawToolExecution, ToolError> {
        self.check()?;
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or(ToolError::LimitExceeded)?;
        let mut limits = self.limits.clone();
        // Inputs have already passed admission; intermediates are bounded by their own limits.
        limits.max_input_bytes = None;
        limits.timeout_s = Some(
            remaining
                .as_secs()
                .saturating_add(u64::from(remaining.subsec_nanos() != 0))
                .max(1),
        );
        let result = engine::execute(
            spec,
            spec.engine,
            inputs,
            &params,
            false,
            self.workspace,
            &limits,
            self.scrubber,
            self.engine_program,
            None,
            self.cancel.clone(),
            events,
        )?;
        self.check()?;
        self.warnings.extend(result.warnings.clone());
        Ok(result)
    }

    fn scan(
        &mut self,
        input: &Path,
        page: u32,
        dpi: u32,
        clean: bool,
        events: &mut dyn FnMut(ToolEvent),
    ) -> Result<PathBuf, ToolError> {
        let spec = internal("scan.render")?;
        let output = self.invoke(
            &spec,
            &[(input.to_owned(), "pdf".into())],
            json!({"pages":page.to_string(), "dpi":dpi, "clean":clean, "deskew":false}),
            events,
        )?;
        let mut image = None;
        let mut index_seen = false;
        for (path, format) in output.artifacts {
            if format == "png" && image.is_none() {
                let expected = format!("p{page:04}.png");
                if path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
                    return Err(ToolError::EngineFailure(
                        "PDFium returned the wrong page".into(),
                    ));
                }
                let dest = self
                    .workspace
                    .input_dir()
                    .join(format!("ocr-page-{page}.png"));
                let bytes = self.read_artifact(
                    &path,
                    self.limits.max_decompressed_bytes.min(128 * 1024 * 1024),
                )?;
                fs::write(&dest, bytes)
                    .map_err(|_| ToolError::OutputIo("could not stage rendered page".into()))?;
                fs::remove_file(path).map_err(|_| {
                    ToolError::OutputIo("could not remove scan intermediate".into())
                })?;
                image = Some(dest);
            } else if format == "json" && !index_seen {
                let index = self.read_artifact(&path, 1024 * 1024)?;
                let value: Value = serde_json::from_slice(&index).map_err(|_| {
                    ToolError::EngineFailure("PDFium returned an invalid scan index".into())
                })?;
                if value.as_array().is_none_or(|pages| {
                    pages.len() != 1
                        || pages[0].get("page").and_then(Value::as_u64) != Some(u64::from(page))
                }) {
                    return Err(ToolError::EngineFailure(
                        "PDFium returned an invalid scan index".into(),
                    ));
                }
                index_seen = true;
                fs::remove_file(path).map_err(|_| {
                    ToolError::OutputIo("could not remove scan intermediate".into())
                })?;
            } else {
                return Err(ToolError::EngineFailure(
                    "PDFium returned unexpected scan artifacts".into(),
                ));
            }
        }
        if !index_seen {
            return Err(ToolError::EngineFailure(
                "PDFium did not return a scan index".into(),
            ));
        }
        image.ok_or_else(|| {
            ToolError::EngineFailure("PDFium did not render the requested page".into())
        })
    }

    fn json<T: serde::de::DeserializeOwned>(
        &mut self,
        id: &str,
        input: &Path,
        format: &str,
        params: Value,
        events: &mut dyn FnMut(ToolEvent),
    ) -> Result<T, ToolError> {
        let spec = internal(id)?;
        let output = self.invoke(
            &spec,
            &[(input.to_owned(), format.to_owned())],
            params,
            events,
        )?;
        if output.artifacts.len() != 1 || output.artifacts[0].1 != "json" {
            return Err(ToolError::EngineFailure(
                "OCR engine returned unexpected artifacts".into(),
            ));
        }
        let path = &output.artifacts[0].0;
        let bytes = self.read_artifact(path, MAX_OCR_JSON_BYTES as u64)?;
        let result = serde_json::from_slice(&bytes)
            .map_err(|_| ToolError::EngineFailure("OCR engine returned invalid JSON".into()))?;
        fs::remove_file(path)
            .map_err(|_| ToolError::OutputIo("could not remove OCR intermediate".into()))?;
        Ok(result)
    }

    fn warn(&mut self, code: &str, message: &str) {
        self.warnings.push(ConvertWarning {
            code: code.into(),
            message: message.into(),
        });
    }
}

fn internal(id: &str) -> Result<ToolSpec, ToolError> {
    ocr_scan::internal_specs()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| ToolError::EngineFailure("internal OCR operation is unavailable".into()))
}

fn pdf_error(job: &Job<'_>, error: ariad_pdf::PdfError) -> ToolError {
    inprocess::map_pdf_error(error, job.scrubber)
}
fn input_error(error: ConvertError) -> ToolError {
    match error {
        ConvertError::LimitExceeded => ToolError::LimitExceeded,
        _ => ToolError::InputIo("could not read OCR input".into()),
    }
}
fn sidecar_error(error: io::Error) -> ToolError {
    if error.kind() == io::ErrorKind::InvalidData {
        ToolError::EngineFailure("OCR text cannot be represented as XML".into())
    } else {
        ToolError::LimitExceeded
    }
}
fn write_pdf(job: &Job<'_>, doc: &mut PdfDoc, path: &Path) -> Result<(), ToolError> {
    job.check()?;
    let bytes = doc.save_bytes().map_err(|e| pdf_error(job, e))?;
    if bytes.len() as u64 > job.limits.max_decompressed_bytes {
        return Err(ToolError::LimitExceeded);
    }
    fs::write(path, bytes).map_err(|_| ToolError::OutputIo("could not stage PDF".into()))
}
fn rotate(
    doc: PdfDoc,
    page: u32,
    degrees: u16,
    ctx: &ToolCtx,
) -> Result<PdfDoc, ariad_pdf::PdfError> {
    let range = ariad_core::tools::PageRange::parse(&page.to_string())
        .map_err(|e| ariad_pdf::PdfError::InvalidParams(e.to_string()))?;
    let params = ariad_core::tools::params::RotateParams {
        pages: range,
        angle: i32::from(degrees),
    };
    let mut result = ariad_pdf::tools::rotate::run_owned(doc, &params, ctx)?;
    match result.pop().ok_or(ariad_pdf::PdfError::Malformed)?.payload {
        OutputPayload::Pdf(pdf) => Ok(*pdf),
        _ => Err(ariad_pdf::PdfError::Malformed),
    }
}

/// Verify both engines before staging an OCR composite.
pub(super) fn preflight(
    id: &str,
    program: Option<&Path>,
    override_command: Option<&EngineCommand>,
) -> Result<(), ToolError> {
    if override_command.is_some() {
        return Err(ToolError::Usage(
            "engine override is not supported for OCR composites".into(),
        ));
    }
    engine::resolve_engine_command("tesseract", program, None)?;
    if id != "image.ocr" {
        engine::resolve_engine_command("pdfium", program, None)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute(
    spec: &ToolSpec,
    inputs: &[(PathBuf, String)],
    params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    cancel: &CancellationToken,
    events: &mut dyn FnMut(ToolEvent),
    clock: inprocess::HostClock,
) -> Result<RawToolExecution, ToolError> {
    let bridge = CancelBridge::new(cancel);
    let started = Instant::now();
    let deadline = started
        .checked_add(Duration::from_secs(ocr_timeout(0, limits.timeout_s)))
        .ok_or(ToolError::LimitExceeded)?;
    let mut ctx = inprocess::build_tool_ctx_with_clock(limits, Arc::clone(&bridge.flag), clock);
    ctx.deadline = Some(deadline);
    let mut job = Job {
        workspace,
        limits,
        scrubber,
        engine_program,
        cancel,
        deadline,
        ctx,
        _bridge: bridge,
        warnings: Vec::new(),
    };
    let mut sidecar = Vec::new();
    let sidecar_cap = limits.max_decompressed_bytes.min(128 * 1024 * 1024).min(
        limits
            .max_memory_mb
            .map_or(u64::MAX, |mb| u64::from(mb) * 1024 * 1024),
    );
    let sidecar_cap = usize::try_from(sidecar_cap).map_err(|_| ToolError::LimitExceeded)?;
    let (
        mut doc,
        original,
        output_kind,
        pages,
        dpi,
        languages,
        mode,
        auto_rotate,
        clean,
        deskew,
        min_confidence,
    ) = match spec.id {
        "image.ocr" => {
            let options: ImageOcrParams = serde_json::from_value(params.clone())
                .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
            let doc = if options.output == ImageOcrOutput::Pdf {
                let mut images = Vec::with_capacity(inputs.len());
                let mut total = 0u64;
                for (path, _) in inputs {
                    let remaining = limits.max_input_bytes.map(|cap| cap.saturating_sub(total));
                    let bytes = read_bounded_file(path, remaining).map_err(input_error)?;
                    total = total
                        .checked_add(bytes.len() as u64)
                        .ok_or(ToolError::LimitExceeded)?;
                    images.push(bytes);
                }
                let pdf_bytes = ariad_pdf::image_to_pdf::run_with_ctx(
                    &images,
                    &Default::default(),
                    limits,
                    &job.ctx,
                )
                .map_err(|e| pdf_error(&job, e))?;
                let mut internal_limits = limits.clone();
                internal_limits.max_input_bytes = None;
                Some(
                    ariad_pdf::document::open_with_ctx(&pdf_bytes, &internal_limits, &job.ctx)
                        .map_err(|e| pdf_error(&job, e))?,
                )
            } else {
                None
            };
            let count = u32::try_from(inputs.len()).map_err(|_| ToolError::LimitExceeded)?;
            let output = match options.output {
                ImageOcrOutput::Pdf => "pdf",
                ImageOcrOutput::Txt => "txt",
                ImageOcrOutput::Hocr => "hocr",
            };
            (
                doc,
                None,
                output,
                (1..=count).collect::<Vec<_>>(),
                options.dpi,
                options.languages,
                OcrMode::Force,
                false,
                false,
                false,
                2.0,
            )
        }
        "pdf.ocr" | "pdf.auto_rotate" => {
            let bytes =
                read_bounded_file(&inputs[0].0, limits.max_input_bytes).map_err(input_error)?;
            let original = ariad_pdf::document::open_with_ctx(&bytes, limits, &job.ctx)
                .map_err(|e| pdf_error(&job, e))?;
            let count = u32::try_from(original.inner().get_pages().len())
                .map_err(|_| ToolError::LimitExceeded)?;
            if limits.max_pages.is_some_and(|max| count > max) {
                return Err(ToolError::LimitExceeded);
            }
            if spec.id == "pdf.ocr" {
                let p: OcrParams = serde_json::from_value(params.clone())
                    .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
                let pages = p
                    .pages
                    .expand(count, DuplicatePolicy::Reject)
                    .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
                let output = match p.output {
                    OcrOutput::Pdf => "pdf",
                    OcrOutput::PdfText => "pdf+txt",
                    OcrOutput::PdfHocr => "pdf+hocr",
                };
                (
                    Some(original.clone()),
                    Some(original),
                    output,
                    pages,
                    p.dpi,
                    p.languages,
                    p.mode,
                    p.auto_rotate,
                    p.clean,
                    p.deskew,
                    2.0,
                )
            } else {
                let p: AutoRotateParams = serde_json::from_value(params.clone())
                    .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
                let pages = p
                    .pages
                    .expand(count, DuplicatePolicy::Reject)
                    .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
                (
                    Some(original.clone()),
                    Some(original),
                    "pdf",
                    pages,
                    p.dpi,
                    Vec::new(),
                    OcrMode::SkipText,
                    true,
                    false,
                    false,
                    p.min_confidence,
                )
            }
        }
        _ => return Err(ToolError::UnknownTool(spec.id.into())),
    };
    job.deadline = started
        .checked_add(Duration::from_secs(ocr_timeout(
            pages.len(),
            limits.timeout_s,
        )))
        .ok_or(ToolError::LimitExceeded)?;
    job.ctx.deadline = Some(job.deadline);
    if output_kind == "hocr" || output_kind == "pdf+hocr" {
        Sidecar { bytes: &mut sidecar, cap: sidecar_cap }.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\n").map_err(|_| ToolError::LimitExceeded)?;
    }
    let mut selected = Vec::new();
    for page in pages {
        job.check()?;
        if spec.id == "pdf.ocr" {
            let presence = ariad_pdf::scan_text::classify_text(
                doc.as_ref().expect("PDF input"),
                page,
                &job.ctx,
            )
            .map_err(|e| pdf_error(&job, e))?;
            match mode {
                OcrMode::SkipText if presence.any() => {
                    job.warn(
                        "ocr_skipped_text",
                        "A selected page already contains text and was skipped.",
                    );
                    continue;
                }
                OcrMode::Force if presence.any() => job.warn(
                    "ocr_existing_text",
                    "A new OCR layer was added to a page that already contains text.",
                ),
                OcrMode::Redo if presence.clipping => {
                    return Err(ToolError::InvalidParams(
                        "cannot safely replace text that modifies clipping".into(),
                    ));
                }
                OcrMode::Redo if presence.invisible => {
                    ariad_pdf::scan_text::remove_invisible_text_in_place(
                        doc.as_mut().expect("PDF input"),
                        page,
                        &job.ctx,
                    )
                    .map_err(|e| pdf_error(&job, e))?;
                }
                _ => {}
            }
        }
        selected.push(page);
    }
    selected.sort_unstable();
    let staged_pdf = workspace.input_dir().join("ocr-document.pdf");
    if deskew && !selected.is_empty() {
        write_pdf(&job, doc.as_mut().expect("PDF input"), &staged_pdf)?;
        let range = selected
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let deskew_spec = ocr_scan::specs()
            .into_iter()
            .find(|s| s.id == "pdf.deskew")
            .ok_or_else(|| ToolError::EngineFailure("deskew operation unavailable".into()))?;
        let result = job.invoke(
            &deskew_spec,
            &[(staged_pdf.clone(), "pdf".into())],
            json!({"pages":range}),
            events,
        )?;
        if result.artifacts.len() != 1 || result.artifacts[0].1 != "pdf" {
            return Err(ToolError::EngineFailure(
                "deskew returned unexpected artifacts".into(),
            ));
        }
        let bytes = job.read_artifact(&result.artifacts[0].0, limits.max_decompressed_bytes)?;
        fs::remove_file(&result.artifacts[0].0)
            .map_err(|_| ToolError::OutputIo("could not remove deskew intermediate".into()))?;
        let mut internal_limits = limits.clone();
        internal_limits.max_input_bytes = None;
        doc = Some(
            ariad_pdf::document::open_with_ctx(&bytes, &internal_limits, &job.ctx)
                .map_err(|e| pdf_error(&job, e))?,
        );
    }
    if spec.id != "image.ocr" && !selected.is_empty() {
        write_pdf(&job, doc.as_mut().expect("PDF input"), &staged_pdf)?;
    }
    let total = selected.len() as u64;
    if auto_rotate {
        let mut rotated = false;
        for (index, &page) in selected.iter().enumerate() {
            let image = job.scan(&staged_pdf, page, dpi, clean, events)?;
            let osd: OcrOrientation =
                job.json("ocr.orientation", &image, "png", json!({"dpi":dpi}), events)?;
            fs::remove_file(&image)
                .map_err(|_| ToolError::OutputIo("could not remove scan intermediate".into()))?;
            if !matches!(osd.rotation_cw, 0 | 90 | 180 | 270)
                || !osd.confidence.is_finite()
                || osd.confidence < 0.0
            {
                return Err(ToolError::EngineFailure(
                    "OCR engine returned invalid orientation".into(),
                ));
            }
            if osd.confidence > 0.0 && osd.confidence >= min_confidence && osd.rotation_cw != 0 {
                doc = Some(
                    rotate(
                        doc.take().expect("PDF input"),
                        page,
                        osd.rotation_cw,
                        &job.ctx,
                    )
                    .map_err(|e| pdf_error(&job, e))?,
                );
                rotated = true;
            } else if osd.confidence <= 0.0 || osd.confidence < min_confidence {
                job.warn(
                    "orientation_uncertain",
                    "No reliable page orientation was detected.",
                );
            }
            if spec.id == "pdf.auto_rotate" {
                events(ToolEvent::Progress {
                    stage: "ocr".into(),
                    done: Some(index as u64 + 1),
                    total: Some(total),
                });
            }
        }
        if rotated && spec.id == "pdf.ocr" {
            write_pdf(&job, doc.as_mut().expect("PDF input"), &staged_pdf)?;
        }
    }
    for (index, page) in selected.into_iter().enumerate() {
        if spec.id == "pdf.auto_rotate" {
            continue;
        }
        job.check()?;
        let image = if spec.id == "image.ocr" {
            inputs[(page - 1) as usize].0.clone()
        } else {
            job.scan(&staged_pdf, page, dpi, clean, events)?
        };
        let format = if spec.id == "image.ocr" {
            inputs[(page - 1) as usize].1.clone()
        } else {
            "png".into()
        };
        if spec.id != "pdf.auto_rotate" {
            let result: OcrPage = job.json(
                "ocr.recognize",
                &image,
                &format,
                json!({"languages":languages, "dpi":dpi}),
                events,
            )?;
            result.validate().map_err(|_| {
                ToolError::EngineFailure("OCR engine returned invalid page data".into())
            })?;
            if result.dpi != dpi || result.rotation_cw != 0 || result.languages != languages {
                return Err(ToolError::EngineFailure(
                    "OCR engine returned data for a different request".into(),
                ));
            }
            if result.paragraphs.is_empty() {
                job.warn("ocr_no_text", "No text was recognized on a selected page.");
            }
            if clean && spec.id == "pdf.ocr" {
                let decoded = crate::engines::tesseract_image::decode(&image, limits, job.deadline)
                    .map_err(|e| match e.code {
                        ariad_core::protocol::ErrorCode::LimitExceeded => ToolError::LimitExceeded,
                        _ => ToolError::EngineFailure("invalid cleaned scan".into()),
                    })?;
                if !ariad_pdf::scan_pages::replace_with_cleaned_image(
                    doc.as_mut().expect("PDF input"),
                    page,
                    decoded.width(),
                    decoded.height(),
                    decoded.as_raw(),
                    &job.ctx,
                )
                .map_err(|e| pdf_error(&job, e))?
                {
                    job.warn(
                        "clean_preserved_page",
                        "A page with non-image content was preserved without cleaning.",
                    );
                }
            }
            if matches!(output_kind, "pdf" | "pdf+txt" | "pdf+hocr") {
                ariad_pdf::ocr_layer::add_text_layer_in_place(
                    doc.as_mut().expect("PDF output"),
                    page,
                    &result,
                    &job.ctx,
                )
                .map_err(|e| pdf_error(&job, e))?;
            }
            if matches!(output_kind, "txt" | "pdf+txt") {
                ocr_text::text_page(
                    &result,
                    &mut Sidecar {
                        bytes: &mut sidecar,
                        cap: sidecar_cap,
                    },
                )
                .map_err(|_| ToolError::LimitExceeded)?;
            }
            if matches!(output_kind, "hocr" | "pdf+hocr") {
                ocr_text::hocr_page(
                    &result,
                    page as usize,
                    &mut Sidecar {
                        bytes: &mut sidecar,
                        cap: sidecar_cap,
                    },
                )
                .map_err(sidecar_error)?;
            }
            if sidecar.len() as u64 > limits.max_decompressed_bytes {
                return Err(ToolError::LimitExceeded);
            }
        }
        if spec.id != "image.ocr" {
            fs::remove_file(image)
                .map_err(|_| ToolError::OutputIo("could not remove scan intermediate".into()))?;
        }
        events(ToolEvent::Progress {
            stage: "ocr".into(),
            done: Some(index as u64 + 1),
            total: Some(total),
        });
    }
    if output_kind == "hocr" || output_kind == "pdf+hocr" {
        Sidecar {
            bytes: &mut sidecar,
            cap: sidecar_cap,
        }
        .write_all(b"</body></html>\n")
        .map_err(|_| ToolError::LimitExceeded)?;
    }
    let mut outputs = Vec::new();
    if matches!(output_kind, "pdf" | "pdf+txt" | "pdf+hocr") {
        outputs.push(OutputDoc::from_pdf("output.pdf", doc.expect("PDF output")));
    }
    if matches!(output_kind, "txt" | "pdf+txt") {
        outputs.push(OutputDoc::from_bytes("output.txt", "txt", sidecar));
    } else if matches!(output_kind, "hocr" | "pdf+hocr") {
        outputs.push(OutputDoc::from_bytes("output.hocr", "hocr", sidecar));
    }
    let originals: Vec<_> = original.into_iter().collect();
    let result =
        ariad_pdf::tools::finalize_tool_output(spec.id, &originals, "output", outputs, &job.ctx)
            .map_err(|e| pdf_error(&job, e))?;
    job.warnings
        .extend(result.warnings.into_iter().map(|message| ConvertWarning {
            code: "pdf_warning".into(),
            message,
        }));
    job.warnings
        .extend(result.coded_warnings.into_iter().map(|w| ConvertWarning {
            code: w.code,
            message: w.message,
        }));
    let mut artifacts = Vec::new();
    let mut total_bytes = 0u64;
    for artifact in result.artifacts {
        total_bytes = total_bytes
            .checked_add(artifact.bytes.len() as u64)
            .ok_or(ToolError::LimitExceeded)?;
        if total_bytes > limits.max_decompressed_bytes {
            return Err(ToolError::LimitExceeded);
        }
        let path = workspace
            .write_artifact(&artifact.name, &artifact.bytes)
            .map_err(|_| ToolError::OutputIo("could not stage OCR output".into()))?;
        artifacts.push((path, artifact.format));
    }
    Ok(RawToolExecution {
        artifacts,
        warnings: job.warnings,
        metrics: result.metrics,
        inline_json: None,
    })
}

#[cfg(test)]
mod timeout_tests {
    use super::ocr_timeout;

    #[test]
    fn timeout_scales_with_pages_and_respects_caller_limit() {
        assert_eq!(ocr_timeout(50, None), 1350);
        assert_eq!(ocr_timeout(50, Some(900)), 900);
        assert_eq!(ocr_timeout(1, Some(900)), 615);
        assert_eq!(ocr_timeout(920, None), 14_400);
        assert_eq!(ocr_timeout(921, None), 14_400);
        assert_eq!(ocr_timeout(10_000, Some(20_000)), 20_000);
    }
}
