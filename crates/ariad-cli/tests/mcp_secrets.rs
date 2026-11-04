//! Local MCP secret handle checks over stdio.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

#[test]
fn secret_put_enforces_capacity_and_never_echoes_values() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .arg("mcp")
        .env("RUST_LOG", "trace")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    fn send(stdin: &mut impl Write, value: Value) {
        writeln!(stdin, "{value}").unwrap();
        stdin.flush().unwrap();
    }
    fn response(stdout: &mut impl BufRead, stdin: &mut impl Write, id: u64) -> Value {
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0);
            let value: Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id && value.get("result").is_some() {
                return value;
            }
            if value["method"] == "roots/list" {
                send(
                    stdin,
                    json!({"jsonrpc":"2.0", "id":value["id"], "result":{"roots":[]}}),
                );
            }
        }
    }
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{"roots":{"listChanged":true}},"clientInfo":{"name":"test","version":"1"}}}),
    );
    response(&mut stdout, &mut stdin, 1);
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let secret = "quote \" slash \\ newline tiếng Việt";
    for (id, value) in [(40, String::new()), (41, "x".repeat(4097))] {
        send(
            &mut stdin,
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":value}}}),
        );
        let rejected = response(&mut stdout, &mut stdin, id);
        assert_eq!(rejected["result"]["structuredContent"]["exit_code"], 9);
    }
    let escaped = serde_json::to_string(secret).unwrap();
    let escaped = &escaped[1..escaped.len() - 1];
    let encoded: String = secret
        .as_bytes()
        .iter()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'_' | b'.' | b'~') {
                char::from(*b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    for id in 2..34 {
        send(
            &mut stdin,
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":secret}}}),
        );
        let result = response(&mut stdout, &mut stdin, id);
        let text = result.to_string();
        assert!(!text.contains(secret) && !text.contains(escaped) && !text.contains(&encoded));
        assert_eq!(result["result"]["structuredContent"]["expires_in_s"], 300);
        assert!(
            result["result"]["structuredContent"]["handle"]
                .as_str()
                .unwrap()
                .starts_with("sec_")
        );
    }
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":34,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":secret}}}),
    );
    let result = response(&mut stdout, &mut stdin, 34);
    assert_eq!(result["result"]["structuredContent"]["exit_code"], 9);
    assert!(!result.to_string().contains(secret));
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
}

