use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

fn ashift_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ashift"));
    cmd.env("ASHIFT_PANDOC", pandoc_path());
    cmd
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

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

fn pdf_fixture(name: &str) -> PathBuf {
    repository_root().join("fixtures/pdf-tools").join(name)
}

fn read_truth_pdf_info(stem: &str) -> Value {
    let truth_path = pdf_fixture(&format!("{stem}.truth.json"));
    let raw = fs::read_to_string(&truth_path).expect("read truth file");
    let truth: Value = serde_json::from_str(&raw).expect("parse truth JSON");
    let mut expected = truth
        .get("pdf_info")
        .cloned()
        .expect("truth file must contain pdf_info");
    if let Value::Object(ref mut map) = expected {
        map.entry("repaired_xref").or_insert(Value::Bool(false));
    }
    expected
}

fn assert_exit_and_stderr(cmd: &mut Command, expected_code: i32, expected_substrings: &[&str]) {
    let out = cmd.output().expect("execute ashift");
    let actual_code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        actual_code, expected_code,
        "expected exit code {expected_code}, got {actual_code}\nstdout: {stdout}\nstderr: {stderr}"
    );
    for needle in expected_substrings {
        assert!(
            stderr.contains(needle),
            "expected stderr to contain {needle:?}, got:\n{stderr}"
        );
    }
}

/// Builds a minimal syntactically valid PDF whose trailer declares an `/Encrypt` dictionary
/// that cannot be unlocked with the empty password, triggering `ReadError::Encrypted` -> exit 12.
fn write_encrypted_stub_pdf(path: &Path) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.4\n");
    let obj1_offset = bytes.len();
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let obj2_offset = bytes.len();
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    let obj3_offset = bytes.len();
    bytes.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
    );
    let obj4_offset = bytes.len();
    bytes.extend_from_slice(
        b"4 0 obj\n<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O <000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f> /U <1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100> >>\nendobj\n",
    );
    let xref_offset = bytes.len();
    let xref = format!(
        "xref\n0 5\n0000000000 65535 f \n{obj1_offset:010} 00000 n \n{obj2_offset:010} 00000 n \n{obj3_offset:010} 00000 n \n{obj4_offset:010} 00000 n \ntrailer\n<< /Size 5 /Root 1 0 R /Encrypt 4 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
    );
    bytes.extend_from_slice(xref.as_bytes());
    fs::write(path, bytes).expect("write encrypted stub PDF");
}

/// Materializes an encrypted PDF from one of the 7-object `fixtures/pdf-tools/*.plaintext.pdf`
/// fixtures by appending an `8 0 obj` `/Encrypt` dictionary right before `xref\n0 8\n` and
/// updating the cross-reference table and trailer in place so object 1..7 offsets stay intact.
fn materialize_encrypted_from_plaintext(
    plaintext_path: &Path,
    out_path: &Path,
    encrypt_dict: &[u8],
) {
    let raw = fs::read(plaintext_path).expect("read plaintext fixture");
    let needle = b"\nxref\n0 8\n";
    let pos = raw
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("find xref in plaintext fixture");
    let body_end = pos + 1; // keep trailing '\n' after `7 0 obj ... endobj\n`
    let after_header = pos + needle.len();

    let rest = std::str::from_utf8(&raw[after_header..]).expect("utf8 xref and trailer");
    let (xref_entries, trailer_and_eof) = rest
        .split_once("trailer <<")
        .expect("split xref entries and trailer");
    let (trailer_body, _) = trailer_and_eof
        .split_once(">>\nstartxref\n")
        .expect("split trailer body and startxref");
    let updated_trailer = trailer_body.replace("/Size 8", "/Size 9 /Encrypt 8 0 R");

    let mut out = Vec::new();
    out.extend_from_slice(&raw[..body_end]);
    let obj8_offset = out.len();
    out.extend_from_slice(b"8 0 obj\n");
    out.extend_from_slice(encrypt_dict);
    out.extend_from_slice(b"\nendobj\n");
    let new_xref_offset = out.len();
    let tail = format!(
        "xref\n0 9\n{xref_entries}{obj8_offset:010} 00000 n \ntrailer <<{updated_trailer}>>\nstartxref\n{new_xref_offset}\n%%EOF\n"
    );
    out.extend_from_slice(tail.as_bytes());
    fs::write(out_path, out).expect("write materialized encrypted fixture");
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, out);
            } else {
                out.push(path);
            }
        }
    }
}

