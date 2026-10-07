use std::{fs, io::Write, path::Path};

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

#[test]
fn promote_into_copies_complete_contents() {
    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    fs::write(&artifact, b"capability bytes").expect("write artifact");

    let dest_temp = tempfile::tempdir().expect("create destination directory");
    let parent_cap =
        cap_std::fs::Dir::open_ambient_dir(dest_temp.path(), cap_std::ambient_authority())
            .expect("open capability handle");

    workspace
        .promote_into(&artifact, &parent_cap, Path::new("promoted.docx"), false)
        .expect("promote_into succeeds");

    let dest_file = dest_temp.path().join("promoted.docx");
    assert_eq!(fs::read(dest_file).unwrap(), b"capability bytes");
}

#[test]
fn promote_into_refuses_existing_destination_without_overwrite() {
    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    fs::write(&artifact, b"new bytes").expect("write artifact");

    let dest_temp = tempfile::tempdir().expect("create destination directory");
    let dest_file = dest_temp.path().join("promoted.docx");
    fs::write(&dest_file, b"old bytes").expect("write existing");

    let parent_cap =
        cap_std::fs::Dir::open_ambient_dir(dest_temp.path(), cap_std::ambient_authority())
            .expect("open capability handle");

    let err = workspace
        .promote_into(&artifact, &parent_cap, Path::new("promoted.docx"), false)
        .unwrap_err();
    assert!(matches!(err, WorkspaceError::DestinationExists));
    assert_eq!(fs::read(&dest_file).unwrap(), b"old bytes");

    // Overwrite succeeds
    workspace
        .promote_into(&artifact, &parent_cap, Path::new("promoted.docx"), true)
        .expect("overwrite succeeds");
    assert_eq!(fs::read(&dest_file).unwrap(), b"new bytes");
}

#[test]
fn promote_into_refuses_multi_component_filename() {
    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    fs::write(&artifact, b"bytes").expect("write artifact");

    let dest_temp = tempfile::tempdir().expect("create destination directory");
    let parent_cap =
        cap_std::fs::Dir::open_ambient_dir(dest_temp.path(), cap_std::ambient_authority())
            .expect("open capability handle");

    let err = workspace
        .promote_into(&artifact, &parent_cap, Path::new("../escaped.docx"), false)
        .unwrap_err();
    assert!(matches!(err, WorkspaceError::Promotion(_)));

    let err_sub = workspace
        .promote_into(&artifact, &parent_cap, Path::new("sub/bad.docx"), false)
        .unwrap_err();
    assert!(matches!(err_sub, WorkspaceError::Promotion(_)));
}

#[test]
fn promote_into_parent_swapped_after_check_does_not_redirect_write() {
    let workspace = Workspace::new().expect("create workspace");
    let artifact = workspace.output_dir().join("document.docx");
    fs::write(&artifact, b"secret payload").expect("write artifact");

    let root_temp = tempfile::tempdir().expect("create root tempdir");
    let allowed_parent = root_temp.path().join("allowed_parent");
    fs::create_dir(&allowed_parent).expect("create allowed_parent");

    let untrusted_secret = root_temp.path().join("untrusted_secret");
    fs::create_dir(&untrusted_secret).expect("create untrusted_secret");

    // Step 1: Open capability handle to the checked parent directory
    let parent_cap =
        cap_std::fs::Dir::open_ambient_dir(&allowed_parent, cap_std::ambient_authority())
            .expect("open capability handle");

    // Step 2: Attacker swaps the parent directory after the check
    let moved_away = root_temp.path().join("allowed_parent_moved");
    fs::rename(&allowed_parent, &moved_away).expect("rename away allowed_parent");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&untrusted_secret, &allowed_parent)
            .expect("create attack symlink pointing to untrusted_secret");
    }
    #[cfg(windows)]
    {
        // On Windows, create a directory symlink or junction
        let _ = std::os::windows::fs::symlink_dir(&untrusted_secret, &allowed_parent);
    }

    // Step 3: Promote using the original capability handle
    let _ = workspace.promote_into(&artifact, &parent_cap, Path::new("out.docx"), false);

    // CRITICAL SECURITY ASSERTION:
    // The write must NEVER follow the swapped parent path into untrusted_secret!
    let leaked_path = untrusted_secret.join("out.docx");
    assert!(
        !leaked_path.exists(),
        "CRITICAL: write was redirected through swapped parent symlink to untrusted target!"
    );
}

