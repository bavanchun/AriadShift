//! Host-managed composite orchestration for PDF redaction and independent verification.
//!
//! Redaction pipeline:
//! 1. Verify engine availability and pre-check input document (reject encrypted, reject signed
//!    without explicit consent).
//! 2. PDFium (`redact.inspect`) runs out of process to extract character geometry evidence.
//! 3. In-process plan generation and document rewrite via `ariad_pdf::redact`.
//! 4. Full suite of mandatory verification checks on the candidate document:
//!    - PDFium text re-extraction: zero matches.
//!    - Core reader text re-extraction: zero matches.
//!    - Rendered pixel verification: opaque coverage over all targeted regions.
//!    - Image pixel verification: decoded image streams scrubbed.
//!    - Object graph verification: decoded stream payloads and reachable strings scrubbed.
//!    - History verification: absence of incremental updates or previous revisions.
//! 5. Verification status derived: fail-closed if any check fails or is unverifiable.
//! 6. Candidate published atomically into `<workspace>/out/` only when all checks pass.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    tools::{
        ToolSpec,
        params::{
            redact::{
                RedactParams, RedactVerifyParams, RedactionTarget, RedactionVerificationReport,
            },
            redact_internal::{
                RedactGeometry, RedactionCheckStatus, RedactionChecks, RedactionVerificationStatus,
            },
        },
        specs::ocr_scan,
    },
};
use ariad_pdf::{
    ToolCtx,
    redact::{
        Matrix, Region, has_signatures, plan, rewrite_document_with_evidence, verify_core_text,
        verify_image_pixels, verify_object_graph, verify_pdfium_text, verify_raw_literals,
        verify_raw_literals_for_plan, verify_rendered_pixels_with_overlay, verify_rewrite_history,
    },
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    convert::{ConvertWarning, read_bounded_file},
    runner::EngineCommand,
    tools::{
        engine,
        inprocess::{self, CancelBridge, RawToolExecution},
        run::{ToolError, ToolEvent},
        secrets::SecretScrubber,
    },
    workspace::Workspace,
};

pub(super) fn preflight(
    id: &str,
    program: Option<&Path>,
    override_command: Option<&EngineCommand>,
) -> Result<(), ToolError> {
    if override_command.is_some() {
        return Err(ToolError::Usage(
            "engine override is not supported for redaction composites".into(),
        ));
    }
    engine::resolve_engine_command("pdfium", program, None)?;
    if id != "pdf.redact" && id != "pdf.redact_verify" {
        return Err(ToolError::UnknownTool(id.to_owned()));
    }
    Ok(())
}

fn internal_spec(id: &str) -> Result<ToolSpec, ToolError> {
    ocr_scan::internal_specs()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| {
            ToolError::EngineFailure(format!("internal operation '{id}' is unavailable"))
        })
}

struct RedactJob<'a> {
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

