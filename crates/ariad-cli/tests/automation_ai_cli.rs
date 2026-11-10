//! End-to-end automation, offline packs and consent-gated host cloud execution.
use lopdf::{Document, Object, Stream, dictionary};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
    thread,
};
use tempfile::TempDir;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn command() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ashift"));
    for name in [
        "ASHIFT_PLAN",
        "ASHIFT_AI_CLOUD_URL",
        "ASHIFT_TEST_ALLOW_LOOPBACK_AI",
        "ASHIFT_PACK_AI",
        "ASHIFT_PACK_INPAINT",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("ASHIFT_PANDOC", root().join(".tools/pandoc/bin/pandoc"));
    cmd
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn failure(output: Output, code: i32, message: &str) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(message));
}
fn pdf(path: &Path, count: u32, title: &str) {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let mut kids = Vec::new();
    for index in 1..=count {
        let text = if index == 1 {
            "Project revenue is 42 dollars."
        } else {
            "The launch date is November 10."
        };
        let stream = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 20 160 Td ({text}) Tj ET").into_bytes(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! {"F1" => font},
            },
            "Contents" => stream,
        });
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>count}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    let info = doc.add_object(dictionary! {"Title"=>Object::string_literal(title)});
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    doc.save(path).unwrap();
}
fn recipe(path: &Path, steps: Value, naming: Option<&str>) {
    let mut value = json!({"version":"ariad-recipe/1","steps":steps});
    if let Some(naming) = naming {
        value["naming"] = json!(naming);
    }
    fs::write(path, value.to_string()).unwrap();
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

#[test]
fn watch_batches_one_hundred_pdfs_and_skips_reintroduced_content() {
    let tmp = TempDir::new().unwrap();
    let incoming = tmp.path().join("incoming");
    fs::create_dir(&incoming).unwrap();
    let out = tmp.path().join("out");
    let spec = tmp.path().join("recipe.json");
    recipe(
        &spec,
        json!([{"tool":"pdf.rotate","params":{"angle":90}}]),
        None,
    );
    for n in 0..100 {
        pdf(
            &incoming.join(format!("input-{n:03}.pdf")),
            1,
            &format!("Document {n}"),
        );
    }
    let run = || {
        success(
            command()
                .env("ASHIFT_PLAN", "pro")
                .args(["recipe", "run"])
                .arg(&spec)
                .arg("--watch")
                .arg(&incoming)
                .args([
                    "--watch-once",
                    "--settle-ms",
                    "1",
                    "--confirmed",
                    "--json",
                    "-o",
                ])
                .arg(&out)
                .output()
                .unwrap(),
        )
    };
    let first = run();
    assert_eq!(first["processed"], 100);
    assert_eq!(first["runs_executed"], 1);
    assert_eq!(first["failed"], 0);
    assert_eq!(first["artifacts"].as_array().unwrap().len(), 100);
    for artifact in first["artifacts"].as_array().unwrap() {
        assert_eq!(
            rotations(Path::new(artifact["path"].as_str().unwrap())),
            vec![90]
        );
    }
    for n in 0..100 {
        let name = format!("input-{n:03}.pdf");
        fs::copy(incoming.join(".done").join(&name), incoming.join(&name)).unwrap();
    }
    let second = run();
    assert_eq!(second["processed"], 0);
    assert_eq!(second["runs_executed"], 0);
    assert_eq!(second["skipped_idempotent"], 100);
    failure(
        command()
            .args(["recipe", "run"])
            .arg(&spec)
            .arg(incoming.join(".done/input-000.pdf"))
            .arg("--watch")
            .arg(&incoming)
            .output()
            .unwrap(),
        2,
        "cannot be used",
    );
}

#[test]
fn conditional_error_policies_and_title_naming_work_through_cli() {
    let tmp = TempDir::new().unwrap();
    let spec = tmp.path().join("recipe.json");
    let input = tmp.path().join("input.pdf");
    pdf(&input, 2, "Quarterly report");
    recipe(
        &spec,
        json!([{"tool":"pdf.rotate","params":{"angle":90},"if":"page_count > 1"}]),
        Some("{title}-{n}.pdf"),
    );
    let out = tmp.path().join("conditional");
    let result = success(
        command()
            .args(["recipe", "run"])
            .arg(&spec)
            .arg(&input)
            .args(["--yes", "--json", "-o"])
            .arg(&out)
            .output()
            .unwrap(),
    );
    let path = Path::new(result["artifacts"][0]["path"].as_str().unwrap());
    assert_eq!(rotations(path), vec![90, 90]);
    assert!(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Quarterly report-")
    );
    for policy in ["skip", "continue"] {
        recipe(
            &spec,
            json!([{"tool":"pdf.extract_pages","params":{"pages":"999"},"on_error":policy}]),
            None,
        );
        let result = success(
            command()
                .args(["recipe", "run"])
                .arg(&spec)
                .arg(&input)
                .args(["--yes", "--json", "-o"])
                .arg(tmp.path().join(policy))
                .output()
                .unwrap(),
        );
        assert_eq!(
            result["artifacts"].as_array().unwrap().len(),
            usize::from(policy == "continue")
        );
        assert!(
            result["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["code"]
                    == format!(
                        "recipe_step_{}",
                        if policy == "skip" {
                            "skipped"
                        } else {
                            "continued"
                        }
                    ))
        );
    }
}

fn packs() -> &'static TempDir {
    static PACKS: OnceLock<TempDir> = OnceLock::new();
    PACKS.get_or_init(|| {
        let tmp = TempDir::new().unwrap();
        for pack in ["ai", "inpaint"] {
            let output = Command::new("uv")
                .current_dir(root())
                .args([
                    "run",
                    "--no-project",
                    "--python",
                    "3.14",
                    "--with",
                    "pypdf==7.0.0",
                    "python3",
                ])
                .arg(format!("engines/{pack}/pack/build.py"))
                .args(["build", "--out"])
                .arg(tmp.path().join(pack))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "pack {pack}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        tmp
    })
}
fn run_tool(tool: &str, input: Option<&Path>, params: &Value, out: &Path) -> Value {
    let mut cmd = command();
    cmd.env("ASHIFT_PACKS_DIR", packs().path())
        .env("ASHIFT_PACK_AI", packs().path().join("ai"))
        .env("ASHIFT_PACK_INPAINT", packs().path().join("inpaint"));
    cmd.args(["tool", tool]);
    if let Some(input) = input {
        cmd.arg(input);
    }
    let params_path = out.parent().unwrap().join(format!(
        "{}.params.json",
        out.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&params_path, params.to_string()).unwrap();
    cmd.arg("--params-json")
        .arg(params_path)
        .args(["--json", "-o"])
        .arg(out);
    success(cmd.output().unwrap())
}
fn artifact(result: &Value) -> PathBuf {
    PathBuf::from(result["artifacts"][0]["path"].as_str().unwrap())
}

#[test]
fn offline_ai_pack_formats_citations_translation_tables_and_recipe_confirmation() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("input.pdf");
    pdf(&input, 2, "Original source");
    let summary = run_tool(
        "ai.summarize",
        Some(&input),
        &json!({}),
        &tmp.path().join("summary.json"),
    );
    assert!(
        summary["inline_json"]["summary"]
            .as_str()
            .unwrap()
            .contains("42 dollars")
    );
    let md = tmp.path().join("summary.md");
    run_tool("ai.summarize", Some(&input), &json!({"format":"md"}), &md);
    assert!(fs::read_to_string(md).unwrap().contains("42 dollars"));
    let answer = run_tool(
        "ai.ask",
        Some(&input),
        &json!({"question":"What is the launch date?"}),
        &tmp.path().join("answer.json"),
    );
    let citations = answer["inline_json"]["citations"].as_array().unwrap();
    assert!(!citations.is_empty());
    assert_eq!(citations[0]["page"], 2);
    assert_eq!(citations[0]["quote"], "The launch date is November 10.");
    let source = ariad_core::pipeline::read(
        ariad_core::format::Format::Markdown,
        b"# Hello\n\nThe document is important.",
        &Default::default(),
    )
    .unwrap()
    .document;
    let ir = tmp.path().join("source.json");
    fs::write(&ir, serde_json::to_vec(&source).unwrap()).unwrap();
    let result = run_tool(
        "ai.translate",
        Some(&ir),
        &json!({"source_lang":"en","target_lang":"vi","format":"json"}),
        &tmp.path().join("translated.json"),
    );
    let translated: Value = serde_json::from_slice(&fs::read(artifact(&result)).unwrap()).unwrap();
    let source_value = serde_json::to_value(&source).unwrap();
    assert_eq!(translated["version"], "ariad-ir/2");
    assert_eq!(translated["body"][0]["id"], source_value["body"][0]["id"]);
    assert_ne!(translated["body"], source_value["body"]);
    let table = tmp.path().join("table.md");
    fs::write(&table, "| Item | Count |\n| --- | --- |\n| Apples | 42 |\n").unwrap();
    for format in ["csv", "json", "xlsx"] {
        let result = run_tool(
            "ai.extract_tables",
            Some(&table),
            &json!({"format":format}),
            &tmp.path().join(format!("table.{format}")),
        );
        let bytes = fs::read(artifact(&result)).unwrap();
        if format == "xlsx" {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut text = String::new();
            zip.by_name("xl/worksheets/sheet1.xml")
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            assert!(text.contains("Apples"));
        } else {
            assert!(String::from_utf8(bytes).unwrap().contains("Apples"));
            if format == "json" {
                assert!(result["inline_json"].is_object());
            }
        }
    }
    let result = run_tool(
        "ai.recipe_from_text",
        None,
        &json!({"prompt":"Rotate all pages by 90 degrees"}),
        &tmp.path().join("generated.json"),
    );
    assert_eq!(result["inline_json"]["origin"], "imported");
    assert_eq!(result["inline_json"]["requires_confirmation"], true);
    failure(
        command()
            .args(["recipe", "run"])
            .arg(artifact(&result))
            .arg(&input)
            .args(["--origin", "imported", "-o"])
            .arg(tmp.path().join("refused"))
            .output()
            .unwrap(),
        9,
        "confirmation",
    );
}

fn raster_pdf(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let mut pixels = vec![255_u8; 64 * 64 * 3];
    for y in 25..36 {
        for x in 20..44 {
            pixels[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3].fill(180);
        }
    }
    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 64,
            "Height" => 64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        pixels,
    ));
    let contents = doc.add_object(Stream::new(
        dictionary! {},
        b"q 64 0 0 64 0 0 cm /Im0 Do Q".to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 64.into(), 64.into()],
        "Resources" => dictionary! {
            "XObject" => dictionary! {"Im0" => image},
        },
        "Contents" => contents,
    });
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}
#[test]
fn inpaint_pack_publishes_descriptive_previews_and_supports_single_pdf_target() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("raster.pdf");
    raster_pdf(&input);
    let out = tmp.path().join("previews");
    fs::create_dir(&out).unwrap();
    let result = run_tool(
        "pdf.watermark_remove_raster",
        Some(&input),
        &json!({}),
        &out,
    );
    let artifacts = result["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 3);
    for suffix in [
        "before_page_001.png",
        "after_page_001.png",
        "watermark_remove_raster.pdf",
    ] {
        assert!(
            artifacts
                .iter()
                .any(|a| a["path"].as_str().unwrap().ends_with(suffix))
        );
    }
    let before = fs::read(out.join("raster.before_page_001.png")).unwrap();
    let after = fs::read(out.join("raster.after_page_001.png")).unwrap();
    assert_ne!(before, after);
    let result = run_tool(
        "pdf.watermark_remove_raster",
        Some(&input),
        &json!({}),
        &tmp.path().join("cleaned.pdf"),
    );
    assert_eq!(result["artifacts"].as_array().unwrap().len(), 1);
}

