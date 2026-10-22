mod support;

use std::{collections::BTreeMap, fs, path::Path};

use ariad_core::{
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    packs::{
        self, InstallRequest, InstallSource, InstallStatus, PackError, PackHealth, PackSource,
        SignatureError,
    },
    runner,
    workspace::Workspace,
};
use tokio_util::sync::CancellationToken;

use support::{Entry, Key, PackBuilder, Route, Server, context, gzip, tar_bytes, zip_bytes, zstd};

fn request(name: &str, manifest_url: String) -> InstallRequest {
    InstallRequest {
        name: name.to_owned(),
        version: None,
        source: InstallSource::Manifest(manifest_url),
    }
}

/// Serves a pack written by `builder` and installs it from the server.
fn install_served(
    builder: &PackBuilder,
    key: &Key,
    trusted: &Key,
    data: &Path,
) -> Result<packs::InstallReport, PackError> {
    let source = support::temp();
    builder.write(source.path(), key);
    let server = Server::start();
    server.put_dir("pack", source.path());
    packs::install(
        &context(data, trusted.trusted()),
        &request(&builder.name, server.url("pack/pack.toml")),
        &CancellationToken::new(),
        &mut |_| {},
    )
}

fn assert_no_residue(data: &Path) {
    let packs_dir = data.join("packs");
    assert!(
        support::tree(&packs_dir.join(".staging")).is_empty(),
        "staging must be empty: {:?}",
        support::tree(&packs_dir.join(".staging"))
    );
    assert!(
        support::tree(&packs_dir.join(".cache"))
            .iter()
            .all(|file| !file.ends_with(".part")),
        "no partial downloads may remain"
    );
}

#[test]
fn valid_install_resolves_to_an_engine_command() {
    let key = Key::generate();
    let data = support::temp();
    let report = install_served(
        &PackBuilder::standard("sample", "1.0.0"),
        &key,
        &key,
        data.path(),
    )
    .expect("install");
    assert_eq!(report.status, InstallStatus::Installed);
    assert_eq!(report.version, "1.0.0");
    assert_eq!(report.files, 3);
    let root = data.path().join("packs").join("sample").join("1.0.0");
    assert_eq!(report.root, root);
    assert!(root.join("receipt.toml").is_file());
    assert_eq!(
        fs::read_to_string(data.path().join("packs/sample/current")).unwrap(),
        "1.0.0\n"
    );
    assert_no_residue(data.path());
    assert!(
        support::tree(&data.path().join("packs/.cache")).is_empty(),
        "a successful install consumes its cached downloads"
    );

    let ctx = context(data.path(), key.trusted());
    let pack = packs::resolve_in(&ctx, "sample")
        .expect("resolve")
        .expect("installed");
    assert_eq!(pack.source, PackSource::Installed);
    assert_eq!(pack.root, root);
    assert_eq!(pack.program, root.join("bin").join("engine"));
    assert_eq!(pack.env, vec![("HF_HUB_OFFLINE".into(), "1".into())]);
    assert_eq!(
        pack.options["artifacts_path"],
        format!("{}/models", root.display())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&pack.program).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the engine keeps its executable bit");
    }
    assert_eq!(packs::check(&ctx, "sample", "1.0.0"), PackHealth::Ok);
    let listed = packs::list_installed(&ctx).unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].active);
}

