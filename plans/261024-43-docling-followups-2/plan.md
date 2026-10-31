# Docling pack runtime follow-ups

Status: Blocked by the base branch advisory; implementation verified

## Outcome and scope

Restrict Tesseract installation-data access, explain and diagnose the Windows
Visual C++ prerequisite, make the standalone CLI's static CRT explicit, and
record the Windows packaging decision. Intel IPP, OpenCV and OpenMP decisions
are outside this change. No dependency or protocol changes are planned.

## Design notes

- Retain read access to `<prefix>/share/tessdata` for custom Tesseract installs;
  deny the surrounding `share` directory. The required-sandbox doctor test and
  real OCR conversions must both pass.
- Recognize DLL import failures in both engine result messages and crash stderr,
  only on Windows. Keep unrelated errors generic and path-free.
- Print the Windows prerequisite beside the existing Tesseract install hint;
  assert it in the same installed-pack test.
- cargo-dist 0.33.0 documents `[dist].msvc-crt-static`; set it explicitly and
  verify whether generation changes the patched release workflow.

## Execution and acceptance

| Step | Status | Evidence required |
|---|---|---|
| Inspect and implement the five requested fixes | Done | Scoped code, regression tests and owning docs |
| Verify and review | Done with a baseline failure | Focused tests, 9 real-engine tests, local CI subset, full diff review |
| Publish for review | Blocked on the overall CI gate | Rebase, commit lint and a draft PR into `dev` are authorized; ready requires green CI |
| Complete | Blocked | The advisory must be resolved separately before all CI can be green and the PR ready |

Heavy commands use the shared build lock, three Cargo jobs and three nextest
threads. Only this feature branch may be pushed. No merge, tag or repository
setting change is authorized. Validation details and limitations go in the
requested external worker report.

## Verification notes

- The two focused host tests passed, and the custom-prefix doctor regression
  passed with required isolation. All 9 real-engine tests passed, including
  Vietnamese PNG/PDF OCR to Markdown and DOCX using the pinned pack data.
- cargo-dist 0.33.0's [configuration reference](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/book/src/reference/config.md#msvc-crt-static)
  documents static CRT. The pinned, SHA-256-verified generator produced identical
  workflows with the default and with explicit `true`; existing release patches
  remain intact. Generation was compared in a scratch export with `allow-dirty`
  disabled there only, because the repository intentionally protects patched CI.
- `just ci` fails on unchanged `crypto-bigint 0.5.5`, advisory
  `RUSTSEC-2026-0387`. The base branch's
  [CI run](https://github.com/bavanchun/AriadShift/actions/runs/40068418457)
  fails identically. Dependency and advisory-policy edits are outside scope;
  all remaining CI recipes passed when run explicitly: static checks other
  than advisory denial, Clippy, WASM, 763 workspace tests, doctests, 64 benchmark
  tests, benchmark checks, four packaged-crate verifications, web typechecks,
  59 Vitest tests and the production web build. The full CI gate remains red.
- Self-review found no additional issues in the requested diff. No dependency,
  lockfile, licensing-policy or unrelated platform decision was changed.
- Windows and macOS execution is delegated to the requested CI runners. Two
  local PowerShell parser mutation checks were skipped because `pwsh` is absent.
  The external worker report records the final draft PR and remote run evidence;
  it must not claim completion while the overall CI gate fails.
