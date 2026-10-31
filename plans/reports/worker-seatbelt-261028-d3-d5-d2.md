# macOS Seatbelt implementation report

Date: 2026-10-28. Runtime: macOS 27.0.1, Apple Silicon.
Worktree: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-seatbelt`.
Branch: `feat/v1-hardening--seatbelt`.
Base: `f9ecfceb09fd2cbe8c5b02b85f6e84020bcb6b14` from the fetched `origin/feat/v1-hardening`.
Head: `2d9ac106205e16a3938054e905cafd8abe507a44`.

The three repairs are implemented in spec order and committed separately. No push or PR was made. The main checkout remains on `dev`, with no tracked changes; this requested report is the only file written there. The implementation worktree is clean. No task-owned background process remains running.

## Commits

- `321ae7c` — `fix(sandbox): let macos engines read system identity` (D3).
- `91d2e9e` — `fix(sandbox): exclude package-manager state from macos grants` (D5).
- `2d9ac10` — `fix(sandbox): confine libreoffice ipc to a private macos directory` (D2).

## Proving tests and real engines

- D3: added `engines_can_read_the_system_identity` first. After resolving the startup prerequisites below, it failed before the hostname grant with `uname: sysctl: Operation not permitted`, then passed with `kern.hostname`. A real default-mode Docling conversion of `fixtures/scan/vi-garden-300dpi.png` produced `Khu vườn đọc sách` with an empty warning list.
- D5: added the two specified policy tests and the runtime state-read test before editing grants. Both policy tests failed, and the runtime probe could read an actual Homebrew fontconfig cache under `/opt/homebrew/var`. After narrowing all three grant sources, those tests and the real Pandoc quoted-directory conversion passed. Docling OCR was rerun after narrowing: the same Vietnamese title was present and warnings remained empty.
- D2: the existing PDF/Typst/Office launcher test failed at the real Office leg before the fix. The planner selected `libreoffice` for the legacy DOC fixture, and CLI conversion failed. The new IPC probe also failed before the implementation because binding was denied. After the fix, all three built-in engine contract tests passed with Office installed, and real DOC to DOCX conversion preserved the Vietnamese title with no warnings.
- The IPC test verifies allowed bind/connect only inside the dedicated directory; denied sockets outside it, including inside the writable workspace; denied symlink escape; denied TCP/UDP; and directory cleanup after process exit. Another test checks a typed best-effort profile failure without executing an unconfined fallback.

## Final gates

All runtime tests used the real `/usr/bin/sandbox-exec`; no confinement-support override was set in the test environment. Existing tests exercise their own missing-support subprocess deliberately.

Local environment: `CARGO_TARGET_DIR=/private/tmp/ariad-seatbelt-build`, `ASHIFT_PANDOC=/private/tmp/ariad-seatbelt-pandoc/bin/pandoc`, and `ASHIFT_SOFFICE=/private/tmp/seatbelt-libreoffice/LibreOffice.app/Contents/MacOS/soffice`. `just` uses its worktree-local Pandoc path, which points at that same verified binary.

| Command | Result |
|---|---|
| `cargo nextest run -p ariad-host --features test-probe --test sandbox_macos` | PASS: 9 passed, 0 skipped. |
| `cargo nextest run -p ariad-host --lib` | PASS: 149 passed, 0 skipped. |
| `just test` | FAIL, exit 101: the existing large debug CLI triggers Apple's linker warning `__eh_frame section too large (max 16MB)`, which the recipe's `CARGO_BUILD_WARNINGS=deny` rejects. Warning handling was not weakened. |
| `just lint-commits` | FAIL, exit 1: inherited commits `afb5ea1` and `9734cfc` exceed length limits; `8d9d72d` contains a prohibited tool reference. No inherited history was rewritten. |
| `just lint-commits f9ecfce..HEAD` | PASS: all three new commits comply. |
| `cargo clippy --workspace --all-targets --features ariad-host/test-probe` | PASS, no warnings after the test helper was adjusted to the current Clippy syntax. |
| `cargo fmt --all --check` | PASS. |
| `cargo test --workspace --doc` | PASS. |
| `cargo nextest run -p ariad-cli --features ariad-host/test-probe --test builtin_engine_contracts` | PASS: 3 passed, 0 skipped, including real LibreOffice. |

A supplemental `cargo nextest run --workspace --features ariad-host/test-probe --no-fail-fast` completed with 994 tests run: 989 passed, 5 failed, 10 skipped. Its five failures are:

1. `commands_cli::doctor_json_has_no_leaked_paths`: existing doctor text includes `/usr/bin/sandbox-exec`.
2. `commands_cli::doctor_reports_the_engine_sandbox`: existing assertion expects macOS platform support to be false.
3. `sandbox_cli::best_effort_without_kernel_support_converts_and_reports_the_gap`: existing assertion expects no macOS sandbox warning.
4. `ariad-server sandbox::tests::required_uses_the_host_confinement_contract`: existing supported value omits Seatbelt fields.
5. `openapi_drift::openapi_document_is_committed_and_current`: committed OpenAPI differs from generation.

Those test/contract files are unchanged from `f9ecfce`. They were left for their owning hardening work, rather than changing unrelated contracts or hiding failures.

Final logs: `/tmp/seatbelt-gate-{macos,host-lib,just-test,lint-commits,own-commits}.log`; supplementary logs: `/tmp/seatbelt-final-{workspace-diagnostic,clippy,fmt,doc-tests}.log`. Engine-only denial evidence is saved in `/tmp/seatbelt-office-and-probe-denials.log`.

## Spec deviations and environment

- On this Mac, the original profile killed `/usr/bin/true` in dyld before the hostname test could run. A literal read of the root directory entry `/` fixes macOS 27 startup without granting descendants. Existing tests also exposed denial of Apple's `/var` aliases and Pandoc's inherited, ungranted cwd. D3 therefore preserves the fixed `/var`, `/tmp`, `/etc` aliases for scoped canonical grants and starts inside an allowed directory. Existing document, symlink, installation and network confinement tests pass. A pre-existing profile unit test also needed `deny_network: true` to match its assertion.
- D2 uses a short, host-owned temporary IPC directory outside the normal workspace, since the normal path plus LibreOffice's pipe name can exceed Darwin's socket-path limit. This required runner lifetime management and forwarding `OSL_SOCKET_PATH` through the LibreOffice adapter's cleared environment. The SBPL socket forms from the spec were validated on this Mac. Only the observed `com.apple.coreservices.launchservicesd` lookup was added; clipboard and WindowServer services remain denied. LibreOffice is not exempted, `/opt` remains Linux-only, and no desktop setting was added.
- The Homebrew LibreOffice wrapper was stale and `/Applications/LibreOffice.app` was absent. Tests used a temporary bundle of the [official stable LibreOffice 26.8.1 download](https://download.documentfoundation.org/libreoffice/stable/26.8.1/mac/aarch64/LibreOffice_26.8.1_MacOS_aarch64.dmg), reporting version 26.8.1.1; SHA-256 was `ca074e0b13571efb30a31fc531c5f95e997030637325e5d5de92d67aefdf0dfc`. The disk image was detached after copying the bundle.
- `just pandoc` installed the pinned 3.12 binary. Cargo-nextest 0.9.146 was already present. Tesseract 5.5.3 was installed through Homebrew for real OCR. `just docling-dev` initially refused system uv 0.12.21; setup then used the repository-required uv 0.12.23 through `uvx`, with frozen dependencies and the repository asset fetcher. Build and Docling runtime artifacts were placed on the Mac SSD after external-volume I/O stalls; no dependency manifest or lockfile changed.
- `ARCHITECTURE.md` was not edited, as the spec reserves its Seatbelt rewrite for D27. The owning CLI documentation was updated with the implemented behavior.

## Open concerns

The mandatory all-green gate is not met: the workspace linker warning, five inherited test/contract failures and three inherited commit-lint violations need their owning work before integration. Linux and Windows runtime validation was not performed on this Mac.

Status: DONE_WITH_CONCERNS
Summary: All three macOS repairs are committed and pass targeted Seatbelt, host-library and real-engine checks. Full delivery remains conditional on resolving the inherited workspace and commit-range gate failures above.


## Rework 1

All three review findings are repaired in the requested worktree on macOS with real `/usr/bin/sandbox-exec`. No finding was rejected. New HEAD: `91c520f9d746e9d0de248528afc51bb7b10ac42b`. No push was made, the implementation worktree is clean, and no task-owned background process remains.

### Commits

- `825bfaa` — `fix(host): create macos ipc directories with owner-only access`.
- `d98248a` — `fix(host): resolve relative macos overrides before changing cwd`.
- `91c520f` — `fix(host): keep macos ipc setup diagnostics path-free`.

### Proving tests and repairs

- The live IPC mode assertion failed first with decimal 511 (`0777`) under an isolated subprocess with umask `000`. Setting `tempfile::Builder::permissions` to `0700` before creation made it pass with decimal 448 (`0700`). The pinned tempfile implementation passes that mode directly into `DirBuilder::create`; no create-then-chmod window is introduced. Existing socket confinement, network denial and cleanup assertions still pass.
- Relative Pandoc and LibreOffice overrides first reported `ToolAvailability::Missing`. Both now report their real installed versions through sandboxed built-in `describe`. The launcher anchors executable and tool override paths against the caller's cwd before entering its granted cwd, including its best-effort execution branch. It preserves the interpreter's symlink spelling. A real Docling check from its development virtualenv with `ASHIFT_DOCLING_ENGINE='bin/python -I -m ariad_docling'` additionally reproduced `Operation not permitted`: the relative `bin/python` prefix had not granted the virtualenv. Anchoring the path before deriving Docling runtime grants repairs that case. Real Python now verifies the confined cwd for both describe-style launches (`/`) and workspace launches, and the real Docling engine answers `describe`.
- The IPC creation test uses an outer real Seatbelt policy to deny creation beneath `/private/tmp`, without changing shared directory permissions or mocking filesystem errors. Before repair it returned an absolute `/private/tmp/as-ipc-...` path. The missing-IPC profile test likewise exposed the supplied temporary path. Both tests now pass with exact fixed messages: `could not create the LibreOffice IPC directory` and `could not resolve the Seatbelt IPC directory`. Assertions cover typed errors, protocol stdout, launcher stderr, public conversion-error formatting and marker absence, so refusal never executes an unconfined fallback.
- The owning CLI documentation records owner-only permissions, caller-relative override resolution and path-free IPC diagnostics. Local diff review found no outstanding issue within this rework.

### Gate results

The environment retained `CARGO_TARGET_DIR=/private/tmp/ariad-seatbelt-build`, `ASHIFT_PANDOC=/private/tmp/ariad-seatbelt-pandoc/bin/pandoc`, and `ASHIFT_SOFFICE=/private/tmp/seatbelt-libreoffice/LibreOffice.app/Contents/MacOS/soffice`. `just pandoc` completed before conversion work. The installed development Docling environment was exercised; none of the new runtime checks skipped its relevant engine leg on this Mac.

| Command | Result |
|---|---|
| `cargo nextest run -p ariad-host --features test-probe --test sandbox_macos` | PASS: 12 passed, 0 skipped. |
| `cargo nextest run -p ariad-host --lib` | PASS: 149 passed, 0 skipped. |
| `cargo clippy -p ariad-host --all-targets --features test-probe -- -D warnings` | PASS, no warnings. |
| `cargo fmt --all --check` | PASS. |
| `just lint-commits 2d9ac10..HEAD` | PASS for all three new commits. |
| `cargo nextest run -p ariad-host --features test-probe --test runner --test session` | PASS: 44 passed, 0 skipped. |
| `git diff 2d9ac10..HEAD --check` | PASS. |

Gate logs: `/tmp/seatbelt-rw1-gate-macos.log`, `/tmp/seatbelt-rw1-gate-host-lib.log`, `/tmp/seatbelt-rw1-gate-clippy.log`, `/tmp/seatbelt-rw1-gate-fmt.log`, `/tmp/seatbelt-rw1-gate-commits.log`, and `/tmp/seatbelt-rw1-runner-session.log`. Before/after evidence is in `/tmp/seatbelt-rw1-mode-{before,after}.log`, `/tmp/seatbelt-rw1-relative-{before,after}.log`, `/tmp/seatbelt-rw1-venv-before.log`, and `/tmp/seatbelt-rw1-errors-{before,after}.log`.

The previously recorded workspace linker warning, inherited test/contract failures and older commit-lint violations remain outside this rework. Their files and history were not changed; those broader failing commands were not rerun. Linux and Windows runtime validation was not performed. No unresolved question remains within the requested rework.

Status: DONE
Summary: All three review findings are repaired and committed, and every requested gate passes on this Mac. Previously recorded integration issues remain unchanged and owned elsewhere.

## Rework 2

The remaining IPC diagnostic finding is repaired. New HEAD: `27d0f497002605692ed3b9ba0d453939bddc663a`. Commit: `27d0f49` — `fix(host): sanitize macos ipc resolution failures`. Work was confined to the requested Seatbelt worktree, apart from this explicitly requested report and result file. Nothing was pushed, the implementation worktree is clean, and no task-owned background process remains.

### Repair and proving tests

The production policy repeats the IPC path in `read_write` and `private_tmp`. The shared grant helper now recognizes that IPC path before formatting any canonicalization error, including errors through read-only and executable grants. It uses the same fixed message as private-temp validation: `could not resolve the Seatbelt IPC directory`. Grant operations, successful path resolution, network rules and launcher refusal behavior are unchanged; no grant was widened and no fallback was added.

The new real launcher test first failed with the path-bearing `Seatbelt path …/missing-ipc: No such file or directory` message (nextest exit 100). A direct `Profile::new` regression test also failed first with the same disclosure. Both passed after the repair. The launcher regression puts the same missing path in both policy fields and retains the private-temp-only case as a separate test. Both assert the exact protocol and stderr messages, typed refusal, no IPC or temporary-root path in stdout, stderr or the public conversion diagnostic derived from the returned error, and absence of the unconfined marker. The direct profile test covers missing paths and symlink-loop resolution failures through every policy field.

Before/after evidence: `/tmp/seatbelt-rw2-before-launcher.log`, `/tmp/seatbelt-rw2-before-profile.log`, `/tmp/seatbelt-rw2-after-launcher.log`, and `/tmp/seatbelt-rw2-after-profile.log`.

### Gate results

Tests ran on this Mac using real `/usr/bin/sandbox-exec`, with no confinement-support override. `just pandoc` completed first. The environment used `CARGO_TARGET_DIR=/private/tmp/ariad-seatbelt-build`, `ASHIFT_PANDOC=/private/tmp/ariad-seatbelt-pandoc/bin/pandoc`, and `ASHIFT_SOFFICE=/private/tmp/seatbelt-libreoffice/LibreOffice.app/Contents/MacOS/soffice`.

| Command | Result |
|---|---|
| `cargo nextest run -p ariad-host --features test-probe --test sandbox_macos` | PASS: 13 passed, 0 skipped; success output confirms no relevant engine leg skipped. |
| `cargo nextest run -p ariad-host --lib` | PASS: 150 passed, 0 skipped. |
| `cargo nextest run -p ariad-host --features test-probe --test sandbox --test runner --test session` | PASS: 44 passed, 0 skipped; the Linux-only sandbox target contains no tests on macOS. |
| `cargo clippy -p ariad-host --all-targets --features test-probe -- -D warnings` | PASS, no warnings. |
| `cargo fmt --all --check` | PASS after correcting one line-wrap difference found by the initial check. |
| `just lint-commits 91c520f..HEAD` | PASS for the new commit. |
| `git diff 91c520f..HEAD --check` | PASS. |

Gate logs: `/tmp/seatbelt-rw2-gate-macos.log`, `/tmp/seatbelt-rw2-gate-host-lib.log`, `/tmp/seatbelt-rw2-gate-sandbox-runner-session.log`, `/tmp/seatbelt-rw2-gate-clippy.log`, `/tmp/seatbelt-rw2-gate-fmt.log`, and `/tmp/seatbelt-rw2-gate-commits.log`. The final diff review found no outstanding issue in this repair. The existing CLI documentation already specifies path-free IPC diagnostics and remains accurate. No unresolved question remains within this rework.

Status: DONE
Summary: IPC resolution failures now remain path-free across policy grants, including the production duplicated-path route. The regression tests failed before the fix, all requested gates pass, and the repair is committed without a push.
