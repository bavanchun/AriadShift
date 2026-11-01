# Worker Fix Report: Stream Large Cloud Output Downloads (`stream-downloads`)

## Cause and Evidence

### Triage Verification
1. **Unbounded buffering in `@ariadshift/sdk` (`packages/sdk/src/client.ts`)**:
   - `AriadClient.download` previously called `await response.blob()` and `await blob.arrayBuffer()` (around line 502 of `packages/sdk/src/client.ts`), buffering the entire downloaded artifact twice in browser memory (as a `Blob` and as an `ArrayBuffer` for `sha256Hex`) before checking its length and SHA-256 digest.
2. **Callers buffering large cloud outputs (`apps/web`)**:
   - `CloudJob.download` (`apps/web/src/lib/cloud/cloud-job.ts`, lines 505-506) called `await this.client.download(...)` and `saveBlob(blob, output.filename)`.
   - `JobRow` (`apps/web/src/components/account/job-row.tsx`, line 238) called `await api.download(target)` and `saveBlob(blob, target.filename)`.
3. **Duplicated `saveBlob` helper**:
   - `saveBlob` was duplicated in `apps/web/src/lib/cloud/cloud-job.ts` (around line 92) and `apps/web/src/components/account/jobs-api.ts` (around line 95).

## Fix Summary

1. **Incremental Streaming Download & Integrity Verification (`packages/sdk/src/client.ts`)**:
   - Added `DownloadStreamTarget`, `DownloadIntegrityErrorCode`, and `DownloadIntegrityError` (extending `AriadApiError` with `code = "download_failed"` and `status = undefined`, plus `reason: "download_size_mismatch" | "download_hash_mismatch"`, `expected`, and `actual`).
   - Added `AriadClient.downloadToStream(output, target, options)` using `@noble/hashes/sha2.js` (`sha256.create()`, already locked in `@ariadshift/sdk` at `2.4.0`, MIT licence) to incrementally hash chunks from `response.body.getReader()`.
   - Enforced byte-count bounds during streaming: if received bytes exceed `output.size`, or if total bytes or final SHA-256 hex digest do not match `output.size` / `output.sha256`, `target.abort(err)` is invoked before throwing `DownloadIntegrityError`, ensuring partial or corrupted files are discarded and never finalized with `target.close()`.
   - Updated `AriadClient.download` to also throw `DownloadIntegrityError` on size or SHA-256 mismatch while preserving the `Blob` return contract for small downloads and existing callers.

2. **Thresholded Streaming & Bounded Fallback (`apps/web/src/lib/cloud/cloud-job.ts`)**:
   - Exported the shared `saveBlob` helper, `STREAM_DOWNLOAD_THRESHOLD_BYTES` (`16 MiB`), `FALLBACK_DOWNLOAD_LIMIT_BYTES` (`64 MiB`), and `downloadAndSaveOutput(client, output, options)`.
   - Below `16 MiB` (when no custom `target` is passed), `downloadAndSaveOutput` keeps the fast in-memory `client.download` + `saveBlob` path.
   - At or above `16 MiB`, `downloadAndSaveOutput` streams via `client.downloadToStream` into:
     1. A user-chosen `FileSystemWritableFileStream` via `window.showSaveFilePicker` when available on a user gesture;
     2. An Origin Private File System (`navigator.storage.getDirectory()`) temporary file writable that triggers `saveBlob(await handle.getFile(), output.filename)` only after `close()` succeeds and removes the temporary entry on `abort()` or after triggering the download;
     3. A bounded chunk sink capped at `FALLBACK_DOWNLOAD_LIMIT_BYTES` (`64 MiB`) when neither File System Access API nor OPFS writable streams are available in the browser, aborting before buffering unbounded data.
   - Updated `CloudJob.download` to use `downloadAndSaveOutput` so post-download server deletion (`deleteNow("download")`) only runs after the stream has passed integrity verification and closed cleanly.

3. **Account Jobs Deduplication & Streaming (`apps/web/src/components/account/jobs-api.ts`, `apps/web/src/components/account/job-row.tsx`)**:
   - Removed the duplicate `saveBlob` from `apps/web/src/components/account/jobs-api.ts` and exposed optional `downloadToStream` on `JobsApi`.
   - Updated `JobRow` in `apps/web/src/components/account/job-row.tsx` to call `downloadAndSaveOutput(api, target)`.