#[test]
fn installed_engine_receives_the_pack_environment() {
    let key = Key::generate();
    let data = support::temp();
    let probe = fs::read(env!("CARGO_BIN_EXE_engine-probe")).expect("read engine-probe");
    // `file` components install as `<dest>/<component name>`; keep `.exe` on Windows.
    let program = format!("engine-probe{}", std::env::consts::EXE_SUFFIX);
    let mut builder = PackBuilder::new("probe", "2.0.0")
        .component(&program, &program, probe, "bin")
        .component("LICENSE.txt", "LICENSE.txt", b"MIT\n".to_vec(), ".");
    builder.entrypoint = vec![format!("bin/{program}"), "dump-env".to_owned()];
    builder
        .env
        .push(("TESSDATA_PREFIX".to_owned(), "{root}/tessdata".to_owned()));
    install_served(&builder, &key, &key, data.path()).expect("install probe pack");

    let ctx = context(data.path(), key.trusted());
    let pack = packs::resolve_in(&ctx, "probe").unwrap().unwrap();
    let workspace = Workspace::new().expect("workspace");
    let request = Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "pack-env".to_owned(),
        input: Input {
            path: workspace
                .input_dir()
                .join("document.ir.json")
                .display()
                .to_string(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options: BTreeMap::new(),
        limits: Limits::local(),
        session_id: None,
    };
    let command =
        runner::EngineCommand::from_pack(Path::new(env!("CARGO_BIN_EXE_engine-probe")), &pack);
    let outcomes = [
        runner::run_with_env(
            &pack.program,
            &pack.args,
            &pack.env,
            &request,
            CancellationToken::new(),
            |_| {},
        ),
        runner::run_command(&command, &request, CancellationToken::new(), |_| {}),
    ];
    for outcome in outcomes {
        let outcome = outcome.expect("run the pack engine");
        let env = outcome.metrics.expect("dump-env returns the environment");
        assert_eq!(env["HF_HUB_OFFLINE"], "1");
        assert_eq!(
            env["TESSDATA_PREFIX"],
            format!("{}/tessdata", pack.root.display())
        );
        assert!(
            !env.contains_key("ASHIFT_DATA_DIR") && !env.contains_key("HOME"),
            "the engine environment stays cleared"
        );
    }

    // A sibling data file needs the whole pack root grant, beyond the executable itself.
    let mut check = command.clone();
    check.args = vec!["sandbox-check".into()];
    let mut check_request = request.clone();
    if let Request::Convert { options, .. } = &mut check_request {
        options.insert(
            "read".into(),
            serde_json::json!(pack.root.join("LICENSE.txt")),
        );
        options.insert(
            "outside".into(),
            serde_json::json!(pack.root.join("escape.txt")),
        );
    }
    let outcome = runner::run_command(&check, &check_request, CancellationToken::new(), |_| {})
        .expect("check pack sandbox");
    let metrics = outcome.metrics.unwrap();
    assert_eq!(metrics["read_install_root"], "allowed");
    assert_eq!(metrics["exec_self"], "allowed");
    assert_eq!(metrics["write_workspace"], "allowed");
    let support = ariad_host::sandbox::support();
    if support.platform && support.landlock_filesystem {
        assert_eq!(metrics["write_outside"], "denied");
        assert!(!pack.root.join("escape.txt").exists());
    } else {
        eprintln!("skip pack write denial: Landlock filesystem isolation unavailable");
    }
}

#[test]
fn bad_signature_is_rejected() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let manifest = source.path().join(packs::MANIFEST_FILE);
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace("license = \"MIT\"", "license = \"0BSD\""),
    )
    .unwrap();
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &InstallRequest {
            name: "sample".to_owned(),
            version: None,
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(error, PackError::Signature(SignatureError::Invalid { .. })),
        "{error}"
    );
    assert_eq!(error.exit_code(), 1);
    assert!(
        error
            .to_string()
            .starts_with("pack signature verification failed")
    );
    assert!(support::tree(&data.path().join("packs")).is_empty());
}

#[test]
fn signature_from_an_untrusted_key_is_rejected() {
    let signer = Key::generate();
    let trusted = Key::generate();
    let data = support::temp();
    let error = install_served(
        &PackBuilder::standard("sample", "1.0.0"),
        &signer,
        &trusted,
        data.path(),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            PackError::Signature(SignatureError::UnknownKey { .. })
        ),
        "{error}"
    );
}

#[test]
fn trusted_comment_must_name_the_manifest() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    support::sign_into(source.path(), &key, "pack:sample version:0.9.0");
    let server = Server::start();
    server.put_dir("pack", source.path());
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &request("sample", server.url("pack/pack.toml")),
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            PackError::Signature(SignatureError::TrustedComment { .. })
        ),
        "{error}"
    );
}

