# Writer bounds result

## Dev merge validation

DONE. Fetched `origin/dev` at `47afeb445fee2692724e392f3ae1aad5fc7cd414` and merged it into `fix/writer-bounds` without conflicts or manual resolutions. Validated merge HEAD: **`a725f489f4244a38a244cc80c9801cfdc48485fc`**. Its parents are `b351caf8b7013af2ec1d2b21eb4e52faf03749b8` and the fetched dev SHA. All writer code, Markdown reader code, writer bounds tests and goldens remain identical to the pre-merge task head. Both imported CI files match fetched dev exactly.

```sh
git fetch origin
git merge --no-ff origin/dev -m 'chore: merge dev into writer bounds'
cargo fmt --all --check
cargo clippy -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --all-targets --features ariad-host/test-probe -- -D warnings
cargo nextest run -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --features ariad-host/test-probe --no-fail-fast
just lint-commits
```

The exact merged head passed every named check. Result: `Summary [ 460.365s] 907 tests run: 907 passed (2 slow), 12 skipped` across 49 binaries. Per crate: ariad-core 424, ariad-host 304, ariad-cli 171, ariad-wasm 8, all passed. All 21 writer bounds cases, both required shared-image read-backs, the reader scaling regression and all six route golden tests passed. No additional assertion, filter or snapshot edits were made while validating this merge.

Gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t10-merge-dev-final-gate.log`. Exact terminal:

```text
=== 13:25:12 ALL GREEN head=a725f489f4244a38a244cc80c9801cfdc48485fc
```

The detached slot-1 job PID 3918522 ran nice 19, with Pandoc 3.12, `CARGO_BUILD_JOBS=4` for named checks and an isolated TMPDIR. Preparation used two jobs/two CPUs and finished in 19m 01s. After 60 quiet seconds, nextest started at load 11.76 with one test thread and all eight CPUs; sampled load was 3.21–20.17. No job was paused or cancelled. The TMPDIR was removed and tracked verification files were clean after the gate.

The CI scope defect is independently reproduced: with `EVENT_NAME=pull_request`, `PR_BASE_SHA`/`PR_HEAD_SHA` unset and `--classify-cloud crates/ariad-core/src/lib.rs`, the old script exits 1 (`PR_BASE_SHA: parameter null or not set`); the merged script returns `true`, exit 0. Receipt: `/home/vchun/Codes/ariadshift-logs/d2120-t10-merge-dev-scope.log`.

This report is recorded in a separate `docs: record writer bounds merge validation` commit after the tested merge. That commit changes only this report; its follow-up commit-lint receipt is `/home/vchun/Codes/ariadshift-logs/d2120-t10-merge-dev-report-lint.log`. The code tested above is unchanged by the report commit. Nothing was pushed, rebased or forced. GitHub CI remains the merge condition after the lead synchronizes the branch. No unresolved local question remains.

## Previous writer validation

Rework 6 is DONE. Both newly exposed compact-row goldens passed fresh real-CLI comparisons of the complete IR document and were accepted using cargo-insta. All six route golden tests passed before committing. The final four-crate gate then passed on the exact committed head: **885 passed, zero failed, 12 skipped**, plus fmt, Clippy and commit lint. No source, assertion, limit or doctor snapshot changed in this rework. Nothing was pushed.

- Branch: `fix/writer-bounds`.
- Rework 6 validated HEAD: `b351caf8b7013af2ec1d2b21eb4e52faf03749b8`.
- New local commit: `test(cli): update more route goldens for compact rows`.
- Parent: `a429dc73a50040377522842c0d6c761159f3325c`.
- At rework 6 completion the report was local, with an identical copy in `/home/vchun/Codes/ariadshift-ops/reports/d2120-task10-writer-bounds.md`.

## Rework 6 golden review and IR proof

The lead's rework 6 decision extends acceptance to every compact-row-only diff, provided each complete old/new Markdown body is read by the real CLI and yields equal IR. Any caption, text, image, ordering or escaping difference remains a stop condition. Both accepted files satisfy this rule. Each has the same line count and all changed lines retain an identical prefix, followed only by removed empty trailing cells. No other content changed. Cargo-insta drops the generated `assertion_line` metadata, leaving only the reviewed Markdown changes below.

For each file, the CLI executed both commands against the complete snapshot body, excluding only the Insta metadata. It wrote real IR JSON; both the complete document objects and their body fields were asserted equal. Temporary inputs under this worktree's `plans/reports/` were removed in `finally`. There is no fake converter or relaxed equality check.

```sh
ashift convert plans/reports/d2120-t10-rework6-ir-before.md --to json -o <private-temp>/before.json
ashift convert plans/reports/d2120-t10-rework6-ir-after.md --to json -o <private-temp>/after.json
cargo insta accept --manifest-path /tmp/d2120-t10-insta-review/Cargo.toml --snapshot synthetic-merged-table.md.snap
cargo insta accept --manifest-path /tmp/d2120-t10-insta-review/Cargo.toml --snapshot vn-tt-28-2012-conformity.md.snap
```

The same external-fixture discovery workaround documented below was reused: byte-identical original and pending snapshots in a metadata-only scratch package, explicit cargo-insta acceptance, then validation of the exact accepted bytes before copying back. No repository manifest changed. Evidence: `/tmp/d2120-t10-rework6-accepted-goldens.json`.

- `synthetic-merged-table`: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework6-review-complete.log`, ending `=== 12:32:22 ALL GREEN review head=a429dc73a50040377522842c0d6c761159f3325c`.
- `vn-tt-28-2012-conformity`: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework6-review-conformity.log`, ending `=== 12:37:16 ALL GREEN review head=a429dc73a50040377522842c0d6c761159f3325c`.

### synthetic-merged-table.md.snap

Complete old/new IR document equality: **PASS**. Reviewed permanent diff:

```diff
--- fixtures/golden/synthetic-merged-table.md.snap
+++ fixtures/golden/synthetic-merged-table.md.snap
@@ -6,4 +6,4 @@
 | **Route / Tuyến** |  |  |  |
 | --- | --- | --- | --- |
 | Riverbank | Bến sông |  | 8 min |
