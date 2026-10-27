# Fix specs for the verified dev findings (host/engines, server/CLI/MCP, web/wasm)

- Ref: `origin/dev` at `e6bd29b`, the commit all three verification reports used. Every line number below refers to `origin/dev`. This is a read-only review: nothing was built or run.
- Inputs: `console-review-261027-2035-verify-dev-host-engines.md`, `console-review-261027-2035-verify-dev-server-cli-mcp.md` and `console-review-261027-2035-verify-dev-web-wasm.md`.
- Scope: one spec for each confirmed or partly confirmed finding, eleven in all. Two groups are left out. Host M3 (the LibreOffice `Link=2` claim) was refuted as likely wrong, so only its test gap is noted below. Low findings 4–16 of the server/CLI report were never re-verified.
- Spec IDs (MCP-1, HOST-2 and so on) exist only in this report. AGENTS and the review rules forbid finding codes in code comments, test names and commit messages, so workers must describe the behaviour instead.

## 1. Verdict

**Needs fixes.** Eleven findings hold: one high, eight medium and two low. They split into seven worker tasks with disjoint file ownership. Only the Docling native-library inventory (HOST-3) is blocked, because it needs a licence-policy decision and one built Linux pack.

## 2. Findings and specs, most severe first

| ID | Sev | File:line (origin/dev) | Defect | Failure scenario | Fix summary | Size | Worker |
|---|---|---|---|---|---|---|---|
| MCP-1 | high | `crates/ariad-cli/src/mcp/tools.rs:199-209`, `:258-268`, `:383-393` | All three MCP tools stage input with `create_isolated_copy(None, Some(ANALYSIS_MAX_BYTES))`, a 32 MiB cap. A larger input fails as `input_io` with exit 2. The CLI has no such cap. | For a 33 MiB `big.md`, MCP `inspect`, `plan` and `convert` return `isError` with `input_io`/2. `ashift inspect --json big.md` exits 0 with a `document_too_large` warning. This breaks the CLI parity promised at `ARCHITECTURE.md:220`. | Stage without a cap and map copy errors through `ConvertError`. | S | W1 |
| SRV-1 | medium | `crates/ariad-server/src/storage.rs:245-261`, `:486`, `:565`; `api/blobs.rs:67-145`; `config.rs:305-320`; `app.rs:33-38` | One global budget of 4 blob transfers per process, with no per-client rule. `with_blob_transfer_limits` has no production caller. | An anonymous client keeps 4 slow PUTs (or 4 slow GETs of one output URL) open for up to 15 minutes, and every other `/v1/blobs` request gets 429. | Admit transfers per subject using a keyed subject tag in the token claims. Make the limits configurable through `ApiConfig` and apply them in `api_state`. | M | W2 |
| MCP-2 | medium | `crates/ariad-cli/src/mcp/mod.rs:67-94`, `:134-136` | `refresh_roots_with_peer` does nothing when `list_roots()` fails, so a failed refresh after `roots/list_changed` keeps the old roots. | The client revokes root A and answers the server's `roots/list` with a JSON-RPC error. Tools and `resources/read` still accept paths under A. | When a list-changed refresh fails or times out, fall back to the `--allow-dir` set only. | S | W1 |
| HOST-1 | medium | `engines/docling/pack/licenses.py:111-121`, `:125-133`, `:200-211` | `license_of` drops an unrecognised `License` field and every unrecognised classifier, GPL/AGPL classifiers included, then derives a licence from the classifiers it does know. | `License="GPL-3.0-or-later"` next to an MIT classifier, or MIT and AGPL classifiers together, both pass the gate, and the pack manifest records `MIT`. | Fail closed on any unrecognised field or classifier, and add a raw-text deny pass. | S | W3 |
| HOST-2 | medium | `crates/ariad-core/src/package_meta.rs:40-42` | `stamp_into` rejects any entry whose declared size is over 64 MiB, even media entries it only raw-copies. Stamping runs on every Pandoc DOCX/EPUB output. | Markdown with a data-URI image over 64 MiB, converted locally to DOCX or EPUB: Pandoc succeeds, then stamping fails with exit 1. | Cap only the properties entry and keep the 256 MiB total budget in `stamp_bytes`. | S | W4 |
| HOST-3 | medium (partly; blocked) | `engines/docling/pack/licenses.py:321-352`; `build.py:634-637`, `:702-705` | Native libraries vendored inside installed wheels (`*.libs/`, `.dylibs/`) are never inventoried. `check_runtime_files` skips `site-packages` and runs before `install_wheels`. | A Linux pack probably ships `torch.libs/libgomp*` (GPL-3.0 with the GCC exception), and possibly gfortran/quadmath from numpy/scipy, with no reviewed notice. This contradicts `ARCHITECTURE.md:516` and `licenses.py:3`. | Run an inventory check over vendored native files after the last site-packages change, backed by a reviewed allow-list. | M | W3 |
| WEB-1 | medium | `apps/web/src/converter/session.ts:102-106`; `engines.ts:85-111` | Every engine group gets `max_input_bytes = 16 MiB`. The second group of a DOCX/EPUB/PDF → Markdown/HTML route reads the intermediate IR JSON under that cap (`pipeline.rs:310-313`), and base64 media inflate that IR. | A 12.5 MiB DOCX holding one 12.5 MiB JPEG fails with `limit_exceeded` when converted to Markdown or HTML in `/app`. | Apply the input cap to the first group only. | S | W6 |
| WEB-2 | medium (UX; partly) | `apps/web/src/components/converter/converter-app.tsx:565-570`, `:611-616`; `apps/web/src/converter/client.ts:60-87` | A running conversion has no cancel path: Convert and "Start over" are both disabled while busy. | A long Pandoc or Typst run on a phone stays on "Converting…" until the user reloads or leaves the page, and the worker's OPFS folder is left for the next sweep. | Add `ConverterClient.cancel()`, which retires the worker, and a Cancel button. | M | W6 |
| WEB-3 | medium | `apps/web/src/app/licenses/page.tsx:48-61`; `apps/web/package.json` scripts; `about.toml` targets | The web build serves no Rust or JS notice texts, no MPL-2.0 source pointer and no link to `SOURCE.txt`. The only NOTICE link points at GitHub `blob/dev`. | Visitors receive the wasm and JS without the MIT, BSD, Apache-2.0 and MPL-2.0 notices that their licences require. | Serve the generated notices from `/notices/`, link them, and add a web JS notice generator and check. | M | W7 |
| HOST-4 | low (partly) | `crates/ariad-host/src/runner.rs:334`, `:785-807`; `engines/libreoffice.rs:201-222` | After a successful one-shot run on Unix, `reap(false)` neither kills the group nor bounds the stderr join. The LibreOffice `--version` probe kills only the direct child, and its stdout join is unbounded. | An engine leaves a descendant that holds stderr: `run_command` blocks for as long as the descendant lives, ignoring cancellation, and the descendant outlives the job. | Kill the group after every one-shot run, bound the stderr join, and give the probe its own process group. | M | W5 |
| HOST-5 | low (partly) | `crates/ariad-host/src/session.rs:382-395`, `:513-517` | A sandbox launcher failure in session mode comes back as an unclassified `EngineFailed` and is marked reusable. | A transient Landlock or seccomp failure after the pool's describe step is reported as exit 1 instead of `sandbox_unavailable` (exit 5). | Classify it with a helper shared with the one-shot path, and drop the dead process. | S | W5 |

