//! macOS engine isolation through the same launcher and runner used by conversions.
#![cfg(target_os = "macos")]

use std::{
    collections::BTreeMap, ffi::OsString, fs, net::TcpListener, path::PathBuf, process::Command,
};

use ariad_core::{
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
};
#[cfg(debug_assertions)]
use ariad_host::runner::RunError;
use ariad_host::{
    runner::{self, EngineCommand},
    sandbox::{self, SandboxMode, SandboxOutcome, SandboxPolicy},
    workspace::Workspace,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn probe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"))
}

fn seatbelt_available() -> bool {
    let support = sandbox::support();
    if !support.seatbelt_available {
        eprintln!("skipping Seatbelt enforcement: /usr/bin/sandbox-exec is unavailable");
        return false;
    }
    assert!(
        support.seatbelt_usable,
        "sandbox-exec exists but could not enforce the profile: {support:?}"
    );
    true
}

fn engine(mode: SandboxMode) -> EngineCommand {
    EngineCommand {
        engine: "probe".to_owned(),
        program: probe(),
        args: vec!["sandbox-check".into()],
        env: Vec::new(),
        install_root: None,
        launcher: Some(probe()),
        sandbox: Some(mode),
    }
}

fn request(workspace: &Workspace, options: BTreeMap<String, Value>) -> Request {
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "seatbelt".to_owned(),
        input: Input {
            path: workspace
                .input_dir()
                .join("input.bin")
                .display()
                .to_string(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options,
        limits: Limits::local(),
        session_id: None,
    }
}

#[test]
fn seatbelt_confines_workspace_installation_and_network() {
    if !seatbelt_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let quoted = root.path().join("job \" (allow default) ; \\ newline\n");
    fs::create_dir(&quoted).unwrap();
    let workspace = Workspace::new_in(&quoted).unwrap();
    let outside = root.path().join("outside.txt");
    let new_outside = root.path().join("must-not-be-created.txt");
    fs::write(&outside, b"original").unwrap();
    let install = root.path().join("engine \" (install)");
    fs::create_dir(&install).unwrap();
    let program = install.join("engine-probe");
    fs::copy(probe(), &program).unwrap();
    let data = install.join("data.txt");
    fs::write(&data, b"install data").unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let unix_path = root.path().join("host.sock");
    let _unix = std::os::unix::net::UnixListener::bind(&unix_path).unwrap();
    // A symlink created inside the writable workspace must not authorize its target.
    let escape = workspace.work_dir().join("symlink-escape");
    std::os::unix::fs::symlink(&outside, &escape).unwrap();
    let mut command = engine(SandboxMode::Required);
    command.program = program;
    command.install_root = Some(install.clone());
    for target in [&outside, &escape, &new_outside] {
        let options = BTreeMap::from([
            ("outside".to_owned(), json!(target)),
            ("read".to_owned(), json!(data)),
            ("write_install".to_owned(), json!(data)),
            (
                "port".to_owned(),
                json!(listener.local_addr().unwrap().port()),
            ),
            ("unix".to_owned(), json!(unix_path)),
        ]);
        let outcome = runner::run_command(
            &command,
            &request(&workspace, options),
            CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(outcome.sandbox, SandboxOutcome::Applied);
        let metrics = outcome.metrics.unwrap();
        for name in [
            "write_outside",
            "write_install_root",
            "tcp_connect",
            "udp_socket",
            "unix_connect",
        ] {
            assert_eq!(metrics[name], "denied", "{name}");
        }
        for name in ["read_install_root", "write_workspace", "exec_self"] {
            assert_eq!(metrics[name], "allowed", "{name}");
        }
    }
    assert_eq!(fs::read(&outside).unwrap(), b"original");
    assert!(!new_outside.exists());
    assert_eq!(fs::read(&data).unwrap(), b"install data");
}

#[test]
fn seatbelt_keeps_ungranted_document_contents_unreadable() {
    if !seatbelt_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("host-document");
    fs::write(&outside, b"private contents").unwrap();
    let workspace = Workspace::new().unwrap();
    let command = engine(SandboxMode::Required);
    let data_volume_alias =
        PathBuf::from("/System/Volumes/Data").join(outside.strip_prefix("/").unwrap());
    for target in [&outside, &data_volume_alias]
        .into_iter()
        .filter(|path| path.exists())
    {
        let outcome = runner::run_command(
            &command,
            &request(
                &workspace,
                BTreeMap::from([("read".to_owned(), json!(target))]),
            ),
            CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(outcome.metrics.unwrap()["read_install_root"], "denied");
    }
}

#[test]
fn seatbelt_allows_a_real_pandoc_conversion_in_a_quoted_job_directory() {
    if !seatbelt_available() {
        return;
    }
    let pandoc = ariad_host::pandoc_bin::resolve_path()
        .expect("install the supported Pandoc with just pandoc before this test")
        .canonicalize()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("conversion \" (job) \\ newline\n");
    fs::create_dir(&directory).unwrap();
    let input = directory.join("input.md");
    let output = directory.join("output.html");
    fs::write(&input, "# Seatbelt conversion\n\nNormal **content**.\n").unwrap();
    let args = vec![
        OsString::from("--sandbox"),
        "-f".into(),
        "markdown".into(),
        "-t".into(),
        "html".into(),
        input.into_os_string(),
        "-o".into(),
        output.clone().into_os_string(),
    ];
    let policy = SandboxPolicy::for_engine("pandoc", &pandoc, std::slice::from_ref(&directory));
    let result = Command::new(probe())
        .args(policy.launcher_args(true, &pandoc, &args))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TMPDIR", &directory)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let html = fs::read_to_string(output).unwrap();
    assert!(html.contains("Seatbelt conversion") && html.contains("<strong>content</strong>"));
}

#[test]
fn launcher_disables_core_dumps_and_reports_profile_setup_errors() {
    if !seatbelt_available() {
        return;
    }
    let policy = SandboxPolicy::for_engine("probe", std::path::Path::new("/bin/sh"), &[]);
    let output = Command::new(probe())
        .args(policy.launcher_args(
            true,
            std::path::Path::new("/bin/sh"),
            &["-c".into(), "ulimit -c".into()],
        ))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
    let root = tempfile::tempdir().unwrap();
    let invalid = SandboxPolicy {
        read_write: vec![root.path().join("missing-workspace")],
        ..policy
    };
    let output = Command::new(probe())
        .args(invalid.launcher_args(false, std::path::Path::new("/bin/true"), &[]))
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(i32::from(sandbox::LAUNCH_FAILURE_EXIT))
    );
    let event: ariad_core::protocol::Event = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        matches!(event, ariad_core::protocol::Event::Result { ok: false, error: Some(ref error), .. } if error.message.starts_with(sandbox::LAUNCH_FAILURE_PREFIX))
    );
}

#[test]
#[cfg(debug_assertions)]
fn missing_seatbelt_degrades_best_effort_and_refuses_required_mode() {
    const CHILD: &str = "ARIAD_TEST_MACOS_MISSING_SEATBELT";
    if std::env::var_os(CHILD).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "missing_seatbelt_degrades_best_effort_and_refuses_required_mode",
            ])
            .env(CHILD, "1")
            .env("ARIAD_TEST_SANDBOX_SUPPORT", "none")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let workspace = Workspace::new().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("unconfined");
    let request = request(
        &workspace,
        BTreeMap::from([("outside".to_owned(), json!(target))]),
    );
    let mut command = engine(SandboxMode::BestEffort);
    command
        .env
        .push(("ARIAD_TEST_SANDBOX_SUPPORT".into(), "none".into()));
    let outcome =
        runner::run_command(&command, &request, CancellationToken::new(), |_| {}).unwrap();
    assert!(
        matches!(outcome.sandbox, SandboxOutcome::Degraded(ref gap) if gap.contains("Seatbelt"))
    );
    assert!(target.exists());
    command.sandbox = Some(SandboxMode::Required);
    let error =
        runner::run_command(&command, &request, CancellationToken::new(), |_| {}).unwrap_err();
    assert!(matches!(error, RunError::SandboxUnavailable(_)));
    command.sandbox = Some(SandboxMode::Off);
    let outcome =
        runner::run_command(&command, &request, CancellationToken::new(), |_| {}).unwrap();
    assert_eq!(outcome.sandbox, SandboxOutcome::Off);
}

