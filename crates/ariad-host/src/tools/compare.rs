//! Host composite tool for `pdf.compare`.
//!
//! Coordinates text diffing (optionally with PDFium `pdf.text_layout` word boxes) and visual
//! pixel diffing (with PDFium `pdf.render` page images), generating a JSON report and an
//! annotated PDF report with Phase 8 annotations.

use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    tools::{
        PageRange, ToolSpec,
        page_range::DuplicatePolicy,
        params::{Background, Combine, CompareMode, CompareParams, RasterFormat, RenderParams},
    },
};
pub use ariad_pdf::compare::{MAX_DIFF_BOXES, MAX_PAGE_PIXELS, MAX_VISUAL_PAGES};
use ariad_pdf::{
    ToolCtx,
    compare::{
        CompareReport, CompareSummary, PageCompareResult, PageStatus, VisualChange, WordItem,
        build_report_pdf, diff_words, extract_words_from_reader, visual_diff_images,
    },
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use zip::ZipArchive;

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

/// Preflights `pdf.compare` by verifying that required engines are available.
pub fn preflight(
    params_value: &Value,
    engine_program: Option<&Path>,
    engine_command: Option<&EngineCommand>,
) -> Result<(), ToolError> {
    let params: CompareParams = serde_json::from_value(params_value.clone())
        .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
    params.validate().map_err(ToolError::InvalidParams)?;

    if matches!(params.mode, CompareMode::Visual | CompareMode::Both) {
        let _ = engine::resolve_engine_command("pdfium", engine_program, engine_command)?;
    }

    Ok(())
}

fn parse_page_no_from_name(name: &str) -> Option<u32> {
    let stem = name.split('.').next_back().map(|_| {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() >= 2 {
            parts[parts.len() - 2]
        } else {
            parts[0]
        }
    })?;
    stem.split('-')
        .next_back()
        .and_then(|s| s.parse::<u32>().ok())
}

#[allow(clippy::too_many_arguments)]
fn render_pages_to_image_files(
    staged_input: &(PathBuf, String),
    pages: Option<&PageRange>,
    subdir: &str,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<BTreeMap<u32, PathBuf>, ToolError> {
    let render_spec = *ariad_core::tools::find("pdf.render")
        .ok_or_else(|| ToolError::UnknownTool("pdf.render".into()))?;

    let render_params = RenderParams {
        pages: pages.cloned().unwrap_or_else(PageRange::all),
        dpi: 150,
        format: RasterFormat::Png,
        quality: None,
        background: Background::White,
        annotations: true,
        combine: Combine::None,
    };
    let render_params_value =
        serde_json::to_value(render_params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

    let render_exec = engine::execute(
        &render_spec,
        "pdfium",
        std::slice::from_ref(staged_input),
        &render_params_value,
        false,
        workspace,
        limits,
        scrubber,
        &crate::tools::secrets::SecretBag::new(),
        engine_program,
        engine_command_override,
        cancel.clone(),
        on_event,
    )?;

    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    let mut images = BTreeMap::new();
    let extract_dir = workspace.work_dir().join(subdir);
    std::fs::create_dir_all(&extract_dir)
        .map_err(|e| ToolError::OutputIo(format!("could not create {subdir} directory: {e}")))?;

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
                    let page_no = parse_page_no_from_name(&name).unwrap_or((i + 1) as u32);
                    let target_path = extract_dir.join(&name);
                    let mut out_file = std::fs::File::create(&target_path).map_err(|e| {
                        ToolError::OutputIo(format!("could not create extracted image file: {e}"))
                    })?;
                    std::io::copy(&mut zip_file, &mut out_file).map_err(|e| {
                        ToolError::OutputIo(format!("could not write extracted image file: {e}"))
                    })?;
                    images.insert(page_no, target_path);
                }
            }
        } else {
            let filename = artifact_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("page-001.png");
            let page_no = parse_page_no_from_name(filename).unwrap_or(1);
            images.insert(page_no, artifact_path.clone());
        }
    }

    Ok(images)
}

