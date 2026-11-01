mod support;

use std::{fs, path::Path};

use ariad_core::protocol::ErrorCode;
use ariad_host::{
    engines::{
        self, Protocol, ResolveError,
        resolve::{resolve_hash_computations_for, resolve_pack, resolve_pack_in},
    },
    packs::{self, InstallRequest, InstallSource, PackError},
};
use tokio_util::sync::CancellationToken;

use support::{Key, PackBuilder, Server, context};

fn install_pack(name: &str, version: &str, key: &Key, data: &Path) -> std::path::PathBuf {
    let source = support::temp();
    let builder = PackBuilder::standard(name, version);
    builder.write(source.path(), key);
    let server = Server::start();
    server.put_dir("pack", source.path());
    let report = packs::install(
        &context(data, key.trusted()),
        &InstallRequest {
            name: name.to_owned(),
            version: None,
            source: InstallSource::Manifest(server.url("pack/pack.toml")),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .expect("install pack");
    report.root
}

#[test]
fn resolve_pack_verifies_signature_and_caches_sha256_by_inode_mtime_size() {
    let key = Key::generate();
    let data = support::temp();
    let root = install_pack("qpdf", "1.0.0", &key, data.path());
    let ctx = context(data.path(), key.trusted());

    assert_eq!(resolve_hash_computations_for(&root), 0);

    let command = resolve_pack_in(&ctx, "qpdf").expect("first resolve succeeds");
    assert_eq!(command.engine, "qpdf");
    assert_eq!(command.protocol(), Protocol::V2);
    assert!(command.sandbox_required());
    assert_eq!(command.program, root.join("bin").join("engine"));
    assert_eq!(command.install_root.as_deref(), Some(root.as_path()));

    let first_hashes = resolve_hash_computations_for(&root);
    assert_eq!(
        first_hashes, 3,
        "first resolve must hash all 3 installed files in the receipt"
    );

    // Resolving again without modifying any file must hit the (inode, mtime, size) cache.
    let second = resolve_pack_in(&ctx, "qpdf").expect("second resolve succeeds");
    assert_eq!(second.program, command.program);
    assert_eq!(
        resolve_hash_computations_for(&root),
        first_hashes,
        "second resolve of unchanged pack must not recompute SHA-256 digests"
    );
}

#[test]
fn tampered_executable_or_payload_is_rejected_on_resolve_with_tool_missing_and_reinstall_hint() {
    let key = Key::generate();
    let data = support::temp();
    let root = install_pack("qpdf", "1.0.0", &key, data.path());
    let ctx = context(data.path(), key.trusted());

    // Prime the verification cache.
    resolve_pack_in(&ctx, "qpdf").expect("initial resolve succeeds");
    let hashes_before = resolve_hash_computations_for(&root);

    // Tamper with the executable using the exact same byte length so size alone wouldn't catch it
    // if mtime/inode or content weren't checked.
    let binary = root.join("bin/engine");
    let original = fs::read(&binary).expect("read binary");
    let mut same_len_tampered = original.clone();
    let last = same_len_tampered.len() - 1;
    same_len_tampered[last] ^= 0x01;
    // Write via a temporary replacement so inode / mtime updates deterministically.
    let replacement = root.join("bin/engine.tampered");
    fs::write(&replacement, &same_len_tampered).expect("write tampered binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::rename(&replacement, &binary).expect("replace binary");

    let err = resolve_pack_in(&ctx, "qpdf").expect_err("tampered binary must fail resolve");
    assert_eq!(err.code(), "tool_missing");
    assert_eq!(err.error_code(), ErrorCode::ToolMissing);
    assert_eq!(err.exit_code(), 5);
    assert_eq!(
        err.hint(),
        Some("run 'ashift engines install qpdf' to reinstall")
    );
    assert!(
        err.to_string().contains("tool_missing")
            && err
                .to_string()
                .contains("run 'ashift engines install qpdf' to reinstall"),
        "unexpected error message: {err}"
    );
    assert!(
        resolve_hash_computations_for(&root) > hashes_before,
        "changed inode/mtime must invalidate the SHA-256 cache and re-hash the file"
    );

    // Direct `packs::resolve_in` must also fail with `PackError::VerificationFailed`.
    let pack_err = packs::resolve_in(&ctx, "qpdf").expect_err("packs::resolve_in must fail");
    assert!(
        matches!(pack_err, PackError::VerificationFailed { .. }),
        "expected VerificationFailed, got {pack_err:?}"
    );
    assert_eq!(pack_err.code(), "verification_failed");
    assert!(
        pack_err
            .to_string()
            .contains("run 'ashift engines install qpdf' to reinstall"),
        "unexpected pack error: {pack_err}"
    );
}

#[test]
fn tampered_signature_or_receipt_is_rejected_on_resolve() {
    let key = Key::generate();
    let untrusted = Key::generate();
    let data = support::temp();
    let root = install_pack("pdfium", "1.0.0", &key, data.path());

    // 1. Resolving against an untrusted key must fail signature verification.
    let wrong_key_ctx = context(data.path(), untrusted.trusted());
    let err =
        resolve_pack_in(&wrong_key_ctx, "pdfium").expect_err("untrusted key must fail resolve");
    assert_eq!(err.code(), "tool_missing");
    assert_eq!(err.exit_code(), 5);
    assert_eq!(
        err.hint(),
        Some("run 'ashift engines install pdfium' to reinstall")
    );

    // 2. Tampering with the embedded manifest inside receipt.toml must fail signature verification.
    let ctx = context(data.path(), key.trusted());
    let receipt_path = root.join("receipt.toml");
    let receipt_text = fs::read_to_string(&receipt_path).expect("read receipt");
    let tampered_receipt = receipt_text.replace("HF_HUB_OFFLINE = \"1\"", "HF_HUB_OFFLINE = \"0\"");
    assert_ne!(receipt_text, tampered_receipt);
    fs::write(&receipt_path, tampered_receipt).expect("write tampered receipt");

    let err = resolve_pack_in(&ctx, "pdfium").expect_err("tampered receipt manifest must fail");
    assert_eq!(err.code(), "tool_missing");
    assert_eq!(
        err.hint(),
        Some("run 'ashift engines install pdfium' to reinstall")
    );

    // 3. Removing receipt.toml while the version directory still exists must fail closed rather
    // than silently returning NotInstalled.
    fs::remove_file(&receipt_path).expect("remove receipt.toml");
    let err = resolve_pack_in(&ctx, "pdfium").expect_err("missing receipt must fail verification");
    assert!(
        matches!(err, ResolveError::VerificationFailed { .. }),
        "expected VerificationFailed, got {err:?}"
    );
    assert_eq!(err.code(), "tool_missing");
    assert_eq!(
        err.hint(),
        Some("run 'ashift engines install pdfium' to reinstall")
    );
}

#[test]
fn unknown_engine_fails_closed_and_uninstalled_engine_reports_not_installed() {
    let key = Key::generate();
    let data = support::temp();
    let ctx = context(data.path(), key.trusted());

    for unknown in ["", "unknown-pack", "pandoc", "pdf", "typst", "libreoffice"] {
        let err = resolve_pack_in(&ctx, unknown).expect_err("non-pack or unknown engine must fail");
        assert!(
            matches!(err, ResolveError::UnknownEngine { .. }),
            "expected UnknownEngine for {unknown:?}, got {err:?}"
        );
        assert_eq!(err.code(), "unknown_engine");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.hint(), None);
        assert!(matches!(
            resolve_pack(unknown),
            Err(ResolveError::UnknownEngine { .. })
        ));
    }

    for known_pack in [
        "docling",
        "pdfium",
        "qpdf",
        "tesseract",
        "sign",
        "verapdf",
        "ai",
        "inpaint",
    ] {
        assert!(engines::is_pack_engine(known_pack));
        let err = resolve_pack_in(&ctx, known_pack).expect_err("uninstalled pack must fail");
        assert!(
            matches!(err, ResolveError::NotInstalled { .. }),
            "expected NotInstalled for {known_pack}, got {err:?}"
        );
        assert_eq!(err.code(), "tool_missing");
        assert_eq!(err.error_code(), ErrorCode::ToolMissing);
        assert_eq!(err.exit_code(), 5);
        assert!(
            err.hint()
                .is_some_and(|h| h.contains(&format!("ashift engines install {known_pack}"))),
            "expected install hint for {known_pack}, got {:?}",
            err.hint()
        );
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn receipt_edited_to_match_tampered_file_is_refused() {
    let key = Key::generate();
    let data = support::temp();
    let root = install_pack("qpdf", "1.0.0", &key, data.path());
    let ctx = context(data.path(), key.trusted());

    resolve_pack_in(&ctx, "qpdf").expect("initial resolve succeeds");

    // Tamper with the installed binary AND update receipt.toml so its unsigned [[files]] entry
    // records the tampered file's SHA-256 and size while keeping the signed manifest intact.
    let binary = root.join("bin/engine");
    let original_bytes = fs::read(&binary).expect("read original binary");
    let original_sha256 = sha256_hex(&original_bytes);
    let tampered_bytes = b"#!/bin/sh\necho compromised\nexit 1\n";
    let tampered_sha256 = sha256_hex(tampered_bytes);
    fs::write(&binary, tampered_bytes).expect("write tampered binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let receipt_path = root.join("receipt.toml");
    let original_receipt = fs::read_to_string(&receipt_path).expect("read receipt.toml");
    let forged_receipt = original_receipt
        .replace(&original_sha256, &tampered_sha256)
        .replace(
            &format!("size = {}", original_bytes.len()),
            &format!("size = {}", tampered_bytes.len()),
        );
    assert_ne!(original_receipt, forged_receipt);
    fs::write(&receipt_path, &forged_receipt).expect("write forged receipt.toml");

    let err = resolve_pack_in(&ctx, "qpdf")
        .expect_err("forged receipt matching tampered file must be refused");
    assert_eq!(err.code(), "tool_missing");
    assert_eq!(err.error_code(), ErrorCode::ToolMissing);
    assert_eq!(err.exit_code(), 5);
    assert_eq!(
        err.hint(),
        Some("run 'ashift engines install qpdf' to reinstall")
    );
    let pack_err = packs::resolve_in(&ctx, "qpdf").expect_err("packs::resolve_in must fail");
    assert!(matches!(pack_err, PackError::VerificationFailed { .. }));

    // Also test a pack with a direct Unpack::File component where the file hash lives directly
    // in the minisign-signed manifest.
    let file_pack_src = support::temp();
    let program = format!("engine{}", std::env::consts::EXE_SUFFIX);
    let mut builder = PackBuilder::new("sign", "1.0.0")
        .component(&program, &program, b"#!/bin/sh\nexit 0\n".to_vec(), "bin")
        .component("LICENSE.txt", "LICENSE.txt", b"MIT\n".to_vec(), ".");
    builder.entrypoint = vec![format!("bin/{program}")];
    builder.write(file_pack_src.path(), &key);
    let sign_report = packs::install(
        &ctx,
        &InstallRequest {
            name: "sign".to_owned(),
            version: None,
            source: InstallSource::Directory(file_pack_src.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .expect("install sign pack");
    resolve_pack_in(&ctx, "sign").expect("initial sign resolve succeeds");

    let sign_bin = sign_report.root.join("bin").join(&program);
    let sign_orig = fs::read(&sign_bin).unwrap();
    let sign_tampered = b"#!/bin/sh\nexit 9\n";
    fs::write(&sign_bin, sign_tampered).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sign_bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let sign_receipt_path = sign_report.root.join("receipt.toml");
    let sign_receipt = fs::read_to_string(&sign_receipt_path).unwrap();
    let forged_sign_receipt =
        sign_receipt.replace(&sha256_hex(&sign_orig), &sha256_hex(sign_tampered));
    assert_ne!(sign_receipt, forged_sign_receipt);
    fs::write(&sign_receipt_path, forged_sign_receipt).unwrap();

    let sign_err = resolve_pack_in(&ctx, "sign")
        .expect_err("forged receipt for Unpack::File component must be refused");
    assert_eq!(sign_err.code(), "tool_missing");
    assert_eq!(sign_err.exit_code(), 5);
}

#[test]
fn extra_unlisted_executable_in_pack_directory_is_refused() {
    let key = Key::generate();
    let data = support::temp();
    let root = install_pack("qpdf", "1.0.0", &key, data.path());
    let ctx = context(data.path(), key.trusted());

    resolve_pack_in(&ctx, "qpdf").expect("initial resolve succeeds");

    // 1. Drop an extra executable file into the pack directory that is not listed in the
    // signed manifest.
    let rogue_name = format!("helper{}", std::env::consts::EXE_SUFFIX);
    let rogue = root.join("bin").join(&rogue_name);
    let rogue_bytes = b"#!/bin/sh\nexit 0\n";
    fs::write(&rogue, rogue_bytes).expect("write unlisted executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&rogue, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let err = resolve_pack_in(&ctx, "qpdf")
        .expect_err("extra unlisted executable in pack directory must be refused");
    assert_eq!(err.code(), "tool_missing");
    assert_eq!(err.error_code(), ErrorCode::ToolMissing);
    assert_eq!(err.exit_code(), 5);
    assert!(
        err.to_string().contains("unlisted executable"),
        "expected unlisted executable reason, got: {err}"
    );

    // 2. Even if the extra executable is appended to receipt.toml, it is still not declared as an
    // executable in the signed manifest and must be refused.
    let receipt_path = root.join("receipt.toml");
    let original_receipt = fs::read_to_string(&receipt_path).expect("read receipt");
    let appended_receipt = format!(
        "{original_receipt}\n[[files]]\npath = \"bin/{rogue_name}\"\nsha256 = \"{}\"\nsize = {}\n",
        sha256_hex(rogue_bytes),
        rogue_bytes.len()
    );
    fs::write(&receipt_path, appended_receipt).expect("write receipt with extra executable");
    let err = resolve_pack_in(&ctx, "qpdf")
        .expect_err("unlisted executable added to receipt.toml must still be refused");
    assert_eq!(err.code(), "tool_missing");
    assert!(
        err.to_string().contains("unlisted executable"),
        "expected unlisted executable reason, got: {err}"
    );

    // Restore receipt.toml and remove the extra binary; resolve succeeds again.
    fs::write(&receipt_path, &original_receipt).unwrap();
    fs::remove_file(&rogue).unwrap();
    resolve_pack_in(&ctx, "qpdf").expect("clean pack resolves again");

    // 3. Marking an existing non-executable file in the pack as executable must also be refused.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let notice = root.join("LICENSE.txt");
        fs::set_permissions(&notice, fs::Permissions::from_mode(0o755)).unwrap();
        let err = resolve_pack_in(&ctx, "qpdf")
            .expect_err("non-executable file marked +x must be refused");
        assert_eq!(err.code(), "tool_missing");
        assert!(
            err.to_string().contains("unlisted executable"),
            "expected unlisted executable reason, got: {err}"
        );
    }
}

#[test]
fn replacing_file_and_restoring_mtime_invalidates_verification_cache() {
    let key = Key::generate();
    let data = support::temp();
    let root = install_pack("qpdf", "1.0.0", &key, data.path());
    let ctx = context(data.path(), key.trusted());

    // Prime the verification cache.
    resolve_pack_in(&ctx, "qpdf").expect("initial resolve succeeds");
    let hashes_before = resolve_hash_computations_for(&root);
    assert_eq!(hashes_before, 3);

    let binary = root.join("bin/engine");
    let original_metadata = fs::metadata(&binary).expect("stat binary");
    let original_mtime = original_metadata.modified().expect("read original mtime");
    let original_bytes = fs::read(&binary).expect("read original binary");
    let mut tampered = original_bytes.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;

    // Advance the clock slightly so the kernel updates ctime to a distinct timestamp when the file
    // is rewritten in-place and its mtime is restored.
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Rewrite in-place (preserving inode and size) and restore the exact original mtime so only
    // ctime changes.
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&binary)
            .expect("open binary in-place");
        file.write_all(&tampered).expect("write tampered bytes");
        file.sync_all().expect("sync binary");
        file.set_modified(original_mtime)
            .expect("restore original mtime");
    }
    assert_eq!(
        fs::metadata(&binary).unwrap().modified().unwrap(),
        original_mtime,
        "mtime must be restored to the pre-tamper value"
    );

    let err = resolve_pack_in(&ctx, "qpdf")
        .expect_err("tampered file with restored mtime must fail verification");
    assert_eq!(err.code(), "tool_missing");
    assert_eq!(err.error_code(), ErrorCode::ToolMissing);
    assert_eq!(err.exit_code(), 5);
    assert!(
        resolve_hash_computations_for(&root) > hashes_before,
        "changed ctime must invalidate the verification cache even when inode, mtime and size match"
    );
}

#[cfg(unix)]
#[test]
fn archive_pack_with_symlink_entrypoint_and_runtime_executables_resolves_cleanly() {
    use support::{Entry, tar_bytes, zstd};

    let key = Key::generate();
    let data = support::temp();
    let source = support::temp();

    let runtime = zstd(&tar_bytes(&[
        Entry::Dir("runtime/"),
        Entry::Dir("runtime/bin/"),
        Entry::Dir("runtime/lib/"),
        Entry::File(
            "runtime/bin/python3.14",
            b"#!/bin/sh\nexit 0\n".to_vec(),
            0o755,
        ),
        Entry::Symlink("runtime/bin/python3", "python3.14"),
        Entry::File(
            "runtime/lib/libpython3.14.so.1.0",
            b"\x7fELF-mock-shared-object".to_vec(),
            0o755,
        ),
        Entry::File("runtime/lib/module.py", b"print('ok')\n".to_vec(), 0o644),
        Entry::File("LICENSE.txt", b"MIT License\n".to_vec(), 0o644),
    ]));

    let mut builder =
        PackBuilder::new("docling", "1.0.0").component("runtime", "runtime.tar.zst", runtime, ".");
    builder.entrypoint = vec![
        "runtime/bin/python3".to_owned(),
        "-I".to_owned(),
        "-m".to_owned(),
        "ariad_docling".to_owned(),
    ];
    builder.write(source.path(), &key);

    let ctx = context(data.path(), key.trusted());
    let report = packs::install(
        &ctx,
        &InstallRequest {
            name: "docling".to_owned(),
            version: None,
            source: InstallSource::Directory(source.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .expect("install docling archive pack");

    let resolved_pack = packs::resolve_in(&ctx, "docling")
        .expect("packs::resolve_in succeeds")
        .expect("docling pack is installed");
    assert_eq!(
        resolved_pack.program,
        report.root.join("runtime/bin/python3")
    );

    let command = resolve_pack_in(&ctx, "docling").expect("resolve_pack_in succeeds");
    assert_eq!(command.engine, "docling");
    assert_eq!(command.program, report.root.join("runtime/bin/python3"));

    // Marking a 0644 archive member as +x after install is still rejected as an unlisted executable.
    {
        use std::os::unix::fs::PermissionsExt;
        let module_py = report.root.join("runtime/lib/module.py");
        fs::set_permissions(&module_py, fs::Permissions::from_mode(0o755)).unwrap();
        let err = resolve_pack_in(&ctx, "docling")
            .expect_err("non-executable archive file marked +x must be refused");
        assert_eq!(err.code(), "tool_missing");
        assert!(
            err.to_string().contains("unlisted executable"),
            "expected unlisted executable rejection, got: {err}"
        );
    }
}

#[test]
fn tampered_manifest_record_executables_cannot_widen_allowed_set() {
    let key = Key::generate();
    let data = support::temp();
    let root = install_pack("qpdf", "1.0.0", &key, data.path());
    let ctx = context(data.path(), key.trusted());

    resolve_pack_in(&ctx, "qpdf").expect("initial resolve succeeds");

    let record_path = data.path().join("packs/.locks/qpdf-1.0.0.manifest");
    let original_record = fs::read_to_string(&record_path).expect("read manifest record");
    let rogue_rel = format!("bin/rogue{}", std::env::consts::EXE_SUFFIX);
    let tampered_record = original_record.replace(
        "executables = [\"bin/engine\"]",
        &format!(
            "executables = [\"bin/engine\", \"LICENSE.txt\", \"models/weights.bin\", \"{rogue_rel}\"]"
        ),
    );
    assert_ne!(original_record, tampered_record);
    fs::write(&record_path, &tampered_record).expect("write tampered manifest record");

    // Extra paths in the local manifest record are ignored when no unlisted executable is on disk.
    resolve_pack_in(&ctx, "qpdf")
        .expect("extra entries in local manifest record executables are ignored");

    // Dropping an executable at the tampered path is still refused on every target.
    let rogue_path = root.join(&rogue_rel);
    fs::write(&rogue_path, b"#!/bin/sh\nexit 0\n").expect("write rogue executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&rogue_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let err = resolve_pack_in(&ctx, "qpdf")
        .expect_err("unlisted executable listed in tampered manifest record must be refused");
    assert_eq!(err.code(), "tool_missing");
    assert!(
        err.to_string().contains("unlisted executable"),
        "expected unlisted executable rejection, got: {err}"
    );
    fs::remove_file(&rogue_path).unwrap();

    // Marking a recorded non-executable archive file +x is still refused even when added to
    // the local manifest record's executables list.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let weights = root.join("models/weights.bin");
        fs::set_permissions(&weights, fs::Permissions::from_mode(0o755)).unwrap();
        let err = resolve_pack_in(&ctx, "qpdf").expect_err(
            "non-executable archive member added to manifest record executables must be refused",
        );
        assert_eq!(err.code(), "tool_missing");
        assert!(
            err.to_string().contains("unlisted executable"),
            "expected unlisted executable rejection, got: {err}"
        );
        fs::set_permissions(&weights, fs::Permissions::from_mode(0o644)).unwrap();
    }

    // Also test a pack with a non-executable Unpack::File component whose filename has an
    // executable extension on Windows (.exe) and +x on Unix: the signed component flag
    // (executable = false) must bound the allowed set on every target even if the local
    // manifest record lists it in executables.
    let file_pack_src = support::temp();
    let program = format!("engine{}", std::env::consts::EXE_SUFFIX);
    let mut builder = PackBuilder::new("sign", "1.0.0")
        .component(&program, &program, b"#!/bin/sh\nexit 0\n".to_vec(), "bin")
        .component(
            "helper.exe",
            "helper.exe",
            b"#!/bin/sh\nexit 0\n".to_vec(),
            "bin",
        )
        .component("LICENSE.txt", "LICENSE.txt", b"MIT\n".to_vec(), ".");
    builder.entrypoint = vec![format!("bin/{program}")];
    let manifest_toml = builder.write_unsigned(file_pack_src.path());
    let mut manifest = packs::Manifest::parse(&manifest_toml).expect("parse generated manifest");
    for component in &mut manifest.targets[0].components {
        if component.name == "helper.exe" {
            component.executable = false;
        }
    }
    fs::write(
        file_pack_src.path().join(packs::MANIFEST_FILE),
        manifest.to_toml().unwrap(),
    )
    .unwrap();
    support::sign_into(file_pack_src.path(), &key, "pack:sign version:1.0.0");

    let sign_report = packs::install(
        &ctx,
        &InstallRequest {
            name: "sign".to_owned(),
            version: None,
            source: InstallSource::Directory(file_pack_src.path().to_path_buf()),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .expect("install sign pack");
    let helper_exe = sign_report.root.join("bin/helper.exe");
    assert!(helper_exe.is_file());

    let sign_record_path = data.path().join("packs/.locks/sign-1.0.0.manifest");
    let sign_record = fs::read_to_string(&sign_record_path).expect("read sign manifest record");
    let tampered_sign_record = if sign_record.contains("bin/helper.exe") {
        sign_record
    } else {
        sign_record.replace(
            &format!("executables = [\"bin/{program}\"]"),
            &format!("executables = [\"bin/{program}\", \"bin/helper.exe\"]"),
        )
    };
    fs::write(&sign_record_path, tampered_sign_record).expect("write tampered sign record");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper_exe, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let sign_err = resolve_pack_in(&ctx, "sign")
        .expect_err("non-executable file component added to record executables must be refused");
    assert_eq!(sign_err.code(), "tool_missing");
    assert!(
        sign_err.to_string().contains("unlisted executable"),
        "expected unlisted executable rejection, got: {sign_err}"
    );
}
