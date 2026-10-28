# AGENTS.md

Rules for AI agents working in this repository. Design authority is [ARCHITECTURE.md](ARCHITECTURE.md); read the section you are touching before changing structure, stack or boundaries.

## Current state

Roadmap 1a (`v0.1.0`) is shipped. `ashift` converts across all 12 directional routes between Markdown (`md`/`markdown`), HTML (`html`/`htm`), DOCX (`docx`), and EPUB (`epub`) through `ariad-ir+json` (native Markdown and HTML readers/writers in `ariad-core`; out-of-process Pandoc `>= 3.12, < 4` for DOCX and EPUB). The base build also reads PDF text, writes PDF through Typst, exposes IR JSON (`--to json`), and detects system LibreOffice >=24.2 for Office/ODF routes. Docling adds OCR and layout-aware PDF, Office and image extraction through its optional engine pack ([docs/docling.md](docs/docling.md)). PDF, Typst and LibreOffice use the same out-of-process runner as Pandoc. See [docs/cli.md](docs/cli.md) for the CLI commands (`convert`, `inspect`, `plan`, `engines`, `doctor`), [docs/mcp.md](docs/mcp.md) for the `ashift mcp` server, [docs/releasing.md](docs/releasing.md) for the release procedure, and [docs/web.md](docs/web.md) for the web app in `apps/web`, which converts Markdown, HTML, DOCX, EPUB, digital PDF and IR JSON in the browser through `ariad-wasm`, the separate official Pandoc WASM reactor and lazy `ariad-typst` module. See [docs/desktop.md](docs/desktop.md) for the Tauri app in `apps/desktop`, backed directly by `ariad-host`. Run `just pandoc` before working on conversion tasks and `just ci` as the local quality gate.

## Architecture changes

- Do not add a crate, app, service or dependency that contradicts ARCHITECTURE.md. If a change is warranted, update ARCHITECTURE.md, including its "Decision Log" table, in the same change.
- Conversion logic belongs in `ariad-core`, `ariad-host`, or the shared in-memory `ariad-typst` compiler only. Surfaces (CLI, desktop, web, server) never carry their own conversion code.
- Third-party engines run out of process through the engine protocol. Never link them into the host.
- The user-facing CLI binary is `ashift`; Rust crate names retain the `ariad-*` prefix.

## Versions

- Use the latest LTS where a channel exists, otherwise the latest stable release. Never use alpha, beta or RC builds. A date-pinned Rust nightly toolchain is the sole exception, used only by the scheduled sanitizer fuzz job.
- Verify a version against its registry (npm, crates.io, PyPI, GitHub Releases, endoflife.date) before pinning it. Do not pin from memory.
- Pin exact versions through lockfiles and commit the lockfile with the manifest.
- Package managers: `pnpm` for JS, `uv` for Python, `cargo` for Rust. Do not use npm, yarn or pip.
- Root tasks go into `just` recipes. Do not add Turborepo or Nx.

## Licensing