## 3. Implementation specs

### MCP-1: MCP staging cap breaks CLI parity (high, S, W1)

- **Files:** `crates/ariad-cli/src/mcp/tools.rs`, `crates/ariad-cli/tests/mcp_stdio.rs`.
- **Behaviour change:**
  1. In `inspect` (`:199-209`) and `plan` (`:258-268`), call `create_isolated_copy(None, None)`. Host `inspect`/`plan` then apply their own 32 MiB and 4 MiB rules (`commands/inspect.rs:161-178`, `commands/plan.rs:83`). A prefix copy will not do: host `inspect` streams UTF-8 validation over the whole Markdown file (`:163`) and preflights the ZIP central directory (`:135-137`), and both need the full file.
  2. In `convert` (`:383-393`), pass `Limits::local().max_input_bytes`, which is `None` today. That keeps MCP tied to the CLI's local limits (`convert.rs:575`).
  3. Replace the three hard-coded `tool_error("input_io", 2, …)` blocks with one helper. `io::ErrorKind::FileTooLarge` becomes `tool_convert_error(&ConvertError::LimitExceeded)` (`limit_exceeded`, exit 4), and any other error becomes `tool_convert_error(&ConvertError::InputIo)` (`input_io`, exit 1, matching `convert.rs:345`). Drop the `ANALYSIS_MAX_BYTES` import if nothing uses it any more.
- **Proof tests** (`mcp_stdio.rs`):
  - Create a file of `ANALYSIS_MAX_BYTES + 1` bytes with the same fixture shape as the CLI tests at `crates/ariad-cli/tests/commands_cli.rs:1200-1220` (inspect) and `:1390-1430` (plan).
  - Assert that MCP `inspect` and `plan` return `isError: false` and that their `structuredContent` equals the CLI `--json` output, including the `document_too_large` warning.
  - Assert that MCP `convert` of a plain-text Markdown file just over 32 MiB to `html` succeeds and writes the output.
  - Unit-test the error helper: `FileTooLarge` maps to `limit_exceeded`/4, and other errors map to `input_io`/1.
- **Risk:** low. `inspect` and `plan` now copy the whole input to a temporary directory, which costs disk space and time on multi-GB files. `convert` already pays that cost. The MCP error contract for staging failures changes (exit 2 becomes 1, and the copy can now return `limit_exceeded`); this restores parity with the CLI, so say so in the PR.

### SRV-1: one client can take the whole blob transfer budget (medium, M, W2)