fn mock_cloud(content: Value) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(15)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let n = socket.read(&mut buffer).unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let body = if request.starts_with(b"POST /messages ") {
            json!({"content":[{"type":"text","text":content.to_string()}]})
        } else {
            json!({"choices":[{"message":{"content":content.to_string()}}]})
        }
        .to_string();
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        String::from_utf8(request).unwrap()
    });
    (format!("http://{address}/chat/completions"), handle)
}
#[test]
fn cloud_consent_and_tier_precede_input_reads_and_do_not_need_local_pack() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("missing.pdf");
    for (consent, code, message) in [
        (false, 9, "explicit consent"),
        (true, 11, "tier_not_allowed"),
    ] {
        failure(
            command()
                .args(["tool", "ai.summarize"])
                .arg(&missing)
                .args(["--param", "provider=cloud", "--param"])
                .arg(format!("cloud_consent={consent}"))
                .output()
                .unwrap(),
            code,
            message,
        );
    }
    let input = tmp.path().join("source.md");
    fs::write(&input, "Original cloud source.").unwrap();
    for byok in [false, true] {
        let key = "ephemeral-cloud-test-value";
        let keyfile = tmp.path().join("key");
        fs::write(&keyfile, key).unwrap();
        let (url, server) = mock_cloud(
            json!({"summary":format!("Original cloud source. {key}"),"bullet_points":[],"language":"en"}),
        );
        let mut cmd = command();
        cmd.env("ASHIFT_AI_CLOUD_URL", url)
            .env("ASHIFT_TEST_ALLOW_LOOPBACK_AI", "1");
        cmd.args(["tool", "ai.summarize"])
            .arg(&input)
            .args([
                "--param",
                "provider=cloud",
                "--param",
                "cloud_consent=true",
                "--json",
                "-o",
            ])
            .arg(tmp.path().join(format!("cloud-{byok}.json")));
        if byok {
            cmd.arg("--secret")
                .arg(format!("api_key=@{}", keyfile.display()));
        } else {
            cmd.env("ASHIFT_PLAN", "pro");
        }
        let output = cmd.output().unwrap();
        if byok {
            assert!(!String::from_utf8_lossy(&output.stdout).contains(key));
            assert!(!String::from_utf8_lossy(&output.stderr).contains(key));
        }
        let result = success(output);
        assert!(
            result["inline_json"]["summary"]
                .as_str()
                .unwrap()
                .contains("Original cloud source")
        );
        let received = server.join().unwrap();
        assert!(received.contains("Original cloud source"));
        if byok {
            assert!(received.contains(&format!("Bearer {key}")));
            assert!(!fs::read_to_string(artifact(&result)).unwrap().contains(key));
        }
    }
}

