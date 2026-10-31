---
phase: 3
title: "PDF reader, Typst writer, LibreOffice engine"
status: completed
priority: P1
effort: "24h"
dependencies: []
runs_on: ASUS
branch: feat/pdf-typst-office-routes
---

# Phase 3: PDF reader, Typst writer, LibreOffice engine

## Goal

The base install handles PDF and Office documents without any pack: it inspects PDFs and turns their text layer into IR, writes PDF from IR through Typst, and converts legacy Office and ODF documents through a system LibreOffice. The docling pack (phase 4) later adds layout analysis and OCR on top.

Read first: ARCHITECTURE §1 (principle 3), §6, §7, §11; `crates/ariad-core/src/{format.rs,reader,writer,planner}`, `crates/ariad-core/data/capabilities.json`, `crates/ariad-host/src/{convert.rs,engines,commands/inspect.rs}`, `fixtures/manifest.toml`, `fixtures/gen/`.

## Requirements

### Formats

- Add registry entries with stable ids and extensions for `pptx`, `xlsx`, `odt`, `ods`, `odp`, `doc`, `xls`, `ppt`, `rtf`, `jpeg`, `webp`, `tiff` (keep `pdf` and `png`), and expose IR JSON to users as `--to json`. Content sniffing in `inspect` recognizes each (OLE2 compound files, ZIP central directory part names, magic numbers).

### PDF reader (base install)

- `ariad-core::reader::pdf`, behind a default-on cargo feature, I/O-free and WASM-safe, built on a maintained pure-Rust PDF library chosen by measurement (candidates: `lopdf`, `pdf-extract`, `pdf`); MuPDF and other AGPL code are excluded.
- Output IR: text with reading order (single and simple multi-column pages), paragraphs, headings from font-size outliers, bullet and numbered lists, repeated page headers and footers moved to `furniture`, metadata (title, author, subject, keywords, language), and a `LayoutIndex` with page geometry and block boxes.
- Limits: pages, objects, decompressed stream bytes, nesting; encrypted PDFs open with an empty user password or fail with a typed error. A page without a text layer yields a `ocr_required` warning, and `inspect` reports `needs_ocr`.
- The CLI runs it out of process as `ashift __engine pdf` (engine protocol, `pdf → ariad-ir+json`), inside the phase 2 sandbox once that lands.
- `ashift inspect` on PDF reports pages, text pages, image-only pages, `needs_ocr`, encryption, metadata and producer.
- New fuzz target `pdf_reader`.

### Typst writer

- `ariad-core::writer::typst` turns IR into Typst markup with complete escaping: headings, paragraphs, emphasis, links, lists (including task lists), tables with spans, code, quotes, figures with captions (images as virtual files), footnotes, page breaks, metadata (`document(title, author, date)` honouring `SOURCE_DATE_EPOCH`). TeX math converts to Typst math through a permissively licensed converter when one exists; otherwise it is kept as raw text with a warning.
- `ashift __engine typst` compiles the markup with the `typst` crate (a `World` with in-memory files and embedded fonts) and exports PDF. Fonts are permissively licensed and cover Vietnamese; check the output with a Vietnamese fixture.
- New fuzz target `typst_writer` (no panic, markup always compiles for arbitrary IR within limits).

### LibreOffice engine

- `ashift __engine libreoffice` finds `soffice` through `ASHIFT_SOFFICE`, `PATH`, and the standard install paths on macOS and Windows; checks the version against a stated floor; runs `--headless --convert-to` with a per-job `-env:UserInstallation` profile in the workspace whose registry sets macro security to the highest level and disables macro execution; enforces timeouts and kills the process tree.
- Routes: `doc`, `rtf`, `odt` → `docx`; `xls`, `ods` → `xlsx`; `ppt`, `odp` → `pptx`; every Office and ODF format → `pdf`.

### Planner, commands, docs

