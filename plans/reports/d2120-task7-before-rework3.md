# Docling pack gates — final honesty pass

Final HEAD: `7cf3e7ecdd6c64ca88ba61558e83a0dbe7751cf2`. Branch: `fix/pack-prebuild-gates`.
Six local commits remain; no push, merge, tag or publication occurred.
All four findings in `r-task7-delta-terra.md` are repaired under the brief's
CI threat model. A-0100, A-0245 and A-0700 remain implemented with no pending
owner answer. This report supersedes the earlier bf1481d acceptance report,
preserved as `d2120-t7-before-rework2-report.md` beside the gate logs.

## Accepted review and final small change

The third independent review, `r-task7-delta2-terra.md`, accepted de8511d with
three lows. This pass narrows the module threat-model docstring, AGENTS.md,
ARCHITECTURE.md and its Decision Log to the actual notice-presence and RECORD
refresh limits documented below. The build helper docstring now says it
rehashes all DLL/PYD rows. No refresh or notice-content behavior was expanded.
The wait for other gates was handled without pausing any lock holder.

A four-line ZIP local-header check closes the renamed .bin/extensionless ZIP
case for non-native files, with explicit NPZ/DOCX/PPTX/XLSX data exemptions.
Before repair, the new focused regressions gave **3 failed, 4 passed**:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-honesty-zip-proof.log` (expected rc=1).
The real-fixture signature census found only 20 NPZ, one DOCX and one PPTX.
The seven added tests cover three renamed ZIP refusals and four valid data
suffixes, including case-insensitive PPTX. The final gate includes all 39
integrity cases in its 150-test focused run. The earlier separately queued focused runner was
cancelled before acquiring its slot; it produced no test result.
The final gate repeats the real census and inventory at the exact new head.

All small edits are in amended commit 4. The two following commits were replayed
in their existing order, preserving the approved GEOS/GNU entries and texts.
Only documentation conflict resolution was needed; there are still six commits
and the GNU source-pointer commit is last. The previous accepted report and
its gate remain preserved as historical evidence beside the logs.

## Threat model and repaired boundary

CI builds from wheel bytes fixed by uv.lock hashes and a hash-pinned Python
standalone archive. The gate catches accidental licensing mistakes after a
dependency or lock change: a new native library, forbidden licence hidden by
permissive metadata, or missing notice. It does not defend against malicious
wheels with forged RECORD files. This is explicit in the module docstring,
ARCHITECTURE.md and its Decision Log; no adversarial binary analysis was added.

1. Every native file must match its RECORD SHA-256 and size, with exactly one
   licensed owner. Only ABI-tagged extensions, .pyd modules, Mach-O extension
   bundles, and explicitly listed native components may pass. A plain .so,
   extensionless ELF or library in RECORD alone is insufficient. Family matching
   uses a case-insensitive normalized basename in any directory, including
   .data/platlib. Native hard links and symlinks are refused.
2. Every native owner needs a nonempty bundled dist-info licence/notice or an
   explicit reviewed inventory notice source. Both the native checker and
   `write_notices` enforce presence. Default discovery accepts any nonempty file
   under dist-info/licenses without validating its content. Explicit reviewed
   inventory sources are independently hash-checked. Accepted files are copied
   in full. ONNX Runtime's reviewed LICENSE and ThirdPartyNotices cover its same-owner extension too.
   Tokenizers 0.23.3 has no wheel notice, so its matching upstream Apache-2.0
   licence is now bundled through a version-bound, hash-verified inventory entry.
3. Directory symlinks and all escaping symlinks fail anywhere in the site scan.
   .zip/.egg/.whl files and .pth archive references fail, including references
   outside the tree. ZIP local-header magic (PK\x03\x04) also fails for non-native
   files except NPZ/DOCX/PPTX/XLSX data formats. Data archive contents and other
   archive magic are not inspected. Ordinary in-tree text links remain outside
   the prohibition.
4. Present-but-empty License, License-Expression and License-File-style headers
   fail through `check` and `pack_expression`; missing headers remain distinct.
   Prior unknown-OR, inconsistent-field, duplicate and classifier-case repairs
   remain in place.

The named GNU and GEOS byte/source/notice safeguards are unchanged. RECORD
refresh rewrites digests and sizes for every existing DLL/PYD record and
LICENSE-3RD-PARTY.txt, not only intentionally replaced files. It can therefore
mask accidental post-install changes on Windows; that target remains deferred.
The final native gate checks the refreshed installed bytes. Narrowing the
refresh and adding a Windows pipeline regression remain prerequisites to
that target's release.

## Real-input problems caught by the stronger gate

The first full run at 8caef69 passed 139 focused tests and 230 full-suite tests
with four skips, then stopped at a pip launcher RECORD mismatch. The six pip
Windows launcher bytes and their stale RECORD declarations were compared with
the original SHA-verified Python standalone archive. The bytes match that
archive exactly; its recorded digests are stale. Diagnostic log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-standalone-record-proof.log`.
Last line: `=== Upstream stale launcher records confirmed ALL GREEN`.