#[allow(clippy::too_many_arguments)]
fn extract_words_via_pdfium(
    staged_input: &(PathBuf, String),
    pages: Option<&PageRange>,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<BTreeMap<u32, Vec<WordItem>>, ToolError> {
    let spec = *ariad_core::tools::find("pdf.text_layout")
        .ok_or_else(|| ToolError::UnknownTool("pdf.text_layout".into()))?;

    let params_val = json!({
        "pages": pages.map(PageRange::as_str),
    });

    let exec = engine::execute(
        &spec,
        "pdfium",
        std::slice::from_ref(staged_input),
        &params_val,
        false,
        workspace,
        limits,
        scrubber,
        &crate::tools::secrets::SecretBag::new(),
        engine_program,
        engine_command_override,
        cancel.clone(),
        on_event,
    )?;

    let mut words_map = BTreeMap::new();
    let Some((artifact_path, _)) = exec.artifacts.first() else {
        return Ok(words_map);
    };

    let mut file = workspace
        .open_artifact(artifact_path)
        .map_err(|_| ToolError::EngineFailure("could not open text_layout artifact".into()))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|_| ToolError::EngineFailure("could not read text_layout artifact".into()))?;

    let json_doc: Value = serde_json::from_slice(&data)
        .map_err(|e| ToolError::EngineFailure(format!("invalid text_layout JSON: {e}")))?;

    if let Some(pages_arr) = json_doc.get("pages").and_then(Value::as_array) {
        for page_val in pages_arr {
            let page_no = page_val.get("page").and_then(Value::as_u64).unwrap_or(1) as u32;
            let mut words = Vec::new();
            if let Some(words_arr) = page_val.get("words").and_then(Value::as_array) {
                for w_val in words_arr {
                    let text = w_val.get("text").and_then(Value::as_str).unwrap_or("");
                    if let Some(bbox_arr) = w_val.get("bbox").and_then(Value::as_array)
                        && bbox_arr.len() == 4
                    {
                        let left = bbox_arr[0].as_f64().unwrap_or(0.0);
                        let bottom = bbox_arr[1].as_f64().unwrap_or(0.0);
                        let right = bbox_arr[2].as_f64().unwrap_or(0.0);
                        let top = bbox_arr[3].as_f64().unwrap_or(0.0);
                        words.push(WordItem {
                            text: text.to_string(),
                            bbox: [left, bottom, right, top],
                        });
                    }
                }
            }
            words_map.insert(page_no, words);
        }
    }

    Ok(words_map)
}