- New edges in `capabilities.json` with `metrics: null` until phase 5 measures them. Check that the planner finds, for example, `doc ─libreoffice→ docx ─pandoc→ ariad-ir ─core→ md` and `md ─core→ ariad-ir ─typst→ pdf`.
- `engines` and `doctor` report the PDF, Typst and LibreOffice engines with tool status; MCP tools pick the routes up through the host.
- Generated, distributable fixtures: digital PDFs in English and Vietnamese with truth, and DOC, RTF, ODT, XLS, ODS, PPT, ODP samples produced by the generator or by LibreOffice from generated sources. Record each in `fixtures/manifest.toml`.
- Golden tests: PDF → MD on the generated PDFs; MD → PDF verified by reading the produced PDF back (page count, extracted text); LibreOffice routes skip with a message when `soffice` is absent and run on Linux CI with LibreOffice installed.
- `docs/cli.md`, ARCHITECTURE §4, §6.4 (format matrix: base PDF through the Rust reader, PDFium only inside the docling pack and bench), §7 and the Decision Log are updated.

## Acceptance criteria

- `ashift convert report.pdf --to md`, `ashift convert notes.md --to pdf`, `ashift convert legacy.doc --to md` and `ashift convert slides.pptx --to pdf` work on a machine with LibreOffice installed.
- `ashift inspect scan.pdf` reports `needs_ocr: true` for an image-only PDF.
- `cargo check -p ariad-core --target wasm32-unknown-unknown` still passes with the PDF reader and the Typst writer.
- The release targets still build (`typst` adds size; record the binary size before and after in the pull request).

## Risks

| Risk | Response |
|---|---|
| Pure-Rust text extraction mangles Vietnamese diacritics | Measure with the Vietnamese fixture; prefer the library with correct ToUnicode handling; NFC-normalize |
| Typst raises the binary size and build time sharply | Gate behind a default feature; report the numbers |
| LibreOffice output varies by version | Assert structure, not bytes |

## Design notes

### Formats

- New `Format` variants with ids equal to their serde names: `pptx`, `xlsx`, `odt`, `ods`, `odp`, `doc`, `xls`, `ppt`, `rtf`, `jpeg` (extensions `jpg`, `jpeg`), `webp`, `tiff` (extensions `tif`, `tiff`). `Format::ALL` lists every variant; `Format::from_user_name` accepts an id or extension, and `json` names IR JSON (`ariad-ir+json`). `pandoc+json` stays internal.
- `format::classify(prefix, container_entries, extension)` keeps its shape. `container_entries` carries ZIP central-directory names or, for OLE2 compound files, the CFB directory entry names. Rules: EPUB and ODF by a stored `mimetype` first entry and payload; DOCX, XLSX, PPTX by `[Content_Types].xml` plus `word/document.xml`, `xl/workbook.xml` or `ppt/presentation.xml`; OLE2 magic plus a `WordDocument`, `Workbook`/`Book` or `PowerPoint Document` stream; `{\rtf`; PNG, JPEG, WebP and TIFF magic numbers.
- `ariad-host::archive::read_cfb_entry_names` reads the CFB directory with a bounded, panic-free parser (header, DIFAT in the header, FAT chain walk, at most 64 directory sectors). No new dependency.

### Route executor

- `ariad-host::convert` executes any planner route of one or more steps instead of exactly reader + writer. State between steps is either an in-memory IR document or a file. A step whose engine is `ariad-core` runs in process (Markdown and HTML readers and writers, IR JSON read and write). Every other step runs `ashift __engine <engine>` through `runner::run` with the same request shape: the input file (or the IR serialized to `in/document.ir.json`) is copied into a fresh workspace `in/` with archive preflight, and an `ariad-ir+json` artifact is read back through the bounded `ir_io` reader. Pandoc DOCX and EPUB outputs keep their `package_meta` stamp.
- `engines::REGISTRY` declares each out-of-process engine once: id, licence, routes, install hint. `executor_edges()`, `engines`, `doctor`, availability and the capabilities coverage test all derive from it, so a new engine adds one entry. Availability runs `describe` only for the engines a plan needs.
- The executor uses the central `Format` registry directly; `Format::from_user_name` and `user_name` own CLI aliases, including `json`, so surfaces do not duplicate the registry.