Pinned archive:
`https://github.com/astral-sh/python-build-standalone/releases/download/20261021/cpython-3.14.9+20261021-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz`,
SHA-256 `561e91c1cadf30e84aec94d1917384ae3267e5f3e6e135e51fd1578bb19f4064`.

The production builder now omits unused Windows launcher resources from
pip/setuptools in Unix packs and removes their RECORD rows. It runs that
cleanup during runtime pruning and again after final wheel installation.
Windows resources and the Python packages themselves are preserved by the
platform-specific rule. No native digest was rewritten to force acceptance.
The actual Unix fixture consequently drops 14 foreign executables, from 270
native files to 256. The named component inventory no longer needs launcher
exceptions. Cleanup regressions proved Linux/macOS failing and Windows passing
before repair (2 failed, 1 passed) in `d2120-t7-launcher-prune-proof.log`.

The next full run at ccce266 passed 142 focused tests and 233 whole-suite tests
with four skips, then correctly stopped at missing ONNX/tokenizers owner
notices. The owner-notice regression separately failed at that SHA in
`d2120-t7-owner-notice-proof.log`. Those failing runs remain visible; neither
is the accepted final gate.

## Tests first and final gate

Initial second-review regressions at bf1481d: **25 failed, 1 passed**, covering
RECORD hash/size, unknown native companions, case/.data, hard links, missing and
empty notices, links/archives/.pth and empty metadata. Log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework2-proof.log`.
Later positive regressions cover Mach-O bundles, explicit external/upstream
notices and Unix launcher pruning. Existing fixtures now supply valid RECORD
hashes so GNU/GEOS mutation tests still reach their independent object checks.

Final gate ran detached through `gate-slot.sh 2`, nice 19, with one
Cargo/test/BLAS/Rayon job. It used the clean verification worktree
`/home/vchun/Codes/01-AriadShift-wt-t7-pack-verify` at the explicit local SHA
after the runner's preliminary origin/dev checkout. The load guard delayed
execution while the host was busy. The slot-1 queue was cancelled before
acquiring its lock when slot 2 became free; no slot holder was paused.

Log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-honesty-final-gate.log`.
Last line: `=== PACK GATE ALL GREEN HEAD=7cf3e7ecdd6c64ca88ba61558e83a0dbe7751cf2`.

| Check at final HEAD | Result |
|---|---|
| Focused metadata/native inventory tests | 150 passed |
| Whole engines/docling/tests | 241 passed, 4 existing conditional skips |
| Real native inventory after production Unix pruning | 256 accounted for; 52 named inventory files, 204 extension modules |
| Native RECORD integrity and ownership | SHA-256/size and unique licensed owner for every file |
| Native owner notices | All default or reviewed sources present, nonempty and bundled |
| Real-fixture ZIP signatures | 22 legitimate data files (20 NPZ, one DOCX, one PPTX); no false positives |
| Seven named GNU objects | Reviewed SHA-256 and ELF shared-object checks passed |
| uv lock --check | Passed |
| uv audit --frozen | No known vulnerabilities or adverse project statuses |
| Ruff check/format, typos, just lint-commits | Passed |
| Verification worktree git diff --exit-code | Passed |

Launcher: `zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 /bin/bash /tmp/t7-launch-honesty-gate.sh`, detached with `setsid nohup` and redirected to the final gate log.

Exact commands in the preserved final driver:

