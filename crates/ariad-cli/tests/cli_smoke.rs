use std::process::Command;

#[test]
fn version_reports_the_binary_name_and_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .arg("--version")
        .output()
        .expect("ashift binary should start");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        concat!("ashift ", env!("CARGO_PKG_VERSION"))
    );
}
