//! Plan command implementation.

use std::path::Path;

pub use ariad_core::planner::{PlanScore, PlanStep, Profile};
use ariad_core::{format::Format, limits::Limits, planner};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    commands::inspect::{classify_file, count_document},
    convert::ConvertError,
    engines::{self, AvailabilityReport, EngineInvocation},
};

/// Alternative conversion route within a plan output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanAlternative {
    pub route: Vec<PlanStep>,
    pub score: PlanScore,
    pub measured: bool,
    #[serde(skip)]
    pub runs_locally: bool,
    #[serde(skip)]
    pub missing_engine: Option<String>,
    #[serde(skip)]
    pub startup_failure: Option<String>,
}

/// Output data structure for the `plan` command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanOutput {
    pub input: String,
    pub route: Vec<PlanStep>,
    pub score: PlanScore,
    pub measured: bool,
    pub alternatives: Vec<PlanAlternative>,
    #[serde(skip)]
    pub input_summary: Option<String>,
    #[serde(skip)]
    pub runs_locally: bool,
    #[serde(skip)]
    pub missing_engine: Option<String>,
    #[serde(skip)]
    pub startup_failure: Option<String>,
}

fn reachable_from_targets(reachable: &[Format]) -> String {
    let mut targets: Vec<&'static str> = reachable
        .iter()
        .copied()
        .filter(|f| f.is_user_facing())
        .map(Format::user_name)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    targets.join(", ")
}

fn check_route_engines(
    steps: &[PlanStep],
    available: Option<&AvailabilityReport>,
) -> (bool, Option<String>, Option<String>) {
    let missing = steps
        .iter()
        .find(|step| !available.is_some_and(|report| report.is_available(&step.engine)))
        .map(|step| step.engine.clone());
    let startup_failure = missing
        .as_deref()
        .and_then(|engine| available?.get(engine)?.startup_failure.clone());
    (missing.is_none(), missing, startup_failure)
}

fn build_input_summary(path: &Path, format: Format) -> Result<String, ConvertError> {
    let raw_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "input".to_owned());
    let file_name = crate::convert::sanitize_identifier(&raw_name, 64);

    let file_len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if file_len > crate::convert::PLAN_SUMMARY_MAX_BYTES {
        if format == Format::Markdown {
            crate::convert::validate_utf8_file_streaming(path)?;
        }
        return Ok(format!("{file_name} · {}", format.id()));
    }

    match format {
        Format::Markdown => {
            let res =
                crate::convert::read_document_to_ir(path, Format::Markdown, &Limits::local())?;
            let counts = count_document(&res.document);
            let p_label = if counts.paragraphs == 1 {
                "paragraph"
            } else {
                "paragraphs"
            };
            let w_label = if counts.words == 1 { "word" } else { "words" };
            Ok(format!(
                "{file_name} · markdown · {} {p_label} · {} {w_label}",
                counts.paragraphs, counts.words
            ))
        }
        Format::Html => {
            let res = crate::convert::read_document_to_ir(path, Format::Html, &Limits::local())?;
            let counts = count_document(&res.document);
            let p_label = if counts.paragraphs == 1 {
                "paragraph"
            } else {
                "paragraphs"
            };
            let w_label = if counts.words == 1 { "word" } else { "words" };
            Ok(format!(
                "{file_name} · html · {} {p_label} · {} {w_label}",
                counts.paragraphs, counts.words
            ))
        }
        Format::Pdf => Ok(format!("{file_name} · pdf")),
        Format::Docx => Ok(format!("{file_name} · docx")),
        Format::Epub => Ok(format!("{file_name} · epub")),
        _ => Ok(format!("{file_name} · {}", format.id())),
    }
}

/// Generates a plan with automatic input-reader selection.
pub fn plan(
    path: &Path,
    target: &str,
    profile: Profile,
    caps: &planner::Capabilities,
    engine_program: Option<&Path>,
    cancel: CancellationToken,
) -> Result<PlanOutput, ConvertError> {
    plan_with_engine(path, target, profile, caps, engine_program, None, cancel)
}

/// Generates a conversion plan from an input file to a target format.
pub fn plan_with_engine(
    path: &Path,
    target: &str,
    profile: Profile,
    caps: &planner::Capabilities,
    engine_program: Option<&Path>,
    reader_engine: Option<&str>,
    cancel: CancellationToken,
) -> Result<PlanOutput, ConvertError> {
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let classification = classify_file(path).map_err(|_| ConvertError::InputIo)?;
    let input_format = classification
        .format
        .ok_or_else(|| ConvertError::UnsupportedRoute {
            detail: Some("unknown input format".to_owned()),
        })?;

    // Archive preflight for archives to fail fast with typed errors
    if input_format.is_zip_container() {
        crate::archive::preflight_archive(path, &Limits::local())?;
    }

    let to_fmt = Format::from_user_name(target).ok_or_else(|| ConvertError::UnsupportedRoute {
        detail: Some(format!("unknown target format '{target}'")),
    })?;

    let registered_engines = caps.registered_engines();

    if input_format == to_fmt {
        let reachable =
            planner::reachable_formats(caps, input_format, profile, &registered_engines);
        let detail = format!(
            "reachable targets from {}: {}",
            input_format.user_name(),
            reachable_from_targets(&reachable)
        );
        return Err(ConvertError::UnsupportedRoute {
            detail: Some(detail),
        });
    }

    let default_exe = std::env::current_exe().map_err(|_| ConvertError::Failed)?;
    let program = engine_program.unwrap_or(&default_exe);
    let selected = crate::convert::routing::select(
        path,
        input_format,
        to_fmt,
        profile,
        caps,
        &EngineInvocation::new(program),
        reader_engine,
        &Limits::local(),
        &cancel,
    )?;
    let plan = selected.plan;

    let summary = build_input_summary(path, input_format)?;

    let available = {
        let default_exe = std::env::current_exe().ok();
        match engine_program.or(default_exe.as_deref()) {
            Some(program) => {
                let mut needed: Vec<&str> = plan
                    .steps
                    .iter()
                    .chain(plan.alternatives.iter().flat_map(|alt| alt.steps.iter()))
                    .map(|step| step.engine.as_str())
                    .collect();
                needed.sort_unstable();
                needed.dedup();
                match engines::availability_of(
                    &EngineInvocation::new(program),
                    &needed,
                    cancel.clone(),
                ) {
                    Ok(report) => Some(report),
                    Err(crate::runner::RunError::Cancelled) => {
                        return Err(ConvertError::Interrupted);
                    }
                    Err(_) => None,
                }
            }
            None => None,
        }
    };

    let (runs_locally, missing_engine, startup_failure) =
        check_route_engines(&plan.steps, available.as_ref());

    let raw_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let input_name = crate::convert::sanitize_identifier(&raw_name, usize::MAX);

    let alternatives = plan
        .alternatives
        .into_iter()
        .map(|alt| {
            let (alt_locally, alt_missing, alt_startup) =
                check_route_engines(&alt.steps, available.as_ref());
            PlanAlternative {
                route: alt.steps,
                score: alt.score,
                measured: alt.measured,
                runs_locally: alt_locally,
                missing_engine: alt_missing,
                startup_failure: alt_startup,
            }
        })
        .collect();

    Ok(PlanOutput {
        input: input_name,
        route: plan.steps,
        score: plan.score,
        measured: plan.measured,
        alternatives,
        input_summary: Some(summary),
        runs_locally,
        missing_engine,
        startup_failure,
    })
}