#[test]
fn sweep_stale_cleans_only_expired_ariadshift_workspaces() {
    use ariad_host::workspace::sweep_stale_in;
    use std::time::Duration;

    let isolated_root = tempfile::tempdir().expect("create isolated sweep tempdir");
    let temp_root = isolated_root.path();
    let unique_suffix = uuid::Uuid::new_v4();

    // 1. Create a matching workspace directory
    let ws_dir = temp_root.join(format!("ariadshift-test-stale-{}", unique_suffix));
    fs::create_dir(&ws_dir).expect("create test workspace dir");
    fs::write(ws_dir.join("artifact.txt"), b"temp content").expect("write artifact");

    // 2. Create a non-matching directory
    let other_dir = temp_root.join(format!("othertool-test-stale-{}", unique_suffix));
    fs::create_dir(&other_dir).expect("create other dir");

    // 3. With a 24-hour threshold, fresh workspace is preserved
    let _swept = sweep_stale_in(temp_root, Duration::from_secs(24 * 3600)).expect("sweep_stale");
    assert!(ws_dir.exists(), "fresh workspace must not be swept");
    assert!(other_dir.exists(), "other directory must not be swept");

    // 4. With a 0-second threshold, the matching workspace is swept
    let _ = sweep_stale_in(temp_root, Duration::from_secs(0)).expect("sweep_stale with 0 duration");
    assert!(!ws_dir.exists(), "stale workspace should have been swept");
    assert!(other_dir.exists(), "other directory must remain untouched");
}

#[test]
fn sweep_stale_and_count_stale_refuse_symlinks() {
    use ariad_host::workspace::{count_stale_in, sweep_stale_in};
    use std::time::Duration;

    let isolated_root = tempfile::tempdir().expect("create isolated sweep tempdir");
    let temp_root = isolated_root.path();
    let unique_suffix = uuid::Uuid::new_v4();

    // Target directory that must not be deleted or modified
    let target_dir = temp_root.join(format!("external-target-{}", unique_suffix));
    fs::create_dir(&target_dir).expect("create target dir");
    let target_file = target_dir.join("vital.txt");
    fs::write(&target_file, b"cannot be deleted").expect("write target file");

    // Symlink with ariadshift- prefix pointing to the target directory
    let symlink_ws = temp_root.join(format!("ariadshift-symlink-{}", unique_suffix));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target_dir, &symlink_ws).expect("create symlink");
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_dir(&target_dir, &symlink_ws);

    // Stale real workspace
    let real_ws = temp_root.join(format!("ariadshift-real-{}", unique_suffix));
    fs::create_dir(&real_ws).expect("create real ws dir");

    if symlink_ws.exists() {
        // count_stale_in should count only real directory, not symlink
        let count = count_stale_in(temp_root, Duration::from_secs(0)).expect("count_stale_in");
        assert_eq!(
            count, 1,
            "count_stale_in must only count real workspace directories, not symlinks"
        );

        // sweep_stale_in with 0 duration
        let swept = sweep_stale_in(temp_root, Duration::from_secs(0)).expect("sweep_stale_in");
        assert_eq!(
            swept, 1,
            "sweep_stale_in must sweep only the real workspace"
        );
        assert!(!real_ws.exists(), "real workspace should be swept");
        assert!(
            target_file.exists(),
            "target of symlink must NEVER be touched"
        );
        assert!(target_dir.exists(), "target dir must NEVER be touched");
    }
}
