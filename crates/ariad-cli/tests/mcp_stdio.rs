//! Integration tests for `ashift mcp` stdio server.
//!
//! Tests verify JSON-RPC protocol compliance, stdout purity,
//! tool schema & invocation matching CLI outputs, path confinement,
//! capability-based promotion, and session-scoped resources.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use ariad_host::confine::path_to_file_uri;
use serde_json::{Value, json};
use tempfile::TempDir;

fn pandoc_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ASHIFT_PANDOC") {
        return PathBuf::from(path);
    }
    let name = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    repository_root().join(".tools/pandoc/bin").join(name)
}

fn test_capabilities_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("test_capabilities.json")
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of ariad-cli")
        .parent()
        .expect("root repository")
        .to_path_buf()
}

/// Helper client for testing `ashift mcp` over stdio.
struct McpTestClient {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: BufReader<ChildStdout>,
    recorded_stdout: Vec<String>,
    advertised_roots: Vec<String>,
    roots_queries_count: usize,
}

impl Drop for McpTestClient {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl McpTestClient {
    fn spawn(args: &[&str], advertised_roots: Vec<String>) -> Self {
        Self::spawn_with_env(args, advertised_roots, &[])
    }

    fn spawn_with_env(
        args: &[&str],
        advertised_roots: Vec<String>,
        envs: &[(&str, &std::ffi::OsStr)],
    ) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ashift"));
        cmd.arg("mcp")
            .args(args)
            .env("ASHIFT_PANDOC", pandoc_path())
            .env("SOURCE_DATE_EPOCH", "1700000000")
            .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().expect("failed to spawn ashift mcp");
        let stdin = child.stdin.take().expect("stdin pipe");
        let stdout = child.stdout.take().expect("stdout pipe");
        let reader = BufReader::new(stdout);

        Self {
            child: Some(child),
            stdin: Some(stdin),
            reader,
            recorded_stdout: Vec::new(),
            advertised_roots,
            roots_queries_count: 0,
        }
    }

    #[cfg(unix)]
    fn child_pid(&self) -> u32 {
        self.child.as_ref().expect("child exists").id()
    }

    fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    fn update_roots(&mut self, new_roots: Vec<String>) {
        self.advertised_roots = new_roots;
    }

    fn write_line(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin exists");
        stdin.write_all(line.as_bytes()).expect("write to stdin");
        stdin.write_all(b"\n").expect("write newline to stdin");
        stdin.flush().expect("flush stdin");
    }

    fn send_request(&mut self, id: u64, method: &str, params: Value) {
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.write_line(&req.to_string());
    }

    fn send_notification(&mut self, method: &str, params: Value) {
        let notif = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_line(&notif.to_string());
    }

    /// Reads next JSON message, automatically handling server-to-client `roots/list` requests.
    fn read_message(&mut self) -> Value {
        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .expect("read line from stdout");
            if bytes_read == 0 {
                panic!("unexpected EOF on stdout");
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.recorded_stdout.push(trimmed.to_string());

            let val: Value = serde_json::from_str(trimmed)
                .unwrap_or_else(|e| panic!("stdout contained non-JSON line: '{trimmed}': {e}"));

            // Check if server is asking for roots/list
            if val.get("method").and_then(Value::as_str) == Some("roots/list")
                && let Some(req_id) = val.get("id")
            {
                let roots_array: Vec<Value> = self
                    .advertised_roots
                    .iter()
                    .map(|uri| {
                        json!({
                            "uri": uri,
                            "name": "workspace"
                        })
                    })
                    .collect();

                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "roots": roots_array
                    }
                });
                self.roots_queries_count += 1;
                self.write_line(&resp.to_string());
                continue;
            }

            return val;
        }
    }

    /// Wait for server to refresh roots by completing a roots/list request cycle.
    fn wait_for_roots_refresh(&mut self) {
        let initial = self.roots_queries_count;
        for req_id in 900..950 {
            if self.roots_queries_count > initial {
                break;
            }
            self.send_request(req_id, "tools/list", json!({}));
            let _ = self.read_response(req_id);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Reads messages until a response matching `expected_id` is found.
    fn read_response(&mut self, expected_id: u64) -> Value {
        loop {
            let msg = self.read_message();
            if msg.get("id").and_then(Value::as_u64) == Some(expected_id) {
                return msg;
            }
        }
    }

    /// Perform standard MCP initialize sequence.
    fn initialize(&mut self) -> Value {
        self.send_request(
            1,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "roots": { "listChanged": true }
                },
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0"
                }
            }),
        );
        let resp = self.read_response(1);
        self.send_notification("notifications/initialized", json!({}));
        self.wait_for_roots_refresh();
        resp
    }

    /// Perform MCP initialize sequence with a specific protocol version.
    fn initialize_with_version(&mut self, version: &str) -> Value {
        self.send_request(
            1,
            "initialize",
            json!({
                "protocolVersion": version,
                "capabilities": {
                    "roots": { "listChanged": true }
                },
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0"
                }
            }),
        );
        let resp = self.read_response(1);
        self.send_notification("notifications/initialized", json!({}));
        self.wait_for_roots_refresh();
        resp
    }

    /// Complete session, closing stdin, ensuring exit code 0 and verifying stdout purity.
    fn close_and_assert_purity(mut self) {
        drop(self.stdin.take());

        // Read remaining lines until EOF
        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .expect("read remaining stdout");
            if bytes_read == 0 {
                break;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                self.recorded_stdout.push(trimmed.to_string());
                let _: Value = serde_json::from_str(trimmed).unwrap_or_else(|e| {
                    panic!("remaining stdout contained non-JSON line: '{trimmed}': {e}")
                });
            }
        }

        let mut child = self.child.take().expect("child exists");
        let status = child.wait().expect("wait for child");
        assert!(
            status.success(),
            "server process did not exit cleanly: {status:?}"
        );

        // Extra verification: every single recorded line was valid JSON
        for line in &self.recorded_stdout {
            let res: Result<Value, _> = serde_json::from_str(line);
            assert!(
                res.is_ok(),
                "stdout purity violated: line is not valid JSON: {line}"
            );
        }
    }
}