- **Files:** `crates/ariad-server/src/storage.rs`, `config.rs`, `app.rs`, `api/blobs.rs`, `api/uploads.rs`, `api/jobs.rs`, `crates/ariad-server/tests/blob-transfers.rs`, `docs/server.md`. If the utoipa text in `blobs.rs` changes, also regenerate the OpenAPI document that `tests/openapi_drift.rs` checks.
- **Behaviour change:**
  1. **Claims.** `BlobClaims` (`storage.rs:107-123`) gains `#[serde(rename = "s", default, skip_serializing_if = "Option::is_none")] subject: Option<String>`. The value is a keyed tag, for example the first 16 bytes, hex-encoded, of HMAC-SHA256 over `"blob-subject:" + principal.subject` with the local signing secret. The raw `ip:<hash>` or `ws:<uuid>` subject never goes into the URL. Tokens issued before the deploy have no `s`, live at most 15 minutes, and fall back to the global budget only.
  2. **Signing.** `presign_put(key, size, sha256, subject)` and `presign_get(key, subject)` take `&caller.principal.subject` from `uploads.rs:130-134` and `jobs.rs:392-396`. S3 signing ignores the new argument. Update the test caller at `storage.rs:710`.
  3. **Admission.** `admit(upload, subject)` (`storage.rs:245-261`) keeps the global count and the one-PUT-per-key rule. It adds `HashMap<String, usize>` per subject tag and returns `Busy` when the subject already holds `max_per_subject` slots. `TransferPermit` (`:149-162`) decrements the subject count on drop. `put_stream_until` and `get_stream_until` take `subject: Option<&str>`, which `blobs.rs` fills from `claims.subject.as_deref()`.
  4. **Configuration.** `BlobTransferLimits` (`config.rs:307-320`) gains `max_per_subject: usize`, default 1. `ApiConfig` (`:325-383`) gains `blob_limits`, read from `ARIAD_BLOB_MAX_CONCURRENT` (default 4), `ARIAD_BLOB_MAX_PER_SUBJECT` (default 1), `ARIAD_BLOB_IDLE_TIMEOUT_S` (default 15) and `ARIAD_BLOB_TRANSFER_TIMEOUT_S` (default 900). Configuration is rejected when a value is zero or `max_per_subject > max_concurrent`. `api_state` (`app.rs:34-38`) applies `.with_blob_transfer_limits(config.blob_limits)?`.
  5. **Docs.** Rewrite `docs/server.md:97` to describe the per-subject rule, the four variables, and the fact that IPv6 /64 rotation still calls for per-client limits at ingress. Add the variables to the env reference in `docs/server.md`.
  6. Keep PUT replay inside the URL window, since `sweeper.rs:135-139` documents it as intended, unless Q3 below says otherwise. If it does, also reject PUTs whose upload row is not `pending`.
- **Proof tests** (`tests/blob-transfers.rs`):
  - With `max_concurrent = 4` and `max_per_subject = 1`, subject A holds a stalled PUT. A second PUT from A on another key gets 429, and a PUT from subject B is admitted.
  - Two concurrent GETs of one output URL: the second gets 429.
  - A legacy token without `s` is admitted under the global budget only.
  - `ApiConfig::from_env` rejects `ARIAD_BLOB_MAX_PER_SUBJECT=0` and `5` when the global maximum is 4.
  - `api_state` applies configured limits: with `max_concurrent = 1`, the second transfer is refused.
- **Risk:** medium.
  - The token format changes, though it stays backward compatible through the serde default.
  - The per-subject default of 1 makes a client that downloads several outputs in parallel see 429. The SDK's `download`, `downloadStream` and `transfer` do not retry on 429 (`packages/sdk/src/client.ts:377-401`). See Q3.
  - Only local (`file://`) storage is affected. The S3 path is unchanged.

### MCP-2: roots refresh fails open (medium, S, W1)

- **Files:** `crates/ariad-cli/src/mcp/mod.rs`, `crates/ariad-cli/tests/mcp_stdio.rs`, `docs/mcp.md`.
- **Behaviour change:**
  1. Give `refresh_roots_with_peer` a `fail_closed: bool` parameter. `on_initialized` (`:130-132`) passes `false`; `on_roots_list_changed` (`:134-136`) passes `true`.
  2. Wrap `peer.list_roots()` in `tokio::time::timeout(ROOTS_LIST_TIMEOUT, …)`, with a timeout of about 10 s, because it is unverified whether rmcp 3.5.1 times out server-to-client requests.
  3. On `Err` or timeout with `fail_closed`, install `AllowedDirs::new(&existing_allow_dirs, &[])`, which keeps only the `--allow-dir` directories, or `AllowedDirs::default()` if that call fails. Without `fail_closed` (the client may simply lack roots), keep today's no-op.
  4. Take the write lock with `unwrap_or_else(PoisonError::into_inner)` so a revocation is never skipped silently. Today `if let Ok(…)` skips it at `:82` and `:88`.
  5. Extend `docs/mcp.md:85` to say that a failed or unanswered `roots/list` after `list_changed` drops client roots until the next successful refresh.
