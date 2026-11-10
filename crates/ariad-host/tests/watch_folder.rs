//! Real in-process recipe and cross-platform watcher acceptance tests.

use std::{fs, path::Path, time::Duration};

use ariad_core::{
    format::Format,
    tools::{Plan, Recipe, RecipeOrigin},
};
use ariad_host::{
    tools::{
        InputRef, ToolError,
        recipe::{RecipeRequest, run_recipe},
    },
    watch::{WatchRequest, run_watch_folder},
};
use lopdf::{Document, Object, Stream, dictionary};
use serde_json::json;
use tempfile::TempDir;

fn write_pdf(path: &Path, pages: u32, title: &str) {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
    );
    let mut kids = Vec::new();
    for page in 0..pages {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 10 20 Td (Page {page}) Tj ET").into_bytes(),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            "Resources" => dictionary! {"Font" => dictionary! {"F1" => font_id}}, "Contents" => content,
        });
        kids.push(Object::Reference(page_id));
    }
    doc.objects.insert(
        pages_id,
        dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => pages}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    let info = doc.add_object(dictionary! {"Title" => Object::string_literal(title)});
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    doc.save(path).unwrap();
}

fn recipe(steps: serde_json::Value) -> Recipe {
    Recipe::from_json_str(
        &json!({"version": "ariad-recipe/1", "steps": steps}).to_string(),
        RecipeOrigin::Trusted,
    )
    .unwrap()
}
fn rotations(path: &Path) -> Vec<i64> {
    let doc = Document::load(path).unwrap();
    doc.get_pages()
        .values()
        .map(|id| {
            doc.get_dictionary(*id)
                .unwrap()
                .get(b"Rotate")
                .and_then(Object::as_i64)
                .unwrap_or(0)
        })
        .collect()
}
fn watch_request(root: &Path, plan: Plan) -> WatchRequest {
    let watch = root.join("incoming");
    let output = root.join("output");
    fs::create_dir_all(&watch).unwrap();
    fs::create_dir_all(&output).unwrap();
    let request = RecipeRequest::new(
        recipe(json!([{"tool": "pdf.rotate", "params": {"angle":90}}])),
        vec![],
        output,
    )
    .with_plan(plan);
    let mut request = WatchRequest::new(watch, request);
    request.once = true;
    request.settle_ms = 1;
    request
}

