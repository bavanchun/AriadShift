# The Docling Engine

Docling turns PDF (digital and scanned), Office files and images into a structured document that AriadShift maps to its IR. It runs out of process as a Python engine speaking `ariad-engine/1`, and ships as the `docling` [engine pack](packs.md). This page covers what the engine does, how to run it from a checkout, and how its pack is built. Design context: [ARCHITECTURE.md §6.2, §7.2](../ARCHITECTURE.md#72-the-docling-engine).

## Routes

| Input | Output | Notes |
|---|---|---|
| `pdf`, `docx`, `pptx`, `xlsx`, `png`, `jpeg`, `tiff`, `webp` | `docling+json` | The engine. Scans, images and PDFs without a text layer get full-page OCR. |
| `docling+json` | `ariad-ir+json` | The in-process mapper in `ariad-core`. It needs no engine, so `ashift convert page.docling.json --to md` works anywhere. |

The planner appends any writer, so `ashift convert scan.pdf --to docx` is `pdf ─docling→ docling+json ─ariad-core→ ariad-ir+json ─pandoc→ docx`. A `.docx` input still goes through Pandoc, which is measured; the Docling `docx` edge stays unmeasured until the benchmarks fill it.

## What it needs

- The `docling` pack (`ashift engines install docling`), or a development engine (below). Without either, a route that needs Docling exits with code 5 and says how to get it.
- **Windows:** install the [Microsoft Visual C++ Redistributable x64](https://aka.ms/vc14/vc_redist.x64.exe) (2015–2022 family, latest supported v14) before running the Docling pack. The pack uses the system runtime and excludes `vcruntime*` and `msvcp*` DLLs, including copies from wheels. The distributed `ashift.exe` uses static CRT; this prerequisite applies to Docling, not the standalone host.
- **Tesseract 5** on `PATH` for OCR of Vietnamese and mixed Vietnamese-English text. The pack ships the `vie`, `eng` and `osd` data and `configs/tsv`, not the program: `apt install tesseract-ocr`, `brew install tesseract`, or the UB Mannheim installer on Windows. Without it, digital documents still convert. Installation prints the prerequisite, and `ashift doctor` explains what is missing.
- On Linux, the sandbox grants the canonical Tesseract binary execution access. For a binary under `<prefix>/bin/`, only `<prefix>/{lib,lib64,libexec,share,etc}` receives read access; reading the whole prefix is refused. A binary in `/bin`, `~/bin` or `~/.local/bin` therefore cannot grant access to all of `/`, the home directory or `~/.local`. Libraries and data must live in those subdirectories or the base system paths (`/usr`, `/lib`, `/lib64`, `/nix/store`). Dependencies spread across other prefixes, such as separate Linuxbrew Cellar packages, need a system installation or a self-contained prefix.
- No network. The engine sets the Hugging Face and Transformers offline switches, disables remote services and external plugins, and keeps temporary files, caches and its home directory inside the job workspace.

## Engine options

Request `options` come from the pack manifest or the caller.

| Option | Values | Meaning |
|---|---|---|
| `artifacts_path` | directory | Docling models. A pack sets `{root}/models`; a development environment defaults to the `models` directory next to its interpreter prefix. |
| `tessdata_path` | directory | `TESSDATA_PREFIX`. A pack sets `{root}/tessdata`; the default is the `tessdata` directory next to the interpreter prefix. |
| `ocr` | `auto` (default), `tesseract`, `rapidocr`, `off` | `auto` uses Tesseract `vie+eng` unless every language hint is a non-Vietnamese language RapidOCR covers. `off` skips OCR. |
| `lang` | list of ISO 639-1 or 639-3 codes | Language hints. An unsupported hint gives an `ocr_language_unsupported` warning. |

`limits.max_pages` and `limits.max_input_bytes` map to Docling's own caps and fail as `limit_exceeded`; `limits.max_memory_mb` is monitored by the host across the engine process tree on every platform. Linux also applies a soft `RLIMIT_DATA`, checking memory headroom through `/proc/self/status` or `getrusage` when the sandbox denies `/proc`; `describe` reports engine-side `enforces_memory_limit: false` elsewhere. Progress is in pages. Warnings are `ocr_fallback`, `ocr_language_unsupported`, `partial_conversion` and `content_dropped`.

## Running it from a checkout

```bash
just docling-dev           # sync .tools/docling/python-env, fetch models and OCR data (about 1.6 GiB)
export ASHIFT_DOCLING_ENGINE="\"$PWD/.tools/docling/python-env/bin/python\" -I -m ariad_docling"
cargo run -p ariad-cli --bin ashift -- convert fixtures/scan/vi-garden-300dpi.png --to md
```

`ASHIFT_DOCLING_ENGINE` is a command line; double quotes group words that contain spaces, so the interpreter path is quoted inside the value to survive a checkout path with spaces. It wins over any installed pack. `just docling-dev` downloads the pinned models and OCR data into `~/.cache/ariadshift/docling`, checks every file's size and SHA-256, and links them under `.tools/docling/`; a second run reuses the cache. On Windows the interpreter is `.tools/docling/python-env/Scripts/python.exe`.

| Recipe | What it does |
|---|---|
| `just docling-dev` | Sets up the environment, models and OCR data. |
| `just docling-test` | `ruff check`, `ruff format --check` and `pytest` over `engines/docling`. The tests need only the environment, not the models. |
| `just docling-engine-test` | Runs `crates/ariad-cli/tests/docling_engine.rs` against the real engine: protocol schema conformance (describe, convert, progress, warning, result last, clean stdout), scanned Vietnamese PNG and PDF to Markdown and DOCX, a digital PDF table, PPTX and XLSX, and cancellation stopping the engine and Tesseract. These tests are ignored by default and need Tesseract. |
| `just pack-docling` | Builds the unsigned pack for this host into `target/pack-docling`. |

`ASHIFT_PACK_DOCLING=<dir>` runs an unpacked pack instead (see [packs.md](packs.md#resolution)); `build.py verify` uses it.

## The pack

`engines/docling/pack/` holds the build:

| File | Role |
|---|---|
| `pins.toml` | Every non-wheel input with its source, revision, licence, size and SHA-256: python-build-standalone `20261021` (CPython 3.14.9) per target, the Heron layout, TableFormer and RapidOCR models, and `tessdata_best` with `configs/tsv`. Wheels are pinned by `uv.lock`. |
| `fetch.py` | Downloads pinned files into the cache and verifies them; a cached blob is re-hashed before every use. |
| `licenses.py` | The licence gate and the notices writer. |
| `build.py` | `build`, `verify` and `manifest` (below). |

`build.py build --target <triple> --out <dir> [--ashift <path>]` (or `just pack-docling`):

1. Unpacks the standalone Python for the target, removes unused GUI/test content, `_dbm` (including its Linux Berkeley DB copy) and Windows `libtommath.dll`, then checks every native runtime file against the licence inventory before installing wheels. Unlisted `.so*`, `.dylib`, `.pyd` or `.dll` files fail the build with their paths; every selected inventory entry must match a file. Statically linked dependencies name their containing `libpython` or extension file. Installs the wheels of `uv export --package ariad-docling` with `--require-hashes` and `--python-platform` for the target, and the engine wheel. The PyTorch CPU index and PyPI are mixed safely because every wheel is hash-pinned.
2. Byte-compiles the standard library and the packages with hash-based pycs (native builds only, with file names relative to the library directory, and with the interpreter forbidden to add caches of its own), uses a fixed hash seed and compilation order, removes the installer's `direct_url.json` and `uv_cache.json` records (including their `RECORD` entries and those for removed console scripts), and checks that the freshly built engine answers `describe` with a cleared environment. Native builds also convert a small digital PDF directly through the bundled runtime, with a timed traceback to diagnose a library or model hang. Neither the bytecode nor the records name the build directory, so the same inputs give the same archives wherever the pack is built.
3. Runs the licence gate over every installed distribution. Each distribution's SPDX expression (or its licence field or classifiers) is parsed: `OR` lets the build pick an allowed alternative, `AND` needs every operand allowed, and `WITH` accepts only reviewed exceptions. The licences must be on the allow-list of permissive licences, with MPL-2.0 only for reviewed, unmodified exceptions; AGPL, GPL, non-OSI, OpenRAIL and unknown licences fail the build, and so does a licence nobody has reviewed yet. Model and data licences come from `pins.toml`. The notices of every distribution, the model and data licences, CPython and its C-library licence texts, and the AriadShift `LICENSE` and `NOTICE` go to `notices/`. [runtime-libraries.toml](../engines/docling/pack/runtime-libraries.toml) records runtime library versions, upstream release URLs and text digests; changing the Python release requires reviewing this inventory. Unused Tk/Tcl, tkinter, idlelib and standard-library test directories are removed before wheel installation. The manifest licence expression combines the gate-approved distribution expressions with the runtime, model, data and project licences.
4. Writes reproducible `tar.zst` archives (`SOURCE_DATE_EPOCH`, normalized ownership and modes), each under 2 GiB: `runtime` (the interpreter, the packages and `notices/`, per target), `models` and `tessdata` (shared by all targets).
5. Writes an unsigned `pack.toml` with `ashift __pack manifest`, which hashes the archives and declares their unpacked size and entry count. The entrypoint is `runtime/bin/python3 -I -m ariad_docling` (`runtime/python.exe` on Windows) and the default options are `artifacts_path = "{root}/models"` and `tessdata_path = "{root}/tessdata"`.

`build.py verify --out <dir> --input <file> --expect <text> [--offline]` unpacks the archives into a new directory, uses it as `ASHIFT_PACK_DOCLING`, and converts `<input>` to Markdown with `ashift`, so the manifest, engine, models and OCR data all take part. With `--offline` on Linux the conversion runs under `unshare -rn`, which proves that nothing is downloaded. `build.py manifest --out <dir> <triple>...` merges the targets built into `<dir>` into one `pack.toml`.

Targets are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. The macOS pack needs macOS 14 or later, because `docling-parse` ships no older wheels (`MACOS_DEPLOYMENT_TARGET` in `build.py`).

### CI

[`.github/workflows/pack-docling.yml`](../.github/workflows/pack-docling.yml) runs on pull requests that touch the engine, the mapper, `uv.lock`, the `justfile` or the workflow, and on `pack-docling-v*` tags:

- **Engine tests** on `ubuntu-26.04` and `windows-2025`: ruff and pytest.
- **Engine conformance** on Linux with cached models and Tesseract: `just docling-engine-test`.
- **Pack** builds the Linux x86_64 pack twice in separate directories, compares the component and manifest hashes, and converts the scanned Vietnamese page with it offline. Tags, and pull requests that change `engines/docling/pack/` or the workflow, also build `aarch64` Linux (not run), macOS and Windows (each converts a digital PDF) and merge their manifests.

Native pack jobs also sign a test copy of the manifest with an ephemeral key, install it through
`ashift engines install docling --from`, convert a digital PDF using installed-pack resolution,
and run two Office jobs in a shared session before checking a 64 MiB memory limit. The test
copy accepts the development host version; the distributed manifest still requires 0.2.x.

Signing the manifest and attaching it with the archives to the `pack-docling-v<version>` release is described in [packs.md](packs.md#signing-and-publishing).

### Updating a pin

Check the new version or revision against its registry (PyPI, GitHub Releases, Hugging Face), take the SHA-256 and size from the publisher or from a verified download, update `pins.toml` or `uv.lock` together with them, and run `just docling-dev`, `just docling-test` and `just docling-engine-test`. Never use alpha, beta or release-candidate builds. A new model licence must also pass the licence gate (`licenses.ALLOWED_DATA`).
