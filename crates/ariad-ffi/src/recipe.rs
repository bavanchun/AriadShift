//! Recipe validation and execution for the native app.
//!
//! `validate_recipe` checks a recipe against `schemas/recipe.v1.json` and then against the host's
//! semantic rules. `run_recipe` drives the host recipe runner and reports each step through a
//! [`RecipeListener`].

use std::{path::PathBuf, sync::Arc, sync::LazyLock};

use ariad_core::tools::{Recipe, RecipeOrigin, Step, generated_recipe_schema};
use ariad_host::tools::{
    InputRef, RecipeEvent, RecipeRequest, ToolEvent, run_recipe_with_step_events,
};
use futures_util::FutureExt;
use tokio_util::sync::CancellationToken;

use crate::{CancelToken, FfiError, FfiWarning, catch_panics, engine_program, panic_message};

static RECIPE_VALIDATOR: LazyLock<Result<jsonschema::Validator, String>> = LazyLock::new(|| {
    let schema: serde_json::Value = serde_json::from_str(&generated_recipe_schema())
        .map_err(|e| format!("recipe schema is invalid JSON: {e}"))?;
    jsonschema::validator_for(&schema).map_err(|e| format!("recipe schema failed to compile: {e}"))
});

/// One step of a validated recipe.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RecipeStepSummary {
    /// `tool` or `convert`.
    pub kind: String,
    /// Tool id or target format id.
    pub target: String,
}

/// Summary of a recipe that passed validation.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RecipeSummary {
    pub version: String,
    pub naming: Option<String>,
    pub steps: Vec<RecipeStepSummary>,
}

fn origin(imported: bool) -> RecipeOrigin {
    if imported {
        RecipeOrigin::Imported
    } else {
        RecipeOrigin::Trusted
    }
}

fn step_summary(step: &Step) -> RecipeStepSummary {
    match step {
        Step::Tool { tool, .. } => RecipeStepSummary {
            kind: "tool".to_string(),
            target: tool.to_string(),
        },
        Step::Convert { to, .. } => RecipeStepSummary {
            kind: "convert".to_string(),
            target: to.user_name().to_string(),
        },
    }
}

/// Validates a recipe document against `schemas/recipe.v1.json` and the host's recipe rules.
///
/// `imported` is `true` for a recipe loaded from a file: it may not configure watch-folder
/// automation and needs explicit confirmation before it runs. Every schema violation is listed
/// in the error message.
#[uniffi::export]
pub fn validate_recipe(recipe_json: String, imported: bool) -> Result<RecipeSummary, FfiError> {
    catch_panics(|| {
        let validator = RECIPE_VALIDATOR
            .as_ref()
            .map_err(|message| FfiError::Internal {
                message: message.clone(),
            })?;
        let document: serde_json::Value =
            serde_json::from_str(&recipe_json).map_err(|e| FfiError::InvalidRequest {
                message: format!("recipe is not valid JSON: {e}"),
            })?;
        let violations: Vec<String> = validator
            .iter_errors(&document)
            .map(|e| format!("{}: {e}", e.instance_path()))
            .collect();
        if !violations.is_empty() {
            return Err(FfiError::InvalidRequest {
                message: format!("recipe violates schema: {}", violations.join("; ")),
            });
        }
        let recipe = Recipe::from_json_str(&recipe_json, origin(imported)).map_err(|e| {
            FfiError::InvalidRequest {
                message: e.to_string(),
            }
        })?;
        Ok(RecipeSummary {
            version: recipe.version.clone(),
            naming: recipe.naming.clone(),
            steps: recipe.steps.iter().map(step_summary).collect(),
        })
    })
}

/// Request parameters for `run_recipe`.
#[derive(Clone, Debug, uniffi::Record)]
pub struct RecipeRunRequest {
    pub recipe_json: String,
    pub inputs: Vec<String>,
    pub output_dir: String,
    /// `true` when the recipe was loaded from a file (see [`validate_recipe`]).
    pub imported: bool,
    /// The user explicitly confirmed running an imported recipe.
    pub confirmed: bool,
    pub overwrite: bool,
    pub cancel_token: Option<Arc<CancelToken>>,
}

/// Result of one executed step.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RecipeStepOutcome {
    pub step_index: u32,
    pub kind: String,
    pub target: String,
    pub artifacts: u32,
    pub elapsed_ms: u64,
}

/// Outcome of a completed recipe run.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RecipeRunOutcome {
    pub version: String,
    pub steps: Vec<RecipeStepOutcome>,
    pub output_paths: Vec<String>,
    pub warnings: Vec<FfiWarning>,
    /// Host recipe metrics (`steps`, `artifacts`, `elapsed_ms`) as JSON.
    pub metrics_json: String,
}

/// Callback interface for recipe step events.
///
/// A failing step reports `on_step_failed` once and the run then returns the error. A cancelled
/// run reports no failure; it returns [`FfiError::Cancelled`].
#[uniffi::export(callback_interface)]
pub trait RecipeListener: Send + Sync + std::fmt::Debug {
    fn on_step_started(&self, step_index: u32, total: u32, kind: String, target: String);
    fn on_progress(&self, step_index: u32, stage: String, done: u64, total: u64);
    fn on_step_finished(&self, step: RecipeStepOutcome);
    fn on_step_failed(&self, step_index: u32, code: String, message: String);
}

struct RecipeListenerHandle(Box<dyn RecipeListener>);
impl std::panic::UnwindSafe for RecipeListenerHandle {}
impl std::panic::RefUnwindSafe for RecipeListenerHandle {}

