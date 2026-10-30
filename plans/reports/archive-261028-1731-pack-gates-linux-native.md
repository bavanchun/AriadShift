# Pack gates: Linux reviews and macOS OpenCV source route

Linux native review is complete: production discovery accepts all **805 native
files** in the two locked Linux input sets with zero unknown names or
notice/source/hash findings. The full local gate is **ALL GREEN** at `8b84f0ab8282e82e165e41144dd8a70b229c2617`.
macOS remains **BLOCKED for release validation**: the video-disabled source route
is implemented, but native macOS compilation, an actual wheel hash and the final
macOS inventory cannot be established on this Linux worker. No GPL waiver,
wheel stripping, release-target drop, push or workflow dispatch was made.

## Commits

- Starting report head: `c93481d9173bbee433c19130db7946d173779964`.
- Linux source: `abc5f0892cf79ae8e83aa153759df71fb96a6e1a`,
  `build(docling): complete Linux native licence reviews`.
- macOS source: `3bfe1a49bce94896cc53dda6c56af373c29df297`,
  `build(docling): build video-disabled OpenCV on macOS`.
- Final source/tested head: `8b84f0ab8282e82e165e41144dd8a70b229c2617`,
  `fix(docling): keep bundled native notices consistent`.
- The report-only successor has the tested source as its first parent.
- Master A-1610/rework 8 supersedes the macOS decision stop. Existing
  A-0100/A-0245/A-0700 govern the named Linux reviews.
- [Previous report](archive-261028-1536-pack-gates-native-stop.md) preserves
  the six-platform census and earlier verification. Windows remains deferred.

## OpenCV step 0: cv2 is required

`uv tree --invert --package opencv-python-headless` selects OpenCV **5.0.0.93**
through **RapidOCR 3.10.0 → docling-slim 2.142.0**, with selected
`feat-ocr-rapidocr-onnx`. RapidOCR requires `opencv-python-headless>=4.5.1.48`.
The engine also selects `convert-core`, `format-office`, `format-pdf` and
`models-local`. The narrower optional OpenCV extra of docling-ibm-models is not
selected in the inverse tree. No root OpenCV override is configured.

Two required runtime paths hard-import cv2:

- Installed `rapidocr/main.py:10` imports `cv2`; `from rapidocr import RapidOCR`
  loads it. Our `ariad_docling/convert.py` configures its ONNX Runtime backend.
- Installed `docling_ibm_models/tableformer/data_management/tf_predictor.py:10`
  imports `cv2`. Docling's `table_structure_model.py:79` imports `TFPredictor`
  during initialization. Our PDF/image pipeline enables TableFormer even with
  OCR off.

The isolated `/home/vchun/Codes/t7-no-opencv-env` has the frozen engine
requirements with OpenCV omitted from the investigation export. The probe
asserts no OpenCV distribution and no discoverable cv2, then both feature
imports fail with `ModuleNotFoundError: No module named 'cv2'`. This proves the
hard-import branch of the brief applies; conversions/conformance/pack smoke are
not claimed to pass without cv2. Removing it would remove shipped OCR/table
features, so step 1 is required.

[Inverse tree](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-opencv-tree.log),
[isolated probe log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-no-opencv.log),
[probe script](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-no-opencv.py).

## OpenCV step 1: every compatible macOS wheel fails

