# Worker Report: Phase 2 — Task `p2-t3` (Toolchain Wheels CI, Release Pinning, Licence Gate & Docs)

- **Worker**: `p2-t3` (Gemini 4 Argon High)
- **Worktree**: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-no-intel`
- **Branch**: `fix/docling-drop-intel-ipp-2`
- **Status**: DONE

## 1. Files Changed

All modified and created files are strictly within `p2-t3` ownership:

- `.github/workflows/toolchain-wheels.yml` (created)
- `engines/docling/pack/licenses.py` (modified)
- `engines/docling/pack/pins.toml` (modified)
- `docs/docling.md` (modified)
- `docs/packs.md` (modified)
- `docs/releasing.md` (modified)
- `ARCHITECTURE.md` (modified — added one new row in the `## 18. Decision Log` table only)

Note on `engines/docling/pack/licenses/ittapi.txt`: retained unchanged because `engines/docling/pack/build.py` (line 691) copies `PACK_DIR / "licenses/ittapi.txt"` into `notices/ittapi-LICENSE.txt` to cover the permissive BSD-3-Clause ITT instrumentation symbols present in the retained official ARM PyTorch wheels (`test_vendor_gate_allows_permissive_itt_and_onednn_symbols` in `engines/docling/tests/test_pack.py`).

## 2. Commits Made

1. `00a1869` — `build(docling): add toolchain-wheels workflow, wheel audit gate and release pins`
2. `0fa0b24` — `docs(docling): document toolchain-wheels release flow and decision log`

## 3. Summary of Changes

1. **Toolchain wheels CI workflow (`.github/workflows/toolchain-wheels.yml`)**:
   - Triggered on `workflow_dispatch` and `push.tags: ["toolchain-wheels-v*"]`.
   - Least-privilege `permissions: contents: read` at workflow level and on `build`, `arm`, and `checksums` jobs; `permissions: contents: write` granted only to the `publish` job (`if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/toolchain-wheels-v')`).
   - All GitHub Actions are pinned to 40-character commit SHAs with version comments (`actions/checkout` v7.0.1 with `persist-credentials: false`, `astral-sh/setup-uv` v10.3.0, `taiki-e/install-action` v2.87.28, `ilammy/msvc-dev-cmd` v1.13.0, `actions/cache` / `restore` / `save` v6.1.0 with `# zizmor: ignore[cache-poisoning]`, `actions/upload-artifact` v7.0.2, `actions/download-artifact` v8.0.2). No `${{ ... }}` expressions are interpolated inside `run:` blocks (`zizmor` `template-injection` safe).
   - **`build` job** (`x86_64-unknown-linux-gnu` on `ubuntu-26.04`, `x86_64-pc-windows-msvc` on `windows-2025`):
     - Caches source blobs (`~/.cache/ariadshift/docling/blobs`), OpenCV/LLVM native builds (`~/.cache/ariadshift/docling/native`), `sccache` compiler state, and audited PyTorch wheels (`docling-torch-${{ matrix.target }}-${{ steps.key.outputs.value }}`).
     - Builds and verifies MKL-free CPU PyTorch (`torch_build.py build` / `verify`) and IPP-free / ittapi-free OpenCV (`native_build.opencv` / `verify_opencv` with `pins["toolchain"]` stripped so the toolchain builder always produces/verifies the source wheel).
     - Collates built `.whl` files, sidecars (`OpenBLAS-LICENSE-<target>.txt`, plus `libiomp5md.dll` and `LLVM-OpenMP-LICENSE.txt` on Windows), and provenance records (`opencv-build-<target>.json`, `torch-build-<target>.json`) into `wheels-out/`.
     - Audits all built wheels and sidecars via `licenses.check_wheel_files(..., allow_itt=False, openmp_dll=openmp)` and uploads `toolchain-wheels-${{ matrix.target }}`.
   - **`arm` job** (`aarch64-unknown-linux-gnu` on `ubuntu-26.04-arm`, `aarch64-apple-darwin` on `macos-26`):
     - Audits the retained MKL-free official ARM wheels and verifies native CPU ABI (`torch_build.py verify-upstream`).
   - **`checksums` job**:
     - Downloads all `toolchain-wheels-*` artifacts, renders `toolchain-pins.toml` (`licenses.py --render-toolchain-pins`), generates and verifies `SHA256SUMS`, and uploads `toolchain-wheels-release`.
   - **`publish` job** (`toolchain-wheels-v*` tags only):
     - Verifies `SHA256SUMS` and publishes all wheels, sidecars, provenance files, `toolchain-pins.toml`, and `SHA256SUMS` to the GitHub Release on this repository via `gh release create` / `gh release upload --clobber`.