impl RedactJob<'_> {
    fn check(&self) -> Result<(), ToolError> {
        self.ctx
            .check()
            .map_err(|e| inprocess::map_pdf_error(e, self.scrubber))
    }

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

    fn inspect_geometry(
        &mut self,
        pdf_path: &Path,
        events: &mut dyn FnMut(ToolEvent),
    ) -> Result<RedactGeometry, ToolError> {
        let spec = internal_spec("redact.inspect")?;
        let output = self.invoke(
            &spec,
            &[(pdf_path.to_owned(), "pdf".into())],
            json!({"pages": "1-"}),
            events,
        )?;

        let mut geometry_bytes = None;
        for (artifact_path, format) in output.artifacts {
            if format == "json" && geometry_bytes.is_none() {
                let bytes = self.read_artifact(&artifact_path, 128 * 1024 * 1024)?;
                let _ = fs::remove_file(&artifact_path);
                geometry_bytes = Some(bytes);
            } else {
                let _ = fs::remove_file(&artifact_path);
            }
        }

        let bytes = geometry_bytes.ok_or_else(|| {
            ToolError::EngineFailure("PDFium did not return geometry evidence".into())
        })?;
        serde_json::from_slice(&bytes).map_err(|_| {
            ToolError::EngineFailure("PDFium returned invalid geometry evidence".into())
        })
    }

    fn render_page(
        &mut self,
        pdf_path: &Path,
        page: u32,
        dpi: u32,
        events: &mut dyn FnMut(ToolEvent),
    ) -> Result<image::RgbaImage, ToolError> {
        let spec = internal_spec("scan.render")?;
        let output = self.invoke(
            &spec,
            &[(pdf_path.to_owned(), "pdf".into())],
            json!({"pages": page.to_string(), "dpi": dpi, "clean": false, "deskew": false}),
            events,
        )?;

        let mut image_bytes = None;
        for (artifact_path, format) in output.artifacts {
            if format == "png" && image_bytes.is_none() {
                let bytes = self.read_artifact(
                    &artifact_path,
                    self.limits.max_decompressed_bytes.min(128 * 1024 * 1024),
                )?;
                let _ = fs::remove_file(&artifact_path);
                image_bytes = Some(bytes);
            } else {
                let _ = fs::remove_file(&artifact_path);
            }
        }

        let bytes = image_bytes.ok_or_else(|| {
            ToolError::EngineFailure("PDFium did not return rendered page".into())
        })?;
        let img = image::load_from_memory(&bytes)
            .map_err(|_| ToolError::EngineFailure("Failed to decode rendered page image".into()))?;
        Ok(img.to_rgba8())
    }
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
    if inputs.len() != 1 || inputs[0].1 != "pdf" {
        return Err(ToolError::WrongInput("expected 1 PDF file".into()));
    }
    let input_path = &inputs[0].0;
    let input_bytes =
        read_bounded_file(input_path, limits.max_input_bytes).map_err(|e| match e {
            crate::convert::ConvertError::LimitExceeded => ToolError::LimitExceeded,
            other => ToolError::InputIo(scrubber.scrub(&other.to_string())),
        })?;

    let bridge = CancelBridge::new(cancel);
    let started = Instant::now();
    let timeout_secs = limits.timeout_s.unwrap_or(600);
    let deadline = started
        .checked_add(Duration::from_secs(timeout_secs))
        .ok_or(ToolError::LimitExceeded)?;
    let mut ctx = inprocess::build_tool_ctx_with_clock(limits, Arc::clone(&bridge.flag), clock);
    ctx.deadline = Some(deadline);

    // Initial admission check: refuse encrypted documents upfront
    match ariad_pdf::document::open_with_ctx(&input_bytes, limits, &ctx) {
        Ok(doc) => {
            if doc.is_encrypted() {
                return Err(ToolError::EncryptedInput {
                    needs_password: false,
                });
            }
        }
        Err(ariad_pdf::PdfError::Encrypted { needs_password }) => {
            return Err(ToolError::EncryptedInput { needs_password });
        }
        Err(err) => return Err(inprocess::map_pdf_error(err, scrubber)),
    }

    let mut job = RedactJob {
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

    match spec.id {
        "pdf.redact" => execute_redact(&mut job, input_path, &input_bytes, params, events),
        "pdf.redact_verify" => execute_verify(&mut job, input_path, &input_bytes, params, events),
        other => Err(ToolError::UnknownTool(other.to_owned())),
    }
}