#[tokio::test]
async fn pro_watch_processes_one_hundred_pdfs_and_skips_reintroduced_content() {
    let root = TempDir::new().unwrap();
    let request = watch_request(root.path(), Plan::Pro);
    for n in 0..100 {
        write_pdf(
            &request.watch_dir.join(format!("input-{n:03}.pdf")),
            1,
            &format!("Document {n}"),
        );
    }
    fs::write(request.watch_dir.join(".ignored.tmp"), b"partial").unwrap();
    fs::create_dir(request.watch_dir.join("ignored-directory")).unwrap();
    let first = run_watch_folder(&request).await.unwrap();
    assert_eq!(
        (first.processed, first.runs_executed, first.failed),
        (100, 1, 0)
    );
    assert_eq!(first.artifacts.len(), 100);
    for artifact in &first.artifacts {
        assert_eq!(rotations(&artifact.path), vec![90]);
    }
    let ledger: serde_json::Value = serde_json::from_slice(
        &fs::read(request.watch_dir.join(".ariad-watch-ledger.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(ledger.as_object().unwrap().len(), 100);
    for n in 0..100 {
        let name = format!("input-{n:03}.pdf");
        fs::copy(
            request.watch_dir.join(".done").join(&name),
            request.watch_dir.join(name),
        )
        .unwrap();
    }
    let second = run_watch_folder(&request).await.unwrap();
    assert_eq!(
        (
            second.processed,
            second.runs_executed,
            second.skipped_idempotent
        ),
        (0, 0, 100)
    );
    assert!(second.artifacts.is_empty());
    assert_eq!(
        fs::read_dir(request.recipe.output_dir).unwrap().count(),
        100
    );
    assert_eq!(
        fs::read_dir(request.watch_dir.join(".processing"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn free_watch_enforces_each_trigger_batch_before_claiming_inputs() {
    for (count, accepted) in [(25, true), (26, false)] {
        let root = TempDir::new().unwrap();
        let request = watch_request(root.path(), Plan::Free);
        for n in 0..count {
            write_pdf(
                &request.watch_dir.join(format!("input-{n}.pdf")),
                1,
                &format!("Document {n}"),
            );
        }
        let result = run_watch_folder(&request).await;
        if accepted {
            assert_eq!(result.unwrap().processed, count);
        } else {
            assert!(matches!(result, Err(ToolError::TierNotAllowed { .. })));
            assert!(request.watch_dir.join("input-0.pdf").is_file());
            assert_eq!(
                fs::read_dir(request.watch_dir.join(".processing"))
                    .unwrap()
                    .count(),
                0
            );
            assert_eq!(fs::read_dir(&request.recipe.output_dir).unwrap().count(), 0);
        }
    }
    let root = TempDir::new().unwrap();
    let request = watch_request(root.path(), Plan::Free);
    let large = fs::File::create(request.watch_dir.join("large.pdf")).unwrap();
    large
        .set_len(ariad_core::tools::FREE_MAX_RUN_BYTES + 1)
        .unwrap();
    assert!(matches!(
        run_watch_folder(&request).await,
        Err(ToolError::TierNotAllowed { .. })
    ));
}

#[test]
fn conditional_steps_inspect_each_current_item_and_pass_false_items_unchanged() {
    let root = TempDir::new().unwrap();
    let short = root.path().join("short.pdf");
    let long = root.path().join("long.pdf");
    write_pdf(&short, 1, "Short");
    write_pdf(&long, 2, "Long");
    let before = fs::read(&short).unwrap();
    let request = RecipeRequest::new(
        recipe(json!([{"tool":"pdf.rotate", "params":{"angle":90}, "if":"page_count > 1"}])),
        vec![InputRef::new(&short), InputRef::new(&long)],
        root.path().join("out"),
    );
    let result = run_recipe(&request).unwrap();
    assert_eq!(result.artifacts.len(), 2);
    assert_eq!(fs::read(&result.artifacts[0].path).unwrap(), before);
    assert_eq!(rotations(&result.artifacts[1].path), vec![90, 90]);
}

#[test]
fn tool_error_policies_drop_or_preserve_failed_items_and_stop_is_atomic() {
    for policy in ["skip", "continue", "stop"] {
        let root = TempDir::new().unwrap();
        let short = root.path().join("short.pdf");
        let long = root.path().join("long.pdf");
        write_pdf(&short, 1, "Short");
        write_pdf(&long, 2, "Long");
        let request = RecipeRequest::new(
            recipe(json!([
                {"tool":"pdf.rotate", "params":{"pages":"2", "angle":90}, "on_error":policy},
                {"tool":"pdf.rotate", "params":{"angle":90}}
            ])),
            vec![InputRef::new(&short), InputRef::new(&long)],
            root.path().join("out"),
        );
        let result = run_recipe(&request);
        if policy == "stop" {
            assert!(result.is_err());
            assert_eq!(fs::read_dir(&request.output_dir).unwrap().count(), 0);
        } else {
            let result = result.unwrap();
            assert_eq!(result.artifacts.len(), if policy == "skip" { 1 } else { 2 });
            assert!(result.warnings.iter().any(|w| w.code
                == if policy == "skip" {
                    "recipe_step_skipped"
                } else {
                    "recipe_step_continued"
                }));
            assert_eq!(
                rotations(&result.artifacts.last().unwrap().path),
                vec![90, 180]
            );
            if policy == "continue" {
                assert_eq!(rotations(&result.artifacts[0].path), vec![90]);
            }
        }
    }
}

#[test]
fn conversion_conditions_and_error_policies_apply_to_each_input() {
    for policy in ["skip", "continue", "stop"] {
        let root = TempDir::new().unwrap();
        let good = root.path().join("good.json");
        let bad = root.path().join("bad.json");
        fs::write(
            &good,
            serde_json::to_vec(&ariad_core::ir::Document::default()).unwrap(),
        )
        .unwrap();
        fs::write(&bad, [0xff, 0xfe, 0xfd]).unwrap();
        let request = RecipeRequest::new(
            recipe(json!([{"convert":{"to":"html"}, "on_error":policy, "if":"format == 'json'"}])),
            vec![InputRef::new(&good), InputRef::new(&bad)],
            root.path().join("out"),
        );
        let result = run_recipe(&request);
        if policy == "stop" {
            assert!(result.is_err());
            assert_eq!(fs::read_dir(&request.output_dir).unwrap().count(), 0);
        } else {
            let result = result.unwrap();
            assert_eq!(result.artifacts.len(), if policy == "skip" { 1 } else { 2 });
            assert_eq!(result.artifacts[0].format, Format::Html.id());
            if policy == "continue" {
                assert_eq!(
                    fs::read(&result.artifacts[1].path).unwrap(),
                    [0xff, 0xfe, 0xfd]
                );
            }
        }
    }
}

#[test]
fn split_fan_out_keeps_part_stems_and_order_for_single_and_many_input_steps() {
    let root = TempDir::new().unwrap();
    let input = root.path().join("source.pdf");
    write_pdf(&input, 3, "Parts");
    let steps = json!([
        {"tool":"pdf.split", "params":{"mode":"every", "every":1}},
        {"tool":"pdf.rotate", "params":{"angle":90}}
    ]);
    let mut request = RecipeRequest::new(
        recipe(steps),
        vec![InputRef::new(&input)],
        root.path().join("parts"),
    );
    request.recipe.naming = Some("{stem}-{n}.pdf".into());
    let parts = run_recipe(&request).unwrap();
    assert_eq!(parts.artifacts.len(), 3);
    for (idx, artifact) in parts.artifacts.iter().enumerate() {
        assert_eq!(rotations(&artifact.path), vec![90]);
        assert!(
            artifact
                .path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .contains(&format!("{:03}", idx + 1))
        );
    }
    request
        .recipe
        .steps
        .push(serde_json::from_value(json!({"tool":"pdf.merge", "params":{}})).unwrap());
    request.output_dir = root.path().join("merged");
    let merged = run_recipe(&request).unwrap();
    assert_eq!(merged.artifacts.len(), 1);
    assert_eq!(rotations(&merged.artifacts[0].path), vec![90, 90, 90]);
    let doc = Document::load(&merged.artifacts[0].path).unwrap();
    for (idx, id) in doc.get_pages().keys().enumerate() {
        assert!(
            doc.extract_text(&[*id])
                .unwrap()
                .contains(&format!("Page {idx}"))
        );
    }
}

#[test]
fn title_template_sanitizes_metadata_and_falls_back_to_stem() {
    let root = TempDir::new().unwrap();
    for (index, title, expected) in [
        (0, "  Quarterly  Report / 2026  ", "Quarterly Report 2026"),
        (1, "", "document-1"),
        (2, "///", "document-2"),
    ] {
        let input = root.path().join(format!("document-{index}.pdf"));
        write_pdf(&input, 1, title);
        let mut request = RecipeRequest::new(
            recipe(json!([{"tool":"pdf.rotate", "params":{"angle":90}}])),
            vec![InputRef::new(&input)],
            root.path().join(format!("out-{index}")),
        );
        request.recipe.naming = Some("{title}-{n}.pdf".into());
        let outcome = run_recipe(&request).unwrap();
        assert_eq!(
            outcome.artifacts[0]
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            format!("{expected}-001.pdf")
        );
    }
}

#[tokio::test]
async fn watcher_requires_import_confirmation_and_records_failures_without_ledger_entry() {
    let root = TempDir::new().unwrap();
    let mut request = watch_request(root.path(), Plan::Free);
    request.recipe.origin = RecipeOrigin::Imported;
    fs::write(request.watch_dir.join("bad.pdf"), b"not a PDF").unwrap();
    assert!(matches!(
        run_watch_folder(&request).await,
        Err(ToolError::InvalidParams(_))
    ));
    assert!(request.watch_dir.join("bad.pdf").is_file());
    request.recipe.confirmed = true;
    let result = run_watch_folder(&request).await.unwrap();
    assert_eq!((result.processed, result.failed), (0, 1));
    assert!(request.watch_dir.join(".failed/bad.pdf").is_file());
    let report: serde_json::Value = serde_json::from_slice(
        &fs::read(request.watch_dir.join(".failed/bad.pdf.error.json")).unwrap(),
    )
    .unwrap();
    assert!(report["code"].is_string());
    assert!(!request.watch_dir.join(".ariad-watch-ledger.json").exists());
}

#[tokio::test]
async fn continuous_watch_settles_changes_and_stops_on_cancellation() {
    let root = TempDir::new().unwrap();
    let mut request = watch_request(root.path(), Plan::Free);
    request.once = false;
    request.settle_ms = 5;
    let path = request.watch_dir.join("later.pdf");
    let cancel = request.recipe.cancel.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(15));
        write_pdf(&path, 1, "Later");
        std::thread::sleep(Duration::from_millis(250));
        cancel.cancel();
    });
    let outcome = run_watch_folder(&request).await.unwrap();
    worker.join().unwrap();
    assert_eq!(outcome.processed, 1);
    assert_eq!(outcome.failed, 0);
}

#[test]
fn skipping_every_item_publishes_nothing() {
    let root = TempDir::new().unwrap();
    let input = root.path().join("short.pdf");
    write_pdf(&input, 1, "Short");
    let request = RecipeRequest::new(
        recipe(json!([
            {"tool":"pdf.rotate", "params":{"pages":"2", "angle":90}, "on_error":"skip"},
            {"tool":"pdf.rotate", "params":{"angle":90}}
        ])),
        vec![InputRef::new(&input)],
        root.path().join("out"),
    );
    let outcome = run_recipe(&request).unwrap();
    assert!(outcome.artifacts.is_empty());
    assert!(outcome.steps.iter().all(|step| step.artifacts == 0));
    assert_eq!(fs::read_dir(&request.output_dir).unwrap().count(), 0);
}

#[tokio::test]
async fn watch_ledger_distinguishes_recipe_hash_and_regenerates_missing_artifacts() {
    let root = TempDir::new().unwrap();
    let mut request = watch_request(root.path(), Plan::Free);
    request.recipe.overwrite = true;
    let input = request.watch_dir.join("document.pdf");
    write_pdf(&input, 1, "Title");
    let original = fs::read(&input).unwrap();
    let first = run_watch_folder(&request).await.unwrap();
    fs::remove_file(&first.artifacts[0].path).unwrap();
    fs::write(&input, &original).unwrap();
    assert_eq!(run_watch_folder(&request).await.unwrap().processed, 1);
    request.recipe.recipe.steps =
        recipe(json!([{"tool":"pdf.rotate", "params":{"angle":180}}])).steps;
    fs::write(&input, &original).unwrap();
    let changed = run_watch_folder(&request).await.unwrap();
    assert_eq!(changed.processed, 1);
    assert_eq!(rotations(&changed.artifacts[0].path), vec![180]);
    let ledger: serde_json::Value = serde_json::from_slice(
        &fs::read(request.watch_dir.join(".ariad-watch-ledger.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(ledger.as_object().unwrap().len(), 2);
}

#[tokio::test]
async fn watch_respects_confinement_for_claimed_inputs_and_rejects_external_directories() {
    use ariad_host::confine::AllowedDirs;
    let root = TempDir::new().unwrap();
    let mut request = watch_request(root.path(), Plan::Free);
    let input = request.watch_dir.join("document.pdf");
    write_pdf(&input, 1, "Confined");
    request.recipe.allowed_dirs =
        Some(AllowedDirs::new(&[root.path().to_path_buf()], &[]).unwrap());
    let result = run_watch_folder(&request).await.unwrap();
    assert_eq!((result.processed, result.failed), (1, 0));
    let external = TempDir::new().unwrap();
    request.recipe.allowed_dirs =
        Some(AllowedDirs::new(&[external.path().to_path_buf()], &[]).unwrap());
    assert!(matches!(
        run_watch_folder(&request).await,
        Err(ToolError::Confinement(_))
    ));
}

#[tokio::test]
async fn once_watch_defers_files_changed_during_the_stability_window() {
    let root = TempDir::new().unwrap();
    let mut request = watch_request(root.path(), Plan::Free);
    request.settle_ms = 50;
    let input = request.watch_dir.join("changing.pdf");
    fs::write(&input, b"partial").unwrap();
    let watch = run_watch_folder(&request);
    tokio::pin!(watch);
    // Poll through the first snapshot before changing the file. A writer thread
    // with a fixed sleep can finish before that snapshot under filesystem load.
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(watch.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    write_pdf(&input, 1, "Settled");
    let first = watch.await.unwrap();
    assert_eq!(first.runs_executed, 0);
    assert!(request.watch_dir.join("changing.pdf").is_file());
    assert_eq!(run_watch_folder(&request).await.unwrap().processed, 1);
}

#[cfg(unix)]
#[tokio::test]
async fn watcher_ignores_symlink_inputs_and_rejects_symlink_staging() {
    let root = TempDir::new().unwrap();
    let request = watch_request(root.path(), Plan::Free);
    let target = root.path().join("external.pdf");
    write_pdf(&target, 1, "External");
    std::os::unix::fs::symlink(&target, request.watch_dir.join("link.pdf")).unwrap();
    assert_eq!(run_watch_folder(&request).await.unwrap().runs_executed, 0);
    fs::remove_dir(request.watch_dir.join(".processing")).unwrap();
    std::os::unix::fs::symlink(root.path(), request.watch_dir.join(".processing")).unwrap();
    assert!(matches!(
        run_watch_folder(&request).await,
        Err(ToolError::Confinement(_))
    ));
}

#[test]
fn title_naming_reads_metadata_after_the_final_step() {
    let root = TempDir::new().unwrap();
    let input = root.path().join("document.pdf");
    write_pdf(&input, 1, "Old title");
    let mut request = RecipeRequest::new(
        recipe(json!([{"tool":"pdf.metadata", "params":{"set":{"title":"New title"}}}])),
        vec![InputRef::new(&input)],
        root.path().join("out"),
    );
    request.recipe.naming = Some("{title}.pdf".into());
    let outcome = run_recipe(&request).unwrap();
    assert_eq!(
        outcome.artifacts[0].path.file_name().unwrap(),
        "New title.pdf"
    );
}

#[tokio::test]
async fn watcher_recovers_unfinished_and_completed_claims() {
    let root = TempDir::new().unwrap();
    let request = watch_request(root.path(), Plan::Free);
    let processing = request.watch_dir.join(".processing");
    fs::create_dir(&processing).unwrap();
    write_pdf(&processing.join("unfinished.pdf"), 1, "Unfinished");
    let first = run_watch_folder(&request).await.unwrap();
    assert_eq!(first.processed, 1);
    // Simulate termination after ledger commit but before input archival.
    fs::rename(
        request.watch_dir.join(".done/unfinished.pdf"),
        processing.join("unfinished.pdf"),
    )
    .unwrap();
    // A newly arrived same-name document must also survive recovery.
    write_pdf(&request.watch_dir.join("unfinished.pdf"), 1, "New content");
    let mut request = request;
    request.recipe.overwrite = true;
    let second = run_watch_folder(&request).await.unwrap();
    assert_eq!((second.processed, second.skipped_idempotent), (1, 1));
    assert_eq!(fs::read_dir(processing).unwrap().count(), 0);
}

#[tokio::test]
async fn watcher_refuses_concurrent_ownership_and_releases_the_os_lock() {
    let root = TempDir::new().unwrap();
    let request = watch_request(root.path(), Plan::Free);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(request.watch_dir.join(".ariad-watch-lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(matches!(
        run_watch_folder(&request).await,
        Err(ToolError::Usage(_))
    ));
    drop(lock);
    assert_eq!(run_watch_folder(&request).await.unwrap().processed, 0);
}

#[tokio::test]
async fn missing_output_does_not_invalidate_other_inputs_in_the_batch() {
    let root = TempDir::new().unwrap();
    let request = watch_request(root.path(), Plan::Free);
    for name in ["first.pdf", "second.pdf"] {
        write_pdf(&request.watch_dir.join(name), 1, name);
    }
    let first = run_watch_folder(&request).await.unwrap();
    assert_eq!((first.processed, first.runs_executed), (2, 1));
    let first_output = first
        .artifacts
        .iter()
        .find(|a| {
            a.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("first.")
        })
        .unwrap();
    fs::remove_file(&first_output.path).unwrap();
    fs::copy(
        request.watch_dir.join(".done/second.pdf"),
        request.watch_dir.join("second.pdf"),
    )
    .unwrap();
    let duplicate = run_watch_folder(&request).await.unwrap();
    assert_eq!(
        (
            duplicate.skipped_idempotent,
            duplicate.runs_executed,
            duplicate.failed
        ),
        (1, 0, 0)
    );
    fs::copy(
        request.watch_dir.join(".done/first.pdf"),
        request.watch_dir.join("first.pdf"),
    )
    .unwrap();
    let restored = run_watch_folder(&request).await.unwrap();
    assert_eq!((restored.processed, restored.runs_executed), (1, 1));
}

#[tokio::test]
async fn changing_output_directory_regenerates_outputs_in_the_new_destination() {
    let root = TempDir::new().unwrap();
    let mut request = watch_request(root.path(), Plan::Free);
    let input = request.watch_dir.join("source.pdf");
    write_pdf(&input, 1, "Source");
    let first = run_watch_folder(&request).await.unwrap();
    assert_eq!(first.processed, 1);
    fs::copy(request.watch_dir.join(".done/source.pdf"), &input).unwrap();
    request.recipe.output_dir = root.path().join("different-output");
    let second = run_watch_folder(&request).await.unwrap();
    assert_eq!(
        (
            second.processed,
            second.runs_executed,
            second.skipped_idempotent
        ),
        (1, 1, 0)
    );
    assert!(
        second.artifacts[0]
            .path
            .starts_with(&request.recipe.output_dir)
    );
    assert!(first.artifacts[0].path.is_file());
}

#[test]
fn false_condition_skips_a_tool_that_takes_no_inputs() {
    let root = TempDir::new().unwrap();
    let input = root.path().join("source.pdf");
    write_pdf(&input, 1, "Source");
    let request = RecipeRequest::new(
        recipe(json!([{
            "tool":"ai.recipe_from_text", "params":{"prompt":"Rotate all pages by 90 degrees"},
            "if":"page_count > 1"
        }])),
        vec![InputRef::new(&input)],
        root.path().join("output"),
    );
    let outcome = run_recipe(&request).unwrap();
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(outcome.artifacts[0].format, "pdf");
    assert_eq!(
        fs::read(&outcome.artifacts[0].path).unwrap(),
        fs::read(input).unwrap()
    );
}