### PDF reader

- Library measured on the PDF fixtures and on 3000 randomly mutated copies of five of them: `lopdf` 0.46.0 decoded Vietnamese correctly and panicked 0 times; `pdf-extract` 0.12.1 decoded the same text but panicked on 841 of 3000 inputs (it would fail the fuzz target); `pdf` 0.10.0 has no text extraction; `pdf_oxide` 0.3.78 pulls image, layout and Office stacks into the core. Chosen: `lopdf` 0.46.0 (MIT) with `default-features = false` (no rayon, so it runs in a browser), `wasm_js` on wasm32 only, and our own content-stream interpreter.
- `ariad_core::reader::pdf::read(bytes, limits) -> Result<PdfReadOutput, PdfError>` behind the default-on `pdf` feature. The interpreter tracks graphics and text state (`cm`, `q`/`Q`, `BT`/`ET`, `Tf`, `Tm`, `Td`, `TD`, `T*`, `TL`, `Tc`, `Tw`, `Tz`, `Ts`, `Tj`, `TJ`, `'`, `"`, `Do` for form XObjects with a depth cap) and decodes glyphs through ToUnicode CMaps, font encodings with `Differences`, and font widths. Layout: glyphs to lines to blocks, single and simple two-column reading order, headings from font-size outliers, bullet and numbered lists, repeated top and bottom lines moved to `furniture`, NFC text, `LayoutIndex` with page geometry and block boxes, and metadata from the Info dictionary.
- Limits: `max_pages`, a fixed object cap, `max_decompressed_bytes` per stream and in total, content operator cap, form XObject depth, `max_blocks`. Encrypted PDFs open with the empty user password or fail with `PdfError::Encrypted`. Pages without text yield `ocr_required`. Statistics (pages, text pages, image-only pages, encryption, producer) travel in `Document.provenance["pdf"]`, so `inspect` needs no protocol change.
- `ashift __engine pdf` serves `pdf → ariad-ir+json`. Fuzz target `pdf_reader`.

### Typst writer and engine

- `ariad_core::writer::typst::write(&Document) -> TypstOutput { markup, files, warnings }` is pure string generation (WASM-safe, no `typst` dependency in core). Every text run is emitted through string literals or escaped markup, so IR content can never become Typst code. TeX math has no maintained, permissive TeX-to-Typst converter (`mitex` needs its Typst package scope and is unreleased since 2024), so math stays raw text with a `math_not_converted` warning.
- `ariad-host` gains the `typst` feature (default on): `typst` 0.15.1 and `typst-pdf` 0.15.1, a `World` with in-memory sources and images, embedded fonts, `SOURCE_DATE_EPOCH` for the document date. Fonts (OFL-1.1, Vietnamese coverage verified with fontTools): Libertinus Serif Regular, Bold, Italic, Bold Italic and Libertinus Math 7.051; Source Code Pro Regular and Bold 2.042. They live in `crates/ariad-host/fonts/` with their licence texts. `ashift __engine typst` serves `ariad-ir+json → pdf`. Fuzz target `typst_writer` compiles the markup.
- `typst-library` depends on `citationberg`, which pins `quick-xml` 0.38 (RUSTSEC-2026-0194 and -0195). The generated markup never loads a bibliography or CSL file, so the vulnerable parser is unreachable; `deny.toml` ignores the two advisories with that reason. `roman-numerals-rs` (`0BSD OR CC0-1.0`) gets a per-crate `0BSD` exception.

### LibreOffice engine