#[cfg(all(feature = "test-probe", unix))]
#[test]
fn handles_reach_probe_once_and_reuse_is_rejected() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("input.pdf");
    std::fs::write(&input, b"%PDF-1.7\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let probe = dir.path().join("secret-probe.py");
    std::fs::write(&probe, r####"#!/usr/bin/python3
import json, os, sys, urllib.parse
request = json.loads(sys.stdin.readline())
path = request["params_file"]
with open(path, encoding="utf-8") as stream:
    params = json.load(stream)
value = next(iter(params.values()))
assert all(item == value for item in params.values())
print(json.dumps({"type":"warning", "code":"probe", "message":value + " " + urllib.parse.quote(value, safe="")}))
output = os.path.join(request["output"]["dir"], "output.pdf")
with open(output, "wb") as stream:
    stream.write(b"%PDF-1.7\n")
print(json.dumps({"type":"artifact", "path":output, "format":"pdf"}))
print(json.dumps({"type":"result", "ok":True, "metrics":{"params_keys":len(params), "params_file_mode":os.stat(path).st_mode & 0o777}}))
"####).unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .arg("mcp")
        .arg("--allow-dir")
        .arg(dir.path())
        .env("RUST_LOG", "trace")
        .env("ARIAD_MCP_TEST_PROBE", "1")
        .env("ARIAD_MCP_TEST_PROBE_PROGRAM", &probe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    fn send(stdin: &mut impl Write, value: Value) {
        writeln!(stdin, "{value}").unwrap();
        stdin.flush().unwrap();
    }
    fn read(stdout: &mut impl BufRead, stdin: &mut impl Write, id: u64) -> Value {
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0);
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == id && message.get("result").is_some() {
                return message;
            }
            if message["method"] == "roots/list" {
                send(
                    stdin,
                    json!({"jsonrpc":"2.0","id":message["id"],"result":{"roots":[]}}),
                );
            }
        }
    }
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{"roots":{"listChanged":true}},"clientInfo":{"name":"test","version":"1"}}}),
    );
    read(&mut stdout, &mut stdin, 1);
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let value = "sensitive-\"\\-tiếng Việt";
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":value}}}),
    );
    let response = read(&mut stdout, &mut stdin, 2);
    let handle = response["result"]["structuredContent"]["handle"]
        .as_str()
        .unwrap();
    assert!(!response.to_string().contains(
        &serde_json::to_string(value).unwrap()[1..serde_json::to_string(value).unwrap().len() - 1]
    ));
    for (id, params) in [
        (5, json!({"owner_password": value})),
        (6, json!({"owner_password": {"$secret": "prompt"}})),
        (7, json!({"owner_password": {"$secret": "sec_invalid!"}})),
        (8, json!({"algorithm": {"$secret": handle}})),
        (
            12,
            json!({"algorithm": {"nested": {"password": {"$secret": handle}}}}),
        ),
    ] {
        send(
            &mut stdin,
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"run_doc_tool","arguments":{"tool":"pdf.encrypt","inputs":[input],"params":params}}}),
        );
        let rejected = read(&mut stdout, &mut stdin, id);
        assert_eq!(rejected["result"]["structuredContent"]["exit_code"], 9);
        let serialized = rejected.to_string();
        let escaped = serde_json::to_string(value).unwrap();
        assert!(!serialized.contains(&escaped[1..escaped.len() - 1]));
    }
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":"one-use"}}}),
    );
    let extra = read(&mut stdout, &mut stdin, 30);
    let one_use = extra["result"]["structuredContent"]["handle"]
        .as_str()
        .unwrap();
    let failed = json!({"tool":"pdf.encrypt","inputs":[dir.path().join("missing.pdf")],"params":{"owner_password":{"$secret":one_use}}});
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"run_doc_tool","arguments":failed}}),
    );
    assert_eq!(read(&mut stdout, &mut stdin, 31)["result"]["isError"], true);
    let reuse = json!({"tool":"pdf.encrypt","inputs":[input],"params":{"owner_password":{"$secret":one_use}}});
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":32,"method":"tools/call","params":{"name":"run_doc_tool","arguments":reuse}}),
    );
    let result = read(&mut stdout, &mut stdin, 32);
    assert_eq!(result["result"]["structuredContent"]["exit_code"], 9);
    let args = json!({"tool":"pdf.encrypt","inputs":[input],"output_dir":dir.path(),"params":{"user_password":{"$secret":handle},"owner_password":{"$secret":handle}}});
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"run_doc_tool","arguments":args}}),
    );
    let response = read(&mut stdout, &mut stdin, 3);
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["structuredContent"]["metrics"]["params_keys"],
        2
    );
    #[cfg(unix)]
    assert_eq!(
        response["result"]["structuredContent"]["metrics"]["params_file_mode"],
        0o600
    );
    let encoded: String = value
        .as_bytes()
        .iter()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'_' | b'.' | b'~') {
                char::from(*b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let escaped = serde_json::to_string(value).unwrap();
    let serialized = response.to_string();
    assert!(!serialized.contains(value));
    assert!(!serialized.contains(&escaped[1..escaped.len() - 1]));
    assert!(!serialized.contains(&encoded));
    assert!(serialized.contains("[REDACTED]"));
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"run_doc_tool","arguments":args}}),
    );
    let response = read(&mut stdout, &mut stdin, 4);
    assert_eq!(response["result"]["structuredContent"]["exit_code"], 9);
    assert!(!response.to_string().contains(value));

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":value}}}),
    );
    let fresh = read(&mut stdout, &mut stdin, 9);
    let fresh_handle = fresh["result"]["structuredContent"]["handle"]
        .as_str()
        .unwrap();
    for id in [10, 11] {
        let output = dir.path().join(format!("race-{id}.pdf"));
        send(
            &mut stdin,
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"run_doc_tool","arguments":{"tool":"pdf.decrypt","inputs":[input],"output":output,"params":{"password":{"$secret":fresh_handle}}}}}),
        );
    }
    let mut responses = Vec::new();
    while responses.len() < 2 {
        let mut line = String::new();
        assert_ne!(stdout.read_line(&mut line).unwrap(), 0);
        let message: Value = serde_json::from_str(&line).unwrap();
        if message["id"] == 10 || message["id"] == 11 {
            responses.push(message);
        }
    }
    assert_eq!(
        responses
            .iter()
            .filter(|r| r["result"]["isError"] == false)
            .count(),
        1
    );
    assert_eq!(
        responses
            .iter()
            .filter(|r| r["result"]["structuredContent"]["exit_code"] == 9)
            .count(),
        1
    );
    let escaped = serde_json::to_string(value).unwrap();
    assert!(
        responses
            .iter()
            .all(|r| !r.to_string().contains(&escaped[1..escaped.len() - 1]))
    );
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains(value));
    assert!(!stderr.contains(&escaped[1..escaped.len() - 1]));
    assert!(!stderr.contains(&encoded));
}