The [official PyPI registry](https://pypi.org/project/opencv-python-headless/)
was enumerated for stable, non-yanked macOS arm64 wheels satisfying the active
RapidOCR range and CPython 3.14/macOS 14 compatibility. All **14 candidates**
contain positive native GPL declarations or GPL flags. No OpenCV downgrade or
runtime re-lock was made. Absence of a matching string alone is not a full
licence proof; the refusals below all have positive binary evidence.

| Version | Native objects inspected | GPL declaration/flag objects | Wheel SHA-256 |
| --- | ---: | ---: | --- |
| 5.0.0.93 | 100 | 8 | `030ca5e0837a2963ab36ef896baa9767eb8d2b83353fb28af5a521e40dd8756f` |
| 4.14.0.94 | 94 | 8 | `bc7db37dc234f7bb3190a158fd9dd750357246fc7d8adb23698843ef721a993b` |
| 4.13.0.92 | 94 | 8 | `1a7d040ac656c11b8c38677cc8cccdc149f98535089dbe5b081e80a4e5903209` |
| 4.13.0.90 | 94 | 8 | `12a28674f215542c9bf93338de1b5bffd76996d32da9acb9e739fdb9c8bbd738` |
| 4.12.0.88 | 67 | 5 | `1e58d664809b3350c1123484dd441e1667cd7bed3086db1b9ea1b6f6cb20b50e` |
| 4.11.0.86 | 66 | 5 | `48128188ade4a7e517237c8e1e11a9cdf5c282761473383e77beb875bb1e61ca` |
| 4.10.0.84 | 67 | 5 | `a4f4bcb07d8f8a7704d9c8564c224c8b064c63f430e95b61ac0bffaa374d330e` |
| 4.10.0.82 | 67 | 5 | `a09ed50ba21cc5bf5d436cb0e784ad09c692d6b1d1454252772f6c8f2c7b4088` |
| 4.9.0.80 | 64 | 5 | `e0ee54e27be493e8f7850847edae3128e18b540dac1d7b2e4001b8944e11e1c6` |
| 4.8.1.78 | 62 | 5 | `2c7d45721df9801c4dcd34683a15caa0e30f38b185263fec04a6eb274bc720f0` |
| 4.8.0.76 | 62 | 5 | `8ee3bf1c9086493c340c6a87899f1c7778d729de92bce8560b8c31ab8a9cdf79` |
| 4.7.0.72 | 62 | 5 | `6e13954f705e640e52962dfb252ee1f0a0af7b33cdbe8dd73f27e7e31172b7a2` |
| 4.6.0.66 | 56 | 6 | `ffbf26fcd697af996408440a93bc69c49c05a36845771f984156dfbeaa95d497` |
| 4.5.5.64 | 56 | 6 | `ca4f013fa958f60fb2327fe87e6127c1ac0ab536890b1d4b00847f417b7af1ba` |

Versions 4.8.0.74, 4.7.0.68, 4.5.5.62, 4.5.4.60, 4.5.4.58, 4.5.3.56,
4.5.2.54, 4.5.2.52 and 4.5.1.48 have no eligible non-yanked wheel for these
runtime tags. Older CPython-specific wheels are not treated as ABI3 wheels.
The declarations/configurations come from the native binaries, independently of
the carrying wheel's consolidated LGPL notice.

[Every candidate, object hash and binary declaration](/home/vchun/Codes/t7-rework8-opencv-review/macos-binary-review.json),
[registry snapshot](/home/vchun/Codes/t7-rework8-opencv-review/registry.json),
[inspection log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-opencv-macos.log).

## OpenCV step 2: source route implemented; native validation pending

The [toolchain workflow](../../.github/workflows/toolchain-wheels.yml) adds an
independent `opencv-macos` job on `macos-26` arm64; checksum collation waits for
it. It builds the existing locked OpenCV **5.0.0.93** source, SHA-256
`b82f9831daab90b725c7c1ee1b36cb5732c367096ac76d119e64e14eb70d5f3c`.
Options include `WITH_FFMPEG=OFF`, `BUILD_opencv_videoio=OFF`,
`WITH_AVFOUNDATION=OFF`, arm64 architecture and deployment floor 14.0, in
addition to existing IPP/ITT/vendor backend exclusions.

[Delocate 0.13.0](https://pypi.org/project/delocate/0.13.0/) repairs the complete
wheel, requires arm64/macOS 14 compatibility and sanitizes rpaths. Native probes
require a cv2 import, disabled IPP and absent `VideoCapture`; the streamed wheel
audit checks every native payload for forbidden markers. No GPL dylib is
removed. The later installed pack inventory still requires every native
companion's named notice review.

The source cache hashes actual output files. Shared `export_opencv()` writes the
actual output SHA-256, size and source/options/tool key in the existing schema-1
manifest consumed by checksum/pin rendering. `build.py` replaces macOS OpenCV
and writes its target-specific options into pack notices. Source fallback
refuses a non-native macOS arm64 host before compilation; a valid authoritative
release pin can still be fetched elsewhere. No output hash/pin is fabricated.
ARM PyTorch selection stays unchanged. `uv lock` regenerated the lock, adding
only macOS build tools delocate/macholib/altgraph; runtime pins are unchanged.

AGENTS Licensing, ARCHITECTURE §7.2/Decision Log and docs/docling.md record the
route and release prerequisite in the same source commit. Two regressions fail
before implementation; afterwards `test_pack.py` has **63 passed, 4 skipped**.
A third test checks exported bytes/hash/size/key. Ruff, actionlint, zizmor and
typos pass in the [focused log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-macos-focused-final.log);
[red proof](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-macos-source-red.log).

## Complete Linux inventory and binary census

The final run uses production `_native_files`, `_native_record_owners`,
`vendored_libraries` and `check_vendored_native_files` over each **entire**
extracted site, then per-file diagnostics collect every refusal. Owner metadata
results are memoized only within immutable inputs. The final harness asserts
two targets and zero global, site-level or per-file findings. No native bytes or
installed RECORD receipts are weakened.

| Target | Locked wheels | Native files | Named inventory files | Other licensed extensions | Findings |
| --- | ---: | ---: | ---: | ---: | ---: |
| x86_64-unknown-linux-gnu | 88 | 402 | 200 | 202 | 0 |
| aarch64-unknown-linux-gnu | 88 | 403 | 201 | 202 | 0 |

These conservative input supersets include official torch helpers and the
upstream x86 OpenCV wheel that the pack later replaces. They are not identical
to the final source-built pack fixture. All named files retain installed RECORD
SHA-256/size checks. Ordinary extension rows retain their checked owner
declarations/notices; that metadata is not used for a separate vendored project.

The [805-row CSV](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-linux-native-files.csv) and
[JSON](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-linux-native-files.json) list platform,
owner/version, exact path, native hash, wheel/hash, reviewed licence,
notice paths/hashes/scope and source pointers. No UNVERIFIED Linux row remains.
Artifact SHA-256 — CSV: `ed63c4fd44b55962c6104470dc7265e44a4367c37073dd6729df3df65f6f924a`; JSON: `c8426b623d1f262ba05f7c7bb1b02723bc912e35bcecba903d204ba582683c26`.

[Full production inventory items](/home/vchun/Codes/t7-rework8-linux-final/platform-summary.json).

Both Linux OpenCV wheels also underwent a binary census: five bundled FFmpeg
objects per architecture declare **LGPL version 2.1 or later**; none has GPL or
nonfree flags. Avutil version evidence and the exact locked wrapper source's
architecture-specific manylinux Dockerfiles agree on FFmpeg **8.1.1**
(`ARG FFMPEG_VERSION=8.1.1`). This is source-recipe evidence, not a dated image
digest or exact GCC compiler version.
[Declarations, configurations and object hashes](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-linux-opencv-binaries.json).

## Added library reviews and notices

[native-components.toml](../../engines/docling/pack/native-components.toml)
fixes exact owners/versions/paths and notice hashes. The CSV expands every path,
including torch's helpers. These are finite named reviews, not glob permissions.
Five newly copied texts had trailing whitespace/EOF blanks normalized; all
copyright, conditions and disclaimers remain. Original downloads are preserved.

| Named review | Carrying distribution | Licence | Full notice source bundled |
| --- | --- | --- | --- |
| `onnxruntime-native` (4 paths) | onnxruntime 1.31.0 | MIT | `onnxruntime/LICENSE` in wheel; `onnxruntime/ThirdPartyNotices.txt` in wheel |
| `torch-native` (142 paths) | torch 2.14.1+cpu | Apache-2.0 AND Apache-2.0 WITH LLVM-exception AND BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND MIT | [pytorch-2.14.1-native-notices.txt](../../engines/docling/pack/licenses/pytorch-2.14.1-native-notices.txt); [llvm-openmp-notice.txt](../../engines/docling/pack/licenses/llvm-openmp-notice.txt) |
| `opencv-linux-aom` (2 paths) | opencv-python-headless 5.0.0.93 | BSD-2-Clause | [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `opencv-linux-avif` (2 paths) | opencv-python-headless 5.0.0.93 | BSD-2-Clause AND BSD-3-Clause | [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `opencv-linux-png` (2 paths) | opencv-python-headless 5.0.0.93 | libpng-2.0 | [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `opencv-linux-vpx` (2 paths) | opencv-python-headless 5.0.0.93 | BSD-3-Clause | [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `opencv-linux-openssl` (4 paths) | opencv-python-headless 5.0.0.93 | OpenSSL | [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `opencv-linux-drm` (2 paths) | opencv-python-headless 5.0.0.93 | MIT | [libdrm-core-notices.txt](../../engines/docling/pack/licenses/libdrm-core-notices.txt); [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `opencv-linux-openblas` (2 paths) | opencv-python-headless 5.0.0.93 | BSD-3-Clause | [openblas-0.3.15.txt](../../engines/docling/pack/licenses/openblas-0.3.15.txt); [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `opencv-linux-ffmpeg` (10 paths) | opencv-python-headless 5.0.0.93 | LGPL-2.1-or-later | [lgpl-2.1.txt](../../engines/docling/pack/licenses/lgpl-2.1.txt); [ffmpeg-8.1.1-notice.txt](../../engines/docling/pack/licenses/ffmpeg-8.1.1-notice.txt); [opencv-5.0.0.93-linux-notices.txt](../../engines/docling/pack/licenses/opencv-5.0.0.93-linux-notices.txt) |
| `torch-arm-compute` (2 paths) | torch 2.14.1+cpu | MIT AND Apache-2.0 AND BSD-3-Clause | [arm-compute-native-notice.txt](../../engines/docling/pack/licenses/arm-compute-native-notice.txt) |
| `torch-openblas` (1 path) | torch 2.14.1+cpu | BSD-3-Clause | [openblas-0.3.34.txt](../../engines/docling/pack/licenses/openblas-0.3.34.txt) |
| `torchvision-glibc-loader` (1 path) | torchvision 0.29.1+cpu | LGPL-2.1-or-later | [lgpl-2.1.txt](../../engines/docling/pack/licenses/lgpl-2.1.txt); [glibc-2.28-loader-notice.txt](../../engines/docling/pack/licenses/glibc-2.28-loader-notice.txt); [glibc-2.28-notices.txt](../../engines/docling/pack/licenses/glibc-2.28-notices.txt) |
| Additional `libgfortran` paths | Locked NumPy/SciPy/OpenCV/PyTorch | GPL-3.0-or-later WITH GCC-exception-3.1 | Wheel notices, full GNU texts and source/offer sidecars |
| Additional OpenCV `libquadmath` path | OpenCV 5.0.0.93 | LGPL-2.1-or-later | Wheel notice, full LGPL-2.1 and source/offer sidecars |
| Additional `libgomp` paths | PyTorch 2.14.1+cpu | GPL-3.0-or-later WITH GCC-exception-3.1 | Wheel notice, full GNU texts and source/offer sidecars |

OpenCV's full notice covers AOM, AVIF/dav1d, PNG, VPX and OpenSSL. The libdrm
sidecar supplies missing core copyright/conditions from upstream source; SONAME
2.4.0 is not misreported as an exact source release. OpenBLAS 0.3.15/0.3.34 have
their full release notices. PyTorch's combined notice preserves its top-level
text plus **23 submodule notices** fetched at the v2.14.1 tree's exact gitlinks,
plus LLVM OpenMP's full exception notice. Arm Compute 53.2.0 identifies build
commit `7b256bb7965f2fd99cdee790a4b0e56dab438a8c`; its sidecar bundles all three
licence texts and component copyright declarations. ONNX Runtime's existing
full notices now cover both ABI extension paths. Existing NumPy BSD development
archives/CC0 metadata, PDFium, Pillow, GEOS/Rtree and TorchVision reviews keep
their prior constraints. No new static/copyleft allowance was added.

FFmpeg 8.1.1 binds ten original ELF shared-object paths to reviewed hashes and
its [exact source tarball](https://ffmpeg.org/releases/ffmpeg-8.1.1.tar.gz), with
full LGPL-2.1 and copyright notices. TorchVision's ARM dynamic loader identifies
glibc 2.28, is ELF/hash-bound and carries full LGPL-2.1, loader copyright, glibc
notices and [exact source](https://ftp.gnu.org/gnu/libc/glibc-2.28.tar.xz).
LGPL/MPL native components now require a shared-object review map; missing maps,
wrong sources/versions, changed bytes or static format fail. GPL/AGPL binary
self-declarations and GPL flags fail installed and streamed-wheel checks.

Thirteen Linux regression cases pass. Initial failures are preserved in
[entry/format red](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-native-red.log),
[wheel GPL red](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-wheel-gpl-red.log) and
[focused green](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-linux-focused-final.log).

The final notice audit also found the newly copied common OpenCV sidecar still
contained the inapplicable x86-only IPP section. The unchanged final vendor gate
rejects it; the new sidecar test reproduces that failure. The existing reviewed
`remove_inapplicable_ipp_notice()` operation removed exactly that section, and
all other sections compare byte-for-byte unchanged. All eight sidecar hash
references were updated. This removes no wheel file or native byte and grants
no proprietary/GPL exception. The ARM wheel notice explicitly scopes this section to x86; the source-built
x86 route disables IPP. No new binary or native file was removed. [Transformation receipt](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-ipp-notice-transform.json),
[red](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-sidecar-red.log),
[71-test final green](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-notices-final-green.log).

A second real-input check found the glibc notice retained its 15 original
form-feed separators while the inventory digest described a normalized variant.
The file and every copyright/condition stay intact; the declaration now hashes
those exact retained bytes. A regression validates all **14 upstream sidecar
digests**, preventing another platform-only mismatch.
[Red proof](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-notice-digests-red.log),
[complete digest receipt](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-upstream-notice-digests.json).

## GNU provenance: ABI evidence only

Nine additional target/distribution/path hashes cover Linux wheel variants;
the existing seven source-pack bindings remain unchanged. Raw official Linux
inputs contain **15 GNU objects**. All require reviewed SHA-256 and ELF shared
object magic/ET_DYN; modified, static or unlisted objects remain refused.
Full GPL-3.0, GCC Runtime Library Exception 3.1, LGPL-2.1, notices, source/build
provenance and a written three-year corresponding-source offer are bundled.
The [source-pointer sidecar](../../engines/docling/pack/licenses/gnu-runtime-source-pointers.txt)
records each carrying wheel's exact hash and publisher URL.

| Target | Exact object path | Lowest evidenced series | Defining ABI tags |
| --- | --- | --- | --- |
| x86_64 | `numpy.libs/libgfortran-83c28eba-468e71e5.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| x86_64 | `numpy.libs/libquadmath-2284e583-a9307bba.so.0.0.0` | GCC 6 ABI | `QUADMATH_1.0`, `QUADMATH_1.1` |
| x86_64 | `opencv_python_headless.libs/libgfortran-83c28eba.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| x86_64 | `opencv_python_headless.libs/libquadmath-2284e583.so.0.0.0` | GCC 6 ABI | `QUADMATH_1.0`, `QUADMATH_1.1` |
| x86_64 | `scipy.libs/libgfortran-040039e1-0352e75f.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| x86_64 | `scipy.libs/libgfortran-83c28eba.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| x86_64 | `scipy.libs/libquadmath-2284e583.so.0.0.0` | GCC 6 ABI | `QUADMATH_1.0`, `QUADMATH_1.1` |
| x86_64 | `scipy.libs/libquadmath-96973f99-934c22de.so.0.0.0` | GCC 4 ABI | `QUADMATH_1.0` |
| x86_64 | `torch/lib/libgomp.so.1` | GCC 9 ABI | `GOMP_5.0`, `OMP_5.0` |
| aarch64 | `numpy.libs/libgfortran-e1b7dfc8-d8198c01.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| aarch64 | `opencv_python_headless.libs/libgfortran-e1b7dfc8.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| aarch64 | `scipy.libs/libgfortran-daac5196-038a5e3c.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| aarch64 | `scipy.libs/libgfortran-e1b7dfc8.so.5.0.0` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| aarch64 | `torch/lib/libgfortran.so.5` | GCC 8 ABI | `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8` |
| aarch64 | `torch/lib/libgomp.so.1` | GCC 13 ABI | `GOMP_5.0`, `GOMP_5.1.1`, `GOMP_PLUGIN_1.4`, `OMP_5.0`, `OMP_5.2` |

Both `readelf --version-info` and `objdump -T` were run for every object.
[Complete tags/hashes/receipts](/home/vchun/Codes/t7-rework8-sources/gnu/objects.json)
and sibling `.version-info.txt`/`.symbols.txt` files preserve the evidence.
The series is an ABI baseline, **not an exact GCC source/compiler version**.
Imported GCC/GLIBC requirements identify required ABIs only. The upstream source
pointer is [the GCC archive](https://ftp.gnu.org/gnu/gcc/); source maps establish
GCC 8 for GFORTRAN_8, GCC 6/4.6 for QUADMATH_1.1/1.0, GCC 9 for GOMP/OMP_5.0,
and GCC 13 for ARM libgomp's OMP_5.2/GOMP_5.1.1/plugin_1.4 interfaces.

NumPy/SciPy provenance uses release-tag cibuildwheel configuration in
`pyproject.toml`; PyTorch uses v2.14.1 binary workflow and manywheel Dockerfile
URLs. OpenCV's exact PyPI sdist supplies its manylinux Dockerfiles because the
presumed GitHub tag URL does not exist. Those recipes select floating manylinux
images; they do not prove dated image digests or exact runtime source releases.
The A-0700 offer covers corresponding source for the actual shipped bytes.
Architecture/Decision Log record the extended rule in the Linux source commit.

## Exact-head gate

Final line: `=== PACK GATE ALL GREEN HEAD=8b84f0ab8282e82e165e41144dd8a70b229c2617`.

Detached `gate-slot.sh 1`, nice 19, one
Cargo/test/BLAS/Rayon job and isolated Python environments. No holder was paused.
The full Rust matrix/`just ci` remains delegated to GitHub CI as instructed.

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 bash /tmp/t7-rework8-final-pack-gate.sh
```

| Check | Final result |
| --- | --- |
| CI scope / cloud classifier | 76 passed / expected true |
| Five focused suites / all Docling tests | 205 passed / 299 passed, 4 existing platform skips |
| Three real pywin32 wheels and uv installations | Passed; altered source and forged installed RECORD refused |
| CP1252 / C-locale probes | 5 passed each |
| Actual source-built Linux fixture | 256 native files; 54 named, 202 other extensions; seven GNU hashes/ELF checks pass |
| Full locked Linux input inventories | x86: 402 files, 200 named; ARM: 403 files, 201 named; zero findings |
| Linux OpenCV binary census | 31 objects; ten FFmpeg objects declare LGPL-2.1-or-later, no GPL/nonfree flags |
| Complete ARM torch archive scan | 146 native files detected; this is not a native ARM execution test |
| Real ZIP/model payloads and wheel receipts | 148 payloads accepted; 216 locked wheel hashes match |
| uv lock --check / uv audit --frozen | Passed; no known vulnerabilities/adverse statuses in 112 audited packages |
| Explicit UTF-8 audit / Ruff | 19 files, no implicit encodings / check and format pass for 10 modules |
| Current-source CLI build / bench tests | Passed / 75 passed |
| All fixture generators | Two runs byte-identical; 135 stored files and manifest SHA-256 values match |
| actionlint / zizmor / typos / just lint-commits | Passed; zizmor has no unsuppressed findings |
| Tracked diff / exact source-head assertion | Passed |

[Gate log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-final-gate.log). The script and helpers
preserved beside it contain every command/environment variable.
The [report closure log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-report-closure.log)
records the report-only head and verifies its parent is this tested source. The first run
stopped on a diagnostic count assumption (53 named files). Comparing the actual
fixture proves two newly named extensions (torch/_C and ONNX Runtime), so the
correct count is **54 named / 202 other extensions / 256 total**. The helper was
corrected. The next run was cancelled cleanly with SIGTERM to fix the separately
reproduced sidecar issue above. The final gate starts afresh at the new source
head. A third run was cancelled to correct the separately reproduced glibc
notice digest mismatch. No product check or test was weakened; all three earlier
logs are preserved, and the final run uses the corrected committed source.

## Remote CI and remaining work

At 2026-10-28 10:12:27 UTC,
[run 40644845744](https://github.com/bavanchun/AriadShift/actions/runs/40644845744)
has the x86_64 pack job **in_progress** and **zero artifacts**. Its remote head
predates this local source; no real CI pack artifact is available to inventory.
[Job JSON](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-ci-run-latest.json),
[artifact JSON](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework8-ci-artifacts-latest.json).

Remaining: execute the new source job on native macOS arm64, review its actual
dylib notices/inventory and conversion/smoke results, generate/pin the real wheel
SHA-256, and run the full macOS pack gate. The previous raw macOS census has
other unreviewed native files; replacing OpenCV alone is not claimed to resolve
them. The mandatory Linux native review is complete. This task does not select
a Linux-only release or new macOS pack tag. No further Q asks for GPL permission;
a target-drop escalation is needed only if the source route cannot finish.
The required full gate completed before machine load rose above 34 from other
jobs. An additional optional ARM OpenCV vendor audit was cancelled while queued
for the shared slot; no result is claimed for that optional rerun. Report-only
spelling/commit/diff checks are lightweight and use nice 19.
No push or workflow dispatch. AgentWiki publish skipped.

Status: BLOCKED