- **Proof tests** (`mcp_stdio.rs`):
  - Give the test client's automatic `roots/list` responder (`:143-170`) a switch that answers with a JSON-RPC error.
  - Test 1: no `--allow-dir`, roots `[A]`. `convert` in A succeeds. Flip the switch, send `notifications/roots/list_changed` and wait for the cycle. `inspect` on `A/x.md` now returns the confinement error, and `resources/read` of the earlier output fails.
  - Test 2: `--allow-dir B` plus roots `[A]`. After the failed refresh, B still works and A is refused.
- **Risk:** low. A client with a flaky `roots/list` loses root-granted access until its next successful `list_changed`. That is the intended fail-closed trade-off.

### HOST-1: the Docling licence gate fails open (medium, S, W3)

- **Files:** `engines/docling/pack/licenses.py`, `engines/docling/tests/test_licenses.py`.
- **Behaviour change** in `license_of` (`:103-122`):
  1. After `OVERRIDES` and `License-Expression`, return `None` when `License` is non-empty and not in `_KNOWN`. Today it falls through to the classifiers at `:114`.
  2. Collect the tail of every `License ::` classifier. Ignore the bare umbrella entry `License :: OSI Approved` (tail `osi approved`). If any remaining tail is not in `_KNOWN`, return `None`.
  3. In `check` (`:200-211`), before calling `verdict`, run `_DENIED` over the raw `License` field and every `License ::` classifier. Extend that raw-text pass with `(?<!Lesser )General Public License|Affero`, because classifier text such as "GNU Affero General Public License v3" contains no `AGPL` token. A match produces `Finding(name, <raw>, "excluded licence")`.
  4. Run the gate over the locked environment. For each distribution that now fails only because its `License` field holds full licence text, add a reviewed `OVERRIDES` entry with a comment, following the `scipy` pattern at `:87-88`. List each new override in the PR.
- **Proof tests:**
  - Add both cases from the verification report to the parametrised table (`test_licenses.py:43-68`) as `False`: `License="GPL-3.0-or-later"` plus an MIT classifier, and MIT plus "GNU Affero General Public License v3" classifiers.
  - Add `["License :: OSI Approved", "License :: OSI Approved :: MIT License"]` as `True`.
  - Add an unknown classifier next to MIT (for example "Other/Proprietary License") as `False`.
  - `test_installed_distributions_pass_the_gate` (`:38-40`) must stay green over the locked set.
- **Risk:** low to medium. Distributions that pass today may start failing, which blocks the pack build until their overrides are reviewed. That is the intended fail-closed behaviour.

### HOST-2: the 64 MiB stamping cap breaks large local DOCX/EPUB output (medium, S, W4)

- **Files:** `crates/ariad-core/src/package_meta.rs`, `crates/ariad-host/src/package_meta.rs` (tests).
- **Behaviour change:**
  1. In `stamp_into` (`:38-68`), remove the declared-size check at `:40-42` for non-target entries. They are raw-copied (`:50`), never decompressed, so they carry no zip-bomb or memory risk. Check `entry.size() > MAX_ENTRY_BYTES` inside the target branch before reading, and keep the `take(MAX_ENTRY_BYTES + 1)` bound (`:59-64`).
  2. Change the `EntryTooLarge` message (`:17`) to "package properties entry is larger than 64 MiB".
  3. Give `stamp_bytes` (`:76-87`) a distinct `PackageTooLarge` variant ("package is larger than 256 MiB") for its 256 MiB total budget, so web errors stay accurate (`ariad-wasm/src/lib.rs:283-289` surfaces `e.to_string()`).
  4. Leave `convert.rs:1052-1058` alone.
- **Proof tests:**
  - Rewrite `rejects_a_zip_entry_over_the_uncompressed_size_cap` (host `package_meta.rs:246-263`) as a success test. DOCX and EPUB packages that contain a non-target entry of `MAX_ENTRY_BYTES + 1` bytes stamp successfully, and that entry's compressed bytes are unchanged (use `compressed_entry_bytes`).
  - Add `rejects_an_oversized_properties_entry`: a `docProps/core.xml` (and an `.opf`) over 64 MiB still returns `EntryTooLarge`.
  - Add a core test that `stamp_bytes` returns `PackageTooLarge` when declared sizes exceed 256 MiB. Use deflated filler so the compressed archive stays small.
- **Risk:** low. Native and web callers change behaviour only for packages that fail today. Cloud is unaffected, because `Limits::cloud()` caps assets at 50 MiB.

### HOST-3: vendored native libraries in installed wheels are not inventoried (medium, M, W3, blocked on Q1 and Q2)