-| Nested record |  |  |  |
+| Nested record |  |  |
```

### vn-tt-28-2012-conformity.md.snap

Complete old/new IR document equality: **PASS**. Reviewed permanent diff:

```diff
--- fixtures/golden/vn-tt-28-2012-conformity.md.snap
+++ fixtures/golden/vn-tt-28-2012-conformity.md.snap
@@ -844,8 +844,8 @@
 
 |  |  |  |  |  |  |  |  |
 | :---: | --- | --- | --- | --- | --- | --- | --- |
-| **Các quá trình sản xuất cụ thể** | **Kế hoạch kiểm soát chất lượng** |  |  |  |  |  |  |
-| **Các chỉ tiêu giám sát/kiểm soát** | **Tiêu chuẩn/quy chuẩn kỹ thuật** | **Tần suất lấy mẫu/cỡ mẫu** | **Thiết bị thử nghiệm/kiểm tra** | **Phương pháp thử/kiểm tra** | **Biểu ghi chép** | **Ghi chú** |  |
+| **Các quá trình sản xuất cụ thể** | **Kế hoạch kiểm soát chất lượng** |
+| **Các chỉ tiêu giám sát/kiểm soát** | **Tiêu chuẩn/quy chuẩn kỹ thuật** | **Tần suất lấy mẫu/cỡ mẫu** | **Thiết bị thử nghiệm/kiểm tra** | **Phương pháp thử/kiểm tra** | **Biểu ghi chép** | **Ghi chú** |
 | **(1)** | **(2)** | **(3)** | **(4)** | **(5)** | **(6)** | **(7)** | **(8)** |
 |  |  |  |  |  |  |  |  |
 
@@ -901,8 +901,8 @@
 
 |  |  |  |  |  |  |  |  |
 | :---: | --- | --- | --- | --- | --- | --- | --- |
