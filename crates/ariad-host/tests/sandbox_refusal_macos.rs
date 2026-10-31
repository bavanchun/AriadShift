//! Refused Seatbelt must never execute an engine, including through a direct launcher.
#![cfg(all(target_os = "macos", debug_assertions))]

use ariad_core::{
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    runner::{self, EngineCommand, RunError},
    sandbox::{self, SandboxMode, SandboxOutcome, SandboxPolicy},
    workspace::Workspace,
};
use std::{collections::BTreeMap, path::PathBuf, process::Command};
use tokio_util::sync::CancellationToken;

#[test]
fn refused_seatbelt_prevents_execution_in_both_modes() {
    const CHILD: &str = "ARIAD_TEST_REFUSAL_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "refused_seatbelt_prevents_execution_in_both_modes",
            ])
            .env(CHILD, "1")
            .env("ARIAD_TEST_SANDBOX_SUPPORT", "seatbelt-refused")
            .env_remove("ASHIFT_SANDBOX")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let support = sandbox::support();
    assert!(
        support.seatbelt_available && !support.seatbelt_usable,
        "{support:?}"
    );
    let workspace = Workspace::new().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let marker = outside.path().join("engine-executed");
    let probe = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let request = Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "refusal".to_owned(),
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
        options: BTreeMap::from([("outside".to_owned(), serde_json::json!(marker))]),
        limits: Limits::local(),
        session_id: None,
    };
    let mut command = EngineCommand {
        engine: "probe".to_owned(),
        program: probe.clone(),
        args: vec!["sandbox-check".into()],
        env: vec![(
            "ARIAD_TEST_SANDBOX_SUPPORT".into(),
            "seatbelt-refused".into(),
        )],
        install_root: None,
        launcher: Some(probe.clone()),
        sandbox: None,
    };
    for mode in [SandboxMode::BestEffort, SandboxMode::Required] {
        command.sandbox = Some(mode);
        for launcher in [Some(probe.clone()), None] {
            command.launcher = launcher;
            let error = runner::run_command(&command, &request, CancellationToken::new(), |_| {})
                .unwrap_err();
            assert!(
                matches!(error, RunError::SandboxUnavailable(ref text) if text.contains("refused")),
                "{error:?}"
            );
            assert!(!marker.exists());
        }
        let output = Command::new(&probe)
            .args(SandboxPolicy::default().launcher_args(
                mode == SandboxMode::Required,
                std::path::Path::new("/usr/bin/touch"),
                &[marker.clone().into_os_string()],
            ))
            .env("ARIAD_TEST_SANDBOX_SUPPORT", "seatbelt-refused")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(i32::from(sandbox::LAUNCH_FAILURE_EXIT))
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("simulated refusal"));
        assert!(!marker.exists());
    }
    command.sandbox = Some(SandboxMode::Off);
    let outcome =
        runner::run_command(&command, &request, CancellationToken::new(), |_| {}).unwrap();
    assert_eq!(outcome.sandbox, SandboxOutcome::Off);
    assert!(marker.is_file(), "off is an explicit operator bypass");
}
