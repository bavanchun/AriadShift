# Worker 5-T1 result: CloudConvert job UX study

## Files changed

- `plans/reports/rea-261025-cloud-converter-ux.md` (new): the study and the MUST/SHOULD requirements for web cloud mode.
- `plans/reports/worker-5-t1-opus55.md` (new): this result file.

No other file was touched.

## Commits

- `docs: add cloud converter UX study and cloud-mode requirements` on `feat/cloud-product--rea`.

## What was done

- The REA MCP tools (`mcp__rea__*`) and the `reverse-engineer-anything` skill are not installed on this machine. As the brief allows, the study used `agent-browser` 0.39.1 (the `ak:agent-browser` skill) in its own named session. The mise shim for `agent-browser` fails in this shell ("No version is set for shim"), so the binary under `~/.local/share/mise/installs/node/24.22.0/bin/` was called directly.
- Studied the anonymous `cloudconvert.com` flow with synthetic files only (`hello.md` 41 B, `synthetic-3mb.md` 3.4 MB of filler lines, `broken.png` 17 B of text). Six conversions, below the free daily allowance; no account, no payment, no limit exceeded. Three jobs were deleted in-page; three expire on CloudConvert's 24-hour schedule.
- Captured accessibility-tree snapshots, a visible-text timeline of every state, the network shape (job tasks, Frankfurt presigned upload, socket.io status channel), focus and tab order, live regions, the error and offline states, the delete behaviour (`DELETE /v2/jobs/{id}` with no confirmation), the privacy, security and pricing claims, and desktop and 390 px screenshots. Screenshots stay in the session scratchpad; five are described in text in the study.
- Mapped the findings onto AriadShift's `/v1` contract (`UploadStatus`, `JobStatus`, `JobEvent.kind`, `ErrorDetail.code`) and the plan table in `docs/server.md`, and wrote requirements for the destination choice and privacy disclosure, staging, upload progress, the job step model, download, delete and retention, error copy per code, and keyboard/ARIA, marking where AriadShift should be more explicit than CloudConvert.

## Verification for the coordinator

Documentation only; nothing to build. To check the two Markdown files:

```sh
typos plans/reports/rea-261025-cloud-converter-ux.md plans/reports/worker-5-t1-opus55.md
just lint-commits
```

`typos` was run locally on the study and passed.

## Open concerns

- The API exposes plan limits, remaining quota and retention only through refusals. Showing limits before upload (a MUST in the study) needs a small read endpoint or fields on `/v1/capabilities`. That is an API change for the server owner.
- `/v1` has no client call to delete an upload that never became a job, so "Cancel upload" cannot promise that nothing was kept. The study gives interim copy; an upload `DELETE` would let the UI make the stronger promise.
- The `agent-browser` mise shim is broken in worker shells on this box (no global node version for the shim).

Status: DONE_WITH_CONCERNS
