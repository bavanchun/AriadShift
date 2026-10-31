# Torch toolchain worker result

The owned toolchain and workflow fixes are committed on `fix/docling-drop-intel-ipp-2`. Static checks pass. Native builds, runtime probes and the four-target CI acceptance gate remain for the coordinator; this worker ran no builds or tests, as required by the common brief.

## Files and commits

Changed in `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-no-intel`:

- `engines/docling/pack/torch_build.py`
- `engines/docling/pack/torch-build.toml`
- `.github/workflows/torch-docling.yml`

Commits:

- `d48253f` — `fix(engine): harden MKL-free torch build configuration`
- `332d4d1` — `ci: cache verified torch wheels and audit native ARM targets`

The two vcomp C++ files were reviewed and left unchanged. Changes belonging to T2 and T3 were preserved and excluded from these commits. Nothing was pushed, rebased or published.

## Diagnosis and changes

- Windows job [121066592424](https://github.com/bavanchun/AriadShift/actions/runs/40159069750/job/121066592424) really failed because the LLVM OpenMP helper selected Visual Studio 17 2022 on an image with Visual Studio 18 / 2026. The existing Ninja/MSVC fix in `native_build.py` belongs to T2 and must be included in the integrated revision. T1 explicitly selects Ninja and MSVC for the subsequent PyTorch build, passes LLVM OpenMP headers and its import library, and uses PEP 517 CMake definitions so Windows paths and flags retain their argument boundaries.
- Linux job [121066592595](https://github.com/bavanchun/AriadShift/actions/runs/40159069750/job/121066592595) was canceled during OpenBLAS compilation at approximately object 11709/14175. Its supplied log does not establish a Linux compiler failure. Cache archival then failed because compiler processes were still changing files. The workflow now names its container deterministically, stops that container before cache archival, and stops the Windows compiler-cache server. Compiler cache fallback and per-target concurrency from the abandoned attempt were retained.
- A separate, source-proven configuration defect would affect the next build stage: [OpenBLAS 0.3.34 installs `cblas.h` under `include/openblas`](https://github.com/OpenMathLib/OpenBLAS/blob/v0.3.34/CMakeLists.txt), while the previous driver passed `include`. The driver now resolves and validates the installed header location. OpenBLAS is explicitly static, sequential and locked for concurrent callers. MKL package discovery is disabled; OpenBLAS, oneDNN and OpenMP remain selected.
- The [bash-startup issue](https://github.com/actions/runner-images/issues/14052) is separate from the CMake error. The pinned installer already contains a ten-attempt startup retry, and installation succeeded in the supplied Windows run. Owned source-key, build, verification and cleanup steps now use `python {0}` with checked subprocess calls, avoiding bash startup for those steps.
- The runtime probes now cover concurrent matrix multiplication and an actual oneDNN convolution, alongside the existing MKL/backend checks, NMS and ROIAlign. A rebuilt Linux torchvision wheel is repaired for manylinux while excluding the torch libraries supplied by its declared torch dependency.
- Following the accepted plan and architecture decision, only x86_64 wheels are source-built. Native ARM jobs audit and execute the retained hash-locked official wheels on Linux ARM64 and macOS ARM64. They do not impose the x86 OpenBLAS/oneDNN configuration on Apple's backend. The [documented `ubuntu-26.04-arm` label](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) uses a constant expression because the repository's pinned actionlint predates it.

## Artifact contract for T3 and T2

Workflow name: `Docling torch toolchain`. Source-build job ID: `build`.

| Target | Exact artifact name | Uploaded files |
| --- | --- | --- |
| Linux x86_64 | `docling-torch-x86_64-unknown-linux-gnu` | `torch-out/*` |
| Windows x86_64 | `docling-torch-x86_64-pc-windows-msvc` | `torch-out/*` |

Each artifact has a flat root after download. It contains one `torch-*.whl`, `OpenBLAS-LICENSE.txt` and `build.json`. `torchvision-*.whl` exists only when the upstream wheel fails the ABI probe and must be rebuilt. Windows additionally contains `libiomp5md.dll` and `LLVM-OpenMP-LICENSE.txt`; the DLL is built LLVM, not Intel's distribution. The pack must install the sidecar into `torch/lib/libiomp5md.dll` before import. ARM jobs only verify retained upstream wheels and produce no source-build artifact.

`build.json` remains schema 1. Its `files` object maps every payload basename to `sha256` and `size`; it excludes `build.json` itself. It also records `target`, `build_key`, `sources`, `openblas_flags` and `torchvision_source_build`. Download these `docling-torch-*` artifacts into separate target directories because sidecar and provenance basenames overlap. Published wheel filenames should come from this manifest, since auditwheel can emit multiple compatible platform tags. T3's newly committed publisher builds/collates separately and uses `toolchain-wheels-<target>` artifacts with renamed provenance and OpenBLAS notices; its torch cache contract matches this worker's workflow.

Audited wheel cache key: `docling-torch-<target>-<build_key>`, path `torch-out`. The key covers target, source hashes/options, the driver, native helper, vcomp sources, workflow, frozen dependencies, LLVM and manylinux pins. Every restored payload is rehashed, scanned and imported; newly built payloads receive the same verification before saving. Compiler work has a separate content-addressed sccache fallback. Model and published output digests do not enter the wheel source key.

Current `torch-build.toml` SHA-256: `8603a3ee5c08c39e2d4c0d0ee87dd1f07d3e155a4be86787de6438f185c41771`. Existing source versions and archive hashes were not changed. T3 must pin actual outputs after the coordinator builds them; no wheel digest was invented.

## Verification

Passed locally:

```sh
.venv/bin/ruff check engines/docling/pack/torch_build.py
.venv/bin/ruff format --check engines/docling/pack/torch_build.py
actionlint -color .github/workflows/torch-docling.yml
git diff --check -- engines/docling/pack/torch_build.py engines/docling/pack/torch-build.toml .github/workflows/torch-docling.yml
```

Python AST parsing also passed for the driver, all three probe scripts and the four Python workflow steps; TOML parsing passed. A source review using `ak-code-review` found no unresolved critical/high issue in the owned changes. These are static results, not evidence that native compilation passes.

Coordinator commands, under the shared sequential build queue:

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just docling-test
just docling-engine-test
just lint-workflows
```

On a matching native Linux x86_64 runner with Docker, uv and sccache:

```sh
just docling-torch-build x86_64-unknown-linux-gnu torch-out
uv run --package ariad-docling --group torch-build --frozen python engines/docling/pack/torch_build.py verify --target x86_64-unknown-linux-gnu --out torch-out
```

On a matching Windows x86_64 runner after MSVC environment setup:

```sh
just docling-torch-build x86_64-pc-windows-msvc torch-out
uv run --package ariad-docling --group torch-build --frozen python engines/docling/pack/torch_build.py verify --target x86_64-pc-windows-msvc --out torch-out
```

On the respective native ARM runners:

```sh
uv run --package ariad-docling --frozen python engines/docling/pack/torch_build.py verify-upstream --target aarch64-unknown-linux-gnu
uv run --package ariad-docling --frozen python engines/docling/pack/torch_build.py verify-upstream --target aarch64-apple-darwin
```

After the coordinator's authorized push, require all four `CPU torch (<target>)` jobs in `torch-docling.yml` and the integrated `pack-docling.yml` jobs to pass. Re-run the toolchain workflow once to exercise restored-wheel verification. Then publish and pin the actual artifacts through T3's workflow.

## Open concerns

- T2 must include the Ninja/MSVC LLVM helper change; the original Windows failure occurs in that separately owned file before T1's PyTorch configuration runs.
- Native compilation, runtime probes, published output hashes and pack conformance are unverified because this worker was prohibited from building or testing. No four-target green CI claim is made.
- T2/T3 own the new published-wheel lock/consumer integration and durable documentation. Their output must use the artifact and sidecar contract above.
- A final read of T3's concurrently committed `toolchain-wheels.yml` found two integration follow-ups outside T1 ownership: its literal `ubuntu-26.04-arm` matrix label needs the same pinned-actionlint compatibility handling, and its OpenCV install/probe step runs under the plain Python shell rather than the uv package environment. That step must use the dependency-bearing interpreter (for example a `uv run --package ariad-docling --group torch-build --frozen python {0}` shell), since `uv run` in earlier subprocesses does not activate that environment for later workflow steps.

Status: DONE_WITH_CONCERNS