2. **Wheel vendor audit & pin rendering (`engines/docling/pack/licenses.py`)**:
   - Preserved the common-notice fix in `check_vendor_files()` so consolidated third-party notice files mentioning unused MKL license titles (such as ONNX Runtime's `ThirdPartyNotices.txt`) pass when native binaries contain no MKL markers, while explicit IPP/ICV notice sections and native markers remain forbidden.
   - Extended `check_vendor_files(root, approved_openmp=None, *, allow_itt=True)` to also inspect entries inside `.whl` zip archives if present under `root`.
   - Added `check_wheel_files(wheels, approved_openmp=None, *, allow_itt=False, openmp_dll=None)` and CLI flag `--wheels` (`--openmp-dll`, `--allow-itt`):
     - Defaults to `allow_itt=False` so source-built toolchain wheels (`USE_ITT=0`, `-DWITH_ITT=OFF`, `-DLIBOMP_USE_ITT_NOTIFY=OFF`) are strictly rejected if any Intel IPP, MKL, Intel OpenMP, or `ittapi` (`ittnotify`, `libitt`, `__itt_`, `ijit_notifyevent`) library or symbol is present.
     - For `opencv_python_headless-*.whl`, strips only the single reviewed x86-only `_IPP_NOTICE_HEADING` section in-memory before scanning the remaining notice text for forbidden IPP/ICV notices.
   - Added `render_toolchain_pins(artifacts, release_tag)` and CLI flags `--render-toolchain-pins DIR --release-tag TAG` to render the `[toolchain]` and `[toolchain.targets.<target>.<kind>]` tables for `pins.toml`.

3. **Toolchain release pins (`engines/docling/pack/pins.toml`)**:
   - Kept `[opencv]`, `[llvm]`, `[manylinux]`, `[python]`, `[[models]]`, and `[tessdata]` intact so `native_build.cache_key()` and `torch_build.key()` hashes are preserved.
   - Added `[toolchain]` (`release = "toolchain-wheels-v0.2.0"`, `source = "https://github.com/bavanchun/AriadShift/releases/tag/toolchain-wheels-v0.2.0"`) and `[toolchain.targets.<triple>]` sections matching `native_build.release_files(kind, target, pins, cache, destination, build_key)`:
     - During bootstrap (before the first `toolchain-wheels-v0.2.0` tag is pushed and published), `.opencv` and `.torch` subtables under `[toolchain.targets.<triple>]` are omitted so `release_files()` returns `None` and falls back to building from the pinned sources.
     - Documented the exact `[toolchain.targets.<triple>.opencv]` and `[toolchain.targets.<triple>.torch]` schema (`build_key` and `files = [{ name, url, sha256, size }]`) and the `licenses.py --render-toolchain-pins` command.

4. **Documentation & Architecture (`docs/docling.md`, `docs/packs.md`, `docs/releasing.md`, `ARCHITECTURE.md`)**:
   - Updated `docs/docling.md`, `docs/packs.md`, and `docs/releasing.md` to document `.github/workflows/toolchain-wheels.yml`, the `toolchain-wheels-v*` release flow, `[toolchain]` pins in `engines/docling/pack/pins.toml`, and `licenses.py --wheels` / `--render-toolchain-pins`.
   - Added one row (`Docling toolchain wheel hosting and pinning`) to the `## 18. Decision Log` table in `ARCHITECTURE.md`.

## 4. Verification Commands for Coordinator

Run from `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-no-intel`:

```bash
actionlint .github/workflows/toolchain-wheels.yml
zizmor .github/workflows/toolchain-wheels.yml
uv run --package ariad-docling --frozen ruff check engines/docling
uv run --package ariad-docling --frozen ruff format --check engines/docling
uv run --package ariad-docling --frozen pytest engines/docling/tests
just docling-test
```

## 5. Open Concerns / Coordinator Notes

- **`.github/workflows/release.yml` tag exclusion**: `.github/workflows/release.yml` (line 54, outside `p2-t3` file ownership) currently excludes `- '!toolchain-torch-*'` under `on.push.tags` (`'**[0-9]+.[0-9]+.[0-9]+*'`). Because `toolchain-wheels-v0.2.0` contains `[0-9]+.[0-9]+.[0-9]+`, `.github/workflows/release.yml` (and `scripts/check-release-workflow.sh` if applicable) should add `- '!toolchain-wheels-*'` before pushing a `toolchain-wheels-v*` tag.
