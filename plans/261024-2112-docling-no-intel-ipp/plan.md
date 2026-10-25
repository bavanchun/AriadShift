# Remove proprietary Intel libraries from Docling packs

Status: IN_PROGRESS — source-built x86_64 PyTorch and permissive ITT policy authorized

## Outcome and boundaries

Distributed Docling packs contain no Intel IPP ICV or Intel OpenMP code. Keep the
locked OpenCV version and real cv2 API, the engine protocol and conversion routes.
Build OpenCV without IPP/ITT on x86_64; retain audited upstream aarch64 wheels.
Build LLVM OpenMP with the five MSVC compatibility exports on Windows, replacing
the Intel DLL and removing its stubs. No licensing exception or cv2 shim.

## Execution

1. Verify the OpenCV sdist and LLVM release metadata and downloaded hashes. Inspect
   upstream build options, notices, and the actual PyTorch import surface.
2. Add native builds and target/version/flags caches. Integrate replacements before
   notices and packaging; update wheel RECORD hashes after replacement.
3. Reject proprietary notices and native libraries structurally, with regression
   tests. Exercise actual layout, tables, RapidOCR, Tesseract and conformance from
   the x86_64 packs; compare table and OCR PDFs with the existing pack.
4. Update owning docs and the architecture decision log. Run focused checks and
   the local CI gate under the shared build lock, then self-review.
5. Commit, rebase onto origin/dev, push the feature branch, open a draft PR into
   dev and repair CI on all three OSes and every pack target. Mark ready only
   after those checks pass. Write the requested coordinator report.

## Acceptance evidence

- Verified upstream source pins; no unreviewed proprietary acceleration enabled.
- Regression tests reject IPP ICV notices and original Intel OpenMP binaries,
  while admitting only the built LLVM replacement and its recorded licence.
- Windows and Linux x86_64 packs pass real conversion and protocol checks.
- Table/OCR comparisons record differences, including unchanged results.
- Local checks and remote CI are green, or concrete blockers are reported.

## Risks and rollback

OpenCV's source build can expose platform toolchain or Python ABI issues. LLVM's
MSVC wrappers must preserve OpenMP semantics and every imported symbol. Fail
closed on unknown source/build layouts or binary provenance. Revert the feature
commits to roll back; do not publish the previous proprietary packs.

## Design and verification notes

- PyPI confirms OpenCV 5.0.0.93's sdist size 81,817,738 and SHA-256
  `b82f9831daab90b725c7c1ee1b36cb5732c367096ac76d119e64e14eb70d5f3c`.
  GitHub's LLVM 23.1.4 release API confirms source size 179,272,572 and SHA-256
  `819ecf7351f1a66f76b2d701d8942faaf5faeb324cb90c9d5b090de142434bb2`.
  Both complete downloads matched. Quay's manifest digest pins the manylinux
  builder. Direct registry lookups verified all explicitly added build-tool pins.
- Both ARM wheels include two copies of the common notice with an explicitly
  x86/x86_64 IPP section. Their native files contain no forbidden IPP/OpenMP
  markers. Remove only that inactive section; preserve their wheel binaries.
- The pinned Windows torch_cpu.dll imports 37 OpenMP symbols, including the
  five MSVC wrappers. The replacement keeps all operations in LLVM team state.
  A real MSVC-generated program and direct ctypes probes check the ABI.
- Workflow lint, Python lint and the Python suite passed. Build-only pyelftools
  has legacy licence metadata, so tests run in the documented engine environment
  rather than the separate environment containing native build tools; the pack
  export excludes build dependencies.
- The initial branch records an older crypto-bigint advisory gate failure. This
  session's `just ci` passed cargo-deny and reached Cargo checking; it was
  interrupted after the MKL blocker was confirmed, not reported as a full pass.

## Blocking evidence and handoff

The locked Linux x86_64 CPU PyTorch 2.14.1 wheel reports Intel oneMKL 2024.2,
`BLAS_INFO=mkl` and `USE_MKL=ON`. Its libtorch_cpu.so defines
`MKL_Get_Version_String`, `MKL_Set_Num_Threads_Local` and `mkl_blas_sgemm`, with
no MKL shared-library dependency. MKL is therefore statically present, not an
optional system prerequisite. Intel's [licence FAQ](https://www.intel.com/content/www/us/en/developer/articles/tool/onemkl-license-faq.html)
and [2024.2 package metadata](https://pypi.org/project/mkl/2024.2.0/) identify its
proprietary ISSL licence. This is separate from Apache-2.0 oneDNN/MKL-DNN.

Removing IPP and replacing the OpenMP DLL cannot remove statically linked MKL
from the retained PyTorch wheel. The vendor gate now also rejects its actual
version marker, and this rejection was reproduced against the installed binary.
No exception is proposed. The coordinator must choose the expanded PyTorch
build/replacement approach before pack distribution can satisfy AGENTS.md.

Current work is uncommitted and unpushed. OpenCV's verified sdist built on Linux
with IPP disabled, and cv2 import/resize passed. The final auditwheel integration,
Windows LLVM build, real conversion comparisons, conformance and remote CI
remain unverified. The requested external report records exact commands and
artifact hashes. All processes and containers started by this worker stopped.

## Accepted continuation

The 2026-10-25 correction permits BSD-3-Clause ITT in every pack. Keep the
official ARM torch wheels and their ITT code, recording its license. Build
Linux and Windows x86_64 torch with OpenBLAS, oneDNN enabled, MKL and device
backends disabled, and distributed disabled. Build on GitHub Actions only;
cache by source, target, build options and tools. Publish verified wheels to
a dedicated toolchain release and pin their sizes and SHA-256 in the pack
inputs. No pack or product release tag is authorized.

The final gate rejects proprietary MKL, IPP and Intel OpenMP while accepting
ITT and the exact source-built Windows LLVM replacement. Compare table,
RapidOCR and Tesseract outputs and timings against the original wheels.
Finish with local CI, four-target pack CI, three-OS CI, a ready PR into dev,
and the requested external report. Commit and push checkpoints early; squash
the inherited WIP and provisional build fixes into tested final commits.

ARM wheel audits and Windows MKL identification are recorded in the external
continuation report. The earlier ITT scope blocker is withdrawn by the
coordinator correction; it must not be treated as a user licensing decision.
