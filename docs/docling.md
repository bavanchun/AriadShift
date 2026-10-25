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
- **Windows:** install the [Microsoft Visual C++ Redistributable x64](https://aka.ms/vc14/vc_redist.x64.exe) (2015–2022 family, latest supported v14) before running the Docling pack. The latest x64 redistributable is a hard requirement; check the installed version in the `Version` value under `HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64` and update using the linked installer if needed. The pack uses the system runtime and excludes `vcruntime*` and `msvcp*` DLLs, including copies from wheels. The distributed `ashift.exe` uses static CRT; this prerequisite applies to Docling, not the standalone host.
- **Tesseract 5** on `PATH` for OCR of Vietnamese and mixed Vietnamese-English text. The pack ships the `vie`, `eng` and `osd` data and `configs/tsv`, not the program: `apt install tesseract-ocr`, `brew install tesseract`, or the UB Mannheim installer on Windows. Without it, digital documents still convert. Installation prints the Tesseract prerequisite and, on Windows, the Visual C++ Redistributable prerequisite; `ashift doctor` explains what is missing.
- On Linux, the sandbox grants the canonical Tesseract binary execution access. For a binary under `<prefix>/bin/`, only `<prefix>/{lib,lib64,libexec,share/tessdata,etc}` receives read access; reading the whole prefix is refused. A binary in `/bin`, `~/bin` or `~/.local/bin` therefore cannot grant access to all of `/`, the home directory or `~/.local`. The Tesseract grant does not include `~/.local/share`; only its `tessdata` subdirectory is allowed. Libraries and data must live in those subdirectories or the base system paths (`/usr`, `/lib`, `/lib64`, `/nix/store`). Dependencies spread across other prefixes, such as separate Linuxbrew Cellar packages, need a system installation or a self-contained prefix.
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
| `just pack-docling-test` | Runs the real engine conformance and acceptance suite against that pack; needs Pandoc and Tesseract. |

`ASHIFT_PACK_DOCLING=<dir>` runs an unpacked pack instead (see [packs.md](packs.md#resolution)); `build.py verify` uses it.

## The pack

`engines/docling/pack/` holds the build:

| File | Role |
|---|---|
| `pins.toml` | Every non-wheel input with its source, revision, licence, size and SHA-256: python-build-standalone `20261021` (CPython 3.14.9) per target, the Heron layout, TableFormer and RapidOCR models, `tessdata_best` with `configs/tsv`, and `[toolchain]` release pins for pre-built Intel-free OpenCV, CPU PyTorch and Windows LLVM OpenMP wheels/sidecars. Upstream PyPI wheels are pinned by `uv.lock`. |
| `fetch.py` | Downloads pinned files into the cache and verifies them; a cached blob is re-hashed before every use. |
| `licenses.py` | The licence gate, notice writer, native wheel vendor gate (`--wheels`), and `[toolchain]` pin renderer (`--render-toolchain-pins`). |
| `build.py` | `build`, `verify` and `manifest` (below). |
| `native_build.py` | Hash-verified OpenCV and LLVM source builds, cached by source, target, flags and build tools. |
| `torch_build.py`, `torch-build.toml` | The MKL-free x86_64 CPU toolchain builder, source pins and build options. |
| `vcomp-compat.cpp` | Apache-2.0 MSVC OpenMP entrypoints compiled into the Windows LLVM runtime. |
| `conversion_proof.py`, `probe-runtime.py` | Original/replacement table and OCR comparisons, cv2 configuration and parallel PyTorch/Windows ABI checks. |

`build.py build --target <triple> --out <dir> [--ashift <path>]` (or `just pack-docling`):

1. Unpacks the standalone Python for the target, removes unused GUI/test content, `_dbm` (including its Linux Berkeley DB copy) and Windows `libtommath.dll`, then checks every native runtime file against the licence inventory before installing wheels. Unlisted `.so*`, `.dylib`, `.pyd` or `.dll` files fail the build with their paths; every selected inventory entry must match a file. Statically linked dependencies name their containing `libpython` or extension file. Installs the wheels of `uv export --package ariad-docling` with `--require-hashes` and `--python-platform` for the target, and the engine wheel. The PyTorch CPU index and PyPI are mixed safely because every wheel is hash-pinned.
2. Byte-compiles the standard library and the packages with hash-based pycs (native builds only, with file names relative to the library directory, and with the interpreter forbidden to add caches of its own), uses a fixed hash seed and compilation order, removes the installer's `direct_url.json` and `uv_cache.json` records (including their `RECORD` entries and those for removed console scripts), and checks that the freshly built engine answers `describe` with a cleared environment. Native builds also convert a small digital PDF directly through the bundled runtime, with a timed traceback to diagnose a library or model hang. Neither the bytecode nor the records name the build directory, so the same inputs give the same archives wherever the pack is built.
3. Runs the licence gate over every installed distribution. Each distribution's SPDX expression (or its licence field or classifiers) is parsed: `OR` lets the build pick an allowed alternative, `AND` needs every operand allowed, and `WITH` accepts only reviewed exceptions. The licences must be on the allow-list of permissive licences, with MPL-2.0 only for reviewed, unmodified exceptions; AGPL, GPL, non-OSI, OpenRAIL and unknown licences fail the build, and so does a licence nobody has reviewed yet. Model and data licences come from `pins.toml`. The notices of every distribution, the model and data licences, CPython and its C-library licence texts, and the AriadShift `LICENSE` and `NOTICE` go to `notices/`. [runtime-libraries.toml](../engines/docling/pack/runtime-libraries.toml) records runtime library versions, upstream release URLs and text digests; changing the Python release requires reviewing this inventory. Unused Tk/Tcl, tkinter, idlelib and standard-library test directories are removed before wheel installation. The manifest licence expression combines the gate-approved distribution expressions with the runtime, model, data and project licences.
4. Writes reproducible `tar.zst` archives (`SOURCE_DATE_EPOCH`, normalized ownership and modes), each under 2 GiB: `runtime` (the interpreter, the packages and `notices/`, per target), `models` and `tessdata` (shared by all targets).
5. Writes an unsigned `pack.toml` with `ashift __pack manifest`, which hashes the archives and declares their unpacked size and entry count. The entrypoint is `runtime/bin/python3 -I -m ariad_docling` (`runtime/python.exe` on Windows) and the default options are `artifacts_path = "{root}/models"` and `tessdata_path = "{root}/tessdata"`.

The x86_64 packs replace the published OpenCV wheel with a build of the same locked
sdist, with IPP, ITT and proprietary device/acceleration backends disabled. Linux
builds use a digest-pinned manylinux 2.28 container and auditwheel; Docker is a
build prerequisite on Linux x86_64. Windows builds use Ninja with the runner's
native MSVC developer environment instead of assuming a particular Visual Studio generator.
The `pack-build` dependency group locks the build tools without adding them to
the distributed runtime. Source URLs, sizes and SHA-256 digests are in `pins.toml`;
the OpenCV pin must match `uv.lock` when it is updated.

The Windows toolchain builds LLVM OpenMP from its pinned release with five MSVC
`_vcomp_*` wrappers using LLVM's `__kmpc_*` operations. It keeps PyTorch's required
`libiomp5md.dll` name, removes `libiompstubs5md.dll`, refreshes wheel RECORD hashes,
and includes the LLVM licence and wrapper source in the notices. Every OpenMP
region and thread query uses this one runtime. Parallel matrix, reduction and
elementwise checks, plus direct wrapper checks of captures, barriers, serialized
regions and static loops, run before packaging.

ARM packs retain the upstream hash-locked OpenCV wheels. Their common third-party
notice includes an IPP section explicitly scoped to x86/x86_64, even though IPP
does not apply to ARM. The builder removes only that reviewed section, and does
the same for source builds after probing cv2's configuration. Every other notice
is preserved. The structural gate scans the final runtime and notices, including
wheel contents, and rejects IPP ICV notice text, IPP binary markers, Intel OpenMP
version resources and unapproved `iomp` files. Only the exact LLVM build digest
is allowed at the replacement path; its bytes are still scanned. An unrecognized
notice layout or native artifact fails the build.

The official Linux and Windows x86_64 CPU PyTorch wheels contain proprietary
oneMKL. These targets use a source-built CPU wheel with static OpenBLAS and C
LAPACK instead. Apache-2.0 oneDNN remains enabled; CUDA, ROCm, XPU and distributed
execution are disabled. The official torchvision wheel is retained only after
its native NMS and ROIAlign operators pass against the replacement torch;
otherwise the toolchain builds torchvision against that torch. ARM retains the
official hash-locked PyTorch wheels, which were audited without MKL markers.

BSD-3-Clause ITT instrumentation is permitted, including the ITT code in the ARM
PyTorch wheels and LLVM OpenMP. Its license ships in the native third-party
notices. The gate rejects proprietary MKL identifiers, IPP/ICV and Intel OpenMP,
independently of the permissive top-level wheel metadata. ITT symbols and
Apache-2.0 oneDNN names are not proprietary MKL identifiers.
Common third-party notice files can name optional components that were not built.
The ONNX Runtime CPU wheels retain their upstream consolidated notices, including
the unused MKL license entry; the native files still pass the MKL marker gate.
An explicit IPP/ICV notice section remains forbidden independently of metadata.

The [toolchain wheels workflow](../.github/workflows/toolchain-wheels.yml) and
[torch toolchain workflow](../.github/workflows/torch-docling.yml) own source
builds. They run natively on Linux and Windows x86_64 and verify the retained
ARM wheels on Linux aarch64 and macOS arm64; the Mac arm64 development machine
does not emulate x86_64 source builds. Compiler caches preserve completed work
across bounded jobs, and wheel caches avoid rebuilding unchanged toolchains.
The build key covers source, options, target and frozen build tools, excluding
model pins and the published wheel manifest. Restored wheels are rehashed and
run through CPU/torchvision and OpenCV probes. Linux uses the pinned manylinux
builder to preserve the pack's glibc floor. OpenBLAS C LAPACK avoids a bundled
Fortran runtime.

Build artifacts contain wheels, native licenses, the Windows LLVM DLL,
per-target `*-build-<target>.json` provenance records, `toolchain-pins.toml` and
`SHA256SUMS`. Pushing a `toolchain-wheels-v*` tag triggers
[`.github/workflows/toolchain-wheels.yml`](../.github/workflows/toolchain-wheels.yml)
to publish the audited wheels, sidecars and `SHA256SUMS` to a GitHub Release on
this repository. Maintainers then pin each target's `build_key` and `files`
under `[toolchain]` in `engines/docling/pack/pins.toml` (using
`licenses.py --render-toolchain-pins`). While a target's `[toolchain.targets.<triple>.<kind>]`
entry is omitted during bootstrap, `native_build.release_files()` falls back to
building from the pinned sources. Once pinned, pack builds download and verify
those exact release inputs by SHA-256 and size; they never select the latest
artifact or silently fall back if a pin is stale. Changing source versions or
options requires a new toolchain build, `toolchain-wheels-v*` release and
updated `pins.toml` entries. A toolchain tag does not publish an AriadShift or
Docling pack release.

Native x86_64 builds convert the table PDF and two OCR PDFs with both the original
locked wheels and the replacements. `comparison-<target>.json` records extracted
text, table data and layout differences for TableFormer, RapidOCR and Tesseract.
The comparison also records three timed samples after a discarded warmup and
reports median elapsed time and engine-process CPU time. The latter includes
native threads and excludes OCR child processes; elapsed time includes the full
conversion. Both sides use the same runner, fixture, engine limits and thread
policy. This is a bounded CPU conversion comparison, not a throughput guarantee.
The original proprietary libraries are used only for this build-time comparison;
they are replaced before the distribution gate and archive writer run.

`build.py verify --out <dir> --input <file> --expect <text> [--offline]` unpacks the archives into a new directory, uses it as `ASHIFT_PACK_DOCLING`, and converts `<input>` to Markdown with `ashift`, so the manifest, engine, models and OCR data all take part. With `--offline` on Linux the conversion runs under `unshare -rn`, which proves that nothing is downloaded. `build.py manifest --out <dir> <triple>...` merges the targets built into `<dir>` into one `pack.toml`.

Targets are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. The macOS pack needs macOS 14 or later, because `docling-parse` ships no older wheels (`MACOS_DEPLOYMENT_TARGET` in `build.py`).

### CI

[`.github/workflows/pack-docling.yml`](../.github/workflows/pack-docling.yml) runs on pull requests that touch the engine, the mapper, `uv.lock`, the `justfile` or the workflow, and on `pack-docling-v*` tags:

- **Engine tests** on `ubuntu-26.04` and `windows-2025`: ruff and pytest.
- **Engine conformance** on Linux with cached models and Tesseract: `just docling-engine-test`.
- **Pack** builds the Linux x86_64 pack twice in separate directories, compares the component and manifest hashes, and converts the scanned Vietnamese page with it offline. Tags, and pull requests that change `engines/docling/pack/` or the workflow, also build `aarch64` Linux (not run), macOS and Windows (each converts a digital PDF) and merge their manifests. Linux and Windows x86_64 jobs additionally run the conformance/acceptance tests with the packaged interpreter and upload the original/replacement conversion comparisons. Native OpenCV/LLVM caches are separate from downloaded source/model blobs.

For a failed matrix job while its sibling is still running, obtain the job ID
with `gh run view <run-id> --json jobs`. Download only that completed job's logs
with `gh api --allow-escape-sequences repos/bavanchun/AriadShift/actions/jobs/"$job_id"/logs > "$log_path"`.
This uses the repository's existing Actions read access. The CLI's run-wide log
command waits for the whole run to finish; the job endpoint allows earlier
diagnosis. Keep raw logs in an ignored local file and strip terminal escapes
before displaying them.

Native pack jobs also sign a test copy of the manifest with an ephemeral key, install it through
`ashift engines install docling --from`, convert a digital PDF using installed-pack resolution,
and run two Office jobs in a shared session before checking a 64 MiB memory limit. The test
copy accepts the development host version; the distributed manifest still requires 0.2.x.

Signing the manifest and attaching it with the archives to the `pack-docling-v<version>` release is described in [packs.md](packs.md#signing-and-publishing).

### Updating a pin

Check the new version or revision against its registry (PyPI, GitHub Releases, Hugging Face), take the SHA-256 and size from the publisher or from a verified download, update `pins.toml` or `uv.lock` together with them, and run `just docling-dev`, `just docling-test` and `just docling-engine-test`. Never use alpha, beta or release-candidate builds. A new model licence must also pass the licence gate (`licenses.ALLOWED_DATA`).

When OpenCV, PyTorch, OpenBLAS, LLVM OpenMP or manylinux build inputs change:

1. Update the source pins in `engines/docling/pack/pins.toml`, `engines/docling/pack/torch-build.toml` and `uv.lock`.
2. Trigger or tag [`.github/workflows/toolchain-wheels.yml`](../.github/workflows/toolchain-wheels.yml) with `toolchain-wheels-v<version>` (after explicit maintainer authorization) so CI builds, caches, audits and publishes the Intel-free wheels, sidecars, provenance files, `toolchain-pins.toml` and `SHA256SUMS` to GitHub Releases.
3. Download the `toolchain-wheels-release` artifact (or release assets), verify `sha256sum --check SHA256SUMS`, and update `[toolchain]` in `engines/docling/pack/pins.toml` with the rendered `toolchain-pins.toml` entries (`uv run --no-project --python 3.14 python engines/docling/pack/licenses.py --render-toolchain-pins <dir> --release-tag toolchain-wheels-v<version>`).
