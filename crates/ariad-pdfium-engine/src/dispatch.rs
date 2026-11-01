//! Request handling: `describe` answers the capabilities, `run_tool` validates the request and
//! calls the module of the requested tool.

use std::io::Write;

use ariad_core::{
    protocol::{
        ToolAvailability, ToolStatus,
        v2::{EventV2, PROTOCOL_V2, RequestV2, ToolCapability},
    },
    tools::{
        params::{
            CropWhitespaceParams, DetectScannedParams, ExtractImagesParams, RemoveBlankParams,
            RenderParams, TextLayoutParams, ThumbnailsParams, ToImagesParams,
        },
        specs::pdfium::specs,
    },
};
use ariad_engine_io::{EngineFailure, emit_capabilities_v2, validate_run_tool};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    ENGINE_ID, blank, crop_whitespace, detect_scanned, extract_images, library,
    output::{ToolContext, ToolResult},
    render, text_layout, thumbnails,
};

/// Every tool id the engine handles. `pdf.split` is served for its pixel `blank-separator` mode;
/// the structural PDF tools handle the other modes in process.
pub const TOOL_IDS: &[&str] = &[
    "pdf.render",
    "pdf.to_images",
    "pdf.thumbnails",
    "pdf.extract_images",
    "pdf.text_layout",
    "pdf.remove_blank",
    "pdf.crop_whitespace",
    "pdf.detect_scanned",
    "pdf.split",
];

/// Contract version reported for `pdf.split`, whose spec lives with the structural PDF tools.
const SPLIT_VERSION: &str = "1";

/// Builds the `capabilities` event: the tools handled, and whether libpdfium is installed.
#[must_use]
pub fn capabilities() -> EventV2 {
    let library_found = library::locate_default().is_ok();
    capabilities_with(library_found)
}

fn capabilities_with(library_found: bool) -> EventV2 {
    let specs = specs();
    let tools = TOOL_IDS
        .iter()
        .map(|id| ToolCapability {
            id: (*id).to_owned(),
            version: specs
                .iter()
                .find(|spec| spec.id == *id)
                .map_or(SPLIT_VERSION, |spec| spec.version)
                .to_owned(),
        })
        .collect();
    EventV2::Capabilities {
        protocol: PROTOCOL_V2.to_owned(),
        engine: ENGINE_ID.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        tool: ToolStatus {
            name: "libpdfium".to_owned(),
            version: None,
            status: if library_found {
                ToolAvailability::Found
            } else {
                ToolAvailability::Missing
            },
        },
        license: "Apache-2.0".to_owned(),
        routes: Vec::new(),
        tools,
        enforces_memory_limit: false,
        supports_session: false,
        models: None,
    }
}

/// Handles one request, writing its events to `out`.
///
/// A failure is returned for `serve_v2` to report as the failed `result`.
pub fn handle<W: Write>(request: RequestV2, out: &mut W) -> Result<(), EngineFailure> {
    handle_with(request, out, None)
}

/// [`handle`] with an explicit libpdfium directory, for tests.
pub fn handle_with<W: Write>(
    request: RequestV2,
    out: &mut W,
    library_dir: Option<&std::path::Path>,
) -> Result<(), EngineFailure> {
    match request {
        RequestV2::Describe { protocol, .. } => {
            let found = match library_dir {
                Some(dir) => library::find_in(dir).is_some(),
                None => library::locate_default().is_ok(),
            };
            emit_capabilities_v2(out, &protocol, capabilities_with(found))
        }
        RequestV2::Convert { .. } => Err(EngineFailure::unsupported_route()),
        RequestV2::RunTool {
            protocol,
            job,
            tool,
            inputs,
            output,
            params,
            params_file,
            work_dir,
            limits,
        } => {
            let (paths, params) = validate_run_tool(
                &protocol,
                &inputs,
                &output,
                &params,
                params_file.as_deref(),
                &work_dir,
                &limits,
            )?;
            if !TOOL_IDS.contains(&tool.as_str()) {
                return Err(EngineFailure::unsupported_route());
            }
            let mut ctx = ToolContext::new(job, tool, paths, limits, out);
            if let Some(dir) = library_dir {
                ctx = ctx.with_library_dir(dir);
            }
            let metrics = run_tool(&mut ctx, params)?;
            ctx.finish(metrics)
        }
    }
}

