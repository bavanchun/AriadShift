---
phase: 2
title: "Intel-free docling pack (#51) and toolchain wheels"
status: pending
depends_on: [1]
branch: fix/docling-drop-intel-ipp
---

# Phase 2: Intel-free docling pack

Finishes #51 under its own plan, `plans/261024-2112-docling-no-intel-ipp/plan.md` on the #51 branch, and applies boss decision A-1655 §2. The IPP-free OpenCV and OpenBLAS/oneDNN PyTorch wheels are built in our pack CI, cached by source hash, published on a `toolchain-wheels-<ver>` GitHub Release in this repo, and pinned by sha256 in the pack lock. Worktrees: `wt-opencv` and `wt-no-intel` (kept until this lands). Rebase onto `origin/dev` first.

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | GPT-6.1-Sol High | `engines/docling/pack/torch_build.py`, `torch-build.toml`, `vcomp-compat.cpp`, `vcomp-probe.cpp`, `.github/workflows/torch-docling.yml` | Fix the CPU-torch failures: the Windows `CMake Error at CMakeLists.txt:30 (project)` (run 40159069750) and the Linux x86_64 failure. Make it build with OpenBLAS/oneDNN and no MKL. Separate the runner bash-startup flake (actions/partner-runner-images#169) from real failures. |
| T2 | GPT-6.1-Sol High | `engines/docling/pack/native_build.py`, `build.py`, `probe-runtime.py`, `conversion_proof.py`, `.github/workflows/pack-docling.yml`, `engines/docling/tests/test_pack.py` | Fix the four Pack target failures (run 40159069799), building OpenCV/OpenMP without IPP, and consume the pinned toolchain wheels. |
| T3 | Gemini 4 Argon High | `.github/workflows/toolchain-wheels.yml` (new), `engines/docling/pack/pins.toml`, `licenses.py`, `licenses/**`, `docs/packs.md`, `docs/docling.md`, `docs/releasing.md`, the ARCHITECTURE.md Decision Log row | The wheel publish workflow: cache keyed on a source hash, upload to `toolchain-wheels-<ver>`, and a sha256 pin in `pins.toml`, which the pack build verifies. Document the flow. |

## Build and test pass (coordinator, sequential)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just docling-test && just docling-engine-test
just pack-docling && just pack-docling-verify      # aarch64-apple-darwin
just lint-workflows
```

Then one PR CI run of `torch-docling.yml` and `pack-docling.yml`. This is the only evidence for the Windows and Linux targets, so it is part of this phase's gate (plan risk table).

## Acceptance

- [ ] The pack builds and passes `pack-docling-verify` locally on macOS arm64; the license check finds no Intel IPP/MKL components.
- [ ] The torch and pack jobs pass for all four targets on the PR run.
- [ ] Toolchain wheels are published on `toolchain-wheels-<ver>` and pinned by sha256. Publishing needs a push to a release, so the boss's go-ahead is needed if it is not covered by A-1655 §2.
- [ ] #51 is squash-merged; `wt-opencv` and `wt-no-intel` are removed after merge.