```sh
uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pack_inventory_integrity.py engines/docling/tests/test_pack_gate_validation.py engines/docling/tests/test_pack_license_gates.py engines/docling/tests/test_licenses.py
uv run --package ariad-docling --frozen pytest engines/docling/tests
uv run --package ariad-docling --frozen python /tmp/t7-honesty-total-native-inventory.py
uv run --package ariad-docling --frozen python /tmp/t7-honesty-zip-census.py
uv lock --check
uv audit --frozen
/home/vchun/Codes/t7-pack-python-env/bin/ruff check engines/docling/pack/licenses.py engines/docling/pack/build.py engines/docling/tests/test_pack_license_gates.py engines/docling/tests/test_pack_gate_validation.py engines/docling/tests/test_licenses.py engines/docling/tests/test_pack_inventory_integrity.py engines/docling/tests/conftest.py
/home/vchun/Codes/t7-pack-python-env/bin/ruff format --check engines/docling/pack/licenses.py engines/docling/pack/build.py engines/docling/tests/test_pack_license_gates.py engines/docling/tests/test_pack_gate_validation.py engines/docling/tests/test_licenses.py engines/docling/tests/test_pack_inventory_integrity.py engines/docling/tests/conftest.py
typos
just lint-commits
git diff --exit-code
```

