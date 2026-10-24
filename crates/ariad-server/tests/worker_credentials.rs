//! The actual procfs permission boundary between a worker and a same-user helper.
#![cfg(target_os = "linux")]

use std::process::Command;

#[test]
fn same_user_children_cannot_read_protected_worker_credentials() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "protected_worker_procfs_probe", "--nocapture"])
        .env("ARIAD_TEST_PROTECTED_WORKER", "1")
        .env("DATABASE_URL", "procfs-credential-sentinel")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn protected_worker_procfs_probe() {
    if std::env::var_os("ARIAD_TEST_PROTECTED_WORKER").is_none() {
        return;
    }
    let pid = std::process::id().to_string();
    let read = |blocked: bool| {
        let code = if blocked {
            "import sys\nfor name in ('environ', 'mem'):\n try:\n  open('/proc/' + sys.argv[1] + '/' + name, 'rb')\n except PermissionError:\n  continue\n raise SystemExit('protected procfs file was readable: ' + name)"
        } else {
            "import sys; assert b'procfs-credential-sentinel' in open('/proc/' + sys.argv[1] + '/environ', 'rb').read()"
        };
        let output = Command::new("python3")
            .args(["-c", code, &pid])
            .env_clear()
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    read(false);
    ariad_server::sandbox::protect_worker_credentials().unwrap();
    read(true);
}