/// Parses and checks `params`, then runs the tool named by `ctx.tool()`.
pub fn run_tool(ctx: &mut ToolContext<'_>, params: Value) -> ToolResult {
    ctx.check()?;
    match ctx.tool().to_owned().as_str() {
        "pdf.render" => {
            let params: RenderParams = parse(params)?;
            params.validate().map_err(invalid)?;
            render::run(ctx, params)
        }
        "pdf.to_images" => {
            let params: ToImagesParams = parse(params)?;
            params.validate().map_err(invalid)?;
            render::run(ctx, params.into())
        }
        "pdf.thumbnails" => {
            let params: ThumbnailsParams = parse(params)?;
            params.validate().map_err(invalid)?;
            thumbnails::run(ctx, params)
        }
        "pdf.extract_images" => {
            let params: ExtractImagesParams = parse(params)?;
            params.validate().map_err(invalid)?;
            extract_images::run(ctx, params)
        }
        "pdf.text_layout" => text_layout::run(ctx, parse::<TextLayoutParams>(params)?),
        "pdf.remove_blank" => {
            let params: RemoveBlankParams = parse(params)?;
            params.validate().map_err(invalid)?;
            blank::run(ctx, params)
        }
        "pdf.crop_whitespace" => {
            let params: CropWhitespaceParams = parse(params)?;
            params.validate().map_err(invalid)?;
            crop_whitespace::run(ctx, params)
        }
        "pdf.detect_scanned" => detect_scanned::run(ctx, parse::<DetectScannedParams>(params)?),
        "pdf.split" => blank::split_pixel(ctx, &params),
        _ => Err(EngineFailure::unsupported_route()),
    }
}

fn parse<P: DeserializeOwned>(params: Value) -> Result<P, EngineFailure> {
    serde_json::from_value(params).map_err(|_| EngineFailure::invalid_request())
}