- `ashift __engine libreoffice` finds `soffice` through `ASHIFT_SOFFICE`, then `PATH` (`soffice`, `libreoffice`), then standard install paths on macOS and Windows. Version floor 24.2 (the oldest line in current LTS distributions; older lines are out of security support), read from `soffice --version`.
- Each job gets a profile under the workspace `tmp/` (`-env:UserInstallation=file:///…`) seeded with `registrymodifications.xcu`: macro security level 3, `DisableMacrosExecution`, no update checks. Command: `soffice --headless --invisible --nologo --norestore --nolockcheck --nodefault -env:UserInstallation=… --convert-to <ext>:<filter> --outdir <out> <input>`. `HOME`, `TMPDIR`, `TMP`, `TEMP` point into the workspace. LibreOffice inherits the runner process group or Job Object, so cancellation and timeouts reach its whole process tree. The runner's environment allow-list gains `ASHIFT_SOFFICE`.
- Routes: `doc`, `rtf`, `odt` → `docx`; `xls`, `ods` → `xlsx`; `ppt`, `odp` → `pptx`; `doc`, `rtf`, `odt`, `docx`, `xls`, `ods`, `xlsx`, `ppt`, `odp`, `pptx` → `pdf`.

### Planner, commands, fixtures

- Edges with `metrics: null`: `pdf → ariad-ir+json` (engine `pdf`), `ariad-ir+json → pdf` (engine `typst`), the LibreOffice routes above, and `ariad-ir+json` as a user target (`--to json`) through the existing reader edges.
- `inspect` on PDF runs the PDF engine and reports `page_count` plus a `pdf` object (`pages`, `text_pages`, `image_only_pages`, `needs_ocr`, `encrypted`, `producer`) and metadata.
- `engines` lists `pdf`, `typst` and `libreoffice` from the registry; `doctor` adds their describe round trips (LibreOffice optional, so its absence never fails `doctor`).
- Fixtures: the existing generated `vi-garden-route.pdf` and `en-map-room.pdf` get text truth; ODT, ODS and ODP are generated with the standard library; DOC, RTF, XLS, PPT, DOCX-free XLSX and PPTX samples are produced by LibreOffice from them during `just fixtures`; an image-only PDF exercises `needs_ocr`.
- CI: the Linux verify job installs the distribution `libreoffice-writer`, `libreoffice-calc` and `libreoffice-impress` packages; macOS and Windows tests skip LibreOffice routes with a message. The fuzz job gains the two targets.
- Binary size (dist profile, x86_64 Linux) before the change: 18,067,576 bytes.
- `pdfium-render` leaves the workspace dependency table; ARCHITECTURE §2.3, §4, §6.4, §7, §11 and the Decision Log follow the code.

### Takeover validation

- Registry versions rechecked on 2026-10-22: lopdf 0.46.0 (MIT), typst and typst-pdf 0.15.1 (Apache-2.0) are the latest stable releases. Font release sources and SHA-256 digests are recorded in `crates/ariad-host/fonts/SOURCES.txt`.
- Rebased over current `origin/dev`; both Cargo lockfiles are regenerated with Cargo from the updated manifests.
- The interrupted Typst fuzz stub is replaced by bounded IR JSON/Markdown generation and real PDF compilation. PDF/Typst fuzz targets join the stable PR workflow; Linux installs distribution-candidate versions of LibreOffice Writer, Calc, Impress and qpdf explicitly.
- Preliminary read-only PDF review found unmetered font tables and loader stream limits that needed tightening. Retained font allocations now consume the amplification budget; loader caps include the input amplification bound and wasm32 headroom, strict loading propagates errors, and object/xref streams consume the aggregate budget. Regressions cover shared-width font dictionaries, per-stream/total object-stream bombs, decomposed ToUnicode text and all-unmapped pages.
- The PDF reader preserves the plan's empty-password and blank-page OCR semantics. Invisible formatting characters follow the existing core text policy; escaping protects writer output. Native PDF runs out of process, and browser callers supply their own limits. The separate host RSS watchdog remains owned by the isolation work.
- LibreOffice inherits the runner's group/job, rather than creating an escaping nested group. A real adapter lifecycle test verifies both cancellation and timeout stop its descendants.
- The Typst/LibreOffice preliminary reviewer hit the external Claude session limit before returning findings; a full cross-model PR review is still required after green CI.
- Seed-corpus fuzzing exposed one-ULP geometry drift in serde_json's default float parser. Enable its `float_roundtrip` feature for lossless IR JSON round trips and retain a Type 3 font-transform regression. Heavy fuzz targets use development builds to keep compile time bounded; all coverage and debug assertions remain enabled.
- A local full debug build plus packaging used 38 GiB of artifacts. CI disables dev/test debug information and incremental compilation; release profiles retain their existing settings.
- A crafted page tree with three `i64::MAX` `/Count` values reproduced an upstream iterator size-hint overflow. Page collection now grows only from observed page ids, without consulting that hint. A deterministic regression and an original CC0 fuzz seed preserve the case.