## Assumptions Recorded
- **Threshold (`16 MiB`) and Fallback Cap (`64 MiB`)**: Outputs below `16 MiB` use the existing in-memory `download` + `saveBlob` path. Outputs `>= 16 MiB` use `downloadToStream`. When a browser supports neither `showSaveFilePicker` nor OPFS `createWritable()`, the in-memory chunk fallback is bounded to `64 MiB` (`FALLBACK_DOWNLOAD_LIMIT_BYTES`) so memory usage cannot grow without bound.
- **Error Classification**: `DownloadIntegrityError` extends `AriadApiError` with `code = "download_failed"` and `status = undefined` so that both `/app` (`describeCloudError` in `apps/web/src/lib/cloud/errors.ts`) and `/jobs` (`apiErrorCopy` in `apps/web/src/components/account/job-status.ts`) surface transfer/download failure copy rather than generic network or 5xx server failure copy.

## Security Notes (`/ak-security`)
- **Incremental Cryptographic Verification**: Uses `@noble/hashes/sha2.js` (`2.4.0`, MIT) to compute SHA-256 over every chunk as it arrives.
- **Fail-Closed Writable Target Lifecycle**: `target.close()` is only called after both total byte length (`received === output.size`) and SHA-256 digest (`computed === output.sha256.toLowerCase()`) match. Any mid-stream overflow (`received > output.size`), truncation, network failure, or hash mismatch cancels the reader and calls `target.abort(err)`, discarding uncommitted data in `FileSystemWritableFileStream` / OPFS and preventing `saveBlob` from firing.
- **Server Output Retention on Integrity Failure**: Because `downloadAndSaveOutput` rejects before `CloudJob.download` reaches `deleteNow("download")`, a corrupted transfer does not delete the server copy prematurely, allowing the user to retry.

## Regression Tests Added
- `packages/sdk/test/client.test.ts`:
  - `streams a multi-chunk body with incremental SHA-256 and closes the target`
  - `aborts the stream target and throws DownloadIntegrityError on a corrupted chunk`
- `apps/web/test/cloud-job.test.ts`:
  - `streams downloads at or above the streaming threshold before deleting the job`
  - `streams a multi-chunk body and produces the right bytes and digest`
  - `aborts the writable target with DownloadIntegrityError on a corrupted chunk and leaves no saved file`
  - `keeps the fallback path bounded when File System Access API is unavailable`
- `apps/web/test/account-jobs.test.tsx`:
  - `streams a large output through downloadToStream when above the threshold`

## Verification Commands & Results
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm --filter @ariadshift/sdk exec vitest run test/client.test.ts` — **PASS** (21 passed)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm --filter @ariadshift/web exec vitest run test/cloud-job.test.ts test/account-jobs.test.tsx test/account-jobs-api.test.ts test/account-helpers.test.ts test/client.test.ts test/client-cancel.test.ts` — **PASS** (81 passed)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm --filter @ariadshift/sdk typecheck` — **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm --filter @ariadshift/web typecheck` — **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env UV_NO_CONFIG=1 just web-typecheck` — **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm exec biome check packages/sdk/src/client.ts packages/sdk/test/client.test.ts apps/web/src/lib/cloud/cloud-job.ts apps/web/src/components/account/job-row.tsx apps/web/src/components/account/jobs-api.ts apps/web/test/cloud-job.test.ts apps/web/test/account-jobs.test.tsx` — **PASS**
- `sh scripts/check-commits.sh origin/dev..HEAD` — failed only because `.tools/bin/committed` is missing in the worktree; ran `/home/vchun/Codes/01-AriadShift/.tools/bin/committed --config committed.toml origin/dev..HEAD` instead — **PASS**

## What Only CI or Another OS Can Verify
- Real browser native file-picker (`window.showSaveFilePicker`) and OPFS `FileSystemWritableFileStream` disk commit/discard behaviour across Chromium, Safari, and Firefox on macOS/Windows/Linux (unit tests run in `jsdom` with simulated `ReadableStream` and writable targets).
- Full workspace CI matrix across all packages and Rust crates.

## Suggested PR Text

**Title:** `feat(web): stream large cloud downloads with sha256 verification`

**Body:**
```markdown
Closes #67

### Summary
- Add `AriadClient.downloadToStream` and `DownloadIntegrityError` in `@ariadshift/sdk` to stream response bodies chunk-by-chunk, compute SHA-256 incrementally via `@noble/hashes`, and abort the destination writable before closing if byte length or SHA-256 mismatches.
- Add `downloadAndSaveOutput` in `apps/web/src/lib/cloud/cloud-job.ts` to keep downloads below 16 MiB on the in-memory `Blob` path while streaming larger outputs through File System Access API / OPFS writables (or a bounded 64 MiB fallback when unavailable).
- Deduplicate `saveBlob` between `apps/web/src/lib/cloud/cloud-job.ts` and `apps/web/src/components/account/jobs-api.ts`, and wire `/jobs` downloads through `downloadAndSaveOutput`.
```
