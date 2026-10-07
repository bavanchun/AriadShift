//! Plan command implementation.

use std::path::Path;

pub use ariad_core::planner::{PlanScore, PlanStep, Profile};
use ariad_core::{
    format::Format,
    limits::Limits,
    planner::{self, PlanError},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    commands::inspect::{classify_file, count_document},
    convert::{ConvertError, DocumentFormat},
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
}

fn reachable_from_targets(reachable: &[Format]) -> String {
    let mut targets: Vec<&'static str> = reachable
        .iter()
        .copied()
        .filter(|&f| f != Format::AriadIrJson && f != Format::PandocJson)
        .map(|f| f.id())
        .collect();
    targets.sort_unstable();
    targets.dedup();
    targets.join(", ")
}

fn check_route_engines(steps: &[PlanStep], pandoc_available: bool) -> (bool, Option<String>) {
    let missing = steps.iter().find_map(|step| {
        if step.engine == crate::engines::IN_PROCESS_ENGINE {
            None
        } else if step.engine == "pandoc" {
            if pandoc_available {
                None
            } else {
                Some("pandoc".to_owned())
            }
        } else {
            Some(step.engine.clone())
        }
    });
    (missing.is_none(), missing)
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
            let res = crate::convert::read_document_to_ir(
                path,
                crate::convert::DocumentFormat::Markdown,
                &Limits::local(),
            )?;
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
            let res = crate::convert::read_document_to_ir(
                path,
                crate::convert::DocumentFormat::Html,
                &Limits::local(),
            )?;
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

/// Generates a conversion plan from an input file to a target format.
pub fn plan(
    path: &Path,
    target: &str,
    profile: Profile,
    caps: &planner::Capabilities,
    engine_program: Option<&Path>,
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
    if input_format == Format::Docx || input_format == Format::Epub {
        crate::archive::preflight_archive(path, &Limits::local())?;
    }

    let target_format =
        DocumentFormat::parse(target).ok_or_else(|| ConvertError::UnsupportedRoute {
            detail: Some(format!("unknown target format '{target}'")),
        })?;
    let to_fmt = Format::from(target_format);

    let registered_engines = caps.registered_engines();

    if input_format == to_fmt {
        let reachable =
            planner::reachable_formats(caps, input_format, profile, &registered_engines);
        let detail = format!(
            "reachable targets from {}: {}",
            input_format.id(),
            reachable_from_targets(&reachable)
        );
        return Err(ConvertError::UnsupportedRoute {
            detail: Some(detail),
        });
    }

    let plan = match planner::plan(caps, input_format, to_fmt, profile, &registered_engines) {
        Ok(p) => p,
        Err(PlanError::NoRoute { reachable, .. }) => {
            let detail = format!(
                "no reachable targets from {}: {}",
                input_format.id(),
                reachable_from_targets(&reachable)
            );
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(detail),
            });
        }
        Err(e) => {
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(e.to_string()),
            });
        }
    };

    let summary = build_input_summary(path, input_format)?;

    let pandoc_available = {
        let default_exe = std::env::current_exe().ok();
        let ep = engine_program.or(default_exe.as_deref());
        if let Some(ep) = ep {
            match crate::engines::availability(ep, &[], cancel.clone()) {
                Ok(report) => report
                    .engines
                    .iter()
                    .any(|e| e.engine == "pandoc" && e.available),
                Err(crate::runner::RunError::Cancelled) => return Err(ConvertError::Interrupted),
                Err(_) => false,
            }
        } else {
            crate::pandoc_bin::locate_with_cancel(Some(&cancel)).is_ok()
        }
    };

    let (runs_locally, missing_engine) = check_route_engines(&plan.steps, pandoc_available);

    let raw_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let input_name = crate::convert::sanitize_identifier(&raw_name, usize::MAX);

    let alternatives = plan
        .alternatives
        .into_iter()
        .map(|alt| {
            let (alt_locally, alt_missing) = check_route_engines(&alt.steps, pandoc_available);
            PlanAlternative {
                route: alt.steps,
                score: alt.score,
                measured: alt.measured,
                runs_locally: alt_locally,
                missing_engine: alt_missing,
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
    })
}