### Review and close-out

- Rebased over the merged engine pack work: the engine registry rows, `doctor` checks and `engines` output keep the pack rows and add the PDF, Typst and LibreOffice rows after Pandoc. Pack resolution is not wired into the route executor; the docling phase adds its pack-backed engine to the registry.
- Independent read-only review by the other model found no high findings and four medium ones, all verified in source and fixed with regressions: the Markdown writer's list numbering overflowed on an IR JSON start near `u64::MAX` (now saturates); the runner's cleared environment hid `ProgramFiles`, `ProgramW6432` and `ProgramFiles(x86)` from LibreOffice discovery on Windows (now forwarded); `Symbol` and `ZapfDingbats` fonts without an encoding decoded as Latin text (built-in tables from the Unicode Consortium mapping files); digit-labelled PDF lists that restart or skip numbers were merged into one sequential list (now split, keeping each start; letter and Roman labels still merge because their values are ambiguous).
- `cargo package --workspace` verifies the `ariad-cli` tarball against the packaged `ariad-host` through a temporary registry. Cargo treats registry crates as immutable, so a reused target directory that holds an older build of the same version fails that step locally; a fresh target directory passes, as CI does.

### Host isolation integration

- Recovered the Mac bundle at `caba2b8`, whose merge base is `7e2f389`, and reviewed/applied the interrupted integration patch. All three adapters explicitly advertise `supports_session: false`; pooled conversions fall back to one-shot leases. The host watchdog enforces `max_memory_mb` even though these adapters do not enforce RSS themselves.
- Route execution retains each lease until IR is read or a writer artifact is copied through the workspace's captured directory handle into a separate host workspace. Package stamping and later route steps use that private snapshot.
- Actual sandboxed LibreOffice conversion exposed a hard-coded `/tmp/OSL_PIPE_*` socket that ignores `TMPDIR`. A syscall trace reproduced `bind(...)=EACCES`; the same generated input converted outside the sandbox. Granting access to the host `/tmp` would break confinement. The launcher instead creates user/mount namespaces and mounts a workspace-backed private `/tmp`, preserving the job's original paths with a bind mount. It maps the caller to a non-root namespace identity so mount capabilities are lost at exec. Namespace failure stops launch; Linux deployments must allow these namespace operations when using sandboxed LibreOffice.
- The engine path declaration covers PDF, Typst and LibreOffice; LibreOffice's discovered installation roots allow sibling helper execution. Describe calls get temporary workspaces for the same private-temp setup. No new dependency version was added; existing `nix` features provide the safe namespace and mount APIs.
- Focused integration verification passed all 24 tests: PDF/Typst/Office launcher execution, session fallback, Typst watchdog termination, batch canonical extensions, Office conversions, cancellation/timeout, private-temp filesystem/network isolation, temporary executable aliases, read-only installation mounts, and mount capability removal at exec. The lifecycle heartbeat uses a custom invocation because it intentionally observes a path outside the job; separate built-in tests exercise the real sandbox.

### Integrated verification