Full Rust just ci remains excluded by the lead's pack-only gate instruction.
The shell driver, inventory helper, machine inventory and source diagnostic are
preserved beside the logs with the `d2120-t7-` prefix. Full generated notices:
`/home/vchun/Codes/t7-native-inventory/notices-honesty`.
Machine inventory with paths, classifications, owners, licences and hashes:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-honesty-total-native-inventory.json`.

## Six commits and preserved decisions

1. `20a40fde1b926af2a2053f15b7c0fcf547041ac0` — fix(docling): fail closed on pack licences and notices.
2. `508ef10cdadd82d61d722da995c850c06c4cb73c` — build(docling): allow permissive Pillow codec licences.
3. `b78530f9e8ed3ed7c87255021d392d295cb999e3` — build(deps): update Pillow to 13.0.0.
4. `f4a5ce085b833e780c977957e781467f0f90ed35` — fix(docling): validate all metadata and native payloads.
5. `162f5a8f0570f75cbdcde792a460231817a674d0` — build(docling): allow GEOS and libspatialindex notices.
6. `7cf3e7ecdd6c64ca88ba61558e83a0dbe7751cf2` — build(docling): record GNU runtime source pointers.

The first three SHAs are unchanged. The mechanism commit owns the second
rework, with fixture corrections in their owning mechanism/GEOS commits.
GEOS and GNU remain separate and last, in that order; the GNU subject remains
exactly `build(docling): record GNU runtime source pointers`. The local owning
commit was amended and the later commits replayed without rebasing.

AST comparisons confirmed unchanged approved native licence/notice constants,
seven GNU hashes, 14 GEOS/Rtree hashes and exact source pointers. The latest
comparison is `d2120-t7-honesty-preserved-check.log` beside the gate log. All six
approved GNU/GEOS legal/source notice files are byte-identical to bf1481d.
No seventh report commit was added; this report and the local plan stay local.

## Explicit native companions and notice sources

Of 256 native files, 35 retain the existing reviewed library inventory; 17
additional files now have explicit entries in six distribution groups in
`engines/docling/pack/native-components.toml`. No broad RECORD-only inheritance
remains. Every other native file is an extension module. Each group is bound
to its reviewed distribution version and notice digest(s):

### tokenizers-native — Apache-2.0

Owner/version: `tokenizers 0.23.3`.

- `tokenizers/tokenizers.abi3.so`

Notice sources: `tokenizers-0.23.3.txt`.

### numpy-native — BSD-3-Clause

Owner/version: `numpy 2.5.4`.

- `numpy/_core/lib/libnpymath.a`
- `numpy/random/lib/libnpyrandom.a`

Notice sources: `numpy-2.5.4.dist-info/licenses/LICENSE.txt`, `numpy-2.5.4.dist-info/licenses/numpy/random/LICENSE.md`.

### onnxruntime-native — MIT

Owner/version: `onnxruntime 1.31.0`.

- `onnxruntime/capi/libonnxruntime.so.1.31.0`
- `onnxruntime/capi/libonnxruntime_providers_shared.so`

Notice sources: `onnxruntime/LICENSE`, `onnxruntime/ThirdPartyNotices.txt`.

### pypdfium2-native — Apache-2.0 AND BSD-3-Clause

Owner/version: `pypdfium2 5.14.0`.

- `pypdfium2_raw/libpdfium.so`

Notice sources: `pypdfium2-5.14.0.dist-info/licenses/LICENSES/Apache-2.0.txt`, `pypdfium2-5.14.0.dist-info/licenses/LICENSES/BSD-3-Clause.txt`.

### torch-native — Apache-2.0 AND Apache-2.0 WITH LLVM-exception AND BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND MIT

Owner/version: `torch 2.14.1+cpu`.

- `torch/bin/protoc`
- `torch/bin/protoc-3.21.12.0`
- `torch/bin/torch_shm_manager`
- `torch/lib/libc10.so`
- `torch/lib/libshm.so`
- `torch/lib/libtorch.so`
- `torch/lib/libtorch_cpu.so`
- `torch/lib/libtorch_global_deps.so`
- `torch/lib/libtorch_python.so`

Notice sources: `torch-2.14.1+cpu.dist-info/licenses/LICENSE`.

### torchvision-native — BSD-3-Clause

Owner/version: `torchvision 0.29.1+cpu`.

- `torchvision/_C_stable.so`
- `torchvision/image_stable.so`

Notice sources: `torchvision-0.29.1+cpu.dist-info/LICENSE`.

Tokenizers' complete upstream licence was fetched from
[v0.23.3/LICENSE](https://raw.githubusercontent.com/huggingface/tokenizers/v0.23.3/LICENSE),
SHA-256 `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4`,
and is bundled as `engines/docling/pack/licenses/tokenizers-0.23.3.txt`.
The two NumPy archives were already in the pack and are explicitly listed
under BSD-3-Clause. No weak-copyleft static component is permitted.
Complete dist-info notice trees are copied in addition to these named sources.

## Approved native licence entries retained

| Library | Distribution(s) | Chosen SPDX licence |
|---|---|---|
| `libgeos_c` | shapely | `LGPL-2.1-only` |
| `libgeos` | shapely | `LGPL-2.1-only` |
| `libspatialindex` | rtree | `MIT` |
| `libgomp` | torch | `GPL-3.0-or-later WITH GCC-exception-3.1` |
| `libgfortran` | numpy, opencv-python-headless, scipy | `GPL-3.0-or-later WITH GCC-exception-3.1` |
| `libquadmath` | numpy, opencv-python-headless, scipy | `LGPL-2.1-or-later` |
| `libscipy_openblas` | numpy, scipy | `BSD-3-Clause` |
| `libopenblas` | opencv-python-headless | `BSD-3-Clause` |
| `libXau` | pillow | `MIT` |
| `libavif` | pillow | `BSD-2-Clause` |
| `libbrotlicommon` | pillow | `MIT` |
| `libbrotlidec` | pillow | `MIT` |
| `libfreetype` | pillow | `FTL` |
| `libjpeg` | pillow, torchvision | `BSD-3-Clause AND IJG AND Zlib` |
| `libpng16` | pillow, torchvision | `libpng-2.0` |
| `libtiff` | pillow | `libtiff` |
| `libharfbuzz` | pillow | `MIT` |
| `liblcms2` | pillow | `MIT` |
| `liblzma` | pillow | `0BSD` |
| `libopenjp2` | pillow | `BSD-2-Clause` |
| `libsharpyuv` | pillow, torchvision | `BSD-3-Clause` |
| `libwebp` | pillow, torchvision | `BSD-3-Clause` |
| `libwebpdemux` | pillow | `BSD-3-Clause` |
| `libwebpmux` | pillow | `BSD-3-Clause` |
| `libxcb` | pillow | `MIT` |
| `libzstd` | pillow | `BSD-3-Clause` |
| `libz` | pillow, torchvision | `Zlib` |

## GNU wheel provenance and source offer

A-0700 supersedes the original exact-GCC-version stop. All six NumPy/SciPy
objects match the SHA-verified locked wheels byte for byte. Torch libgomp
matches its reviewed native-cache wheel. Carrying wheels:

| Wheel | SHA-256 |
|---|---|
| `numpy-2.5.4-cp314-cp314-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl` | `d549420b8858885cea8838a727842249218b9c1da24dd517e25c9c7a948310a3` |
| `scipy-1.18.1-cp314-cp314-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl` | `ac0333bdf38309aa3dcbe7e3fa7ea29e7a2c37c6ea306a757b700ded8e4596ad` |
| `torch-2.14.1+cpu-cp314-cp314-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl` | `ded7771f615fb7207a90904849a4972b747b7b1aff201e4e583bd55eef19d7ed` |

Verified upstream release-tag build configurations:

- [NumPy v2.5.4](https://github.com/numpy/numpy/tree/v2.5.4),
  [cibuildwheel config](https://github.com/numpy/numpy/blob/v2.5.4/pyproject.toml):
  manylinux_2_28 for Linux x86_64 and the before-build hook.
- [SciPy v1.18.1](https://github.com/scipy/scipy/tree/v1.18.1),
  [cibuildwheel config](https://github.com/scipy/scipy/blob/v1.18.1/pyproject.toml):
  manylinux_2_28 for Linux x86_64 and the before-build hook.
- [PyTorch v2.14.1](https://github.com/pytorch/pytorch/tree/v2.14.1),
  [Linux wheel workflow](https://github.com/pytorch/pytorch/blob/v2.14.1/.github/workflows/_binary-build-linux.yml)
  and [manywheel Dockerfile](https://github.com/pytorch/pytorch/blob/v2.14.1/.ci/docker/manywheel/Dockerfile_2_28):
  a manylinux_2_28_x86_64 base stage, AlmaLinux 8 common stage and default GCC
  toolset 13. These pointers do not prove which container built the local
  cached wheel or which GCC source supplied its copied libgomp. Its local build
  record names PyTorch v2.14.1 source and OpenMP/OpenBLAS/no-MKL options, not
  per-runtime source versions.

Every GNU entry includes the [GCC archive index](https://ftp.gnu.org/gnu/gcc/)
and the following evidence. Raw `readelf --version-info <object>` and
`objdump -T <object>` output:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-honesty-gnu-symbol-tags.log`.
Defined tags establish upstream ABI baselines only. Required GCC/GLIBC tags
identify needed interfaces, not the importing library's source version.
No exact compiler/source release is claimed. All `.comment` sections are
absent; Torch's annobin 8.5.0 annotation is not treated as its source release.

