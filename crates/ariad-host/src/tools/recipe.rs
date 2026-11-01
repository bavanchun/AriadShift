//! Pipeline recipe runner (`ariad-recipe/1`, `docs/tools.md` §9, §12).
//!
//! Validates a [`Recipe`] under its [`RecipeOrigin`] (`Trusted` or `Imported`), enforces explicit
//! user confirmation (`confirmed: true`) on imported recipes, checks Free-plan batch quotas via
//! [`within_run_quota`], executes `Step::Tool` and `Step::Convert` steps sequentially inside a
//! shared isolated [`Workspace`], and atomically publishes only the final step's outputs into the
//! destination directory.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use ariad_core::{
    limits::Limits,
    planner::Profile,
    tools::{
        InputArity, Plan, RECIPE_VERSION, Recipe, RecipeOrigin, RunSize, SecretSource, Step, Tier,
        ToolSpec, access::within_run_quota, validate_secret_params,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    confine::AllowedDirs,
    convert::{ConvertError, ConvertRequest, ConvertWarning, convert_with_limits},
    runner::EngineCommand,
    tools::{
        publish::{
            PublishedArtifact, StagedArtifact, ensure_no_parent_traversal,
            open_confined_output_dir, publish_atomic, render_naming_template,
        },
        run::{
            InputRef, OutputTarget, ToolError, ToolEvent, ToolMetrics, ToolRequest,
            run_tool_with_events,
        },
        secrets::SecretBag,
    },
    workspace::Workspace,
};

/// Host request to execute an `ariad-recipe/1` pipeline.
#[derive(Clone, Debug)]
pub struct RecipeRequest {
    /// Parsed recipe definition.
    pub recipe: Recipe,
    /// Trust origin of the recipe (`Trusted` or `Imported`).
    pub origin: RecipeOrigin,
    /// Whether the user explicitly confirmed running an imported recipe (`--yes`).
    pub confirmed: bool,
    /// Input files to feed into step 0 of the recipe.
    pub inputs: Vec<InputRef>,
    /// Destination directory where final artifacts will be atomically published.
    pub output_dir: PathBuf,
    /// Out-of-band resolved secrets (`SecretBag`).
    pub secrets: SecretBag,
    /// Resource limits enforced on each step.
    pub limits: Limits,
    /// Active plan used for `ToolAccess` and batch quota checks.
    pub plan: Plan,
    /// Whether existing files in `output_dir` may be replaced.
    pub overwrite: bool,
    /// Whether the hosting surface permits `overwrite: true`.
    pub allow_overwrite: bool,
    /// Optional directory confinement policy.
    pub allowed_dirs: Option<AllowedDirs>,
    /// Optional path to the `ashift` binary for resolving built-in engines and packs.
    pub engine_program: Option<PathBuf>,
    /// Optional explicit engine command override (for integration tests).
    pub engine_command: Option<EngineCommand>,
    /// Optional tool specification overrides keyed by tool ID (for integration tests).
    pub spec_overrides: BTreeMap<String, ToolSpec>,
    /// Optional 0-based artifact index at which final atomic publication should inject a failure.
    pub fail_publish_at_index: Option<usize>,
    /// Cooperative cancellation token.
    pub cancel: CancellationToken,
}

impl RecipeRequest {
    /// Creates a new [`RecipeRequest`] with `RecipeOrigin::Trusted`, `Plan::Free`, and `Limits::local()`.
    #[must_use]
    pub fn new(recipe: Recipe, inputs: Vec<InputRef>, output_dir: impl Into<PathBuf>) -> Self {
        Self {
            recipe,
            origin: RecipeOrigin::Trusted,
            confirmed: false,
            inputs,
            output_dir: output_dir.into(),
            secrets: SecretBag::new(),
            limits: Limits::local(),
            plan: Plan::Free,
            overwrite: false,
            allow_overwrite: true,
            allowed_dirs: None,
            engine_program: None,
            engine_command: None,
            spec_overrides: BTreeMap::new(),
            fail_publish_at_index: None,
            cancel: CancellationToken::new(),
        }
    }