- Distributed artifacts must not contain AGPL or non-OSI code. This rules out PyMuPDF, `pdf2docx` and MinerU.
- MPL-2.0 Rust dependencies are allowed only unmodified and through per-crate cargo-deny exceptions. The Docling Python pack allows unmodified `certifi` and `tqdm` through per-distribution exceptions in `engines/docling/pack/licenses.py`.
- The Docling licence gate requires known, consistent single-value declarations and checks every SPDX alternative. Every native file requires matching RECORD SHA-256/size and one licensed owner with a nonempty notice file; default discovery accepts any nonempty file under dist-info/licenses without validating content. Only ABI-tagged or Mach-O bundle extensions and explicitly listed native components may ship; unsafe links, .zip/.egg/.whl suffixes and matching .pth references fail. ZIP local-header magic also fails in non-native files outside known NPZ/DOCX/PPTX/XLSX data formats or exact distribution/version/path/SHA-256 model archive reviews in `engines/docling/pack/reviewed-data-archives.toml`. Reviewed models require one matching RECORD owner; archive contents are not parsed at runtime and other archive magic is not inspected. RECORD refresh rewrites every DLL/PYD digest and the third-party notice record, so accidental Windows post-install changes can be masked; that target remains deferred. GNU runtime exceptions are bound to reviewed ELF shared-object bytes.
- Named GNU runtime exceptions require complete GPL-3.0, GCC Runtime Library Exception 3.1 and LGPL-2.1 texts, the GCC archive index, evidenced ABI baselines and release-tag wheel-build provenance, plus a corresponding-source offer valid for at least three years after last distribution. An exact GCC release is not mandatory; ABI tags never establish one or create a general copyleft exception.
- The Docling pack permits per-library, per-distribution exceptions for the libgomp/libgfortran/libquadmath runtime cases and the permissive native licences FTL (never the GPL alternative), IJG, libpng-2.0 and libtiff, with full notices. GEOS (`libgeos`, `libgeos_c`, Shapely) is allowed as unmodified shared objects under LGPL-2.1 with full notices and an exact upstream source pointer; Rtree libspatialindex requires its upstream MIT copyright notice. Other permissive attribution-only or OSI weak-copyleft (LGPL-2.1/3.0, MPL-2.0) dynamically loaded, unmodified libraries in this out-of-process pack require the same named review, full text, notice and exact source pointer. CC0 covers data or metadata only (`engines/docling/pack/licenses.py`).
- pywin32 312 has a reviewed composite licence, including dynamically imported, unmodified adodbapi 2.6.2.0 under LGPL-2.1-or-later. The gate binds every wheel file to hash-checked original RECORD receipts and verifies installed RECORD SHA-256/size, allowing only known uv installation changes and derived bytecode caches for reviewed sources. Unlisted files inside its owned trees fail; notices include the immutable upstream source URL. This creates no generic LGPL exception.
- The macOS OpenCV wheel's four OpenEXR/Iex/IlmThread dylibs and one Imath dylib have exact per-distribution/path BSD-3-Clause reviews in `engines/docling/pack/native-components.toml`. Their complete upstream notices are bundled because the wheel omits them. This does not permit its GPL-enabled FFmpeg or other unreviewed companions.
- Linux companion reviews in that inventory cover the exact OpenCV codec/OpenSSL/libdrm/OpenBLAS paths, PyTorch native helpers and Arm Compute/OpenBLAS companions, ONNX Runtime extensions and TorchVision's glibc loader. Weak-copyleft entries require reviewed ELF shared-object hashes and an exact upstream version/source; complete notices include missing upstream copyright texts. Additional GNU runtime hashes are bound to target, distribution and path, with the same full texts, ABI evidence, wheel provenance and three-year source offer. Native GPL/AGPL self-declarations and `--enable-gpl` build flags are refused independently of Python metadata.
- JavaScript dependencies allow CC-BY-4.0 for `caniuse-lite` (data) and OFL-1.1 for the Plus Jakarta Sans packages through per-package exceptions.
- GPL tools such as Pandoc are invoked as separate processes, never linked.
  In the browser, only the unmodified official Pandoc WASM may be aggregated as a
  separate file, with licence notices and corresponding-source links, never linked.
- Fixtures exclude CC-BY-SA documents, GPL test suites, and research-only or non-commercial datasets. Record each fixture's source and license in `fixtures/manifest.toml`.
- Model weights follow the same licensing rules as code, with CDLA-Permissive-2.0 accepted specifically for the Docling TableFormer weights (a permissive data licence, not an OSI code licence); exclude OpenRAIL-M, custom or missing licenses.
- Never commit a user's or private document.

## Brand assets

- `brand/svg/`, `brand/png/`, `brand/icon-composer/` and `brand/preview.png` are generated. The build deletes and rewrites them, so never hand-edit them.
- Change `brand/src/*.mjs` or `brand/build.mjs`, then run `pnpm --dir brand install` (first time only) and `pnpm --dir brand build`.
- After a build, open `brand/preview.png` and check the 16px and 32px tiles before committing.
- Commit the sources and the regenerated outputs together.
- Keep SVGO's `prefixIds` plugin. Without it, IDs collide when several logos are inlined into one page.
- Design rules and the palette live in [docs/brand/design-direction.md](docs/brand/design-direction.md). Update that file when the design changes.

## Docs and language

- Write every repository file in English. Reply to the user in Vietnamese.
- Markdown goes under `docs/` or `plans/`. Root markdown is limited to `README.md`, `ARCHITECTURE.md`, `AGENTS.md` and `CLAUDE.md` (which only imports `AGENTS.md`; put rules in `AGENTS.md`).

## Git

- Follow [docs/git-workflow.md](docs/git-workflow.md) for the full Git workflow.
- Work lands on `dev`. `main` is the stable branch: it receives `dev` only through a promotion, plus hotfixes, and release tags are cut on it.
- Commit only when the user asks; an authorized plan run counts as that request for commits within the plan's scope.
- Pushes and GitHub settings require the user's explicit go-ahead.
- Stage explicit paths. Never run `git add -A` or `git add .` at the repository root.
- Use conventional commits (`feat(brand): …`, `docs: …`) with no AI or tool references.
- Never commit `node_modules/`, `.env*`, credentials or private keys.
