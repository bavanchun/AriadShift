use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_host::confine::{
    AllowedDirs, ConfineError, confine_input, confine_output, is_windows_device_stem,
    parse_file_uri, path_to_file_uri, validate_windows_path_syntax,
};
use tempfile::tempdir;

#[test]
fn test_empty_allowed_set() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("test.md");
    fs::write(&file, "# Test").unwrap();

    let allowed = AllowedDirs::default();
    assert!(allowed.is_empty());

    let input_err = confine_input(&file, &allowed).unwrap_err();
    assert!(matches!(input_err, ConfineError::EmptyAllowedSet));
    assert_eq!(input_err.exit_code(), 2);

    let output_err = confine_output(&file, None, "docx", false, false, &allowed).unwrap_err();
    assert!(matches!(output_err, ConfineError::EmptyAllowedSet));
    assert_eq!(output_err.exit_code(), 2);
}

#[test]
fn test_valid_input_and_output() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let input = canon_temp.join("doc.md");
    fs::write(&input, "# Hello").unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&canon_temp), &[]).unwrap();
    assert!(!allowed.is_empty());

    let canonical_in = confine_input(&input, &allowed).expect("input should be valid");
    assert_eq!(canonical_in.path, input);

    // Default output beside input
    let out = confine_output(&input, None, "docx", false, false, &allowed)
        .expect("output should be valid");
    assert_eq!(out.destination, canon_temp.join("doc.docx"));
    assert_eq!(out.file_name, "doc.docx");
    assert!(!out.overwrite);
}

#[test]
fn test_path_traversal_dotdot_escape() {
    let root = tempdir().unwrap();
    let canon_root = fs::canonicalize(root.path()).unwrap();
    let inside = canon_root.join("inside");
    let outside = canon_root.join("outside");
    fs::create_dir(&inside).unwrap();
    fs::create_dir(&outside).unwrap();

    let secret = outside.join("secret.md");
    fs::write(&secret, "confidential").unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&inside), &[]).unwrap();

    // Try path traversal .. escaping inside
    let traversal = inside.join("../outside/secret.md");
    let err = confine_input(&traversal, &allowed).unwrap_err();
    assert!(matches!(err, ConfineError::AccessDenied { .. }));
    assert_eq!(err.exit_code(), 2);

    // Assert error message reports the caller-supplied traversal path, not secret.md directly
    let msg = err.to_string();
    assert!(msg.contains("outside allowed directories"));

    // Traversal output
    let out_traversal = inside.join("../outside/out.docx");
    let out_err = confine_output(
        &traversal,
        Some(&out_traversal),
        "docx",
        false,
        false,
        &allowed,
    )
    .unwrap_err();
    assert!(matches!(out_err, ConfineError::AccessDenied { .. }));
}

#[cfg(unix)]
#[test]
fn test_symlink_escape_unix() {
    let root = tempdir().unwrap();
    let canon_root = fs::canonicalize(root.path()).unwrap();
    let inside = canon_root.join("allowed");
    let outside = canon_root.join("outside");
    fs::create_dir(&inside).unwrap();
    fs::create_dir(&outside).unwrap();

    let secret = outside.join("secret.txt");
    fs::write(&secret, "secret data").unwrap();

    let symlink_path = inside.join("symlink_to_secret.md");
    std::os::unix::fs::symlink(&secret, &symlink_path).unwrap();

    let allowed = AllowedDirs::new(&[inside], &[]).unwrap();

    let err = confine_input(&symlink_path, &allowed).unwrap_err();
    assert!(matches!(err, ConfineError::AccessDenied { .. }));

    // Security check: Refused paths are reported without echoing resolved symlink targets
    let msg = err.to_string();
    assert!(
        !msg.contains("secret.txt"),
        "error message must NOT leak the resolved symlink target"
    );
    assert!(
        msg.contains("symlink_to_secret.md"),
        "error message should cite caller path"
    );
}

#[test]
fn test_directory_symlink_or_junction_escape() {
    let root = tempdir().unwrap();
    let canon_root = fs::canonicalize(root.path()).unwrap();
    let inside = canon_root.join("allowed");
    let outside = canon_root.join("outside");
    fs::create_dir(&inside).unwrap();
    fs::create_dir(&outside).unwrap();

    let secret_file = outside.join("target.md");
    fs::write(&secret_file, "outside content").unwrap();

    let link_dir = inside.join("linked_dir");

    #[cfg(windows)]
    {
        // mklink /J creates a directory junction on Windows without admin rights
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link_dir)
            .arg(&outside)
            .status()
            .expect("mklink /J execution");
        assert!(status.success(), "mklink /J should succeed on Windows");
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, &link_dir).expect("create directory symlink");
    }

    let allowed = AllowedDirs::new(&[inside], &[]).unwrap();

    // Access through junction / dir symlink to outside file
    let input_via_link = link_dir.join("target.md");
    let err = confine_input(&input_via_link, &allowed).unwrap_err();
    assert!(matches!(err, ConfineError::AccessDenied { .. }));
}

