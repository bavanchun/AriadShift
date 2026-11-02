//! The sandbox the host builds for the `pdfium` pack engine: the pack directory is read-only,
//! `/tmp` is private, the job directory is writable and the network is closed.
//!
//! The engine process is the `engine-probe` test helper running its `sandbox-check` action
//! under the real `pdfium` engine id, so the policy comes from `engines::sandbox` exactly as it
//! does for the installed pack. No PDFium library is involved.
//!
//! The Linux test runs everywhere the kernel offers Landlock and seccomp; each assertion that
//! needs a kernel feature checks `sandbox::support()` first. The macOS test needs the
//! Seatbelt launcher and only runs on macOS CI runners, never on the Linux development host.
#![cfg(all(unix, feature = "test-probe"))]

use std::{
    collections::BTreeMap, ffi::OsString, fs, net::TcpListener, os::unix::net::UnixListener,
    path::PathBuf,
};

use ariad_core::{
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    runner::{self, EngineCommand, RunOutcome},
    sandbox::{self, SandboxMode, SandboxOutcome},
    workspace::Workspace,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn probe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"))
}

/// A scratch directory outside `/tmp`, so a denied write cannot be mistaken for the private
/// `/tmp` swallowing it.
fn scratch() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("scratch directory")
}

fn request(workspace: &Workspace, options: BTreeMap<String, Value>) -> Request {
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "pdfium-sandbox".to_owned(),
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

struct Run {
    outcome: RunOutcome,
    /// File the engine tried to create outside its job directory (must not exist afterwards).
    escape: PathBuf,
    /// Pack file the engine tried to overwrite (must be unchanged afterwards).
    pack_file: PathBuf,
    _scratch: tempfile::TempDir,
    _install: tempfile::TempDir,
}

impl Run {
    fn verdict(&self, name: &str) -> &str {
        self.outcome
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.get(name))
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("the probe did not report {name}"))
    }
}

/// Runs the probe as the `pdfium` engine with a pack directory holding its program and a data
/// file. `escape_in_tmp` aims the outside write at `/tmp` instead of the scratch directory.
fn run_as_pdfium(mode: SandboxMode, escape_in_tmp: bool) -> Run {
    let workspace = Workspace::new().expect("job workspace");
    let install = scratch();
    let scratch = self::scratch();
    let program = install.path().join("engine-probe");
    fs::copy(probe(), &program).expect("copy the probe into the pack directory");
    let pack_file = install.path().join("libpdfium.so");
    fs::write(&pack_file, b"pack data").expect("pack data file");

    let escape = if escape_in_tmp {
        PathBuf::from(format!("/tmp/ariadshift-pdfium-{}", uuid::Uuid::new_v4()))
    } else {
        scratch.path().join("escape.txt")
    };
    let socket_path = scratch.path().join("host.sock");
    let _unix_listener = UnixListener::bind(&socket_path).expect("bind a UNIX socket");
    let tcp_listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a local TCP port");
    let port = tcp_listener.local_addr().expect("local address").port();
    let mut options = BTreeMap::from([
        ("outside".to_owned(), json!(escape.display().to_string())),
        ("read".to_owned(), json!(pack_file.display().to_string())),
        (
            "write_install".to_owned(),
            json!(pack_file.display().to_string()),
        ),
        ("port".to_owned(), json!(port)),
        ("unix".to_owned(), json!(socket_path.display().to_string())),
    ]);
    if cfg!(target_os = "linux") {
        options.insert(
            "mount".to_owned(),
            json!(
                workspace
                    .work_dir()
                    .join("mount-check")
                    .display()
                    .to_string()
            ),
        );
    }
    let command = EngineCommand {
        engine: "pdfium".to_owned(),
        program,
        args: vec![OsString::from("sandbox-check")],
        env: Vec::new(),
        install_root: Some(install.path().to_path_buf()),
        launcher: Some(probe()),
        sandbox: Some(mode),
    };
    let outcome = runner::run_command(
        &command,
        &request(&workspace, options),
        CancellationToken::new(),
        |_| {},
    )
    .expect("the sandbox-check probe runs under the pdfium engine id");
    Run {
        outcome,
        escape,
        pack_file,
        _scratch: scratch,
        _install: install,
    }
}