    /// Sets the [`RecipeOrigin`] and confirmation flag.
    #[must_use]
    pub fn with_origin(mut self, origin: RecipeOrigin, confirmed: bool) -> Self {
        self.origin = origin;
        self.confirmed = confirmed;
        self
    }

    /// Sets the [`SecretBag`] for this recipe run.
    #[must_use]
    pub fn with_secrets(mut self, secrets: SecretBag) -> Self {
        self.secrets = secrets;
        self
    }

    /// Sets the [`Limits`] for this recipe run.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets the [`Plan`] for tier and batch quota checks.
    #[must_use]
    pub fn with_plan(mut self, plan: Plan) -> Self {
        self.plan = plan;
        self
    }

    /// Sets `overwrite` on this recipe run.
    #[must_use]
    pub fn with_overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }

    /// Sets `allow_overwrite` on this recipe run.
    #[must_use]
    pub fn with_allow_overwrite(mut self, allow_overwrite: bool) -> Self {
        self.allow_overwrite = allow_overwrite;
        self
    }

    /// Confines all input and output paths to `allowed_dirs`.
    #[must_use]
    pub fn with_allowed_dirs(mut self, allowed_dirs: AllowedDirs) -> Self {
        self.allowed_dirs = Some(allowed_dirs);
        self
    }

    /// Sets the `ashift` engine launcher binary path.
    #[must_use]
    pub fn with_engine_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.engine_program = Some(program.into());
        self
    }

    /// Overrides the [`EngineCommand`] used by tool steps.
    #[must_use]
    pub fn with_engine_command(mut self, command: EngineCommand) -> Self {
        self.engine_command = Some(command);
        self
    }

    /// Registers a [`ToolSpec`] override for a step's tool ID (used by integration tests).
    #[must_use]
    pub fn with_spec(mut self, spec: ToolSpec) -> Self {
        self.spec_overrides.insert(spec.id.to_owned(), spec);
        self
    }

    /// Injects a deterministic publication failure at 0-based artifact `index`.
    #[must_use]
    pub fn with_fail_publish_at(mut self, index: usize) -> Self {
        self.fail_publish_at_index = Some(index);
        self
    }

    /// Sets the [`CancellationToken`] for this recipe run.
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }
}

/// Summary of one executed step in a [`RecipeOutcome`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StepOutcome {
    /// 0-based step index.
    pub step_index: usize,
    /// Step kind (`"tool"` or `"convert"`).
    pub kind: String,
    /// Tool ID or target conversion format.
    pub target: String,
    /// Number of intermediate artifacts produced by this step.
    pub artifacts: usize,
    /// Elapsed time of this step in milliseconds.
    pub elapsed_ms: u64,
}

/// Outcome of a completed [`RecipeRequest`] execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecipeOutcome {
    /// Recipe schema version (`"ariad-recipe/1"`).
    pub version: String,
    /// Per-step execution summaries.
    pub steps: Vec<StepOutcome>,
    /// Final atomically published artifacts.
    pub artifacts: Vec<PublishedArtifact>,
    /// Secret-scrubbed warnings collected across all steps.
    pub warnings: Vec<ConvertWarning>,
    /// Aggregate execution metrics.
    pub metrics: ToolMetrics,
}

#[derive(Clone, Debug)]
struct PipelineItem {
    path: PathBuf,
    stem: String,
    format: String,
}

/// Executes a [`RecipeRequest`] synchronously with no event callback.
pub fn run_recipe(request: &RecipeRequest) -> Result<RecipeOutcome, ToolError> {
    run_recipe_with_events(request, |_| {})
}

/// Executes a [`RecipeRequest`] synchronously, streaming [`ToolEvent`]s to `on_event`.
pub fn run_recipe_with_events<F>(
    request: &RecipeRequest,
    mut on_event: F,
) -> Result<RecipeOutcome, ToolError>
where
    F: FnMut(ToolEvent),
{
    let scrubber = request.secrets.scrubber();
    run_recipe_inner(request, &mut on_event).map_err(|e| e.scrubbed(&scrubber))
}

