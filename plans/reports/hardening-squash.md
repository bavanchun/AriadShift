# Hardening squash CI fixes

PR [#103](https://github.com/bavanchun/AriadShift/pull/103) targets `dev`.
The Windows startup test, the two flagged diagnostics, and Windows MSI
component authoring are repaired locally. No CI-fix commit has been pushed.
A successful dev run on the same new macOS image establishes a
PR-dependent link-size regression; the native fix decision needs the lead.
No merge or auto-merge was requested.

## Revisions and delivery boundary

- Branch: `feat/v1-hardening-squash`.
- Published PR head: `65f20aaad527e548a7526423cc077dc80b850bd5`.
- Unmodified dev baseline: `3c7f058dd3f82e6fb92c89fc7ada834d350c2f7c`.
- Verified source head: `064271ae48647ef3074e3485b61dfaff6e6c7307`.
- `645529ef6b0bbcfd618be19e626cbd679352e667`:
  `fix(ci): make startup tests portable and diagnostics static`.
- `064271ae48647ef3074e3485b61dfaff6e6c7307`:
  `fix(desktop): assign explicit GUIDs to MSI file components`.

New conventional commits are above the accepted published squash. There
has been no rebase, history rewrite, force push, or push during this task.
The single permitted CI-fix push remains unused. Publication must wait
for the macOS decision and lead ACCEPT of the resulting revision.

The earlier publication record and complete original gate evidence are
preserved in the [publication report](/home/vchun/Codes/ariadshift-ops/reports/hardening-squash-published-65f20aa.md).
The accepted squash was the reviewed tree of `9481b07`, with one parent
`3c7f058`, and passed the original exact-head gate. Its terminal marker is
`=== hardening-squash ALL GREEN`. That historical result does not imply
that remote PR CI passed.

## 1. Windows Verify: portable startup test

The failing Windows Verify job used `windows-2025-vs2026 20261021.278.1`.
Classification: deterministic PR test defect. The Unix-only file URL has
no Windows drive, and `Url::to_file_path` rejects it. The test now creates
a native temporary directory and constructs its URL with
`Url::from_directory_path`. The directory guard stays alive through both
the API and worker checks. All assertions and production code are intact.

The focused `startup_secret_policy` suite passes all 11 tests on Linux.
The existing portable configuration test uses the same URL constructor.
A `PureWindowsPath` shape probe produced
`file:///C:/Users/runneradmin/AppData/Local/Temp/ariad%20secret%20policy/`;
this establishes an absolute drive and correct escaping by construction,
not native Windows execution. A Windows Verify run of the new revision
is still required.

## 2. CodeQL: static assertion messages

The exact predicates remain `!debug.contains(sentinel)` and
`keys.ids().contains(&id)`. Only the failure diagnostics changed:
`configuration debug leaked a secret` and `verified by an unknown key`.
The first test must not dump configuration if redaction ever regresses;
the second value was a public signing-key identifier, not a secret.
Production validation, CodeQL settings and rule severity are unchanged;
no alerts were dismissed.

The redaction regression test passes; the fuzz target builds with its
locked manifest. The focused redaction selection filters 66 unrelated
tests; those are not missing database coverage. CodeQL clearance is not
proven until GitHub analyzes a published revision.

## 3. macOS: PR-dependent link growth; native fix decision pending

Both PR jobs stop linking the `ariad-desktop` binary. Apple ld reports
`__eh_frame` exceeding the 16 MB compact-unwind encoding limit. Rust emits
`linker_messages`, and `CARGO_BUILD_WARNINGS=deny` turns it into failure.
Debug information is already disabled in both workflows. This is the same
root cause in Verify and Desktop; no Seatbelt runtime test failed first.

Both workflows support manual dispatch. CI runs on PRs, dev/main pushes,
and manual dispatch. Desktop runs on matching PR paths, tags, and manual
dispatch, so ordinary dev pushes supply no Desktop baseline.

### Runner image evidence

Every image below is read from the Runner Image block, not the separate
runner provisioner version or the job label.

| Run / job | Revision | Image | Result |
| --- | --- | --- | --- |
| PR CI `41055230540` / `124061026448` | `65f20aa` | `macos-26-arm64 20261021.0445.8` | Unwind linker warning promoted to failure. |
| PR Desktop `41055230497` / `124060965579` | `65f20aa` | `macos-26-arm64 20261021.0445.8` | Same failure. |
| Dev dispatch `41057851204` / `124069852778` | `3c7f058` | `macos-26-arm64 20261013.0424.1` | Verify and whole workflow passed, on the OLD image. |
| Dev dispatch `41060733754` / `124079599567` | `3c7f058` | `macos-26-arm64 20261021.0445.8` | Verify and whole workflow passed; 1,093 tests passed, 10 existing ignored. |
| Dev dispatch `41060791127` / `124089592065` | `3c7f058` | `macos-26-arm64 20261026.0455.2` | Canceled after the decisive baseline; no test result. |

The initial dev dispatch is [run 41057851204](https://github.com/bavanchun/AriadShift/actions/runs/41057851204/job/124069852778).
Its success does not establish that dev passes on the new image. The
additional two dispatches are the bounded attempts requested by the lead.
CI serialized these workflows for the same SHA, so the latter waited for
the former; no source was pushed to obtain any baseline.

The decisive [dev run 41060733754](https://github.com/bavanchun/AriadShift/actions/runs/41060733754/job/124079599567)
uses the exact base SHA and NEW image, the same Rust 1.99.0 and warning
denial as the failing PR jobs. The full macOS Verify job and workflow
succeeded, including app linking, 1,093 tests, doctests and packaging.
This excludes a runner-only failure independent of the PR. Classification:
PR-dependent desktop/host link growth on the new image. The old-image
run `41057851204` was not used to draw this conclusion.

Once that same-image baseline was available, the redundant third dispatch
`41060791127` was canceled to stop unnecessary CI work. Its canceled
status is not a build result and contributes no attribution evidence.

### Link input and mitigation evidence

The desktop, host and Typst Cargo manifests are unchanged over dev.
The lockfile addition is server `rmcp` transport support and `sse-stream`;
it does not explain the standalone Desktop job. The new Windows startup
lock is excluded from the ordinary macOS app by `cfg(windows)`.
The linked additions include file-open parsing/delivery (382 source
lines, including tests), its Tauri launch/event hooks, the macOS Seatbelt
launcher/profile code (190 and 274 lines, including tests), and the shared
host sandbox/runner changes. The standalone Desktop failure rules out
server-only rmcp transport as a sufficient explanation. Source
line counts do not identify which crosses a native linker threshold.
Linux ELF measurements cannot establish Apple Mach-O compact-unwind
encoding sizes, and no same-image native symbol map has been obtained.

Current Linux executables were selected from `cargo nextest list` metadata,
not guessed from cached artifact hashes. With debug information disabled:

| Linux executable | File bytes | `.eh_frame` bytes |
| --- | --- | --- |
| Desktop library tests | 70,652,128 | 1,816,832 |
| Desktop binary test harness | 1,588,576 | 51,696 |
| Ordinary app binary built for workspace integration tests | 249,963,568 | 10,274,592 |

This shows why the small test harness is not a proxy for the app link.
It is not a dev/PR comparison and excludes macOS-only Seatbelt code;
ELF/DWARF sizes do not predict Apple's compact-unwind offset encoding.

No macOS workaround has been applied. The same-image result resolves
PR-versus-runner attribution, but source counts and Linux sizes cannot
isolate a particular macOS function or prove a native mitigation. Removing
accepted file-open or Seatbelt behavior would be an unjustified scope cut.
The lead decision is between the following concrete options:

1. Obtain section sizes and link maps for the dev and PR app on image
   `20261021.0445.8`, and isolate the added desktop/host code with native
   experiments. Start with package-scoped test optimization, preserving
   unwind behavior and warning denial.
   The proposed native command is
   `cargo --config 'profile.test.package.ariad-desktop.opt-level=1' nextest run --locked -p ariad-desktop`.
   Linux only established that Cargo accepts this override; successful
   macOS linking and unchanged native behavior must still be measured.
2. Approve `#![cfg_attr(target_os = "macos", allow(linker_messages))]`
   at `apps/desktop/src-tauri/src/main.rs`'s crate root, while retaining
   `CARGO_BUILD_WARNINGS=deny` and every other lint. This changes diagnostic policy, not link inputs or unwind tables.
   It suppresses all linker-message warnings for that binary, not just the
   size warning, so the scope and trade-off require explicit acceptance.
3. Trial `-Wl,-no_compact_unwind` only on Darwin. Apple ld's published
   [option parser](https://github.com/apple-oss-distributions/ld64/blob/main/src/ld/Options.cpp)
   recognizes `-no_compact_unwind`. Cargo accepts the proposed Darwin
   rustflags syntax on Linux, but that does not prove current native ld
   acceptance, successful linking, or correct panic unwinding. A primary
   [Rust issue](https://github.com/rust-lang/rust/issues/156812) reports
   unwind concerns with this option. It is not a safe blind default.

A controlled Linux compiler/linker probe tested option 2's mechanism:
with warning denial, a synthetic linker diagnostic fails (exit 101);
adding crate-level `allow(linker_messages)` succeeds (exit 0); adding an
unrelated dead-code warning still fails (exit 101). The temporary crates
were removed. This is not a macOS build, runtime or production fix.
The [Rust lint reference](https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html#linker-messages)
and [Cargo warning policy](https://doc.rust-lang.org/cargo/reference/config.html#buildwarnings)
document the two independent controls. A package-scoped test optimization
profile also passes Cargo configuration syntax validation, but its size
benefit and build cost need a native experiment before selection.

Recommendation: prefer a native size/optimization experiment retaining
warning denial. If the lead accepts a narrowly scoped diagnostic
workaround before identifying the exact contributing functions, trial
option 2 on the failing macOS binary and run native conversion plus panic
unwinding checks. Do not relax workspace warnings or alter panic strategy.

## 4. Desktop failures

### Ubuntu: transport rerun candidate

PR job [124060965411](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060965411)
and the earlier successful Desktop job
[123155395189](https://github.com/bavanchun/AriadShift/actions/runs/40784863931/job/123155395189)
both use `ubuntu-26.04 20261025.179.1`, WebKitGTK
`2.52.6-0ubuntu0.26.04.1` and tauri-driver `2.1.0`.
The earlier successful revision is `331efae48d55426ebb82c5e26576631adf49efa2`.
The smoke script is byte-identical, and the summary, path input and Add
file subtree are byte-identical too: SHA-256
`9bfb4800e071e241ea3d90289afb676b291bbb4f8b4023093e8cee711b463d4c`.
The click target `//summary[normalize-space()='Add a local file by path']`
exists in this PR.

The failed log records a WebDriver connection reset and
`RemoteDisconnected` at the click, while the app, WebKit processes and
both driver listeners remain present. Classification: observed transport
failure and rerun candidate; a deterministic interaction is not proved
or completely excluded by one failure. No UI change or retry policy was
added, and no unchanged PR rerun was triggered.

The repository supports
`xvfb-run -a dbus-run-session -- just desktop-smoke`, but this ASUS
worktree lacks `xvfb-run`, `tauri-driver` and `WebKitWebDriver`. Therefore
no local AppImage WebDriver smoke is claimed. Rerun the original smoke
once on the decided revision with driver diagnostics retained.

### Windows: seventh failure, deterministic MSI defect repaired

[Job 124060965616](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060965616)
is the 24m22s Desktop Windows job in run `41055230497`. Its image is
`windows-2025-vs2026 20261021.278.1`. Rust release compilation succeeded
in 15m59s; the pre-bundle resource check also passed. WiX candle
`3.14.1.8722` then rejected generated `main.wxs` lines 132 and 136 with
`CNDL0230`: `Guid="*"` cannot identify a file-bearing component whose
keypath is a registry value.

Classification: PR installer authoring defect, independent of Windows
Verify's file URL, the macOS linker limit, and Ubuntu's transport error.
Rerunning the unchanged job cannot fix this authoring error.

The handwritten per-user template now assigns a fixed new executable
GUID and uses `{{ bin.guid }}` for additional binaries. The pinned Tauri
CLI's Binary data exposes that GUID field. HKCU registry keypaths,
per-user limited installation, file entries and upgrade/uninstall order
are preserved. Explorer and generated resource GUIDs are unchanged.
The additional-binary GUID follows Tauri's existing per-build generation;
this fix does not change its upstream lifecycle policy.

The regression check failed on the original automatic-GUID template,
then passed on the fix. All 10 MSI/generator checks pass. The desktop
documentation now describes the actual explicit GUID policy.
Native candle/light compilation, MSI install/upgrade/uninstall and NSIS
remain Windows validation requirements; Linux tests do not prove them.

The [WiX Component documentation](https://docs.firegiant.com/wix3/xsd/wix/component/)
limits automatic component GUIDs to supported keypath/file combinations.
The field is confirmed in
[Tauri CLI 2.12.4's MSI source](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/windows/msi/mod.rs).

## Gates and review

The source gate ran detached at nice 19 with four Cargo jobs, two nextest
threads, pinned Node/pnpm/uv and every heavy command through
`~/Codes/ariadshift-ops/gate-slot.sh 2`. Its runner is
[hardening-squash-ci-combined-gate.bash](/home/vchun/Codes/ariadshift-logs/hardening-squash-ci-combined-gate.bash).
The [combined log](/home/vchun/Codes/ariadshift-logs/hardening-squash-ci-combined-gate.log)
prints source head `064271ae48647ef3074e3485b61dfaff6e6c7307` at entry
and completion, and ends:

```text
=== hardening-squash CI combined ALL GREEN
```

| Exact command | Result |
| --- | --- |
| `node --test apps/desktop/os-integration/msi-scope.test.mjs apps/desktop/os-integration/generate.test.mjs` | 10 passed, zero skipped. |
| `cargo nextest run --locked -p ariad-server --test startup_secret_policy` | 11 passed, zero skipped. |
| `cargo nextest run --locked -p ariad-server --lib -E 'test(configuration_debug_never_includes_secrets)'` | Selected regression passed; 66 unrelated tests filtered. |
| `cargo check --locked --manifest-path fuzz/Cargo.toml --bin pack_manifest` | Passed. |
| `just desktop-ci` | Bindings, build, 25 UI tests, 17 tool tests, and 29 Rust tests passed. |
| `just test` | 1,262 passed, 12 existing ignored tests; doctests passed. Database/S3 tests ran with required database mode. |
| `just static` | Passed, including typos, licences, formatting and workflow guards; 78 workflow mutation controls passed, two PowerShell controls skipped. |
| `cargo clippy --locked -p ariad-server --all-targets --features ariad-host/test-probe -- -D warnings` | Passed. |
| `just lint-commits origin/dev..HEAD`; `git diff --check` | Passed. |

The disposable PostgreSQL/pgmq and S3 services used owned container names
`ariadshift-hardening-ci-pg` and `ariadshift-hardening-ci-s3`, on loopback
ports 55489 and 58389. Both were removed; both ports are unbound. Existing
services and foreign processes were not changed.

The earlier exact-`645529e` focused gate remains in
`hardening-squash-ci-fixes-gate.log`, ending
`=== hardening-squash CI fixes ALL GREEN`. It is supplementary evidence,
not the final source revision gate. An initial formatting failure is
retained in `hardening-squash-ci-fixes-gate-first.log`; formatting was
corrected and the complete focused gate passed afterward.
The intentional MSI regression run ends `EXPECTED REGRESSION EXIT: 1`;
it fails on the old automatic GUID, and the repaired-template run ends
`=== hardening MSI tools ALL GREEN`. Additional evidence ends
`=== hardening additional evidence ALL GREEN` and the linker probe ends
`=== hardening linker policy probe ALL GREEN`.

A subsequent report-only commit records this result. The final delivery
SHA and clean-tree receipt are in
[hardening-ci-delivery-receipt.json](/home/vchun/Codes/ariadshift-logs/hardening-ci-delivery-receipt.json).
The report-only delivery is checked through slot 2 with
`typos plans/reports/hardening-squash.md`,
`just lint-commits origin/dev..HEAD`, and `git diff --check`.
The final [documentation gate log](/home/vchun/Codes/ariadshift-logs/hardening-ci-delivery-gate.log)
prints its exact delivery head at entry and completion and ends
`=== hardening CI delivery ALL GREEN`. It checks this new report; the
full source gate above is retained without claiming a native fix.

The scoped review used `ak-code-review` on all fixes above, compared the
unchanged assertion predicates and directory lifetimes, and inspected
Tauri's template context plus WiX keypath rules. No open critical or high
finding was found in these changes. Native platform and CodeQL gates
remain explicitly unproven.

## Evidence and remaining limits

All raw logs were downloaded with a timeout and written to files before
inspection. In-progress job-log retrieval can return unavailable; this
is a retrieval limitation, not a job result. Evidence lives under
`/home/vchun/Codes/ariadshift-logs/`:

- `hardening-ci-pr-windows-desktop.log`: seventh failure, exact authoring errors.
- `hardening-ci-pr-linux-desktop.log` and
  `hardening-ci-old-linux-desktop.log`: same-image smoke comparison.
- `hardening-ci-baseline-results.json` and `hardening-ci-dev-41060733754-macos.log`:
  the decisive new-image dev success, job metadata and exact image version.
- `hardening-ci-extra-baseline-cancel.log`: redundant dispatch cancellation.
- `hardening-ci-dev-new-image-macos.log`: despite the historical filename,
  run `41057851204` actually used the OLD image.
- `hardening-ci-local-evidence.json`: URL reasoning and UI/script checks.
- `hardening-ci-linux-unwind-sizes.json`: Linux ELF evidence and caveat.
- `hardening-ci-linker-policy-results.json`: controlled warning-policy probe.
- `hardening-ci-tauri-wix-source.rs`: pinned upstream GUID field evidence.

`gh pr checks 103` snapshot at `2026-10-30T13:29:28.020271+00:00`: 7 fail, 35 pass, 8 skipping, 1 pending.
Its exit code is 1 because existing PR checks failed. `CI passed` is an
aggregate of the failed CI matrix, not an additional independent cause.
These checks belong to published `65f20aa`, not either local fix commit.
The PR remains open into dev, unmerged, with auto-merge disabled.
Raw output is `hardening-ci-pr-checks.txt`; metadata is
`hardening-ci-pr-state.json`.

| Check | State | Duration | Details |
| --- | --- | --- | --- |
| CI passed | fail | 2s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124070118728) |
| CodeQL | fail | 2s | [job](https://github.com/bavanchun/AriadShift/runs/124061219765) |
| Desktop (macos-26) | fail | 6m47s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060965579) |
| Desktop (ubuntu-26.04) | fail | 15m7s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060965411) |
| Desktop (windows-2025) | fail | 24m22s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060965616) |
| Verify (macos-26) | fail | 8m23s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026448) |
| Verify (windows-2025) | fail | 8m11s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026536) |
| Analyze (actions) | pass | 43s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230564/job/124060965907) |
| Analyze (rust) | pass | 10m46s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230564/job/124060965703) |
| Engine conformance (real engine) | pass | 5m5s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230376/job/124060965101) |
| Engine tests (ubuntu-26.04) | pass | 47s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230376/job/124060965394) |
| Engine tests (windows-2025) | pass | 1m24s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230376/job/124060965496) |
| Fuzz | pass | 21m16s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026517) |
| Fuzz (ASan, archive_preflight) | pass | 13m38s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966380) |
| Fuzz (ASan, docling_to_ir) | pass | 12m48s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966319) |
| Fuzz (ASan, docx_meta) | pass | 13m5s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966323) |
| Fuzz (ASan, html_reader) | pass | 13m50s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966222) |
| Fuzz (ASan, ir_json) | pass | 13m48s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060965927) |
| Fuzz (ASan, markdown_reader) | pass | 14m0s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966257) |
| Fuzz (ASan, pack_archive) | pass | 12m29s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966368) |
| Fuzz (ASan, pandoc_ast_to_ir) | pass | 12m45s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966295) |
| Fuzz (ASan, pdf_reader) | pass | 11m31s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230576/job/124060966318) |
| Lint (windows-2025) | pass | 6m32s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026409) |
| Pack (aarch64-unknown-linux-gnu) | pass | 7m16s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230376/job/124061583569) |
| Pack (x86_64-unknown-linux-gnu) | pass | 27m15s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230376/job/124061583632) |
| Pack manifest | pass | 5m3s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230376/job/124073282822) |
| Remote MCP OAuth (core) | pass | 9m13s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230352/job/124061018958) |
| Request source-matched CI pack | pass | 7s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230352/job/124061018844) |
| Scope | pass | 4s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230352/job/124060965445) |
| Scope | pass | 10s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230376/job/124060965497) |
| Scope | pass | 5s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124060965293) |
| Server integration (ubuntu-26.04) | pass | 15m2s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026532) |
| Static checks | pass | 2m43s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026580) |
| Verify (ubuntu-26.04) | pass | 6m2s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026548) |
| Web | pass | 12m33s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230540/job/124061026682) |
| build-global-artifacts | pass | 36s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124070178156) |
| build-local-artifacts (aarch64-apple-darwin) | pass | 13m17s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124061200366) |
| build-local-artifacts (aarch64-unknown-linux-musl) | pass | 7m20s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124061200472) |
| custom-publish-crates | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124070459280) |
| plan | pass | 30s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124060965885) |
| build-local-artifacts (x86_64-pc-windows-msvc) | pass | 20m59s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124061200448) |
| Desktop release (${{ matrix.os }}) | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060967561) |
| Benchmark (Ubuntu) | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230421/job/124060966676) |
| build-local-artifacts (x86_64-unknown-linux-musl) | pass | 10m55s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124061200333) |
| build-local-artifacts (x86_64-apple-darwin) | pass | 12m54s | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124061200434) |
| Verify desktop updater release | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060967461) |
| Wait for source-matched pack | pending | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230352/job/124061098776) |
| announce | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124070460488) |
| host | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124070459239) |
| prepare-homebrew-formula | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124070459042) |
| publish-homebrew-formula | skipping | 0 | [job](https://github.com/bavanchun/AriadShift/actions/runs/41055230476/job/124070460770) |

Known earlier limits remain: no Linux proof of macOS Seatbelt, incomplete
cloud WebKit coverage, the later security re-land's unknown-path CSP note,
and the inherited optional non-UTF-8 DB URL and ZIP name-byte follow-ups.
PowerShell mutation checks require `pwsh`, which is absent here. The
original gate's Next.js panic warnings were nonfatal and were not changed.

No owned local gate, test runner, or disposable service remains.
The baseline monitor exited after saving both logs; the redundant remote
baseline was canceled after the decisive comparison.
The working tree is clean after the report-only commit. The result is
copied to `/home/vchun/Codes/ariadshift-ops/reports/hardening-squash.md`.
The native macOS mitigation decision is left to the lead, with the single
CI-fix push unused.

Status: NEEDS_CONTEXT