fn cloud_command(
    tool: &str,
    input: Option<&Path>,
    params: Value,
    output: &Path,
    response: Value,
) -> (Command, thread::JoinHandle<String>) {
    let (url, server) = mock_cloud(response);
    let params_file = output.with_extension("params.json");
    let mut params = params;
    params["provider"] = json!("cloud");
    params["cloud_consent"] = json!(true);
    fs::write(&params_file, params.to_string()).unwrap();
    let mut cmd = command();
    cmd.env("ASHIFT_PLAN", "pro")
        .env("ASHIFT_AI_CLOUD_URL", url)
        .env("ASHIFT_TEST_ALLOW_LOOPBACK_AI", "1")
        .args(["tool", tool]);
    if let Some(input) = input {
        cmd.arg(input);
    }
    cmd.arg("--params-json")
        .arg(params_file)
        .args(["--json", "-o"])
        .arg(output);
    (cmd, server)
}

#[test]
fn cloud_validates_citations_and_translation_structure_before_publication() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("source.pdf");
    pdf(&input, 2, "Cloud citations");
    for page in [1, 2] {
        let out = tmp.path().join(format!("answer-{page}.json"));
        let (mut cmd, server) = cloud_command(
            "ai.ask",
            Some(&input),
            json!({"question":"When is the launch?"}),
            &out,
            json!({"answer":"November 10", "citations":[{"page":page,"quote":"The launch date is November 10.","score":1.0}]}),
        );
        let result = cmd.output().unwrap();
        if page == 1 {
            failure(result, 1, "tool contract");
            assert!(!out.exists());
        } else {
            assert_eq!(success(result)["inline_json"]["citations"][0]["page"], 2);
        }
        server.join().unwrap();
    }
    let document = ariad_core::pipeline::read(
        ariad_core::format::Format::Markdown,
        b"# Hello\n\nImportant document.",
        &Default::default(),
    )
    .unwrap()
    .document;
    let source = serde_json::to_value(&document).unwrap();
    let ir = tmp.path().join("source.json");
    fs::write(&ir, source.to_string()).unwrap();
    for changed_structure in [false, true] {
        let mut translated = source.clone();
        translated["body"][0]["content"][0]["text"] = json!("Xin chào");
        translated["meta"]["language"] = json!("vi");
        if changed_structure {
            translated["body"][0]["level"] = json!(2);
        }
        let output = tmp
            .path()
            .join(format!("translated-{changed_structure}.json"));
        let (mut cmd, server) = cloud_command(
            "ai.translate",
            Some(&ir),
            json!({"target_lang":"vi","format":"json"}),
            &output,
            translated,
        );
        let result = cmd.output().unwrap();
        if changed_structure {
            failure(result, 1, "changed document structure");
            assert!(!output.exists());
        } else {
            let result = success(result);
            assert_eq!(
                result["inline_json"]["body"][0]["content"][0]["text"],
                "Xin chào"
            );
        }
        server.join().unwrap();
    }
}

