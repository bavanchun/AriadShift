//! Offline engine dispatch and consent-gated, host-brokered cloud AI.

use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use ariad_core::{
    format::Format,
    ir::Document,
    pipeline::{self, WriteOptions},
    tools::{Plan, Recipe, RecipeOrigin, Tier, ToolSpec, params::EditExportFormat},
};
use serde_json::{Value, json};
use url::Url;

use super::{
    engine,
    inprocess::RawToolExecution,
    run::{ToolError, ToolEvent, ToolRequest},
    secrets::SecretScrubber,
};
use crate::{
    net::{Client, HttpRequest},
    workspace::Workspace,
};

/// Whether the caller explicitly selected cloud execution.
pub fn is_cloud(params: &Value) -> bool {
    params.get("provider").and_then(Value::as_str) == Some("cloud")
}

fn api_key(params: &Value) -> Option<&str> {
    params
        .get("api_key")
        .and_then(Value::as_str)
        .filter(|key| !key.trim().is_empty())
}

/// Gate cloud access before reading any input, including format sniffing.
pub fn check_access(spec: &ToolSpec, params: &Value, plan: Plan) -> Result<(), ToolError> {
    if !is_cloud(params) {
        return Ok(());
    }
    if params.get("cloud_consent").and_then(Value::as_bool) != Some(true) {
        return Err(ToolError::InvalidParams(
            "cloud AI provider requires explicit consent (cloud_consent: true)".into(),
        ));
    }
    if plan == Plan::Free && api_key(params).is_none() {
        return Err(ToolError::TierNotAllowed {
            tool: spec.id.into(),
            tier: Tier::Pro,
            plan,
        });
    }
    Ok(())
}

fn endpoint() -> Result<(Url, bool), ToolError> {
    let value = std::env::var("ASHIFT_AI_CLOUD_URL").map_err(|_| {
        ToolError::InvalidParams("configure ASHIFT_AI_CLOUD_URL for cloud AI".into())
    })?;
    let url =
        Url::parse(&value).map_err(|_| ToolError::InvalidParams("invalid cloud AI URL".into()))?;
    let test_loopback = std::env::var("ASHIFT_TEST_ALLOW_LOOPBACK_AI").as_deref() == Ok("1")
        && url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    if !test_loopback
        && url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| !crate::net::is_public_ip(ip))
        })
    {
        return Err(ToolError::InvalidParams(
            "cloud AI URL must be a public HTTPS endpoint".into(),
        ));
    }
    crate::net::validate_url(
        &url,
        &crate::net::UrlPolicy {
            https_only: !test_loopback,
            allow_loopback: test_loopback,
            allowed_ports: if test_loopback {
                url.port().into_iter().collect()
            } else {
                Vec::new()
            },
        },
    )
    .map_err(|_| ToolError::InvalidParams("cloud AI URL must be a public HTTPS endpoint".into()))?;
    Ok((url, test_loopback))
}

/// Cloud execution does not require an installed local AI pack.
pub fn preflight() -> Result<(), ToolError> {
    endpoint().map(|_| ())
}

/// Execute AI locally, or broker a bounded cloud request in the trusted host.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    params: &Value,
    has_secrets: bool,
    workspace: &Workspace,
    request: &ToolRequest,
    scrubber: &SecretScrubber,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    check_access(spec, params, request.plan)?;
    if is_cloud(params) {
        return cloud(spec, staged_inputs, params, workspace, request, scrubber)
            .map_err(|e| e.scrubbed(scrubber));
    }
    let prompt_inputs;
    let inputs = if spec.id == "ai.recipe_from_text" {
        let path = workspace.input_dir().join("input-0.txt");
        fs::write(
            &path,
            params.get("prompt").and_then(Value::as_str).unwrap_or(""),
        )
        .map_err(|_| ToolError::OutputIo("could not stage recipe prompt".into()))?;
        prompt_inputs = vec![(path, "txt".into())];
        &prompt_inputs
    } else {
        staged_inputs
    };
    let mut result = engine::execute(
        spec,
        "ai",
        inputs,
        params,
        has_secrets,
        workspace,
        &request.limits,
        scrubber,
        &request.secrets,
        request.engine_program.as_deref(),
        request.engine_command.as_ref(),
        request.cancel.clone(),
        on_event,
    )?;
    if spec.id == "ai.recipe_from_text" {
        let value = result
            .inline_json
            .take()
            .ok_or_else(|| ToolError::EngineFailure("AI recipe output must be JSON".into()))?;
        validate_recipe_draft(&value)?;
        result.inline_json =
            Some(json!({"recipe":value,"origin":"imported","requires_confirmation":true}));
    }
    Ok(result)
}

