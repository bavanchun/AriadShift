# Verification: fuzz and SDK/web review findings on `feat/v1-hardening`

Refs: `origin/feat/v1-hardening` = `dcb34bc`, `origin/dev` = `e6bd29b`, merge base = `68ac304`. Read-only. Nothing was built, fuzzed, merged or tested. `git merge-tree` and `gh run list` needed approval and were not run, so the conflict and CI claims below rest on reading the content, not on execution.

Scope: every critical or high finding in `console-review-261027-1935-hardening-fuzz.md` and `console-review-261027-1935-hardening-sdk-web.md`. Neither report labels a medium finding "security", so I also checked the three mediums that sit on a trust or quota boundary (fuzz 3 and 4, SDK 4).

## 1. Verdict

**Needs fixes.** All three high findings hold. Fuzz 1 will fail the required CI Fuzz job. SDK 2 will fail the `openapi_drift` test in `just test`. SDK 1 blocks a clean merge, and the conflict reaches well beyond the SDK. I found no new high-severity issue in this scope.

## 2. Verification results

| Finding | Earlier severity | Result | Evidence on the branch |
|---|---|---|---|
| Fuzz 1: `ir_json_versioned` panics on its own seeds | high | **CONFIRMED** | `read_versioned` overwrites the version at `crates/ariad-host/src/ir_io.rs:207`. Plain `read` (`ir_io.rs:161-169`) keeps it. `Document` derives `PartialEq` over `version` (`crates/ariad-core/src/ir/mod.rs:18-22`). All four seeds carry `"version": "ariad-ir/0"` (`vi-garden-notice.json:44`, `en-fenced-code.json:40`, `en-footnotes.json:77`, `vi-aligned-table.json:186`). I checked that `vi-garden-notice.json` deserializes as a current `Document`: its heading, paragraph and text blocks are valid, and `source_format: "html"` matches `format.rs:9`. Under `Limits::local()` (`limits.rs:56-69`) it passes the byte, depth and block checks, so execution reaches `assert_eq!(plain, document)` at `ir_json_versioned.rs:29` and fails. The CI loop (`.github/workflows/ci.yml:424-436`) runs under `shell: bash`, which GitHub starts with `-e`. `ir_json_versioned` is in the target list and has a `[[bin]]` entry (`fuzz/Cargo.toml:155-159`). `CI passed` requires `FUZZ=success` on heavy runs (`ci.yml:547-572`). The target is new on this branch (`f57b1c1`) and does not exist on `dev`. **Addition:** replacing the seeds with `ariad-ir/1` ones would not fix this, because a single-byte mutation from `1` to `0` brings the panic back within seconds. The oracle itself has to change. |
| Fuzz 3: `engine_protocol` skips the host's event validation | medium | **CONFIRMED** | The harness only calls `serde_json::from_slice::<Event>` (`engine_protocol.rs:48-56`). `parse_event` (`runner.rs:825-862`) is private and is the code that enforces artifact confinement (`artifact_is_confined`, `runner.rs:864-875`), rejects capabilities on a convert request, and checks that `ok` and `error` agree. I found no false positive in the round-trip oracle. The untagged `Message` re-decodes to the same variant, `Limits` defaults are all `None` for its optional fields, and `serde_json` has `float_roundtrip` enabled (`fuzz/Cargo.toml:17`). |
| Fuzz 4: `pack_archive` asserts only counts | medium | **CONFIRMED** | `pack_archive.rs:38-55` asserts only `stats.entries`, `stats.bytes` and a ZIP entry-count bound. A broken entry rule would still return `Ok`, and no assertion would notice. |
| SDK 1: add/add conflicts with `dev` | high | **CONFIRMED**, and the conflict is wider than the report says | `packages/sdk/` does not exist at `68ac304`, and both sides add it. Three of the added files differ between the refs, so they are add/add content conflicts: `generator.lock.json:4`, `src/client.ts` (header at lines 1-8 plus the new hunks) and `test/events.test.ts:32,166-168`. In `schemas/openapi.v1.json`, the base has no `/v1/blobs/{token}` and both sides insert it in the same place (`dev:71`, branch `:72`) with different responses, so that region overlaps. **Outside the SDK**, the same add/add pattern hits nine files in `services/auth/`, plus `crates/ariad-server/tests/blob-transfers.rs` and `infra/compose/docker-compose.yml`, all with different content on each side. Many server files were also modified on both sides. Resolving the conflict therefore needs a full `dev` merge, not just an SDK fix-up. |
| SDK 2: committed OpenAPI spec is stale | high | **PARTLY**: the defect and the CI failure are confirmed, but the stated cause is wrong | `blobs.rs:60-61` and `:122-123` annotate 408 and 429 responses, and `blobs.rs` is identical on both refs. The branch spec has no `"408"` at all, and its blob operations list only 200/403/404/500 and 200/403/422/500 (lines 72-211). The frozen snapshot is the same blob (`4539714`). `openapi_drift.rs:8-16` compares the files byte for byte, and CI runs it through `just test` (`justfile:43-44`, `ci.yml:257`), so CI fails. **What is wrong:** the report says the spec was "hand-written and never regenerated". The branch spec is exactly the `d7eba58` blob. At that commit `blobs.rs` had no 408 or 429 annotations, so the spec was generated correctly for its own line of work. The annotations came from `f9deea5`, which is not an ancestor of `d7eba58` or `dddb062`. They arrived in integration merge `0c5f037`, and nobody regenerated the spec after that merge. **This answers the SDK report's question 2:** `openapi_v1_snapshot.rs:1-14` treats new response codes as additive. The snapshot test passes today and will still pass after regeneration. Refreshing the baseline is optional; it only freezes 408 and 429 into the contract. |
| SDK 4: `upload()` gives the caller no way to clean up | medium | **CONFIRMED** | `client.ts:285-297`: `upload()` has no `try` around `transfer` or `completeUpload`, and the thrown error carries no upload id. The server does count pending uploads against a quota (`api/limits.rs:83`, `dto.rs:388`). |