-| **STT** | **Số tiếp nhận** | **Tên tổ chức, cá nhân công bố** | **Tên sản phẩm, hàng hóa, dịch vụ, quá trình, môi trường** | **Tiêu chuẩn/quy chuẩn** | **Loại hình đánh giá** | **Ghi chú** |  |
-| **Bên thứ nhất (tên tổ chức chứng nhận đã đăng ký/được chỉ định)** | **Bên thứ ba (tự đánh giá)** |  |  |  |  |  |  |
+| **STT** | **Số tiếp nhận** | **Tên tổ chức, cá nhân công bố** | **Tên sản phẩm, hàng hóa, dịch vụ, quá trình, môi trường** | **Tiêu chuẩn/quy chuẩn** | **Loại hình đánh giá** | **Ghi chú** |
+| **Bên thứ nhất (tên tổ chức chứng nhận đã đăng ký/được chỉ định)** | **Bên thứ ba (tự đánh giá)** |  |
 | 1 |  |  |  |  |  |  |  |
 | 2 |  |  |  |  |  |  |  |
 | .... |  |  |  |  |  |  |  |
```

## Rework 6 route loop

The actual compiled `ariad-cli::route_golden` test binary from nextest's metadata ran under slot 1, nice 19 and a fresh private TMPDIR, with `--nocapture --test-threads=1`. No test filter or automatic snapshot update was enabled.

1. `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework6-routes-1.log`: before acceptance, five passed and DOCX failed only on `synthetic-merged-table`; terminal `STOPPED: route-goldens rc=101`.
2. `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework6-routes-2.log`: after the first acceptance, five passed and DOCX reached `vn-tt-28-2012-conformity`, exposing exactly the four reviewed compact-row changes; same expected terminal failure.
3. `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework6-routes-3.log`: after the second acceptance, **six passed, zero failed**, 15.90 s; terminal `=== 12:38:17 ALL GREEN routes precommit-parent=a429dc73a50040377522842c0d6c761159f3325c`.

The final loop traversed all eight DOCX-to-Markdown, four EPUB-to-Markdown and six HTML-to-Markdown fixtures, including the fixtures after the formerly failing DOCX snapshot. Markdown-to-HTML, Markdown-to-EPUB and both cross routes passed too. No further compact-row or other golden changes were found. The two new snapshots, five changed rows total, were then committed as `b351caf8b7013af2ec1d2b21eb4e52faf03749b8`. Existing writer tests and the two required image round-trip functions are unchanged; `writer_bounds.rs` SHA-256 remains `a88d8a7ea3dfbed6baf51f138025f9a31f86970a9bd9985812fdba8bdba7cf00`.

## Rework 6 final four-crate gate

**ALL GREEN**. Completed detached slot-1 job PID 3817832, nice 19; exact head `b351caf8b7013af2ec1d2b21eb4e52faf03749b8`. Log: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework6-final-gate-verified-head.log`. Driver: `/tmp/d2120-t10-rework6-final.py`; launcher: `/tmp/d2120-t10-rework6-final.zsh`. The current verification worktree is reused and checked out to the local committed head; there is no push, source overlay, or reset to a different tested revision.

Setup-only log `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework6-final-gate.log` ended `STOPPED: final gate head=b351caf8b7013af2ec1d2b21eb4e52faf03749b8 rc=1`: Git refused switching from the parent with the two accepted snapshots still dirty. No gate check or nextest run started. Both dirty paths were verified byte-for-byte against the new commit, all other tracked and staged paths were clean, and only then the owned verification worktree was force-checked out to that exact commit. The actual single final gate uses the new log above.

Final result: `Summary [ 347.784s] 885 tests run: 885 passed, 12 skipped`. Fmt, four-crate Clippy and `just lint-commits` all returned zero. The terminal is exactly:

```text
=== 12:47:29 ALL GREEN head=b351caf8b7013af2ec1d2b21eb4e52faf03749b8
```

