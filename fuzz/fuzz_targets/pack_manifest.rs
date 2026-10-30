#![no_main]

//! Engine pack manifests (`ariad-pack/1` TOML) and their minisign signatures.
//! The harness parses strictly, then verifies using the parsed name/version (or `fuzz 0.0.0`).
//! Install verifies the signature before trusting manifest fields; this parser harness also
//! exercises invalid manifests independently of signature validity.
//!
//! Input layout: `<pack.toml> [NUL <pack.toml.minisig> [NUL <public key>]]`.

use std::sync::OnceLock;

use ariad_host::packs::{
    ALLOWED_ENV, MANIFEST_SCHEMA, Manifest, SignatureError, TrustedKey, TrustedKeys,
};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 64 * 1024;
/// `install` reads at most this many signature bytes (`signature::MAX_SIGNATURE_BYTES`).
const MAX_SIGNATURE_BYTES: usize = 4096;

/// A path from a verified manifest never leaves the install root.
fn assert_relative(path: &str) {
    assert!(!path.is_empty(), "empty path accepted");
    assert!(!path.starts_with('/'), "absolute path {path:?} accepted");
    assert!(
        !path.contains('\\') && !path.contains(':') && !path.chars().any(char::is_control),
        "unsafe characters in path {path:?} accepted"
    );
    assert!(
        path.split('/').all(|part| !matches!(part, "" | "." | "..")),
        "unsafe component in path {path:?} accepted"
    );
}

fn check_manifest(manifest: &Manifest) {
    assert_eq!(manifest.schema, MANIFEST_SCHEMA);
    manifest.validate().expect("a parsed manifest validates");
    for notice in &manifest.notices {
        assert_relative(notice);
    }
    for target in &manifest.targets {
        assert_relative(&target.entrypoint[0]);
        assert!(
            target
                .env
                .keys()
                .all(|key| ALLOWED_ENV.contains(&key.as_str()))
        );
        assert!(!target.components.is_empty());
        for component in &target.components {
            if component.dest != "." {
                assert_relative(&component.dest);
            }
            assert_relative(&component.file_path());
            assert_eq!(component.sha256.len(), 64);
        }
        let selected = manifest
            .select_target(std::slice::from_ref(&target.triple))
            .expect("every listed triple is selectable");
        assert_eq!(selected, target);
    }

    // The authoring tool writes manifests with `to_toml`; what it writes parses back unchanged.
    if let Ok(text) = manifest.to_toml() {
        let reparsed = Manifest::parse(&text).expect("a serialized manifest parses again");
        assert_eq!(
            &reparsed, manifest,
            "manifest changed across a TOML round trip"
        );
    }
}

fn check_signature(
    keys: &TrustedKeys,
    manifest: &[u8],
    signature: &str,
    name: &str,
    version: &str,
) -> bool {
    match keys.verify(manifest, signature, name, version) {
        Ok(id) => {
            assert!(keys.ids().contains(&id), "verified by an unknown key {id}");
            let mut tampered = manifest.to_vec();
            tampered.push(b'\n');
            assert!(
                matches!(
                    keys.verify(&tampered, signature, name, version),
                    Err(SignatureError::Invalid { .. })
                ),
                "tampered manifest verified"
            );
            for (wrong_name, wrong_version) in [
                (format!("{name}-other"), version.to_owned()),
                (name.to_owned(), format!("{version}-other")),
            ] {
                assert!(
                    matches!(
                        keys.verify(manifest, signature, &wrong_name, &wrong_version),
                        Err(SignatureError::TrustedComment { .. })
                    ),
                    "trusted comment did not bind identity"
                );
            }
            true
        }
        Err(SignatureError::NoTrustedKeys) => {
            assert!(keys.is_empty());
            false
        }
        Err(
            SignatureError::Malformed
            | SignatureError::Legacy
            | SignatureError::UnknownKey { .. }
            | SignatureError::Invalid { .. }
            | SignatureError::TrustedComment { .. },
        ) => false,
    }
}

/// Pin the success path to a real, once-signed fixture; only its public key is retained.
fn check_verified_seed() {
    static CHECKED: OnceLock<()> = OnceLock::new();
    CHECKED.get_or_init(|| {
        let seed = include_bytes!("../seeds/pack_manifest/verified-throwaway.bin");
        let mut sections = seed.splitn(3, |byte| *byte == 0);
        let bytes = sections.next().unwrap();
        let signature = std::str::from_utf8(sections.next().unwrap()).unwrap();
        let public = std::str::from_utf8(sections.next().unwrap()).unwrap();
        let manifest = Manifest::parse(std::str::from_utf8(bytes).unwrap()).unwrap();
        let key = TrustedKey::parse("throwaway.pub", public).unwrap();
        assert!(
            check_signature(
                &TrustedKeys::new(vec![key]),
                bytes,
                signature,
                &manifest.name,
                &manifest.version
            ),
            "the signed fixture must verify"
        );
    });
}

fuzz_target!(|data: &[u8]| {
    check_verified_seed();
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let mut sections = data.splitn(3, |byte| *byte == 0);
    let manifest_bytes = sections.next().unwrap_or_default();
    let signature_bytes = sections.next();
    let key_bytes = sections.next();

    // `install` rejects a manifest that is not UTF-8 before parsing it.
    let Ok(manifest_text) = std::str::from_utf8(manifest_bytes) else {
        return;
    };
    let parsed = Manifest::parse(manifest_text);
    if let Ok(manifest) = &parsed {
        check_manifest(manifest);
    }

    let Some(signature_text) = signature_bytes
        .filter(|bytes| bytes.len() <= MAX_SIGNATURE_BYTES)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
    else {
        return;
    };
    let (name, version) = parsed.as_ref().map_or(("fuzz", "0.0.0"), |manifest| {
        (manifest.name.as_str(), manifest.version.as_str())
    });

    check_signature(
        &TrustedKeys::compiled(),
        manifest_bytes,
        signature_text,
        name,
        version,
    );
    check_signature(
        &TrustedKeys::default(),
        manifest_bytes,
        signature_text,
        name,
        version,
    );

    // A public key supplied with the input exercises the key parser and the full verify path.
    if let Some(key_text) = key_bytes.and_then(|bytes| std::str::from_utf8(bytes).ok())
        && let Ok(key) = TrustedKey::parse("fuzz.pub", key_text)
    {
        assert_eq!(key.id.len(), 16);
        assert!(key.id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let keys = TrustedKeys::new(vec![key]);
        check_signature(&keys, manifest_bytes, signature_text, name, version);
    }
});
