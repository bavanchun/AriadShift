# Docling pack worker result

The pack now consumes hash-pinned native release artifacts from `pins.toml` and can bootstrap from source while release entries are absent. Windows OpenCV and LLVM OpenMP use Ninja with the activated MSVC toolchain. Implementation is committed; runtime validation remains with the coordinator, as required by the worker brief.

## Files changed and commits

- `engines/docling/pack/native_build.py` adds the release consumer, retains source-build fallback, and fixes the Windows generators.
- `engines/docling/pack/build.py` replaces the missing `torch-wheels.toml` dependency with release pins or a native source build. Cached torch builds are verified before reuse. Wheel versions and required OpenBLAS/LLVM sidecars are checked before installation.
- `engines/docling/tests/test_pack.py` covers release provenance, checksums, damaged cache rejection, stale pins, invalid filenames, bootstrap selection, release consumption and required sidecars. The inherited ONNX common-notice regression is retained and formatted.
- `.github/workflows/pack-docling.yml` provisions MSVC and sccache, caches native outputs and compiler work, frees Linux build space, and allows 350 minutes for bootstrap builds.

Commits on `fix/docling-drop-intel-ipp-2`:

- `1eb97a34da91f371ff471b1c0e7c7da6ed8baefc` — `fix(engine): consume pinned native wheels with source fallback`
- `f0284d5` — `ci(engine): provision native pack fallback builds`

The inherited changes in these files were reviewed and retained. No files owned by T1 or T3 were edited. `probe-runtime.py` and `conversion_proof.py` already provide the required native math/ABI and table/OCR comparisons and did not need changes. No push was performed.

## Failure diagnosis

[Pack run 40159069799](https://github.com/bavanchun/AriadShift/actions/runs/40159069799) has four real failures in `Build the pack`:

- Linux x86_64 and Windows x86_64 stop at the missing `torch-wheels.toml` message. The replacement reads the shared pack pins and has a source fallback.
- Linux ARM and macOS ARM stop when the vendor gate scans ONNX Runtime's `ThirdPartyNotices.txt`. The old gate rejected a generic Intel Simplified Software License title. T3 owns the gate correction; its committed implementation preserves common notices while rejecting specific IPP notices and proprietary native markers. This worker retains the regression test.

[torch run 40159069750](https://github.com/bavanchun/AriadShift/actions/runs/40159069750) confirms the Windows LLVM failure: CMake cannot find any instance of the selected Visual Studio 17 2022 generator. Ninja with explicit `cl` compilers removes that generator dependency. The Linux torch job in this run is cancelled, rather than a proven compiler failure.

The Windows logs print install-action's Bash-startup retry script, including its failure text. That echoed script is not the terminal cause of these four Pack failures: all four reach the pack build. Runner-startup flakes must be handled separately from the missing pins, vendor gate and CMake failures; no blanket build retry was introduced.

## Release pin contract

T3's committed `pins.toml` and toolchain workflow match this consumer contract:

| Key | Required value |
|---|---|
| `toolchain.release` | `toolchain-wheels-<version>` in `bavanchun/AriadShift` |
| `toolchain.targets.<triple>.opencv.build_key` | `native_build.cache_key("opencv", triple, pins)` |
| `toolchain.targets.<triple>.torch.build_key` | `torch_build.key(triple)` |
| Each kind's `files[]` | `name`, `url`, SHA-256 `sha256`, positive byte `size` |

Configure both kinds for Linux x86_64 and Windows x86_64. ARM retains the audited upstream wheels. URLs must belong to the configured release in this repository. Asset basenames may differ from local `name`, allowing target-specific license asset names. Torch requires `OpenBLAS-LICENSE.txt`; Windows additionally requires `libiomp5md.dll` and `LLVM-OpenMP-LICENSE.txt`. Torchvision replacement wheels are optional when the retained wheel passes the native ABI probe.

Absent per-kind entries select source builds. Present stale pins, invalid release locations and failed checksum/download checks fail closed. No placeholder checksums were added. The publisher clears the consumer pins before building fresh OpenCV outputs, avoiding stale release inputs during an update.

## Verification

Passed without building or running tests:

- `.venv/bin/ruff check --no-cache engines/docling/pack/build.py engines/docling/pack/native_build.py engines/docling/tests/test_pack.py`
- `.venv/bin/ruff format --check --no-cache engines/docling/pack/build.py engines/docling/pack/native_build.py engines/docling/tests/test_pack.py`
- `actionlint .github/workflows/pack-docling.yml`
- Python AST parsing of the three changed Python files and `git diff --check`.

Current API references checked: [uv CLI](https://docs.astral.sh/uv/reference/cli/), [OpenCV packaging source](https://github.com/opencv/opencv-python/blob/master/setup.py), [LLVM OpenMP build configuration](https://github.com/llvm/llvm-project/blob/main/openmp/runtime/CMakeLists.txt), and [auditwheel](https://github.com/pypa/auditwheel).

Run these commands sequentially from the worktree as coordinator:

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
UV_PROJECT_ENVIRONMENT=.tools/docling/python-env uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pack.py
just docling-test
just docling-engine-test
just pack-docling
just pack-docling-verify
just lint-workflows
```

Then run the PR's `torch-docling.yml` and `pack-docling.yml` jobs and verify all four pack targets. For native x86_64, inspect the uploaded `comparison-<target>.json`, runtime probes and conformance results. After publishing audited toolchain wheels, render their pins with T3's `licenses.py --render-toolchain-pins` route, commit the real hashes, and rerun the pack workflow to exercise the release-consumer path.

## Open concerns

No builds, pytest runs, runtime probes or remote workflows were executed by this worker. Passing native builds and conversions are not claimed. The release artifact entries are still absent during bootstrap; publishing and pinning actual wheel digests remains the phase's integration gate. No daemon or development server was started.

Unresolved questions: none.

Status: DONE_WITH_CONCERNS