#[test]
fn engines_can_read_the_system_identity() {
    if !seatbelt_available() {
        return;
    }
    let uname = std::path::Path::new("/usr/bin/uname");
    let policy = SandboxPolicy::for_engine("probe", uname, &[]);
    let output = Command::new(probe())
        .args(policy.launcher_args(true, uname, &["-a".into()]))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("Darwin "));
}

#[test]
fn seatbelt_keeps_package_manager_state_unreadable() {
    if !seatbelt_available() {
        return;
    }
    let samples: Vec<PathBuf> = [
        "/opt/homebrew/var",
        "/opt/homebrew/etc",
        "/usr/local/var",
        "/usr/local/etc",
    ]
    .into_iter()
    .filter_map(|dir| readable_file_under(std::path::Path::new(dir), 3))
    .collect();
    if samples.is_empty() {
        eprintln!("skipping: no package-manager state on this Mac");
        return;
    }
    let workspace = Workspace::new().unwrap();
    let command = engine(SandboxMode::Required);
    for target in &samples {
        let outcome = runner::run_command(
            &command,
            &request(
                &workspace,
                BTreeMap::from([("read".to_owned(), json!(target))]),
            ),
            CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            outcome.metrics.unwrap()["read_install_root"],
            "denied",
            "{}",
            target.display()
        );
    }
}

