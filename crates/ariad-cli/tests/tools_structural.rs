use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const DETERMINISTIC_EPOCH: &str = "1700000000";

fn ashift_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ashift"));
    cmd.env("ASHIFT_PANDOC", pandoc_path());
    cmd.env("SOURCE_DATE_EPOCH", DETERMINISTIC_EPOCH);
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

fn inspect_pdf_info(pdf_path: &Path) -> Value {
    let out = ashift_cmd()
        .args(["tool", "pdf.info"])
        .arg(pdf_path)
        .arg("--json")
        .output()
        .expect("run ashift tool pdf.info");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        code,
        0,
        "pdf.info failed on {} (exit {code})\nstdout: {stdout}\nstderr: {stderr}",
        pdf_path.display()
    );
    let info: Value = serde_json::from_slice(&out.stdout).expect("parse pdf.info JSON");
    assert!(info.is_object(), "pdf.info output must be a JSON object");
    info
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
    let body_end = pos + 1;
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

struct GoldenCase {
    tool_id: &'static str,
    inputs: &'static [&'static str],
    params: &'static [&'static str],
    expected_outputs: &'static [&'static str],
}

const STRUCTURAL_GOLDEN_CASES: &[GoldenCase] = &[
    GoldenCase {
        tool_id: "pdf.merge",
        inputs: &["rotated-pages.pdf", "annotations.pdf"],
        params: &["outline=per-file", "page_labels=renumber"],
        expected_outputs: &["rotated-pages.merged.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.split",
        inputs: &["rotated-pages.pdf"],
        params: &["mode=every", "every=2"],
        expected_outputs: &["rotated-pages.part-001.pdf", "rotated-pages.part-002.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.extract_pages",
        inputs: &["multipage-20.pdf"],
        params: &["pages=1-3"],
        expected_outputs: &["multipage-20.extracted.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.delete_pages",
        inputs: &["rotated-pages.pdf"],
        params: &["pages=2,4"],
        expected_outputs: &["rotated-pages.trimmed.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.reorder",
        inputs: &["rotated-pages.pdf"],
        params: &["order=reverse"],
        expected_outputs: &["rotated-pages.reordered.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.alternate_mix",
        inputs: &["rotated-pages.pdf", "annotations.pdf"],
        params: &["reverse_second=true"],
        expected_outputs: &["rotated-pages.mixed.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.insert_blank",
        inputs: &["rotated-pages.pdf"],
        params: &["after=1-1", "count=2", "size=same"],
        expected_outputs: &["rotated-pages.inserted.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.rotate",
        inputs: &["rotated-pages.pdf"],
        params: &["pages=1-1", "angle=90"],
        expected_outputs: &["rotated-pages.rotated.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.crop",
        inputs: &["rotated-pages.pdf"],
        params: &[
            "pages=all",
            r#"box={"left":36,"bottom":36,"right":576,"top":756}"#,
            "target=crop",
        ],
        expected_outputs: &["rotated-pages.cropped.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.scale",
        inputs: &["mixed-sizes.pdf"],
        params: &["pages=all", "to=A4", "fit=contain"],
        expected_outputs: &["mixed-sizes.scaled.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.single_page",
        inputs: &["rotated-pages.pdf"],
        params: &[],
        expected_outputs: &["rotated-pages.single-page.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.rotate_content",
        inputs: &["rotated-pages.pdf"],
        params: &["pages=1-1", "angle_deg=45", "expand=true"],
        expected_outputs: &["rotated-pages.rotated-content.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.decompress",
        inputs: &["object-streams.pdf"],
        params: &["object_streams=expand"],
        expected_outputs: &["object-streams.decompressed.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.nup",
        inputs: &["rotated-pages.pdf"],
        params: &["per_sheet=2", "sheet=A4", "order=row"],
        expected_outputs: &["rotated-pages.nup.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.booklet",
        inputs: &["rotated-pages.pdf"],
        params: &["sheet=A4", "binding=left", "creep_pt=0"],
        expected_outputs: &["rotated-pages.booklet.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.overlay",
        inputs: &["rotated-pages.pdf", "watermark-layer.pdf"],
        params: &["overlay_pages=first", "position=over", "pages=all"],
        expected_outputs: &["rotated-pages.overlay.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.divide_pages",
        inputs: &["rotated-pages.pdf"],
        params: &["pages=all", "cols=2", "rows=2", "order=row"],
        expected_outputs: &["rotated-pages.divided.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.posterize",
        inputs: &["annotations.pdf"],
        params: &[
            r#"tiles={"cols":2,"rows":2}"#,
            "sheet=A4",
            "overlap_mm=10",
            "crop_marks=true",
        ],
        expected_outputs: &["annotations.posterized.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.metadata",
        inputs: &["rotated-pages.pdf"],
        params: &[r#"set={"title":"Golden Title","author":"AriadShift"}"#],
        expected_outputs: &["rotated-pages.metadata.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.outline",
        inputs: &["multipage-20.pdf"],
        params: &["action=clear"],
        expected_outputs: &["multipage-20.outline.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.attachments",
        inputs: &["rotated-pages.pdf", "annotations.pdf"],
        params: &["action=add"],
        expected_outputs: &["rotated-pages.attachments.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.page_labels",
        inputs: &["multipage-20.pdf"],
        params: &[
            r#"ranges=[{"from":1,"style":"roman-lower"},{"from":5,"style":"decimal","prefix":"p-","start":1}]"#,
        ],
        expected_outputs: &["multipage-20.labeled.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.viewer_prefs",
        inputs: &["rotated-pages.pdf"],
        params: &["page_layout=two-column-left", "fit_window=true"],
        expected_outputs: &["rotated-pages.prefs.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.portfolio",
        inputs: &["rotated-pages.pdf", "annotations.pdf"],
        params: &["action=create", "view=details"],
        expected_outputs: &["rotated-pages.portfolio.pdf"],
    },
    GoldenCase {
        tool_id: "pdf.layers",
        inputs: &["watermark-layer.pdf"],
        params: &["action=remove", r#"names=["AriadShift Watermark"]"#],
        expected_outputs: &["watermark-layer.layers.pdf"],
    },
];

fn run_golden_case_into(case: &GoldenCase, out_dir: &Path) -> Vec<PathBuf> {
    fs::create_dir_all(out_dir).expect("create output dir");
    let mut cmd = ashift_cmd();
    cmd.args(["tool", case.tool_id]);
    for input in case.inputs {
        cmd.arg(pdf_fixture(input));
    }
    for param in case.params {
        cmd.args(["--param", param]);
    }
    cmd.arg("-o").arg(out_dir);

    let out = cmd.output().expect("execute ashift tool");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        code, 0,
        "tool {} failed with exit {code}\nstdout: {stdout}\nstderr: {stderr}",
        case.tool_id
    );

    let printed_lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        printed_lines.len(),
        case.expected_outputs.len(),
        "tool {} stdout artifact line count mismatch: {printed_lines:?}",
        case.tool_id
    );

    let mut produced = Vec::with_capacity(case.expected_outputs.len());
    for (line, expected_name) in printed_lines.iter().zip(case.expected_outputs.iter()) {
        let printed_path = PathBuf::from(line);
        assert_eq!(
            printed_path.file_name().and_then(|n| n.to_str()),
            Some(*expected_name),
            "tool {} output filename mismatch",
            case.tool_id
        );
        let expected_path = out_dir.join(expected_name);
        assert_eq!(printed_path, expected_path);
        assert!(
            expected_path.is_file(),
            "tool {} expected output file {} to exist",
            case.tool_id,
            expected_path.display()
        );
        produced.push(expected_path);
    }
    produced
}

fn assert_case_facts(tool_id: &str, paths: &[PathBuf], infos: &[Value]) {
    for info in infos {
        assert_eq!(
            info["repaired_xref"],
            json!(false),
            "tool {tool_id} output must have a clean cross-reference table"
        );
        assert_eq!(info["encrypted"], json!(false));
    }

    match tool_id {
        "pdf.merge" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(5));
            assert_eq!(info["outline_count"], json!(2));
            let pages = info["pages"].as_array().expect("pages array");
            assert_eq!(pages[0]["rotation"], json!(0));
            assert_eq!(pages[1]["rotation"], json!(90));
            assert_eq!(pages[2]["rotation"], json!(180));
            assert_eq!(pages[3]["rotation"], json!(270));
            assert_eq!(pages[4]["rotation"], json!(0));
        }
        "pdf.split" => {
            assert_eq!(infos.len(), 2);
            assert_eq!(infos[0]["page_count"], json!(2));
            assert_eq!(infos[0]["pages"][0]["rotation"], json!(0));
            assert_eq!(infos[0]["pages"][1]["rotation"], json!(90));
            assert_eq!(infos[1]["page_count"], json!(2));
            assert_eq!(infos[1]["pages"][0]["rotation"], json!(180));
            assert_eq!(infos[1]["pages"][1]["rotation"], json!(270));
        }
        "pdf.extract_pages" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(3));
            assert_eq!(info["pages"][0]["paper"], json!("A4"));
        }
        "pdf.delete_pages" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(2));
            assert_eq!(info["pages"][0]["rotation"], json!(0));
            assert_eq!(info["pages"][1]["rotation"], json!(180));
        }
        "pdf.reorder" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            assert_eq!(info["pages"][0]["rotation"], json!(270));
            assert_eq!(info["pages"][1]["rotation"], json!(180));
            assert_eq!(info["pages"][2]["rotation"], json!(90));
            assert_eq!(info["pages"][3]["rotation"], json!(0));
        }
        "pdf.alternate_mix" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(5));
        }
        "pdf.insert_blank" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(6));
            assert_eq!(info["pages"][1]["blank"], json!(true));
            assert_eq!(info["pages"][2]["blank"], json!(true));
        }
        "pdf.rotate" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            assert_eq!(info["pages"][0]["rotation"], json!(90));
            assert_eq!(info["pages"][1]["rotation"], json!(90));
            assert_eq!(info["pages"][2]["rotation"], json!(180));
            assert_eq!(info["pages"][3]["rotation"], json!(270));
        }
        "pdf.crop" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            assert_eq!(info["pages"][0]["width_pt"], json!(540.0));
            assert_eq!(info["pages"][0]["height_pt"], json!(720.0));
        }
        "pdf.scale" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(3));
            for page in info["pages"].as_array().expect("pages") {
                assert_eq!(page["paper"], json!("A4"));
                assert_eq!(page["width_pt"], json!(595.28));
                assert_eq!(page["height_pt"], json!(841.89));
            }
        }
        "pdf.single_page" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(1));
            let height = info["pages"][0]["height_pt"].as_f64().unwrap();
            assert!(
                height > 2000.0,
                "expected tall single-page height, got {height}"
            );
        }
        "pdf.rotate_content" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            let width = info["pages"][0]["width_pt"].as_f64().unwrap();
            assert!(
                width > 612.0,
                "expected expanded page width after 45deg content rotation, got {width}"
            );
        }
        "pdf.decompress" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(2));
        }
        "pdf.nup" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(2));
        }
        "pdf.booklet" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(2));
        }
        "pdf.overlay" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
        }
        "pdf.divide_pages" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(16));
        }
        "pdf.posterize" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            assert_eq!(info["pages"][0]["paper"], json!("A4"));
        }
        "pdf.metadata" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            assert_eq!(info["metadata"]["title"], json!("Golden Title"));
            assert_eq!(info["metadata"]["author"], json!("AriadShift"));
        }
        "pdf.outline" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(20));
            assert_eq!(info["outline_count"], json!(0));
        }
        "pdf.attachments" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            assert_eq!(info["has_attachments"], json!(true));
        }
        "pdf.page_labels" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(20));
            let raw = fs::read(&paths[0]).expect("read labeled pdf");
            let text = String::from_utf8_lossy(&raw);
            assert!(text.contains("/PageLabels"), "expected /PageLabels in PDF");
        }
        "pdf.viewer_prefs" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(4));
            let raw = fs::read(&paths[0]).expect("read viewer_prefs pdf");
            let text = String::from_utf8_lossy(&raw);
            assert!(
                text.contains("/ViewerPreferences") && text.contains("/TwoColumnLeft"),
                "expected /ViewerPreferences with /TwoColumnLeft in PDF"
            );
        }
        "pdf.portfolio" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(1));
            assert_eq!(info["has_attachments"], json!(true));
        }
        "pdf.layers" => {
            let info = &infos[0];
            assert_eq!(info["page_count"], json!(3));
            let raw = fs::read(&paths[0]).expect("read layers pdf");
            let text = String::from_utf8_lossy(&raw);
            assert!(
                !text.contains("AriadShift Watermark"),
                "expected removed layer name to be absent from output PDF"
            );
        }
        other => panic!("unexpected structural tool id {other}"),
    }
}