fn instruction(tool: &str) -> &'static str {
    match tool {
        "ai.summarize" => {
            "Return only JSON with summary (string), bullet_points (array of strings), language and length. Summarize only source facts, following the requested length and language."
        }
        "ai.ask" => {
            "Return only JSON with answer (string), citations (array of {page: one-based integer, quote: exact source substring, score: number}). Cite only the supplied pages and respect max_citations. Do not invent supporting text."
        }
        "ai.translate" => {
            "Return only the translated ariad-ir/2 JSON document. Preserve all structure, IDs, assets, formatting and metadata. Change only text strings in text nodes and meta.language to target_lang."
        }
        "ai.extract_tables" => {
            "Return only JSON with tables: an array of {page: one-based integer, rows: array of arrays of strings}. Extract only actual source tables and selected pages."
        }
        _ => {
            "Return only an ariad-recipe/1 JSON recipe with version, steps and optional naming. Use only registered tools and valid parameters from the supplied tool catalogue. Never execute the recipe."
        }
    }
}

fn pages(document: &Document) -> BTreeMap<u32, String> {
    let mut result = BTreeMap::new();
    for block in &document.body {
        let page = document
            .source_map
            .get(block.id())
            .and_then(|positions| positions.first())
            .map(|position| position.page_no)
            .or_else(|| {
                document
                    .layout
                    .as_ref()
                    .and_then(|layout| layout.blocks.get(block.id()))
                    .map(|p| p.page_no)
            })
            .unwrap_or(1);
        let page_text = result.entry(page).or_insert_with(String::new);
        page_text.push_str(&ariad_core::writer::csv::blocks_to_text(
            std::slice::from_ref(block),
        ));
        page_text.push('\n');
    }
    result
}