fn invalid(error: ariad_core::tools::params::ParamsError) -> EngineFailure {
    EngineFailure::owned(
        ariad_core::protocol::ErrorCode::InvalidRequest,
        error.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use ariad_core::protocol::{ErrorCode, Input};
    use ariad_engine_io::serve_v2;
    use serde_json::json;

    use super::*;

    fn run_request(request: &Value) -> (Vec<Value>, std::process::ExitCode) {
        let mut line = serde_json::to_vec(request).expect("request json");
        line.push(b'\n');
        let mut out = Vec::new();
        let code = serve_v2(Cursor::new(line), &mut out, |request, out| {
            handle_with(request, out, None)
        });
        let events = out
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).expect("event json"))
            .collect();
        (events, code)
    }

    fn describe() -> Value {
        json!({"op":"describe","protocol":"ariad-engine/2","job":"job-d"})
    }

    fn run_tool_request(root: &std::path::Path, tool: &str, params: Value) -> Value {
        let input = root.join("in").join("doc.pdf");
        json!({
            "op": "run_tool",
            "protocol": "ariad-engine/2",
            "job": "job-r",
            "tool": tool,
            "inputs": [Input { path: input.to_string_lossy().into_owned(), format: "pdf".to_owned() }],
            "output": {"dir": root.join("out")},
            "params": params,
            "work_dir": root.join("tmp"),
            "limits": {},
        })
    }

    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(dir.path().join(sub)).expect("create workspace dir");
        }
        fs::write(dir.path().join("in").join("doc.pdf"), b"%PDF-1.7").expect("write input");
        dir
    }

    #[test]
    fn describe_lists_every_tool_the_engine_handles() {
        let (events, code) = run_request(&describe());
        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert_eq!(events[0]["type"], "capabilities");
        assert_eq!(events[0]["protocol"], "ariad-engine/2");
        assert_eq!(events[0]["engine"], "pdfium");
        assert_eq!(events[0]["supports_session"], false);
        let ids: Vec<&str> = events[0]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|tool| tool["id"].as_str().expect("tool id"))
            .collect();
        assert_eq!(
            ids,
            [
                "pdf.render",
                "pdf.to_images",
                "pdf.thumbnails",
                "pdf.extract_images",
                "pdf.text_layout",
                "pdf.remove_blank",
                "pdf.crop_whitespace",
                "pdf.detect_scanned",
                "pdf.split",
            ]
        );
        assert_eq!(events[1], json!({"type":"result","ok":true}));
    }

    #[test]
    fn every_registered_pdfium_spec_is_served() {
        for spec in specs().iter().filter(|spec| spec.engine == ENGINE_ID) {
            assert!(TOOL_IDS.contains(&spec.id), "{} is not served", spec.id);
        }
    }

    #[test]
    fn missing_library_or_truncated_pdf_returns_typed_failure_for_every_tool() {
        let ws = workspace();
        let empty_lib = tempfile::tempdir().expect("empty lib dir");
        for (tool, params) in [
            ("pdf.render", json!({})),
            ("pdf.to_images", json!({"combine": "vertical"})),
            ("pdf.thumbnails", json!({"max_px": 128})),
            ("pdf.extract_images", json!({})),
            ("pdf.text_layout", json!({})),
            ("pdf.remove_blank", json!({})),
            ("pdf.crop_whitespace", json!({})),
            ("pdf.detect_scanned", json!({})),
            (
                "pdf.split",
                json!({"mode": "blank-separator", "detect": "pixel"}),
            ),
        ] {
            let mut line =
                serde_json::to_vec(&run_tool_request(ws.path(), tool, params)).expect("json");
            line.push(b'\n');
            let mut out = Vec::new();
            let code = serve_v2(Cursor::new(line), &mut out, |request, out| {
                handle_with(request, out, Some(empty_lib.path()))
            });
            assert_eq!(code, std::process::ExitCode::from(1), "{tool}");
            let events: Vec<Value> = out
                .split(|b| *b == b'\n')
                .filter(|l| !l.is_empty())
                .map(|l| serde_json::from_slice(l).expect("event json"))
                .collect();
            let last = events.last().expect("a result event");
            assert_eq!(last["ok"], false, "{tool}");
            let err_code = last["error"]["code"].as_str().expect("error code");
            assert!(
                matches!(
                    err_code,
                    "tool_missing" | "invalid_request" | "engine_failure"
                ),
                "{tool}: unexpected error code {err_code}"
            );
        }
    }

    #[test]
    fn invalid_params_are_rejected_before_the_tool_runs() {
        let ws = workspace();
        for (tool, params) in [
            ("pdf.render", json!({"dpi": 10_000})),
            ("pdf.render", json!({"unknown": true})),
            ("pdf.thumbnails", json!({"max_px": 1})),
            ("pdf.remove_blank", json!({"threshold": 2})),
            ("pdf.detect_scanned", json!({"pages": "all"})),
        ] {
            let (events, _) = run_request(&run_tool_request(ws.path(), tool, params));
            let last = events.last().expect("a result event");
            assert_eq!(last["error"]["code"], "invalid_request", "{tool}");
        }
    }

    #[test]
    fn unknown_tools_and_convert_requests_are_unsupported() {
        let ws = workspace();
        let (events, _) = run_request(&run_tool_request(ws.path(), "pdf.merge", json!({})));
        assert_eq!(events[0]["error"]["code"], "unsupported_route");

        let convert = json!({
            "op": "convert",
            "protocol": "ariad-engine/2",
            "job": "job-c",
            "input": {"path": ws.path().join("in").join("doc.pdf"), "format": "pdf"},
            "output": {"dir": ws.path().join("out"), "format": "png"},
            "work_dir": ws.path().join("tmp"),
            "limits": {},
        });
        let (events, _) = run_request(&convert);
        assert_eq!(events[0]["error"]["code"], "unsupported_route");
    }

    #[test]
    fn malformed_workspace_paths_are_rejected() {
        let ws = workspace();
        let mut request = run_tool_request(ws.path(), "pdf.render", json!({}));
        request["output"]["dir"] = json!(ws.path().join("elsewhere"));
        let (events, _) = run_request(&request);
        assert_eq!(events[0]["error"]["code"], "invalid_request");
        assert_eq!(ErrorCode::InvalidRequest, ErrorCode::InvalidRequest);
    }
}