#[test]
fn component_hash_mismatch_is_rejected() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let server = Server::start();
    server.put_dir("pack", source.path());
    let mut tampered = fs::read(source.path().join("runtime.tar.zst")).unwrap();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    server.put("pack/runtime.tar.zst", Route::Body(tampered));
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &request("sample", server.url("pack/pack.toml")),
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, PackError::HashMismatch { .. }), "{error}");
    assert_eq!(error.exit_code(), 1);
    assert_no_residue(data.path());
    assert!(!data.path().join("packs/sample").exists());
}

#[test]
fn component_size_mismatch_is_rejected() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let server = Server::start();
    server.put_dir("pack", source.path());
    let mut longer = fs::read(source.path().join("runtime.tar.zst")).unwrap();
    longer.extend_from_slice(b"trailing bytes");
    server.put("pack/runtime.tar.zst", Route::Body(longer));
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &request("sample", server.url("pack/pack.toml")),
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, PackError::SizeMismatch { .. }), "{error}");
    assert_eq!(
        server.hits("pack/runtime.tar.zst"),
        1,
        "size errors are not retried"
    );
    assert_no_residue(data.path());
}

#[test]
fn short_components_have_typed_size_errors_online_and_offline() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let path = source.path().join("runtime.tar.zst");
    let mut short = fs::read(&path).unwrap();
    short.pop();
    fs::write(&path, short).unwrap();
    let server = Server::start();
    server.put_dir("pack", source.path());
    for source in [
        InstallSource::Manifest(server.url("pack/pack.toml")),
        InstallSource::Directory(source.path().to_path_buf()),
    ] {
        let data = support::temp();
        let error = packs::install(
            &context(data.path(), key.trusted()),
            &InstallRequest {
                name: "sample".to_owned(),
                version: None,
                source,
            },
            &CancellationToken::new(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, PackError::SizeMismatch { .. }), "{error}");
        assert_no_residue(data.path());
    }
    assert_eq!(server.hits("pack/runtime.tar.zst"), 1);
}

#[test]
fn decoded_counts_must_match_the_manifest() {
    let key = Key::generate();
    for change_entries in [true, false] {
        let source = support::temp();
        let text = PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
        let mut manifest = packs::Manifest::parse(&text).unwrap();
        let component = &mut manifest.targets[0].components[0];
        if change_entries {
            *component.entries.as_mut().unwrap() += 1;
        } else {
            *component.unpacked_size.as_mut().unwrap() += 1;
        }
        fs::write(
            source.path().join(packs::MANIFEST_FILE),
            manifest.to_toml().unwrap(),
        )
        .unwrap();
        support::sign_into(source.path(), &key, "pack:sample version:1.0.0");
        let data = support::temp();
        let error = packs::install(
            &context(data.path(), key.trusted()),
            &InstallRequest {
                name: "sample".to_owned(),
                version: None,
                source: InstallSource::Directory(source.path().to_path_buf()),
            },
            &CancellationToken::new(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, PackError::CorruptArchive { .. }), "{error}");
        assert_no_residue(data.path());
    }
}