## 3. Findings, ranked

This is a correction list only. For the full defect text and fixes, see the earlier reports.

| Severity | File:line (branch) | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|
| high | `fuzz/fuzz_targets/ir_json_versioned.rs:19-29` | Fuzz 1, confirmed. The check at lines 19-22 always passes, and the comparison at line 29 is wrong for legacy input. | The first legacy seed panics during the initial corpus pass, and the required Fuzz job fails on every heavy run. | Compare after normalizing the version, assert `is_supported_version` on the input version, and add `ariad-ir/1` and future-version seeds. Seeds alone are not enough. |
| high | `schemas/openapi.v1.json:72-211`; `crates/ariad-server/tests/snapshots/openapi_v1.json` | SDK 2, confirmed. The spec went stale through the merge in `0c5f037`; it was not hand-written. | `openapi_drift` fails in the `verify` job (`just test`). Regenerating the spec then changes its hash, so `just sdk-check` fails until the lock is refreshed. | `just openapi`, then `pnpm --filter @ariadshift/sdk lock`. A snapshot refresh is optional. Most of this falls out of the `dev` merge if `dev`'s blob hunk is kept when the spec conflict is resolved. |
| high | `packages/sdk/*`, `schemas/openapi.v1.json`, `services/auth/*`, `crates/ariad-server/tests/blob-transfers.rs`, `infra/compose/docker-compose.yml` | SDK 1, confirmed and wider than reported. | A PR or merge into `dev` reports add/add conflicts across the SDK, the auth service, the server tests and compose. Taking the branch side wholesale silently drops `dev`'s Biome fix and its regenerated spec. | Merge `origin/dev` into the branch and resolve the conflicts file by file. Then regenerate the spec, refresh the SDK lock and run `just ci`. |

**New high-severity issues: none found.** I checked the other new fuzz targets for the same failure as fuzz 1: an oracle that panics on a committed seed or after a trivial mutation. None of them does.

- `asset_resolve`: the seeds contain no `data:` URIs, and `resolve` returns errors only for the base directory.
- `uri_parsers`: the oracles match `allowed_link` and `parse_file_uri`.
- `archive_preflight`: the name listing reuses `parse_central_directory`.
- `capabilities_json`: `plan` and `reachable_formats` run the same search; scores are products of values that `validate` keeps in `[0, 1]`; `p50_ms` is capped.
- `pack_manifest`: `validate` rejects duplicate triples.
- `docx_meta`: restamping is idempotent.

On the SDK side, `deleteUpload` and `limits` use the same `unwrap` pattern as the existing `deleteJob`.

## 4. Contract or architecture violations

I found no new violations. I did not re-verify the earlier low finding that `ARCHITECTURE.md` §14 is out of date.

## 5. Test gaps that matter

- No test pins the relationship between `read` and `read_versioned`. The fuzz target is the only check on it, and its oracle is wrong.
- No CI or local run backs any of the 18 PR fuzz targets on this branch.

## 6. Unresolved questions

1. Has the CI Fuzz job run on any commit that contains `f57b1c1`? A green run would contradict fuzz 1. `gh run list` needed approval, so I could not check.
2. I did not run `git merge-tree`, so the exact conflict list in SDK 1 is inferred from the content. Run `git merge-tree --write-tree origin/dev origin/feat/v1-hardening` before planning the merge.
