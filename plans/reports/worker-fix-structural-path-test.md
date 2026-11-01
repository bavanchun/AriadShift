# Structural tool output path test

The host canonicalizes the destination directory in `crates/ariad-host/src/tools/publish.rs` before constructing published paths. The structural tool tests compared those paths literally to paths constructed from the original output directory. Equivalent paths can differ on macOS and Windows.

The `all_twenty_five_structural_tools_golden_facts_and_deterministic_bytes` test and `multi_output_pdf_split_lists_every_artifact_in_human_and_json_modes` test now assert each expected file exists and compare canonicalized actual and expected paths. This covers human and JSON output. CLI output is unchanged. The task supplied no issue number, so the suggested PR identifies the issue by its description.

## Validation

- `git diff --check`: PASS.
- `cargo fmt --all --check`: PASS.
- `cargo test -p ariad-cli --features ariad-host/test-probe --test tools_structural`: PENDING. The manager directed the local test to run through the build lock after pushing. Earlier pre-fix and post-fix attempts were stopped while waiting for that lock and did not run.
- `cargo clippy -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`: NOT RUN, pending the build lock.
- macOS-26 and windows-2025 CI must verify the platform-specific paths after the branch is pushed.

The lock holder belonged to another worktree and was left untouched. Waiting for the lock is not a Cargo timeout. No assumption was made that the pending checks would pass.

## Suggested pull request

Title: test(cli): compare canonical output paths in structural tool tests

The structural output path test fails when the system resolves a temporary directory to a different spelling. Compare canonical output paths for the structural golden tests and the human and JSON split output checks, after verifying the files exist.
