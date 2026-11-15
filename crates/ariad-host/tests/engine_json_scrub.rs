//! Integration tests for the secret scrub applied to JSON artifacts staged by out-of-process
//! engines (`docs/tools.md` §11): escaped secret spellings, the scrub size bound and fail-closed
//! handling of malformed staged JSON.
#![cfg(unix)]

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use ariad_core::limits::Limits;
use ariad_host::{
    runner::EngineCommand,
    sandbox::SandboxMode,
    tools::{InputRef, OutputTarget, SecretBag, ToolError, ToolOutcome, ToolRequest, run_tool},
};
use serde_json::Value;
use tempfile::TempDir;

const PUBLISHED_NAME: &str = "published.json";

fn sample_pdf() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf")
}

/// Runs `pdf.text_layout` through a fake `/bin/sh` engine that stages `staged_json` verbatim as
/// its only JSON artifact, with `secret` registered in the request's [`SecretBag`].
fn run_fake_json_engine(
    tmp: &TempDir,
    staged_json: &str,
    secret: &str,
) -> Result<ToolOutcome, ToolError> {
    let input_pdf = tmp.path().join("report.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy sample pdf");
    let staged_src = tmp.path().join("staged-src.json");
    fs::write(&staged_src, staged_json).expect("write staged JSON source");

    let mut spec = *ariad_core::tools::find("pdf.text_layout").expect("registered pdf.text_layout");
    spec.engine = "qpdf";
    let script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
cat "$1" > "$ws_out/output.json"
printf '{"type":"artifact","path":"%s/output.json","format":"json"}\n' "$ws_out"
printf '%s\n' '{"type":"result","ok":true}'
"#;
    let cmd = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![
            OsString::from("-c"),
            OsString::from(script),
            OsString::from("fake-engine"),
            staged_src.into_os_string(),
        ],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };
    let secrets = SecretBag::new().with("api_key", secret).unwrap();
    let req = ToolRequest::new(
        "pdf.text_layout",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(tmp.path().join(PUBLISHED_NAME)),
    )
    .with_spec(spec)
    .with_limits(Limits::local())
    .with_secrets(secrets)
    .with_engine_command(cmd);
    run_tool(&req)
}

/// Encodes every character of `s` as a JSON `\uXXXX` escape.
fn unicode_escape_all(s: &str) -> String {
    s.encode_utf16().map(|u| format!("\\u{u:04x}")).collect()
}

fn collect_strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(s) => out.push(s),
        Value::Array(items) => items.iter().for_each(|v| collect_strings(v, out)),
        Value::Object(map) => {
            for (k, v) in map {
                out.push(k);
                collect_strings(v, out);
            }
        }
        _ => {}
    }
}

/// Asserts that `value` carries no decoded string or key containing `secret`.
fn assert_value_free_of(value: &Value, secret: &str, what: &str) {
    let mut strings = Vec::new();
    collect_strings(value, &mut strings);
    for s in strings {
        assert!(!s.contains(secret), "{what} leaked secret in {s:?}");
    }
}

/// Asserts that the published file and `inline_json` contain neither the raw secret, the
/// escaped spellings the engine used, nor any decoded form of it.
fn assert_scrubbed(
    tmp: &TempDir,
    outcome: &ToolOutcome,
    secret: &str,
    spellings: &[&str],
) -> Value {
    let published = tmp.path().join(PUBLISHED_NAME);
    let text = fs::read_to_string(&published).expect("published JSON is UTF-8");
    assert!(!text.contains(secret), "raw secret in published bytes");
    for spelling in spellings {
        assert!(
            !text.contains(spelling),
            "escaped secret spelling {spelling:?} in published bytes"
        );
    }
    let parsed: Value = serde_json::from_str(&text).expect("published JSON stays valid");
    assert_value_free_of(&parsed, secret, "published JSON");
    let inline = outcome
        .inline_json
        .as_ref()
        .expect("small single JSON output is inlined");
    assert_value_free_of(inline, secret, "inline_json");
    let inline_text = serde_json::to_string(inline).unwrap();
    assert!(!inline_text.contains(secret));
    parsed
}