#[test]
fn test_server_info_and_tools_list() {
    let temp_dir = TempDir::new().unwrap();
    let mut client = McpTestClient::spawn(
        &["--allow-dir", temp_dir.path().to_str().unwrap()],
        Vec::new(),
    );

    let init_resp = client.initialize();
    let result = init_resp.get("result").expect("init result");
    let server_info = result.get("serverInfo").expect("serverInfo");
    assert_eq!(server_info.get("name").unwrap(), "ashift");
    assert_eq!(
        server_info.get("version").unwrap(),
        env!("CARGO_PKG_VERSION")
    );
    let instructions = result
        .get("instructions")
        .expect("instructions")
        .as_str()
        .unwrap();
    assert!(instructions.contains("AriadShift document conversion server"));
    assert!(instructions.contains("allow-dir"));

    client.send_request(2, "tools/list", json!({}));
    let tools_resp = client.read_response(2);
    let tools = tools_resp["result"]["tools"]
        .as_array()
        .expect("tools array");
    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();

    assert!(tool_names.contains(&"list_engines"));
    assert!(tool_names.contains(&"inspect"));
    assert!(tool_names.contains(&"plan"));
    assert!(tool_names.contains(&"convert"));

    // Verify outputSchema is declared for all 4 tools (H5)
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            tool.get("outputSchema").is_some(),
            "tool {name} must declare outputSchema"
        );
        let output_schema = &tool["outputSchema"];
        assert!(
            output_schema.is_object(),
            "outputSchema for {name} must be an object schema"
        );
        match name {
            "list_engines" => assert_eq!(output_schema["type"], "array"),
            "inspect" | "plan" | "convert" => assert_eq!(output_schema["type"], "object"),
            _ => panic!("unexpected tool: {name}"),
        }
    }

    client.close_and_assert_purity();
}

