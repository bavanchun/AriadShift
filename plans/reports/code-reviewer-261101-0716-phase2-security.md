# Phase 2 tool framework: security review

- Branch: `feat/tool-framework` (worktree `01-AriadShift-wt-p02-tools`), diff `fce3077...HEAD` (19 commits, ~18.4K LOC).
- Scope: the five focus areas in the brief only. Read-only review. No builds or tests were run, as the brief required.
- Method: read the code and traced call paths. The lopdf 0.46.0 behaviour was checked against the crate source in `~/.cargo/registry`.

## Findings, ranked

### High

**H1. pdf.info decompresses attacker streams with no limit, inside the host process.**
`crates/ariad-pdf/src/info.rs:221` (page `/Contents`), `:357` (form XObjects) and `:633` (XMP `/Metadata`) call `Stream::decompressed_content()`. In lopdf 0.46 that call is `decode_filters(None)`, which has no size limit. `LoadOptions.max_decompressed_size` covers only object streams at load time, and the `Budget` that `reader::pdf::read` uses is not applied here.
- Failure: a PDF of a few MB holds one FlateDecode content stream that inflates about 1000:1. A `/Contents` array can reference that stream N times, and the page bytes are concatenated into one `Vec` (`info.rs:218-227`). `Content::decode` then parses the whole buffer before `.take(MAX_INSPECT_OPERATORS)` applies.
- Impact: the host process exhausts memory (OOM). This hits the CLI, the long-running MCP server and the browser build (`ariad-wasm` uses the same code). In-process tools get no sandbox, no `timeout_s`, no memory watchdog and no cancellation checks after dispatch (`inprocess.rs:50`).
- Fix: use `decompressed_content_with_limit(remaining_budget)` with a per-document budget derived from `limits.max_decompressed_bytes` and the input size, the way `reader::pdf` does. Cap the accumulated `contents` size, and deduplicate repeated content-stream IDs.

**H2. The raw trailer scan is quadratic and runs before any limit is checked.**
`crates/ariad-pdf/src/document.rs:196-216` is called from `open_for_info` at `:133-134`, before `load_bounded`. For every `trailer` occurrence it scans forward to `startxref`, or to the end of the file when `startxref` is missing. That is O(occurrences × size), and it runs twice (once for `/Encrypt`, once for `/Prev`).
- Failure: a 20 MB file of repeated `trailer\n` keeps a host thread busy for hours. Local limits set `max_input_bytes = None`, and nothing interrupts the scan.
- Fix: scan once, linearly. Search for `startxref` only within a bounded window (for example 4 KiB) after each `trailer`, or resume the next search after the previous match. Also bound the total work.