fn unsafe_archive(entries: &[Entry]) -> PackError {
    let key = Key::generate();
    let builder = PackBuilder::new("evil", "1.0.0").component(
        "runtime",
        "runtime.tar.zst",
        zstd(&tar_bytes(entries)),
        ".",
    );
    let source = support::temp();
    builder.write_unscanned(source.path(), &key);
    // Declare the real counts so each malformed path or link is the failure under test.
    let manifest_path = source.path().join(packs::MANIFEST_FILE);
    let mut manifest =
        packs::Manifest::parse(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let component = &mut manifest.targets[0].components[0];
    component.entries = Some(entries.len() as u64);
    component.unpacked_size = Some(
        entries
            .iter()
            .map(|entry| match entry {
                Entry::File(_, bytes, _) => bytes.len() as u64,
                _ => 0,
            })
            .sum(),
    );
    fs::write(manifest_path, manifest.to_toml().unwrap()).unwrap();
    support::sign_into(source.path(), &key, "pack:evil version:1.0.0");
    let server = Server::start();
    server.put_dir("pack", source.path());
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &request("evil", server.url("pack/pack.toml")),
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert_no_residue(data.path());
    assert!(!data.path().join("packs/evil").exists());
    error
}

#[test]
fn archive_entry_with_parent_traversal_is_rejected() {
    let error = unsafe_archive(&[Entry::File("bin/../../escape", b"x".to_vec(), 0o644)]);
    assert!(
        matches!(&error, PackError::UnsafeArchive { reason, .. } if reason.contains("..")),
        "{error}"
    );
}

#[test]
fn archive_entry_with_absolute_path_is_rejected() {
    let error = unsafe_archive(&[Entry::File("/tmp/ashift-escape", b"x".to_vec(), 0o644)]);
    assert!(
        matches!(&error, PackError::UnsafeArchive { reason, .. } if reason.contains("absolute")),
        "{error}"
    );
}

#[test]
fn archive_symlink_leaving_the_root_is_rejected() {
    let error = unsafe_archive(&[Entry::Symlink("bin", "../../outside")]);
    assert!(matches!(error, PackError::UnsafeArchive { .. }), "{error}");
    let error = unsafe_archive(&[Entry::Symlink("abs", "/etc")]);
    assert!(matches!(error, PackError::UnsafeArchive { .. }), "{error}");
}

#[cfg(unix)]
#[test]
fn chained_symlinks_cannot_escape_or_be_written_through() {
    // `deep/t` points at the root, `deep/l` lexically stays in `deep` but really resolves to
    // the root's parent; writing through it must fail before anything leaves the root.
    let error = unsafe_archive(&[
        Entry::Dir("deep/"),
        Entry::Symlink("deep/t", ".."),
        Entry::Symlink("deep/l", "t/.."),
        Entry::File("deep/l/escaped", b"x".to_vec(), 0o644),
    ]);
    assert!(matches!(error, PackError::UnsafeArchive { .. }), "{error}");
    let error = unsafe_archive(&[
        Entry::Dir("deep/"),
        Entry::Symlink("deep/t", ".."),
        Entry::Symlink("deep/l", "t/.."),
    ]);
    assert!(
        matches!(&error, PackError::UnsafeArchive { reason, .. } if reason.contains("outside")),
        "{error}"
    );
}

#[test]
fn archive_device_and_fifo_entries_are_rejected() {
    for entry in [Entry::CharDevice("dev/null"), Entry::Fifo("pipe")] {
        let error = unsafe_archive(&[entry]);
        assert!(
            matches!(&error, PackError::UnsafeArchive { reason, .. } if reason.contains("special")),
            "{error}"
        );
    }
}

#[test]
fn archive_cannot_overwrite_the_receipt() {
    let error = unsafe_archive(&[Entry::File("receipt.toml", b"x".to_vec(), 0o644)]);
    assert!(
        matches!(&error, PackError::UnsafeArchive { reason, .. } if reason.contains("reserved")),
        "{error}"
    );
}

#[test]
fn archive_larger_than_declared_is_rejected() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write_unsigned(source.path());
    let manifest_path = source.path().join(packs::MANIFEST_FILE);
    let text = fs::read_to_string(&manifest_path).unwrap();
    let mut manifest = packs::Manifest::parse(&text).unwrap();
    manifest.targets[0].components[0].unpacked_size = Some(16);
    manifest.targets[0].components[0].entries = Some(1);
    fs::write(&manifest_path, manifest.to_toml().unwrap()).unwrap();
    support::sign_into(source.path(), &key, "pack:sample version:1.0.0");
    let bomb = zstd(&tar_bytes(&[Entry::File(
        "big.bin",
        vec![0_u8; 3 << 20],
        0o644,
    )]));
    fs::write(source.path().join("runtime.tar.zst"), &bomb).unwrap();
    // Re-declare the archive's own hash and size so only the unpack bound is wrong.
    let mut manifest =
        packs::Manifest::parse(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let digest = sha256(&bomb);
    let component = &mut manifest.targets[0].components[0];
    component.sha256 = digest;
    component.size = bomb.len() as u64;
    fs::write(&manifest_path, manifest.to_toml().unwrap()).unwrap();
    support::sign_into(source.path(), &key, "pack:sample version:1.0.0");
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &InstallRequest {
            name: "sample".to_owned(),
            version: None,
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, PackError::ExtractLimit { .. }), "{error}");
    assert_eq!(error.exit_code(), 4);
    assert_no_residue(data.path());
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn interrupted_download_leaves_no_staging_residue() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let server = Server::start();
    server.put_dir("pack", source.path());
    let body = fs::read(source.path().join("runtime.tar.zst")).unwrap();
    let after = body.len() / 2;
    server.put("pack/runtime.tar.zst", Route::Truncated { body, after });
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &request("sample", server.url("pack/pack.toml")),
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, PackError::Download { .. }), "{error}");
    assert_eq!(
        server.hits("pack/runtime.tar.zst"),
        2,
        "transfers are retried"
    );
    assert_no_residue(data.path());
    assert!(!data.path().join("packs/sample").exists());
}