#[test]
fn test_list_engines_tool_matches_cli() {
    let temp_dir = TempDir::new().unwrap();
    let mut client = McpTestClient::spawn(
        &["--allow-dir", temp_dir.path().to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    client.send_request(
        3,
        "tools/call",
        json!({
            "name": "list_engines",
            "arguments": {}
        }),
    );

    let resp = client.read_response(3);
    assert_eq!(resp["result"]["isError"], false);

    // Verify structuredContent matches CLI `ashift engines --json` (H5)
    let cli_output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .arg("engines")
        .arg("--json")
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path())
        .output()
        .expect("ashift engines --json");
    assert!(cli_output.status.success());
    let cli_json: Value =
        serde_json::from_slice(&cli_output.stdout).expect("parse cli engines json");

    let structured = &resp["result"]["structuredContent"];
    assert_eq!(
        structured, &cli_json,
        "structuredContent must match CLI --json value-for-value"
    );

    let content = resp["result"]["content"].as_array().unwrap();
    assert!(!content.is_empty());
    let structured_text = &content[0]["text"].as_str().unwrap();
    let engines_val: Value = serde_json::from_str(structured_text).unwrap();
    assert_eq!(&engines_val, &cli_json);

    client.close_and_assert_purity();
}

#[test]
fn test_inspect_and_plan_tools() {
    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = fs::canonicalize(temp_dir.path()).unwrap();
    let input_path = canonical_temp.join("test.md");
    fs::write(&input_path, "# Test Document\n\nHello AriadShift.").unwrap();

    let mut client = McpTestClient::spawn(
        &["--allow-dir", canonical_temp.to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    // 1. Test inspect
    client.send_request(
        4,
        "tools/call",
        json!({
            "name": "inspect",
            "arguments": {
                "input": input_path.to_str().unwrap()
            }
        }),
    );
    let inspect_resp = client.read_response(4);
    assert_eq!(inspect_resp["result"]["isError"], false);

    // Verify structuredContent matches CLI `ashift inspect <file> --json` (H5)
    let cli_inspect_output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .arg("inspect")
        .arg(&input_path)
        .arg("--json")
        .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path())
        .output()
        .expect("ashift inspect --json");
    assert!(cli_inspect_output.status.success());
    let cli_inspect_json: Value = serde_json::from_slice(&cli_inspect_output.stdout).unwrap();
    assert_eq!(
        &inspect_resp["result"]["structuredContent"], &cli_inspect_json,
        "inspect structuredContent must match CLI --json"
    );

    // 2. Test plan
    client.send_request(
        5,
        "tools/call",
        json!({
            "name": "plan",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "html"
            }
        }),
    );
    let plan_resp = client.read_response(5);
    assert_eq!(plan_resp["result"]["isError"], false);

    // Verify structuredContent matches CLI `ashift plan <file> --to html --json` (H5)
    let cli_plan_output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .arg("plan")
        .arg(&input_path)
        .arg("--to")
        .arg("html")
        .arg("--json")
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path())
        .output()
        .expect("ashift plan --json");
    assert!(cli_plan_output.status.success());
    let cli_plan_json: Value = serde_json::from_slice(&cli_plan_output.stdout).unwrap();
    assert_eq!(
        &plan_resp["result"]["structuredContent"], &cli_plan_json,
        "plan structuredContent must match CLI --json"
    );

    client.close_and_assert_purity();
}

#[test]
fn test_convert_and_resources_read() {
    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = fs::canonicalize(temp_dir.path()).unwrap();
    let input_path = canonical_temp.join("alice.md");
    fs::write(
        &input_path,
        "# Alice in Wonderland\n\nCuriouser and curiouser!",
    )
    .unwrap();

    let output_path = canonical_temp.join("alice.html");

    let mut client = McpTestClient::spawn(
        &["--allow-dir", canonical_temp.to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    // 1. Call convert
    client.send_request(
        6,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "html",
                "output": output_path.to_str().unwrap()
            }
        }),
    );

    let convert_resp = client.read_response(6);
    assert_eq!(convert_resp["result"]["isError"], false);
    assert!(output_path.exists(), "output file must be produced on disk");

    // Verify structuredContent format (matches ConvertOutput) (H5)
    let structured = &convert_resp["result"]["structuredContent"];
    assert!(
        structured["output"]
            .as_str()
            .unwrap()
            .ends_with("alice.html")
    );
    assert!(!structured["route"].as_array().unwrap().is_empty());

    // Check resource_link in content blocks
    let content = convert_resp["result"]["content"].as_array().unwrap();
    let mut found_resource_link = false;
    let expected_uri = path_to_file_uri(&output_path);

    for block in content {
        if block.get("type").and_then(Value::as_str) == Some("resource_link") {
            assert_eq!(block["uri"], expected_uri);
            assert!(block["mimeType"].as_str().unwrap().starts_with("text/html"));
            found_resource_link = true;
        }
    }
    assert!(
        found_resource_link,
        "convert response must contain resource_link"
    );

    // 2. Call resources/read on produced resource
    client.send_request(
        7,
        "resources/read",
        json!({
            "uri": expected_uri
        }),
    );
    let read_resp = client.read_response(7);
    let contents = read_resp["result"]["contents"]
        .as_array()
        .expect("contents array");
    assert!(!contents.is_empty());
    assert_eq!(contents[0]["uri"], expected_uri);
    let text = contents[0]["text"].as_str().expect("resource text");
    assert!(text.contains("Alice in Wonderland"));

    // 3. Call resources/read on unauthorized URI (not produced by this session)
    let foreign_uri = path_to_file_uri(&input_path);
    client.send_request(
        8,
        "resources/read",
        json!({
            "uri": foreign_uri
        }),
    );
    let foreign_resp = client.read_response(8);
    assert!(
        foreign_resp.get("error").is_some(),
        "must return error for foreign resource"
    );

    client.close_and_assert_purity();
}