- **Files:** `engines/docling/pack/licenses.py`, `engines/docling/pack/build.py`, a new `engines/docling/pack/vendored-libraries.toml`, new notice texts under `engines/docling/pack/licenses/`, and `engines/docling/tests/test_licenses.py`. `ARCHITECTURE.md` changes only if Q1 changes the policy.
- **Behaviour change:**
  1. Add `check_vendored_native_files(site, inventory, target)`. It lists files matching `*.so*`, `*.dylib`, `*.dll` and `*.pyd` inside wheel vendor directories only: `*.libs/` (auditwheel and delvewheel) and `**/.dylibs/` (delocate). A distribution's own extension modules stay covered by the metadata gate. Every listed file must match one reviewed inventory entry (`name`, `distribution`, `license` as SPDX, `notice`, and `native_files` per platform). An unmatched file, or an entry whose patterns match nothing on the target platform, fails the build. This mirrors `check_runtime_files` (`:321-352`).
  2. In `build.py`, call it after the last site-packages change (after `refresh_wheel_records(site)` at `:702`) and before `write_notices` (`:705`).
  3. Append each entry's licence to the `pack_expression` extras (`build.py:467-476`) and copy its notice into `notices/` the way `write_runtime_notices` does.
  4. Q1 decides the content. If the GCC-exception libraries (`libgomp`) and LGPL (`libquadmath`) are acceptable, list them with their exception text, handle `GCC-exception-3.1` explicitly (the `_DENIED` regex at `:57` would refuse it in any `verdict` path), and update `ARCHITECTURE.md:516`, the `licenses.py:3` docstring and the Decision Log. If they are not, build Linux torch against LLVM OpenMP as Windows already does (`torch_build.py:353`, `:386-388`) and leave `libgomp` unlisted so the check fails on it.
- **Proof tests:**
  - With a `tmp_path` site containing `torch.libs/libgomp-abc.so.1` and `numpy.libs/libgfortran-x.so.5`: the check fails while they are unlisted, passes once they are listed, and fails for an entry that matches nothing.
  - One real Linux x86_64 pack build passes the check, and its `notices/` contains each listed library's text.
- **Risk:** medium. The check can break pack builds on platforms nobody has inspected yet, and the mechanism cannot land without a correct inventory. Inspect one built pack first (Q2).

### WEB-1: the browser input cap is applied to intermediate IR (medium, S, W6)

- **Files:** `apps/web/src/converter/engines.ts`, a new `apps/web/test/engines.test.ts` (or additions to `apps/web/test/session.test.ts`).
- **Behaviour change:** in `runRoute` (`engines.ts:94-106`), pass `options` unchanged to the first group. Later groups get `{ ...options, limits: <options.limits without max_input_bytes> }`. Add a one-line comment saying that `ingest` already capped the user's bytes (`session.ts:62`) and that the intermediate IR was produced in-process. `session.ts` stays unchanged. All other limits, including `max_ir_json_bytes`, still apply to later groups. This is limits plumbing only; no conversion logic moves into the surface.
- **Proof tests:**
  - Use a fake `Core` with recording engines `pandoc` and `ariad-core`, and a route `docx→ariad-ir+json (pandoc)` then `ariad-ir+json→markdown (ariad-core)`. The first call sees `max_input_bytes === 16 MiB`; the second sees no `max_input_bytes` and still sees the other limits.
  - A one-group route keeps the cap.
  - Optional e2e in `heavy-routes.spec.ts`: a DOCX generated at test time with one incompressible image of about 12.5 MiB converts to Markdown. This keeps the fixture licence rules satisfied.
- **Risk:** low. Later groups can now read IR above 16 MiB, but that IR is already in worker memory, and the post-convert recycle check (`session.ts:119`, `:133-135`) still applies. `docs/web.md:40` becomes accurate and needs no edit.

### WEB-2: no way to cancel a browser conversion (medium UX, M, W6)

- **Files:** `apps/web/src/converter/client.ts`, `apps/web/src/components/converter/converter-app.tsx`, `apps/web/test/client.test.ts`, `apps/web/test/converter-app.test.tsx`, `docs/web.md`.
- **Behaviour change:**
  1. Add `ConverterClient.cancel()`, which calls `this.#retire()` and then `#failAll({ code: "cancelled", message: "The conversion was cancelled.", fatal: false })`. Reusing `#retire` (`:79-87`) records the worker's session, so the next fresh worker deletes its OPFS folder (`:45-51`). That is the step `terminate()` skips.
  2. In `converter-app.tsx`, add an outline "Cancel" button next to Convert while `phase === "converting"`. Its handler calls `client().cancel()` and then `await client().request({ type: "reset" })`, which reaches a fresh worker that retires the old folder, as `reset` does at `:294-295`.
  3. In `convert`'s catch (`:255-268`), handle `code === "cancelled"` like this: clear `input` and `output`, set no error, set `phase` to `"idle"`, and set the status to "Conversion cancelled. Files were removed from this device." The URL preselection (`preselected`) is kept. Because the app holds no `File` reference, the user picks the file again (Q4).
  4. Add one sentence about Cancel to the conversion step of `docs/web.md`.
- **Proof tests:**
  - `client.test.ts`: with a fake spawn, a pending `convert` rejects with `{ code: "cancelled", fatal: false }` after `cancel()`, the worker is terminated, and the next request spawns a new worker whose first messages are `init` and then `retire` with the old session id.
  - `converter-app.test.tsx`: with a client whose `convert` never settles, clicking Cancel returns to the drop zone with no error alert, shows the cancelled status, and issues `reset`.
- **Risk:** low. A result that arrives just before termination is ignored, because pending requests are cleared. A folder still locked by the terminated worker is reported as `held` and swept at the next start (`file-store.ts:290`).

### WEB-3: the web build lacks third-party notices (medium, M, W7)