#[test]
fn missing_files_on_the_server_fail_without_retries() {
    let key = Key::generate();
    let server = Server::start();
    server.put("pack/pack.toml", Route::Status(404));
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &request("sample", server.url("pack/pack.toml")),
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(&error, PackError::Download { detail, .. } if detail.contains("404")),
        "{error}"
    );
    assert_eq!(server.hits("pack/pack.toml"), 1);
}

#[test]
fn reinstall_is_idempotent_and_repairs_damage() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let server = Server::start();
    server.put_dir("pack", source.path());
    let data = support::temp();
    let ctx = context(data.path(), key.trusted());
    let install = || {
        packs::install(
            &ctx,
            &request("sample", server.url("pack/pack.toml")),
            &CancellationToken::new(),
            &mut |_| {},
        )
    };
    assert_eq!(install().unwrap().status, InstallStatus::Installed);
    assert_eq!(install().unwrap().status, InstallStatus::AlreadyInstalled);
    assert_eq!(server.hits("pack/runtime.tar.zst"), 1, "no second download");

    let weights = data.path().join("packs/sample/1.0.0/models/weights.bin");
    fs::write(&weights, vec![8_u8; 4096]).unwrap();
    assert!(
        !packs::verify(&ctx, "sample", None, &CancellationToken::new())
            .unwrap()
            .ok
    );
    assert_eq!(install().unwrap().status, InstallStatus::Installed);
    assert!(
        packs::verify(&ctx, "sample", None, &CancellationToken::new())
            .unwrap()
            .ok,
        "reinstall repairs changed content even when the size stays the same"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&weights, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(install().unwrap().status, InstallStatus::Installed);
        assert!(
            packs::verify(&ctx, "sample", None, &CancellationToken::new())
                .unwrap()
                .ok,
            "an unreadable installed file must be replaceable"
        );
    }

    fs::remove_file(data.path().join("packs/sample/1.0.0/models/weights.bin")).unwrap();
    assert!(matches!(
        packs::check(&ctx, "sample", "1.0.0"),
        PackHealth::Problem(_)
    ));
    assert_eq!(install().unwrap().status, InstallStatus::Installed);
    assert!(
        packs::verify(&ctx, "sample", None, &CancellationToken::new())
            .unwrap()
            .ok
    );
    assert_no_residue(data.path());
}

#[test]
fn zstd_archives_can_span_multiple_frames() {
    let archive = tar_bytes(&[
        Entry::File("bin/engine", b"#!/bin/sh\nexit 0\n".to_vec(), 0o755),
        Entry::File("LICENSE.txt", b"MIT licence text\n".to_vec(), 0o644),
        Entry::File("models/weights.bin", vec![7_u8; 4096], 0o644),
    ]);
    // Split the tar stream after its first file, at a header boundary.
    let mut compressed = zstd(&archive[..1024]);
    compressed.extend(zstd(&archive[1024..]));
    let key = Key::generate();
    let data = support::temp();
    let report = install_served(
        &PackBuilder::new("sample", "1.0.0").component(
            "runtime",
            "runtime.tar.zst",
            compressed,
            ".",
        ),
        &key,
        &key,
        data.path(),
    )
    .expect("install every frame");
    assert_eq!(report.files, 3);
    assert_eq!(
        fs::read(data.path().join("packs/sample/1.0.0/models/weights.bin")).unwrap(),
        vec![7_u8; 4096]
    );
}