#[test]
fn test_hidden_components_refused() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let input = canon_temp.join("doc.md");
    fs::write(&input, "# Hello").unwrap();
    let allowed = AllowedDirs::new(std::slice::from_ref(&canon_temp), &[]).unwrap();

    let dot_dir = canon_temp.join(".hidden_dir");
    fs::create_dir(&dot_dir).unwrap();
    let sub = canon_temp.join("sub");
    fs::create_dir(&sub).unwrap();

    let hidden_cases = [
        canon_temp.join(".github/out.docx"),
        canon_temp.join(".vscode/out.docx"),
        canon_temp.join(".hidden.docx"),
        canon_temp.join(".env.docx"),
        dot_dir.join("out.docx"),
        sub.join(".dotfile.docx"),
    ];

    for path in hidden_cases {
        let err = confine_output(&input, Some(&path), "docx", false, false, &allowed)
            .expect_err(&format!("hidden path {} must be refused", path.display()));
        assert!(matches!(err, ConfineError::HiddenComponent { .. }));
        assert_eq!(err.exit_code(), 2);
    }

    // Input with hidden components must also be refused
    let hidden_input = canon_temp.join(".secret.md");
    fs::write(&hidden_input, "# Secret").unwrap();
    let err_in =
        confine_input(&hidden_input, &allowed).expect_err("hidden input file must be refused");
    assert!(matches!(err_in, ConfineError::HiddenComponent { .. }));

    let hidden_dir_input = dot_dir.join("doc.md");
    fs::write(&hidden_dir_input, "# Secret in dir").unwrap();
    let err_in_dir = confine_input(&hidden_dir_input, &allowed)
        .expect_err("input in hidden directory must be refused");
    assert!(matches!(err_in_dir, ConfineError::HiddenComponent { .. }));

    // Output path traversing with .. into hidden directory must be refused
    let hidden_out = sub.join("../.hidden_dir/out.docx");
    let err_out = confine_output(&input, Some(&hidden_out), "docx", false, false, &allowed)
        .expect_err("output traversing into hidden directory via .. must be refused");
    assert!(matches!(err_out, ConfineError::HiddenComponent { .. }));
}

#[test]
fn test_extension_mismatch() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let input = canon_temp.join("doc.md");
    fs::write(&input, "# Hello").unwrap();
    let allowed = AllowedDirs::new(std::slice::from_ref(&canon_temp), &[]).unwrap();

    // Expected docx, given html
    let bad_out = canon_temp.join("out.html");
    let err = confine_output(&input, Some(&bad_out), "docx", false, false, &allowed).unwrap_err();
    assert!(matches!(err, ConfineError::ExtensionMismatch { .. }));
    assert_eq!(err.exit_code(), 2);

    // Expected md, given docx
    let bad_md = canon_temp.join("out.docx");
    let err2 = confine_output(&input, Some(&bad_md), "md", false, false, &allowed).unwrap_err();
    assert!(matches!(err2, ConfineError::ExtensionMismatch { .. }));

    // Valid matching extensions
    let valid_md = canon_temp.join("out.markdown");
    assert!(confine_output(&input, Some(&valid_md), "md", false, false, &allowed).is_ok());

    let valid_htm = canon_temp.join("out.htm");
    assert!(confine_output(&input, Some(&valid_htm), "html", false, false, &allowed).is_ok());
}

#[test]
fn test_existing_output_overwrite_policy() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let input = canon_temp.join("doc.md");
    let output = canon_temp.join("out.docx");
    fs::write(&input, "# Hello").unwrap();
    fs::write(&output, "existing binary").unwrap();

    let allowed = AllowedDirs::new(&[canon_temp], &[]).unwrap();

    // Case 1: no overwrite flag, no allow_overwrite
    let err1 = confine_output(&input, Some(&output), "docx", false, false, &allowed).unwrap_err();
    assert!(matches!(err1, ConfineError::DestinationExists { .. }));
    assert_eq!(err1.exit_code(), 6);

    // Case 2: overwrite flag = true, allow_overwrite = false
    let err2 = confine_output(&input, Some(&output), "docx", true, false, &allowed).unwrap_err();
    assert!(matches!(err2, ConfineError::DestinationExists { .. }));
    assert_eq!(err2.exit_code(), 6);

    // Case 3: overwrite flag = false, allow_overwrite = true
    let err3 = confine_output(&input, Some(&output), "docx", false, true, &allowed).unwrap_err();
    assert!(matches!(err3, ConfineError::DestinationExists { .. }));
    assert_eq!(err3.exit_code(), 6);

    // Case 4: overwrite flag = true, allow_overwrite = true -> allowed!
    let ok = confine_output(&input, Some(&output), "docx", true, true, &allowed)
        .expect("overwrite should succeed when both flags set");
    assert!(ok.overwrite);
}

#[cfg(unix)]
#[test]
fn test_existing_symlink_destination_always_refused() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let input = canon_temp.join("doc.md");
    let target = canon_temp.join("target.docx");
    let symlink_dest = canon_temp.join("out.docx");
    fs::write(&input, "# Hello").unwrap();
    fs::write(&target, "target").unwrap();

    std::os::unix::fs::symlink(&target, &symlink_dest).unwrap();

    let allowed = AllowedDirs::new(&[canon_temp], &[]).unwrap();

    // Even with overwrite = true and allow_overwrite = true, symlink output is refused
    let err =
        confine_output(&input, Some(&symlink_dest), "docx", true, true, &allowed).unwrap_err();
    assert!(matches!(
        err,
        ConfineError::SymlinkDestinationRefused { .. }
    ));
    assert_eq!(err.exit_code(), 6);
}