fn readable_file_under(dir: &std::path::Path, depth: usize) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file() && fs::File::open(&path).is_ok() {
            return Some(path);
        }
        if file_type.is_dir()
            && depth > 0
            && let Some(found) = readable_file_under(&path, depth - 1)
        {
            return Some(found);
        }
    }
    None
}

#[test]
fn libreoffice_ipc_stays_in_a_private_directory_and_is_cleaned_up() {
    const CHILD: &str = "ARIAD_TEST_MACOS_IPC_PERMISSIONS";
    if std::env::var_os(CHILD).is_none() {
        let status = Command::new("/bin/sh")
            .args(["-c", "umask 000; exec \"$@\"", "sh"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "libreoffice_ipc_stays_in_a_private_directory_and_is_cleaned_up",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    if !seatbelt_available() {
        return;
    }
    if !ariad_host::engines::sandbox::paths_for("libreoffice").private_tmp {
        eprintln!("skipping: install LibreOffice to exercise its IPC policy");
        return;
    }
    let workspace = Workspace::new().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let host_socket = outside.path().join("host.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&host_socket).unwrap();
    let workspace_socket = workspace.work_dir().join("workspace.sock");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let options = BTreeMap::from([
        ("ipc".to_owned(), json!(true)),
        ("unix".to_owned(), json!(host_socket)),
        ("bind_unix".to_owned(), json!(workspace_socket)),
        (
            "port".to_owned(),
            json!(listener.local_addr().unwrap().port()),
        ),
    ]);
    for id in ["probe", "libreoffice"] {
        let mut command = engine(SandboxMode::Required);
        command.engine = id.to_owned();
        let outcome = runner::run_command(
            &command,
            &request(&workspace, options.clone()),
            CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(outcome.sandbox, SandboxOutcome::Applied);
        let metrics = outcome.metrics.unwrap();
        for name in ["unix_connect", "unix_bind", "tcp_connect", "udp_socket"] {
            assert_eq!(metrics[name], "denied", "{id}: {name}");
        }
        let expected = if id == "libreoffice" {
            "allowed"
        } else {
            "denied"
        };
        assert_eq!(metrics["ipc_bind"], expected, "{id}");
        assert_eq!(metrics["ipc_connect"], expected, "{id}");
        if id == "libreoffice" {
            assert_eq!(
                metrics["ipc_mode"], 0o700,
                "IPC must be owner-only at creation"
            );
            assert_eq!(metrics["ipc_symlink_connect"], "denied");
            let ipc = PathBuf::from(metrics["ipc_dir"].as_str().unwrap());
            assert!(ipc.starts_with("/private/tmp"));
            assert!(!ipc.starts_with(workspace.root()));
            assert!(
                !ipc.exists(),
                "IPC directory is removed after the process exits"
            );
        }
    }
}

#[test]
fn libreoffice_profile_failure_never_retries_without_isolation() {
    assert_libreoffice_profile_failure_is_path_free(false);
}

#[test]
fn libreoffice_profile_failure_with_writable_ipc_has_a_path_free_diagnostic() {
    assert_libreoffice_profile_failure_is_path_free(true);
}

fn assert_libreoffice_profile_failure_is_path_free(writable_ipc: bool) {
    if !seatbelt_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("unconfined");
    let shell = std::path::Path::new("/bin/sh");
    let ipc = root.path().join("missing-ipc");
    let writable = if writable_ipc {
        vec![ipc.clone()]
    } else {
        Vec::new()
    };
    let policy = SandboxPolicy {
        private_tmp: Some(ipc.clone()),
        ..SandboxPolicy::for_engine("probe", shell, &writable)
    };
    let output = Command::new(probe())
        .args(policy.launcher_args(
            false,
            shell,
            &[
                "-c".into(),
                "printf unconfined > \"$1\"".into(),
                "sh".into(),
                marker.clone().into_os_string(),
            ],
        ))
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(i32::from(sandbox::LAUNCH_FAILURE_EXIT))
    );
    let event: ariad_core::protocol::Event = serde_json::from_slice(&output.stdout).unwrap();
    let ariad_core::protocol::Event::Result {
        ok: false,
        error: Some(error),
        ..
    } = event
    else {
        panic!("expected a typed launch refusal");
    };
    assert_eq!(error.code, ariad_core::protocol::ErrorCode::EngineFailure);
    let detail = "could not resolve the Seatbelt IPC directory";
    assert_eq!(
        error.message,
        format!("{}{detail}", sandbox::LAUNCH_FAILURE_PREFIX)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("ashift sandbox: {detail}\n")
    );
    let public_error = ariad_host::convert::ConvertError::SandboxUnavailable(error.message);
    assert_eq!(public_error.code(), "sandbox_unavailable");
    let diagnostic = public_error.to_string();
    for text in [
        String::from_utf8_lossy(&output.stdout).as_ref(),
        String::from_utf8_lossy(&output.stderr).as_ref(),
        &diagnostic,
    ] {
        assert!(!text.contains(ipc.to_str().unwrap()));
        assert!(!text.contains(root.path().to_str().unwrap()));
        assert!(!text.contains("missing-ipc"));
    }
    assert!(!marker.exists());
}

#[test]
fn libreoffice_ipc_creation_failure_has_a_path_free_diagnostic() {
    const CHILD: &str = "ARIAD_TEST_IPC_CREATION_FAILURE";
    if std::env::var_os(CHILD).is_none() {
        if !seatbelt_available() {
            return;
        }
        if !ariad_host::engines::sandbox::paths_for("libreoffice").private_tmp {
            eprintln!("skipping: install LibreOffice to exercise its IPC setup");
            return;
        }
        let status = Command::new("/usr/bin/sandbox-exec")
            .args([
                "-p",
                "(version 1)(allow default)(deny file-write-create (subpath \"/private/tmp\"))",
            ])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "libreoffice_ipc_creation_failure_has_a_path_free_diagnostic",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    assert!(ariad_host::engines::sandbox::paths_for("libreoffice").private_tmp);
    let workspace = Workspace::new().unwrap();
    let marker = workspace.work_dir().join("must-not-run");
    let mut command = engine(SandboxMode::BestEffort);
    command.engine = "libreoffice".to_owned();
    let error = runner::run_command(
        &command,
        &request(
            &workspace,
            BTreeMap::from([("outside".to_owned(), json!(marker))]),
        ),
        CancellationToken::new(),
        |_| {},
    )
    .unwrap_err();
    assert!(!marker.exists());
    let ariad_host::runner::RunError::SandboxUnavailable(detail) = &error else {
        panic!("expected a typed IPC setup refusal: {error}");
    };
    assert_eq!(detail, "could not create the LibreOffice IPC directory");
    let diagnostic =
        ariad_host::convert::ConvertError::SandboxUnavailable(detail.clone()).to_string();
    for text in [error.to_string(), diagnostic] {
        assert!(!text.contains("/private/tmp"));
        assert!(!text.contains("as-ipc-"));
        assert!(!text.contains(workspace.root().to_str().unwrap()));
    }
}

#[test]
fn relative_builtin_tool_overrides_survive_the_cwd_change() {
    const CHILD: &str = "ARIAD_TEST_RELATIVE_TOOL";
    let Some(id) = std::env::var_os(CHILD) else {
        let pandoc = ariad_host::pandoc_bin::locate().expect("install Pandoc with just pandoc");
        let mut tools = vec![("pandoc", "ASHIFT_PANDOC", pandoc.path)];
        if let Ok(office) = ariad_host::engines::libreoffice::locate() {
            tools.push(("libreoffice", "ASHIFT_SOFFICE", office.path));
        } else {
            eprintln!("skipping Office override: install LibreOffice >=24.2");
        }
        let mut success = true;
        for (id, key, program) in tools {
            let program = program.canonicalize().unwrap();
            let relative = program.file_name().unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "relative_builtin_tool_overrides_survive_the_cwd_change",
                ])
                .current_dir(program.parent().unwrap())
                .env(CHILD, id)
                .env(key, relative)
                .status()
                .unwrap();
            success &= status.success();
        }
        assert!(success);
        return;
    };
    if !seatbelt_available() {
        return;
    }
    let id = id.to_str().unwrap();
    let mut command = EngineCommand::builtin(&probe(), id);
    command.sandbox = Some(SandboxMode::Required);
    let event =
        runner::describe_command(&command, runner::DESCRIBE_TIMEOUT, CancellationToken::new())
            .unwrap();
    let ariad_core::protocol::Event::Capabilities { tool, .. } = event else {
        panic!("expected capabilities");
    };
    assert_eq!(
        tool.status,
        ariad_core::protocol::ToolAvailability::Found,
        "{id}: {tool:?}"
    );
    assert!(tool.version.is_some());
}

#[test]
fn relative_docling_virtualenv_keeps_its_runtime_grant() {
    const CHILD: &str = "ARIAD_TEST_RELATIVE_DOCLING";
    if std::env::var_os(CHILD).is_none() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tools/docling/python-env")
            .canonicalize();
        let Some(venv) = venv.ok().filter(|root| root.join("bin/python").is_file()) else {
            eprintln!("skipping: install the development Docling environment");
            return;
        };
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "relative_docling_virtualenv_keeps_its_runtime_grant",
            ])
            .current_dir(venv)
            .env(CHILD, "1")
            .env("ASHIFT_DOCLING_ENGINE", "bin/python -I -m ariad_docling")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    if !seatbelt_available() {
        return;
    }
    let mut command = ariad_host::engines::docling::resolve()
        .unwrap()
        .runner_command(&probe());
    assert!(command.program.is_relative());
    let workspace = Workspace::new().unwrap();
    for writable in [vec![], vec![workspace.work_dir().to_path_buf()]] {
        let policy = SandboxPolicy::for_engine("docling", &command.program, &writable);
        let output = Command::new(probe())
            .args(policy.launcher_args(
                true,
                &command.program,
                &[
                    "-I".into(),
                    "-c".into(),
                    "import os; print(os.getcwd())".into(),
                ],
            ))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let expected = writable
            .first()
            .map_or_else(|| PathBuf::from("/"), |path| path.canonicalize().unwrap());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            expected.to_str().unwrap()
        );
    }
    command.sandbox = Some(SandboxMode::Required);
    let event =
        runner::describe_command(&command, runner::DESCRIBE_TIMEOUT, CancellationToken::new())
            .unwrap();
    assert!(
        matches!(event, ariad_core::protocol::Event::Capabilities { ref engine, .. } if engine == "docling")
    );
}
