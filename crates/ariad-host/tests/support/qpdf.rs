use ariad_host::packs::{self, Component, Manifest, Target, Unpack};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn pack() -> Option<tempfile::TempDir> {
    let Some(source) = std::env::var_os("ARIAD_TEST_QPDF_DIR") else {
        eprintln!("skipping qpdf test: ARIAD_TEST_QPDF_DIR is unset");
        return None;
    };
    let source = PathBuf::from(source);
    let root = tempfile::tempdir().expect("pack directory");
    copy_tree(&source.join("bin"), &root.path().join("bin"));
    copy_tree(&source.join("lib"), &root.path().join("lib"));
    let manifest = Manifest {
        schema: packs::MANIFEST_SCHEMA.into(),
        name: "qpdf".into(),
        version: "12.4.2".into(),
        description: String::new(),
        license: "Apache-2.0".into(),
        requires_ashift: ">=0.1.0".into(),
        engine_protocol: Some("ariad-engine/2".into()),
        notices: vec![],
        targets: vec![Target {
            triple: packs::host_targets()[0].clone(),
            entrypoint: vec!["bin/qpdf".into()],
            executables: vec![],
            env: Default::default(),
            options: Default::default(),
            components: vec![Component {
                name: "qpdf".into(),
                url: "qpdf".into(),
                sha256: "0".repeat(64),
                size: 1,
                unpack: Unpack::File,
                dest: ".".into(),
                unpacked_size: None,
                entries: None,
                executable: true,
            }],
        }],
    };
    fs::write(root.path().join("pack.toml"), manifest.to_toml().unwrap()).unwrap();
    Some(root)
}
fn copy_tree(source: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(source).expect("qpdf development directory") {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(from, to).unwrap();
        }
    }
}
pub fn in_child(name: &str) -> Option<tempfile::TempDir> {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_some() {
        return None;
    }
    let pack = pack()?;
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .envs([
            ("ARIAD_QPDF_TEST_CHILD", std::ffi::OsStr::new("1")),
            ("ASHIFT_PACK_QPDF", pack.path().as_os_str()),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some(pack)
}
