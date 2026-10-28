//! Integration tests for `ashift mcp` stdio server.
//!
//! Tests verify JSON-RPC protocol compliance, stdout purity,
//! tool schema & invocation matching CLI outputs, path confinement,
//! capability-based promotion, and session-scoped resources.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
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

#[derive(Clone, Copy)]
enum RootsFailure {
    Error,
    Timeout,
}

/// Helper client for testing `ashift mcp` over stdio.
struct McpTestClient {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: std::sync::mpsc::Receiver<std::io::Result<String>>,
    pending_roots: Vec<Value>,
    read_timeout: std::time::Duration,
    recorded_stdout: Vec<String>,
    advertised_roots: Vec<String>,
    roots_queries_count: usize,
    roots_failure: Option<RootsFailure>,
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
        let (sender, reader) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match stdout.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });

        Self {
            child: Some(child),
            stdin: Some(stdin),
            reader,
            pending_roots: Vec::new(),
            read_timeout: std::time::Duration::from_secs(60),
            recorded_stdout: Vec::new(),
            advertised_roots,
            roots_queries_count: 0,
            roots_failure: None,
        }
    }

    #[cfg(unix)]
    fn child_pid(&self) -> u32 {
        self.child.as_ref().expect("child exists").id()
    }

    #[cfg(unix)]
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
    fn read_message_until(&mut self, deadline: std::time::Instant) -> Value {
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "server response deadline elapsed"
            );
            let line = self
                .reader
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("bounded read from server stdout")
                .expect("read server stdout");

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
                self.roots_queries_count += 1;
                if let Some(failure) = self.roots_failure {
                    if matches!(failure, RootsFailure::Error) {
                        self.write_line(
                            &json!({
                                "jsonrpc": "2.0", "id": req_id,
                                "error": { "code": -32603, "message": "roots unavailable" }
                            })
                            .to_string(),
                        );
                    } else {
                        self.pending_roots.push(req_id.clone());
                    }
                    continue;
                }
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
                self.write_line(&resp.to_string());
                continue;
            }

            return val;
        }
    }

    /// Assert a new roots request arrived; successful replies must become observable grants.
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
        assert!(
            self.roots_queries_count > initial,
            "server never requested refreshed roots"
        );
        if self.roots_failure.is_none()
            && let Some(root) = self.advertised_roots.iter().find_map(|uri| {
                ariad_host::confine::parse_file_uri(uri)
                    .ok()
                    .filter(|path| path.is_dir())
            })
        {
            let mut probe = tempfile::Builder::new()
                .prefix("roots-ready-")
                .suffix(".md")
                .tempfile_in(root)
                .unwrap();
            probe.write_all(b"# Roots ready").unwrap();
            self.wait_for_input_state(probe.path(), true);
        }
    }

    /// Waits for an observable authorization decision, including asynchronous roots installation.
    fn wait_for_input_state(&mut self, input: &std::path::Path, allowed: bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(13);
        loop {
            self.send_request(
                899,
                "tools/call",
                json!({"name":"inspect", "arguments":{"input":input}}),
            );
            let response = self.read_response_until(899, deadline);
            if response["result"]["isError"] == !allowed {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "authorization state was not installed: {response}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Reads messages until a response matching `expected_id` is found.
    fn read_response(&mut self, expected_id: u64) -> Value {
        self.read_response_until(expected_id, std::time::Instant::now() + self.read_timeout)
    }

    fn read_response_until(&mut self, expected_id: u64, deadline: std::time::Instant) -> Value {
        loop {
            let msg = self.read_message_until(deadline);
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

        // Bound the entire drain and process exit, including a child that closes stdout early.
        loop {
            let line = match self
                .reader
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(line) => line.expect("read remaining stdout"),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(error) => panic!("server did not close stdout promptly: {error}"),
            };
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                self.recorded_stdout.push(trimmed.to_string());
                let _: Value = serde_json::from_str(trimmed).unwrap_or_else(|e| {
                    panic!("remaining stdout contained non-JSON line: '{trimmed}': {e}")
                });
            }
        }

        let child = self.child.as_mut().expect("child exists");
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll child exit") {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("server closed stdout but did not exit promptly");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        drop(self.child.take());
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
fn test_convert_calls_share_the_engine_pool_and_leave_no_sessions_behind() {
    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = fs::canonicalize(temp_dir.path()).unwrap();
    let isolated_tmp = canonical_temp.join("tmp_root");
    fs::create_dir(&isolated_tmp).unwrap();
    let mut client = McpTestClient::spawn_with_env(
        &["--allow-dir", canonical_temp.to_str().unwrap()],
        Vec::new(),
        &[
            ("TMPDIR", isolated_tmp.as_os_str()),
            ("TMP", isolated_tmp.as_os_str()),
            ("TEMP", isolated_tmp.as_os_str()),
        ],
    );
    client.initialize();

    for (id, name) in [(20, "first"), (21, "second"), (22, "third")] {
        let input = canonical_temp.join(format!("{name}.md"));
        fs::write(&input, format!("# {name}\n\nBody.\n")).unwrap();
        client.send_request(
            id,
            "tools/call",
            json!({
                "name": "convert",
                "arguments": {"input": input.to_str().unwrap(), "to": "docx"}
            }),
        );
        let response = client.read_response(id);
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert!(canonical_temp.join(format!("{name}.docx")).is_file());
    }
    client.close_and_assert_purity();

    let leftovers: Vec<_> = fs::read_dir(&isolated_tmp)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("ariadshift-")
        })
        .map(|entry| entry.path())
        .collect();
    assert!(
        leftovers.is_empty(),
        "workspaces and session directories are removed at shutdown: {leftovers:?}"
    );
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
fn test_convert_refuses_hidden_sidecar_images_and_embeds_visible_ones() {
    use sha2::{Digest, Sha256};

    const VISIBLE_PNG: &[u8] = b"\x89PNG\r\n\x1a\nvisible sibling";
    const HIDDEN_DIR_PNG: &[u8] = b"\x89PNG\r\n\x1a\nhidden directory";
    const HIDDEN_FILE_PNG: &[u8] = b"\x89PNG\r\n\x1a\nhidden file";

    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = fs::canonicalize(temp_dir.path()).unwrap();
    let input_path = canonical_temp.join("note.md");
    fs::write(
        &input_path,
        "# Note\n\n![visible](photo.png)\n\n![hidden directory](.private/photo.png)\n\n\
         ![hidden file](.photo.png)\n",
    )
    .unwrap();
    fs::write(canonical_temp.join("photo.png"), VISIBLE_PNG).unwrap();
    fs::create_dir(canonical_temp.join(".private")).unwrap();
    fs::write(
        canonical_temp.join(".private").join("photo.png"),
        HIDDEN_DIR_PNG,
    )
    .unwrap();
    fs::write(canonical_temp.join(".photo.png"), HIDDEN_FILE_PNG).unwrap();
    let output_path = canonical_temp.join("note.json");

    let mut client = McpTestClient::spawn(
        &["--allow-dir", canonical_temp.to_str().unwrap()],
        Vec::new(),
    );
    client.initialize();
    client.send_request(
        30,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "json",
                "output": output_path.to_str().unwrap()
            }
        }),
    );

    let response = client.read_response(30);
    assert_eq!(response["result"]["isError"], false);
    let warnings = response["result"]["structuredContent"]["warnings"]
        .as_array()
        .expect("warnings array");
    let not_embedded = warnings
        .iter()
        .filter(|warning| warning["code"] == "image_not_embedded")
        .count();
    assert_eq!(
        not_embedded, 2,
        "both hidden image references must be refused"
    );

    let ir = fs::read_to_string(&output_path).unwrap();
    assert!(ir.contains(&hex::encode(Sha256::digest(VISIBLE_PNG))));
    assert!(!ir.contains(&hex::encode(Sha256::digest(HIDDEN_DIR_PNG))));
    assert!(!ir.contains(&hex::encode(Sha256::digest(HIDDEN_FILE_PNG))));

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

    // 2. Invalid target format "rst" in convert
    client.send_request(
        22,
        "tools/call",
        json!({
            "name": "convert",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "rst",
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
        err_msg2.contains("unsupported conversion route; unknown target format 'rst'"),
        "expected CLI unknown target format error message, got: {err_msg2}"
    );
    assert!(
        err_msg2.contains("reachable targets from md: docx, epub, html, json"),
        "expected reachable targets list, got: {err_msg2}"
    );

    // 3. Invalid target format "rst" in plan
    client.send_request(
        23,
        "tools/call",
        json!({
            "name": "plan",
            "arguments": {
                "input": input_path.to_str().unwrap(),
                "to": "rst"
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
        err_msg3.contains("unsupported conversion route; unknown target format 'rst'"),
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

#[cfg(unix)]
struct EngineGuard(Option<u32>);

#[cfg(unix)]
impl Drop for EngineGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
    }
}

#[cfg(unix)]
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
    let min_sustained = std::time::Duration::from_millis(750);
    let mut candidate: Option<(u32, std::time::Instant)> = None;

    while start.elapsed() < timeout {
        if let Ok(output) = Command::new("ps")
            .args(["-axo", "pid=,ppid=,stat=,command="])
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut procs = Vec::new();
            for line in stdout.lines() {
                let trimmed = line.trim_start();
                if trimmed.is_empty() {
                    continue;
                }
                let mut parts = trimmed.split_whitespace();
                let Some(pid_str) = parts.next() else {
                    continue;
                };
                let Some(ppid_str) = parts.next() else {
                    continue;
                };
                let Some(stat_str) = parts.next() else {
                    continue;
                };
                let Ok(pid) = pid_str.parse::<u32>() else {
                    continue;
                };
                let Ok(ppid) = ppid_str.parse::<u32>() else {
                    continue;
                };
                if stat_str.contains('Z') {
                    continue;
                }
                let after_pid = trimmed[pid_str.len()..].trim_start();
                let after_ppid = after_pid[ppid_str.len()..].trim_start();
                let cmdline = if after_ppid.len() >= stat_str.len() {
                    after_ppid[stat_str.len()..].trim_start()
                } else {
                    ""
                };
                procs.push((pid, ppid, cmdline));
            }

            let mut descendants = std::collections::HashSet::new();
            let mut queue = vec![parent_pid];
            while let Some(current) = queue.pop() {
                for &(pid, ppid, _) in &procs {
                    if ppid == current && descendants.insert(pid) {
                        queue.push(pid);
                    }
                }
            }

            let mut engine_pids = Vec::new();
            for &(pid, _, cmdline) in &procs {
                if descendants.contains(&pid)
                    && cmdline.contains("__engine")
                    && is_process_alive(pid)
                {
                    engine_pids.push(pid);
                }
            }

            if let Some((cand_pid, cand_since)) = candidate {
                if engine_pids.contains(&cand_pid) && is_process_alive(cand_pid) {
                    if cand_since.elapsed() >= min_sustained {
                        return Some(cand_pid);
                    }
                } else {
                    // Candidate exited or died (e.g. transient describe child)
                    candidate = None;
                }
            }

            if candidate.is_none()
                && let Some(&first_engine_pid) = engine_pids.first()
            {
                candidate = Some((first_engine_pid, std::time::Instant::now()));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
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
        let mut _engine_guard = EngineGuard(Some(engine_pid));
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
        _engine_guard.0 = None;

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
    let mut _engine_guard = EngineGuard(Some(engine_pid));
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

    // Engine exit precedes host reaping and workspace removal. Wait for both
    // cancellation effects within one deadline, without closing the server.
    let cleanup_start = std::time::Instant::now();
    let (engine_alive, entries) = loop {
        let engine_alive = is_process_alive(engine_pid);
        let entries: Vec<_> = fs::read_dir(&isolated_tmp)
            .expect("read isolated_tmp")
            .map(|entry| entry.expect("read workspace entry"))
            .filter(|e| e.file_name().to_string_lossy().starts_with("ariadshift-"))
            .map(|entry| entry.path())
            .collect();
        if (!engine_alive && entries.is_empty())
            || cleanup_start.elapsed() >= std::time::Duration::from_secs(3)
        {
            break (engine_alive, entries);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(
        !engine_alive,
        "engine process {engine_pid} must be dead after cancellation"
    );
    _engine_guard.0 = None;

    assert!(
        entries.is_empty(),
        "workspace directories remained after cancellation: {entries:?}"
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

mod limits_and_roots {
    use super::*;
    use std::time::{Duration, Instant};

    fn tool(client: &mut McpTestClient, name: &str, args: Value) -> Value {
        client.send_request(
            700,
            "tools/call",
            json!({ "name": name, "arguments": args }),
        );
        client.read_response(700)
    }

    fn large_analysis_matches_cli(name: &str) {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("large.md");
        fs::File::create(&input)
            .unwrap()
            .set_len(ariad_host::convert::ANALYSIS_MAX_BYTES + 1)
            .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
        command
            .arg(name)
            .arg(&input)
            .arg("--json")
            .env("ASHIFT_PANDOC", pandoc_path())
            .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path());
        if name == "plan" {
            command.args(["--to", "html"]);
        }
        let cli = command.output().unwrap();
        assert!(cli.status.success(), "{cli:?}");
        let expected: Value = serde_json::from_slice(&cli.stdout).unwrap();
        if name == "inspect" {
            assert!(
                expected["warnings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|w| w["code"] == "document_too_large")
            );
        }
        let mut client =
            McpTestClient::spawn(&["--allow-dir", dir.path().to_str().unwrap()], vec![]);
        client.initialize();
        let response = tool(&mut client, name, json!({ "input": input, "to": "html" }));
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert_eq!(response["result"]["structuredContent"], expected);
        client.close_and_assert_purity();
    }

    #[test]
    fn inspect_above_analysis_limit_matches_cli() {
        large_analysis_matches_cli("inspect");
    }

    #[test]
    fn plan_above_analysis_limit_matches_cli() {
        large_analysis_matches_cli("plan");
    }

    #[test]
    fn convert_above_analysis_limit_succeeds() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("large.md");
        let mut file = fs::File::create(&input).unwrap();
        file.write_all(b"# Large document\n\n").unwrap();
        let spaces = [b' '; 64 * 1024];
        for _ in 0..512 {
            file.write_all(&spaces).unwrap();
        }
        file.write_all(b"\n").unwrap();
        assert!(file.metadata().unwrap().len() > ariad_host::convert::ANALYSIS_MAX_BYTES);
        let mut client =
            McpTestClient::spawn(&["--allow-dir", dir.path().to_str().unwrap()], vec![]);
        client.initialize();
        let output = dir.path().join("large.html");
        let response = tool(
            &mut client,
            "convert",
            json!({
                "input": input, "to": "html", "output": output
            }),
        );
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert!(
            fs::read_to_string(output)
                .unwrap()
                .contains("Large document")
        );
        client.close_and_assert_purity();
    }

    #[cfg(feature = "test-probe")]
    #[test]
    fn invalid_capabilities_uses_host_error_codes() {
        let dir = TempDir::new().unwrap();
        let caps = dir.path().join("invalid.json");
        fs::write(&caps, "invalid json").unwrap();
        let input = dir.path().join("note.md");
        fs::write(&input, "# Note").unwrap();
        let mut client = McpTestClient::spawn_with_env(
            &["--allow-dir", dir.path().to_str().unwrap()],
            vec![],
            &[("ASHIFT_TEST_CAPABILITIES", caps.as_os_str())],
        );
        client.initialize();
        let error = ariad_host::convert::ConvertError::InvalidCapabilities(String::new());
        for name in ["list_engines", "inspect", "plan"] {
            let response = tool(&mut client, name, json!({"input": input, "to": "html"}));
            let structured = &response["result"]["structuredContent"];
            assert_eq!(structured["code"], error.code(), "{response}");
            assert_eq!(structured["exit_code"], error.exit_code(), "{response}");
        }
        client.close_and_assert_purity();
    }

    #[test]
    fn large_analysis_still_validates_utf8_past_the_limit() {
        use std::io::{Seek, SeekFrom};
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("invalid.md");
        let mut file = fs::File::create(&input).unwrap();
        file.seek(SeekFrom::Start(ariad_host::convert::ANALYSIS_MAX_BYTES))
            .unwrap();
        file.write_all(&[0xff]).unwrap();
        let mut client =
            McpTestClient::spawn(&["--allow-dir", dir.path().to_str().unwrap()], vec![]);
        client.initialize();
        for name in ["inspect", "plan"] {
            let response = tool(&mut client, name, json!({"input": input, "to": "html"}));
            assert_eq!(response["result"]["isError"], true, "{response}");
            assert_eq!(response["result"]["structuredContent"]["code"], "input_io");
            assert_eq!(response["result"]["structuredContent"]["exit_code"], 1);
        }
        client.close_and_assert_purity();
    }

    #[test]
    fn initial_roots_error_keeps_explicit_allow_dirs() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("note.md");
        fs::write(&input, "# Explicit directory").unwrap();
        let mut client =
            McpTestClient::spawn(&["--allow-dir", dir.path().to_str().unwrap()], vec![]);
        client.roots_failure = Some(RootsFailure::Error);
        client.initialize();
        let response = tool(&mut client, "inspect", json!({"input": input}));
        assert_eq!(response["result"]["isError"], false, "{response}");
        client.close_and_assert_purity();
    }

    fn failed_refresh_revokes_root(keep_allow_dir: bool, unanswered: bool) {
        let root = TempDir::new().unwrap();
        let explicit = TempDir::new().unwrap();
        let input = root.path().join("note.md");
        let other = explicit.path().join("other.md");
        fs::write(&input, "# Root document").unwrap();
        fs::write(&other, "# Explicit directory").unwrap();
        let args = if keep_allow_dir {
            vec!["--allow-dir", explicit.path().to_str().unwrap()]
        } else {
            vec![]
        };
        let mut client = McpTestClient::spawn(&args, vec![path_to_file_uri(root.path())]);
        client.initialize();
        let output = root.path().join("note.html");
        let converted = tool(
            &mut client,
            "convert",
            json!({
                "input": input, "output": output, "to": "html"
            }),
        );
        assert_eq!(converted["result"]["isError"], false, "{converted}");
        let uri = path_to_file_uri(&output);
        client.send_request(701, "resources/read", json!({"uri": uri}));
        assert!(client.read_response(701).get("result").is_some());
        client.roots_failure = Some(if unanswered {
            RootsFailure::Timeout
        } else {
            RootsFailure::Error
        });
        client.send_notification("notifications/roots/list_changed", json!({}));
        client.wait_for_roots_refresh();
        let start = Instant::now();
        client.wait_for_input_state(&input, false);
        assert!(
            start.elapsed() < Duration::from_secs(if unanswered { 12 } else { 2 }),
            "roots revocation deadline elapsed"
        );
        client.send_request(702, "resources/read", json!({"uri": uri}));
        assert!(client.read_response(702).get("error").is_some());
        if keep_allow_dir {
            let inspected = tool(&mut client, "inspect", json!({"input": other}));
            assert_eq!(inspected["result"]["isError"], false, "{inspected}");
        }
        client.roots_failure = None;
        client.send_notification("notifications/roots/list_changed", json!({}));
        client.wait_for_roots_refresh();
        client.wait_for_input_state(&input, true);
        let restored = tool(&mut client, "inspect", json!({"input": input}));
        assert_eq!(restored["result"]["isError"], false, "{restored}");
        client.close_and_assert_purity();
    }

    #[test]
    fn roots_error_revokes_tools_and_resources() {
        failed_refresh_revokes_root(false, false);
    }

    #[test]
    fn roots_error_preserves_explicit_allow_dirs() {
        failed_refresh_revokes_root(true, false);
    }

    #[test]
    fn roots_timeout_revokes_tools_and_resources() {
        failed_refresh_revokes_root(true, true);
    }
}

mod concurrent_roots {
    use super::*;
    use std::time::{Duration, Instant};

    fn stale_completion(
        initialization: bool,
        newer_timeout: bool,
        newer_success: bool,
        older_error: bool,
    ) {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("note.md");
        fs::write(&input, "# Authorized").unwrap();
        let newest = TempDir::new().unwrap();
        let newest_input = newest.path().join("newest.md");
        fs::write(&newest_input, "# Newest root").unwrap();
        let new_grant = older_error || initialization;
        let root = path_to_file_uri(dir.path());
        let mut client = McpTestClient::spawn(&[], vec![root.clone()]);
        if initialization {
            client.roots_failure = Some(RootsFailure::Timeout);
        }
        client.initialize();
        if !initialization {
            client.wait_for_input_state(&input, true);
            client.roots_failure = Some(RootsFailure::Timeout);
            client.send_notification("notifications/roots/list_changed", json!({}));
            client.wait_for_roots_refresh();
        }
        let old_id = client
            .pending_roots
            .pop()
            .expect("older roots request is outstanding");
        client.roots_failure = if newer_timeout {
            Some(RootsFailure::Timeout)
        } else if newer_success || new_grant {
            None
        } else {
            Some(RootsFailure::Error)
        };
        if new_grant {
            client.update_roots(vec![path_to_file_uri(newest.path())]);
        } else if newer_success {
            client.update_roots(vec![]);
        }
        client.send_notification("notifications/roots/list_changed", json!({}));
        client.wait_for_roots_refresh();
        let observed_input = if new_grant { &newest_input } else { &input };
        client.wait_for_input_state(observed_input, new_grant);
        client.wait_for_input_state(&input, false);
        let response = if older_error {
            json!({"jsonrpc":"2.0", "id":old_id, "error":{"code":-32603,"message":"old failure"}})
        } else {
            json!({"jsonrpc":"2.0", "id":old_id, "result":{"roots":[{"uri":root}]}})
        };
        client.write_line(&response.to_string());
        let granted_after_completion = new_grant && !older_error;
        client.wait_for_input_state(observed_input, granted_after_completion);
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            client.send_request(
                880,
                "tools/call",
                json!({"name":"inspect", "arguments":{"input":observed_input}}),
            );
            let response = client.read_response(880);
            assert_eq!(
                response["result"]["isError"], !granted_after_completion,
                "roots completion violated the authorization policy: {response}"
            );
        }
        client.close_and_assert_purity();
    }

    #[test]
    fn late_initial_roots_success_cannot_restore_a_revoked_grant() {
        stale_completion(true, false, false, false);
    }
    #[test]
    fn older_roots_success_cannot_restore_after_a_newer_error() {
        stale_completion(false, false, false, false);
    }
    #[test]
    fn older_roots_success_cannot_restore_after_a_newer_timeout() {
        stale_completion(false, true, false, false);
    }
    #[test]
    fn older_roots_success_cannot_override_newer_empty_roots() {
        stale_completion(false, false, true, false);
    }
    #[test]
    fn older_roots_error_revokes_even_after_a_newer_success() {
        stale_completion(false, false, true, true);
    }
}

#[cfg(feature = "test-probe")]
mod staging_responsiveness {
    use super::*;
    use std::time::{Duration, Instant};

    fn slow_staging(name: &str, unanswered: bool) {
        let root = TempDir::new().unwrap();
        let input = root.path().join("slow.md");
        let marker = root.path().join("started");
        let file = fs::File::create(&input).unwrap();
        file.set_len(16 * 1024 * 1024).unwrap();
        let mut client = McpTestClient::spawn_with_env(
            &[],
            vec![path_to_file_uri(root.path())],
            &[
                ("ASHIFT_TEST_STAGE_DELAY_MS", std::ffi::OsStr::new("100")),
                ("ASHIFT_TEST_STAGE_STARTED", marker.as_os_str()),
            ],
        );
        client.initialize();
        let probe = root.path().join("ready.md");
        fs::write(&probe, "# Ready").unwrap();
        client.wait_for_input_state(&probe, true);
        fs::remove_file(&marker).unwrap();
        client.send_request(
            810,
            "tools/call",
            json!({"name":name,"arguments":{"input":input,"to":"html"}}),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() {
            assert!(Instant::now() < deadline, "staging never began");
            std::thread::sleep(Duration::from_millis(10));
        }
        client.read_timeout = Duration::from_secs(2);
        client.roots_failure = Some(if unanswered {
            RootsFailure::Timeout
        } else {
            RootsFailure::Error
        });
        let refresh_start = Instant::now();
        client.send_notification("notifications/roots/list_changed", json!({}));
        client.wait_for_roots_refresh();
        assert!(
            client.roots_queries_count >= 2,
            "refresh was not processed during staging"
        );
        client.wait_for_input_state(&probe, false);
        assert!(
            refresh_start.elapsed() < Duration::from_secs(12),
            "roots deadline was delayed by staging"
        );
        let start = Instant::now();
        client.close_and_assert_purity();
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "shutdown did not cancel staging"
        );
    }
    #[test]
    fn slow_inspect_staging_allows_roots_refresh_and_shutdown() {
        slow_staging("inspect", false);
    }
    #[test]
    fn slow_plan_staging_allows_roots_refresh_and_shutdown() {
        slow_staging("plan", false);
    }
    #[test]
    fn slow_convert_staging_allows_roots_refresh_and_shutdown() {
        slow_staging("convert", false);
    }
    #[test]
    fn roots_timeout_keeps_its_deadline_during_slow_staging() {
        slow_staging("inspect", true);
    }
}

#[test]
fn an_older_roots_timeout_revokes_even_after_a_newer_success() {
    let root = TempDir::new().unwrap();
    let input = root.path().join("note.md");
    fs::write(&input, "# Root").unwrap();
    let mut client = McpTestClient::spawn(&[], vec![path_to_file_uri(root.path())]);
    client.initialize();
    client.wait_for_input_state(&input, true);
    client.roots_failure = Some(RootsFailure::Timeout);
    client.send_notification("notifications/roots/list_changed", json!({}));
    client.wait_for_roots_refresh();
    let newest = TempDir::new().unwrap();
    let newest_input = newest.path().join("newest.md");
    fs::write(&newest_input, "# Newest root").unwrap();
    client.update_roots(vec![path_to_file_uri(newest.path())]);
    client.roots_failure = None;
    client.send_notification("notifications/roots/list_changed", json!({}));
    client.wait_for_roots_refresh();
    client.wait_for_input_state(&newest_input, true);
    client.wait_for_input_state(&input, false);
    client.wait_for_input_state(&newest_input, false);
    client.send_notification("notifications/roots/list_changed", json!({}));
    client.wait_for_roots_refresh();
    client.wait_for_input_state(&newest_input, true);
    client.close_and_assert_purity();
}

fn overlapping_refreshes_revoke_at_the_first_deadline(answer_older: bool) {
    use std::time::{Duration, Instant};
    let root = TempDir::new().unwrap();
    let explicit = TempDir::new().unwrap();
    let input = root.path().join("note.md");
    let output = root.path().join("note.html");
    let other = explicit.path().join("explicit.md");
    fs::write(&input, "# Client root").unwrap();
    fs::write(&other, "# Explicit root").unwrap();
    let root_uri = path_to_file_uri(root.path());
    let mut client = McpTestClient::spawn(
        &["--allow-dir", explicit.path().to_str().unwrap()],
        vec![root_uri.clone()],
    );
    client.initialize();
    client.send_request(
        920,
        "tools/call",
        json!({
            "name":"convert", "arguments":{"input":input,"output":output,"to":"html"}
        }),
    );
    assert_eq!(client.read_response(920)["result"]["isError"], false);
    let uri = path_to_file_uri(&output);
    client.send_request(921, "resources/read", json!({"uri":uri}));
    assert!(client.read_response(921).get("result").is_some());

    client.roots_failure = Some(RootsFailure::Timeout);
    let first = Instant::now();
    client.send_notification("notifications/roots/list_changed", json!({}));
    client.wait_for_roots_refresh();
    let older = client.pending_roots.pop().unwrap();
    std::thread::sleep((first + Duration::from_secs(9)).saturating_duration_since(Instant::now()));
    client.send_notification("notifications/roots/list_changed", json!({}));
    client.wait_for_roots_refresh();
    let newest = client.pending_roots.pop().unwrap();
    assert!(
        first.elapsed() < Duration::from_secs(10),
        "refreshes did not overlap"
    );
    if answer_older {
        client.write_line(
            &json!({
                "jsonrpc":"2.0", "id":older, "result":{"roots":[{"uri":root_uri}]}
            })
            .to_string(),
        );
    }
    let deadline = first + Duration::from_secs(12);
    loop {
        client.send_request(
            922,
            "tools/call",
            json!({"name":"inspect","arguments":{"input":input}}),
        );
        let response = client.read_response_until(922, deadline);
        if response["result"]["isError"] == true {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "first unanswered roots deadline was postponed: {response}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    client.send_request(923, "resources/read", json!({"uri":uri}));
    assert!(client.read_response(923).get("error").is_some());
    client.send_request(
        924,
        "tools/call",
        json!({"name":"inspect","arguments":{"input":other}}),
    );
    assert_eq!(client.read_response(924)["result"]["isError"], false);

    // The newest request remains answerable after the shared revocation deadline.
    client.write_line(
        &json!({
            "jsonrpc":"2.0", "id":newest, "result":{"roots":[{"uri":root_uri}]}
        })
        .to_string(),
    );
    client.wait_for_input_state(&input, true);
    client.send_request(925, "resources/read", json!({"uri":uri}));
    assert!(client.read_response(925).get("result").is_some());
    client.close_and_assert_purity();
}

#[test]
fn overlapping_unanswered_refreshes_keep_the_first_revocation_deadline() {
    overlapping_refreshes_revoke_at_the_first_deadline(false);
}

#[test]
fn stale_success_does_not_restart_the_pending_revocation_deadline() {
    overlapping_refreshes_revoke_at_the_first_deadline(true);
}

mod staging_cleanup {
    use super::*;
    use std::{
        path::Path,
        time::{Duration, SystemTime},
    };

    const PREFIX: &str = "ashift-mcp-stage-";

    fn process_directory(base: &Path) -> PathBuf {
        let entries: Vec<_> = fs::read_dir(base)
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(PREFIX))
            .map(|entry| entry.path())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "missing dedicated process staging directory: {entries:?}"
        );
        entries[0].clone()
    }

    fn set_old(path: &Path) {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.access_mode(0x0100); // FILE_WRITE_ATTRIBUTES permits setting timestamps.
            options.custom_flags(0x0200_0000); // FILE_FLAG_BACKUP_SEMANTICS
        }
        options
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(48 * 3600))
            .unwrap();
    }

    #[test]
    fn process_staging_is_removed_after_normal_shutdown() {
        let temp = TempDir::new().unwrap();
        let input = temp.path().join("note.md");
        let base = temp.path().join("staging");
        fs::create_dir(&base).unwrap();
        fs::write(&input, "# Normal shutdown").unwrap();
        let mut client = McpTestClient::spawn_with_env(
            &["--allow-dir", temp.path().to_str().unwrap()],
            vec![],
            &[
                ("TMPDIR", base.as_os_str()),
                ("TMP", base.as_os_str()),
                ("TEMP", base.as_os_str()),
            ],
        );
        client.initialize();
        let directory = process_directory(&base);
        assert!(
            directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("{PREFIX}{}-", client.child.as_ref().unwrap().id()))
        );
        client.wait_for_input_state(&input, true);
        assert!(
            fs::read_dir(&directory).unwrap().next().is_none(),
            "completed staging copy remains"
        );
        client.close_and_assert_purity();
        assert!(
            !directory.exists(),
            "process staging directory remains after shutdown"
        );
    }

    #[test]
    fn startup_sweeps_only_old_staging_of_dead_processes() {
        let temp = TempDir::new().unwrap();
        let mut exited = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .arg("--version")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let dead_pid = exited.id();
        assert!(exited.wait().unwrap().success());
        let stale = temp.path().join(format!("{PREFIX}{dead_pid}-stale"));
        let recent = temp.path().join(format!("{PREFIX}{dead_pid}-recent"));
        let live = temp
            .path()
            .join(format!("{PREFIX}{}-live", std::process::id()));
        let unrelated = temp.path().join("other-application");
        let malformed = temp.path().join(format!("{PREFIX}invalid-owner"));
        for path in [&stale, &recent, &live, &unrelated, &malformed] {
            fs::create_dir(path).unwrap();
            fs::write(path.join("keep.md"), "private staging bytes").unwrap();
            if path != &recent {
                set_old(path);
            }
        }
        #[cfg(unix)]
        let link = {
            let link = temp.path().join(format!("{PREFIX}{dead_pid}-link"));
            std::os::unix::fs::symlink(&unrelated, &link).unwrap();
            link
        };
        let mut client = McpTestClient::spawn_with_env(
            &[],
            vec![],
            &[
                ("TMPDIR", temp.path().as_os_str()),
                ("TMP", temp.path().as_os_str()),
                ("TEMP", temp.path().as_os_str()),
            ],
        );
        client.initialize();
        assert!(
            !stale.exists(),
            "startup did not sweep stale process staging"
        );
        for path in [&recent, &live, &unrelated, &malformed] {
            assert!(
                path.join("keep.md").exists(),
                "startup touched protected directory {path:?}"
            );
        }
        #[cfg(unix)]
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        client.close_and_assert_purity();
    }

    #[cfg(feature = "test-probe")]
    #[test]
    fn cancelled_staging_removes_partial_copies_before_shutdown() {
        use std::time::Instant;
        let temp = TempDir::new().unwrap();
        let input = temp.path().join("slow.md");
        fs::File::create(&input)
            .unwrap()
            .set_len(16 * 1024 * 1024)
            .unwrap();
        let base = temp.path().join("staging");
        let marker = temp.path().join("started");
        fs::create_dir(&base).unwrap();
        let mut client = McpTestClient::spawn_with_env(
            &["--allow-dir", temp.path().to_str().unwrap()],
            vec![],
            &[
                ("TMPDIR", base.as_os_str()),
                ("TMP", base.as_os_str()),
                ("TEMP", base.as_os_str()),
                ("ASHIFT_TEST_STAGE_STARTED", marker.as_os_str()),
                ("ASHIFT_TEST_STAGE_DELAY_MS", std::ffi::OsStr::new("100")),
            ],
        );
        client.initialize();
        let directory = process_directory(&base);
        client.send_request(
            930,
            "tools/call",
            json!({"name":"inspect","arguments":{"input":input}}),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() {
            assert!(Instant::now() < deadline, "staging did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            fs::read_dir(&directory).unwrap().next().is_some(),
            "no staged copy to cancel"
        );
        client.send_notification("notifications/cancelled", json!({"requestId":930}));
        let deadline = Instant::now() + Duration::from_secs(3);
        while fs::read_dir(&directory).unwrap().next().is_some() {
            assert!(Instant::now() < deadline, "cancelled staging copy remains");
            std::thread::sleep(Duration::from_millis(10));
        }
        client.send_request(931, "tools/list", json!({}));
        assert!(client.read_response(931).get("result").is_some());
        client.close_and_assert_purity();
        assert!(
            !directory.exists(),
            "process staging directory remains after shutdown"
        );
    }
}
