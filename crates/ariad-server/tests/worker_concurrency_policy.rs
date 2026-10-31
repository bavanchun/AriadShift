//! Real worker startup must reject parallel best-effort without filesystem isolation.
mod common;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[tokio::test]
async fn process_startup_refuses_parallel_best_effort_without_filesystem_support() {
    let Some(db) = common::TestDb::create("worker_concurrency_startup").await else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ariad-server"))
        .arg("worker")
        .env_clear()
        .env("DATABASE_URL", &db.url)
        .env("ARIAD_WORK_DIR", dir.path())
        .env("ARIAD_ENGINE_PROGRAM", "ashift")
        .env(
            "ARIAD_STORAGE_URL",
            url::Url::from_directory_path(dir.path()).unwrap().as_str(),
        )
        .env("ARIAD_SANDBOX", "best-effort")
        .env("ARIAD_WORKER_CONCURRENCY", "2")
        .env("ARIAD_TEST_SANDBOX_SUPPORT", "none")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr.try_clone().unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    use std::io::{Read, Seek};
    let mut file = stderr;
    file.rewind().unwrap();
    let mut diagnostic = String::new();
    file.read_to_string(&mut diagnostic).unwrap();
    db.drop_database().await;
    let status = status.expect("unsafe parallel worker kept running without filesystem isolation");
    assert!(!status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("ARIAD_WORKER_CONCURRENCY") && diagnostic.contains("filesystem"),
        "{diagnostic}"
    );
}

use ariad_host::sandbox::SandboxSupport;
use ariad_server::{
    config::{Env, WorkerConfig},
    sandbox::{self, SandboxMode},
};

#[test]
fn missing_filesystem_support_refuses_parallel_best_effort() {
    let unsupported = SandboxSupport::default();
    // Use the same public policy as real worker startup; no runtime-name heuristic.
    assert!(
        sandbox::preflight_worker_with_support(SandboxMode::BestEffort, 2, &unsupported).is_err()
    );
    assert!(
        sandbox::preflight_worker_with_support(SandboxMode::BestEffort, 1, &unsupported).is_ok()
    );
    assert!(
        sandbox::preflight_worker_with_support(SandboxMode::Required, 1, &unsupported).is_err()
    );
    assert!(sandbox::preflight_worker_with_support(SandboxMode::Off, 2, &unsupported).is_ok());
}

#[test]
fn supported_native_parallelism_and_positive_config_survive() {
    let support = SandboxSupport {
        platform: true,
        landlock_filesystem: true,
        seccomp: true,
        #[cfg(target_os = "macos")]
        seatbelt_available: true,
        #[cfg(target_os = "macos")]
        seatbelt_usable: true,
        ..Default::default()
    };
    for mode in [
        SandboxMode::Required,
        SandboxMode::BestEffort,
        SandboxMode::Off,
    ] {
        assert!(sandbox::preflight_worker_with_support(mode, 2, &support).is_ok());
    }
    let work_dir = std::env::temp_dir().join("ariad-concurrency-policy");
    let storage = url::Url::from_directory_path(&work_dir)
        .unwrap()
        .to_string();
    let env = Env::from_pairs([
        ("ARIAD_STORAGE_URL", storage.as_str()),
        ("ARIAD_ENGINE_PROGRAM", "ashift"),
        ("ARIAD_WORK_DIR", work_dir.to_str().unwrap()),
        ("ARIAD_WORKER_CONCURRENCY", "2"),
        ("ARIAD_SANDBOX", "best-effort"),
    ]);
    assert_eq!(WorkerConfig::from_env(&env, &[]).unwrap().concurrency, 2);
}

#[test]
fn filesystem_support_is_distinct_from_network_support() {
    let mut support = SandboxSupport {
        platform: true,
        seccomp: true,
        ..Default::default()
    };
    assert!(sandbox::preflight_worker_with_support(SandboxMode::BestEffort, 2, &support).is_err());
    #[cfg(target_os = "macos")]
    {
        support.seatbelt_available = true;
        support.seatbelt_usable = true;
    }
    support.landlock_filesystem = true;
    support.seccomp = false;
    assert!(sandbox::preflight_worker_with_support(SandboxMode::BestEffort, 2, &support).is_ok());
    #[cfg(not(target_os = "macos"))]
    assert!(sandbox::preflight_worker_with_support(SandboxMode::Required, 2, &support).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn real_worker_startup_preserves_serial_off_and_native_supported_modes() {
    let Some(db) = common::TestDb::create("worker_concurrency_controls").await else {
        return;
    };
    let supported = ariad_host::sandbox::support();
    let mut cases = vec![("best-effort", "1", true), ("off", "2", true)];
    #[cfg(target_os = "linux")]
    if supported.landlock_filesystem {
        cases.push(("best-effort", "2", false));
    }
    #[cfg(target_os = "macos")]
    if supported.seatbelt_available && supported.seatbelt_usable {
        cases.push(("best-effort", "2", false));
    }
    for (mode, concurrency, simulate_missing) in cases {
        eprintln!(
            "startup control: mode={mode}, concurrency={concurrency}, simulated_missing={simulate_missing}"
        );
        let work = tempfile::tempdir().unwrap();
        let stderr = tempfile::tempfile().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_ariad-server"));
        command
            .arg("worker")
            .env_clear()
            .env("DATABASE_URL", &db.url)
            .env("ARIAD_WORK_DIR", work.path())
            .env("ARIAD_ENGINE_PROGRAM", common::ashift())
            .env(
                "ARIAD_STORAGE_URL",
                url::Url::from_directory_path(work.path()).unwrap().as_str(),
            )
            .env("ARIAD_SANDBOX", mode)
            .env("ARIAD_WORKER_CONCURRENCY", concurrency)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr.try_clone().unwrap());
        if simulate_missing {
            command.env("ARIAD_TEST_SANDBOX_SUPPORT", "none");
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut diagnostic: String;
        let started = loop {
            use std::os::unix::fs::FileExt;
            let mut buffer = [0_u8; 65536];
            let length = stderr.read_at(&mut buffer, 0).unwrap();
            diagnostic = String::from_utf8_lossy(&buffer[..length]).into_owned();
            if diagnostic.contains("worker started") {
                break true;
            }
            if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(i32::try_from(child.id()).unwrap()),
            nix::sys::signal::Signal::SIGTERM,
        )
        .ok();
        let stop_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= stop_deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(started, "{mode}/{concurrency}: {diagnostic}");
    }
    db.drop_database().await;
}
