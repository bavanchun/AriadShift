use assert_cmd::Command;
use serde_json::Value;

const DETERMINISTIC_EPOCH: &str = "1700000000";

fn ashift_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ashift"));
    cmd.env("SOURCE_DATE_EPOCH", DETERMINISTIC_EPOCH);
    cmd
}

#[test]
fn web_to_pdf_cli_zero_input_invocation() {
    // 1. Missing `url` param exits 9 (invalid_params)
    let missing_url = ashift_cmd()
        .args(["tool", "web.to_pdf"])
        .output()
        .expect("run ashift tool web.to_pdf without params");
    assert_eq!(
        missing_url.status.code(),
        Some(9),
        "missing required url parameter must exit with code 9; stderr: {}",
        String::from_utf8_lossy(&missing_url.stderr)
    );

    // 2. Non-HTTPS url exits with error (code 9 invalid_params)
    let non_https = ashift_cmd()
        .args([
            "tool",
            "web.to_pdf",
            "--param",
            "url=http://example.com/test",
        ])
        .output()
        .expect("run ashift tool web.to_pdf with http URL");
    assert!(
        !non_https.status.success(),
        "non-https url must fail; stderr: {}",
        String::from_utf8_lossy(&non_https.stderr)
    );
    assert_eq!(
        non_https.status.code(),
        Some(9),
        "non-https url must exit with code 9"
    );

    // 3. ashift tools --json lists web.to_pdf
    let tools_list = ashift_cmd()
        .args(["tools", "--json"])
        .output()
        .expect("run ashift tools --json");
    assert!(tools_list.status.success());
    let tools_json: Value = serde_json::from_slice(&tools_list.stdout).unwrap();
    let web_tool = tools_json
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "web.to_pdf");
    assert!(web_tool.is_some(), "tools --json must include web.to_pdf");
    let desc = web_tool.unwrap();
    assert_eq!(desc["group"], "convert");
    assert_eq!(desc["output_format"], "pdf");
    assert_eq!(desc["inputs"]["arity"], "none");
}
