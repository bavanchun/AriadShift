# ToolCtx construction protection

Status: incomplete. The shared build lock remained held by another worktree's
long-running build, so the required checks could not run. No other process was
stopped. The branch has not been pushed because verification is incomplete.

## Issue #142

`ToolCtx` had public fields and no `#[non_exhaustive]` attribute; the literal
in `ariad-host/src/tools/inprocess.rs` demonstrated that callers could depend
on the complete field list. Adding a field would therefore break such callers.

The struct is now non-exhaustive. The host constructs it using the existing
`with_cancel` and `with_max_pages` methods and assigns its existing public
`deadline` field, preserving the shared cancellation flag and limits. The
public documentation includes a runnable constructor example and a compile-fail
example for external struct literals. The unreleased notes describe the API.
The existing `new`, `with_cancel`, `with_deadline`, and `with_max_pages`
methods were assumed to be the intended public construction API.

## Verification

- PASS: `git diff --check` before this report.
- BLOCKED: `cargo test -p ariad-pdf --doc`; the lock was held by another
  worktree and the waiting process was stopped cleanly. The pre-fix reproduction
  also waited on that lock and did not run.
- NOT RUN: `cargo fmt --all --check`, the requested clippy command,
  `cargo test -p ariad-pdf --lib`, WASM target check and fuzz check. The WASM
  target is installed, and the fuzz target constructs a context, so both
  conditional checks remain required.
- NOT RUN: cross-platform CI; only the local Linux environment was inspected.

## Suggested PR

Title: `refactor(pdf): make ToolCtx non-exhaustive with a constructor`

Body: Protect `ToolCtx` from breaking field additions before publication and
migrate the host to its existing construction methods. Add public API examples
and release notes. Fixes #142. Run the required checks before merging.