fn cloud(
    spec: &ToolSpec,
    inputs: &[(PathBuf, String)],
    params: &Value,
    workspace: &Workspace,
    request: &ToolRequest,
    scrubber: &SecretScrubber,
) -> Result<RawToolExecution, ToolError> {
    let (url, loopback) = endpoint()?;
    let document = inputs
        .first()
        .map(|input| {
            super::edit_document::read_for_tool(
                input,
                workspace,
                &request.limits,
                &request.cancel,
                request.engine_program.as_deref(),
            )
        })
        .transpose()?;
    let source_pages = document.as_ref().map(pages).unwrap_or_default();
    let mut public_params = params.clone();
    public_params
        .as_object_mut()
        .expect("validated object")
        .remove("api_key");
    let catalogue = if spec.id == "ai.recipe_from_text" {
        serde_json::to_value(
            ariad_core::tools::all()
                .iter()
                .map(|s| s.to_descriptor())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| ToolError::EngineFailure("could not serialize tool catalogue".into()))?
    } else {
        Value::Null
    };
    // Only translation needs structure. Binary assets stay on the host and are
    // restored before validating the returned IR.
    let cloud_document = if spec.id == "ai.translate" {
        let mut value = serde_json::to_value(&document)
            .map_err(|_| ToolError::EngineFailure("could not serialize source structure".into()))?;
        value["assets"] = json!({});
        value["provenance"] = Value::Null;
        value
    } else {
        Value::Null
    };
    let user_content = json!({
        "tool": spec.id,
        "params": public_params,
        "document": cloud_document,
        "pages": source_pages,
        "tools": catalogue,
    })
    .to_string();
    let anthropic = url.path().trim_end_matches('/').ends_with("/messages");
    let model = params
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let payload = if anthropic {
        json!({"model":model,"max_tokens":4096,"system":instruction(spec.id),
            "messages":[{"role":"user","content":user_content}]})
    } else {
        json!({
            "model": params
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("default"),
            "messages": [
                {"role": "system", "content": instruction(spec.id)},
                {"role": "user", "content": user_content},
            ],
        })
    };
    let body = serde_json::to_vec(&payload)
        .map_err(|_| ToolError::EngineFailure("could not serialize cloud request".into()))?;
    if body.len() as u64 > request.limits.max_ir_json_bytes.min(32 * 1024 * 1024) {
        return Err(ToolError::LimitExceeded);
    }
    let timeout = Duration::from_secs(request.limits.timeout_s.unwrap_or(120));
    let client = Client::builder()
        .https_only(!loopback)
        .allow_loopback(loopback)
        .allowed_ports(if loopback {
            url.port().into_iter().collect()
        } else {
            Vec::new()
        })
        .max_redirects(0)
        .max_response_bytes(request.limits.max_ir_json_bytes.min(32 * 1024 * 1024) as usize)
        .total_timeout(timeout)
        .read_timeout(timeout)
        .build()
        .map_err(|_| ToolError::EngineFailure("could not initialize cloud AI client".into()))?;
    let mut http = HttpRequest::post(url, body, "application/json");
    if anthropic {
        http.headers
            .push(("anthropic-version".into(), "2023-06-01".into()));
    }
    if let Some(key) = api_key(params) {
        if anthropic {
            http.headers.push(("x-api-key".into(), key.into()));
        }
        http.headers
            .push(("authorization".into(), format!("Bearer {key}")));
    }
    if request.cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }
    let response = client.send(http).map_err(|e| {
        ToolError::EngineFailure(scrubber.scrub(&format!("cloud AI request failed: {e}")))
    })?;
    if request.cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }
    if !(200..300).contains(&response.status) {
        return Err(ToolError::EngineFailure(format!(
            "cloud AI returned HTTP {}",
            response.status
        )));
    }
    let response: Value = serde_json::from_slice(&response.body)
        .map_err(|_| ToolError::EngineFailure("invalid cloud AI response".into()))?;
    let content = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| response.pointer("/content/0/text").and_then(Value::as_str))
        .ok_or_else(|| {
            ToolError::EngineFailure("cloud AI response has no message content".into())
        })?;
    let content = content
        .trim()
        .strip_prefix("```json")
        .or_else(|| content.trim().strip_prefix("```"))
        .and_then(|v| v.trim().strip_suffix("```"))
        .unwrap_or(content)
        .trim();
    let value: Value = serde_json::from_str(content)
        .map_err(|_| ToolError::EngineFailure("cloud AI message must contain JSON".into()))?;
    let value = scrubber.scrub_value(&value);
    cloud_artifacts(
        spec,
        params,
        value,
        document.as_ref(),
        &source_pages,
        workspace,
        request,
    )
}