| Crate | Passed | Failed |
|---|---:|---:|
| `ariad-core` | 402 | 0 |
| `ariad-host` | 304 | 0 |
| `ariad-cli` | 171 | 0 |
| `ariad-wasm` | 8 | 0 |

All 21 writer-bound tests passed. Both required read-back tests remain byte-for-byte unchanged and passed: `repeated_asset_references_share_one_markdown_data_uri_and_read_back` in 0.014 s and `shared_markdown_images_keep_their_common_title` in 0.012 s. The reader scaling regression `large_shared_images_keep_reader_work_linear_in_reference_count` passed in 2.695 s. The former exit-5 conversion case passed in 0.225 s; doctor human/JSON passed in 0.293/0.311 s without snapshot changes. The unchanged six-megabyte HTML case passed in 10.449 s. There was exactly one full nextest execution on this final head; no failed test was retried or filtered out.

The actual test phase started at load 7.08 after the quiet window, used all eight CPUs, and sampled load 4.12–15.70 across 70 samples; 16 were at least 12, and none exceeded 24. Runtime evidence: `/tmp/d2120-t10-rework6-final-load.jsonl`. Compilation used the cached worktree and took 0.77 s for Clippy and 0.57 s for test-binary preparation. The isolated TMPDIR was cleaned when the completed gate exited. Exact-head checks and the verification worktree's tracked diff were clean at both ends.

Named checks used `CARGO_BUILD_JOBS=4`; Clippy uses two CPUs. Cached test-binary preparation uses two jobs/two CPUs to avoid linker spikes. Actual tests use all eight CPUs and `NEXTEST_TEST_THREADS=1`, with a private TMPDIR for the whole gate. The driver waits for 60 consecutive seconds of load below 12 and more than 8 GiB available memory after preparation, records test load every five seconds, and runs nextest once without filters. Pandoc 3.12 is pinned on PATH and through `ASHIFT_PANDOC`; `just pandoc` runs first. No job is paused or cancelled for waiting.

```sh
cargo fmt --all --check
cargo clippy -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --all-targets --features ariad-host/test-probe -- -D warnings
cargo nextest run -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --features ariad-host/test-probe --no-fail-fast
just lint-commits
```

## Historical rework 5 outcome

Rework 5 ended BLOCKED. The compatibility fix and the two authorized golden updates are committed locally. The final four-crate gate finished with **880 passed, five failed, 12 skipped**. It uncovered a third compact-row golden outside the brief's explicit two-file acceptance list. That file remained unaccepted until rework 6 above. Three CLI failures passed in a clean-temp follow-up; the HTML timing failure occurred during high load. At that point there was no full-gate ALL GREEN log. Nothing was pushed and no PR operation was performed.

- Branch: `fix/writer-bounds`.
- Core commit: `29907062c4f051dc256354209bb1c3b010cf1e43` — `fix(core): preserve existing Markdown table output`.
- Original golden commit and rework 5 tested HEAD: `a429dc73a50040377522842c0d6c761159f3325c` — `test(cli): update route goldens for compact table rows`.
- Reviewed parent: `b19d60d70b3aaecf353156ca3f17af59d3827d4c`.
- Task worktree: `/home/vchun/Codes/01-AriadShift-wt-t10-writer`; verification worktree: `/home/vchun/Codes/01-AriadShift-wt-t10-writer-verify`.

## Compatibility and GFM proof

`git diff 6fc7e06..b19d60d -- crates/ariad-core/src/writer/markdown.rs` showed that the writer task added table-caption and table-note emission. Neither is required for a bound: `check_output_size` visits and charges the complete IR before normalization, independently of `write_block`; removing emission reduces output. The restored `Block::Table` dispatch was compared programmatically with base `6fc7e06` and is identical. Preflight, nesting, output-size and table-column/row bounds are unchanged.

The annotation test now requires the exact original Markdown output while retaining HTML caption/note preservation and the header-loss warning check. On unmodified writer `b19d60d`, this expectation failed with exit 101 because the output contained `Table caption` and `Table note`. With the restoration, **all 21 writer tests passed** in 36.04 s. No timing or size assertion was relaxed.