#[test]
fn test_same_file_as_input_refused() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let file = canon_temp.join("doc.md");
    fs::write(&file, "# Hello").unwrap();

    let allowed = AllowedDirs::new(&[canon_temp], &[]).unwrap();

    // Target format md, output set to input path
    let err = confine_output(&file, Some(&file), "md", true, true, &allowed).unwrap_err();
    assert!(matches!(err, ConfineError::SameFileAsInput));
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn test_file_uri_parsing() {
    #[cfg(unix)]
    {
        let path = parse_file_uri("file:///home/user/docs").unwrap();
        assert_eq!(path, PathBuf::from("/home/user/docs"));

        let with_spaces = parse_file_uri("file:///home/user/my%20documents").unwrap();
        assert_eq!(with_spaces, PathBuf::from("/home/user/my documents"));

        let with_host = parse_file_uri("file://localhost/var/data").unwrap();
        assert_eq!(with_host, PathBuf::from("/var/data"));

        let uri_out = path_to_file_uri(Path::new("/home/user/my documents"));
        assert_eq!(uri_out, "file:///home/user/my%20documents");
    }

    #[cfg(windows)]
    {
        let path = parse_file_uri("file:///C:/Users/name/docs").unwrap();
        assert_eq!(path, PathBuf::from(r"C:\Users\name\docs"));

        let with_spaces = parse_file_uri("file:///C:/Users/name/my%20documents").unwrap();
        assert_eq!(with_spaces, PathBuf::from(r"C:\Users\name\my documents"));

        let uri_out = path_to_file_uri(Path::new(r"C:\Users\name\my documents"));
        assert_eq!(uri_out, "file:///C:/Users/name/my%20documents");
    }

    // Non-local authority rejected
    assert!(parse_file_uri("file://evilhost/var/data").is_err());
    // Invalid scheme
    assert!(parse_file_uri("http://example.com/doc").is_err());
    // Incomplete escape
    assert!(parse_file_uri("file:///path%2").is_err());
    // NUL byte
    assert!(parse_file_uri("file:///path%00evil").is_err());
}

#[test]
fn test_not_regular_file() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let dir_path = canon_temp.join("sub_dir");
    fs::create_dir(&dir_path).unwrap();

    let allowed = AllowedDirs::new(&[canon_temp], &[]).unwrap();

    let err = confine_input(&dir_path, &allowed).unwrap_err();
    assert!(matches!(err, ConfineError::NotRegularFile { .. }));
}

