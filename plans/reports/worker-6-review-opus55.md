# Worker p6-review result

Task: a read-only code review of phase 6 (v1 hardening) across three diffs, limited to
critical and high findings. No code was edited, and nothing was built or run.

## Files changed

- `plans/reports/code-review-261025-phase6.md`: the full review, with file:line,
  a scenario and a fix for each finding.
- `plans/reports/worker-6-review-opus55.md`: this result file.

## Commits

- `docs(plans): add the phase 6 code review` on `feat/v1-hardening--review`

## Verdicts

| Diff | Verdict |
|---|---|
| `origin/dev...origin/feat/v1-hardening` | FIX_REQUIRED |
| `origin/feat/cloud-foundation...origin/feat/v1-hardening--api` | MERGEABLE |
| `origin/feat/cloud-foundation...origin/feat/v1-hardening--mcp` | MERGEABLE |

## Findings for `feat/v1-hardening`

- **H1:** files opened from the OS are queued but never delivered to the window.
  `file_open::take` has no caller, `command_table.rs` has no command to drain the
  queue, and no frontend code listens for `open-files`. The "Convert with AriadShift"
  entries open the app with an empty selection on every platform.
- **H2:** the `uri_parsers` fuzz harness asserts `trim() == text` for anchors, which
  `allowed_link` accepts with whitespace. The input `#section-2 ` crashes it, so the
  CI Fuzz job and CI passed fail.
- **H3:** the nightly ASan job runs 9 targets × 600 s against `timeout-minutes: 90`,
  so it always times out. The workflow's `pull_request` trigger runs it on this
  branch's PR too.

Medium notes: the CI Fuzz job now runs 18 minutes of fuzzing inside a 35-minute
timeout; the Seatbelt profile grants read on all of `/opt/homebrew` and `/usr`; MCP's
exact `Host` match needs a proxy that forwards the original `Host` over HTTP/1.1.

The Seatbelt profile has no SBPL injection or unconfined fallback. The Explorer verbs
and Linux/macOS launch commands quote and separate arguments correctly. Upload
DELETE is workspace-scoped and race-safe with job creation, `/v1/limits` reports only
the caller's own usage, the freeze test can fail, and MCP checks audience, Origin,
Host, request size and quotas.

## Commands for the coordinator

```sh
just ci && just desktop-ci && just test-server
just fuzz uri_parsers 60
just fuzz archive_preflight 60 && just fuzz docx_meta 60
cargo test -p ariad-host --test sandbox_macos   # on macOS
```

Check H1 by hand: use a file-manager entry and confirm the files appear in the
Convert selection.

## Open concerns

- H1 needs a desktop frontend change that is outside T2's listed files, so the
  coordinator has to assign an owner.

Status: DONE_WITH_CONCERNS