#[test]
fn cloud_exports_tables_and_validates_imported_recipe_drafts() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("source.md");
    fs::write(&input, "| Item | Count |\n| --- | --- |\n| Apples | 42 |\n").unwrap();
    for format in ["csv", "xlsx", "json"] {
        let output = tmp.path().join(format!("cloud-table.{format}"));
        let (mut cmd, server) = cloud_command(
            "ai.extract_tables",
            Some(&input),
            json!({"format":format}),
            &output,
            json!({"tables":[{"page":1,"rows":[["Item","Count"],["Apples","42"]]}]}),
        );
        let result = success(cmd.output().unwrap());
        let bytes = fs::read(artifact(&result)).unwrap();
        assert!(!bytes.is_empty());
        server.join().unwrap();
    }
    let output = tmp.path().join("draft.json");
    let (mut cmd, server) = cloud_command(
        "ai.recipe_from_text",
        None,
        json!({"prompt":"rotate all pages by 90 degrees"}),
        &output,
        json!({"version":"ariad-recipe/1","steps":[{"tool":"pdf.rotate","params":{"angle":90}}]}),
    );
    let result = success(cmd.output().unwrap());
    assert_eq!(result["inline_json"]["requires_confirmation"], true);
    let draft: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(draft["version"], "ariad-recipe/1");
    server.join().unwrap();
    let output = tmp.path().join("invalid.json");
    let (mut cmd, server) = cloud_command(
        "ai.recipe_from_text",
        None,
        json!({"prompt":"convert the document"}),
        &output,
        json!({"version":"ariad-recipe/1","steps":[{"tool":"pdf.unknown","params":{}}]}),
    );
    failure(cmd.output().unwrap(), 1, "tool contract");
    assert!(!output.exists());
    server.join().unwrap();
}