#[allow(clippy::too_many_arguments)]
fn cloud_artifacts(
    spec: &ToolSpec,
    params: &Value,
    mut value: Value,
    document: Option<&Document>,
    source_pages: &BTreeMap<u32, String>,
    workspace: &Workspace,
    request: &ToolRequest,
) -> Result<RawToolExecution, ToolError> {
    let format = params
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or(spec.output_format);
    let mut result = RawToolExecution {
        artifacts: Vec::new(),
        warnings: Vec::new(),
        metrics: BTreeMap::new(),
        inline_json: None,
    };
    let invalid =
        || ToolError::EngineFailure("cloud AI output does not match the tool contract".into());
    match spec.id {
        "ai.recipe_from_text" => {
            let recipe = validate_recipe_draft(value.get("recipe").unwrap_or(&value))?;
            value = serde_json::to_value(recipe).map_err(|_| invalid())?;
            result.inline_json =
                Some(json!({"recipe":value,"origin":"imported","requires_confirmation":true}));
        }
        "ai.ask" => {
            if !value.get("answer").is_some_and(Value::is_string) {
                return Err(invalid());
            }
            let citations = value
                .get("citations")
                .and_then(Value::as_array)
                .ok_or_else(invalid)?;
            if citations.len()
                > params
                    .get("max_citations")
                    .and_then(Value::as_u64)
                    .unwrap_or(5) as usize
            {
                return Err(invalid());
            }
            for citation in citations {
                if !citation
                    .get("score")
                    .and_then(Value::as_f64)
                    .is_some_and(|score| (0.0..=1.0).contains(&score))
                {
                    return Err(invalid());
                }
                let page = citation
                    .get("page")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(invalid)?;
                let quote = citation
                    .get("quote")
                    .and_then(Value::as_str)
                    .filter(|q| !q.is_empty())
                    .ok_or_else(invalid)?;
                if !source_pages
                    .get(&page)
                    .is_some_and(|text| text.contains(quote))
                {
                    return Err(invalid());
                }
            }
        }
        "ai.summarize" => {
            let summary = value
                .get("summary")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            if !value
                .get("bullet_points")
                .and_then(Value::as_array)
                .is_some_and(|points| points.iter().all(Value::is_string))
                || !value
                    .get("language")
                    .and_then(Value::as_str)
                    .is_some_and(|language| !language.trim().is_empty())
            {
                return Err(invalid());
            }
            if format == "md" {
                let path = workspace.output_dir().join("summary.md");
                fs::write(&path, format!("{summary}\n"))
                    .map_err(|_| ToolError::OutputIo("could not write summary".into()))?;
                result.artifacts.push((path, "md".into()));
                return Ok(result);
            }
            let count = summary.split_whitespace().count();
            value["word_count"] = json!(count);
            value["length"] = json!(
                params
                    .get("length")
                    .and_then(Value::as_str)
                    .unwrap_or("standard")
            );
        }
        "ai.translate" => {
            let original = document.ok_or_else(invalid)?;
            let fields = value.as_object_mut().ok_or_else(invalid)?;
            fields.insert(
                "assets".into(),
                serde_json::to_value(&original.assets).map_err(|_| invalid())?,
            );
            fields.insert(
                "provenance".into(),
                serde_json::to_value(&original.provenance).map_err(|_| invalid())?,
            );
            let translated = pipeline::read_ir_json(value.to_string().as_bytes(), &request.limits)
                .map_err(|_| invalid())?;
            let mut before = serde_json::to_value(original).map_err(|_| invalid())?;
            let mut after = serde_json::to_value(&translated).map_err(|_| invalid())?;
            erase_translation(&mut before);
            erase_translation(&mut after);
            if before != after {
                return Err(ToolError::EngineFailure(
                    "cloud translation changed document structure".into(),
                ));
            }
            value = serde_json::to_value(&translated).map_err(|_| invalid())?;
            if format != "json" {
                let export: EditExportFormat =
                    serde_json::from_value(json!(format)).map_err(|_| invalid())?;
                let (path, warnings) = super::edit_document::write_for_tool(
                    &translated,
                    export,
                    workspace,
                    &request.limits,
                    &request.cancel,
                    request.engine_program.as_deref(),
                )?;
                result.artifacts.push((path, format.into()));
                result.warnings = warnings;
                return Ok(result);
            }
        }
        "ai.extract_tables" => {
            let tables = value
                .get("tables")
                .and_then(Value::as_array)
                .ok_or_else(invalid)?;
            if tables.len() > 1000 {
                return Err(ToolError::LimitExceeded);
            }
            let selected = params
                .get("pages")
                .filter(|value| !value.is_null())
                .map(|value| {
                    let range: ariad_core::tools::PageRange =
                        serde_json::from_value(value.clone()).map_err(|_| invalid())?;
                    range
                        .expand(
                            source_pages.keys().next_back().copied().unwrap_or(1),
                            ariad_core::tools::DuplicatePolicy::Reject,
                        )
                        .map_err(|_| invalid())
                })
                .transpose()?;
            for (index, table) in tables.iter().enumerate() {
                let page = table
                    .get("page")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(invalid)?;
                if !source_pages.contains_key(&page)
                    || selected
                        .as_ref()
                        .is_some_and(|pages| !pages.contains(&page))
                {
                    return Err(invalid());
                }
                let path = workspace
                    .output_dir()
                    .join(format!("table_{:03}.{format}", index + 1));
                let rows: Vec<Vec<String>> =
                    serde_json::from_value(table.get("rows").cloned().ok_or_else(invalid)?)
                        .map_err(|_| invalid())?;
                let bytes = if format == "json" {
                    serde_json::to_vec(table).map_err(|_| invalid())?
                } else {
                    let csv = rows
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|cell| {
                                    ariad_core::writer::csv::escape_csv_field(
                                        &ariad_core::writer::csv::guard_formula(cell),
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if format == "csv" {
                        csv.into_bytes()
                    } else {
                        let doc = pipeline::read(Format::Csv, csv.as_bytes(), &request.limits)
                            .map_err(|_| invalid())?
                            .document;
                        pipeline::write(Format::Xlsx, &doc, WriteOptions::default())
                            .map_err(|_| invalid())?
                            .bytes
                    }
                };
                fs::write(&path, bytes)
                    .map_err(|_| ToolError::OutputIo("could not write extracted table".into()))?;
                result.artifacts.push((path, format.into()));
            }
            if result.artifacts.len() == 1 && format == "json" {
                result.inline_json = tables.first().cloned();
            }
            return Ok(result);
        }
        _ => return Err(invalid()),
    }
    let path = workspace.output_dir().join("result.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&value).map_err(|_| invalid())?,
    )
    .map_err(|_| ToolError::OutputIo("could not write cloud output".into()))?;
    result.artifacts.push((path, "json".into()));
    if result.inline_json.is_none() {
        result.inline_json = Some(value);
    }
    Ok(result)
}

fn validate_recipe_draft(value: &Value) -> Result<Recipe, ToolError> {
    let invalid =
        || ToolError::EngineFailure("AI recipe output does not match the tool contract".into());
    let recipe =
        Recipe::from_json_str(&value.to_string(), RecipeOrigin::Imported).map_err(|_| invalid())?;
    for step in &recipe.steps {
        if let ariad_core::tools::Step::Tool { tool, params, .. } = step {
            let spec = ariad_core::tools::find(tool.as_str()).ok_or_else(invalid)?;
            let mut schema_params = params.clone();
            // Secret references remain unresolved in drafts. Validate their channel in
            // Recipe::validate and their surrounding schema with a typed placeholder.
            for &field in spec.secret_params {
                if schema_params.get(field).is_some_and(Value::is_object) {
                    schema_params[field] = json!("secret-reference");
                }
            }
            super::run::validate_params_against_schema(
                &spec.resolved_params_schema(),
                &schema_params,
            )
            .map_err(|_| invalid())?;
            super::run::validate_semantic_params(spec.id, &schema_params).map_err(|_| invalid())?;
        }
    }
    Ok(recipe)
}

fn erase_translation(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if fields.get("type").and_then(Value::as_str) == Some("text") {
                fields.insert("text".into(), Value::Null);
            }
            if let Some(meta) = fields.get_mut("meta").and_then(Value::as_object_mut) {
                meta.remove("language");
            }
            for item in fields.values_mut() {
                erase_translation(item);
            }
        }
        Value::Array(items) => {
            for item in items {
                erase_translation(item);
            }
        }
        _ => {}
    }
}