#[test]
fn test_property_random_paths() {
    let temp = tempdir().unwrap();
    let inside_root = temp.path().join("allowed_dir");
    fs::create_dir(&inside_root).unwrap();
    let canon_root = fs::canonicalize(&inside_root).unwrap();

    // Create a real valid input file inside the allowed root
    let valid_input = canon_root.join("in.md");
    fs::write(&valid_input, "# Test").unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&canon_root), &[]).unwrap();

    // Adversarial prefix and component combinations
    let prefixes = [
        "",
        "/",
        "../",
        "../../",
        "./",
        "....//",
        "/../",
        "subdir/../",
    ];
    let stems = [
        "in",
        "test",
        "..",
        ".",
        "normal",
        ".hidden",
        "CON",
        "NUL",
        "file%20name",
        "a/b/c",
    ];
    let extensions = ["", ".md", ".docx", ".html", ".exe", ".sh"];

    for p in prefixes {
        for s in stems {
            for ext in extensions {
                let candidate_str = format!("{p}{s}{ext}");
                let path = Path::new(&candidate_str);

                for test_path in [path.to_path_buf(), canon_root.join(path)] {
                    if let Ok(validated_in) = confine_input(&test_path, &allowed) {
                        assert!(
                            validated_in.path.starts_with(&canon_root),
                            "validated input {} must lie strictly within allowed dir {}",
                            validated_in.path.display(),
                            canon_root.display()
                        );
                    }

                    if let Ok(validated_out) = confine_output(
                        &valid_input,
                        Some(&test_path),
                        "docx",
                        false,
                        false,
                        &allowed,
                    ) {
                        assert!(
                            validated_out.destination.starts_with(&canon_root),
                            "validated output {} must be confined inside {}",
                            validated_out.destination.display(),
                            canon_root.display()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_sibling_prefix_not_allowed() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let data_dir = canon_temp.join("data");
    let data_evil = canon_temp.join("data-evil");
    fs::create_dir(&data_dir).unwrap();
    fs::create_dir(&data_evil).unwrap();

    let evil_file = data_evil.join("file.md");
    fs::write(&evil_file, "evil content").unwrap();

    // L-SIBLING: pin is_subpath directly so string-prefix mutation dies
    assert!(
        !ariad_host::confine::is_subpath(&data_evil, &data_dir),
        "is_subpath must return false for sibling prefix directory"
    );
    assert!(
        !ariad_host::confine::is_subpath(&evil_file, &data_dir),
        "is_subpath must return false for file inside sibling prefix directory"
    );

    let allowed = AllowedDirs::new(&[data_dir], &[]).unwrap();
    let err = confine_input(&evil_file, &allowed).unwrap_err();
    assert!(matches!(err, ConfineError::AccessDenied { .. }));
}

#[cfg(unix)]
#[test]
fn test_input_symlink_guards_isolated() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let allowed_root = canon_temp.join("root");
    let outside_root = canon_temp.join("outside");
    fs::create_dir(&allowed_root).unwrap();
    fs::create_dir(&outside_root).unwrap();

    let internal_file = allowed_root.join("internal.md");
    fs::write(&internal_file, "# Internal").unwrap();

    let symlink_file = allowed_root.join("sym.md");
    std::os::unix::fs::symlink("internal.md", &symlink_file).unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&allowed_root), &[]).unwrap();
    let root = &allowed.roots()[0];

    // Layer 1 in isolation: symlink_metadata check identifies symlink
    let meta = root.dir.symlink_metadata(Path::new("sym.md")).unwrap();
    assert!(
        meta.is_symlink(),
        "Layer 1 isolation: symlink_metadata must detect symlink"
    );

    // Layer 2 in isolation: open_file_nofollow with O_NOFOLLOW refuses symlink
    let open_res = ariad_host::confine::open_file_nofollow(&root.dir, Path::new("sym.md"));
    assert!(
        open_res.is_err(),
        "Layer 2 isolation: open_file_nofollow with O_NOFOLLOW must fail on symlink"
    );
    let open_err = open_res.unwrap_err();
    assert_eq!(
        open_err.raw_os_error(),
        Some(libc::ELOOP),
        "Layer 2 isolation: open_file_nofollow must return ELOOP on symlink"
    );
}

#[test]
fn test_oracle_elimination() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let allowed_dir = canon_temp.join("sandbox");
    fs::create_dir(&allowed_dir).unwrap();
    let allowed = AllowedDirs::new(&[allowed_dir], &[]).unwrap();

    #[cfg(unix)]
    let outside_probes = [
        PathBuf::from("/etc/passwd"),
        PathBuf::from("/etc/no-such-file-definitely-missing-12345"),
        PathBuf::from("/dev/zero"),
        PathBuf::from("/root/.bashrc"),
    ];
    #[cfg(not(unix))]
    let outside_probes = [
        PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts"),
        PathBuf::from(r"C:\Windows\no-such-file-definitely-missing-12345"),
    ];

    for probe in outside_probes {
        let err = confine_input(&probe, &allowed)
            .expect_err(&format!("probe {} must fail", probe.display()));
        assert!(
            matches!(err, ConfineError::AccessDenied { .. }),
            "probe {} should return AccessDenied, got {:?}",
            probe.display(),
            err
        );
    }
}

#[test]
fn test_relative_paths_refused() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let allowed = AllowedDirs::new(&[canon_temp], &[]).unwrap();

    let err_in = confine_input(Path::new("relative.md"), &allowed).unwrap_err();
    assert!(matches!(err_in, ConfineError::InvalidPath { .. }));

    let err_out = confine_output(
        Path::new("/tmp/input.md"),
        Some(Path::new("relative.docx")),
        "docx",
        false,
        false,
        &allowed,
    )
    .unwrap_err();
    assert!(matches!(err_out, ConfineError::InvalidPath { .. }));
}

#[test]
fn test_windows_syntax_and_multibyte() {
    // ADS
    assert!(validate_windows_path_syntax("C:\\data\\test.html:ads.html").is_err());
    // Reserved device names
    assert!(validate_windows_path_syntax("C:\\data\\CON.md").is_err());
    assert!(validate_windows_path_syntax("C:\\data\\com1.docx").is_err());
    assert!(validate_windows_path_syntax("C:\\data\\aux.txt").is_err());
    // Multibyte string must not panic on char boundaries
    assert!(!is_windows_device_stem("éé"));
    assert!(!is_windows_device_stem("こんにちは"));
    assert!(is_windows_device_stem("CON"));
    assert!(is_windows_device_stem("prn"));
    assert!(is_windows_device_stem("com9"));
}

#[test]
fn test_output_parent_not_found_vs_access_denied() {
    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let allowed_dir = canon_temp.join("sandbox");
    fs::create_dir(&allowed_dir).unwrap();
    let allowed = AllowedDirs::new(std::slice::from_ref(&allowed_dir), &[]).unwrap();
    let input = allowed_dir.join("in.md");
    fs::write(&input, "# Test").unwrap();

    // Nonexistent parent INSIDE allowed root -> OutputParentNotFound
    let non_existent_inside = allowed_dir.join("nonexistent_parent/out.docx");
    let err_inside = confine_output(
        &input,
        Some(&non_existent_inside),
        "docx",
        false,
        false,
        &allowed,
    )
    .unwrap_err();
    assert!(matches!(
        err_inside,
        ConfineError::OutputParentNotFound { .. }
    ));

    // Nonexistent parent OUTSIDE allowed root -> AccessDenied
    let non_existent_outside = canon_temp.join("outside_nonexistent/out.docx");
    let err_outside = confine_output(
        &input,
        Some(&non_existent_outside),
        "docx",
        false,
        false,
        &allowed,
    )
    .unwrap_err();
    assert!(matches!(err_outside, ConfineError::AccessDenied { .. }));
}