#[test]
fn all_twenty_five_structural_tools_golden_facts_and_deterministic_bytes() {
    assert_eq!(
        STRUCTURAL_GOLDEN_CASES.len(),
        25,
        "must cover all 25 structural PDF tools"
    );

    let tmp = TempDir::new().expect("tempdir");
    let mut snapshot_summaries = Vec::with_capacity(STRUCTURAL_GOLDEN_CASES.len());

    for (idx, case) in STRUCTURAL_GOLDEN_CASES.iter().enumerate() {
        let run1_dir = tmp.path().join(format!("run1-{idx:02}"));
        let run2_dir = tmp.path().join(format!("run2-{idx:02}"));

        let run1_paths = run_golden_case_into(case, &run1_dir);
        let run2_paths = run_golden_case_into(case, &run2_dir);

        let mut infos = Vec::with_capacity(run1_paths.len());
        let mut artifact_summaries = Vec::with_capacity(run1_paths.len());

        for (p1, p2) in run1_paths.iter().zip(run2_paths.iter()) {
            let bytes1 = fs::read(p1).expect("read run1 output");
            let bytes2 = fs::read(p2).expect("read run2 output");
            assert_eq!(
                bytes1,
                bytes2,
                "tool {} did not produce byte-identical output on second run for {}",
                case.tool_id,
                p1.display()
            );

            let sha256 = hex::encode(Sha256::digest(&bytes1));
            let info = inspect_pdf_info(p1);
            artifact_summaries.push(json!({
                "file_name": p1.file_name().unwrap().to_str().unwrap(),
                "bytes": bytes1.len(),
                "sha256": sha256,
                "page_count": info["page_count"],
                "repaired_xref": info["repaired_xref"],
                "outline_count": info["outline_count"],
                "has_attachments": info["has_attachments"],
                "metadata": info["metadata"],
                "first_page": info["pages"].as_array().and_then(|p| p.first()).cloned(),
            }));
            infos.push(info);
        }

        assert_case_facts(case.tool_id, &run1_paths, &infos);

        snapshot_summaries.push(json!({
            "tool": case.tool_id,
            "artifacts": artifact_summaries,
        }));
    }

    insta::assert_json_snapshot!("structural_tools_golden_summary", snapshot_summaries);
}

