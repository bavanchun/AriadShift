# Docling pack follow-ups

Status: Implementation completed. Final verification and PR readiness are tracked in the pull request and coordinator handoff report.

Outcome: Close the eight accepted low review findings on the merged Docling engine and pack, publish a focused pull request into `dev`, and verify its checks before marking it ready.

Constraints: Keep existing dependency pins and conversion contracts. Serialize heavy commands with the shared build lock and three build/test workers. Do not merge or change release preparation or the pending PDF/Typst/Office work.

Non-goals: Dependency upgrades, signing/publishing packs, additional conversion routes.

## Execution

1. Complete runtime C-library attribution, prune unused GUI/test runtime content, derive manifest licensing from the gate, and gate Linux reproducibility.
2. Resolve sandbox development source paths at runtime, grant the Tesseract installation prefix with regression coverage, refresh stale wording, and enable the mapper in ASan fuzzing.
3. Record licensing exceptions and build requirements in owning docs, run focused checks and `just ci`, review the full diff, commit, rebase and push.
4. Create a draft PR into `dev`, watch all three OSes and four pack targets, repair failures, mark ready, and write the requested handoff report.

Acceptance: All eight findings have source/test evidence; runtime notices include upstream release URLs and licence texts; Linux CI compares independently rebuilt component hashes; the manifest reflects actual accepted licences; focused checks and local CI pass; remote checks pass before PR readiness.

Risk/rollback: Runtime pruning and sandbox grants can affect engine startup. Real-engine and native pack conversion checks cover this boundary; revert the focused change if they regress.

## Implementation evidence

- Runtime C-library notices include matching upstream release URLs and digest-checked texts. Windows uses its patched CPython libffi revision. The pinned Linux runtime was pruned and its SSL, XML, decimal, SQLite, compression and ctypes extensions still loaded successfully.
- The manifest licence combines accepted distribution expressions and component licences; the target build records carry each target expression into manifest assembly. Linux CI rebuilds in another directory and compares all three archive hashes plus the manifest.
- Editable source access follows the override environment; the sandboxed doctor regression reads Tesseract prefix data. Install guidance, licensing exceptions and the nightly mapper target are updated.
- Rebased onto `be5e305`. `just ci` passed: 750 Rust tests, 64 benchmark tests, 59 web tests, Clippy, WASM, package checks and production build. Python tests passed (59), with affected licensing/pruning tests rerun (28). The focused inspect tests (11) and custom-prefix doctor regression (1) passed.
- Self-review found no remaining implementation issue. Original upstream licence text whitespace is preserved deliberately. Publication is gated on successful remote checks before `gh pr ready`.

## Native Windows verification

The first Windows pack smoke test exposed a case-insensitive path collision: scanning `runtime/lib` also scanned the stdlib `runtime/Lib` and removed `threading.py`. Native-library scans now exclude that directory on Windows. The pruning test models the alias on Unix, failed against the original implementation, and passed after the fix. All 59 Python tests and the complete local `just ci` gate passed again before publishing the repair.

## Runtime inventory and sandbox revision

Outcome: Remove unused Berkeley DB and LibTomMath, fail builds for unattributed native runtime files, restrict Tesseract reads to installation data directories, deduplicate the merged licence expression and refresh the CLI instructions. Keep PR #43 and the existing branch.

Constraints: Use the shared build lock and three build/test workers. Keep Windows redistributable removal and its prerequisite documentation in one isolated commit, following the coordinator's decision to use the system runtime. No dependency upgrades or unrelated conversion changes.

Acceptance: The structural check passes all four pruned runtimes, and archives reject Microsoft redistributable DLLs, including copies from wheels. Tesseract tests prove data access without prefix-wide access and force the sandbox. Focused tests, local CI and all remote Pack/OS jobs must pass before final delivery.

Status: Implementation complete; final remote verification remains gated on CI. All four pinned archives were fetched and hash-verified, and all four pruned runtimes now pass the structural inventory.

Verification: `just docling-test` passed all 61 Python tests and Ruff; final affected pack/licensing checks passed 30 tests. Three sandbox unit tests and the custom-prefix doctor test passed, with the latter requiring the sandbox and asserting that private prefix data stays unreadable. `just ci` passed 751 Rust tests, 64 benchmark tests and 59 web tests, plus static checks, Clippy, WASM, package checks and production build. Full-diff self-review and the read-only simplification review found no remaining mechanical issue. Fetched `origin` and rebased onto unchanged `origin/dev` (`be5e305`); commit lint passed.

Windows evidence: The release workflow uses dist's default static MSVC CRT; PE inspection of the Windows artifact from run `40050574512` found only Windows system imports, with no VC runtime DLL dependency. The Pack job instead uses plain Cargo with no static-CRT override. The pinned Python DLL imports `VCRUNTIME140.dll`. The coordinator chose to remove the bundled redistributables and document a Windows Docling prerequisite; no licensing exception is added.

## Windows runtime prerequisite

The build removes Microsoft runtime DLLs both before and after installing wheels, and the archive writer rejects remaining `vcruntime*` or `msvcp*` DLLs. The original Windows pack contained three hashed MSVCP copies inside NumPy, pandas and Shapely wheels in addition to the two CPython DLLs. Their PE import names are shortened to system DLL names before removal, without relocating image data. An independent `llvm-readobj` check over all 330 remaining native files confirmed there are no hashed Microsoft import names. Normal/delay-import rewriting and archive refusal have regression tests.

Docling's path-free describe failure names the x64 Visual C++ Redistributable prerequisite for Windows missing-DLL loader errors, so doctor can explain the remedy. The distributed release host uses static CRT; only the Docling prerequisite is documented. The standalone host's install requirements and project licensing policy are unchanged.

Final local verification: `just ci` passed 752 Rust tests, 64 benchmark tests and 59 web tests, including static checks, Clippy, WASM, package checks and production build. The complete Docling suite passed Ruff and 66 tests; final archive/import/record checks passed all 10 focused pack tests. Two Docling host unit tests passed. An audit of the real Windows wheel tree verified all 282 remaining native-file RECORD hashes after removal and import retargeting. Self-review found no unresolved implementation issue. Final remote checks and PR readiness are recorded in the coordinator handoff report.