#[test]
fn verify_detects_modified_and_missing_files() {
    let key = Key::generate();
    let data = support::temp();
    install_served(
        &PackBuilder::standard("sample", "1.0.0"),
        &key,
        &key,
        data.path(),
    )
    .unwrap();
    let ctx = context(data.path(), key.trusted());
    let cancel = CancellationToken::new();
    let report = packs::verify(&ctx, "sample", None, &cancel).unwrap();
    assert!(report.ok);
    assert_eq!(report.checked, 3);
    assert!(report.key_id.is_some());

    let other = Key::generate();
    assert!(matches!(
        packs::check(&context(data.path(), other.trusted()), "sample", "1.0.0"),
        PackHealth::Problem(_)
    ));

    let root = data.path().join("packs/sample/1.0.0");
    fs::write(root.join("models/weights.bin"), vec![8_u8; 4096]).unwrap();
    fs::remove_file(root.join("LICENSE.txt")).unwrap();
    let report = packs::verify(&ctx, "sample", Some("1.0.0"), &cancel).unwrap();
    assert!(!report.ok);
    assert_eq!(report.modified, vec!["models/weights.bin".to_owned()]);
    assert_eq!(report.missing, vec!["LICENSE.txt".to_owned()]);

    let report = packs::verify(
        &context(data.path(), other.trusted()),
        "sample",
        None,
        &cancel,
    )
    .unwrap();
    assert!(report.problems.iter().any(|p| p.starts_with("signature")));
}

#[test]
fn remove_deletes_versions_and_keeps_the_rest_active() {
    let key = Key::generate();
    let source = support::temp();
    let server = Server::start();
    let data = support::temp();
    let ctx = context(data.path(), key.trusted());
    for version in ["1.0.0", "1.1.0"] {
        let dir = source.path().join(version);
        PackBuilder::standard("sample", version).write(&dir, &key);
        server.put_dir(version, &dir);
        packs::install(
            &ctx,
            &request("sample", server.url(&format!("{version}/pack.toml"))),
            &CancellationToken::new(),
            &mut |_| {},
        )
        .unwrap();
    }
    assert_eq!(
        packs::resolve_in(&ctx, "sample").unwrap().unwrap().version,
        "1.1.0"
    );
    let report = packs::remove(&ctx, "sample", Some("1.1.0")).unwrap();
    assert_eq!(report.removed, vec!["1.1.0".to_owned()]);
    assert_eq!(report.active.as_deref(), Some("1.0.0"));
    assert_eq!(
        packs::resolve_in(&ctx, "sample").unwrap().unwrap().version,
        "1.0.0"
    );
    let report = packs::remove(&ctx, "sample", None).unwrap();
    assert_eq!(report.removed, vec!["1.0.0".to_owned()]);
    assert_eq!(report.active, None);
    assert!(packs::resolve_in(&ctx, "sample").unwrap().is_none());
    assert!(!data.path().join("packs/sample").exists());
    let error = packs::remove(&ctx, "sample", None).unwrap_err();
    assert!(matches!(error, PackError::NotInstalled { .. }), "{error}");
    assert_eq!(error.exit_code(), 3);
    assert_no_residue(data.path());
}

#[test]
fn unsupported_target_is_reported_with_the_available_triples() {
    let key = Key::generate();
    let mut builder = PackBuilder::standard("sample", "1.0.0");
    builder.triple = "riscv64gc-unknown-none-elf".to_owned();
    let data = support::temp();
    let error = install_served(&builder, &key, &key, data.path()).unwrap_err();
    assert!(
        matches!(&error, PackError::UnsupportedTarget { available, .. } if available.contains("riscv64gc")),
        "{error}"
    );
    assert_eq!(error.exit_code(), 3);
}

#[test]
fn requires_ashift_mismatch_is_a_version_error() {
    let key = Key::generate();
    let mut builder = PackBuilder::standard("sample", "1.0.0");
    builder.requires = ">=99.0.0".to_owned();
    let data = support::temp();
    let error = install_served(&builder, &key, &key, data.path()).unwrap_err();
    assert!(
        matches!(error, PackError::IncompatibleAshift { .. }),
        "{error}"
    );
    assert_eq!(error.exit_code(), 5);
}

#[test]
fn requested_version_must_match_the_manifest() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let data = support::temp();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &InstallRequest {
            name: "sample".to_owned(),
            version: Some("2.0.0".to_owned()),
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, PackError::Mismatch { .. }), "{error}");
}