#[test]
fn structural_tool_on_broken_xref_emits_repair_warning_and_writes_valid_xref() {
    let tmp = TempDir::new().expect("tempdir");
    let human_dir = tmp.path().join("human");
    let json_dir = tmp.path().join("json");
    fs::create_dir_all(&human_dir).expect("create human dir");
    fs::create_dir_all(&json_dir).expect("create json dir");

    let human_out = ashift_cmd()
        .args(["tool", "pdf.rotate"])
        .arg(pdf_fixture("broken-xref.pdf"))
        .args(["--param", "pages=1-1", "--param", "angle=90", "-o"])
        .arg(&human_dir)
        .output()
        .expect("run pdf.rotate on broken-xref.pdf");

    assert_eq!(human_out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&human_out.stderr);
    assert!(
        stderr.contains(
            "warning[repaired_xref]: cross-reference table was malformed and reconstructed from object offsets"
        ),
        "expected repaired_xref warning on stderr, got:\n{stderr}"
    );

    let rewritten_pdf = human_dir.join("broken-xref.rotated.pdf");
    assert!(rewritten_pdf.is_file(), "expected rewritten PDF to exist");

    let info_out = ashift_cmd()
        .args(["tool", "pdf.info"])
        .arg(&rewritten_pdf)
        .arg("--json")
        .output()
        .expect("run pdf.info on rewritten broken-xref.rotated.pdf");
    assert_eq!(info_out.status.code(), Some(0));
    let info_stderr = String::from_utf8_lossy(&info_out.stderr);
    assert!(
        !info_stderr.contains("repaired_xref"),
        "rewritten PDF must not trigger repaired_xref warning again: {info_stderr}"
    );
    let info: Value = serde_json::from_slice(&info_out.stdout).expect("parse pdf.info");
    assert_eq!(info["repaired_xref"], json!(false));
    assert_eq!(info["page_count"], json!(1));
    assert_eq!(info["pages"][0]["rotation"], json!(90));

    let json_out = ashift_cmd()
        .args(["tool", "pdf.rotate"])
        .arg(pdf_fixture("broken-xref.pdf"))
        .args([
            "--param",
            "pages=1-1",
            "--param",
            "angle=90",
            "--json",
            "-o",
        ])
        .arg(&json_dir)
        .output()
        .expect("run pdf.rotate --json on broken-xref.pdf");
    assert_eq!(json_out.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&json_out.stdout).expect("parse json output");
    assert_eq!(envelope["tool"], json!("pdf.rotate"));
    assert_eq!(
        envelope["warnings"],
        json!([{
            "code": "repaired_xref",
            "message": "cross-reference table was malformed and reconstructed from object offsets"
        }])
    );
}

