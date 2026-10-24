//! Built-in engine compatibility with host isolation, sessions and memory limits.
#![cfg(all(feature = "pdf", feature = "typst"))]

mod support;

use std::{collections::BTreeMap, fs, path::Path};

use ariad_core::{
    limits::Limits,
    protocol::{Event, Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    runner::{self, EngineCommand, RunError},
    sandbox::{self, SandboxMode, SandboxOutcome},
    session::{PoolConfig, SessionPool},
    workspace::Workspace,
};
use tokio_util::sync::CancellationToken;

fn command(engine: &str) -> EngineCommand {
    let mut command = EngineCommand::builtin(Path::new(env!("CARGO_BIN_EXE_ashift")), engine);
    command.sandbox = Some(if sandbox::support().gaps().is_empty() {
        SandboxMode::Required
    } else {
        SandboxMode::BestEffort
    });
    command
}

fn request(workspace: &Workspace, input: &Path, from: &str, to: &str) -> Request {
    let path = workspace.input_dir().join(format!("input.{from}"));
    fs::copy(input, &path).unwrap();
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "builtin-contract".to_owned(),
        input: Input {
            path: path.display().to_string(),
            format: from.to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: to.to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options: BTreeMap::new(),
        limits: Limits::local(),
        // Engines without session support must ignore this and still exit after the result.
        session_id: Some("ignored-session".to_owned()),
    }
}

#[test]
fn builtin_engines_advertise_one_shot_and_pool_leases_fall_back() {
    let pool = SessionPool::new(PoolConfig::default());
    for engine in ["pdf", "typst", "libreoffice"] {
        let command = command(engine);
        let capabilities =
            runner::describe_command(&command, runner::DESCRIBE_TIMEOUT, CancellationToken::new())
                .unwrap();
        assert!(matches!(
            capabilities,
            Event::Capabilities {
                supports_session: false,
                enforces_memory_limit: false,
                ..
            }
        ));
        let lease = pool.lease(&command, CancellationToken::new()).unwrap();
        assert!(!lease.is_session(), "{engine} must stay one-shot");
    }
    pool.shutdown();
}

#[test]
fn pdf_typst_and_office_execute_through_the_sandbox_launcher() {
    let root = support::fixtures::repository_root();
    let pdf_workspace = Workspace::new().unwrap();
    let pdf_request = request(
        &pdf_workspace,
        &root.join("fixtures/pdf/vi-garden-route.pdf"),
        "pdf",
        "ariad-ir+json",
    );
    let run = |engine: &str, request: &Request| {
        let outcome =
            runner::run_command(&command(engine), request, CancellationToken::new(), |_| {})
                .unwrap_or_else(|error| panic!("{engine}: {error}"));
        if sandbox::support().platform && sandbox::support().gaps().is_empty() {
            assert_eq!(outcome.sandbox, SandboxOutcome::Applied);
        }
        assert_eq!(outcome.artifacts.len(), 1);
        outcome.artifacts[0].path.clone()
    };
    let ir = run("pdf", &pdf_request);
    let typst_workspace = Workspace::new().unwrap();
    let typst_request = request(&typst_workspace, &ir, "ariad-ir+json", "pdf");
    let pdf = run("typst", &typst_request);
    assert!(fs::read(pdf).unwrap().starts_with(b"%PDF-"));

    if ariad_host::engines::libreoffice::locate().is_err() {
        eprintln!("skipping Office conversion: LibreOffice >=24.2 is not installed");
        return;
    }
    let office_workspace = Workspace::new().unwrap();
    let office_request = request(
        &office_workspace,
        &root.join("fixtures/office/en-map-room-slides.pptx"),
        "pptx",
        "pdf",
    );
    let pdf = run("libreoffice", &office_request);
    assert!(fs::read(pdf).unwrap().starts_with(b"%PDF-"));
}

#[test]
fn typst_memory_is_bounded_by_the_host_watchdog() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("large.json");
    let markdown = "A paragraph that requires typesetting.\n\n".repeat(5000);
    let document = ariad_core::reader::markdown::read(&markdown, &Limits::local())
        .unwrap()
        .document;
    fs::write(&input, serde_json::to_vec(&document).unwrap()).unwrap();
    let workspace = Workspace::new().unwrap();
    let mut request = request(&workspace, &input, "ariad-ir+json", "pdf");
    if let Request::Convert { limits, .. } = &mut request {
        limits.max_memory_mb = Some(1);
        limits.timeout_s = Some(30);
    }
    let result = runner::run_command(
        &command("typst"),
        &request,
        CancellationToken::new(),
        |_| {},
    );
    assert!(
        matches!(result, Err(RunError::MemoryLimitExceeded { limit_mb: 1 })),
        "{result:?}"
    );
    assert!(!workspace.output_dir().join("document.pdf").exists());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an extracted or mounted AppImage, APPDIR and ARIAD_TEST_APPIMAGE_BINARY"]
fn appimage_office_conversion_preserves_the_bundle_in_private_tmp() {
    let program = std::path::PathBuf::from(
        std::env::var_os("ARIAD_TEST_APPIMAGE_BINARY").expect("set ARIAD_TEST_APPIMAGE_BINARY"),
    );
    let bundle = std::fs::canonicalize(std::env::var_os("APPDIR").expect("set APPDIR")).unwrap();
    let command = |engine: &str| {
        let mut command = EngineCommand::builtin(&program, engine);
        assert_eq!(command.install_root.as_ref(), Some(&bundle));
        command.sandbox = Some(SandboxMode::Required);
        command
    };
    let run = |engine: &str, request: &Request| {
        let outcome =
            runner::run_command(&command(engine), request, CancellationToken::new(), |_| {})
                .unwrap_or_else(|error| panic!("{engine}: {error}"));
        assert_eq!(outcome.sandbox, SandboxOutcome::Applied);
        assert_eq!(outcome.artifacts.len(), 1);
        outcome.artifacts[0].path.clone()
    };
    let mut office_available = false;
    for engine in ["pdf", "typst", "libreoffice"] {
        let capabilities = runner::describe_command(
            &command(engine),
            runner::DESCRIBE_TIMEOUT,
            CancellationToken::new(),
        )
        .unwrap();
        let Event::Capabilities { tool, .. } = capabilities else {
            panic!("{engine}: expected capabilities");
        };
        if engine == "libreoffice" {
            office_available = tool.status == ariad_core::protocol::ToolAvailability::Found;
            if !office_available {
                eprintln!(
                    "skipping AppImage Office conversion: LibreOffice >=24.2 is not installed ({tool:?})"
                );
            }
        } else {
            assert_eq!(tool.status, ariad_core::protocol::ToolAvailability::Found);
        }
    }
    let root = support::fixtures::repository_root();
    let pdf_workspace = Workspace::new().unwrap();
    let pdf_request = request(
        &pdf_workspace,
        &root.join("fixtures/pdf/vi-garden-route.pdf"),
        "pdf",
        "ariad-ir+json",
    );
    let ir = run("pdf", &pdf_request);
    let typst_workspace = Workspace::new().unwrap();
    let typst_request = request(&typst_workspace, &ir, "ariad-ir+json", "pdf");
    let pdf = run("typst", &typst_request);
    assert!(fs::read(pdf).unwrap().starts_with(b"%PDF-"));
    eprintln!("AppImage PDF and Typst conversions passed with required isolation");
    if !office_available {
        return;
    }
    let workspace = Workspace::new().unwrap();
    let input =
        support::fixtures::repository_root().join("fixtures/office/en-map-room-slides.pptx");
    let request = request(&workspace, &input, "pptx", "pdf");
    let pdf = fs::read(run("libreoffice", &request)).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    eprintln!("AppImage LibreOffice conversion passed with required isolation");
    let parsed = ariad_core::reader::pdf::read(&pdf, &Limits::local()).unwrap();
    assert!(parsed.document.block_count() > 0);
}