#[cfg(unix)]
#[test]
fn test_parent_swap_race_cannot_escape_root() {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };

    use ariad_host::workspace::Workspace;

    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let allowed_root = canon_temp.join("allowed");
    let outside_root = canon_temp.join("outside");
    fs::create_dir(&allowed_root).unwrap();
    fs::create_dir(&outside_root).unwrap();

    let sub_real = allowed_root.join("sub");
    fs::create_dir(&sub_real).unwrap();

    let sub_symlink = allowed_root.join("subx");
    std::os::unix::fs::symlink(&outside_root, &sub_symlink).unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&allowed_root), &[]).unwrap();
    let running = Arc::new(AtomicBool::new(true));

    let r_clone = Arc::clone(&running);
    let sub_real_clone = sub_real.clone();
    let sub_symlink_clone = sub_symlink.clone();

    // Attacker thread rapidly swapping sub and subx
    let swapper = thread::spawn(move || {
        let temp_swap = sub_real_clone.with_extension("tmp_swap");
        while r_clone.load(Ordering::Relaxed) {
            let _ = fs::rename(&sub_real_clone, &temp_swap);
            let _ = fs::rename(&sub_symlink_clone, &sub_real_clone);
            let _ = fs::rename(&temp_swap, &sub_symlink_clone);
        }
    });

    let input = allowed_root.join("in.md");
    fs::write(&input, "# Hello").unwrap();

    let ws = Workspace::new().unwrap();
    let artifact = ws.output_dir().join("out.docx");
    fs::write(&artifact, b"secret payload").unwrap();

    let target_out = sub_real.join("out.docx");

    for _ in 0..100 {
        if let Ok(validated) =
            confine_output(&input, Some(&target_out), "docx", true, true, &allowed)
        {
            let _ = ws.promote_into(
                &artifact,
                &validated.parent_dir,
                Path::new(&validated.file_name),
                true,
            );
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = swapper.join();

    // SECURITY CHECK:
    // outside_root must NEVER contain out.docx!
    assert!(
        !outside_root.join("out.docx").exists(),
        "CRITICAL: Parent swap race escaped allowed root and wrote into outside directory!"
    );
}

#[test]
fn test_promote_into_create_new_race_no_clobber() {
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    use ariad_host::workspace::Workspace;

    let dest_temp = tempdir().unwrap();
    let canon_dest = fs::canonicalize(dest_temp.path()).unwrap();
    let parent_cap =
        cap_std::fs::Dir::open_ambient_dir(&canon_dest, cap_std::ambient_authority()).unwrap();

    let mut race_hits = 0;
    for i in 0..100 {
        let dest_file_name = format!("race-{i}.docx");
        let dest_path = canon_dest.join(&dest_file_name);

        let barrier = Arc::new(Barrier::new(2));
        let b1 = Arc::clone(&barrier);
        let b2 = Arc::clone(&barrier);

        let parent_clone = parent_cap.try_clone().unwrap();
        let fname_clone = dest_file_name.clone();

        let handle1 = thread::spawn({
            let ws_clone = Workspace::new().unwrap();
            let art = ws_clone.output_dir().join("source.docx");
            fs::write(&art, b"promote bytes").unwrap();
            move || {
                b1.wait();
                ws_clone.promote_into(&art, &parent_clone, Path::new(&fname_clone), false)
            }
        });

        let dest_path_clone = dest_path.clone();
        let dest_dir = canon_dest.clone();
        let handle2 = thread::spawn(move || {
            b2.wait();
            // Alternate between waiting for .ariadshift-promote- tmp file to land precisely in publish window
            // and microsecond spin delays so create_new lands deterministically after symlink_metadata check.
            if i % 2 == 0 {
                let start = std::time::Instant::now();
                while start.elapsed() < std::time::Duration::from_millis(50) {
                    if let Ok(entries) = fs::read_dir(&dest_dir)
                        && entries.flatten().any(|e| {
                            e.file_name()
                                .to_string_lossy()
                                .starts_with(".ariadshift-promote-")
                        })
                    {
                        break;
                    }
                    std::hint::spin_loop();
                }
            } else {
                let delay_us = ((i * 31) % 250) as u64;
                if delay_us > 0 {
                    thread::sleep(std::time::Duration::from_micros(delay_us));
                }
            }
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest_path_clone)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = f.write_all(b"user data");
                    Ok(())
                }
                Err(e) => Err(e),
            }
        });

        let _ = handle1.join().unwrap();
        let res2 = handle2.join().unwrap();

        // If thread 2 succeeded in creating the file, its content "user data" must NEVER be overwritten!
        if res2.is_ok() {
            race_hits += 1;
            let data = fs::read(&dest_path).unwrap();
            assert_eq!(
                data, b"user data",
                "H3 REGRESSION: User data was clobbered by promote_into(overwrite=false)!"
            );
        }
    }
    assert!(
        race_hits > 0,
        "concurrent create_new must have succeeded in at least one iteration"
    );
}