The new `compact_table_rows_read_back_with_empty_trailing_cells` test covers empty, one-cell, two-cell and complete three-cell rows with left/center/right alignments. It verifies compact output, reads compact and padded GFM, compares their complete IR bodies and checks every expected cell, including empty trailing cells. Both reads have no warnings. The two required image round-trip functions are byte-for-byte identical to `b19d60d`. Current `writer_bounds.rs` SHA-256: `a88d8a7ea3dfbed6baf51f138025f9a31f86970a9bd9985812fdba8bdba7cf00`.

```sh
cargo test -p ariad-core --test writer_bounds table_annotations_are_preserved_and_header_loss_warns -- --exact --nocapture
cargo test -p ariad-core --test writer_bounds -- --nocapture --test-threads=1
cargo nextest run -p ariad-cli --features ariad-host/test-probe --test route_golden --no-fail-fast
```

Proving log: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework5-focused-narrow.log`. The route run initially had four passes and the two expected DOCX/EPUB snapshot failures; the HTML golden passed unchanged. Terminal `=== 11:02:06 ALL GREEN` confirms the expected-failure proving assertions, not the final gate.

## Accepted golden diffs

Both old snapshots and both pending files were read in full. Their full text was checked to differ only by the compact footer row. Captions, document titles, image alt/title and other content are identical. The EPUB's single normalized inline PNG line retains its existing 3,568-character payload redaction.

Cargo-insta 1.49.0 was verified against the registry and official release; archive SHA-256 `18879f4e7280c2eb87bb1037ddb9d48af697511fb0d8113edfc39ab8bade4d4e`. Its [package-root discovery](https://github.com/mitsuhiko/insta/blob/1.49.0/cargo-insta/src/cargo.rs#L7) excludes the external fixture directory, so initial in-place commands found nothing and changed nothing. Identical old/new copies were accepted in a temporary review-only Cargo package, checked against the reviewed text, then copied back to exactly the two fixture files. The temporary package was never compiled; no repository manifest or dependency changed.

```sh
cargo insta accept --manifest-path /tmp/d2120-t10-insta-review/Cargo.toml --snapshot vi-route-data.md.snap
cargo insta accept --manifest-path /tmp/d2120-t10-insta-review/Cargo.toml --snapshot vi-epub-table-image.md.snap
```

Each command reported its selected file accepted. Cargo-insta dropped the transient `assertion_line`; these are the complete committed diffs:

### vi-epub-table-image.md.snap

```diff
--- fixtures/golden/vi-epub-table-image.md.snap
+++ fixtures/golden/vi-epub-table-image.md.snap
@@ -16,7 +16,7 @@
 | --- | --- | --- |
 | Bến sông | 8 | Bóng mát |
 | Vườn đọc | 14 | Có ghế dài |
-| Lịch khảo sát cập nhật mỗi tháng. |  |  |
+| Lịch khảo sát cập nhật mỗi tháng. |
 
 ![Sơ đồ tuyến đường ven sông](data:image/png;base64,<redacted: 3568 base64 chars>)
 
```

### vi-route-data.md.snap

```diff
--- fixtures/golden/vi-route-data.md.snap
+++ fixtures/golden/vi-route-data.md.snap
@@ -9,6 +9,6 @@
 | --- | --- | --- |
 | Bến sông | 8 | Bóng mát |
 | Vườn đọc | 14 | Có ghế dài |
-| Lịch khảo sát cập nhật mỗi tháng. |  |  |
+| Lịch khảo sát cập nhật mỗi tháng. |
 
 Các khoảng thời gian là ước lượng đi bộ.
```

## Historical golden acceptance blocker (resolved in rework 6)

The full DOCX loop passed the updated golden and reached `synthetic-merged-table.md`, exposing this third difference. It stops at the first snapshot mismatch, so later fixtures must be rechecked after any authorized acceptance. The generated metadata is included in the review diff:

```diff
--- fixtures/golden/synthetic-merged-table.md.snap
+++ fixtures/golden/synthetic-merged-table.md.snap.new
@@ -1,9 +1,10 @@
 ---
 source: crates/ariad-cli/tests/route_golden.rs