**H3. Rolling back an overwrite deletes the user's existing files.**
`crates/ariad-host/src/tools/publish.rs:426-468` together with `workspace.rs:227-229`. When `overwrite == true`, `promote_into` *renames* each artifact over the existing destination. When artifact k fails, `rollback_batch` removes every name in `created_in_batch`. Those names include files that existed before the run.
- Failure: the user runs `ashift tool pdf.split in.pdf -o out/ --overwrite` again into a folder that holds earlier parts (or the user's own files with the same names), and part 3 fails (ENOSPC, EIO, or a symlink or directory at that name). Parts 1–2 are then neither the old nor the new version: they are gone. The same happens in `run_recipe` (`tools/recipe.rs:515`), where every step also uses `.with_overwrite(true)` internally. This breaks the docs/tools.md §7 promise of all-or-nothing publication.
- Fix: when overwrite is set, first rename each existing destination to a sibling backup inside the same `Dir`, promote the whole batch, then delete the backups. On failure, rename the backups back. Alternatively, stage the whole batch under temporary names and commit it with renames only after every artifact has been written.

**H4. The host reads an engine-named artifact through symlinks and FIFOs before validating it. Latent: no JSON-output pack tool is registered yet.**
`crates/ariad-host/src/tools/engine.rs:129-137` calls `fs::read(&artifacts[0].0)` on a path the engine reported. The runner checks that path only lexically (`runner.rs:1420`). The nofollow, regular-file and inside-`out/` checks come later, in `publish_atomic`. The size accounting at `:112-118` uses `symlink_metadata`, which measures the link, not its target.
- Failure: a compromised sandboxed engine creates `out/r.json` as a symlink to `/dev/zero` (host OOM) or as a FIFO (the host blocks forever, after the engine has exited and so outside the timeout). It can also point the symlink at a host JSON file (cloud credentials, an MCP config). With `OutputTarget::None` (public API, `run.rs:852`), publication is skipped, so the parsed content is returned to the caller in `inline_json`.
- Fix: read through `workspace.open_artifact()` (O_NOFOLLOW plus an `is_file` check) with `take(MAX_INLINE_PARAMS_BYTES + 1)`. Do this only after the same validation `publish_atomic` performs, and also when the output target is `None`.

### Medium

**M1. Resolve-time pack verification does not detect deliberate tampering.**
`crates/ariad-host/src/packs/install.rs:880-925`.
- The per-file SHA-256 and size list comes from `receipt.toml`. That file sits inside the pack root and is not signed: the minisign signature covers only the manifest, whose single archive digest cannot be checked after extraction. Someone who can modify a pack binary can rewrite the receipt to match. Because `files` is `#[serde(default)]` (`:158`), deleting the list makes zero files get checked, and the entrypoint is not required to appear in it.
- Files that are not listed (for example a `.pth` or `sitecustomize.py` dropped into the Docling site-packages) are never detected.
- The process-wide cache (`:709-775`) keys on dev, inode, mtime and size only. In a long-running process (MCP server, desktop), rewriting a file in place at the same size and restoring its mtime with `utimensat` keeps the cache hit.
- The check-then-exec gap remains: files are hashed by path, then executed by path later.
- Fix: include `ctime` (sec and nsec) in `FileStamp`. Fail when the receipt has no `files`, or does not cover the entrypoint. Reject unlisted regular files under the root. Either authenticate the receipt (for example an HMAC keyed per install outside the pack dir), or state plainly in docs/tools.md that this check detects corruption only.

**M2. run_doc_tool reopens confined paths by name, undoing convert's protection against path swaps.**
- Inputs: `run.rs:735-752` calls `confine_input` but then throws away `confined.file` and reopens `confined.path` for `classify_file` (`:602`) and `fs::copy` (`:644`).
- Outputs: `publish.rs:310-341` canonicalizes a path, then `Dir::open_ambient_dir(canon_dir)` reopens it by name.
- MCP `convert` copies from the confined handle (`mcp/tools.rs:531`, `create_isolated_copy_cancellable_in`). ARCHITECTURE.md §MCP claims capability handles protect against race-condition swap attacks; `run_doc_tool` does not get that protection.
- Failure: a concurrent writer inside an allowed root swaps the input or an output directory component for a symlink between the check and the use. The tool then reads from, or publishes into, a location outside the allowed roots.
- Fix: stage from `confined.file`, and open the output directory relative to the allowed root's `Dir` handle, as `confine_output` does.

**M3. A literal secret placed inside `$secret` is echoed back unscrubbed.**
`crates/ariad-core/src/tools/secret.rs:118-120`. The error text `unsupported "$secret" target {secret:?}` contains the caller's value. A user who writes `{"password":{"$secret":"hunter2"}}` (an easy mistake) gets the password copied into the error. That error goes to stderr, MCP error content, recipe-validation errors and logs. The scrubber cannot redact it because the value was never put in a `SecretBag`.
- Related gaps:
  - Argv and recipe validation match names exactly and check only top-level keys (`validate_secret_fields`). So `--param Password=…` or `--param opts='{"password":"…"}'` passes the literal check; only the MCP layer matches without regard to case.
  - `ResolvedSecrets` derives `Debug` (`tools/secrets.rs:200`), and its `schema_params`/`engine_params` hold plaintext secrets.
- Fix:
  - Never interpolate the tag value; say "expected 'prompt' or a sec_ handle".
  - Match secret names without regard to case, recursively, on every channel.
  - Write a redacting `Debug` impl for `ResolvedSecrets`.

**M4. Recipes give every step's engine every secret.**
`tools/secrets.rs:171-181` copies every non-handle `SecretBag` entry into `engine_params`. `tools/recipe.rs:554` passes the full bag to every step.
- Failure: a password meant for a `pdf.encrypt` step reaches the `ai`, `tesseract` or `inpaint` packs in earlier steps through their `params.json`. That breaks least privilege toward third-party engines.
- The CLI `recipe run` has no `--secret` flag today, so this affects library and desktop callers.
- Fix: put only the keys a step references (`spec.secret_params` or a `$secret` ref) into `engine_params`, and give each step its own filtered bag.

**M5. The runner writes the 0600 params file at any caller-supplied path and fails open.**
`crates/ariad-host/src/runner.rs:571-627`. `prepare_job_request` writes to `params_file` as given, with create, truncate and symlink-following. `ParamsFileGuard` then deletes that path. Nothing checks that it lies under `work_dir`.
- Failure: a public `run_command_v2`/`run_v2` caller passes a path outside `work_dir`. The host overwrites or chmods that file and then unlinks it; the engine's sandbox could not read it anyway.
- When `work_dir` is not a directory (`:589`), the function silently sends secret-bearing params inline.
- The host-internal flow through `tools/engine.rs` is safe today.
- Fix: require `params_file == work_dir/params.json` (or a single-component path under it). Open it with `O_NOFOLLOW|O_EXCL`. Return an error instead of falling back to inline secrets.

**M6. Duplicate target names within one batch are not rejected.**
`validate_naming_template` (`core/tools/recipe.rs:211`) does not require `{n}` when a step produces more than one artifact. `publish_atomic` (`publish.rs:366-421`) does not detect duplicate `target_name`s. Recipes with no template use the per-item filename, so `a/doc.pdf` and `b/doc.pdf` collide.
- Failure: with `--overwrite`, artifact 2 silently replaces artifact 1. `ToolOutcome.artifacts` then lists the same path twice, so one output is lost without any error.
- Fix: reject duplicates during pre-validation, or require `{n}` whenever N > 1.

**M7. The engine chooses the output extension, and it is never checked against `spec.output_format`.**
In `run.rs:911-927` and `publish.rs:127-161`, the default artifact name uses the `format` string the engine reported. docs/tools.md §7 says the host validates artifact extensions, but nothing compares the reported format with `spec.output_format`. MCP `run_doc_tool -o <file>` also skips the extension check that MCP `convert` enforces (`confine.rs:917-923`).
- Failure: a compromised engine names its output `x.rotated.command` or `x.rotated.html` in the user's folder. An MCP client can write tool output to any file name inside an allowed root.
- Fix: require `format == spec.output_format` (or an allow-list per tool) and validate the extension of an explicit `-o`.

**M8. xref repair reaches the PDF mutation path.**
`crates/ariad-pdf/src/document.rs:116-117`: `open()`, the entry point for structural tools, calls `open_for_info` → `load_bounded` with `repair_xref: true`. That contradicts commit `4e93ff3` ("repair … only for inspection") and ARCHITECTURE.md:824 (xref repair runs out of process). Non-strict mode also drops the strict-mode propagation of object-stream limit errors noted at `reader/pdf/mod.rs:344`.
- The converter path is clean: `read()` uses `repair_xref: false`.
- Fix: give `open()` its own strict load and keep repair only in `open_for_info`.

**M9. The hand-written params schema validator does not enforce the full schema.**
`run.rs:952-1100` ignores `allOf`, `pattern`, `patternProperties`, `propertyNames`, `uniqueItems` and `multipleOf`. It also ignores sibling keywords next to `$ref`, because it returns early at `:966`. Out-of-process engines receive these params, from MCP and from recipes, without the advertised `invalid_params` gate.
- Fix: use a maintained JSON Schema validator (Draft 2020-12), or restrict the schemas the tools may use to the subset this code enforces and test that restriction.

### Latent, note only
- `PageRange::expand` (`page_range.rs:255`) has no output cap. `"all,all,…"` with `KeepInOrder` grows as tokens × pages. Cap it at a multiple of `page_count` before any tool uses it.
- `PdfDoc::inner()` returns `&Document`, and `Document: Clone`, so the "never save decrypted" guard can be bypassed with `doc.inner().clone().save_to(..)`. `open()` refuses encrypted input outright, so no current code path is exposed.

## Checked and found sound
- `run_tool` gets the same limits as convert. `JobSpec::for_job_request` takes the timeout and memory limit from `RunTool.limits`, TMPDIR is set to `work_dir`, and both use `Limits::local()`. Writable roots for `run_tool` narrow to `[out, work_dir]`, with `in/` read-only. `sandbox_required` packs refuse to degrade under best-effort mode. Unknown pack engines fail closed.
- The params spill happens before the sandbox launches. The file is 0600 inside `work_dir`, the request on the wire carries `params: {}`, and the drop guard plus `engine.rs:105` delete it on success, error, cancellation and panic. Secrets never go on argv.
- Converter paths never use repair mode. The page-range parser has no panic or overflow paths, because every integer goes through `parse::<u32>` and `checked_add`.
- Imported recipes fail closed without `--yes` when stdin is not a terminal. Naming templates cannot contain separators or `..`, and hidden names are rejected under confinement. The CLI rejects inline `--secret`. MCP rejects every secret key (without regard to case) and every `$secret` reference.
- Warnings, progress, metrics, `inline_json` and errors are scrubbed with the resolved-secret scrubber.

Status: DONE_WITH_CONCERNS