fn execute_redact(
    job: &mut RedactJob<'_>,
    input_path: &Path,
    input_bytes: &[u8],
    params: &Value,
    events: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    let redact_params: RedactParams = serde_json::from_value(params.clone())
        .map_err(|e| ToolError::InvalidParams(job.scrubber.scrub(&e.to_string())))?;
    redact_params
        .validate()
        .map_err(|e| ToolError::InvalidParams(job.scrubber.scrub(&e.to_string())))?;

    // Check signature permission
    {
        let doc = ariad_pdf::document::open_with_ctx(input_bytes, job.limits, &job.ctx)
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
        if has_signatures(doc.inner(), &job.ctx)
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?
            && !redact_params.invalidate_signatures
        {
            return Err(ToolError::Usage(
                "redaction would invalidate existing signatures; explicit confirmation is required"
                    .into(),
            ));
        }
    }

    // Step 1: Extract private PDFium character geometry evidence out of process
    let geometry = job.inspect_geometry(input_path, events)?;

    // Step 2: Build redaction plan in-process
    let plan = plan(input_bytes, &redact_params, &geometry, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

    // Step 3: Rewrite document in-process into a candidate
    let (candidate_bytes, carrier_evidence) = rewrite_document_with_evidence(
        input_bytes,
        &redact_params,
        &plan,
        &geometry,
        job.limits,
        &job.ctx,
    )
    .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

    // Stage candidate into private workspace `<ws>/in/candidate.pdf`
    let candidate_path = job.workspace.input_dir().join("candidate.pdf");
    fs::write(&candidate_path, &candidate_bytes)
        .map_err(|e| ToolError::OutputIo(job.scrubber.scrub(&e.to_string())))?;

    // Step 4: Run mandatory verification gates on the candidate
    // If an overlay is specified, produce a clean candidate without overlay text
    // to strictly verify that original text content was removed under every rectangle.
    let (clean_candidate_bytes, clean_geometry) = if redact_params.overlay_text.is_some() {
        let mut unoverlayed_params = redact_params.clone();
        unoverlayed_params.overlay_text = None;
        let (unoverlayed_bytes, _) = rewrite_document_with_evidence(
            input_bytes,
            &unoverlayed_params,
            &plan,
            &geometry,
            job.limits,
            &job.ctx,
        )
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
        let unoverlayed_path = job.workspace.input_dir().join("clean_candidate.pdf");
        fs::write(&unoverlayed_path, &unoverlayed_bytes)
            .map_err(|e| ToolError::OutputIo(job.scrubber.scrub(&e.to_string())))?;
        let unoverlayed_geom = job.inspect_geometry(&unoverlayed_path, events)?;
        (Some(unoverlayed_bytes), Some(unoverlayed_geom))
    } else {
        (None, None)
    };

    // Gate 1: PDFium text re-extraction check
    let candidate_geometry = job.inspect_geometry(&candidate_path, events)?;
    let pdfium_text =
        if let (Some(clean_bytes), Some(clean_geom)) = (&clean_candidate_bytes, &clean_geometry) {
            let mut unoverlayed_params = redact_params.clone();
            unoverlayed_params.overlay_text = None;
            let clean_check = verify_pdfium_text(
                clean_bytes,
                &unoverlayed_params,
                clean_geom,
                job.limits,
                &job.ctx,
            )
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
            if clean_check == RedactionCheckStatus::Fail {
                RedactionCheckStatus::Fail
            } else {
                verify_pdfium_text(
                    &candidate_bytes,
                    &redact_params,
                    &candidate_geometry,
                    job.limits,
                    &job.ctx,
                )
                .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?
            }
        } else {
            verify_pdfium_text(
                &candidate_bytes,
                &redact_params,
                &candidate_geometry,
                job.limits,
                &job.ctx,
            )
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?
        };

    // Gate 2: Core reader text check
    let core_text = if let Some(clean_bytes) = clean_candidate_bytes.as_ref() {
        let mut unoverlayed_params = redact_params.clone();
        unoverlayed_params.overlay_text = None;
        let clean_check = verify_core_text(clean_bytes, &unoverlayed_params, job.limits, &job.ctx)
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
        if clean_check == RedactionCheckStatus::Fail {
            RedactionCheckStatus::Fail
        } else {
            verify_core_text(&candidate_bytes, &redact_params, job.limits, &job.ctx)
                .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?
        }
    } else {
        verify_core_text(&candidate_bytes, &redact_params, job.limits, &job.ctx)
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?
    };

    // Gate 3: Rendered pixel opacity verification for affected pages
    let region_render = verify_candidate_rendering(
        job,
        &candidate_path,
        &candidate_bytes,
        &candidate_geometry,
        &plan,
        &redact_params,
        events,
    )?;

    // Gate 4: Image pixels check
    let image_pixels = verify_image_pixels(&candidate_bytes, &plan, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

    // Gate 5: Object graph scan (decoded streams & reachable strings)
    let object_graph = verify_object_graph(&candidate_bytes, &redact_params, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

    // Gate 6: Full rewrite history check
    let history_ok = verify_rewrite_history(&candidate_bytes, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
    let raw_literals =
        verify_raw_literals_for_plan(&candidate_bytes, &redact_params, &plan, &job.ctx)
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
    let has_text_targets = redact_params.targets.iter().any(|target| {
        matches!(
            target,
            ariad_core::tools::params::redact::RedactionTarget::Exact { .. }
                | ariad_core::tools::params::redact::RedactionTarget::Regex { .. }
                | ariad_core::tools::params::redact::RedactionTarget::Preset { .. }
        )
    });
    let rewrite_history = if !history_ok || raw_literals == RedactionCheckStatus::Fail {
        RedactionCheckStatus::Fail
    } else if raw_literals == RedactionCheckStatus::Unavailable && has_text_targets {
        RedactionCheckStatus::Unavailable
    } else {
        RedactionCheckStatus::Pass
    };

    let checks = RedactionChecks {
        pdfium_text,
        core_text,
        region_render,
        image_pixels,
        object_graph,
        rewrite_history,
    };

    let overall_status = checks.status(redact_params.scope.is_complete(), true, true);

    // Fail closed: never publish an unverified or failed intermediate artifact
    if overall_status == RedactionVerificationStatus::Failed {
        return Err(ToolError::EngineFailure(
            "redaction verification failed".into(),
        ));
    }
    if overall_status == RedactionVerificationStatus::Unverifiable {
        return Err(ToolError::EngineFailure(
            "redaction could not be verified".into(),
        ));
    }

    // Step 5: Publish candidate to output directory
    let staged_output = job.workspace.output_dir().join("redacted.pdf");
    fs::write(&staged_output, &candidate_bytes)
        .map_err(|e| ToolError::OutputIo(job.scrubber.scrub(&e.to_string())))?;

    let mut warnings = Vec::new();
    if plan.invalidates_signatures() {
        warnings.push("Existing digital signatures were invalidated by redaction.".into());
    }

    let report = RedactionVerificationReport {
        status: overall_status,
        scope: redact_params.scope,
        checks,
        target_count: redact_params.targets.len(),
        match_count: plan.match_count(),
        warnings: warnings.clone(),
        carriers: Some(carrier_evidence),
    };

    let mut metrics = BTreeMap::new();
    metrics.insert("target_count".into(), json!(redact_params.targets.len()));
    metrics.insert("match_count".into(), json!(plan.match_count()));
    metrics.insert("status".into(), json!(overall_status));
    metrics.insert(
        "invalidates_signatures".into(),
        json!(plan.invalidates_signatures()),
    );

    let convert_warnings: Vec<ConvertWarning> = warnings
        .into_iter()
        .map(|msg| ConvertWarning {
            code: "signatures_invalidated".into(),
            message: msg,
        })
        .collect();

    Ok(RawToolExecution {
        artifacts: vec![(staged_output, "pdf".into())],
        warnings: convert_warnings,
        metrics,
        inline_json: Some(
            serde_json::to_value(&report).map_err(|e| ToolError::EngineFailure(e.to_string()))?,
        ),
    })
}

fn verify_candidate_rendering(
    job: &mut RedactJob<'_>,
    candidate_path: &Path,
    candidate_bytes: &[u8],
    _geometry: &RedactGeometry,
    plan: &ariad_pdf::redact::RedactionPlan,
    params: &RedactParams,
    events: &mut dyn FnMut(ToolEvent),
) -> Result<RedactionCheckStatus, ToolError> {
    if plan.regions().is_empty() {
        return Ok(RedactionCheckStatus::Pass);
    }

    let color_str = params.color.as_ref().map_or("#000000", |c| c.as_str());
    let mut color = [0u8; 3];
    for (i, byte) in color.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&color_str[1 + i * 2..3 + i * 2], 16)
            .map_err(|_| ToolError::InvalidParams("invalid color hex format".into()))?;
    }
    let has_overlay = params.overlay_text.is_some();

    let doc = ariad_pdf::document::open_with_ctx(candidate_bytes, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
    let pages = doc.inner().get_pages();

    // Group regions by page
    let mut pages_touched = BTreeSet::new();
    for region in plan.regions() {
        match region {
            Region::Page { page } => {
                pages_touched.insert(*page);
            }
            Region::Rectangle { page, .. } => {
                pages_touched.insert(*page);
            }
        }
    }

    for page_num in pages_touched {
        let page_id = pages
            .get(&page_num)
            .copied()
            .ok_or_else(|| ToolError::EngineFailure("page missing in candidate".into()))?;

        let crop = ariad_pdf::xobject::resolve_page_crop_box(doc.inner(), page_id);
        let rotation =
            ariad_pdf::xobject::resolve_page_rotation(doc.inner(), page_id).rem_euclid(360);
        let [llx, lly, urx, ury] = [crop.left, crop.bottom, crop.right, crop.top];
        let [w, h] = [urx - llx, ury - lly];

        let rendered = job.render_page(candidate_path, page_num, 150, events)?;

        let mut regions_on_page = Vec::new();
        for region in plan.regions() {
            match region {
                Region::Page { page } if *page == page_num => {
                    regions_on_page.push([llx, lly, urx, ury]);
                }
                Region::Rectangle { page, bounds } if *page == page_num => {
                    regions_on_page.push(*bounds);
                }
                _ => {}
            }
        }

        let matrix = match rotation {
            90 => Matrix([0.0, h, -w, 0.0, urx, lly]),
            180 => Matrix([-w, 0.0, 0.0, -h, urx, ury]),
            270 => Matrix([0.0, -h, w, 0.0, llx, ury]),
            _ => Matrix([w, 0.0, 0.0, h, llx, lly]),
        };
        let ok = verify_rendered_pixels_with_overlay(
            &rendered,
            matrix,
            &regions_on_page,
            color,
            has_overlay,
            job.limits,
            &job.ctx,
        )
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

        if !ok {
            return Ok(RedactionCheckStatus::Fail);
        }
    }

    Ok(RedactionCheckStatus::Pass)
}

fn execute_verify(
    job: &mut RedactJob<'_>,
    input_path: &Path,
    input_bytes: &[u8],
    params: &Value,
    events: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    let verify_params: RedactVerifyParams = serde_json::from_value(params.clone())
        .map_err(|e| ToolError::InvalidParams(job.scrubber.scrub(&e.to_string())))?;
    verify_params
        .validate()
        .map_err(|e| ToolError::InvalidParams(job.scrubber.scrub(&e.to_string())))?;

    let has_spatial_targets = verify_params.targets.iter().any(|t| {
        matches!(
            t,
            RedactionTarget::Rectangle { .. } | RedactionTarget::Page { .. }
        )
    });

    let redact_equiv = RedactParams {
        targets: verify_params.targets.clone(),
        color: None,
        overlay_text: None,
        scope: verify_params.scope,
        invalidate_signatures: true,
    };

    // Extract geometry for candidate
    let geometry = job.inspect_geometry(input_path, events)?;

    let pdfium_text =
        verify_pdfium_text(input_bytes, &redact_equiv, &geometry, job.limits, &job.ctx)
            .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

    let core_text = verify_core_text(input_bytes, &redact_equiv, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

    let object_graph = verify_object_graph(input_bytes, &redact_equiv, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;

    let history_ok = verify_rewrite_history(input_bytes, job.limits, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
    let raw_literals = verify_raw_literals(input_bytes, &redact_equiv, &job.ctx)
        .map_err(|e| inprocess::map_pdf_error(e, job.scrubber))?;
    let has_text_targets = verify_params.targets.iter().any(|target| {
        matches!(
            target,
            ariad_core::tools::params::redact::RedactionTarget::Exact { .. }
                | ariad_core::tools::params::redact::RedactionTarget::Regex { .. }
                | ariad_core::tools::params::redact::RedactionTarget::Preset { .. }
        )
    });
    let rewrite_history = if !history_ok || raw_literals == RedactionCheckStatus::Fail {
        RedactionCheckStatus::Fail
    } else if raw_literals == RedactionCheckStatus::Unavailable && has_text_targets {
        RedactionCheckStatus::Unavailable
    } else {
        RedactionCheckStatus::Pass
    };

    // Standalone verification cannot verify image pixels or region render without original evidence.
    let image_pixels = RedactionCheckStatus::Unavailable;
    let region_render = RedactionCheckStatus::Unavailable;
    let match_count = 0;

    let checks = RedactionChecks {
        pdfium_text,
        core_text,
        region_render,
        image_pixels,
        object_graph,
        rewrite_history,
    };

    let overall_status = checks.status(
        verify_params.scope.is_complete(),
        has_spatial_targets,
        false, // standalone verification lacks original evidence
    );

    let report = RedactionVerificationReport {
        status: overall_status,
        scope: verify_params.scope,
        checks,
        target_count: verify_params.targets.len(),
        match_count,
        warnings: Vec::new(),
        carriers: None,
    };

    let mut metrics = BTreeMap::new();
    metrics.insert("target_count".into(), json!(verify_params.targets.len()));
    metrics.insert("status".into(), json!(overall_status));

    Ok(RawToolExecution {
        artifacts: Vec::new(),
        warnings: Vec::new(),
        metrics,
        inline_json: Some(
            serde_json::to_value(&report).map_err(|e| ToolError::EngineFailure(e.to_string()))?,
        ),
    })
}