#[test]
fn engine_json_unicode_escaped_secret_is_scrubbed() {
    let tmp = TempDir::new().unwrap();
    let secret = "plain-ascii-secret-token-77";
    let escaped = unicode_escape_all(secret);
    // Fully escaped value, an escaped key, and a partially escaped occurrence inside a longer
    // string (only the first character escaped).
    let partial = format!("\\u0070{}", &secret[1..]);
    let staged =
        format!(r#"{{"value":"{escaped}","{escaped}":1,"nested":["prefix {partial} suffix"]}}"#);
    let outcome = run_fake_json_engine(&tmp, &staged, secret)
        .expect("escaped secret JSON must publish scrubbed");
    let parsed = assert_scrubbed(&tmp, &outcome, secret, &[&escaped, &partial]);
    assert_eq!(parsed["value"], "[REDACTED]");
    assert_eq!(parsed["[REDACTED]"], 1);
    assert_eq!(parsed["nested"][0], "prefix [REDACTED] suffix");
}

#[test]
fn engine_json_solidus_escaped_secret_is_scrubbed() {
    let tmp = TempDir::new().unwrap();
    let secret = "tenant/secret/token-88";
    let escaped = secret.replace('/', "\\/");
    let staged = format!(r#"{{"path":"{escaped}","keep":"a\/b"}}"#);
    let outcome = run_fake_json_engine(&tmp, &staged, secret)
        .expect("solidus-escaped secret JSON must publish scrubbed");
    let parsed = assert_scrubbed(&tmp, &outcome, secret, &[&escaped]);
    assert_eq!(parsed["path"], "[REDACTED]");
    assert_eq!(parsed["keep"], "a/b");
}

#[test]
fn engine_json_without_secret_occurrence_is_published_unchanged() {
    let tmp = TempDir::new().unwrap();
    let staged = "{\n  \"a\" : [1, 2.5e3, true, null],\n  \"b\": \"x\\/y\\u00e9\"\n}\n";
    let outcome = run_fake_json_engine(&tmp, staged, "unrelated-secret-token-55")
        .expect("JSON without secrets must publish");
    let published = fs::read_to_string(tmp.path().join(PUBLISHED_NAME)).unwrap();
    assert_eq!(
        published, staged,
        "untouched JSON must keep its exact bytes"
    );
    let inline = outcome.inline_json.expect("inline JSON");
    assert_eq!(inline["b"], "x/y\u{e9}");
}

#[test]
fn engine_json_over_scrub_cap_is_limit_exceeded_when_secrets_registered() {
    let tmp = TempDir::new().unwrap();
    let staged = format!(r#"{{"pad":"{}"}}"#, "a".repeat(4096));
    let err = ariad_host::tools::run::with_scrub_json_cap_for_test(1024, || {
        run_fake_json_engine(&tmp, &staged, "cap-secret-token-66")
    })
    .expect_err("staged JSON above the scrub cap must fail");
    assert!(
        matches!(err, ToolError::LimitExceeded),
        "expected LimitExceeded, got {err:?}"
    );
    assert!(!tmp.path().join(PUBLISHED_NAME).exists());
}

#[test]
fn engine_json_at_scrub_cap_is_scrubbed() {
    let tmp = TempDir::new().unwrap();
    let secret = "cap-secret-token-67";
    let staged = format!(r#"{{"s":"{secret}","pad":"{}"}}"#, "a".repeat(900));
    let cap = staged.len() as u64;
    let outcome = ariad_host::tools::run::with_scrub_json_cap_for_test(cap, || {
        run_fake_json_engine(&tmp, &staged, secret)
    })
    .expect("staged JSON at the scrub cap must publish");
    let parsed = assert_scrubbed(&tmp, &outcome, secret, &[]);
    assert_eq!(parsed["s"], "[REDACTED]");
}

#[test]
fn engine_json_malformed_fails_closed_when_secrets_registered() {
    for staged in [
        r#"{"s":"no-match-here","#,
        r#"{"s":"no-match-here"} trailing"#,
        r#"{"s":"\ud800"}"#,
        r#"{"s":"bad \q escape"}"#,
    ] {
        let tmp = TempDir::new().unwrap();
        let err = run_fake_json_engine(&tmp, staged, "malformed-secret-token-44")
            .expect_err("malformed staged JSON with active secrets must fail closed");
        assert!(
            matches!(err, ToolError::EngineFailure(_)),
            "expected EngineFailure for {staged:?}, got {err:?}"
        );
        assert!(!tmp.path().join(PUBLISHED_NAME).exists());
    }
}
