//! Document and page editing through the CLI, including signed revisions and crash recovery.

#[path = "support/edit.rs"]
mod edit_support;

use ariad_core::{
    fidelity,
    ir::{Block, BlockId, Document, Inline, blocks_with_paths},
};
use edit_support::PdfiumPack;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command(pack: Option<&PdfiumPack>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ashift"));
    cmd.env_remove("ASHIFT_PLAN");
    cmd.env(
        "ASHIFT_PANDOC",
        std::env::var_os("ASHIFT_PANDOC")
            .unwrap_or_else(|| root().join(".tools/pandoc/bin/pandoc").into_os_string()),
    );
    if let Some(pack) = pack {
        cmd.env("ASHIFT_PLAN", "pro");
        cmd.env("ASHIFT_PACK_PDFIUM", pack.path());
    }
    cmd
}

fn outcome(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn tool(
    pack: Option<&PdfiumPack>,
    id: &str,
    inputs: &[&Path],
    output: &Path,
    params: Value,
) -> Value {
    // stdin keeps large ordered op lists out of argv and exercises --params-json delivery.
    let mut cmd = command(pack);
    cmd.args(["tool", id])
        .args(inputs)
        .arg("-o")
        .arg(output)
        .args(["--json", "--params-json", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&params).unwrap())
        .unwrap();
    outcome(child.wait_with_output().unwrap())
}

fn paragraph(id: &str, text: &str) -> Block {
    Block::Paragraph {
        id: BlockId::new(id).unwrap(),
        style: None,
        direct: Default::default(),
        content: vec![Inline::Text { text: text.into() }],
    }
}

fn read_ir(path: &Path) -> Document {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

struct EditingProcess(Child);

impl Drop for EditingProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn first_text_object_id(path: &Path) -> Value {
    read_json(path)["pages"][0]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["kind"] == "text")
        .unwrap()["id"]
        .clone()
}

#[test]
fn page_tools_reject_the_default_free_plan_before_engine_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let input = root().join("fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    for id in ["edit.page_objects", "edit.apply"] {
        let output = dir.path().join(id);
        let result = command(None)
            .args(["tool", id])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(11));
        assert!(String::from_utf8_lossy(&result.stderr).contains("tier_not_allowed"));
        assert!(!output.exists());
    }
}

#[test]
fn local_plan_values_are_trimmed_and_invalid_values_fail_closed_for_tools_and_recipes() {
    let dir = tempfile::tempdir().unwrap();
    let input = root().join("fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    for plan in ["free", "pro", "team", "enterprise", "  pro  "] {
        outcome(
            command(None)
                .envs([("ASHIFT_PLAN", plan)])
                .args(["tool", "pdf.info"])
                .arg(&input)
                .arg("--json")
                .output()
                .unwrap(),
        );
    }
    let recipe = dir.path().join("recipe.json");
    fs::write(
        &recipe,
        serde_json::to_vec(&json!({"version":"ariad-recipe/1", "steps":[{"tool":"pdf.info"}]}))
            .unwrap(),
    )
    .unwrap();
    for plan in edit_support::invalid_local_plans() {
        let tool = command(None)
            .envs([("ASHIFT_PLAN", &plan)])
            .args(["tool", "pdf.info"])
            .arg(&input)
            .arg("--json")
            .output()
            .unwrap();
        let output = dir.path().join("refused-recipe");
        let recipe = command(None)
            .envs([("ASHIFT_PLAN", &plan)])
            .args(["recipe", "run"])
            .arg(&recipe)
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(["--yes", "--json"])
            .output()
            .unwrap();
        for result in [tool, recipe] {
            assert_eq!(result.status.code(), Some(9));
            assert!(String::from_utf8_lossy(&result.stderr).contains("invalid_params"));
        }
        assert!(!output.exists());
    }
}