#[test]
fn test_inline_markdown_behavior() {
    let temp_dir = TempDir::new().unwrap();
    let input_path = temp_dir.path().join("small.html");
    fs::write(
        &input_path,
        "<h1>Short note</h1><p>Content within 256 KiB.</p>",
    )
    .unwrap();
    let output_path = temp_dir.path().join("out_small.md");

    let mut client = McpTestClient::spawn(
        &["--allow-dir", temp_dir.path().to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    client.send_request(
        9,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "md",
                "output": output_path.to_str().unwrap()
            }
        }),
    );
    let resp = client.read_response(9);
    assert_eq!(resp["result"]["isError"], false);
    let content = resp["result"]["content"].as_array().unwrap();

    // Verify inline text block is included
    let has_inline_md = content.iter().any(|b| {
        b.get("type").and_then(Value::as_str) == Some("text")
            && b.get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("Short note"))
    });
    assert!(
        has_inline_md,
        "inline markdown must be included for output <= 256 KiB"
    );

    // Test large markdown (> 256 KiB)
    let large_input = temp_dir.path().join("large.html");
    let large_str = "A".repeat(300 * 1024);
    fs::write(&large_input, format!("<h1>Large</h1><p>{large_str}</p>")).unwrap();
    let large_output = temp_dir.path().join("out_large.md");

    client.send_request(
        10,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": large_input.to_str().unwrap(),
                "to": "md",
                "output": large_output.to_str().unwrap()
            }
        }),
    );
    let large_resp = client.read_response(10);
    assert_eq!(large_resp["result"]["isError"], false);
    let large_content = large_resp["result"]["content"].as_array().unwrap();

    // Verify raw markdown is omitted and explanatory note is provided
    let has_omitted_note = large_content.iter().any(|b| {
        b.get("type").and_then(Value::as_str) == Some("text")
            && b.get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("exceeds 256 KiB"))
    });
    assert!(
        has_omitted_note,
        "large markdown must include omission note"
    );

    client.close_and_assert_purity();
}

#[test]
fn test_confinement_input_outside_allowed_dir() {
    let allowed_dir = TempDir::new().unwrap();
    let outside_dir = TempDir::new().unwrap();

    let outside_file = outside_dir.path().join("secret.md");
    fs::write(&outside_file, "# Secret\n\nConfidential.").unwrap();

    let mut client = McpTestClient::spawn(
        &["--allow-dir", allowed_dir.path().to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    client.send_request(
        11,
        "tools/call",
        json!({
            "name": "inspect",
            "arguments": {
                "input": outside_file.to_str().unwrap()
            }
        }),
    );
    let resp = client.read_response(11);
    assert_eq!(resp["result"]["isError"], true);
    let err_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(err_text.contains("outside allowed") || err_text.contains("Access denied"));

    client.close_and_assert_purity();
}

#[test]
fn test_confinement_hidden_path_and_mismatched_extension() {
    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = fs::canonicalize(temp_dir.path()).unwrap();
    let input_path = canonical_temp.join("in.md");
    fs::write(&input_path, "# Heading").unwrap();

    let mut client = McpTestClient::spawn(
        &["--allow-dir", canonical_temp.to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    // 1. Hidden component .github
    let hidden_output = canonical_temp.join(".github").join("out.html");
    client.send_request(
        12,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "html",
                "output": hidden_output.to_str().unwrap()
            }
        }),
    );
    let hidden_resp = client.read_response(12);
    assert_eq!(hidden_resp["result"]["isError"], true);
    let hidden_err = hidden_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(hidden_err.contains("hidden"));

    // 2. Extension mismatch: to = html, output = out.docx
    let mismatch_output = canonical_temp.join("out.docx");
    client.send_request(
        13,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "html",
                "output": mismatch_output.to_str().unwrap()
            }
        }),
    );
    let mismatch_resp = client.read_response(13);
    assert_eq!(mismatch_resp["result"]["isError"], true);
    let mismatch_err = mismatch_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(mismatch_err.contains("extension"));

    client.close_and_assert_purity();
}