+assertion_line: 368
 ---
 # Sparse route record
 
 | **Route / Tuyến** |  |  |  |
 | --- | --- | --- | --- |
 | Riverbank | Bến sông |  | 8 min |
-| Nested record |  |  |  |
+| Nested record |  |  |
```

The real CLI read the complete old/new Markdown bodies using `ashift convert <input.md> --to json -o <output.json>`. Their complete IR bodies are equal: the missing fourth trailing cell is recovered as empty. Temporary inputs under the task's `plans/reports/` were removed after comparison. No snapshot was accepted in that follow-up.

The [rework 5 brief](/home/vchun/Codes/ariadshift-ops/briefs/d2120-task10-rework5.md:4) names the two snapshots above and requires acceptance “of only those snapshots.” The third file was left pending at the end of rework 5; rework 6 authorizes and accepts it above. Full evidence: `/tmp/d2120-t10-rework5-extra-golden.json`.

## Controlled timing reproduction

The six reader cases were five HTML tests and one Markdown nesting unit; the seventh case was MCP startup. Exactly those seven ran once on unchanged `b19d60d`, sequentially, nice 19, `CARGO_BUILD_JOBS=4`, using all eight CPUs after preparation. Start load was **11.52**, with all measurement samples **7.89–11.52**. All seven passed in **37.695 s** without code or assertion changes. Under the lead's decision, the original failures are recorded as load/resource sensitivity.

| Test | Passing wall time |
|---|---:|
| `ariad-cli::mcp_stdio::test_stdin_eof_with_hanging_convert_cleans_engine_and_workspace` | 12.072 s |
| `ariad-core::reader::markdown::tests::deep_inline_nesting_is_rejected_on_a_wasm_sized_stack` | 1.146 s |
| `ariad-core::html_reader::adoption_agency_reparenting_large_sibling_subtree_is_linear` | 2.251 s |
| `ariad-core::html_reader::foster_parenting_repeated_table_div_parses_in_bounded_time` | 4.356 s |
| `ariad-core::html_reader::hostile_reconstruction_3_6mb_input_fails_bounded_under_local_limits` | 7.678 s |
| `ariad-core::html_reader::reconstruction_amplification_fails_fast_with_node_limit` | 0.951 s |
| `ariad-core::html_reader::six_megabyte_table_heavy_page_parses_under_local_limits` | 9.240 s |

Exact command:

```sh
cargo nextest run -p ariad-core -p ariad-cli --features ariad-host/test-probe --no-fail-fast -E 'test(=adoption_agency_reparenting_large_sibling_subtree_is_linear) | test(=foster_parenting_repeated_table_div_parses_in_bounded_time) | test(=reconstruction_amplification_fails_fast_with_node_limit) | test(=hostile_reconstruction_3_6mb_input_fails_bounded_under_local_limits) | test(=six_megabyte_table_heavy_page_parses_under_local_limits) | test(=reader::markdown::tests::deep_inline_nesting_is_rejected_on_a_wasm_sized_stack) | test(=test_stdin_eof_with_hanging_convert_cleans_engine_and_workspace)'
```

Log: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework5-controlled-idle.log`. Terminal: `=== 10:07:19 ALL GREEN controlled head=b19d60d70b3aaecf353156ca3f17af59d3827d4c`.

## Historical rework 5 final gate and five failures

The detached runner used slot 1, nice 19, Pandoc 3.12 on PATH and through `ASHIFT_PANDOC`, and checked out the exact committed head without overlays. Named checks used `CARGO_BUILD_JOBS=4`; Clippy workers were limited to two CPUs. Test-binary preparation used two jobs/two CPUs to control linker load. Actual tests used `NEXTEST_TEST_THREADS=1` and all eight CPUs after load fell below 12 with more than 8 GiB available memory.

```sh
cargo fmt --all --check
cargo clippy -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --all-targets --features ariad-host/test-probe -- -D warnings
cargo nextest run -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --features ariad-host/test-probe --no-fail-fast
```

