//! Host-side registration of the `pdfium` engine pack: the manifest `engine_protocol` check at
//! install and resolve, fail-closed resolution, the missing-pack error of every pdfium tool,
//! and the `doctor` and `engines` lines.
//!
//! No PDFium library is involved. Tests that start an engine use the `engine-probe` helper
//! under the `pdfium` engine id, so they run only with the `test-probe` feature. Tests that
//! depend on process environment variables re-run themselves in a child process with a
//! controlled environment.
mod support;

use std::{fs, path::Path, process::Command};

use ariad_host::{
    engines::{self, Protocol, ResolveError},
    packs::{self, InstallRequest, InstallSource, PackError},
};
use support::{Key, PackBuilder, Server, context};
use tokio_util::sync::CancellationToken;

const CHILD: &str = "ARIAD_TEST_PDFIUM_CHILD";

/// Tool ids the pdfium engine provides.
const PDFIUM_TOOLS: &[&str] = &[
    "pdf.render",
    "pdf.thumbnails",
    "pdf.to_images",
    "pdf.extract_images",
    "pdf.text_layout",
    "pdf.remove_blank",
    "pdf.crop_whitespace",
    "pdf.detect_scanned",
];

/// Returns `true` inside the child process. In the parent it re-runs `test` in a child with the
/// pack and sandbox variables cleared (`configure` then sets what the test needs), asserts the
/// child passed and returns `false`.
fn in_child(test: &str, configure: impl FnOnce(&mut Command)) -> bool {
    if std::env::var_os(CHILD).is_some() {
        return true;
    }
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env_remove("ASHIFT_PACK_PDFIUM")
        .env_remove("ASHIFT_SANDBOX");
    configure(&mut command);
    let output = command.output().expect("run the child test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child test {test} failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

fn serve(builder: &PackBuilder, signed_unscanned: bool, key: &Key) -> Server {
    let source = support::temp();
    if signed_unscanned {
        builder.write_unscanned(source.path(), key);
    } else {
        builder.write(source.path(), key);
    }
    let server = Server::start();
    server.put_dir("pack", source.path());
    server
}

fn install(
    server: &Server,
    name: &str,
    data: &Path,
    key: &Key,
) -> Result<packs::InstallReport, PackError> {
    packs::install(
        &context(data, key.trusted()),
        &InstallRequest {
            name: name.to_owned(),
            version: None,
            source: InstallSource::Manifest(server.url("pack/pack.toml")),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
}

#[test]
fn install_accepts_a_pdfium_pack_that_declares_protocol_2() {
    let key = Key::generate();
    let data = support::temp();
    let builder = PackBuilder::standard("pdfium", "0.1.0");
    assert_eq!(builder.engine_protocol.as_deref(), Some("ariad-engine/2"));
    let server = serve(&builder, false, &key);
    let report = install(&server, "pdfium", data.path(), &key).expect("install pdfium pack");

    let command = engines::resolve_pack_in(&context(data.path(), key.trusted()), "pdfium")
        .expect("resolve the installed pack");
    assert_eq!(command.engine, "pdfium");
    assert_eq!(command.protocol(), Protocol::V2);
    assert!(command.sandbox_required());
    assert_eq!(command.install_root.as_deref(), Some(report.root.as_path()));
    assert!(
        command.program.starts_with(&report.root),
        "the entrypoint lies under the pack root"
    );
}

#[test]
fn install_refuses_a_pdfium_pack_whose_protocol_differs_and_installs_nothing() {
    for declared in [None, Some("ariad-engine/1")] {
        let key = Key::generate();
        let data = support::temp();
        let builder = PackBuilder::standard("pdfium", "0.1.0").engine_protocol(declared);
        let server = serve(&builder, true, &key);
        let error = install(&server, "pdfium", data.path(), &key)
            .expect_err("a v1 declaration for a v2 engine must be refused");
        let PackError::InvalidManifest(message) = &error else {
            panic!("expected InvalidManifest, got {error:?}");
        };
        assert!(
            message.contains("engine_protocol")
                && message.contains("'pdfium' engine speaks 'ariad-engine/2'"),
            "{message}"
        );
        assert!(
            packs::list_installed(&context(data.path(), key.trusted()))
                .expect("list")
                .is_empty(),
            "a refused pack must leave nothing installed"
        );
    }
}

#[test]
fn install_refuses_an_unknown_protocol_value() {
    let key = Key::generate();
    let data = support::temp();
    let builder = PackBuilder::standard("pdfium", "0.1.0").engine_protocol(Some("ariad-engine/9"));
    let server = serve(&builder, true, &key);
    let error = install(&server, "pdfium", data.path(), &key).expect_err("unknown value");
    assert!(
        matches!(&error, PackError::InvalidManifest(message)
            if message.contains("engine_protocol 'ariad-engine/9' is not supported")),
        "{error:?}"
    );
}

#[test]
fn install_refuses_a_docling_pack_that_claims_protocol_2() {
    let key = Key::generate();
    let data = support::temp();
    let builder = PackBuilder::standard("docling", "0.1.0").engine_protocol(Some("ariad-engine/2"));
    let server = serve(&builder, true, &key);
    let error = install(&server, "docling", data.path(), &key).expect_err("docling is v1");
    assert!(
        matches!(&error, PackError::InvalidManifest(message)
            if message.contains("'docling' engine speaks 'ariad-engine/1'")),
        "{error:?}"
    );
}

#[test]
fn existing_manifests_without_the_field_stay_valid_for_docling() {
    let key = Key::generate();
    let data = support::temp();
    let builder = PackBuilder::standard("docling", "0.1.0");
    assert_eq!(builder.engine_protocol, None);
    let server = serve(&builder, false, &key);
    install(&server, "docling", data.path(), &key).expect("a manifest without the field installs");
    let command = engines::resolve_pack_in(&context(data.path(), key.trusted()), "docling")
        .expect("and resolves");
    assert_eq!(command.protocol(), Protocol::V1);
}

/// Writes an unpacked development pack for `pdfium` into `dir`: a manifest declaring
/// `protocol` and an entrypoint file `bin/engine`.
fn write_override_pack(dir: &Path, protocol: Option<&str>) {
    let builder = PackBuilder::standard("pdfium", "0.1.0").engine_protocol(protocol);
    let key = Key::generate();
    builder.write_unscanned(dir, &key);
    fs::create_dir_all(dir.join("bin")).expect("bin directory");
    fs::write(dir.join("bin/engine"), b"#!/bin/sh\nexit 0\n").expect("entrypoint");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join("bin/engine"), fs::Permissions::from_mode(0o755))
            .expect("executable");
    }
}

#[test]
fn resolving_a_development_pack_with_the_wrong_protocol_is_refused() {
    let targets = packs::host_targets();
    for declared in [None, Some("ariad-engine/1"), Some("ariad-engine/7")] {
        let dir = support::temp();
        write_override_pack(dir.path(), declared);
        let error = packs::resolve_override("pdfium", dir.path(), &targets)
            .expect_err("a mismatching manifest must not resolve");
        assert!(
            matches!(&error, PackError::InvalidManifest(message) if message.contains("engine_protocol")),
            "{declared:?}: {error:?}"
        );
    }
    let dir = support::temp();
    write_override_pack(dir.path(), Some("ariad-engine/2"));
    packs::resolve_override("pdfium", dir.path(), &targets).expect("a matching manifest resolves");
}

#[test]
fn the_engine_resolver_reports_a_protocol_mismatch_as_tool_missing_exit_5() {
    let dir = support::temp();
    let data = support::temp();
    write_override_pack(dir.path(), None);
    if !in_child(
        "the_engine_resolver_reports_a_protocol_mismatch_as_tool_missing_exit_5",
        |command| {
            command
                .env("ASHIFT_PACK_PDFIUM", dir.path())
                .env("ASHIFT_DATA_DIR", data.path());
        },
    ) {
        return;
    }
    let error = engines::resolve_pack("pdfium").expect_err("mismatching pack must not resolve");
    assert!(
        matches!(&error, ResolveError::VerificationFailed { reason, .. } if reason.contains("engine_protocol")),
        "{error:?}"
    );
    assert_eq!(error.code(), "tool_missing");
    assert_eq!(error.exit_code(), 5);
    assert!(
        error
            .hint()
            .is_some_and(|hint| hint.contains("ashift engines install pdfium"))
    );
}

#[cfg(unix)]
#[test]
fn an_entrypoint_that_leaves_the_pack_root_is_refused() {
    let dir = support::temp();
    let data = support::temp();
    write_override_pack(dir.path(), Some("ariad-engine/2"));
    let outside = support::temp();
    let target = outside.path().join("engine");
    fs::write(&target, b"#!/bin/sh\nexit 0\n").expect("outside program");
    fs::remove_file(dir.path().join("bin/engine")).expect("remove entrypoint");
    std::os::unix::fs::symlink(&target, dir.path().join("bin/engine")).expect("symlink");
    if !in_child(
        "an_entrypoint_that_leaves_the_pack_root_is_refused",
        |command| {
            command
                .env("ASHIFT_PACK_PDFIUM", dir.path())
                .env("ASHIFT_DATA_DIR", data.path());
        },
    ) {
        return;
    }
    let error = engines::resolve_pack("pdfium").expect_err("escaping entrypoint must not resolve");
    assert!(
        matches!(&error, ResolveError::VerificationFailed { reason, .. }
            if reason.contains("outside the pack root")),
        "{error:?}"
    );
    assert_eq!(error.exit_code(), 5);
}

#[test]
fn a_missing_pdfium_pack_fails_every_pdfium_tool_with_exit_5_and_the_install_hint() {
    use ariad_core::tools::{InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec};
    use ariad_host::tools::{InputRef, OutputTarget, ToolError, ToolRequest, run_tool};

    fn empty_schema() -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false
        }))
        .expect("empty schema")
    }

    let data = support::temp();
    if !in_child(
        "a_missing_pdfium_pack_fails_every_pdfium_tool_with_exit_5_and_the_install_hint",
        |command| {
            command.env("ASHIFT_DATA_DIR", data.path());
        },
    ) {
        return;
    }

    let resolve = engines::resolve_pack("pdfium").expect_err("no pack is installed");
    assert!(
        matches!(resolve, ResolveError::NotInstalled { .. }),
        "{resolve:?}"
    );
    assert_eq!(resolve.exit_code(), 5);

    let work = support::temp();
    let input = work.path().join("in.pdf");
    fs::write(&input, b"%PDF-1.7\n1 0 obj <<>> endobj\n").expect("input pdf");
    let hint = engines::PDFIUM.install_hint;
    for &id in PDFIUM_TOOLS {
        let spec = ToolSpec {
            id,
            version: "1",
            group: ToolGroup::Convert,
            summary: "A pdfium tool.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "png",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "pdfium",
            params_schema: empty_schema,
            secret_params: &[],
        };
        let request =
            ToolRequest::new(id, vec![InputRef::new(&input)], OutputTarget::None).with_spec(spec);
        let error = run_tool(&request).expect_err("a missing pack must fail the tool");
        assert!(
            matches!(&error, ToolError::ToolMissing { engine, hint: shown }
                if engine == "pdfium" && shown == hint),
            "{id}: {error:?}"
        );
        assert_eq!(error.exit_code(), 5, "{id}");
        assert_eq!(error.code(), "tool_missing", "{id}");
        assert!(hint.contains("ashift engines install pdfium"), "{hint}");
    }
}

