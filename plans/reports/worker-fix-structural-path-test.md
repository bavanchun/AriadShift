# Canonical output containment and path comparisons

PR #153 macOS-26 Verify run 41326860298 reported two failures. The engine runner compared an artifact's spelling against the request's output directory with a lexical prefix check. An engine can return a canonical path while the workspace path passes through a symlink, so the same directory has two spellings. A local filesystem check reproduced that mismatch, but the requested pre-fix Cargo test could not start while the shared build lock was held. The JSON-output test also compared a canonical published path to the original spelling.

The runner now canonicalizes the output directory once per job, canonicalizes each existing artifact before checking containment, and rejects parent traversal and artifacts resolving outside the directory. Publication fails closed if its workspace output directory cannot be canonicalized. The Unix unit tests `artifact_accepts_output_directory_reached_through_symlink` and `artifact_symlink_outside_output_directory_is_rejected` cover the two boundaries. `confined_artifacts_are_recorded_once_and_rejections_preserve_the_list` now uses a real artifact because validation resolves it.

Path comparisons were updated in `tool_publish_atomic.rs` (explicit JSON artifact), `typst_engine.rs` (engine artifact), `convert_cli.rs` (default, explicit, HTML and inferred outputs), `convert_batch_cli.rs` (batch output lines and JSON input/output paths), `docx_golden.rs` (fixture output), and `pdf_routes.rs` (batch JSON output). The existing structural tool comparisons in `tools_structural.rs` were already canonicalized. Assumption: these comparisons assert file identity rather than the exact spelling supplied by the caller; line counts and output framing remain checked where applicable.

## Validation

- `git fetch origin && git rebase origin/dev`: PASS.
- Local symlink filesystem prefix reproduction: PASS (lexical comparison false, canonical comparison true); this is not a substitute for the requested pre-fix Cargo test.
- `cargo fmt --all --check`: PASS.
- `git diff --check`: PASS before commits.
- `sh scripts/check-commits.sh origin/dev..HEAD`: could not run because this worktree lacks `.tools/bin/committed`. The main checkout's `committed` passed against each non-merge commit using `committed.toml`; commit messages were also inspected.
- Symlinked `TMPDIR` failing-test reproduction: WAITING for the shared build lock. It was queued before the fix, but if it starts later it will see the changed workspace and is not a pre-fix reproduction.
- `cargo clippy -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`: NOT RUN, lock held.
- `TMPDIR=<symlinked dir> cargo test -p ariad-host --features test-probe --test tool_publish_atomic`, new unit tests, `engine_event_rules`, and `cargo test -p ariad-cli --features ariad-host/test-probe --test tools_structural`: NOT RUN, lock held.
- macOS-26 and windows-2025 Verify remain required; no CI result is claimed.

The lock holder is another worktree and has held the lock for more than 20 minutes. It and the queued wait were left untouched as instructed. Test and lint results must be obtained before treating the change as verified.

## Suggested pull request

Title: fix(host): compare canonical paths for output containment

Canonicalize engine output containment checks so symlinked workspace roots work while parent traversal and symlink escapes remain rejected. Compare published paths by filesystem identity in host and CLI tests. This addresses the macOS path failures in PR #153.
