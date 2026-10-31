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