#[test]
fn test_confinement_overwrite_refusal() {
    let temp_dir = TempDir::new().unwrap();
    let input_path = temp_dir.path().join("doc.md");
    fs::write(&input_path, "# Doc").unwrap();

    let existing_output = temp_dir.path().join("existing.html");
    fs::write(&existing_output, "already exists").unwrap();

    // Server started WITHOUT --allow-overwrite
    let mut client = McpTestClient::spawn(
        &["--allow-dir", temp_dir.path().to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    client.send_request(
        14,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "html",
                "output": existing_output.to_str().unwrap(),
                "overwrite": true
            }
        }),
    );
    let resp = client.read_response(14);
    assert_eq!(resp["result"]["isError"], true);
    let err_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(err_text.contains("already exists"));

    client.close_and_assert_purity();
}

#[test]
fn test_empty_allowed_set_errors_with_hint() {
    let temp_dir = TempDir::new().unwrap();
    let file = temp_dir.path().join("any.md");
    fs::write(&file, "# Test").unwrap();

    // Server started without --allow-dir and with no client roots
    let mut client = McpTestClient::spawn(&[], Vec::new());
    client.initialize();

    client.send_request(
        15,
        "tools/call",
        json!({
            "name": "inspect",
            "arguments": {
                "input": file.to_str().unwrap()
            }
        }),
    );
    let resp = client.read_response(15);
    assert_eq!(resp["result"]["isError"], true);
    let err_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(err_text.contains("--allow-dir"));
    assert!(err_text.contains("roots"));

    client.close_and_assert_purity();
}

#[test]
fn test_roots_list_changed_updates_allowed_dirs() {
    let root1 = TempDir::new().unwrap();
    let root2 = TempDir::new().unwrap();

    let file2 = root2.path().join("in_root2.md");
    fs::write(&file2, "# Document in root 2").unwrap();

    // Start with root1 advertised
    let root1_uri = path_to_file_uri(root1.path());
    let mut client = McpTestClient::spawn(&[], vec![root1_uri]);
    client.initialize();

    // Calling inspect on file in root2 should fail initially
    client.send_request(
        16,
        "tools/call",
        json!({
            "name": "inspect",
            "arguments": {
                "input": file2.to_str().unwrap()
            }
        }),
    );
    let resp1 = client.read_response(16);
    assert_eq!(resp1["result"]["isError"], true);

    // Update advertised roots on client and send notification
    let root2_uri = path_to_file_uri(root2.path());
    client.update_roots(vec![root2_uri]);
    client.send_notification("notifications/roots/list_changed", json!({}));

    // Wait for roots/list request cycle to complete and update allowed dirs
    client.wait_for_roots_refresh();

    // Now calling inspect on file in root2 should succeed!
    client.send_request(
        17,
        "tools/call",
        json!({
            "name": "inspect",
            "arguments": {
                "input": file2.to_str().unwrap()
            }
        }),
    );
    let resp2 = client.read_response(17);
    assert_eq!(resp2["result"]["isError"], false);

    client.close_and_assert_purity();
}