fn run_recipe_inner(
    request: &RecipeRequest,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RecipeOutcome, ToolError> {
    let started = Instant::now();
    let scrubber = request.secrets.scrubber();

    if request.cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    // 1. Validate recipe structure, naming template, secret rules, and imported-recipe restrictions.
    request
        .recipe
        .validate(request.origin)
        .map_err(|e| ToolError::InvalidParams(scrubber.scrub(&e.to_string())))?;
    for step in &request.recipe.steps {
        if let Step::Tool { tool, params } = step
            && let Some(spec) = request.spec_overrides.get(tool.as_str())
        {
            validate_secret_params(spec, params, SecretSource::Recipe)
                .map_err(|e| ToolError::InvalidParams(scrubber.scrub(&e.to_string())))?;
        }
    }

    // 2. Imported recipes require explicit user confirmation (`--yes` on CLI or dialog on UI).
    if request.origin.requires_confirmation() && !request.confirmed {
        return Err(ToolError::InvalidParams(
            "imported recipe requires explicit confirmation (--yes)".to_owned(),
        ));
    }

    // 3. Validate limits and overwrite settings.
    if request.limits.validate().is_err() {
        return Err(ToolError::LimitExceeded);
    }
    if request.overwrite && !request.allow_overwrite {
        return Err(ToolError::Usage(
            "overwriting existing files is not enabled".to_owned(),
        ));
    }
    if request.inputs.is_empty() {
        return Err(ToolError::WrongInput(
            "recipe requires at least one input file".to_owned(),
        ));
    }

    // 4. Validate and confine the destination directory before running any steps.
    ensure_no_parent_traversal(&request.output_dir)?;
    let _ = open_confined_output_dir(&request.output_dir, request.allowed_dirs.as_ref())?;

    // 5. Inspect inputs and check per-run batch quota (`within_run_quota`).
    let mut total_input_bytes = 0_u64;
    let mut input_paths = Vec::with_capacity(request.inputs.len());
    let mut current_items = Vec::with_capacity(request.inputs.len());

    for input_ref in &request.inputs {
        let meta = if let Some(ref allowed) = request.allowed_dirs {
            let confined =
                crate::confine::confine_input(&input_ref.path, allowed).map_err(|err| {
                    ToolError::Confinement(format!(
                        "input '{}' is not permitted: {err}",
                        input_ref.path.display()
                    ))
                })?;
            confined.file.metadata().map_err(|_| {
                ToolError::InputIo(format!(
                    "input file '{}' could not be read",
                    input_ref.path.display()
                ))
            })?
        } else {
            let m = fs::metadata(&input_ref.path).map_err(|_| {
                ToolError::InputIo(format!(
                    "input file '{}' could not be read",
                    input_ref.path.display()
                ))
            })?;
            if !m.file_type().is_file() {
                return Err(ToolError::InputIo(format!(
                    "input '{}' is not a regular file",
                    input_ref.path.display()
                )));
            }
            m
        };

        if let Some(max_in) = request.limits.max_input_bytes
            && meta.len() > max_in
        {
            return Err(ToolError::LimitExceeded);
        }
        total_input_bytes = total_input_bytes.saturating_add(meta.len());
        input_paths.push(input_ref.path.clone());

        let stem = input_ref
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document")
            .to_owned();
        let ext = input_ref
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("pdf")
            .to_owned();
        current_items.push(PipelineItem {
            path: input_ref.path.clone(),
            stem,
            format: ext,
        });
    }

    if !within_run_quota(
        request.plan,
        RunSize {
            files: request.inputs.len(),
            bytes: total_input_bytes,
        },
    ) {
        return Err(ToolError::TierNotAllowed {
            tool: "recipe".to_owned(),
            tier: Tier::Pro,
            plan: request.plan,
        });
    }

    // 6. Execute steps sequentially inside a shared isolated workspace.
    let workspace =
        Workspace::new().map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;

    let mut step_outcomes = Vec::with_capacity(request.recipe.steps.len());
    let mut warnings = Vec::new();

    for (step_idx, step) in request.recipe.steps.iter().enumerate() {
        if request.cancel.is_cancelled() {
            return Err(ToolError::Interrupted);
        }
        let step_started = Instant::now();
        let step_dir = workspace.work_dir().join(format!("step-{step_idx:03}"));
        fs::create_dir_all(&step_dir)
            .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;

        match step {
            Step::Tool { tool, params } => {
                let spec = request
                    .spec_overrides
                    .get(tool.as_str())
                    .or_else(|| ariad_core::tools::find(tool.as_str()))
                    .ok_or_else(|| ToolError::UnknownTool(tool.to_string()))?;

                let mut next_items = Vec::new();
                if spec.inputs == InputArity::One && current_items.len() > 1 {
                    for (item_idx, item) in current_items.iter().enumerate() {
                        let item_out_dir = step_dir.join(format!("item-{item_idx:03}"));
                        fs::create_dir_all(&item_out_dir)
                            .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;

                        let outcome = execute_recipe_tool_call(
                            request,
                            spec,
                            params,
                            vec![InputRef::new(&item.path)],
                            &item_out_dir,
                            on_event,
                        )?;
                        warnings.extend(outcome.warnings);
                        for art in outcome.artifacts {
                            next_items.push(PipelineItem {
                                path: art.path,
                                stem: item.stem.clone(),
                                format: art.format,
                            });
                        }
                    }
                } else {
                    let first_stem = current_items
                        .first()
                        .map_or_else(|| "document".to_owned(), |i| i.stem.clone());
                    let step_inputs: Vec<InputRef> = current_items
                        .iter()
                        .map(|i| InputRef::new(&i.path))
                        .collect();
                    let outcome = execute_recipe_tool_call(
                        request,
                        spec,
                        params,
                        step_inputs,
                        &step_dir,
                        on_event,
                    )?;
                    warnings.extend(outcome.warnings);
                    for art in outcome.artifacts {
                        next_items.push(PipelineItem {
                            path: art.path,
                            stem: first_stem.clone(),
                            format: art.format,
                        });
                    }
                }

                let step_elapsed =
                    u64::try_from(step_started.elapsed().as_millis()).unwrap_or(u64::MAX);
                step_outcomes.push(StepOutcome {
                    step_index: step_idx,
                    kind: "tool".to_owned(),
                    target: tool.to_string(),
                    artifacts: next_items.len(),
                    elapsed_ms: step_elapsed,
                });
                current_items = next_items;
            }
            Step::Convert { to, profile } => {
                let target_ext = to.user_name();
                let mut next_items = Vec::with_capacity(current_items.len());
                let engine_prog = request
                    .engine_program
                    .clone()
                    .or_else(|| std::env::current_exe().ok())
                    .unwrap_or_else(|| PathBuf::from("ashift"));

                for (item_idx, item) in current_items.iter().enumerate() {
                    let out_path =
                        step_dir.join(format!("{item_idx:03}-{}.{target_ext}", item.stem));
                    let mut conv_req =
                        ConvertRequest::new(&item.path, &out_path, target_ext, &engine_prog);
                    conv_req.profile = profile.unwrap_or(Profile::Editable);
                    conv_req.overwrite = true;

                    let report = convert_with_limits(
                        &conv_req,
                        request.limits.clone(),
                        request.cancel.clone(),
                        |_| {},
                    )
                    .map_err(map_convert_error)?;

                    for w in report.warnings {
                        warnings.push(scrubber.scrub_warning(&w));
                    }
                    next_items.push(PipelineItem {
                        path: report.output,
                        stem: item.stem.clone(),
                        format: to.id().to_owned(),
                    });
                }

                let step_elapsed =
                    u64::try_from(step_started.elapsed().as_millis()).unwrap_or(u64::MAX);
                step_outcomes.push(StepOutcome {
                    step_index: step_idx,
                    kind: "convert".to_owned(),
                    target: target_ext.to_owned(),
                    artifacts: next_items.len(),
                    elapsed_ms: step_elapsed,
                });
                current_items = next_items;
            }
        }
    }

    // 7. Stage final step outputs into `workspace.output_dir()` and atomically publish.
    let mut staged_final = Vec::with_capacity(current_items.len());
    for (idx, item) in current_items.iter().enumerate() {
        let index_1_based = idx + 1;
        let staged_file = workspace
            .output_dir()
            .join(format!("final-{idx:03}.{}", item.format));
        fs::copy(&item.path, &staged_file)
            .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;

        let target_name = if let Some(ref template) = request.recipe.naming {
            render_naming_template(template, &item.stem, index_1_based)
                .map_err(|e| ToolError::InvalidParams(e.to_string()))?
        } else if let Some(name) = item.path.file_name().and_then(|n| n.to_str()) {
            name.to_owned()
        } else {
            format!("{}.{index_1_based:03}.{}", item.stem, item.format)
        };

        staged_final.push(StagedArtifact::new(
            staged_file,
            item.format.clone(),
            target_name,
        ));
    }

    let published = publish_atomic(
        &workspace,
        &staged_final,
        &request.output_dir,
        &input_paths,
        request.allowed_dirs.as_ref(),
        request.overwrite,
        request.fail_publish_at_index,
    )?;

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut metrics = BTreeMap::new();
    metrics.insert("steps".to_owned(), json!(step_outcomes.len()));
    metrics.insert("artifacts".to_owned(), json!(published.len()));
    metrics.insert("elapsed_ms".to_owned(), json!(elapsed_ms));

    Ok(RecipeOutcome {
        version: RECIPE_VERSION.to_owned(),
        steps: step_outcomes,
        artifacts: published,
        warnings,
        metrics,
    })
}

fn execute_recipe_tool_call(
    recipe_req: &RecipeRequest,
    spec: &ToolSpec,
    params: &serde_json::Value,
    inputs: Vec<InputRef>,
    step_out_dir: &Path,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<crate::tools::run::ToolOutcome, ToolError> {
    let step_secrets = recipe_req.secrets.filter_for_spec(spec, params);
    let mut tool_req = ToolRequest::new(
        spec.id,
        inputs,
        OutputTarget::Directory(step_out_dir.to_path_buf()),
    )
    .with_params(params.clone())
    .with_secrets(step_secrets)
    .with_secret_source(SecretSource::Recipe)
    .with_limits(recipe_req.limits.clone())
    .with_plan(recipe_req.plan)
    .with_overwrite(true)
    .with_cancel(recipe_req.cancel.clone());

    if recipe_req.spec_overrides.contains_key(spec.id) {
        tool_req = tool_req.with_spec(*spec);
    }
    if let Some(ref prog) = recipe_req.engine_program {
        tool_req = tool_req.with_engine_program(prog.clone());
    }
    if let Some(ref cmd) = recipe_req.engine_command {
        tool_req = tool_req.with_engine_command(cmd.clone());
    }
    run_tool_with_events(&tool_req, on_event)
}

fn map_convert_error(err: ConvertError) -> ToolError {
    match err {
        ConvertError::UnsupportedRoute { detail } => {
            ToolError::UnsupportedRoute(detail.unwrap_or_else(|| "unsupported route".to_owned()))
        }
        ConvertError::DestinationSameAsInput => ToolError::DestinationSameAsInput,
        ConvertError::LimitExceeded | ConvertError::MemoryLimitExceeded { .. } => {
            ToolError::LimitExceeded
        }
        ConvertError::SandboxUnavailable(msg) => ToolError::SandboxUnavailable(msg),
        ConvertError::ToolMissing => ToolError::from_missing_engine("pandoc"),
        ConvertError::DoclingMissing | ConvertError::DoclingToolMissing => {
            ToolError::from_missing_engine("docling")
        }
        ConvertError::EngineMissing { engine, hint } => ToolError::ToolMissing { engine, hint },
        ConvertError::DestinationExists => ToolError::DestinationExists("output".to_owned()),
        ConvertError::Interrupted => ToolError::Interrupted,
        ConvertError::InputIo => ToolError::InputIo("the input file could not be read".to_owned()),
        ConvertError::OutputIo => ToolError::OutputIo("could not write output file".to_owned()),
        ConvertError::UnsupportedIrVersion
        | ConvertError::InvalidCapabilities(_)
        | ConvertError::FailedWithMemoryHint { .. }
        | ConvertError::Failed => ToolError::EngineFailure(err.to_string()),
    }
}