Fmt and Clippy passed. Nextest ran **885 tests across 47 binaries**, without filters, including the required probe suites. Summary: `Summary [ 531.144s] 885 tests run: 880 passed, 5 failed, 12 skipped`.

| Crate | Passed | Failed |
|---|---:|---:|
| `ariad-cli` | 167 | 4 |
| `ariad-core` | 401 | 1 |
| `ariad-host` | 304 | 0 |
| `ariad-wasm` | 8 | 0 |

All failures:

1. `ariad-cli::commands_cli::convert_format_mismatch_warning_json_output`: `commands_cli.rs:882`, exit `Some(5)` instead of `Some(0)`; 7.169 s. Its assertion does not print conversion stderr. The same compiled test passed in 0.48 s in isolation; the original exit-5 cause is not independently proven.
2. `ariad-cli::commands_cli::doctor_human_output`: its snapshot gains `; found 5 stale workspace(s)`.
3. `ariad-cli::commands_cli::doctor_json_output`: the workspace message changes and `stale_workspaces` becomes 5 instead of 0. `ariad-host/src/workspace.rs::count_stale` scans `std::env::temp_dir()`. Both existing doctor snapshots match in a fresh private `TMPDIR`; neither was accepted or changed.
4. `ariad-cli::route_golden::every_docx_fixture_has_a_reviewable_md_golden`: `route_golden.rs:368`, the unaccepted `synthetic-merged-table.md` diff. Updated `vi-route-data`, updated `vi-epub-table-image`, and the unchanged HTML golden passed.
5. `ariad-core::html_reader::six_megabyte_table_heavy_page_parses_under_local_limits`: `html_reader.rs:1421`, `6MB table parse took too long: 38.122636013s`, exceeding the unchanged 30-second bound; whole-test time 38.715 s. It passed in 9.240 s in the normal-load control.

The final phase started at load **10.71** but later ranged **5.05–30.39**: 72/107 samples were at least 12 and 47 exceeded 24. Sequential test durations sum to 530.849 s versus 531.144 s total, leaving 0.295 s scheduling/startup overhead. The reconstructed 6 MiB test window is phase seconds 294.528–333.538; its sampled loads were **24.8–29.76**. This failure is not a normal-load reproduction. Its assertion was not changed.

- Failed gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework5-final-gate-launch.log`.
- Inner runner log: `/home/vchun/Codes/ariadshift-logs/t10-writer-verify-111601.log`.
- Exact terminal: `STOPPED: final gate head=a429dc73a50040377522842c0d6c761159f3325c rc=1`.
- Load trace: `/tmp/d2120-t10-rework5-final-load.jsonl`.

The wrapper stopped before `just lint-commits`. That command passed on the same head in the follow-up below; receipt: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework5-lint-commits.log` (`EXIT: 0`). There is no ALL GREEN full-gate log.

## Historical isolated follow-up and process state

A separate slot-1/nice-19 job ran the actual compiled `commands_cli` binary recorded in final nextest metadata, with a private `TMPDIR` and unchanged assertions:

```text
commands_cli-44f2530b11336e71 doctor_human_output --exact --nocapture
commands_cli-44f2530b11336e71 doctor_json_output --exact --nocapture
commands_cli-44f2530b11336e71 convert_format_mismatch_warning_json_output --exact --nocapture
```

All three passed. The same job compared the third golden's complete reader IR bodies and ran `just lint-commits`; both passed. Exact commands/paths: `/home/vchun/Codes/ariadshift-logs/d2120-t10-rework5-env-repro.log`; driver: `/tmp/d2120-t10-rework5-env-repro.py`. Terminal: `=== 12:04:22 ALL GREEN environment-repro head=a429dc73a50040377522842c0d6c761159f3325c`. This narrow diagnostic does not replace the failed full gate.