- Rebased again onto `origin/dev` at `3a2d5ea`, including the merged web foundation. Preserved its shared inspection API and browser surfaces; native PDF targets now appear in the browser's CLI hand-off list. Native, JavaScript and browser expectations follow the same capability registry.
- Local CI-equivalent checks passed: formatting, spelling, workflow lint (zizmor offline), dependency and license audits, generated license inventory, JavaScript lint/licenses, Python checks, release/WinGet guards, workspace Clippy, WASM checks, 756 native tests (one release-only timing test skipped), documentation tests, 64 benchmark tests and benchmark validation. Two PowerShell mutation guards require `pwsh`, which is absent locally and supplied by CI.
- The reused package verification cache held an older `ariad-core 0.1.0` without the web inspection API. Re-running `just package-check` with `CARGO_TARGET_DIR=/tmp/p03-package-verify-261023` rebuilt and verified every workspace tarball successfully. No dependency or source workaround was needed.
- `just web-ci` passed WASM generation, TypeScript checks, all 49 JavaScript tests and the production website build. The focused browser hand-off tests passed all six Chromium/Firefox cases, including PDF CLI routes and route-less images. The three WebKit cases could not launch because this host lacks its libicu74/libxml2/libflite dependencies; CI installs the browser dependencies.
- Linux sandboxed LibreOffice requires user and mount namespaces; failures remain fail-closed. Read-only temporary installation bind mounts and non-root namespace identity prevent the private temporary directory from granting host writes or mount capabilities after exec.

### CI environment follow-up

- On `1174512`, macOS and Windows verification, web (54 browser tests), regular fuzzing, CodeQL and all five release targets passed. Ubuntu rejected namespace creation with `Permission denied`, so the LibreOffice contract tests correctly stopped rather than running without confinement.
- The disposable native Linux runner now enables its unprivileged namespace prerequisite and checks user/mount namespace creation before installing tools. It uses the same primary HTTPS Ubuntu archive as the web job: the failed run spent 19m50s fetching 117 MB through the Azure mirror. Package candidates remain pinned at installation and signatures remain enforced.
- The current GNU x86_64 `dist` binary measures 80,416,152 bytes (76.69 MiB), against the historical pre-feature baseline of 18,067,576 bytes. Its four acceptance conversions, image-only OCR inspection, `doctor` and `engines` checks passed on the ASUS.

## Review follow-up