#[cfg(unix)]
#[test]
fn test_convert_confined_parent_swap_race() {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let allowed_root = canon_temp.join("allowed_root");
    let outside_root = canon_temp.join("outside_root");
    fs::create_dir(&allowed_root).unwrap();
    fs::create_dir(&outside_root).unwrap();

    // Outside directory contains an existing out.md with secret content
    let outside_secret_file = outside_root.join("out.md");
    fs::write(&outside_secret_file, "CONFIDENTIAL_OUTSIDE_SECRET").unwrap();

    let sub_real = allowed_root.join("sub");
    let sub_symlink = allowed_root.join("sub_symlink");
    fs::create_dir(&sub_real).unwrap();
    std::os::unix::fs::symlink(&outside_root, &sub_symlink).unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&allowed_root), &[]).unwrap();
    let running = Arc::new(AtomicBool::new(true));

    let r_clone = Arc::clone(&running);
    let sub_real_clone = sub_real.clone();
    let sub_symlink_clone = sub_symlink.clone();

    // Attacker thread rapidly swapping sub_real and sub_symlink
    let swapper = thread::spawn(move || {
        let temp_swap = sub_real_clone.with_extension("tmp_swap");
        while r_clone.load(Ordering::Relaxed) {
            let _ = fs::rename(&sub_real_clone, &temp_swap);
            let _ = fs::rename(&sub_symlink_clone, &sub_real_clone);
            let _ = fs::rename(&temp_swap, &sub_symlink_clone);
        }
    });

    let input_html = allowed_root.join("input.html");
    fs::write(
        &input_html,
        "<html><body><h1>Allowed Document</h1><p>Inside root.</p></body></html>",
    )
    .unwrap();

    let target_out = sub_real.join("out.md");

    let start = Instant::now();
    let mut completed_conversions = 0;
    while start.elapsed() < Duration::from_secs(3) && completed_conversions < 30 {
        if let Ok(in_confined) = confine_input(&input_html, &allowed)
            && let Ok(out_confined) = confine_output(
                &in_confined,
                Some(&target_out),
                "md",
                true, // overwrite
                true, // allow_overwrite
                &allowed,
            )
        {
            let request = ariad_host::convert::ConvertRequest {
                input: in_confined.path.clone(),
                output: out_confined.destination.clone(),
                target_format: "md".to_owned(),
                profile: ariad_core::planner::Profile::Editable,
                overwrite: out_confined.overwrite,
                engine_program: std::env::current_exe().unwrap(),
                title_fallback: None,
                asset_base_dir: None,
                reader_engine: None,
            };
            let cancel = tokio_util::sync::CancellationToken::new();
            if let Ok(report) = ariad_host::convert::convert_confined(
                &request,
                &out_confined.parent_dir,
                Path::new(&out_confined.file_name),
                cancel,
                |_| {},
            ) {
                completed_conversions += 1;
                if let Some(ref md) = report.inline_markdown {
                    assert!(
                        !md.contains("CONFIDENTIAL_OUTSIDE_SECRET"),
                        "CRITICAL: Inline markdown read leaked outside secret!"
                    );
                }
            }
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = swapper.join();

    assert!(
        completed_conversions > 0,
        "convert_confined must have succeeded at least once during race"
    );

    // SECURITY CHECK:
    // outside_secret_file must NEVER be overwritten with conversion output!
    let outside_content = fs::read_to_string(&outside_secret_file).unwrap();
    assert_eq!(
        outside_content, "CONFIDENTIAL_OUTSIDE_SECRET",
        "CRITICAL: convert_confined escaped root and clobbered outside file during parent swap!"
    );
}

#[test]
fn test_confined_input_parent_dir_is_the_input_directory() {
    let temp = tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("note.md"), "# Note").unwrap();
    fs::write(sub.join("photo.png"), b"sibling").unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&root), &[]).unwrap();
    let input = confine_input(&sub.join("note.md"), &allowed).unwrap();

    assert_eq!(input.parent_dir.read("photo.png").unwrap(), b"sibling");
}