#[test]
fn structural_tools_reject_encrypted_input_with_exit_code_twelve() {
    const OWNER_ONLY_ENCRYPT_DICT: &[u8] = b"<< /CF << /StdCF << /AuthEvent /DocOpen /CFM /Identity /Length 32 >> >> /Filter /Standard /Length 256 /O <1759241f12558fbcb7c006a96405baa704c76400fccd563698d5e4d003a23af01472fedd99192585f679d6e155cfd0d5> /OE <fc53d58a60852ff606cfd6fb0ff86b111eed25fbc6a6992969ba3776f699555a> /P -4 /Perms <f113a416d97ac067641036cd7727fcd4> /R 6 /StmF /StdCF /StrF /StdCF /U <6d20654d13677ac0315cf65c5d612b986e7f6e51727939fed3643034517f8d328bca71289a564169ce97d098f6293467> /\x55E <93a1e62b8928181966fdef4ddcedf45247e1bbba7a6ac22f7caeccd6b32ee3b1> /V 5 >>";
    const USER_LOCKED_AES256_ENCRYPT_DICT: &[u8] = b"<< /CF << /StdCF << /AuthEvent /DocOpen /CFM /AESV3 /Length 32 >> >> /Filter /Standard /Length 256 /O <000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f> /OE <000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f> /P -4 /Perms <000102030405060708090a0b0c0d0e0f> /R 6 /StmF /StdCF /StrF /StdCF /U <1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100202122232425262728292a2b2c2d2e2f> /\x55E <1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100> /V 5 >>";

    let tmp = TempDir::new().expect("tempdir");
    let inputs_dir = tmp.path().join("inputs");
    let out_dir = tmp.path().join("outputs");
    fs::create_dir_all(&inputs_dir).expect("create inputs dir");
    fs::create_dir_all(&out_dir).expect("create outputs dir");

    let user_locked = inputs_dir.join("encrypted-aes256.pdf");
    materialize_encrypted_from_plaintext(
        &pdf_fixture("encrypted-aes256.plaintext.pdf"),
        &user_locked,
        USER_LOCKED_AES256_ENCRYPT_DICT,
    );

    let owner_only = inputs_dir.join("encrypted-owner-only.pdf");
    materialize_encrypted_from_plaintext(
        &pdf_fixture("encrypted-owner-only.plaintext.pdf"),
        &owner_only,
        OWNER_ONLY_ENCRYPT_DICT,
    );

    for encrypted_input in [&user_locked, &owner_only] {
        let out = ashift_cmd()
            .args(["tool", "pdf.rotate"])
            .arg(encrypted_input)
            .args(["--param", "angle=90", "-o"])
            .arg(&out_dir)
            .output()
            .expect("run pdf.rotate on encrypted input");

        assert_eq!(
            out.status.code(),
            Some(12),
            "expected exit code 12 for {}, got {:?}\nstderr: {}",
            encrypted_input.display(),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("error[encrypted_input]") && stderr.contains("pdf.decrypt"),
            "expected encrypted_input error mentioning pdf.decrypt, got:\n{stderr}"
        );
    }

    let remaining: Vec<_> = fs::read_dir(&out_dir).expect("read out_dir").collect();
    assert!(
        remaining.is_empty(),
        "failed encrypted tool runs must not leave partial outputs: {remaining:?}"
    );
}