#[cfg(feature = "test-probe")]
mod with_probe {
    use std::path::PathBuf;

    use super::*;

    use ariad_core::protocol::v2::PROTOCOL_V2;
    use ariad_host::commands::{
        active_capabilities,
        doctor::doctor,
        engines::{EngineStatus, engines as engine_rows},
    };

    fn probe() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"))
    }

    /// An unpacked development pack whose engine is the probe answering as `pdfium`.
    fn probe_pack(dir: &Path) {
        let builder = PackBuilder::standard("pdfium", "0.1.0");
        let key = Key::generate();
        builder.write_unscanned(dir, &key);
        fs::create_dir_all(dir.join("bin")).expect("bin directory");
        fs::copy(probe(), dir.join("bin/engine-probe")).expect("copy the probe");
        let manifest = fs::read_to_string(dir.join(packs::MANIFEST_FILE)).expect("manifest");
        fs::write(
            dir.join(packs::MANIFEST_FILE),
            manifest.replace(
                "entrypoint = [\"bin/engine\"]",
                "entrypoint = [\"bin/engine-probe\", \"__engine\", \"pdfium\"]",
            ),
        )
        .expect("manifest with the probe entrypoint");
    }

    #[test]
    fn doctor_and_engines_show_pdfium_as_not_installed_with_the_hint() {
        let data = support::temp();
        if !in_child(
            "with_probe::doctor_and_engines_show_pdfium_as_not_installed_with_the_hint",
            |command| {
                command.env("ASHIFT_DATA_DIR", data.path());
            },
        ) {
            return;
        }
        let hint = engines::PDFIUM.install_hint;

        let report = doctor(&probe(), CancellationToken::new()).expect("doctor runs");
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "pdfium_engine")
            .expect("doctor lists the pdfium engine");
        assert!(!check.ok);
        assert!(
            !check.required,
            "a missing optional pack never fails doctor"
        );
        assert!(check.message.contains("not installed"), "{}", check.message);
        assert_eq!(check.hint, hint);

        let caps = active_capabilities().expect("capabilities");
        let rows = engine_rows(&probe(), &caps, CancellationToken::new()).expect("engine rows");
        let row = rows
            .iter()
            .find(|row| row.id == "pdfium")
            .expect("engines lists pdfium");
        assert_eq!(row.status, EngineStatus::NotInstalled);
        assert_eq!(row.version, "-");
        assert_eq!(row.license, engines::PDFIUM.license);
        assert_eq!(row.note.as_deref(), Some(hint));
        assert!(row.pack.is_none());
    }

    #[test]
    fn doctor_and_engines_show_the_installed_pdfium_version_and_protocol() {
        let pack = support::temp();
        let data = support::temp();
        probe_pack(pack.path());
        if !in_child(
            "with_probe::doctor_and_engines_show_the_installed_pdfium_version_and_protocol",
            |command| {
                command
                    .env("ASHIFT_PACK_PDFIUM", pack.path())
                    .env("ASHIFT_DATA_DIR", data.path())
                    // Isolation is covered by the sandbox tests; this one checks the report.
                    .env("ASHIFT_SANDBOX", "off");
            },
        ) {
            return;
        }

        let report = doctor(&probe(), CancellationToken::new()).expect("doctor runs");
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "pdfium_engine")
            .expect("doctor lists the pdfium engine");
        assert!(check.ok, "{}", check.message);
        assert!(check.hint.is_empty());
        assert!(
            check.message.contains(env!("CARGO_PKG_VERSION"))
                && check.message.contains(PROTOCOL_V2),
            "{}",
            check.message
        );

        let caps = active_capabilities().expect("capabilities");
        let rows = engine_rows(&probe(), &caps, CancellationToken::new()).expect("engine rows");
        let row = rows
            .iter()
            .find(|row| row.id == "pdfium")
            .expect("engines lists pdfium");
        assert_eq!(row.status, EngineStatus::Ready);
        assert_eq!(row.version, "0.1.0");
        assert_eq!(row.note.as_deref(), Some("protocol ariad-engine/2"));
        let pack = row.pack.as_ref().expect("the row names its pack");
        assert_eq!(
            (pack.name.as_str(), pack.source.as_str()),
            ("pdfium", "override")
        );
    }
}