### `numpy.libs/libgfortran-83c28eba-468e71e5.so.5.0.0`

SHA-256: `a6a368328d02a5d8a149746f13f6268f8cc9192df5c18777b25c3c9b5ef52160`.
Lowest evidenced upstream ABI series: **GCC 8**.
Defined tags: `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8`.
Required tags: `GCC_3.0`, `GCC_3.3`, `GCC_4.2.0`, `GCC_4.3.0`, `GLIBC_2.14`, `GLIBC_2.17`, `GLIBC_2.2.5`, `GLIBC_2.27`, `GLIBC_2.3`, `GLIBC_2.4`, `GLIBC_2.6`, `GLIBC_2.7`, `QUADMATH_1.0`.

### `numpy.libs/libquadmath-2284e583-a9307bba.so.0.0.0`

SHA-256: `6bc3069003caec1f075be3c51ae8355332f081fa9465910c4b5724e7fe7d8ad4`.
Lowest evidenced upstream ABI series: **GCC 6**.
Defined tags: `QUADMATH_1.0`, `QUADMATH_1.1`.
Required tags: `GLIBC_2.10`, `GLIBC_2.14`, `GLIBC_2.2.5`, `GLIBC_2.23`, `GLIBC_2.3`, `GLIBC_2.4`.

### `scipy.libs/libgfortran-040039e1-0352e75f.so.5.0.0`

SHA-256: `c6090048eccc763522c12ef016f81da6b627cb3a044f55cf0479a839c41c0980`.
Lowest evidenced upstream ABI series: **GCC 8**.
Defined tags: `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8`.
Required tags: `GCC_3.0`, `GCC_3.3`, `GCC_4.2.0`, `GCC_4.3.0`, `GCC_4.8.0`, `GLIBC_2.14`, `GLIBC_2.17`, `GLIBC_2.2.5`, `GLIBC_2.3`, `GLIBC_2.4`, `GLIBC_2.6`, `GLIBC_2.7`, `QUADMATH_1.0`.

### `scipy.libs/libgfortran-83c28eba.so.5.0.0`

SHA-256: `468e71e59e1d784821a76702e5ecd1e0dbcfc024499f74a552d1bea4b26f27ad`.
Lowest evidenced upstream ABI series: **GCC 8**.
Defined tags: `GFORTRAN_8`, `GFORTRAN_C99_8`, `GFORTRAN_F2C_8`.
Required tags: `GCC_3.0`, `GCC_3.3`, `GCC_4.2.0`, `GCC_4.3.0`, `GLIBC_2.14`, `GLIBC_2.17`, `GLIBC_2.2.5`, `GLIBC_2.27`, `GLIBC_2.3`, `GLIBC_2.4`, `GLIBC_2.6`, `GLIBC_2.7`, `QUADMATH_1.0`.

### `scipy.libs/libquadmath-2284e583.so.0.0.0`