#[cfg(unix)]
#[test]
fn test_parent_swap_after_input_copy_never_embeds_a_private_image() {
    use ariad_host::{
        assets::AssetBase,
        convert::{ConvertRequest, convert_confined_assets_in},
        session::{PoolConfig, SessionPool},
    };
    use sha2::{Digest, Sha256};

    const PUBLIC_PNG: &[u8] = b"\x89PNG\r\n\x1a\npublic sibling image";
    const PRIVATE_PNG: &[u8] = b"\x89PNG\r\n\x1a\nprivate image outside the root";

    let temp = tempdir().unwrap();
    let canon_temp = fs::canonicalize(temp.path()).unwrap();
    let inbox = canon_temp.join("inbox");
    let private = canon_temp.join("private");
    let sub = inbox.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::create_dir(&private).unwrap();
    fs::write(sub.join("note.md"), "# Note\n\n![image](photo.png)\n").unwrap();
    fs::write(sub.join("photo.png"), PUBLIC_PNG).unwrap();
    fs::write(private.join("photo.png"), PRIVATE_PNG).unwrap();

    let allowed = AllowedDirs::new(std::slice::from_ref(&inbox), &[]).unwrap();
    let mut input = confine_input(&sub.join("note.md"), &allowed).unwrap();
    let output = confine_output(
        &input,
        Some(&inbox.join("note.json")),
        "json",
        false,
        false,
        &allowed,
    )
    .unwrap();
    let (_isolated_dir, isolated_input) = input.create_isolated_copy(None, None).unwrap();
    let asset_base = AssetBase::confined(input.parent_dir.try_clone().unwrap());

    // Replace the input's parent with a symlink to the private directory after the input
    // copy and before images are resolved.
    fs::rename(&sub, inbox.join("sub-original")).unwrap();
    std::os::unix::fs::symlink(&private, &sub).unwrap();

    let request = ConvertRequest {
        input: isolated_input,
        output: output.destination.clone(),
        target_format: "json".to_owned(),
        profile: ariad_core::planner::Profile::Editable,
        overwrite: false,
        engine_program: std::env::current_exe().unwrap(),
        title_fallback: None,
        asset_base_dir: None,
        reader_engine: None,
    };
    let pool = SessionPool::new(PoolConfig::default());
    convert_confined_assets_in(
        &pool,
        &request,
        &output.parent_dir,
        Path::new(&output.file_name),
        &asset_base,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .unwrap();

    let ir = fs::read_to_string(inbox.join("note.json")).unwrap();
    assert!(
        !ir.contains(&hex::encode(Sha256::digest(PRIVATE_PNG))),
        "CRITICAL: a parent swap redirected image resolution to a private directory"
    );
    assert!(
        ir.contains(&hex::encode(Sha256::digest(PUBLIC_PNG))),
        "the image from the authorized directory must still be embedded"
    );
}

#[test]
fn pooled_confined_conversion_keeps_the_existing_call_signature() {
    use ariad_host::{
        convert::{ConvertRequest, convert_confined_in},
        session::{PoolConfig, SessionPool},
    };
    let temp = tempdir().unwrap();
    let input = temp.path().join("note.md");
    fs::write(&input, "# Compatible").unwrap();
    let parent =
        cap_std::fs::Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
    let request = ConvertRequest {
        input,
        output: temp.path().join("note.json"),
        target_format: "json".into(),
        profile: ariad_core::planner::Profile::Editable,
        overwrite: false,
        engine_program: std::env::current_exe().unwrap(),
        title_fallback: None,
        asset_base_dir: None,
        reader_engine: None,
    };
    let pool = SessionPool::new(PoolConfig::default());
    convert_confined_in(
        &pool,
        &request,
        &parent,
        Path::new("note.json"),
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    assert!(
        fs::read_to_string(request.output)
            .unwrap()
            .contains("Compatible")
    );
}

#[cfg(unix)]
#[test]
fn fifo_artifacts_preserve_type_errors_without_blocking() {
    use ariad_host::workspace::{Workspace, WorkspaceError};
    use std::{process::Command, time::Duration};

    if std::env::var_os("ARIAD_FIFO_ARTIFACT_CHILD").is_some() {
        let workspace = Workspace::new().unwrap();
        let fifo = workspace.output_dir().join("pipe.docx");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(matches!(
            workspace.open_artifact(&fifo),
            Err(WorkspaceError::ArtifactNotFile)
        ));
        let allowed = AllowedDirs::new(&[workspace.output_dir().to_path_buf()], &[]).unwrap();
        assert!(matches!(
            confine_input(&fifo, &allowed),
            Err(ConfineError::NotRegularFile { .. })
        ));
        return;
    }

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "fifo_artifacts_preserve_type_errors_without_blocking",
            "--nocapture",
        ])
        .env("ARIAD_FIFO_ARTIFACT_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "FIFO type classification failed: {status}"
            );
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("FIFO artifact or input opening blocked without a writer");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
mod staging_privacy {
    use super::*;
    use ariad_host::confine::ProcessStagingDirectory;
    use std::{
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    const CHILD: &str = "ARIAD_STAGING_PRIVACY_CHILD";

    struct Cleanup(ProcessStagingDirectory);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.cleanup();
        }
    }

    fn run_child(mut command: Command, name: &str) {
        let base = tempdir().unwrap();
        command
            .args(["--exact", name, "--nocapture"])
            .env(CHILD, "1")
            .env("TMPDIR", base.path())
            .env("TMP", base.path())
            .env("TEMP", base.path())
            .stdin(Stdio::null());
        let mut child = command.spawn().expect("spawn isolated staging regression");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "isolated staging regression failed: {status}"
                );
                return;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("isolated staging regression did not exit");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn permission_child(name: &str) {
        let mut command = Command::new("sh");
        command
            .args(["-c", "umask 0; exec \"$@\"", "staging-privacy"])
            .arg(std::env::current_exe().unwrap());
        run_child(command, name);
    }

    #[test]
    fn process_directory_is_owner_only_under_umask_zero() {
        if std::env::var_os(CHILD).is_none() {
            return permission_child(
                "staging_privacy::process_directory_is_owner_only_under_umask_zero",
            );
        }
        let directory = Cleanup(ProcessStagingDirectory::new().unwrap());
        assert_eq!(
            fs::metadata(directory.0.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );
    }

    fn inspect_copies(check_file: bool) {
        let source = tempdir().unwrap();
        let input = source.path().join("note.md");
        fs::write(&input, "# Permission regression").unwrap();
        assert_eq!(
            fs::metadata(&input).unwrap().permissions().mode() & 0o777,
            0o666,
            "fixture did not run under umask zero"
        );
        let allowed = AllowedDirs::new(&[source.path().to_path_buf()], &[]).unwrap();
        let mut input = confine_input(&input, &allowed).unwrap();
        let process = Cleanup(ProcessStagingDirectory::new().unwrap());
        let cancel = tokio_util::sync::CancellationToken::new();
        for variant in 0..3 {
            let (directory, path) = match variant {
                0 => input.create_isolated_copy(None, None),
                1 => input.create_isolated_copy_cancellable(None, None, &cancel),
                _ => {
                    input.create_isolated_copy_cancellable_in(process.0.path(), None, None, &cancel)
                }
            }
            .unwrap();
            let (path, expected) = if check_file {
                (path.as_path(), 0o600)
            } else {
                (directory.path(), 0o700)
            };
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o7777,
                expected,
                "staging API {variant} created permissive permissions"
            );
        }
    }

    #[test]
    fn copy_directories_are_owner_only_under_umask_zero() {
        if std::env::var_os(CHILD).is_none() {
            return permission_child(
                "staging_privacy::copy_directories_are_owner_only_under_umask_zero",
            );
        }
        inspect_copies(false);
    }

    #[test]
    fn staged_files_are_owner_only_under_umask_zero() {
        if std::env::var_os(CHILD).is_none() {
            return permission_child(
                "staging_privacy::staged_files_are_owner_only_under_umask_zero",
            );
        }
        inspect_copies(true);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_directories_owned_by_another_user_are_not_followed_or_swept() {
        use std::{
            os::unix::fs::{MetadataExt, chown, symlink},
            time::SystemTime,
        };
        if std::env::var_os(CHILD).is_none() {
            let executable = std::env::current_exe().unwrap();
            let command = if nix::unistd::geteuid().is_root() {
                Command::new(executable)
            } else {
                let probe = Command::new("unshare")
                    .args(["--user", "--map-auto", "--map-root-user", "true"])
                    .output();
                let unavailable = match probe {
                    Ok(output) if output.status.success() => None,
                    Ok(output) => Some(format!(
                        "unshare --user --map-auto --map-root-user true: {}; {}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    )),
                    Err(error) => Some(format!(
                        "unshare --user --map-auto --map-root-user true: {error}"
                    )),
                };
                if let Some(reason) = unavailable {
                    assert!(
                        std::env::var("ARIAD_REQUIRE_FOREIGN_OWNER_TEST").as_deref() != Ok("1"),
                        "foreign-owner regression is required but unavailable: {reason}"
                    );
                    eprintln!("SKIP foreign-owner integration regression: {reason}");
                    return;
                }
                // A mapped user namespace creates a real foreign owner without host-root access.
                let mut command = Command::new("unshare");
                command
                    .args(["--user", "--map-auto", "--map-root-user"])
                    .arg(executable);
                command
            };
            return run_child(
                command,
                "staging_privacy::stale_directories_owned_by_another_user_are_not_followed_or_swept",
            );
        }
        assert!(
            nix::unistd::geteuid().is_root(),
            "fixture requires a mapped root UID"
        );
        let base = std::env::temp_dir();
        let mut exited = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let dead_pid = exited.id();
        assert!(exited.wait().unwrap().success());
        let foreign = base.join(format!("ashift-mcp-stage-{dead_pid}-foreign"));
        let owned = base.join(format!("ashift-mcp-stage-{dead_pid}-owned"));
        let outside = base.join("unrelated");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.md"), "outside fixture").unwrap();
        for path in [&foreign, &owned] {
            fs::create_dir(path).unwrap();
            fs::write(path.join("keep.md"), "stale fixture").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o777)).unwrap();
            fs::File::open(path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(48 * 3600))
                .unwrap();
        }
        symlink(&outside, foreign.join("link")).unwrap();
        fs::File::open(&foreign)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(48 * 3600))
            .unwrap();
        struct RestoreOwner(PathBuf);
        impl Drop for RestoreOwner {
            fn drop(&mut self) {
                let _ = chown(&self.0, Some(0), None);
            }
        }
        let _restore = RestoreOwner(foreign.clone());
        chown(&foreign, Some(1), None).unwrap();
        assert_eq!(foreign.metadata().unwrap().uid(), 1);
        let link = base.join(format!("ashift-mcp-stage-{dead_pid}-link"));
        symlink(&foreign, &link).unwrap();
        let process = Cleanup(ProcessStagingDirectory::new().unwrap());
        assert!(!owned.exists(), "positive sweep control was not removed");
        assert_eq!(
            foreign.metadata().unwrap().uid(),
            1,
            "foreign owner changed"
        );
        assert_eq!(
            fs::read_to_string(foreign.join("keep.md")).unwrap(),
            "stale fixture"
        );
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(
            fs::read_to_string(outside.join("keep.md")).unwrap(),
            "outside fixture"
        );
        drop(process);
    }
}

#[cfg(unix)]
#[test]
fn startup_explicit_handles_survive_replacement_of_their_canonical_path() {
    use std::io::Read;
    let temp = tempdir().unwrap();
    let original = fs::canonicalize(temp.path()).unwrap().join("root");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("note.md"), "original authorized bytes").unwrap();
    let explicit = AllowedDirs::new(std::slice::from_ref(&original), &[]).unwrap();
    fs::rename(&original, temp.path().join("moved-root")).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("note.md"), "replacement bytes").unwrap();
    for client_roots in [vec![], vec![path_to_file_uri(&original)]] {
        let refreshed = explicit.with_client_roots(&client_roots);
        let mut input = confine_input(&original.join("note.md"), &refreshed).unwrap();
        let mut text = String::new();
        input.file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "original authorized bytes");
    }
}
