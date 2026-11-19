//! Sandbox diagnostics must originate in the host and survive CLI rendering.

use std::process::{Command, Output};

use serde_json::Value;

fn doctor(mode: &str, support: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_SANDBOX", mode)
        .env_remove("ARIAD_TEST_SANDBOX_SUPPORT");
    if let Some(support) = support {
        command.env("ARIAD_TEST_SANDBOX_SUPPORT", support);
    }
    command
        .args(["doctor", "--json"])
        .output()
        .expect("doctor output")
}

fn sandbox_check(report: &Value) -> &Value {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "sandbox")
        .unwrap()
}

#[test]
fn off_mode_explicitly_reports_engines_without_isolation() {
    let output = doctor("off", None);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        sandbox_check(&report)["message"],
        "engine sandbox: off (ASHIFT_SANDBOX=off); engines run without isolation"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(report["sandbox"]["seatbelt_enforced"], false);
}

#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn macos_diagnostics_report_seatbelt_and_refusal_as_a_required_failure() {
    for mode in ["best-effort", "required"] {
        let output = doctor(mode, Some("seatbelt-refused"));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(5));
        let check = sandbox_check(&report);
        assert_eq!(check["required"], true);
        assert_eq!(check["ok"], false);
        assert!(check["message"].as_str().unwrap().contains("refused"));
        assert!(!check["message"].as_str().unwrap().contains("Landlock"));
        assert!(!check["hint"].as_str().unwrap().contains("Linux"));
        assert_eq!(report["sandbox"]["seatbelt_available"], true);
        assert_eq!(report["sandbox"]["seatbelt_usable"], false);
        assert_eq!(report["sandbox"]["seatbelt_enforced"], false);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_best_effort_reports_platform_fields() {
    let output = doctor("best-effort", None);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    for key in ["seatbelt_available", "seatbelt_usable", "seatbelt_enforced"] {
        assert!(report["sandbox"][key].is_boolean(), "{key}");
    }
    assert!(
        sandbox_check(&report)["message"]
            .as_str()
            .unwrap()
            .starts_with("engine sandbox: best-effort; Seatbelt ")
    );
}

#[cfg(debug_assertions)]
#[test]
fn host_diagnostics_preserve_mode_and_platform() {
    const SCENARIO: &str = "ARIAD_TEST_DOCTOR_SCENARIO";
    let Ok(scenario) = std::env::var(SCENARIO) else {
        let mut scenarios = vec![("off", "none"), ("required", "none")];
        if cfg!(target_os = "macos") {
            scenarios.extend([("best-effort", "none"), ("best-effort", "seatbelt-refused")]);
        }
        for (mode, support) in scenarios {
            let status = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "host_diagnostics_preserve_mode_and_platform"])
                .env(SCENARIO, format!("{mode}:{support}"))
                .env("ASHIFT_SANDBOX", mode)
                .env("ARIAD_TEST_SANDBOX_SUPPORT", support)
                .status()
                .unwrap();
            assert!(status.success(), "{mode}:{support}");
        }
        return;
    };
    let (mode, support) = scenario.split_once(':').unwrap();
    let report = ariad_host::commands::doctor::doctor(
        std::path::Path::new(env!("CARGO_BIN_EXE_ashift")),
        tokio_util::sync::CancellationToken::new(),
    )
    .unwrap();
    let host = serde_json::to_value(&report).unwrap();
    let output = doctor(mode, Some(support));
    let cli: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(host["sandbox"], cli["sandbox"]);
    assert_eq!(sandbox_check(&host), sandbox_check(&cli));
    let check = sandbox_check(&host);
    assert_eq!(host["sandbox"]["mode"], mode);
    if mode == "off" {
        assert_eq!(
            check["message"],
            "engine sandbox: off (ASHIFT_SANDBOX=off); engines run without isolation"
        );
        assert_eq!(check["ok"], true);
        assert_eq!(check["required"], false);
    } else if mode == "required" || support == "seatbelt-refused" {
        assert_eq!(check["ok"], false);
        assert_eq!(check["required"], true);
    } else {
        assert_eq!(check["ok"], false);
        assert_eq!(check["required"], false);
    }
    #[cfg(target_os = "macos")]
    {
        for key in ["seatbelt_available", "seatbelt_usable", "seatbelt_enforced"] {
            assert!(host["sandbox"][key].is_boolean(), "{key}");
        }
        assert_eq!(host["sandbox"]["seatbelt_enforced"], false);
        if mode != "off" {
            assert!(check["message"].as_str().unwrap().contains("Seatbelt"));
            assert!(!check["message"].as_str().unwrap().contains("Landlock"));
            assert!(!check["hint"].as_str().unwrap().contains("Linux"));
        }
    }
}

#[cfg(windows)]
#[test]
fn required_mode_names_supported_sandbox_platforms() {
    let mut command = ariad_host::runner::EngineCommand::builtin(
        std::path::Path::new(env!("CARGO_BIN_EXE_ashift")),
        "pandoc",
    );
    command.sandbox = Some(ariad_host::sandbox::SandboxMode::Required);
    let error = ariad_host::runner::describe_command(
        &command,
        ariad_host::runner::DESCRIBE_TIMEOUT,
        tokio_util::sync::CancellationToken::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error, ariad_host::runner::RunError::SandboxUnavailable(ref message)
        if message.contains("Linux and macOS")),
        "{error:?}"
    );
}