- Launcher failures retain their fixed diagnostic through engine availability. For a discovered LibreOffice installation, doctor reports the namespace prerequisite, engines distinguishes an installed engine that cannot start, and conversion returns `sandbox_unavailable` (exit 5) without replacing an initially selected blocked route. The sandbox policy remains fail-closed.
- Debug-only namespace denial exercises real launcher requests through the probe and CLI. The tests cover describe and conversion, both sandbox modes when basic required isolation is supported, and the explicit off-mode bypass.
- LibreOffice profiles disable Writer and Calc link updates using the supported schema values. The CLI reference records the Ubuntu remedy, container seccomp prerequisite, and explicit isolation trade-off; the architecture describes the remaining non-Linux network gap.
- [Release size measurements](../../docs/releasing.md#base-pdf-build-size) now include binaries and compressed archives for every release target, artifact download totals, the historical baseline caveat, and the absence of a size budget. Every native CI Verify job checks the slim CLI build.
- Mac verification passed the slim check, workspace Clippy, WASM check, 744 native tests (one release-only skip), doc-tests, 61 benchmark tests (three skips), capabilities validation, all four crate package checks, JavaScript license/lint checks, and web TypeScript checks. The final focused CLI and sandbox suites passed all 47 tests; all six route golden tests passed too.
- Full local `just ci` attempts reached web tests, where fork workers timed out during startup on the mounted worktree volume. An isolated one-worker retry had the same startup limitation. The production web check and Linux namespace regressions remain delivery checks in [PR #34 CI](https://github.com/bavanchun/AriadShift/pull/34/checks). No test timeout or sandbox requirement was weakened.

- The first Ubuntu review-fix run exposed a missing CLI error label: the launcher cause and exit 5 were already preserved, but stderr omitted the diagnostic code. The command formatter now emits `error[sandbox_unavailable]`; a platform-independent invalid-mode assertion covers that label. The focused 47-test CLI/sandbox run, all 149 CLI tests, CLI Clippy, and slim check passed after the formatter fix.

### Optional engine discovery

- Private `/tmp` is requested only when LibreOffice installation paths exist. An absent LibreOffice stays optional, keeps the install hint, allows DOCX → PDF through Pandoc and Typst, and leaves LibreOffice-only routes with the missing-engine error. An installed engine blocked by the launcher still fails closed without changing the planned route.
- Linux CLI regressions now use an executable stub for the installed case and cover the converse missing case under the same namespace-denial hook. A subprocess-isolated unit test pins the missing-installation sandbox declaration; the host launcher test also supplies an explicit executable. No process-global environment mutation is needed.
- Doctor chooses namespace advice only for namespace diagnostics and points other launcher failures to `ASHIFT_SANDBOX`; the Pandoc describe check keeps the cause too. Human plan output reports `startup_failure`, while all added plan fields remain excluded from JSON. A profile-writer unit test reads `registrymodifications.xcu` and checks both never-update items.
- Rebased onto `origin/dev` at `dc964ed`. Full Mac `just ci` passed: static checks, workspace Clippy, WASM, 747 native tests (one release-only skip), doc-tests, benchmark checks, all four crate package checks, TypeScript, 59 Web unit tests and the production Web build. Two PowerShell mutation guards still require CI's `pwsh`; no check was weakened. The focused 47-test CLI/sandbox suite and all three new unit tests passed before the full run. The slim CLI check passed separately.

- The first Ubuntu run found that the new PPTX plan assertion inherited the older snapshot capability fixture, which has no Office/PDF routes. The namespace tests now clear that fixture override and exercise the real embedded registry. A test-probe build reproduces exit 3 with the snapshot fixture and a successful LibreOffice route preview with the embedded registry; the missing-LibreOffice namespace regression already passed in that CI run. A second full Mac `just ci` passed after the test correction, including all 747 native and 59 Web tests plus production build.

### Docling and cloud integration

The follow-up preserves both merged implementations on `origin/dev` (`be5e305`) and the built-in PDF/Typst/Office routes. Acceptance requires content-aware PDF routing, explicit reader selection, preserved launcher failures through Docling and the worker helper, one PDF inspection path, and green native, Server integration and Pack jobs.

- [x] Rebase and resolve overlapping formats, fixtures, schemas, workflows and documentation.
- [x] Connect Docling extraction and mapping to the generic route executor and restore cloud helper contracts.
- [x] Prefer the built-in reader for digital PDFs; choose Docling for OCR or an explicit reader request, with regressions.
- [x] Verify Office worker environment and namespace failures; document runtime requirements.
- [x] Run supported local gates and prepare remote Server/Pack/native verification; record final CI results and cleanup in the task report.

- The shared executor now resolves Docling pack commands and options, maps bounded Docling JSON, and restores the cloud helper's serialization and explicit-limit API. Availability and worker diagnostics retain launcher causes. A bounded native PDF read supplies automatic OCR routing and is reused for digital text; `--engine` pins the input reader for conversion and plan previews. Inspection has one PDF branch.
- Format-only cloud plans reserve Docling workers for PDF when configured, so scans do not land on core-only workers. The helper forwards `ASHIFT_SOFFICE`; Linux regressions cover namespace denial under both worker modes and preserve the cause in permanent job diagnostics. Runtime docs cover Landlock, seccomp/AppArmor and installed-Office namespace prerequisites.
- Local gates passed with four build jobs and two test threads: 866 native tests (10 configured skips), doc-tests, 61 benchmark tests (three skips), capability/schema checks, workspace Clippy, WASM, slim CLI, static checks and all four package verifications. The focused route suite passed 20 tests and the server/helper suite passed 48. Both changed Python files pass Ruff; two pack-builder tests pass, and all 11 Office fixture outputs reproduce byte for byte.
- Web WASM generation and all TypeScript checks passed. `just ci` reached Vitest: 50 tests passed, while four JSX workers timed out before startup on the mounted volume. Web production/browser checks and the Linux service/confinement checks remain CI delivery requirements. Pack checks explicitly select Docling for digital PDF smoke fixtures; their Rust jobs use the same bounded artifact configuration as native CI. Final remote evidence is recorded in the task report after the push.

### Cloud worker hardening integration

The continuation rebases onto `origin/dev` at `fd4adf6`. It preserves the credential boundary, root supervisor, generic public failures and temporary-directory ownership from cloud hardening, together with content-aware PDF routing and fail-closed Office namespaces. No API schema, credential grant or sandbox restriction changes.

- [x] Audit the previous head's CI and resolve all rebase conflicts while retaining both implementations.
- [x] Correct the namespace test to inspect the primary plan score; a missing optional Docling alternative is valid.
- [x] Run focused routing, worker, helper and CLI checks with one Cargo build job.
- [x] Run `just ci` and `just test-server` sequentially and review the integrated behavior.
- [x] Prepare the leased push and remote verification; record final job results in the task report.

The previous `338b6c4` run passed macOS/Windows verification, Server integration, Web, regular/ASan fuzzing, release rehearsal and all four Docling packs. Ubuntu stopped at a test assertion that searched the whole human plan for `engine missing`: the primary LibreOffice route correctly reported `startup_failure`, while the new Docling alternative correctly reported a missing optional engine. The repaired assertion checks the primary score specifically. Server failures keep `sandbox_unavailable` and a generic public message; the helper preserves the typed namespace cause until server classification.

- Local verification uses one Cargo build job and two test threads. Static checks, workspace Clippy, WASM, all 873 native tests (10 configured skips), doc-tests, 61 benchmark tests (three skips), capabilities validation and all four crate package verifications passed. The initial focused SIGINT startup failure did not recur in the full native suite; neither its timeout nor its behavior changed.
- Web package typechecks passed. Application `next typegen` was stopped after more than ten minutes waiting on mounted-volume filesystem reads. The remaining Web checks are required in remote CI; the local `just ci` run is not recorded as fully passing.
- `just test-server` passed all 88 tests against fresh PostgreSQL/pgmq and S3 containers. Its first run exposed an older PDF API assumption from cloud hardening: core-only deployments now have the native reader. The corrected regression checks the chosen input engine and persisted queue for both deployments, keeps core at its default budget, and retains Docling's rejection below 2,048 MiB before admitting it at that budget.
- Conflict review preserves the credential-isolation code and worker Dockerfiles unchanged from `dev`, together with generic public errors, runtime namespace requirements and helper error typing. Final Linux namespace, container/procfs and Pack evidence belongs to the post-push task report.

### Native surface integration

Rebased onto `dev` at `60e6a4f`, including the desktop, cloud hardening and current
Docling follow-ups. The five rebased commits each pass `cargo check` for
`ariad-host`, `ariad-cli`, `ariad-server` and `ariad-desktop` with `--tests`.
The checks use the shared build lock, three Cargo jobs and three test threads.

- CLI and Desktop share the built-in Pandoc/PDF/Typst/LibreOffice dispatcher.
  Unknown engine names return exit 2 with no protocol output. Desktop and server
  explicitly enable the base PDF features; Desktop uses the common format registry.
- Cloud sandbox failures retain `sandbox_unavailable` with a generic public
  message and can retry on another worker. Helper isolation, credential protection,
  current Tesseract `share/tessdata` grants and Windows DLL diagnostics are preserved.
- The dependency policy keeps the existing JWT advisory entry and Decision Log
  row, the crypto-bigint dependency guard and both quick-xml exceptions. The Rust
  and JavaScript notice inventories use the current `just licenses` generators.
- Full local `just ci` passed: static checks, Clippy, WASM, 919 native tests
  (12 opt-in skips), doctests, 64 benchmark tests, four package verifications,
  67 Web tests, production Web build and the Desktop checks. The local PowerShell
  parser mutations still require Windows CI because this host has no `pwsh`.
- `just test-server` passed all 99 tests against fresh PostgreSQL/pgmq and S3
  containers, including credential boundaries and the restricted database roles.
- A real FUSE-mounted debug AppImage passed PDF/Typst/LibreOffice discovery and
  PPTX to PDF conversion with required isolation. The native PDF reader parsed the
  output. The launcher preserves its bundle through read-only bind mounts, so no
  AppImage-specific launcher change is needed. Its process and mount were removed.

The final feature head must pass the three native platforms, Desktop (including
the macOS updater guard), Engine conformance and all Docling pack targets before
coordinator review. The external worker report records that head and its run URLs.