#[test]
fn secret_handle_runs_registered_qpdf_tool() {
    if std::env::var_os("ASHIFT_PACK_QPDF").is_none() {
        eprintln!("skipping real qpdf test: ASHIFT_PACK_QPDF is unset");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools/encrypted-aes256.plaintext.pdf"),
        &input,
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .arg("mcp")
        .arg("--allow-dir")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    fn call(stdin: &mut impl Write, stdout: &mut impl BufRead, request: Value, id: u64) -> Value {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0);
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == id && message.get("result").is_some() {
                return message;
            }
            if message["method"] == "roots/list" {
                writeln!(
                    stdin,
                    "{}",
                    json!({"jsonrpc":"2.0","id":message["id"],"result":{"roots":[]}})
                )
                .unwrap();
                stdin.flush().unwrap();
            }
        }
    }
    call(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{"roots":{"listChanged":true}},"clientInfo":{"name":"test","version":"1"}}}),
        1,
    );
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    stdin.flush().unwrap();
    let secret = "mcp-real-tool-secret";
    let result = call(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":secret}}}),
        2,
    );
    let handle = result["result"]["structuredContent"]["handle"]
        .as_str()
        .unwrap();
    let arguments = json!({"tool":"pdf.encrypt","inputs":[input],"output":dir.path().join("encrypted.pdf"),"params":{"owner_password":{"$secret":handle}}});
    let result = call(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"run_doc_tool","arguments":arguments}}),
        3,
    );
    assert_eq!(
        result["result"]["isError"],
        false,
        "{}",
        result.to_string().replace(secret, "[redacted]")
    );
    assert!(!result.to_string().contains(secret));
    let result = call(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"run_doc_tool","arguments":arguments}}),
        4,
    );
    assert_eq!(result["result"]["structuredContent"]["exit_code"], 9);
    assert!(!result.to_string().contains(secret));
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
}

#[test]
fn secret_put_certificate_allows_large_payload() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    fn send(stdin: &mut impl Write, value: Value) {
        writeln!(stdin, "{value}").unwrap();
        stdin.flush().unwrap();
    }
    fn response(stdout: &mut impl BufRead, id: u64) -> Value {
        loop {
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return value;
            }
        }
    }
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{"roots":{"listChanged":true}},"clientInfo":{"name":"test","version":"1"}}}),
    );
    response(&mut stdout, 1);
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );

    // Standard string over 4096 bytes is rejected
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":"A".repeat(4097)}}}),
    );
    let rejected = response(&mut stdout, 2);
    assert_eq!(rejected["result"]["structuredContent"]["exit_code"], 9);

    // Certificate flag allows payloads up to 14 MiB
    let cert_payload = "A".repeat(8192);
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"secret_put","arguments":{"value":cert_payload,"certificate":true}}}),
    );
    let accepted = response(&mut stdout, 3);
    assert_eq!(accepted["result"]["isError"], false);
    assert!(
        accepted["result"]["structuredContent"]["handle"]
            .as_str()
            .unwrap()
            .starts_with("sec_")
    );
    drop(stdin);
    let _ = child.wait().unwrap();
}