#[test]
fn test_error_returns_structured_content() {
    let temp_dir = TempDir::new().unwrap();
    let mut client = McpTestClient::spawn(
        &["--allow-dir", temp_dir.path().to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    let missing_path = temp_dir.path().join("missing.md");
    client.send_request(
        20,
        "tools/call",
        json!({
            "name": "inspect",
            "arguments": {
                "input": missing_path.to_str().unwrap()
            }
        }),
    );
    let resp = client.read_response(20);
    assert_eq!(resp["result"]["isError"], true);
    let structured = &resp["result"]["structuredContent"];
    assert!(
        structured.is_object(),
        "structuredContent must be present on error"
    );
    assert!(structured.get("code").is_some());
    assert_eq!(structured["exit_code"], 1);
    assert!(structured.get("message").is_some());

    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("ashift: "),
        "error text must start with ashift prefix: {text}"
    );

    client.close_and_assert_purity();
}

#[test]
fn test_tool_input_validation_enums() {
    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = fs::canonicalize(temp_dir.path()).unwrap();
    let input_path = canonical_temp.join("doc.md");
    fs::write(&input_path, "# Hello").unwrap();
    let output_path = canonical_temp.join("doc.html");

    let mut client = McpTestClient::spawn(
        &["--allow-dir", canonical_temp.to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    // 1. Invalid profile "bogus"
    client.send_request(
        21,
        "tools/call",
        json!({
            "name": "plan",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "html",
                "profile": "bogus"
            }
        }),
    );
    let resp1 = client.read_response(21);
    assert_eq!(resp1["result"]["isError"], true);
    let structured1 = &resp1["result"]["structuredContent"];
    assert_eq!(structured1["exit_code"], 2);
    assert_eq!(structured1["code"], "invalid_profile");
    let err_msg1 = resp1["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        err_msg1.contains("invalid value 'bogus' for '--profile <PROFILE>'"),
        "expected CLI profile error message, got: {err_msg1}"
    );

    // 2. Invalid target format "pdf" in convert
    client.send_request(
        22,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "pdf",
                "output": output_path.to_str().unwrap()
            }
        }),
    );
    let resp2 = client.read_response(22);
    assert_eq!(resp2["result"]["isError"], true);
    let structured2 = &resp2["result"]["structuredContent"];
    assert_eq!(structured2["exit_code"], 3);
    assert_eq!(structured2["code"], "unsupported_route");
    let err_msg2 = resp2["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        err_msg2.contains("unsupported conversion route; unknown target format 'pdf'"),
        "expected CLI unknown target format error message, got: {err_msg2}"
    );
    assert!(
        err_msg2.contains("reachable targets from md: docx, epub, html"),
        "expected reachable targets list, got: {err_msg2}"
    );

    // 3. Invalid target format "pdf" in plan
    client.send_request(
        23,
        "tools/call",
        json!({
            "name": "plan",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "pdf"
            }
        }),
    );
    let resp3 = client.read_response(23);
    assert_eq!(resp3["result"]["isError"], true);
    let structured3 = &resp3["result"]["structuredContent"];
    assert_eq!(structured3["exit_code"], 3);
    assert_eq!(structured3["code"], "unsupported_route");
    let err_msg3 = resp3["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        err_msg3.contains("unsupported conversion route; unknown target format 'pdf'"),
        "expected CLI plan unknown target format error message, got: {err_msg3}"
    );

    client.close_and_assert_purity();
}

#[test]
fn test_protocol_revisions_roots() {
    // Test 2025-11-25 and 2026-07-28 protocol version roots handling
    for version in ["2025-11-25", "2026-07-28"] {
        let root_dir = TempDir::new().unwrap();
        let canonical_root = fs::canonicalize(root_dir.path()).unwrap();
        let input_path = canonical_root.join("test_rev.md");
        fs::write(&input_path, "# Revision Test\n\nContent.").unwrap();

        let root_uri = path_to_file_uri(&canonical_root);
        let mut client = McpTestClient::spawn(&[], vec![root_uri]);
        client.initialize_with_version(version);

        client.send_request(
            25,
            "tools/call",
            json!({
                "name": "inspect",
                "arguments": {
                    "input": input_path.to_str().unwrap()
                }
            }),
        );

        let resp = client.read_response(25);
        assert_eq!(
            resp["result"]["isError"], false,
            "inspect should succeed under protocol version {version}"
        );
        assert_eq!(resp["result"]["structuredContent"]["format"], "markdown");

        client.close_and_assert_purity();
    }
}