#[test]
fn tool_pdf_info_matches_all_corpus_truth_files_and_never_writes_decrypted_copy() {
    let dir = repository_root().join("fixtures/pdf-tools");
    let mut truth_paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read fixtures/pdf-tools")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.file_name()?
                .to_str()?
                .ends_with(".truth.json")
                .then_some(path)
        })
        .collect();
    truth_paths.sort();
    assert_eq!(
        truth_paths.len(),
        22,
        "expected 22 truth files in fixtures/pdf-tools"
    );

    let tmp = TempDir::new().expect("tempdir");
    let materialized_dir = tmp.path().join("materialized");
    fs::create_dir_all(&materialized_dir).expect("create materialized dir");

    // V=5 R=6 empty-user-password `/Encrypt` dictionary with Identity crypt filter so `lopdf`
    // unlocks the owner-password-only document with `""` and preserves the plaintext object bytes.
    const OWNER_ONLY_ENCRYPT_DICT: &[u8] = b"<< /CF << /StdCF << /AuthEvent /DocOpen /CFM /Identity /Length 32 >> >> /Filter /Standard /Length 256 /O <1759241f12558fbcb7c006a96405baa704c76400fccd563698d5e4d003a23af01472fedd99192585f679d6e155cfd0d5> /OE <fc53d58a60852ff606cfd6fb0ff86b111eed25fbc6a6992969ba3776f699555a> /P -4 /Perms <f113a416d97ac067641036cd7727fcd4> /R 6 /StmF /StdCF /StrF /StdCF /U <6d20654d13677ac0315cf65c5d612b986e7f6e51727939fed3643034517f8d328bca71289a564169ce97d098f6293467> /\x55E <93a1e62b8928181966fdef4ddcedf45247e1bbba7a6ac22f7caeccd6b32ee3b1> /V 5 >>";
    const USER_LOCKED_AES256_ENCRYPT_DICT: &[u8] = b"<< /CF << /StdCF << /AuthEvent /DocOpen /CFM /AESV3 /Length 32 >> >> /Filter /Standard /Length 256 /O <000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f> /OE <000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f> /P -4 /Perms <000102030405060708090a0b0c0d0e0f> /R 6 /StmF /StdCF /StrF /StdCF /U <1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100202122232425262728292a2b2c2d2e2f> /\x55E <1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100> /V 5 >>";
    const USER_LOCKED_RC4_ENCRYPT_DICT: &[u8] = b"<< /Filter /Standard /V 1 /R 2 /Length 40 /P -4 /O <000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f> /U <1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100> >>";

    let mut committed_pdf_count = 0usize;
    let mut encrypted_stem_count = 0usize;

    for truth_path in truth_paths {
        let file_name = truth_path.file_name().unwrap().to_str().unwrap();
        let stem = file_name.strip_suffix(".truth.json").unwrap();
        let expected = read_truth_pdf_info(stem);

        match stem {
            "encrypted-owner-only" => {
                encrypted_stem_count += 1;
                let plaintext_input = pdf_fixture("encrypted-owner-only.plaintext.pdf");
                let encrypted_input = materialized_dir.join("encrypted-owner-only.pdf");
                materialize_encrypted_from_plaintext(
                    &plaintext_input,
                    &encrypted_input,
                    OWNER_ONLY_ENCRYPT_DICT,
                );

                let isolated_tmp = tmp.path().join("owner-only-tmp");
                let isolated_out = tmp.path().join("owner-only-out");
                fs::create_dir_all(&isolated_tmp).unwrap();
                fs::create_dir_all(&isolated_out).unwrap();

                // 1. Run `ashift tool pdf.info <file> --json` on the materialized owner-only PDF.
                let output = ashift_cmd()
                    .env("TMPDIR", &isolated_tmp)
                    .env("TMP", &isolated_tmp)
                    .env("TEMP", &isolated_tmp)
                    .args(["tool", "pdf.info"])
                    .arg(&encrypted_input)
                    .arg("--json")
                    .output()
                    .expect("run ashift tool pdf.info on encrypted-owner-only");
                assert!(
                    output.status.success(),
                    "pdf.info failed on encrypted-owner-only: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let actual: Value =
                    serde_json::from_slice(&output.stdout).expect("parse pdf.info JSON");
                assert_eq!(actual["encrypted"], Value::Bool(true));
                assert_eq!(actual, expected, "truth mismatch for {stem}");

                // Assert no files exist in output dir or temp dir after `--json` stdout run.
                let mut tmp_files = Vec::new();
                collect_files_recursive(&isolated_tmp, &mut tmp_files);
                assert!(
                    tmp_files.is_empty(),
                    "no temporary or decrypted files may remain in TMPDIR after pdf.info: {tmp_files:?}"
                );
                let mut out_files = Vec::new();
                collect_files_recursive(&isolated_out, &mut out_files);
                assert!(out_files.is_empty());

                // 2. Also run with `-o <out_dir>/encrypted-owner-only.info.json` and assert only JSON is written.
                let json_out_path = isolated_out.join("encrypted-owner-only.info.json");
                let file_run = ashift_cmd()
                    .env("TMPDIR", &isolated_tmp)
                    .env("TMP", &isolated_tmp)
                    .env("TEMP", &isolated_tmp)
                    .args(["tool", "pdf.info"])
                    .arg(&encrypted_input)
                    .arg("-o")
                    .arg(&json_out_path)
                    .output()
                    .expect("run ashift tool pdf.info -o on encrypted-owner-only");
                assert!(file_run.status.success());

                tmp_files.clear();
                collect_files_recursive(&isolated_tmp, &mut tmp_files);
                assert!(
                    tmp_files.is_empty(),
                    "no temporary or decrypted files may remain in TMPDIR after pdf.info -o: {tmp_files:?}"
                );
                out_files.clear();
                collect_files_recursive(&isolated_out, &mut out_files);
                assert_eq!(
                    out_files,
                    vec![json_out_path.clone()],
                    "pdf.info must write only the JSON report, never a decrypted PDF copy"
                );
                let written_bytes = fs::read(&json_out_path).unwrap();
                assert!(
                    !written_bytes.starts_with(b"%PDF-"),
                    "output must be JSON, not a PDF"
                );
                let written_json: Value = serde_json::from_slice(&written_bytes).unwrap();
                assert_eq!(written_json, expected);

                // 3. Also verify the committed `.plaintext.pdf` source matches every non-encryption field.
                let plain_out = ashift_cmd()
                    .args(["tool", "pdf.info"])
                    .arg(&plaintext_input)
                    .arg("--json")
                    .output()
                    .expect("run ashift tool pdf.info on plaintext source");
                assert!(plain_out.status.success());
                let mut plain_actual: Value = serde_json::from_slice(&plain_out.stdout).unwrap();
                assert_eq!(plain_actual["encrypted"], Value::Bool(false));
                plain_actual["encrypted"] = Value::Bool(true);
                assert_eq!(plain_actual, expected, "plaintext mismatch for {stem}");
            }
            "encrypted-aes256" | "encrypted-rc4-40" => {
                encrypted_stem_count += 1;
                let plaintext_input = pdf_fixture(&format!("{stem}.plaintext.pdf"));
                let encrypted_input = materialized_dir.join(format!("{stem}.pdf"));
                let dict = if stem == "encrypted-aes256" {
                    USER_LOCKED_AES256_ENCRYPT_DICT
                } else {
                    USER_LOCKED_RC4_ENCRYPT_DICT
                };
                materialize_encrypted_from_plaintext(&plaintext_input, &encrypted_input, dict);

                // Materialized user-password-encrypted PDF must fail with exit 12 (`encrypted_input`).
                assert_exit_and_stderr(
                    ashift_cmd()
                        .args(["tool", "pdf.info"])
                        .arg(&encrypted_input),
                    12,
                    &["error[encrypted_input]"],
                );

                // Committed `.plaintext.pdf` companion matches every non-encryption field of the truth entry.
                let plain_out = ashift_cmd()
                    .args(["tool", "pdf.info"])
                    .arg(&plaintext_input)
                    .arg("--json")
                    .output()
                    .expect("run ashift tool pdf.info on plaintext source");
                assert!(plain_out.status.success());
                let mut plain_actual: Value = serde_json::from_slice(&plain_out.stdout).unwrap();
                assert_eq!(plain_actual["encrypted"], Value::Bool(false));
                plain_actual["encrypted"] = Value::Bool(true);
                assert_eq!(plain_actual, expected, "plaintext mismatch for {stem}");
            }
            _ => {
                committed_pdf_count += 1;
                let input = pdf_fixture(&format!("{stem}.pdf"));
                let output = ashift_cmd()
                    .args(["tool", "pdf.info"])
                    .arg(&input)
                    .arg("--json")
                    .output()
                    .expect("run ashift tool pdf.info --json");
                assert!(
                    output.status.success(),
                    "pdf.info failed on {stem}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let actual: Value =
                    serde_json::from_slice(&output.stdout).expect("parse pdf.info JSON");
                assert_eq!(actual, expected, "truth mismatch for {stem}");
            }
        }
    }

    assert_eq!(
        committed_pdf_count, 19,
        "expected 19 committed .pdf fixtures"
    );
    assert_eq!(
        encrypted_stem_count, 3,
        "expected 3 encrypted fixture stems"
    );
}

#[test]
fn tools_command_lists_registry_json_and_human() {
    let data_dir = TempDir::new().expect("tempdir");
    let output = ashift_cmd()
        .env_remove("ASHIFT_PACK_PDFIUM")
        .env("ASHIFT_DATA_DIR", data_dir.path())
        .args(["tools", "--json"])
        .output()
        .expect("run ashift tools --json");
    assert!(output.status.success());
    let tools_json: Value = serde_json::from_slice(&output.stdout).expect("parse tools JSON");
    insta::assert_json_snapshot!("tools_json", tools_json);

    let group_out = ashift_cmd()
        .args(["tools", "--group", "organize", "--json"])
        .output()
        .expect("run ashift tools --group organize --json");
    assert!(group_out.status.success());
    let group_json: Value = serde_json::from_slice(&group_out.stdout).expect("parse group JSON");
    assert_eq!(group_json.as_array().unwrap().len(), 29);

    let optimize_out = ashift_cmd()
        .args(["tools", "--group", "optimize", "--json"])
        .output()
        .expect("run ashift tools --group optimize --json");
    assert!(optimize_out.status.success());
    let optimize_json: Value =
        serde_json::from_slice(&optimize_out.stdout).expect("parse optimize JSON");
    assert_eq!(optimize_json.as_array().unwrap().len(), 1);
    assert_eq!(optimize_json[0]["id"], "pdf.decompress");
    assert_eq!(tools_json.as_array().unwrap().len(), 35);

    let empty_group = ashift_cmd()
        .args(["tools", "--group", "security", "--json"])
        .output()
        .expect("run ashift tools --group security --json");
    assert!(empty_group.status.success());
    let empty_json: Value =
        serde_json::from_slice(&empty_group.stdout).expect("parse empty group JSON");
    assert_eq!(empty_json, json!([]));

    let human_out = ashift_cmd()
        .arg("tools")
        .output()
        .expect("run ashift tools");
    assert!(human_out.status.success());
    let human_stdout = String::from_utf8_lossy(&human_out.stdout);
    assert!(human_stdout.contains("pdf.info"));
    assert!(human_stdout.contains("organize"));
    assert!(human_stdout.contains("ariad-pdf"));
}

#[test]
fn tool_pdf_info_matches_truth_and_snapshots_on_three_corpus_files() {
    for stem in ["multipage-20", "broken-xref", "rotated-pages"] {
        let input = pdf_fixture(&format!("{stem}.pdf"));
        let output = ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&input)
            .arg("--json")
            .output()
            .expect("run ashift tool pdf.info --json");
        assert!(
            output.status.success(),
            "pdf.info failed on {stem}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).expect("parse pdf.info JSON");
        let expected = read_truth_pdf_info(stem);
        assert_eq!(actual, expected, "truth mismatch for {stem}");

        let snapshot_name = format!("tool_pdf_info_{}", stem.replace('-', "_"));
        insta::assert_json_snapshot!(snapshot_name, actual);
    }

    // Verify human output without -o and file output with -o.
    let tmp = TempDir::new().expect("tempdir");
    let out_file = tmp.path().join("report.info.json");
    let human_info = ashift_cmd()
        .args(["tool", "pdf.info"])
        .arg(pdf_fixture("broken-xref.pdf"))
        .output()
        .expect("run ashift tool pdf.info human");
    assert!(human_info.status.success());
    let human_stdout = String::from_utf8_lossy(&human_info.stdout);
    assert!(human_stdout.contains("pdf_version: 1.4"));
    assert!(human_stdout.contains("repaired_xref: true"));

    ashift_cmd()
        .args(["tool", "pdf.info"])
        .arg(pdf_fixture("multipage-20.pdf"))
        .arg("-o")
        .arg(&out_file)
        .assert()
        .success();
    let written: Value =
        serde_json::from_str(&fs::read_to_string(&out_file).expect("read written info.json"))
            .expect("parse written info.json");
    assert_eq!(written, read_truth_pdf_info("multipage-20"));
}

#[test]
fn tool_cli_error_classes_return_documented_exit_codes() {
    let tmp = TempDir::new().expect("tempdir");
    let sample_pdf = pdf_fixture("multipage-20.pdf");

    // Exit 8: unknown_tool
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.does_not_exist"])
            .arg(&sample_pdf),
        8,
        &["error[unknown_tool]"],
    );

    // Exit 9: invalid_params (schema mismatch)
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&sample_pdf)
            .args(["--param", "unexpected_param=42"]),
        9,
        &["error[invalid_params]"],
    );

    // Exit 9: invalid_params (inline secret via --param)
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&sample_pdf)
            .args(["--param", "password=hunter2"]),
        9,
        &[
            "error[invalid_params]",
            "--secret password=@<file> or --secret password=-",
        ],
    );

    // Exit 9: invalid_params (inline secret via --params-json)
    let bad_params_json = tmp.path().join("bad-params.json");
    fs::write(&bad_params_json, r#"{"password": "hunter2"}"#).unwrap();
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&sample_pdf)
            .arg("--params-json")
            .arg(&bad_params_json),
        9,
        &["error[invalid_params]"],
    );

    // Exit 9: invalid_params (inline literal via --secret without @file or -)
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&sample_pdf)
            .args(["--secret", "password=hunter2"]),
        9,
        &[
            "error[invalid_params]",
            "--secret password=@<file> or --secret password=-",
        ],
    );

    // Exit 10: wrong_input (wrong arity: 2 inputs to single-input tool)
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&sample_pdf)
            .arg(pdf_fixture("broken-xref.pdf")),
        10,
        &["error[wrong_input]"],
    );

    // Exit 10: wrong_input (wrong format: Markdown passed to PDF tool)
    let md_file = tmp.path().join("notes.md");
    fs::write(&md_file, b"# Hello\n\nWorld.\n").unwrap();
    assert_exit_and_stderr(
        ashift_cmd().args(["tool", "pdf.info"]).arg(&md_file),
        10,
        &["error[wrong_input]"],
    );

    // Exit 11: tier_not_allowed (Free-plan batch quota > 25 files on recipe run)
    let recipe_file = tmp.path().join("info-recipe.json");
    fs::write(
        &recipe_file,
        serde_json::to_vec_pretty(&json!({
            "version": "ariad-recipe/1",
            "steps": [{ "tool": "pdf.info" }]
        }))
        .unwrap(),
    )
    .unwrap();
    let out_dir = tmp.path().join("quota-out");
    fs::create_dir_all(&out_dir).unwrap();
    let mut quota_cmd = ashift_cmd();
    quota_cmd.args(["recipe", "run"]).arg(&recipe_file);
    for _ in 0..26 {
        quota_cmd.arg(&sample_pdf);
    }
    quota_cmd.arg("-o").arg(&out_dir).arg("--yes");
    assert_exit_and_stderr(&mut quota_cmd, 11, &["error[tier_not_allowed]"]);

    // Exit 12: encrypted_input (PDF requiring a user password)
    let encrypted_pdf = tmp.path().join("locked.pdf");
    write_encrypted_stub_pdf(&encrypted_pdf);
    assert_exit_and_stderr(
        ashift_cmd().args(["tool", "pdf.info"]).arg(&encrypted_pdf),
        12,
        &["error[encrypted_input]"],
    );

    // Exit 6: destination_exists (existing output without --overwrite)
    let existing_out = tmp.path().join("already-exists.info.json");
    fs::write(&existing_out, b"{}").unwrap();
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&sample_pdf)
            .arg("-o")
            .arg(&existing_out),
        6,
        &["error[destination_exists]"],
    );

    // Exit 4: limit_exceeded (nesting depth > Limits::local().max_nesting_depth == 64 in recipe run)
    let deep_md = tmp.path().join("deep-nesting.md");
    let deep_content = format!("{}deeply nested paragraph\n", "> ".repeat(70));
    fs::write(&deep_md, deep_content.as_bytes()).unwrap();
    let convert_recipe = tmp.path().join("convert-html-recipe.json");
    fs::write(
        &convert_recipe,
        serde_json::to_vec_pretty(&json!({
            "version": "ariad-recipe/1",
            "steps": [{ "convert": { "to": "html" } }]
        }))
        .unwrap(),
    )
    .unwrap();
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["recipe", "run"])
            .arg(&convert_recipe)
            .arg(&deep_md)
            .arg("-o")
            .arg(&out_dir)
            .arg("--yes"),
        4,
        &["error[limit_exceeded]"],
    );

    // Exit 2: usage (self-overwrite destination == input)
    let copy_pdf = tmp.path().join("self.pdf");
    fs::copy(&sample_pdf, &copy_pdf).unwrap();
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(&copy_pdf)
            .arg("-o")
            .arg(&copy_pdf)
            .arg("--overwrite"),
        2,
        &["error[usage]"],
    );

    // Exit 1: engine_failure (corrupt PDF with %PDF header) and io (missing file)
    let corrupt_pdf = tmp.path().join("corrupt.pdf");
    fs::write(&corrupt_pdf, b"%PDF-1.7\nnot a valid pdf body\n%%EOF\n").unwrap();
    assert_exit_and_stderr(
        ashift_cmd().args(["tool", "pdf.info"]).arg(&corrupt_pdf),
        1,
        &["error[engine_failure]"],
    );

    assert_exit_and_stderr(
        ashift_cmd()
            .args(["tool", "pdf.info"])
            .arg(tmp.path().join("missing-input.pdf")),
        1,
        &["error[io]"],
    );
}