Compilation-only attempts were cancelled only within owned groups before tests: `d2120-t10-rework5-controlled.log` separated build preparation from measurement; `d2120-t10-rework5-focused.log` stopped a four-job linker burst at load 112.17; `d2120-t10-rework5-focused-bounded.log` narrowed preparation to explicit test binaries. Their `STOPPED` logs are not passing evidence. Queued jobs waited normally. No process was SIGSTOP-paused and no other worker's process was changed. Reconciliation found no live owned gate, test or engine processes. The report remains local/uncommitted with an identical operations copy.

## Historical cache and writer evidence

The accepted `b19d60d` cache fix keeps hits on a short token and normalizes/hashes the long URL once per definition. For a 1.78 MiB Base64 definition, historical debug reads were 32.098975693/250.174000781 s before the fix and 0.886532211/1.452271601 s afterward, for 2,000/20,000 references. IR sizes were 2,212,552/5,290,552 bytes with every image and exactly 1,402,689 decoded bytes preserved once. The larger fixed read had 6.9 times headroom against its unchanged ten-second limit. Logs under `/home/vchun/Codes/ariadshift-logs/`: `t10-writer-verify-074121.log` (expected regression), `t10-writer-verify-074619.log` (fixed focus), `d2120-t10-rework4-gate-launch.log` (historical core-only gate). These are historical measurements, not fresh timings on `a429dc73a50040377522842c0d6c761159f3325c`.

The first ten writer proving tests were added before either writer changed and all failed on original base `6fc7e065d36af18c8d0b82ef25b62073d5671d08` (`t10-writer-verify-015714.log`, ending `STOPPED: cargo test -p ariad-core --test writer_bounds -- --nocapture --test-threads=1 rc=101`). The writer-only fix then passed 16/18 before the reader follow-up (`t10-writer-verify-030300.log`, same terminal failure line). Both logs are under `/home/vchun/Codes/ariadshift-logs/`.

| Check | Original writers | Fixed writers |
|---|---|---|
| Leading spaces, debug | 300,000 spaces took 104.259582664 seconds | 1,000,000 spaces took 203.167411 milliseconds in the earlier private-token focused writer run; the unchanged 10-second bound gives about 49 times headroom |
| Markdown image scaling | 1,099,400 to 4,382,800 bytes | 10,304 to 20,564 bytes |
| HTML image scaling | 1,104,235 to 4,391,635 bytes | 8,740 to 16,600 bytes |
| Empty-table scaling | 755,505 to 3,011,005 bytes | 3,657 to 6,157 bytes |
| 10,000 references | 10,000 copies of the data URI | One copy per writer |

The 512 MiB internal output reservation, supported nesting, sparse/128-column table bound, format-specific table annotation behavior, safe footnote identifiers and escaped HTML fallback remain covered and green.

For the original read-back blocker, historical `reader/markdown.rs:93` on base `3f07309` called Comrak 0.56.0. Its `src/parser/mod.rs:2059` sets `max_ref_size = total_size.min(100000)`; `src/parser/inlines.rs:2443-2448` charges `url.len() + title.len()` at every lookup and silently returns `None` when the remainder is insufficient. It emits no error or warning. The PNG case had a 2,676-byte decoded asset, a 3,590-byte URL and a 3,680-byte source/budget: the counter reached 3,590, leaving 90, so the second and third references failed and the assertion reported `left: 1`, `right: 3`. The common-title case had source/budget 171, URL 66 and title 12: the counter progressed 0 to 78 to 156, leaving 15, so the third lookup failed. These budget rules remain enabled; shared asset identifiers avoid repeated decoded data and long URL expansion.

## Handoff and remaining merge condition

Final process reconciliation found no live owned gate, test or engine processes. The verification worktree is clean; the task worktree contains only this untracked report.

All work requested in rework 6 is complete. The new commit contains only the two reviewed compact-row snapshots; earlier writer/reader changes, all tests and bounds are intact. No additional source, test, manifest or doctor golden changed. At that stage the report was local/uncommitted with an identical operations copy. No push or PR operation was performed. Full GitHub CI on all three OS runners remains the merge condition after the lead's synchronization.

Status: DONE