#[test]
fn test_resources_read_recheck_revocation_and_deletion() {
    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = fs::canonicalize(temp_dir.path()).unwrap();
    let input_path = canonical_temp.join("res.md");
    fs::write(&input_path, "# Resource File\n\nContent for reading.").unwrap();
    let output_path = canonical_temp.join("res.html");

    let root_uri = path_to_file_uri(&canonical_temp);
    let mut client = McpTestClient::spawn(&[], vec![root_uri.clone()]);
    client.initialize();

    // 1. Convert to generate a session resource
    client.send_request(
        30,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "html",
                "output": output_path.to_str().unwrap()
            }
        }),
    );
    let convert_resp = client.read_response(30);
    assert_eq!(convert_resp["result"]["isError"], false);

    let canonical_output = fs::canonicalize(&output_path).unwrap();
    let resource_uri = path_to_file_uri(&canonical_output);

    // 2. Read resource succeeds
    client.send_request(31, "resources/read", json!({ "uri": resource_uri }));
    let read_resp = client.read_response(31);
    assert!(read_resp.get("result").is_some());
    assert!(
        read_resp["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Resource File")
    );

    // 3. Revoke root: client advertises a different root
    let other_temp = TempDir::new().unwrap();
    let other_uri = path_to_file_uri(other_temp.path());
    client.update_roots(vec![other_uri]);
    client.send_notification("notifications/roots/list_changed", json!({}));
    client.wait_for_roots_refresh();

    // 4. Read resource again -> must fail because root was revoked
    client.send_request(32, "resources/read", json!({ "uri": resource_uri }));
    let revoked_resp = client.read_response(32);
    assert!(
        revoked_resp.get("error").is_some(),
        "read_resource must refuse read when root is revoked"
    );

    // 5. Restore root, but delete file on disk
    client.update_roots(vec![root_uri]);
    client.send_notification("notifications/roots/list_changed", json!({}));
    client.wait_for_roots_refresh();

    fs::remove_file(&output_path).unwrap();

    // 6. Read resource again -> must fail because file was deleted
    client.send_request(33, "resources/read", json!({ "uri": resource_uri }));
    let deleted_resp = client.read_response(33);
    assert!(
        deleted_resp.get("error").is_some(),
        "read_resource must refuse read when file is deleted"
    );

    client.close_and_assert_purity();
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: std::time::Duration,
) -> Result<std::process::ExitStatus, String> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(30)),
            Err(e) => return Err(format!("try_wait failed: {e}")),
        }
    }
    Err("process did not exit within timeout".to_string())
}

#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .is_ok_and(|out| out.status.success())
}