#[test]
fn recipes_forward_the_local_plan_to_page_edit_steps() {
    let Some(pack) = PdfiumPack::new() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = root().join("fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    let recipe = dir.path().join("edit-recipe.json");
    fs::write(&recipe, serde_json::to_vec(&json!({"version":"ariad-recipe/1", "steps":[{"tool":"edit.apply", "params":{"ops":[{"op":"add_text", "page":1, "text":"Recipe Tiếng Việt", "x_pt":30.0, "y_pt":30.0, "font_size":12.0}]}}]})).unwrap()).unwrap();
    let denied = dir.path().join("free");
    let result = command(Some(&pack))
        .env_remove("ASHIFT_PLAN")
        .args(["recipe", "run"])
        .arg(&recipe)
        .arg(&input)
        .arg("-o")
        .arg(&denied)
        .args(["--yes", "--json"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(11));
    let output = dir.path().join("pro");
    let result = outcome(
        command(Some(&pack))
            .args(["recipe", "run"])
            .arg(&recipe)
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(["--yes", "--json"])
            .output()
            .unwrap(),
    );
    let artifact = result["artifacts"][0]["path"].as_str().unwrap();
    assert!(
        lopdf::Document::load(artifact)
            .unwrap()
            .extract_text(&[1])
            .unwrap()
            .contains("Recipe Tiếng Việt")
    );
}

#[test]
fn document_mode_opens_edits_and_exports_all_formats_with_measured_fidelity() {
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("original.md");
    fs::write(
        &md,
        "Original paragraph\n\nRemove this paragraph\n\nKeep this paragraph\n",
    )
    .unwrap();
    let mut inputs = vec![md.clone()];
    for format in ["html", "docx", "pdf"] {
        let path = dir.path().join(format!("original.{format}"));
        outcome(
            command(None)
                .arg("convert")
                .arg(&md)
                .args(["--to", format, "-o"])
                .arg(&path)
                .arg("--json")
                .output()
                .unwrap(),
        );
        inputs.push(path);
    }
    for input in inputs {
        let format = input.extension().unwrap().to_str().unwrap();
        let opened = dir.path().join(format!("opened-{format}.json"));
        let result = tool(None, "edit.open_document", &[&input], &opened, json!({}));
        let mut doc = read_ir(&opened);
        assert_eq!(doc.version, "ariad-ir/2");
        assert_eq!(doc.source_map.len(), blocks_with_paths(&doc).len());
        assert!(!doc.source_map.is_empty());
        let previews = result["metrics"]["fidelity_preview"].as_array().unwrap();
        assert_eq!(previews.len(), 4);
        assert_eq!(
            previews
                .iter()
                .map(|p| p["format"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["pdf", "docx", "html", "md"]
        );
        if format == "pdf" {
            assert!(doc.layout.is_some());
        }
        let modified_index = doc
            .body
            .iter()
            .position(|b| matches!(b, Block::Paragraph { .. }))
            .unwrap();
        let modified_id = doc.body[modified_index].id().to_string();
        if let Block::Paragraph { content, .. } = &mut doc.body[modified_index] {
            *content = vec![Inline::Text {
                text: "Edited Tiếng Việt paragraph".into(),
            }];
        }
        let removed = doc.body.remove(if modified_index == 0 { 1 } else { 0 });
        let removed_id = removed.id().to_string();
        for (_, block) in blocks_with_paths(&Document {
            body: vec![removed],
            ..Default::default()
        }) {
            doc.source_map.remove(block.id());
            if let Some(layout) = &mut doc.layout {
                layout.blocks.remove(block.id());
            }
        }
        doc.body
            .push(paragraph("added-paragraph", "Added paragraph"));
        let edited = dir.path().join(format!("edited-{format}.ir.json"));
        fs::write(&edited, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
        for target in ["pdf", "docx", "html", "md"] {
            for baseline in [false, true] {
                let output = dir
                    .path()
                    .join(format!("export-{format}-{baseline}.{target}"));
                let export_inputs = if baseline {
                    vec![edited.as_path(), input.as_path()]
                } else {
                    vec![edited.as_path()]
                };
                let result = tool(
                    None,
                    "edit.export_document",
                    &export_inputs,
                    &output,
                    json!({"format":target}),
                );
                assert_eq!(result["artifacts"][0]["format"], target);
                let report = &result["metrics"]["fidelity"];
                assert_eq!(report["status"], "measured", "{result}");
                assert_eq!(report["basis"], "runtime_readback");
                assert!(
                    report["score"]
                        .as_f64()
                        .is_some_and(|s| (0.0..=1.0).contains(&s)),
                    "{report}"
                );
                let changes = &result["metrics"]["change_list"];
                for key in ["added", "removed", "modified"] {
                    assert!(changes[key].is_array());
                }
                if baseline {
                    assert!(
                        changes["added"]
                            .as_array()
                            .unwrap()
                            .contains(&json!("added-paragraph"))
                    );
                    assert!(
                        changes["removed"]
                            .as_array()
                            .unwrap()
                            .contains(&json!(removed_id))
                    );
                    assert!(
                        changes["modified"]
                            .as_array()
                            .unwrap()
                            .contains(&json!(modified_id))
                    );
                }
                let readback = dir
                    .path()
                    .join(format!("readback-{format}-{target}-{baseline}.json"));
                tool(
                    None,
                    "edit.open_document",
                    &[&output],
                    &readback,
                    json!({"target_format":"md"}),
                );
                let text = fidelity::observe(&read_ir(&readback)).unwrap().text;
                assert!(text.contains("Edited Tiếng Việt paragraph"), "{text}");
                assert!(text.contains("Added paragraph"));
                assert!(!text.contains("Remove this paragraph"));
            }
        }
    }
}

#[test]
fn subset_font_replacement_preserves_vietnamese_and_refuses_uncovered_glyphs() {
    let Some(pack) = PdfiumPack::new() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = root().join("fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    let original = fs::read(&input).unwrap();
    let objects = dir.path().join("objects.json");
    tool(
        Some(&pack),
        "edit.page_objects",
        &[&input],
        &objects,
        json!({"pages":"1"}),
    );
    let target_id = first_text_object_id(&objects);
    let output = dir.path().join("edited.pdf");
    let params = json!({"ops":[{"op":"replace_text", "page":1, "object_id":target_id, "text":"Tiếng Việt Đầy Đủ"}]});
    let result = tool(Some(&pack), "edit.apply", &[&input], &output, params);
    assert!(
        result["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "font_substituted")
    );
    assert!(
        lopdf::Document::load(&output)
            .unwrap()
            .extract_text(&[1])
            .unwrap()
            .contains("Tiếng Việt Đầy Đủ")
    );
    let readback = dir.path().join("readback.json");
    tool(
        Some(&pack),
        "edit.page_objects",
        &[&output],
        &readback,
        json!({"pages":"1"}),
    );
    let listed = read_json(&readback);
    assert!(
        listed["pages"][0]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["text"] == "Tiếng Việt Đầy Đủ")
    );
    let refused = dir.path().join("refused.pdf");
    let result = command(Some(&pack))
        .args(["tool", "edit.apply"])
        .arg(&input)
        .arg("-o")
        .arg(&refused)
        .arg("--param")
        .arg(format!(
            "ops={}",
            json!([{"op":"replace_text", "page":1, "object_id":target_id, "text":"\u{1F600}"}])
        ))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("glyph_unavailable"));
    assert!(!refused.exists());
    assert_eq!(fs::read(input).unwrap(), original);
}

fn signature_ranges(document: &lopdf::Document) -> Vec<(lopdf::ObjectId, Vec<usize>, Vec<u8>)> {
    document
        .objects
        .iter()
        .filter_map(|(id, object)| {
            let dict = object.as_dict().ok()?;
            let range = dict
                .get(b"ByteRange")
                .ok()?
                .as_array()
                .unwrap()
                .iter()
                .map(|v| usize::try_from(v.as_i64().unwrap()).unwrap())
                .collect();
            Some((
                *id,
                range,
                dict.get(b"Contents").unwrap().as_str().unwrap().to_vec(),
            ))
        })
        .collect()
}

fn signed_digest(bytes: &[u8], ranges: &[usize]) -> Vec<u8> {
    assert_eq!(ranges.len(), 4);
    assert_eq!(ranges[0], 0);
    assert!(ranges[1] > 0 && ranges[2] > ranges[1] && ranges[3] > 0);
    let mut digest = Sha256::new();
    for &[offset, length] in ranges.as_chunks::<2>().0 {
        digest.update(&bytes[offset..offset + length]);
    }
    digest.finalize().to_vec()
}

#[test]
fn incremental_edit_preserves_real_signatures_and_their_byte_range_digests() {
    let Some(pack) = PdfiumPack::new() else {
        return;
    };
    // Public government corpus document with two real detached CMS signatures.
    let input = root().join("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let original = fs::read(&input).unwrap();
    let signatures = signature_ranges(&lopdf::Document::load_mem(&original).unwrap());
    assert_eq!(signatures.len(), 2);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("signed-edited.pdf");
    tool(
        Some(&pack),
        "edit.apply",
        &[&input],
        &output,
        json!({"save":"incremental", "ops":[{"op":"add_text", "page":1, "text":"Tiếng Việt Đầy Đủ", "x_pt":30.0, "y_pt":30.0, "font_size":12.0}]}),
    );
    let edited = fs::read(&output).unwrap();
    assert!(edited.len() > original.len());
    assert_eq!(&edited[..original.len()], original.as_slice());
    let updated = lopdf::Document::load_mem(&edited).unwrap();
    let after = signature_ranges(&updated);
    let signature_summary = |entries: &[(lopdf::ObjectId, Vec<usize>, Vec<u8>)]| {
        entries
            .iter()
            .map(|(_, ranges, contents)| (ranges.clone(), hex::encode(Sha256::digest(contents))))
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        signature_summary(&signatures),
        signature_summary(&after),
        "signature ranges and CMS payloads survive PDF object renumbering"
    );
    for path in [&input, &output] {
        let fields = outcome(
            command(None)
                .args(["tool", "pdf.forms_list"])
                .arg(path)
                .arg("--json")
                .output()
                .unwrap(),
        );
        let signatures = fields["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|field| field["type"] == "signature")
            .collect::<Vec<_>>();
        assert_eq!(signatures.len(), 2);
        assert!(signatures.iter().all(|field| field["signed"] == true));
    }
    for (_, ranges, contents) in signatures {
        assert!(contents.starts_with(&[0x30, 0x82]), "CMS DER sequence");
        assert_eq!(
            signed_digest(&original, &ranges),
            signed_digest(&edited, &ranges)
        );
    }
    assert!(
        updated
            .extract_text(&[1])
            .unwrap()
            .contains("Tiếng Việt Đầy Đủ")
    );
}

/// Runs the real engine dispatcher in a child, blocked after the first applied operation.
/// This test-only sink makes the crash point deterministic without adding engine backdoors.
#[test]
fn crash_replay_child() {
    let Some(root) = std::env::var_os("ARIAD_EDIT_CLI_REPLAY_ROOT") else {
        return;
    };

    struct PauseAfterFirstEdit {
        root: PathBuf,
        buffered: Vec<u8>,
    }

    impl Write for PauseAfterFirstEdit {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.buffered.extend_from_slice(bytes);
            while let Some(end) = self.buffered.iter().position(|b| *b == b'\n') {
                let event: Value = serde_json::from_slice(&self.buffered[..end])?;
                self.buffered.drain(..=end);
                if event["type"] == "progress" && event["stage"] == "edit" && event["done"] == 1 {
                    let marker = self.root.join("interrupted.pending");
                    fs::write(&marker, serde_json::to_vec(&event)?)?;
                    fs::rename(marker, self.root.join("interrupted"))?;
                    loop {
                        std::thread::park();
                    }
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let root = PathBuf::from(root);
    let request = serde_json::from_slice(&fs::read(root.join("request.json")).unwrap()).unwrap();
    let lib = PathBuf::from(std::env::var_os("ARIAD_TEST_PDFIUM_DIR").unwrap());
    ariad_pdfium_engine::dispatch::handle_with(
        request,
        &mut PauseAfterFirstEdit {
            root,
            buffered: Vec::new(),
        },
        Some(&lib),
    )
    .unwrap();
    panic!("expected the parent to kill the editing process");
}

#[test]
fn replay_after_engine_kill_matches_uninterrupted_cli_bytes() {
    let Some(pack) = PdfiumPack::new() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("original.pdf");
    fs::copy(
        root().join("fixtures/pdf-tools/vi-diacritics-subset-font.pdf"),
        &input,
    )
    .unwrap();
    let original = fs::read(&input).unwrap();
    let objects = dir.path().join("objects.json");
    tool(
        Some(&pack),
        "edit.page_objects",
        &[&input],
        &objects,
        json!({"pages":"1"}),
    );
    let target_id = first_text_object_id(&objects);
    for save in ["full", "incremental"] {
        let params = json!({"save":save, "ops":[
            {"op":"replace_text", "page":1, "object_id":target_id, "text":"Tiếng Việt Đầy Đủ"},
            {"op":"move_object", "page":1, "object_id":target_id, "dx_pt":5.0, "dy_pt":-10.0},
            {"op":"set_text_style", "page":1, "object_id":target_id, "font_size":16.0, "color":"#AA0000"}
        ]});
        let expected = dir.path().join(format!("uninterrupted-{save}.pdf"));
        tool(
            Some(&pack),
            "edit.apply",
            &[&input],
            &expected,
            params.clone(),
        );
        let workspace = dir.path().join(format!("crash-{save}"));
        for subdirectory in ["in", "out", "tmp"] {
            fs::create_dir_all(workspace.join(subdirectory)).unwrap();
        }
        let saved = workspace.join("in/original.pdf");
        fs::write(&saved, &original).unwrap();
        let request = json!({"protocol":"ariad-engine/2", "op":"run_tool", "job":"crash-replay", "tool":"edit.apply", "inputs":[{"path":saved, "format":"pdf"}], "output":{"dir":workspace.join("out")}, "work_dir":workspace.join("tmp"), "params":params, "limits":ariad_core::limits::Limits::local()});
        fs::write(
            workspace.join("request.json"),
            serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        let mut child = EditingProcess(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash_replay_child", "--nocapture"])
                .env("ARIAD_EDIT_CLI_REPLAY_ROOT", &workspace)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        while !workspace.join("interrupted").exists() && Instant::now() < deadline {
            if child.0.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Always reap the child, including timeout and unexpected-exit failures.
        let interrupted = workspace.join("interrupted").exists();
        let _ = child.0.kill();
        child.0.wait().unwrap();
        assert!(interrupted, "engine did not reach the first edit operation");
        let progress = read_json(&workspace.join("interrupted"));
        assert_eq!(progress["done"], 1);
        assert_eq!(progress["total"], 3);
        assert_eq!(fs::read_dir(workspace.join("out")).unwrap().count(), 0);
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_eq!(fs::read(&saved).unwrap(), original);
        let replayed = dir.path().join(format!("replayed-{save}.pdf"));
        tool(Some(&pack), "edit.apply", &[&saved], &replayed, params);
        assert_eq!(
            fs::read(&replayed).unwrap(),
            fs::read(&expected).unwrap(),
            "{save}"
        );
    }
}