- **Files:** `apps/web/src/app/licenses/page.tsx`; a new `apps/web/scripts/sync-notices.mjs`; `apps/web/package.json`; `scripts/generate-desktop-licenses.mjs`, renamed to `scripts/generate-js-licenses.mjs`; `justfile`; `.gitignore`; `about.toml`; `THIRD_PARTY_LICENSES`; a new `apps/web/THIRD_PARTY_JS_LICENSES`; `apps/web/test/public-pages.test.tsx`; `apps/web/e2e/site.spec.ts`; `docs/releasing.md`. If `.txt` files need headers or precache exclusions, also `apps/web/scripts/write-service-worker.mjs` and `apps/web/scripts/write-static-headers.mjs`.
- **Behaviour change:**
  1. **Generator.** Make the generator take the app directory and header as arguments. The default stays `apps/desktop`, and `just desktop-licenses` output must stay byte-identical. Add `just web-licenses`, which writes `apps/web/THIRD_PARTY_JS_LICENSES`. Call it from `just licenses` (`justfile:152-154`) and compare it in `licenses-check` (`:156-176`).
  2. **Shipped assets the generator may miss.** Check that the inventory covers what the site actually ships, in particular the Plus Jakarta Sans font, which is a web devDependency, and the WASI shim.
  3. **Wasm-only crates.** Add `wasm32-unknown-unknown` to `about.toml` `targets` so crates used only on wasm are inventoried, then regenerate `THIRD_PARTY_LICENSES` with the exact cargo-about version that `just licenses` pins.
  4. **Serving.** `sync-notices.mjs` copies `NOTICE`, `THIRD_PARTY_LICENSES` and `apps/web/THIRD_PARTY_JS_LICENSES` into `apps/web/public/notices/*.txt`. It runs in the `dev`, `build` and `export` scripts. Add `apps/web/public/notices/` to `.gitignore` next to `public/wasm/` (`.gitignore:38`). The folder is `notices/` rather than `licenses/` so it does not collide with the `/licenses` route.
  5. **Page.** On `/licenses`, replace the GitHub link (`:54`) with `/notices/NOTICE.txt`, and add links to `/notices/THIRD_PARTY_LICENSES.txt`, `/notices/THIRD_PARTY_JS_LICENSES.txt` and `/wasm/pandoc/SOURCE.txt`. Add an MPL-2.0 paragraph that names the unmodified MPL-2.0 crates compiled into `ariad_wasm_bg.wasm` (`cssparser`, `dtoa-short`, per `deny.toml:48-54`; confirm the list with `cargo tree -p ariad-wasm --target wasm32-unknown-unknown -e normal`) and links each one's crates.io source for the shipped version. Point the WASI shim entry (`:58-61`) at its notice text.
  6. **Docs.** Extend `docs/releasing.md:65` so that regenerating notices also covers the web JS notices.
- **Proof tests:**
  - `public-pages.test.tsx` asserts the new links and that no `github.com/.../blob/dev/NOTICE` link remains.
  - `site.spec.ts` fetches every `/notices/*.txt` and `/wasm/pandoc/SOURCE.txt` link from the built site and expects 200 with `text/plain`, under both `next start` and the export build.
  - `just licenses-check` fails when `apps/web/THIRD_PARTY_JS_LICENSES` is stale.
- **Risk:** low. Adding the wasm target may pull in new crates that need cargo-about clarifications, and the new `licenses-check` step must stay deterministic.

### HOST-4: one-shot reap leaves descendants and blocks on stderr; the LibreOffice probe kills only its child (low, M, W5)

- **Files:** `crates/ariad-host/src/runner.rs`, `crates/ariad-host/src/engines/libreoffice.rs`, `crates/ariad-host/src/bin/engine-probe.rs` (new test mode), `crates/ariad-host/tests/runner.rs`.
- **Behaviour change:**
  1. In `run_one_shot`, replace `process.reap(result.is_err())` (`:334`) with `process.reap(true)`, so the process group is stopped after every one-shot run. This matches the Windows job kill at `:736-737`. The launcher-failure check at `:337-348` keeps working, because one-shot `run_job` only returns after it has observed the exit status (`:664`, `:731-733`).
  2. In `reap` (`:785-808`), await the stderr task with `time::timeout(STDERR_DRAIN_GRACE, …)`, a new constant of about 2 s. On expiry, `abort()` the task and record `Ok(())`. The tail collected so far stays available. This bound also covers `close()` and `Session::kill`.
  3. Implementation check: confirm that process-wrap's `ProcessGroup` `start_kill` signals the stored pgid after the leader has been reaped, and that it ignores `ESRCH`. The existing kill at `:707-711` already relies on this.
  4. In `libreoffice.rs`, on Unix give the probe command `std::os::unix::process::CommandExt::process_group(0)`. After `try_wait` reports an exit, and on timeout, call `nix::sys::signal::killpg(pgid, SIGKILL)` (nix with `signal` is already a dependency) and ignore `ESRCH`. Replace `reader.join()` (`:221`) with a channel `recv_timeout` bounded by the remaining deadline. Windows behaviour is unchanged.