#[test]
fn multi_output_pdf_split_lists_every_artifact_in_human_and_json_modes() {
    let tmp = TempDir::new().expect("tempdir");
    let human_dir = tmp.path().join("human");
    let json_dir = tmp.path().join("json");
    fs::create_dir_all(&human_dir).expect("create human dir");
    fs::create_dir_all(&json_dir).expect("create json dir");

    let human_out = ashift_cmd()
        .args(["tool", "pdf.split"])
        .arg(pdf_fixture("multipage-20.pdf"))
        .args([
            "--param",
            "mode=ranges",
            "--param",
            r#"ranges=["1-3","4-8","9-20"]"#,
            "-o",
        ])
        .arg(&human_dir)
        .output()
        .expect("run pdf.split in human mode");

    assert_eq!(human_out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&human_out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "expected 3 artifact lines, got {lines:?}");

    let expected_parts = [
        ("multipage-20.part-001.pdf", 3usize),
        ("multipage-20.part-002.pdf", 5usize),
        ("multipage-20.part-003.pdf", 12usize),
    ];

    for (line, (expected_name, expected_pages)) in lines.iter().zip(expected_parts.iter()) {
        let part_path = human_dir.join(expected_name);
        assert_eq!(Path::new(line), part_path);
        assert!(part_path.is_file());
        let info = inspect_pdf_info(&part_path);
        assert_eq!(info["page_count"], json!(*expected_pages));
        assert_eq!(info["repaired_xref"], json!(false));
    }

    let json_out = ashift_cmd()
        .args(["tool", "pdf.split"])
        .arg(pdf_fixture("multipage-20.pdf"))
        .args([
            "--param",
            "mode=ranges",
            "--param",
            r#"ranges=["1-3","4-8","9-20"]"#,
            "--json",
            "-o",
        ])
        .arg(&json_dir)
        .output()
        .expect("run pdf.split in json mode");

    assert_eq!(json_out.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&json_out.stdout).expect("parse split json");
    assert_eq!(envelope["tool"], json!("pdf.split"));
    let artifacts = envelope["artifacts"].as_array().expect("artifacts array");
    assert_eq!(artifacts.len(), 3);
    for (artifact, (expected_name, _)) in artifacts.iter().zip(expected_parts.iter()) {
        let expected_path = json_dir.join(expected_name);
        assert_eq!(
            artifact["path"].as_str(),
            Some(expected_path.to_str().unwrap())
        );
        let actual_len = fs::metadata(&expected_path).expect("stat part").len();
        assert_eq!(artifact["bytes"], json!(actual_len));
    }
}