fn step_outcome(step: &ariad_host::tools::StepOutcome) -> RecipeStepOutcome {
    RecipeStepOutcome {
        step_index: u32::try_from(step.step_index).unwrap_or(u32::MAX),
        kind: step.kind.clone(),
        target: step.target.clone(),
        artifacts: u32::try_from(step.artifacts).unwrap_or(u32::MAX),
        elapsed_ms: step.elapsed_ms,
    }
}

fn failure_parts(error: &FfiError) -> (String, String) {
    match error {
        FfiError::Tool { code, message } => (code.clone(), message.clone()),
        FfiError::UnknownTool { id } => ("unknown_tool".to_string(), format!("unknown tool: {id}")),
        FfiError::InvalidRequest { message } => ("invalid_params".to_string(), message.clone()),
        other => ("internal".to_string(), other.to_string()),
    }
}

fn build_recipe_request(
    request: &RecipeRunRequest,
    cancel: CancellationToken,
) -> Result<RecipeRequest, FfiError> {
    let recipe =
        Recipe::from_json_str(&request.recipe_json, origin(request.imported)).map_err(|e| {
            FfiError::InvalidRequest {
                message: e.to_string(),
            }
        })?;
    let inputs = request
        .inputs
        .iter()
        .map(|p| InputRef::from(p.as_str()))
        .collect();
    let mut recipe_request = RecipeRequest::new(recipe, inputs, PathBuf::from(&request.output_dir))
        .with_origin(origin(request.imported), request.confirmed)
        .with_overwrite(request.overwrite)
        .with_cancel(cancel);
    if let Some(program) = engine_program() {
        recipe_request = recipe_request.with_engine_program(program);
    }
    Ok(recipe_request)
}

/// Runs a recipe and reports each step through `listener`.
#[uniffi::export]
pub async fn run_recipe(
    request: RecipeRunRequest,
    listener: Option<Box<dyn RecipeListener>>,
) -> Result<RecipeRunOutcome, FfiError> {
    let cancel = request
        .cancel_token
        .as_ref()
        .map(|t| t.token().child_token())
        .unwrap_or_default();
    if cancel.is_cancelled() {
        return Err(FfiError::Cancelled);
    }

    let fut = std::panic::AssertUnwindSafe(async move {
        let guard = cancel.clone().drop_guard();
        let recipe_request = build_recipe_request(&request, cancel.clone())?;
        let listener_handle = listener.map(RecipeListenerHandle);

        let (tx, rx) = tokio::sync::oneshot::channel();
        let spawn_res = std::thread::Builder::new()
            .name("ariad-ffi-recipe".to_string())
            .spawn(move || {
                let result = catch_panics(move || {
                    let mut current_step = 0_u32;
                    let mut started_any = false;
                    let outcome = run_recipe_with_step_events(&recipe_request, |event| {
                        if let RecipeEvent::StepStarted { step_index, .. } = &event {
                            current_step = u32::try_from(*step_index).unwrap_or(u32::MAX);
                            started_any = true;
                        }
                        let Some(ref listener) = listener_handle else {
                            return;
                        };
                        match event {
                            RecipeEvent::StepStarted {
                                total,
                                kind,
                                target,
                                ..
                            } => listener.0.on_step_started(
                                current_step,
                                u32::try_from(total).unwrap_or(u32::MAX),
                                kind,
                                target,
                            ),
                            RecipeEvent::Tool(ToolEvent::Progress { stage, done, total }) => {
                                listener.0.on_progress(
                                    current_step,
                                    stage,
                                    done.unwrap_or(0),
                                    total.unwrap_or(0),
                                );
                            }
                            RecipeEvent::StepFinished(step) => {
                                listener.0.on_step_finished(step_outcome(&step));
                            }
                        }
                    });
                    match outcome {
                        Ok(outcome) => Ok(outcome),
                        Err(error) => {
                            let error = FfiError::from(error);
                            if started_any
                                && error != FfiError::Cancelled
                                && let Some(ref listener) = listener_handle
                            {
                                let (code, message) = failure_parts(&error);
                                listener.0.on_step_failed(current_step, code, message);
                            }
                            Err(error)
                        }
                    }
                });
                let _ = tx.send(result);
            });
        if let Err(e) = spawn_res {
            return Err(FfiError::Internal {
                message: format!("failed to spawn recipe worker thread: {e}"),
            });
        }

        let received = rx.await;
        guard.disarm();
        let outcome = received.map_err(|_| FfiError::Internal {
            message: "recipe worker thread terminated prematurely".to_string(),
        })??;

        let metrics_json =
            serde_json::to_string(&outcome.metrics).map_err(|e| FfiError::Internal {
                message: format!("failed to serialize recipe metrics: {e}"),
            })?;
        Ok(RecipeRunOutcome {
            version: outcome.version,
            steps: outcome.steps.iter().map(step_outcome).collect(),
            output_paths: outcome
                .artifacts
                .into_iter()
                .map(|a| a.path.display().to_string())
                .collect(),
            warnings: outcome
                .warnings
                .into_iter()
                .map(|w| FfiWarning {
                    code: w.code,
                    message: w.message,
                })
                .collect(),
            metrics_json,
        })
    });

    match fut.catch_unwind().await {
        Ok(res) => res,
        Err(payload) => Err(FfiError::Internal {
            message: panic_message(&*payload),
        }),
    }
}