- **Proof tests:**
  - `tests/runner.rs`: add an engine-probe mode that spawns a heartbeat child holding stderr (stdout set to null), prints an ok `result`, closes stdout and exits 0. `run_command` returns `Ok` within 5 s and the child's heartbeat stops; reuse `wait_for_heartbeat` and `assert_heartbeat_stopped` (`:358-375`).
  - Unix-only variant: the child calls `setsid()` to escape the group. `run_command` still returns within the grace period, which proves the stderr bound.
  - `libreoffice.rs` unit test (`#[cfg(unix)]`): a fake `soffice` script prints `LibreOffice 24.2.0.3`, backgrounds `sleep 60` holding stdout, and exits. The version probe returns the version within `VERSION_TIMEOUT` and the `sleep` process is gone.
- **Risk:** low to medium. An engine that intentionally leaves a background helper after a one-shot run would now lose it. No shipped engine is known to do this, and the Windows path already behaves this way.

### HOST-5: session launcher failures are not classified (low, S, W5)

- **Files:** `crates/ariad-host/src/runner.rs`, `crates/ariad-host/src/session.rs`, `crates/ariad-host/tests/session.rs` (plus `tests/sandbox.rs` if a Linux integration test is feasible).
- **Behaviour change:**
  1. Extract the match arm at `runner.rs:337-348` into `pub(crate) fn classify_launch_failure(error: RunError, sandbox: &SandboxOutcome, status: Option<ExitStatus>) -> RunError` and use it in `run_one_shot`. Add `pub(crate) fn exit_status(&self) -> Option<ExitStatus>` to `EngineProcess`.
  2. In `Session::run` (`session.rs:382-396`), on `Err(RunError::EngineFailed { code: ErrorCode::EngineFailure, message })` whose message starts with `sandbox::LAUNCH_FAILURE_PREFIX`:
     - call `engine.close(CLOSE_GRACE).await`, which captures the exit status if the launcher exits within the grace period and then reaps the tree;
     - set `*process = None`;
     - return `classify_launch_failure(error, sandbox, status)`.
  3. `SandboxLaunchFailed` already falls into the non-reusable `Err(_)` arm of `Lease::run` (`:513-521`), and `map_engine_error` already maps it to `sandbox_unavailable`, exit 5 (`convert.rs:1797-1799`).
- **Proof tests:**
  - Unit-test `classify_launch_failure` for four cases: prefix, exit 127 and `Applied` give `SandboxLaunchFailed`; exit 1 does not; `SandboxOutcome::Off` does not; a message without the prefix does not.
  - Session test: an engine-probe session mode that emits a failing `result` carrying the prefix and exits 127. With the sandbox off, the error stays `EngineFailed`, and the next job starts a fresh process (new pid). A Linux-only integration test that yields `SandboxLaunchFailed` through a real launcher, if one can be forced in CI.
- **Risk:** low. A session engine that forges the prefix and keeps running is killed and restarted, and is not misclassified.

## 4. Worker tasks (disjoint file ownership)

All seven tasks are independent of one another; merge W1 first because it carries the high finding. Every worker lands on `dev` through a PR (`docs/git-workflow.md`), stages explicit paths, and writes conventional commits with no AI or tool references and no spec IDs. Run `just pandoc` before conversion work and `just ci` before handing back.

| Worker | Specs | Owns (only these files) | Narrow gate, then broaden | Size |
|---|---|---|---|---|
| W1 MCP | MCP-1, MCP-2 | `crates/ariad-cli/src/mcp/tools.rs`, `crates/ariad-cli/src/mcp/mod.rs`, `crates/ariad-cli/tests/mcp_stdio.rs`, `docs/mcp.md` | `cargo test -p ariad-cli --test mcp_stdio` | M |
| W2 Server | SRV-1 | `crates/ariad-server/src/{storage.rs,config.rs,app.rs,api/blobs.rs,api/uploads.rs,api/jobs.rs}`, `crates/ariad-server/tests/blob-transfers.rs`, `docs/server.md`, and the OpenAPI document only if `blobs.rs` annotations change | `cargo test -p ariad-server --lib storage`, then `just test-server` (Docker) | M |
| W3 Docling licences | HOST-1 now; HOST-3 after Q1 and Q2 | `engines/docling/pack/{licenses.py,build.py,vendored-libraries.toml}`, new files under `engines/docling/pack/licenses/`, `engines/docling/tests/test_licenses.py`, `ARCHITECTURE.md` (only if Q1 changes policy) | `uv run pytest engines/docling/tests/test_licenses.py`; HOST-3 also needs one Linux x86_64 pack build | S, then M |
| W4 Package stamping | HOST-2 | `crates/ariad-core/src/package_meta.rs`, `crates/ariad-host/src/package_meta.rs` | `cargo test -p ariad-core package_meta`, `cargo test -p ariad-host package_meta` | S |
| W5 Host process lifecycle | HOST-4, HOST-5 | `crates/ariad-host/src/{runner.rs,session.rs,engines/libreoffice.rs,bin/engine-probe.rs}`, `crates/ariad-host/tests/{runner.rs,session.rs}` (and `tests/sandbox.rs` if used) | `cargo test -p ariad-host --test runner --test session`, then `cargo test -p ariad-host` | M |
| W6 Web converter runtime | WEB-1, WEB-2 | `apps/web/src/converter/{engines.ts,client.ts}`, `apps/web/src/components/converter/converter-app.tsx`, `apps/web/test/{engines.test.ts,session.test.ts,client.test.ts,converter-app.test.tsx}`, `apps/web/e2e/heavy-routes.spec.ts` (optional), `docs/web.md` | `pnpm --filter @ariadshift/web test`, then `pnpm --filter @ariadshift/web e2e` | M |
| W7 Web notices | WEB-3 | `apps/web/src/app/licenses/page.tsx`, `apps/web/scripts/{sync-notices.mjs,write-service-worker.mjs,write-static-headers.mjs}`, `apps/web/package.json`, `scripts/generate-desktop-licenses.mjs` (renamed to `scripts/generate-js-licenses.mjs`), `justfile`, `.gitignore`, `about.toml`, `THIRD_PARTY_LICENSES`, `apps/web/THIRD_PARTY_JS_LICENSES`, `apps/web/test/public-pages.test.tsx`, `apps/web/e2e/site.spec.ts`, `docs/releasing.md` | `just licenses && just licenses-check`, `pnpm --filter @ariadshift/web test`, then the e2e site spec | M |