fn assert_pack_untouched(run: &Run) {
    assert_eq!(
        fs::read(&run.pack_file).expect("pack file"),
        b"pack data",
        "the pack directory must stay read-only"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn pdfium_is_confined_to_its_job_directory_without_network_on_linux() {
    let support = sandbox::support();
    if !support.gaps().is_empty() {
        eprintln!(
            "skipping: this kernel cannot give the pdfium engine full isolation: {:?}",
            support.gaps()
        );
        return;
    }
    let run = run_as_pdfium(SandboxMode::Required, false);
    assert_eq!(run.outcome.sandbox, SandboxOutcome::Applied);

    // The pack directory is readable and executable but never writable.
    assert_eq!(run.verdict("exec_self"), "allowed");
    assert_eq!(run.verdict("read_install_root"), "allowed");
    assert_eq!(run.verdict("write_install_root"), "denied");
    assert_pack_untouched(&run);

    // The job directory is writable; nothing outside it is.
    assert_eq!(run.verdict("write_workspace"), "allowed");
    assert_eq!(run.verdict("write_outside"), "denied");
    assert!(
        !run.escape.exists(),
        "no file may appear outside the job directory"
    );

    // No network of any kind, no host sockets, no mounts.
    assert_eq!(run.verdict("tcp_connect"), "denied");
    assert_eq!(run.verdict("udp_socket"), "denied");
    assert_eq!(run.verdict("mount"), "denied");
    if support.landlock_unix_sockets {
        assert_eq!(run.verdict("unix_connect"), "denied");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn pdfium_gets_a_private_tmp_that_the_host_never_sees_on_linux() {
    let support = sandbox::support();
    if !support.gaps().is_empty() {
        eprintln!("skipping: this kernel cannot give the pdfium engine full isolation");
        return;
    }
    let run = run_as_pdfium(SandboxMode::Required, true);
    assert_eq!(
        run.verdict("write_outside"),
        "allowed",
        "the engine can use its private /tmp for scratch files"
    );
    assert!(
        !run.escape.exists(),
        "a file written to the engine's /tmp must not appear in the host's /tmp"
    );
    assert_eq!(run.verdict("write_install_root"), "denied");
    assert_pack_untouched(&run);
}

#[cfg(target_os = "linux")]
#[test]
fn pdfium_without_the_sandbox_reaches_everything_only_when_explicitly_off() {
    let run = run_as_pdfium(SandboxMode::Off, false);
    assert_eq!(run.outcome.sandbox, SandboxOutcome::Off);
    for check in ["write_outside", "write_install_root", "tcp_connect"] {
        assert_eq!(
            run.verdict(check),
            "allowed",
            "{check} works without the sandbox"
        );
    }
    // The probe really overwrote the pack file here, which proves the sandboxed run above
    // had to be what denied it.
    assert_eq!(fs::read(&run.pack_file).expect("pack file"), b"changed");
    assert!(std::path::Path::new(&run.escape).exists());
}

/// Runs only on macOS CI runners: the Linux development host cannot execute Seatbelt.
#[cfg(target_os = "macos")]
#[test]
fn pdfium_is_confined_to_its_job_directory_without_network_on_macos() {
    let support = sandbox::support();
    if !support.seatbelt_available {
        eprintln!("skipping Seatbelt enforcement: /usr/bin/sandbox-exec is unavailable");
        return;
    }
    assert!(
        support.seatbelt_usable,
        "sandbox-exec exists but could not enforce the profile: {support:?}"
    );
    let run = run_as_pdfium(SandboxMode::Required, false);
    assert_eq!(run.outcome.sandbox, SandboxOutcome::Applied);
    for name in ["exec_self", "read_install_root", "write_workspace"] {
        assert_eq!(run.verdict(name), "allowed", "{name}");
    }
    for name in [
        "write_outside",
        "write_install_root",
        "tcp_connect",
        "udp_socket",
        "unix_connect",
    ] {
        assert_eq!(run.verdict(name), "denied", "{name}");
    }
    assert!(!run.escape.exists());
    assert_pack_untouched(&run);
}
