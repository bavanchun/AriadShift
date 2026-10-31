//! Install and run the actual built pack. CI supplies ASHIFT_DOCLING_PACK_TEST_DIR.

mod support;

use std::{collections::BTreeMap, fs, io::Cursor, path::PathBuf, process::Command};

use ariad_core::{
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    packs, runner,
    session::{PoolConfig, SessionPool},
};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

#[test]
#[ignore = "needs a built docling pack and the test-probe feature"]
fn installed_pack_converts_and_reuses_bounded_sessions() {
    let source = PathBuf::from(
        std::env::var_os("ASHIFT_DOCLING_PACK_TEST_DIR").expect("built pack directory"),
    );
    let directory = tempdir().unwrap();
    let signed = directory.path().join("signed");
    let data = directory.path().join("data");
    fs::create_dir(&signed).unwrap();
    let text = fs::read_to_string(source.join("pack.toml")).unwrap();
    let mut manifest = packs::Manifest::parse(&text).unwrap();
    assert_eq!(manifest.requires_ashift, ">=0.2.0, <2.0.0");
    // Only the unreleased 0.1.0 source-tree test needs an exact-version copy.
    // Released hosts exercise the shipped compatibility range unchanged.
    if env!("CARGO_PKG_VERSION") == "0.1.0" {
        manifest.requires_ashift = "=0.1.0".to_owned();
    }
    let text = toml::to_string(&manifest).unwrap();
    fs::write(signed.join("pack.toml"), &text).unwrap();
    for component in &manifest
        .select_target(&packs::host_targets())
        .unwrap()
        .components
    {
        let name = component.url.rsplit('/').next().unwrap();
        fs::hard_link(source.join(name), signed.join(name))
            .or_else(|_| fs::copy(source.join(name), signed.join(name)).map(|_| ()))
            .unwrap();
    }
    let key = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let public = key.pk.to_box().unwrap().into_string();
    let public_file = directory.path().join("test.pub");
    fs::write(&public_file, &public).unwrap();
    let comment = format!("pack:docling version:{}", manifest.version);
    let signature = minisign::sign(
        Some(&key.pk),
        &key.sk,
        Cursor::new(text.as_bytes()),
        Some(&comment),
        None,
    )
    .unwrap();
    fs::write(signed.join("pack.toml.minisig"), signature.into_string()).unwrap();
    let cli = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
        command
            .env("ASHIFT_DATA_DIR", &data)
            .env("ASHIFT_TEST_PACK_KEYS", &public_file)
            .env_remove("ASHIFT_PACK_DOCLING")
            .env_remove("ASHIFT_DOCLING_ENGINE");
        command
    };
    eprintln!("Installing the signed Docling pack");
    let installed = cli()
        .args(["engines", "install", "docling", "--from"])
        .arg(&signed)
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    assert!(String::from_utf8_lossy(&installed.stdout).contains("Tesseract 5"));
    assert_eq!(
        String::from_utf8_lossy(&installed.stdout)
            .contains("latest Microsoft Visual C++ Redistributable (x64)"),
        cfg!(windows)
    );
    let fixture = support::fixtures::repository_root().join("fixtures/pdf/en-map-room.pdf");
    let markdown = directory.path().join("converted.md");
    eprintln!("Converting a PDF through installed-pack resolution");
    let converted = cli()
        .args(["convert", "--engine", "docling"])
        .arg(&fixture)
        .args(["--to", "md", "-o"])
        .arg(&markdown)
        .output()
        .unwrap();
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );
    assert!(
        fs::read_to_string(markdown)
            .unwrap()
            .contains("Notes from the Map Room")
    );

    let context = packs::PackContext {
        data_dir: data,
        keys: packs::TrustedKeys::new(vec![packs::TrustedKey::parse("test.pub", &public).unwrap()]),
        targets: packs::host_targets(),
        fetch: packs::FetchPolicy::default(),
    };
    let pack = packs::resolve_in(&context, "docling").unwrap().unwrap();
    let command =
        runner::EngineCommand::from_pack(std::path::Path::new(env!("CARGO_BIN_EXE_ashift")), &pack);
    let pool = SessionPool::new(PoolConfig::default());
    let mut session_root = None;
    for job in 0..3 {
        eprintln!("Running session job {job} (64 MiB limit on the last job)");
        let cancel = CancellationToken::new();
        let mut lease = pool.lease(&command, cancel.clone()).unwrap();
        let mut workspace = lease.workspace().unwrap();
        let root = workspace.root().parent().unwrap().to_path_buf();
        if let Some(previous) = &session_root {
            assert_eq!(&root, previous);
        }
        session_root = Some(root);
        let input = workspace.input_dir().join("document.docx");
        fs::copy(
            support::fixtures::repository_root().join("fixtures/docx/vi-route-table.docx"),
            &input,
        )
        .unwrap();
        let request = Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: job.to_string(),
            input: Input {
                path: input.to_string_lossy().into_owned(),
                format: "docx".to_owned(),
            },
            output: Output {
                dir: workspace.output_dir().to_string_lossy().into_owned(),
                format: "docling+json".to_owned(),
            },
            work_dir: workspace.work_dir().to_string_lossy().into_owned(),
            options: pack.options.clone().into_iter().collect::<BTreeMap<_, _>>(),
            limits: Limits {
                max_memory_mb: (job == 2).then_some(64),
                timeout_s: Some(180),
                ..Limits::local()
            },
            session_id: None,
        };
        let result = lease.run(&request, cancel, |_| {});
        if job < 2 {
            let result = result.unwrap();
            assert_eq!(result.artifacts.len(), 1);
            let file = workspace.open_artifact(&result.artifacts[0].path).unwrap();
            assert!(file.metadata().unwrap().len() > 0);
            #[cfg(target_os = "linux")]
            assert_eq!(result.sandbox, ariad_host::sandbox::SandboxOutcome::Applied);
        } else {
            assert!(
                matches!(
                    result,
                    Err(runner::RunError::MemoryLimitExceeded { .. }
                        | runner::RunError::EngineFailed {
                            code: ariad_core::protocol::ErrorCode::LimitExceeded,
                            ..
                        })
                ),
                "{result:?}"
            );
        }
        workspace.close().unwrap();
        drop(lease);
    }
    pool.shutdown();
}