/// Executes `pdf.compare` as a host composite tool.
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
    clock: ariad_pdf::ToolClock,
) -> Result<RawToolExecution, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    if staged_inputs.len() != 2 {
        return Err(ToolError::Usage(format!(
            "pdf.compare requires exactly 2 input PDF documents (before and after), got {}",
            staged_inputs.len()
        )));
    }

    let params: CompareParams = serde_json::from_value(params_value.clone())
        .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
    params.validate().map_err(ToolError::InvalidParams)?;

    let bytes_before = std::fs::read(&staged_inputs[0].0)
        .map_err(|e| ToolError::InputIo(format!("could not read before PDF: {e}")))?;
    let bytes_after = std::fs::read(&staged_inputs[1].0)
        .map_err(|e| ToolError::InputIo(format!("could not read after PDF: {e}")))?;

    let read_before = ariad_core::reader::pdf::read(&bytes_before, limits)
        .map_err(|_| ToolError::EngineFailure("failed to read before PDF".into()))?;
    let read_after = ariad_core::reader::pdf::read(&bytes_after, limits)
        .map_err(|_| ToolError::EngineFailure("failed to read after PDF".into()))?;

    let pages_before = read_before
        .document
        .layout
        .as_ref()
        .map(|l| l.pages.len())
        .unwrap_or(0)
        .max(
            ariad_core::reader::pdf::PdfStats::from_document(&read_before.document)
                .map(|s| s.pages as usize)
                .unwrap_or(1),
        );
    let pages_after = read_after
        .document
        .layout
        .as_ref()
        .map(|l| l.pages.len())
        .unwrap_or(0)
        .max(
            ariad_core::reader::pdf::PdfStats::from_document(&read_after.document)
                .map(|s| s.pages as usize)
                .unwrap_or(1),
        );
    let max_pages = pages_before.max(pages_after).max(1);

    let page_numbers = if let Some(ref range) = params.pages {
        range
            .expand(max_pages as u32, DuplicatePolicy::Reject)
            .map_err(|e| ToolError::InvalidParams(e.to_string()))?
    } else {
        (1..=max_pages as u32).collect()
    };

    let has_pdfium =
        engine::resolve_engine_command("pdfium", engine_program, engine_command_override).is_ok();

    // 1. Text diffing
    let mut words_before_map = BTreeMap::new();
    let mut words_after_map = BTreeMap::new();

    let do_text = matches!(params.mode, CompareMode::Text | CompareMode::Both);
    if do_text {
        if has_pdfium {
            if let Ok(w_before) = extract_words_via_pdfium(
                &staged_inputs[0],
                params.pages.as_ref(),
                workspace,
                limits,
                scrubber,
                engine_program,
                engine_command_override,
                cancel,
                on_event,
            ) {
                words_before_map = w_before;
            }
            if let Ok(w_after) = extract_words_via_pdfium(
                &staged_inputs[1],
                params.pages.as_ref(),
                workspace,
                limits,
                scrubber,
                engine_program,
                engine_command_override,
                cancel,
                on_event,
            ) {
                words_after_map = w_after;
            }
        }

        // Fallback to in-process extraction if pdfium was unavailable or returned empty
        for &p in &page_numbers {
            if !words_before_map.contains_key(&p) && (p as usize) <= pages_before {
                let words = extract_words_from_reader(&read_before.document, p, limits);
                words_before_map.insert(p, words);
            }
            if !words_after_map.contains_key(&p) && (p as usize) <= pages_after {
                let words = extract_words_from_reader(&read_after.document, p, limits);
                words_after_map.insert(p, words);
            }
        }
    }

    let do_visual = matches!(params.mode, CompareMode::Visual | CompareMode::Both);
    if do_visual && page_numbers.len() > MAX_VISUAL_PAGES {
        return Err(ToolError::InvalidParams(format!(
            "visual comparison page count {} exceeds maximum limit of {MAX_VISUAL_PAGES}",
            page_numbers.len()
        )));
    }

    // 2. Visual diffing
    let mut image_files_before = BTreeMap::new();
    let mut image_files_after = BTreeMap::new();

    if do_visual {
        image_files_before = render_pages_to_image_files(
            &staged_inputs[0],
            params.pages.as_ref(),
            "render-before",
            workspace,
            limits,
            scrubber,
            engine_program,
            engine_command_override,
            cancel,
            on_event,
        )?;
        image_files_after = render_pages_to_image_files(
            &staged_inputs[1],
            params.pages.as_ref(),
            "render-after",
            workspace,
            limits,
            scrubber,
            engine_program,
            engine_command_override,
            cancel,
            on_event,
        )?;
    }

    let mut page_results = Vec::with_capacity(page_numbers.len());
    let mut total_text_changes = 0usize;
    let mut total_visual_changes = 0usize;
    let mut identical_count = 0usize;
    let mut changed_count = 0usize;

    for &page_no in &page_numbers {
        if cancel.is_cancelled() {
            return Err(ToolError::Interrupted);
        }

        let has_before = (page_no as usize) <= pages_before;
        let has_after = (page_no as usize) <= pages_after;

        let b_img = if do_visual {
            if let Some(path) = image_files_before.remove(&page_no) {
                let bytes = std::fs::read(&path).map_err(|e| {
                    ToolError::EngineFailure(format!("could not read rendered image: {e}"))
                })?;
                Some(image::load_from_memory(&bytes).map_err(|e| {
                    ToolError::EngineFailure(format!("could not decode rendered image: {e}"))
                })?)
            } else {
                None
            }
        } else {
            None
        };

        let a_img = if do_visual {
            if let Some(path) = image_files_after.remove(&page_no) {
                let bytes = std::fs::read(&path).map_err(|e| {
                    ToolError::EngineFailure(format!("could not read rendered image: {e}"))
                })?;
                Some(image::load_from_memory(&bytes).map_err(|e| {
                    ToolError::EngineFailure(format!("could not decode rendered image: {e}"))
                })?)
            } else {
                None
            }
        } else {
            None
        };

        if has_before && has_after {
            let text_changes = if do_text {
                let bw = words_before_map
                    .get(&page_no)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let aw = words_after_map
                    .get(&page_no)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                diff_words(bw, aw)
            } else {
                Vec::new()
            };

            let visual_changes = if let (Some(b), Some(a)) = (&b_img, &a_img) {
                visual_diff_images(b, a, 150.0).map_err(ToolError::EngineFailure)?
            } else {
                Vec::new()
            };

            let is_identical = text_changes.is_empty() && visual_changes.is_empty();
            if is_identical {
                identical_count += 1;
            } else {
                changed_count += 1;
            }
            total_text_changes += text_changes.len();
            total_visual_changes += visual_changes.len();

            page_results.push(PageCompareResult {
                page_number: page_no,
                page_before: Some(page_no),
                page_after: Some(page_no),
                status: if is_identical {
                    PageStatus::Identical
                } else {
                    PageStatus::Changed
                },
                text_changes,
                visual_changes,
            });
        } else if !has_before && has_after {
            changed_count += 1;
            let visual_changes = if let Some(a) = &a_img {
                let w = a.width() as f64 * (72.0 / 150.0);
                let h = a.height() as f64 * (72.0 / 150.0);
                vec![VisualChange {
                    bbox: [0.0, 0.0, w, h],
                    diff_pixels: (a.width() * a.height()) as usize,
                }]
            } else {
                Vec::new()
            };
            total_visual_changes += visual_changes.len();

            page_results.push(PageCompareResult {
                page_number: page_no,
                page_before: None,
                page_after: Some(page_no),
                status: PageStatus::Added,
                text_changes: Vec::new(),
                visual_changes,
            });
        } else if has_before && !has_after {
            changed_count += 1;
            page_results.push(PageCompareResult {
                page_number: page_no,
                page_before: Some(page_no),
                page_after: None,
                status: PageStatus::Removed,
                text_changes: Vec::new(),
                visual_changes: Vec::new(),
            });
        }
    }

    let summary = CompareSummary {
        pages_before,
        pages_after,
        pages_compared: page_results.len(),
        pages_identical: identical_count,
        pages_changed: changed_count,
        total_changes: total_text_changes + total_visual_changes,
        text_changes: total_text_changes,
        visual_changes: total_visual_changes,
    };

    let report = CompareReport {
        summary,
        pages: page_results,
    };

    let ctx = ToolCtx::default().with_clock(clock);
    let report_pdf_bytes = build_report_pdf(&bytes_after, &report, limits, &ctx)
        .map_err(|e| ToolError::EngineFailure(format!("failed to build report PDF: {e}")))?;

    let json_bytes = serde_json::to_vec_pretty(&report)
        .map_err(|e| ToolError::EngineFailure(format!("failed to serialize report JSON: {e}")))?;

    let json_path = workspace.output_dir().join("report.json");
    std::fs::write(&json_path, &json_bytes)
        .map_err(|e| ToolError::OutputIo(format!("could not write report.json: {e}")))?;

    let pdf_path = workspace.output_dir().join("report.pdf");
    std::fs::write(&pdf_path, &report_pdf_bytes)
        .map_err(|e| ToolError::OutputIo(format!("could not write report.pdf: {e}")))?;

    let inline_json = serde_json::to_value(&report).ok();

    Ok(RawToolExecution {
        artifacts: vec![(json_path, "json".to_owned()), (pdf_path, "pdf".to_owned())],
        warnings: Vec::new(),
        metrics: Default::default(),
        inline_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_checks_pdfium_availability() {
        let dummy = EngineCommand::builtin(Path::new("/bin/echo"), "pdfium");

        // text mode does not require pdfium
        let text_params = json!({"mode": "text"});
        assert!(preflight(&text_params, None, None).is_ok());

        // visual mode with explicit override succeeds
        let visual_params = json!({"mode": "visual"});
        assert!(preflight(&visual_params, None, Some(&dummy)).is_ok());

        // both mode with explicit override succeeds
        let both_params = json!({"mode": "both"});
        assert!(preflight(&both_params, None, Some(&dummy)).is_ok());
    }

    #[test]
    fn visual_mode_exceeding_max_visual_pages_fails() {
        let mut doc = lopdf::Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut page_ids = Vec::new();
        for _ in 0..=MAX_VISUAL_PAGES {
            let page_id = doc.add_object(lopdf::Dictionary::from_iter(vec![
                ("Type", lopdf::Object::Name(b"Page".to_vec())),
                ("Parent", lopdf::Object::Reference(pages_id)),
                (
                    "MediaBox",
                    lopdf::Object::Array(vec![0.into(), 0.into(), 100.into(), 100.into()]),
                ),
            ]));
            page_ids.push(lopdf::Object::Reference(page_id));
        }
        let pages_dict = lopdf::Dictionary::from_iter(vec![
            ("Type", lopdf::Object::Name(b"Pages".to_vec())),
            (
                "Count",
                lopdf::Object::Integer((MAX_VISUAL_PAGES + 1) as i64),
            ),
            ("Kids", lopdf::Object::Array(page_ids)),
        ]);
        doc.set_object(pages_id, pages_dict);
        let catalog_id = doc.add_object(lopdf::Dictionary::from_iter(vec![
            ("Type", lopdf::Object::Name(b"Catalog".to_vec())),
            ("Pages", lopdf::Object::Reference(pages_id)),
        ]));
        doc.trailer
            .set("Root", lopdf::Object::Reference(catalog_id));

        let temp_dir = tempfile::tempdir().unwrap();
        let pdf_path = temp_dir.path().join("doc.pdf");
        doc.save(&pdf_path).unwrap();

        let spec = ariad_core::tools::find("pdf.compare").unwrap();
        let staged_inputs = vec![
            (pdf_path.clone(), "pdf".to_string()),
            (pdf_path, "pdf".to_string()),
        ];
        let ws = Workspace::new().unwrap();
        let limits = Limits::local();
        let scrubber = SecretScrubber::default();
        let cancel = CancellationToken::new();
        let dummy = EngineCommand::builtin(Path::new("/bin/echo"), "pdfium");

        let params_val = json!({
            "mode": "visual"
        });

        let res = execute(
            spec,
            &staged_inputs,
            &params_val,
            &ws,
            &limits,
            &scrubber,
            None,
            Some(&dummy),
            &cancel,
            &mut |_| {},
            ariad_pdf::ToolClock {
                epoch_secs: 0,
                utc_offset_secs: 0,
            },
        );

        assert!(res.is_err(), "must fail when page count > MAX_VISUAL_PAGES");
        let err = res.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("200") && err_str.contains("exceeds"),
            "error message must name the limit (200) and indicate exceeds: {err_str}"
        );
    }
}
