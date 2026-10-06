use std::{fs, io::Write};

use ariad_host::workspace::{Workspace, WorkspaceError};

#[test]
fn creates_and_removes_the_standard_layout() {
    let mut workspace = Workspace::new().expect("create workspace");
    let root = workspace.root().to_path_buf();

    for directory in [
        workspace.input_dir(),
        workspace.output_dir(),
        workspace.work_dir(),
        workspace.log_dir(),
    ] {
        assert!(directory.is_dir());
        assert_eq!(directory.parent(), Some(workspace.root()));
    }

    workspace.close().expect("remove workspace");
    assert!(!root.exists());
    workspace.close().expect("close is idempotent");
}

#[test]
fn refuses_an_existing_destination_without_partial_output() {
    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    fs::write(&artifact, b"complete output").expect("write artifact");
    let destination_dir = tempfile::tempdir().expect("create destination directory");
    let destination = destination_dir.path().join("document.docx");
    fs::write(&destination, b"existing output").expect("write destination");

    assert!(matches!(
        workspace.promote(&artifact, &destination, false),
        Err(WorkspaceError::DestinationExists)
    ));
    assert_eq!(fs::read(destination).unwrap(), b"existing output");
    assert_eq!(fs::read_dir(destination_dir.path()).unwrap().count(), 1);
}

#[test]
fn replaces_an_existing_destination_when_overwrite_is_enabled() {
    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    fs::write(&artifact, b"new complete output").expect("write artifact");
    let destination_dir = tempfile::tempdir().expect("create destination directory");
    let destination = destination_dir.path().join("document.docx");
    fs::write(&destination, b"old output").expect("write destination");

    workspace
        .promote(&artifact, &destination, true)
        .expect("replace destination");
    assert_eq!(fs::read(destination).unwrap(), b"new complete output");
    assert_eq!(fs::read_dir(destination_dir.path()).unwrap().count(), 1);
}

#[test]
fn refuses_artifacts_that_resolve_outside_output() {
    let workspace = Workspace::new().expect("create workspace");
    let external = tempfile::NamedTempFile::new().expect("create external artifact");
    let destination_dir = tempfile::tempdir().expect("create destination directory");
    let destination = destination_dir.path().join("document.docx");

    assert!(matches!(
        workspace.promote(external.path(), &destination, false),
        Err(WorkspaceError::ArtifactOutsideOutput)
    ));
    assert!(!destination.exists());
}

#[test]
fn a_failed_promotion_leaves_no_partial_destination() {
    let workspace = Workspace::new().expect("create workspace");
    let destination_dir = tempfile::tempdir().expect("create destination directory");
    let destination = destination_dir.path().join("document.docx");

    assert!(
        workspace
            .promote(
                &workspace.output_dir().join("missing.docx"),
                &destination,
                false,
            )
            .is_err()
    );
    assert!(!destination.exists());
    assert_eq!(fs::read_dir(destination_dir.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn dangling_symlink_destination_counts_as_existing() {
    use std::os::unix::fs::symlink;

    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    fs::write(&artifact, b"complete output").expect("write artifact");
    let destination_dir = tempfile::tempdir().expect("create destination directory");
    let destination = destination_dir.path().join("document.docx");
    symlink(destination_dir.path().join("missing-target"), &destination)
        .expect("create dangling symlink");

    assert!(matches!(
        workspace.promote(&artifact, &destination, false),
        Err(WorkspaceError::DestinationExists)
    ));
    assert!(fs::symlink_metadata(destination).is_ok());
}

#[test]
fn promotion_copies_complete_contents() {
    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    let mut file = fs::File::create(&artifact).expect("create artifact");
    file.write_all(b"complete artifact bytes")
        .expect("write artifact");
    let destination_dir = tempfile::tempdir().expect("create destination directory");
    let destination = destination_dir.path().join("document.docx");

    workspace
        .promote(&artifact, &destination, false)
        .expect("promote artifact");
    assert_eq!(fs::read(destination).unwrap(), b"complete artifact bytes");
}