/// Runs `ashift tool <tool>` and asserts exit code 0. `out` becomes `-o <out>`.
fn run_tool_ok(
    tool: &str,
    inputs: &[&Path],
    params: &[&str],
    flags: &[&str],
    out: Option<&Path>,
) -> std::process::Output {
    let mut cmd = ashift_cmd();
    cmd.args(["tool", tool]);
    cmd.args(inputs);
    for param in params {
        cmd.args(["--param", param]);
    }
    cmd.args(flags);
    if let Some(out) = out {
        cmd.arg("-o").arg(out);
    }
    let output = cmd.output().expect("execute ashift tool");
    assert_eq!(
        output.status.code(),
        Some(0),
        "tool {tool} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn sorted_file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn top_level_bookmark_titles(outline: &Value) -> Vec<String> {
    outline
        .as_array()
        .expect("outline export is a JSON array")
        .iter()
        .map(|node| node["title"].as_str().expect("bookmark title").to_owned())
        .collect()
}

#[test]
fn merge_per_file_outline_titles_use_input_stems() {
    let tmp = TempDir::new().expect("tempdir");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let rotated = pdf_fixture("rotated-pages.pdf");
    let annotations = pdf_fixture("annotations.pdf");
    run_tool_ok(
        "pdf.merge",
        &[&rotated, &annotations],
        &["outline=per-file"],
        &[],
        Some(&out_dir),
    );
    let merged = out_dir.join("rotated-pages.merged.pdf");

    let export = run_tool_ok(
        "pdf.outline",
        &[&merged],
        &["action=export"],
        &["--json"],
        None,
    );
    let outline: Value = serde_json::from_slice(&export.stdout).expect("parse outline export");
    assert_eq!(
        top_level_bookmark_titles(&outline),
        ["rotated-pages", "annotations"],
        "per-file bookmarks must carry the input file stems, not staged names"
    );
}

#[test]
fn json_emitting_actions_print_json_and_never_write_a_pdf() {
    let tmp = TempDir::new().expect("tempdir");
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).expect("create work dir");

    let rotated = pdf_fixture("rotated-pages.pdf");
    let annotations = pdf_fixture("annotations.pdf");
    let inventory = tmp.path().join("inventory.csv");
    fs::write(&inventory, "id,name\n1,Anh\n2,Bình\n").expect("write csv");

    run_tool_ok(
        "pdf.merge",
        &[&rotated, &annotations],
        &["outline=per-file"],
        &[],
        Some(&work),
    );
    let merged = work.join("rotated-pages.merged.pdf");
    run_tool_ok(
        "pdf.attachments",
        &[&merged, &inventory],
        &["action=add"],
        &[],
        Some(&work),
    );
    let attached = work.join("rotated-pages.merged.attachments.pdf");
    run_tool_ok(
        "pdf.portfolio",
        &[&rotated, &annotations],
        &["action=create"],
        &[],
        Some(&work),
    );
    let portfolio = work.join("rotated-pages.portfolio.pdf");
    fs::copy(
        pdf_fixture("watermark-layer.pdf"),
        work.join("watermark-layer.pdf"),
    )
    .expect("copy watermark fixture");
    let layered = work.join("watermark-layer.pdf");

    let cases: [(&str, &Path, &str, &str); 4] = [
        ("pdf.outline", &merged, "action=export", "rotated-pages"),
        ("pdf.attachments", &attached, "action=list", "inventory.csv"),
        (
            "pdf.portfolio",
            &portfolio,
            "action=list",
            "annotations.pdf",
        ),
        (
            "pdf.layers",
            &layered,
            "action=list",
            "AriadShift Watermark",
        ),
    ];

    let files_before = sorted_file_names(&work);
    for (idx, (tool, input, action, expected_fragment)) in cases.iter().enumerate() {
        // Default target: the JSON is printed and no file appears beside the input.
        let printed = run_tool_ok(tool, &[input], &[action], &["--json"], None);
        let json: Value = serde_json::from_slice(&printed.stdout)
            .unwrap_or_else(|err| panic!("{tool} must print JSON: {err}"));
        assert!(
            json.to_string().contains(expected_fragment),
            "{tool} JSON must mention {expected_fragment}: {json}"
        );
        assert_eq!(
            sorted_file_names(&work),
            files_before,
            "{tool} must not publish a default file beside the input"
        );

        // Human mode prints the same JSON, not the pdf.info summary.
        let human = run_tool_ok(tool, &[input], &[action], &[], None);
        let human_json: Value =
            serde_json::from_slice(&human.stdout).expect("human mode prints JSON");
        assert_eq!(
            human_json, json,
            "{tool} human output must be the JSON payload"
        );

        // Explicit `-o <file>.json` is accepted and holds the same JSON.
        let json_dir = tmp.path().join(format!("json-{idx}"));
        fs::create_dir_all(&json_dir).expect("create json dir");
        let json_file = json_dir.join("result.json");
        run_tool_ok(tool, &[input], &[action], &[], Some(&json_file));
        let written: Value =
            serde_json::from_slice(&fs::read(&json_file).expect("read json output"))
                .expect("written file is JSON");
        assert_eq!(written, json, "{tool} -o file must hold the printed JSON");
        assert_eq!(sorted_file_names(&json_dir), ["result.json"]);
    }
}