#[test]
fn offline_directory_install_needs_no_network() {
    let key = Key::generate();
    let source = support::temp();
    let builder = PackBuilder::standard("sample", "1.0.0")
        .component(
            "extras",
            "extras.tar.gz",
            gzip(&tar_bytes(&[Entry::File(
                "note.txt",
                b"gz".to_vec(),
                0o644,
            )])),
            "extras",
        )
        .component(
            "tables",
            "tables.zip",
            zip_bytes(&[
                Entry::Dir("t/"),
                Entry::File("t/one.csv", b"a,b\n".to_vec(), 0o644),
            ]),
            "data",
        );
    builder.write(source.path(), &key);
    let data = support::temp();
    let ctx = context(data.path(), key.trusted());
    let report = packs::install(
        &ctx,
        &InstallRequest {
            name: "sample".to_owned(),
            version: Some("1.0.0".to_owned()),
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(report.status, InstallStatus::Installed);
    let root = data.path().join("packs/sample/1.0.0");
    assert_eq!(fs::read(root.join("extras/note.txt")).unwrap(), b"gz");
    assert_eq!(fs::read(root.join("data/t/one.csv")).unwrap(), b"a,b\n");
    assert!(
        packs::verify(&ctx, "sample", None, &CancellationToken::new())
            .unwrap()
            .ok
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_hard_links_inside_the_root_are_kept() {
    let key = Key::generate();
    let builder = PackBuilder::new("linked", "1.0.0").component(
        "runtime",
        "runtime.tar.zst",
        zstd(&tar_bytes(&[
            Entry::File("bin/engine-1.0", b"#!/bin/sh\n".to_vec(), 0o755),
            Entry::Symlink("bin/engine", "engine-1.0"),
            Entry::HardLink("bin/engine-copy", "bin/engine-1.0"),
            Entry::File("LICENSE.txt", b"MIT\n".to_vec(), 0o644),
        ])),
        ".",
    );
    let data = support::temp();
    install_served(&builder, &key, &key, data.path()).expect("install linked pack");
    let root = data.path().join("packs/linked/1.0.0");
    assert_eq!(
        fs::read_link(root.join("bin/engine")).unwrap(),
        Path::new("engine-1.0")
    );
    assert_eq!(
        fs::read(root.join("bin/engine-copy")).unwrap(),
        b"#!/bin/sh\n"
    );
    let ctx = context(data.path(), key.trusted());
    let report = packs::verify(&ctx, "linked", None, &CancellationToken::new()).unwrap();
    assert!(report.ok, "{report:?}");
    fs::remove_file(root.join("bin/engine")).unwrap();
    std::os::unix::fs::symlink("engine-copy", root.join("bin/engine")).unwrap();
    let report = packs::verify(&ctx, "linked", None, &CancellationToken::new()).unwrap();
    assert_eq!(report.modified, vec!["bin/engine".to_owned()]);
}

#[test]
fn concurrent_install_of_the_same_pack_is_refused() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let data = support::temp();
    let locks = data.path().join("packs/.locks");
    fs::create_dir_all(&locks).unwrap();
    let held = fs::File::create(locks.join("sample.lock")).unwrap();
    held.lock().unwrap();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &InstallRequest {
            name: "sample".to_owned(),
            version: None,
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, PackError::Busy { .. }), "{error}");
}

#[test]
fn cancelled_install_stops_and_cleans_up() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let data = support::temp();
    let cancel = CancellationToken::new();
    let error = packs::install(
        &context(data.path(), key.trusted()),
        &InstallRequest {
            name: "sample".to_owned(),
            version: None,
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &cancel,
        &mut |event| {
            if matches!(&event, packs::PackEvent::Stage(stage) if stage.starts_with("extract")) {
                cancel.cancel();
            }
        },
    )
    .unwrap_err();
    assert!(matches!(error, PackError::Interrupted), "{error}");
    assert_eq!(error.exit_code(), 130);
    assert_no_residue(data.path());
    assert!(!data.path().join("packs/sample").exists());
}

#[test]
fn development_override_resolves_an_unpacked_directory() {
    let dir = support::temp();
    fs::create_dir_all(dir.path().join("bin")).unwrap();
    fs::write(dir.path().join("bin/engine"), b"engine").unwrap();
    PackBuilder::standard("dev-pack", "0.1.0").write_unsigned(&dir.path().join("unused"));
    fs::copy(
        dir.path().join("unused").join(packs::MANIFEST_FILE),
        dir.path().join(packs::MANIFEST_FILE),
    )
    .unwrap();
    let pack = packs::resolve_override("dev-pack", dir.path(), &packs::host_targets()).unwrap();
    assert_eq!(pack.source, PackSource::Override);
    assert_eq!(pack.program, dir.path().join("bin").join("engine"));
    let error = packs::resolve_override("other", dir.path(), &packs::host_targets()).unwrap_err();
    assert!(matches!(error, PackError::Mismatch { .. }), "{error}");
}

#[test]
fn generated_manifests_describe_archives_exactly() {
    let dir = support::temp();
    let archive = dir.path().join("runtime.tar.zst");
    fs::write(
        &archive,
        zstd(&tar_bytes(&[
            Entry::Dir("bin/"),
            Entry::File("bin/a", vec![1; 10], 0o755),
            Entry::File("b", vec![2; 5], 0o644),
        ])),
    )
    .unwrap();
    let stats = packs::scan(&archive, packs::Unpack::TarZst, "runtime").unwrap();
    assert_eq!(
        stats,
        packs::Stats {
            bytes: 15,
            entries: 3
        }
    );
    let unsafe_archive = dir.path().join("evil.tar.gz");
    fs::write(
        &unsafe_archive,
        gzip(&tar_bytes(&[Entry::File("../x", vec![1], 0o644)])),
    )
    .unwrap();
    assert!(matches!(
        packs::scan(&unsafe_archive, packs::Unpack::TarGz, "evil"),
        Err(PackError::UnsafeArchive { .. })
    ));
}

#[test]
fn sweeping_one_pack_leaves_other_packs_staging_alone() {
    let key = Key::generate();
    let source = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let data = support::temp();
    let staging = data.path().join("packs/.staging");
    let other = staging.join("sample-x.0123456789abcdef");
    let own = staging.join("sample.0123456789abcdef");
    fs::create_dir_all(&other).unwrap();
    fs::create_dir_all(&own).unwrap();
    packs::install(
        &context(data.path(), key.trusted()),
        &InstallRequest {
            name: "sample".to_owned(),
            version: None,
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap();
    assert!(
        other.is_dir(),
        "another pack's staging directory must survive"
    );
    assert!(
        !own.exists(),
        "this pack's stale staging directory is swept"
    );
}

#[test]
fn installing_another_pack_keeps_downloads_available_for_extraction() {
    let key = Key::generate();
    let first = support::temp();
    let second = support::temp();
    PackBuilder::standard("first", "1.0.0").write(first.path(), &key);
    PackBuilder::standard("second", "1.0.0").write(second.path(), &key);
    let data = support::temp();
    let ctx = context(data.path(), key.trusted());
    let mut installed_first = false;
    let second_report = packs::install(
        &ctx,
        &InstallRequest {
            name: "second".to_owned(),
            version: None,
            source: InstallSource::Directory(second.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |event| {
            if matches!(event, packs::PackEvent::Stage(ref stage) if stage == "extract runtime") {
                // Finish another install with identical component bytes between download
                // and extraction, as two concurrently running pack installers can do.
                packs::install(
                    &ctx,
                    &InstallRequest {
                        name: "first".to_owned(),
                        version: None,
                        source: InstallSource::Directory(first.path().to_path_buf()),
                    },
                    &CancellationToken::new(),
                    &mut |_| {},
                )
                .expect("install first pack");
                installed_first = true;
            }
        },
    )
    .expect("another install must not consume this install's downloads");
    assert!(installed_first);
    assert_eq!(second_report.status, InstallStatus::Installed);
    for name in ["first", "second"] {
        assert!(
            packs::verify(&ctx, name, None, &CancellationToken::new())
                .unwrap()
                .ok
        );
    }
    assert_no_residue(data.path());
}