No file appears under two workers. W6 owns `docs/web.md`. W7 deliberately leaves `docs/web.md:70` alone: it already says that "`/licenses` and `NOTICE` document the licence and corresponding source", and that stays true after W7. On a sequential runner (the ASUS manager), W4 can be bundled into W5 because they share a crate but no files.

## 5. Contract and architecture notes

- **MCP-1** changes MCP staging error codes (`input_io` exit 2 becomes exit 1, and `limit_exceeded` exit 4 becomes possible). The change restores the value-for-value parity promised at `ARCHITECTURE.md:220`; call it out in the PR.
- **SRV-1** adds four `ARIAD_BLOB_*` variables and a token claim, which is public configuration. `docs/server.md` must change in the same PR, and OpenAPI must change too if the 429 descriptions are edited.
- **HOST-3**: if Q1 allows GCC-exception or LGPL libraries in the pack, AGENTS.md requires `ARCHITECTURE.md:516` and its Decision Log to change in the same PR. AGENTS.md itself already allows them, since it bans only AGPL and non-OSI code.
- **WEB-1** keeps limits plumbing in `apps/web` and moves no conversion logic into the surface, so it does not conflict with AGENTS "Architecture changes".
- **WEB-3** closes a licence-notice obligation gap for the website distribution. Nothing in AGENTS.md is breached today.
- **Doc/code mismatch, outside these specs and unverified:** `docs/mcp.md:103` says roots are queried on every tool call under protocol `2026-07-28`, but the code calls `list_roots()` only from `on_initialized` and `on_roots_list_changed` (`mod.rs:130-136`). W1 should check rmcp's stateless behaviour and correct the doc while editing `docs/mcp.md`.

## 6. Test gaps that matter

The specs above close these:

- No MCP parity test for inputs over 32 MiB.
- No test where `roots/list` fails after `list_changed`.
- No test that one subject cannot hold every blob slot, through PUT or GET.
- No licence-gate cases with conflicting fields or classifiers.
- No vendored native-file inventory test.
- No stamping test with a media entry over 64 MiB.
- No two-group browser route test that checks the limits passed to each group.
- No cancel test for the worker.
- No check that the web notices are current or that their links resolve.
- No runner test with a descendant holding stderr after success.
- No session launcher-failure classification test.

Left open: a behavioural LibreOffice link-update test (refuted M3), which would prove that an ODT linking a sibling file does not embed it after `--convert-to`.

## 7. Unresolved questions

1. **HOST-3 policy.** May the Docling pack ship GPL-3.0-with-GCC-exception (`libgomp`) or LGPL (`libquadmath`) libraries with notices, or must the build avoid them (for example by using LLVM OpenMP on Linux)? This blocks HOST-3.
2. **HOST-3 evidence.** Does a built Linux x86_64 pack contain `torch.libs/libgomp*`, and which vendored libraries do the locked numpy and scipy wheels carry? The inventory needs one built pack.
3. **SRV-1.** Several sub-questions:
   - Is `file://` storage with anonymous access a supported production setup?
   - Should the per-subject default be 1 or 2, given that the SDK's blob `download`, `downloadStream` and `transfer` do not retry on 429?
   - Should PUT replay inside the URL window stay allowed? If not, add the upload-row `pending` check.
4. **WEB-2.** After a cancel, should the page return to the drop zone (the spec's default), or keep the `File` so a retry can re-ingest it automatically?
5. **MCP-2.** Is a 10 s `roots/list` timeout acceptable, and does rmcp 3.5.1 already time out server-to-client requests?
6. **MCP-1.** Is a full temporary copy acceptable for `inspect` and `plan` of multi-GB inputs, or should they stage race-free by another route?
