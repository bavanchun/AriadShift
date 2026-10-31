use super::*;

#[test]
fn read_only_engine_paths_can_map_libraries_without_program_execution() {
    let root = tempfile::tempdir().unwrap();
    let profile = Profile::new(&SandboxPolicy {
        read_only: vec![root.path().to_path_buf()],
        ..SandboxPolicy::default()
    })
    .unwrap();
    let key = format!("PATH_{}", profile.parameters.len() - 1);
    assert!(profile.source.contains(&format!(
        "(allow file-read* file-map-executable (subpath (param \"{key}\")))"
    )));
    assert!(
        !profile
            .source
            .contains(&format!("process-exec (subpath (param \"{key}\"))"))
    );
}
