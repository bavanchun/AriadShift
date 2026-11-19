#![cfg(not(debug_assertions))]

use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
fn release_binary_ignores_the_test_engine_override() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["__engine", "pandoc"])
        .env("ARIAD_TEST_ENGINE", "hang")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the release binary");

    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll release binary") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("stop a hanging release binary");
            let _ = child.wait();
            panic!("release binary honored the debug-only test engine override");
        }
        thread::sleep(Duration::from_millis(10));
    };

    assert_ne!(status.code(), None, "release binary should exit normally");
}