#[test]
fn recipe_run_prints_steps_requires_yes_and_rejects_literal_secrets() {
    let tmp = TempDir::new().expect("tempdir");
    let md_in = tmp.path().join("guide.md");
    fs::write(&md_in, b"# Guide\n\nRecipe pipeline execution test.\n").unwrap();
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();

    let recipe_path = tmp.path().join("convert.recipe.json");
    fs::write(
        &recipe_path,
        serde_json::to_vec_pretty(&json!({
            "version": "ariad-recipe/1",
            "steps": [
                { "convert": { "to": "html" } }
            ],
            "naming": "{stem}.{n}.html"
        }))
        .unwrap(),
    )
    .unwrap();

    // 1. Imported recipe without --yes prints step summary and fails with exit code 9.
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["recipe", "run"])
            .arg(&recipe_path)
            .arg(&md_in)
            .arg("-o")
            .arg(&out_dir),
        9,
        &["1. convert -> html", "error[invalid_params]"],
    );

    // 2. Imported recipe with --yes prints step summary and publishes output atomically.
    let ok_out = ashift_cmd()
        .args(["recipe", "run"])
        .arg(&recipe_path)
        .arg(&md_in)
        .arg("-o")
        .arg(&out_dir)
        .args(["--yes", "--json"])
        .output()
        .expect("run recipe with --yes --json");
    assert!(
        ok_out.status.success(),
        "recipe run failed: {}",
        String::from_utf8_lossy(&ok_out.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&ok_out.stderr);
    assert!(stderr_text.contains("1. convert -> html"));
    let outcome: Value = serde_json::from_slice(&ok_out.stdout).expect("parse recipe outcome JSON");
    assert_eq!(outcome["artifacts"].as_array().unwrap().len(), 1);
    assert!(out_dir.join("guide.001.html").is_file());

    // 3. Recipe with a literal secret in step params is rejected with exit code 9.
    let secret_recipe_path = tmp.path().join("bad-secret.recipe.json");
    fs::write(
        &secret_recipe_path,
        serde_json::to_vec_pretty(&json!({
            "version": "ariad-recipe/1",
            "steps": [
                {
                    "tool": "pdf.decrypt",
                    "params": { "password": "inline-secret-value" }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    assert_exit_and_stderr(
        ashift_cmd()
            .args(["recipe", "run"])
            .arg(&secret_recipe_path)
            .arg(pdf_fixture("multipage-20.pdf"))
            .arg("-o")
            .arg(&out_dir)
            .arg("--yes"),
        9,
        &["error[invalid_params]"],
    );
}