#[cfg(unix)]
fn find_hanging_engine(parent_pid: u32, timeout: std::time::Duration) -> Option<u32> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.flatten() {
                if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() {
                    let stat_path = format!("/proc/{pid}/stat");
                    let Ok(stat) = fs::read_to_string(&stat_path) else {
                        continue;
                    };
                    let Some(paren_idx) = stat.rfind(')') else {
                        continue;
                    };
                    let rest = &stat[paren_idx + 2..];
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if parts.len() < 2 {
                        continue;
                    }
                    let state = parts[0];
                    let Ok(ppid) = parts[1].parse::<u32>() else {
                        continue;
                    };
                    if ppid == parent_pid && state != "Z" {
                        let cmdline_path = format!("/proc/{pid}/cmdline");
                        if let Ok(cmdline) = fs::read_to_string(&cmdline_path)
                            && cmdline.contains("__engine")
                            && is_process_alive(pid)
                        {
                            std::thread::sleep(std::time::Duration::from_millis(60));
                            if is_process_alive(pid) {
                                return Some(pid);
                            }
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    None
}

#[test]
fn test_stdin_eof_and_cancellation() {
    let temp_dir = TempDir::new().unwrap();
    let mut client = McpTestClient::spawn(
        &["--allow-dir", temp_dir.path().to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();

    // Send a cancellation notification for a non-existent request
    client.send_notification(
        "notifications/cancelled",
        json!({
            "requestId": 9999,
            "reason": "user requested cancellation"
        }),
    );

    // Close stdin and assert pure exit
    client.close_and_assert_purity();
}

#[cfg(unix)]
#[test]
fn test_stdin_eof_with_hanging_convert_cleans_engine_and_workspace() {
    for iter in 0..10 {
        let temp = tempfile::tempdir().expect("create test tempdir");
        let canonical_temp = fs::canonicalize(temp.path()).unwrap();
        let isolated_tmp = canonical_temp.join(format!("tmp_root_{iter}"));
        fs::create_dir(&isolated_tmp).expect("create isolated tmp");

        let input_file = canonical_temp.join(format!("doc_{iter}.md"));
        fs::write(&input_file, "# Test Document\n\nContent for EOF test.\n").unwrap();
        let output_file = canonical_temp.join(format!("doc_{iter}.docx"));

        let mut client = McpTestClient::spawn_with_env(
            &["--allow-dir", canonical_temp.to_str().unwrap()],
            Vec::new(),
            &[
                ("ARIAD_TEST_ENGINE", std::ffi::OsStr::new("hang")),
                ("TMPDIR", isolated_tmp.as_os_str()),
            ],
        );
        client.initialize();

        let ashift_pid = client.child_pid();

        // Start hanging convert
        client.send_request(
            2,
            "tools/call",
            json!({
                "name": "convert",
                "arguments": {
                    "input": input_file.to_str().unwrap(),
                    "to": "docx",
                    "output": output_file.to_str().unwrap()
                }
            }),
        );

        // Wait until engine child process is running
        let engine_pid = find_hanging_engine(ashift_pid, std::time::Duration::from_secs(5))
            .expect("engine process should have been spawned by ashift mcp");
        assert!(
            is_process_alive(engine_pid),
            "engine must be alive before EOF"
        );

        // Close stdin (EOF)
        client.close_stdin();

        // Assert ashift exits cleanly within timeout (bounded wait up to 5s)
        let mut child = client.child.take().expect("child exists");
        let exit_status = wait_with_timeout(&mut child, std::time::Duration::from_secs(5))
            .expect("ashift should exit within timeout after stdin EOF");
        assert!(
            exit_status.success(),
            "ashift should exit cleanly on EOF in iter {iter}: {exit_status:?}"
        );

        // Assert engine process is terminated (poll up to 3s)
        let engine_dead_start = std::time::Instant::now();
        let mut engine_alive = true;
        while engine_dead_start.elapsed() < std::time::Duration::from_secs(3) {
            if !is_process_alive(engine_pid) {
                engine_alive = false;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            !engine_alive,
            "engine process {engine_pid} must be dead after EOF in iter {iter}"
        );

        // Assert no ariadshift-* workspace directories remain in isolated_tmp
        let entries: Vec<_> = fs::read_dir(&isolated_tmp)
            .expect("read isolated_tmp")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("ariadshift-"))
            .collect();
        assert!(
            entries.is_empty(),
            "workspace directories remained after EOF in iter {iter}: {:?}",
            entries.iter().map(|e| e.path()).collect::<Vec<_>>()
        );
    }
}

#[cfg(unix)]
#[test]
fn test_real_request_cancellation_preserves_server_and_cleans_engine() {
    let temp = tempfile::tempdir().expect("create test tempdir");
    let canonical_temp = fs::canonicalize(temp.path()).unwrap();
    let isolated_tmp = canonical_temp.join("tmp_root_cancel");
    fs::create_dir(&isolated_tmp).expect("create isolated tmp");

    let input_file = canonical_temp.join("doc_cancel.md");
    fs::write(&input_file, "# Test Document\n\nContent for cancel test.\n").unwrap();
    let output_file = canonical_temp.join("doc_cancel.docx");

    let mut client = McpTestClient::spawn_with_env(
        &["--allow-dir", canonical_temp.to_str().unwrap()],
        Vec::new(),
        &[
            ("ARIAD_TEST_ENGINE", std::ffi::OsStr::new("hang")),
            ("TMPDIR", isolated_tmp.as_os_str()),
        ],
    );
    client.initialize();

    let ashift_pid = client.child_pid();

    // Start hanging convert with request ID 2
    client.send_request(
        2,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_file.to_str().unwrap(),
                "to": "docx",
                "output": output_file.to_str().unwrap()
            }
        }),
    );

    // Wait until engine child process is running
    let engine_pid = find_hanging_engine(ashift_pid, std::time::Duration::from_secs(5))
        .expect("engine process should have been spawned by ashift mcp");
    assert!(
        is_process_alive(engine_pid),
        "engine must be alive before cancellation"
    );

    // Send notifications/cancelled for request ID 2
    client.send_notification(
        "notifications/cancelled",
        json!({
            "requestId": 2,
            "reason": "user requested cancellation"
        }),
    );

    // Assert engine process is terminated (poll up to 3s)
    let engine_dead_start = std::time::Instant::now();
    let mut engine_alive = true;
    while engine_dead_start.elapsed() < std::time::Duration::from_secs(3) {
        if !is_process_alive(engine_pid) {
            engine_alive = false;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        !engine_alive,
        "engine process {engine_pid} must be dead after cancellation"
    );

    // Assert no ariadshift-* workspace directories remain in isolated_tmp
    let entries: Vec<_> = fs::read_dir(&isolated_tmp)
        .expect("read isolated_tmp")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("ariadshift-"))
        .collect();
    assert!(
        entries.is_empty(),
        "workspace directories remained after cancellation: {:?}",
        entries.iter().map(|e| e.path()).collect::<Vec<_>>()
    );

    // Verify server remains alive and responds to subsequent requests
    client.send_request(3, "tools/list", json!({}));
    let resp3 = client.read_response(3);
    assert!(
        resp3["result"]["tools"].as_array().is_some(),
        "server must remain healthy and answer tools/list after request cancellation"
    );

    client.close_and_assert_purity();
}
