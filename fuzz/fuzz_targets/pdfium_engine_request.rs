#![no_main]

//! Fuzzes the `ariad-pdfium-engine` request and parameter validation boundary over
//! `ariad-engine/2`: raw NDJSON lines, `RequestV2` decoding, workspace path checks,
//! `params_file` loading, and per-tool parameter deserialization and semantic validation.

use std::{fs, io::Cursor, path::Path, sync::OnceLock};

use arbitrary::Unstructured;
use ariad_core::{
    limits::Limits,
    protocol::{
        Input,
        v2::{EventV2, OutputDir, PROTOCOL_V2, RequestV2},
    },
};
use ariad_engine_io::serve_v2;
use ariad_pdfium_engine::dispatch::{TOOL_IDS, handle_with};
use libfuzzer_sys::fuzz_target;
use serde_json::{Map, Value, json};

const MAX_INPUT_BYTES: usize = 512 * 1024;

struct FuzzEnv {
    workspace: tempfile::TempDir,
    empty_lib: tempfile::TempDir,
}

fn env() -> &'static FuzzEnv {
    static ENV: OnceLock<FuzzEnv> = OnceLock::new();
    ENV.get_or_init(|| {
        let workspace = tempfile::tempdir().expect("create fuzz workspace");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(workspace.path().join(sub)).expect("create workspace subdir");
        }
        fs::write(workspace.path().join("in").join("doc.pdf"), b"%PDF-1.7\n")
            .expect("write input stub");
        fs::write(workspace.path().join("tmp").join("params.json"), b"{}")
            .expect("write params.json stub");
        let empty_lib = tempfile::tempdir().expect("create empty library dir");
        FuzzEnv {
            workspace,
            empty_lib,
        }
    })
}

fn assert_valid_event_stream(out: &[u8]) {
    let lines: Vec<&[u8]> = out
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert!(
        !lines.is_empty(),
        "ariad-pdfium-engine must emit at least one protocol event"
    );

    let mut result_indices = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let event: EventV2 = serde_json::from_slice(line).expect("emitted line must be EventV2");
        let reencoded = serde_json::to_vec(&event).expect("EventV2 serializes");
        assert!(
            !reencoded.contains(&b'\n'),
            "serialized EventV2 must not contain a newline"
        );
        if let EventV2::Result { ok, error, .. } = &event {
            assert_eq!(
                *ok,
                error.is_none(),
                "EventV2::Result ok flag must agree with error presence"
            );
            result_indices.push(idx);
        }
    }
    assert_eq!(
        result_indices,
        vec![lines.len() - 1],
        "exactly one terminal EventV2::Result must be emitted last"
    );
}

fn run_engine_line(line: &[u8], empty_lib: &Path) {
    let mut out = Vec::new();
    let _ = serve_v2(Cursor::new(line), &mut out, |request, out| {
        handle_with(request, out, Some(empty_lib))
    });
    assert_valid_event_stream(&out);
}

fn bounded_json_value(u: &mut Unstructured<'_>, depth: usize) -> Value {
    if depth == 0 || u.is_empty() {
        return match u.int_in_range(0u8..=5).unwrap_or(0) {
            0 => Value::Null,
            1 => Value::Bool(u.arbitrary().unwrap_or(false)),
            2 => json!(u.int_in_range(-1000i64..=10000).unwrap_or(150)),
            3 => json!(f64::from(u.int_in_range(0u16..=1000).unwrap_or(995)) / 1000.0),
            4 => {
                let len = u.int_in_range(0usize..=24).unwrap_or(0);
                let bytes = u.bytes(len).unwrap_or(&[]);
                Value::String(String::from_utf8_lossy(bytes).into_owned())
            }
            _ => json!("all"),
        };
    }

    match u.int_in_range(0u8..=4).unwrap_or(0) {
        0..=1 => bounded_json_value(u, 0),
        2 => {
            let count = u.int_in_range(0usize..=4).unwrap_or(0);
            let mut arr = Vec::with_capacity(count);
            for _ in 0..count {
                arr.push(bounded_json_value(u, depth - 1));
            }
            Value::Array(arr)
        }
        _ => {
            let count = u.int_in_range(0usize..=6).unwrap_or(0);
            let mut map = Map::with_capacity(count);
            for _ in 0..count {
                let key = match u.int_in_range(0u8..=12).unwrap_or(0) {
                    0 => "pages".to_owned(),
                    1 => "format".to_owned(),
                    2 => "dpi".to_owned(),
                    3 => "quality".to_owned(),
                    4 => "alpha".to_owned(),
                    5 => "combine".to_owned(),
                    6 => "max_px".to_owned(),
                    7 => "min_px".to_owned(),
                    8 => "threshold".to_owned(),
                    9 => "padding_pt".to_owned(),
                    10 => "mode".to_owned(),
                    11 => "detect".to_owned(),
                    _ => {
                        let k_len = u.int_in_range(1usize..=10).unwrap_or(1);
                        let k_bytes = u.bytes(k_len).unwrap_or(b"k");
                        String::from_utf8_lossy(k_bytes).into_owned()
                    }
                };
                map.insert(key, bounded_json_value(u, depth - 1));
            }
            Value::Object(map)
        }
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let env = env();

    // 1. Raw NDJSON request stream.
    run_engine_line(data, env.empty_lib.path());

    // 2. Structured RequestV2 exercising validate_run_tool and per-tool param validation.
    let mut u = Unstructured::new(data);
    let root = env.workspace.path();
    let input_path = root.join("in").join("doc.pdf");
    let out_dir = root.join("out");
    let work_dir = root.join("tmp");
    let params_file_path = work_dir.join("params.json");

    let tool_idx = u.int_in_range(0usize..=TOOL_IDS.len()).unwrap_or(0);
    let tool = if tool_idx < TOOL_IDS.len() {
        TOOL_IDS[tool_idx].to_owned()
    } else {
        "pdf.unknown".to_owned()
    };

    let use_params_file = u.arbitrary::<bool>().unwrap_or(false);
    let params_obj = bounded_json_value(&mut u, 2);
    let (inline_params, params_file) = if use_params_file {
        let _ = fs::write(
            &params_file_path,
            serde_json::to_vec(&params_obj).unwrap_or_else(|_| b"{}".to_vec()),
        );
        (json!({}), Some(params_file_path))
    } else {
        (params_obj, None)
    };

    let request = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "fuzz-job".to_owned(),
        tool,
        inputs: vec![Input {
            path: input_path.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: out_dir.display().to_string(),
        },
        params: inline_params,
        params_file,
        work_dir: work_dir.display().to_string(),
        limits: Limits::local(),
    };

    if let Ok(mut line) = serde_json::to_vec(&request) {
        line.push(b'\n');
        run_engine_line(&line, env.empty_lib.path());
    }
});