#[test]
fn attachments_add_csv_and_pdf_extract_keeps_original_names_and_bytes() {
    let tmp = TempDir::new().expect("tempdir");
    let carrier_dir = tmp.path().join("carrier");
    let extract_dir = tmp.path().join("extract");
    fs::create_dir_all(&carrier_dir).expect("create carrier dir");
    fs::create_dir_all(&extract_dir).expect("create extract dir");

    let inventory = tmp.path().join("inventory.csv");
    let inventory_bytes = "id,name\r\n1,Nguyễn Văn An\r\n2,Trần Thị Bình\r\n".as_bytes();
    fs::write(&inventory, inventory_bytes).expect("write csv");
    let companion = pdf_fixture("annotations.pdf");
    let companion_bytes = fs::read(&companion).expect("read companion pdf");

    run_tool_ok(
        "pdf.attachments",
        &[&pdf_fixture("rotated-pages.pdf"), &inventory, &companion],
        &["action=add"],
        &[],
        Some(&carrier_dir),
    );
    let carrier = carrier_dir.join("rotated-pages.attachments.pdf");

    let listed = run_tool_ok(
        "pdf.attachments",
        &[&carrier],
        &["action=list"],
        &["--json"],
        None,
    );
    let entries: Value = serde_json::from_slice(&listed.stdout).expect("parse attachment list");
    let entries = entries.as_array().expect("attachment list array");
    let by_name = |name: &str| {
        entries
            .iter()
            .find(|entry| entry["name"] == json!(name))
            .unwrap_or_else(|| panic!("attachment {name} missing from {entries:?}"))
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(by_name("inventory.csv")["media_type"], json!("text/csv"));
    assert_eq!(
        by_name("inventory.csv")["sha256"],
        json!(hex::encode(Sha256::digest(inventory_bytes)))
    );
    assert_eq!(
        by_name("annotations.pdf")["sha256"],
        json!(hex::encode(Sha256::digest(&companion_bytes))),
        "an attached PDF must keep the digest of its original bytes"
    );

    run_tool_ok(
        "pdf.attachments",
        &[&carrier],
        &["action=extract"],
        &[],
        Some(&extract_dir),
    );
    assert_eq!(
        sorted_file_names(&extract_dir),
        [
            "rotated-pages.attachments.attachment-001.annotations.pdf",
            "rotated-pages.attachments.attachment-002.inventory.csv",
        ],
        "extracted files keep their attachment names and extensions"
    );
    assert_eq!(
        fs::read(extract_dir.join("rotated-pages.attachments.attachment-002.inventory.csv"))
            .expect("read extracted csv"),
        inventory_bytes
    );
    assert_eq!(
        fs::read(extract_dir.join("rotated-pages.attachments.attachment-001.annotations.pdf"))
            .expect("read extracted pdf"),
        companion_bytes
    );
}