SHA-256: `a9307bbaa353623fa014e8b682d50c4629f5e5c7d61daa9a8652f7d165cd421a`.
Lowest evidenced upstream ABI series: **GCC 6**.
Defined tags: `QUADMATH_1.0`, `QUADMATH_1.1`.
Required tags: `GLIBC_2.10`, `GLIBC_2.14`, `GLIBC_2.2.5`, `GLIBC_2.23`, `GLIBC_2.3`, `GLIBC_2.4`.

### `scipy.libs/libquadmath-96973f99-934c22de.so.0.0.0`

SHA-256: `6ed5137f412781ad7863439fb543613f620b43c32b63292a0029246162f5bbc6`.
Lowest evidenced upstream ABI series: **GCC 4.6**.
Defined tags: `QUADMATH_1.0`.
Required tags: `GLIBC_2.10`, `GLIBC_2.14`, `GLIBC_2.2.5`, `GLIBC_2.3`, `GLIBC_2.4`.

### `torch.libs/libgomp-6a29c32d.so.1.0.0`

SHA-256: `2aa037784caec5079de4f23a7d526a62bcc03a3fb98ac94031345daabe21c465`.
Lowest evidenced upstream ABI series: **GCC 9**.
Defined tags: `GOACC_2.0`, `GOACC_2.0.1`, `GOMP_1.0`, `GOMP_2.0`, `GOMP_3.0`, `GOMP_4.0`, `GOMP_4.0.1`, `GOMP_4.5`, `GOMP_5.0`, `GOMP_PLUGIN_1.0`, `GOMP_PLUGIN_1.1`, `GOMP_PLUGIN_1.2`, `OACC_2.0`, `OACC_2.0.1`, `OACC_2.5`, `OMP_1.0`, `OMP_2.0`, `OMP_3.0`, `OMP_3.1`, `OMP_4.0`, `OMP_4.5`, `OMP_5.0`.
Required tags: `GLIBC_2.14`, `GLIBC_2.16`, `GLIBC_2.17`, `GLIBC_2.2.5`, `GLIBC_2.3`, `GLIBC_2.3.4`, `GLIBC_2.4`, `GLIBC_2.6`.

Baseline comparisons use upstream source maps:
[GFortran GCC 8.1](https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-8.1.0/libgfortran/gfortran.map),
[quadmath GCC 6.1](https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-6.1.0/libquadmath/quadmath.map)
versus [GCC 5.5](https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-5.5.0/libquadmath/quadmath.map),
[quadmath GCC 4.6](https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-4.6.4/libquadmath/quadmath.map),
and [libgomp GCC 9.1](https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-9.1.0/libgomp/libgomp.map)
versus [GCC 8.1](https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-8.1.0/libgomp/libgomp.map).

The bundled `gnu-runtime-source-offer.txt` offers complete corresponding source
for the actual redistributed runtime libraries, provider changes and build/install
scripts to anyone possessing the object code. It is valid for **at least three
years after the last distribution**. Electronic delivery is free; physical-copy
charges are limited to reasonable distribution cost. Requests use the public
[AriadShift issue tracker](https://github.com/bavanchun/AriadShift/issues),
verified public with issues enabled. Recipients retain rights to replace
libraries and debug modifications; packaging restrictions do not remove them.

All three full legal texts are hash-verified and copied with every GNU entry's
original attribution. Their GCC 14.2 URLs identify legal-text provenance only.
The source-pointers notice includes all seven paths/hashes, defined/required
tags, wheel hashes, release/build URLs and the archive index.

## Limits and handoff

This is an actual Linux x86_64 installed-wheel inventory after the production
cleanup, not a newly built release pack. It reuses the prior SHA-verified native
Torch/OpenCV cache. Its known ITT markers remain allowed by the existing runtime
scanner and rejected by the stricter wheel-export audit; that policy was not
changed. No new Rust or macOS/Windows/aarch64 release artifact is certified.
The new component inventory covers the reviewed Linux files; other platform
components require review when built. Windows remains deferred under OD-13.

The existing GNU source offer remains a continuing release-maintainer obligation
for at least three years after last distribution. No new copyleft exception,
licence waiver or unresolved owner question was introduced. Pillow remains
13.0.0 with its uv-generated lock; this second rework changed no dependency pin.

Local review checked the four findings, direct callers, declared threat model,
unchanged approved entries and the new real-input notice/launcher repairs.
The independent third review accepted the prior head; this final small delta
was checked against its three lows. Integration or push remains with ashift-lead.
All owned gate processes have exited; no slot holder was paused. Nothing pushed.

Status: DONE_WITH_CONCERNS