#[test]
fn cloud_refuses_http_and_private_endpoints_without_test_loopback_opt_in() {
    let tmp = TempDir::new().unwrap();
    for url in [
        "http://127.0.0.1:12345/chat/completions",
        "https://169.254.169.254/latest/meta-data",
    ] {
        let result = command()
            .env("ASHIFT_PLAN", "pro")
            .env("ASHIFT_AI_CLOUD_URL", url)
            .args(["tool", "ai.summarize"])
            .arg(tmp.path().join("missing.pdf"))
            .args(["--param", "provider=cloud", "--param", "cloud_consent=true"])
            .output()
            .unwrap();
        failure(result, 9, "public HTTPS endpoint");
    }
}

#[test]
fn anthropic_message_endpoint_uses_provider_payload_and_ephemeral_headers() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("source.md");
    fs::write(&input, "Source text.").unwrap();
    let key = "test-message-key";
    let keyfile = tmp.path().join("key");
    fs::write(&keyfile, key).unwrap();
    let (url, server) =
        mock_cloud(json!({"summary":"Message summary", "bullet_points":[], "language":"en"}));
    let result = success(
        command()
            .env(
                "ASHIFT_AI_CLOUD_URL",
                url.replace("/chat/completions", "/messages"),
            )
            .env("ASHIFT_TEST_ALLOW_LOOPBACK_AI", "1")
            .args(["tool", "ai.summarize"])
            .arg(&input)
            .args([
                "--param",
                "provider=cloud",
                "--param",
                "cloud_consent=true",
                "--param",
                "model=configured-model",
                "--secret",
            ])
            .arg(format!("api_key=@{}", keyfile.display()))
            .args(["--json", "-o"])
            .arg(tmp.path().join("summary.json"))
            .output()
            .unwrap(),
    );
    assert_eq!(result["inline_json"]["summary"], "Message summary");
    let received = server.join().unwrap();
    assert!(received.contains(&format!("x-api-key: {key}")));
    assert!(received.contains("anthropic-version:"));
    let body: Value = serde_json::from_str(received.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert!(body["system"].is_string());
    assert_eq!(body["model"], "configured-model");
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn malformed_cloud_summary_is_rejected_without_publication() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("source.md");
    fs::write(&input, "Source text.").unwrap();
    let output = tmp.path().join("invalid-summary.json");
    let (mut cmd, server) = cloud_command(
        "ai.summarize",
        Some(&input),
        json!({}),
        &output,
        json!({"summary":"Source text.", "bullet_points":"wrong type", "language":"en"}),
    );
    failure(cmd.output().unwrap(), 1, "tool contract");
    assert!(!output.exists());
    server.join().unwrap();
}
