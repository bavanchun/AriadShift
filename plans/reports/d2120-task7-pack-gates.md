# Pack gates: complete foreign native census and licence stop

The local scoped pack gate passes at `fc3997cf2a0f7360c37165b7fb8fce9e582e0ccb`. The requested all-platform
success condition is **BLOCKED**: the locked macOS OpenCV wheel contains
GPL-enabled FFmpeg dylibs, and the complete foreign census still has unresolved
native name/notice/source/byte reviews. No licence waiver or push was made.
Five independently verified BSD-3-Clause OpenEXR/Imath dylibs now have exact
entries and complete upstream notices. All other unreviewed files remain refused.

- Starting head: `c9d5f324ff85942703c976d24cfa760958c3bf2c`.
- Code and tested head: `fc3997cf2a0f7360c37165b7fb8fce9e582e0ccb`.
- Code commit: `build(docling): attribute reviewed OpenEXR and Imath dylibs`.
- Final gate log: [d2120-t7-rework6-final-gate.log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-final-gate.log).
- Last gate line: `=== PACK GATE ALL GREEN HEAD=fc3997cf2a0f7360c37165b7fb8fce9e582e0ccb`.
- A report-only successor records this result; its first parent is the tested head.
- [Previous result](archive-261028-1440-pack-gates-data-archives.md) preserves
  the data-archive review and earlier licensing, GNU, Windows and fixture evidence.

## Complete native inventory

The [complete CSV table](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-native-files.csv) and
[JSON with discovery items and binary evidence](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-native-files.json)
contain all **2,251 native files**, including every `.so`, versioned `.so`,
`.dylib` and `.dll` (950 rows), plus `.pyd`, executables and native archives.
Each row records platform, distribution/version, relative path, SHA-256,
carrying wheel/hash, existing named licence and notice references where reviewed,
and the owning wheel's available notice paths/digests. Missing library attribution
is explicitly **UNVERIFIED**. An owner's Python metadata is labelled as owner
metadata; it does not establish the licence of a separate bundled native project.
Full upstream/licence attribution for every row is therefore still an unmet
acceptance criterion, rather than an invented permission.

Artifact SHA-256: CSV `fa6480d852597af25b324519d7f4267c0145c7bc8c9b1a7eb2d505cc2e0adbe2`;
JSON `3e97d745b4befbf6e62611d93b3a5a67936c67c847cb5aac1da0ed65082fbc4f`.

| Platform | Selected wheels | Native files | SO/dylib/DLL | Rejected files | Site-level archive errors |
| --- | ---: | ---: | ---: | ---: | ---: |
| x86_64-unknown-linux-gnu | 88 | 402 | 270 | 148 | 0 |
| aarch64-unknown-linux-gnu | 88 | 403 | 271 | 155 | 0 |
| aarch64-apple-darwin | 88 | 351 | 346 | 115 | 0 |
| x86_64-pc-windows-msvc | 89 | 494 | 35 | 235 | 1 |
| i686-pc-windows-msvc | 79 | 145 | 10 | 67 | 0 |
| aarch64-pc-windows-msvc | 85 | 456 | 18 | 227 | 0 |

These are conservative raw locked-wheel input sets: the best compatible normal
CPython 3.14, ABI3 or pure wheel per project, using Linux glibc 2.28/macOS 14
compatibility ranks. Windows x86/ARM64 have fewer available wheels and are partial
input sets, not supported release packs. Dependency markers are not evaluated,
so the census is a superset. This deliberately includes source-build replacement
inputs and upstream torch test/helper binaries that the actual Linux source-built
pack does not ship; those counts cannot be compared with the 256-file production
fixture as equivalent final artifacts. No foreign machine code is executed.

The final diagnostic ends `=== FOREIGN NATIVE REVIEW COMPLETE
HEAD=fc3997cf2a0f7360c37165b7fb8fce9e582e0ccb (findings remain refused)`.
It records 947 rejected native files and one site-level error.

The [final census log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-native-final.log) and
[preserved harness](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-native-census-final.py) call production
`_native_files`, `_native_record_owners`, `vendored_libraries` and
`check_vendored_native_files` on every native file. The full scan first checks
installed RECORD hashes/sizes and ownership. Per-file read-only selectors collect
every rejection instead of stopping at the first. Owner licence results are
memoized only within the immutable extracted input set; licence rules, notices,
file bytes and inventory items are not changed. The final harness also checks
ordinary extensions; the initial diagnostic skipped their owner/notice check.
The restarted final run supersedes that initial diagnostic. Source and wheel
hash verification also passes in the full local gate (216 locked wheel hashes).

Windows x64 additionally fails the real global scan on the zero-byte file
`scipy-1.18.1-cp314-cp314-win_amd64.whl` shipped inside the locked SciPy wheel.
That suffix rejection is retained. The harness records the global error and
continues native enumeration so it does not hide the other findings. It does
not claim that the original global scan passes.

## Added library table

All five paths are under `cv2/.dylibs/` in **opencv-python-headless 5.0.0.93**, from
`opencv_python_headless-5.0.0.93-cp37-abi3-macosx_13_0_arm64.whl`, locked wheel
SHA-256 `030ca5e0837a2963ab36ef896baa9767eb8d2b83353fb28af5a521e40dd8756f`.

| Exact filename | Upstream / notice key | Licence | Locked native SHA-256 |
| --- | --- | --- | --- |
| `libIex-3_3.32.3.3.4.dylib` | OpenEXR 3.3.4 / O | BSD-3-Clause | `df9f5aa8542d0e7a9bd2a6461a9894024a41bd2f8c81f2a0e11f94086b7f4550` |
| `libIlmThread-3_3.32.3.3.4.dylib` | OpenEXR 3.3.4 / O | BSD-3-Clause | `fb8b20381ed5c52c96c74f0972d0b51741339d2d6ee8fde3e68997baf55f7b01` |
| `libImath-3_1.29.11.0.dylib` | Imath 3.1.11 / I | BSD-3-Clause | `1f20d905051a125d8aa4e481b9f0c1c42e2cc0d4a08ae9ddedf88e451a09ef01` |
| `libOpenEXR-3_3.32.3.3.4.dylib` | OpenEXR 3.3.4 / O | BSD-3-Clause | `f1df5d2dbaf3e38c0ba2649e36a5af1fe1a76621346f8e78484bbdac828b906a` |
| `libOpenEXRCore-3_3.32.3.3.4.dylib` | OpenEXR 3.3.4 / O | BSD-3-Clause | `4ece54c65206a77f891019446d93a8ac3ebbeea8af4c1c0561c2b0c613e29718` |

- **O:** complete [OpenEXR v3.3.4 notice](https://github.com/AcademySoftwareFoundation/openexr/blob/v3.3.4/LICENSE.md),
  stored as `engines/docling/pack/licenses/openexr-3.3.4.txt`, SHA-256
  `84853c91d4c2e0310741d4773469d98bb93e61029608e0ba0882a3fffdf330bd`.
- **I:** complete [Imath v3.1.11 notice](https://github.com/AcademySoftwareFoundation/Imath/blob/v3.1.11/LICENSE.md),
  stored as `engines/docling/pack/licenses/imath-3.1.11.txt`, SHA-256
  `c20236d3b39fd20eba8e3d1fb3b892a5483df2e7d8d61bf43f165d3fac22f601`.

The wheel's own four OpenCV notice files omit OpenEXR/Imath attribution: searching
for OpenEXR, Imath, Iex and IlmThread does not establish the requested licences.
The file versions were matched to the upstream tags above and the complete BSD
copyright/conditions/disclaimer texts were copied byte-for-byte. These entries
use the existing explicit upstream-notice mechanism, as Tokenizers does; they
never fall back to SciPy's unrelated native notice. Distribution, wheel version,
exact path and notice digest are fixed in `native-components.toml`. Native bytes
still require the installed RECORD SHA-256 and size. `write_vendored_notices`
bundles the full original text. AGENTS Licensing, ARCHITECTURE and the Decision
Log record this narrow review in the same source commit.

Ten regression cases cover all five valid files and full notice bundling, both
notice texts missing or changed, and an unknown companion still refused. Before
the entries, **9 failed / 1 passed**; after them, **10 passed**. See
[red](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-notices-red.log) and
[green](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-notices-green.log).
The real foreign scan now accepts all five, while retaining the other failures.

## Licence stops and Q candidates

All paths below are under the same macOS wheel's `cv2/.dylibs/`. The eight FFmpeg
objects contain their own `lib… license: GPL version 3 or later` strings and
`--enable-gpl --enable-version3`; their configuration also identifies a Homebrew
FFmpeg 7.1.1_3 prefix. This identifies build provenance, not an exact independently
verified corresponding-source release. The bundled OpenCV text says LGPL-2.1
for FFmpeg, which conflicts with the binary configuration and self-declarations.
The [FFmpeg licence rules](https://ffmpeg.org/general.html) explain that GPL-enabled
builds require GPL terms; no runtime exception is established for these objects.
The [binary proof JSON](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-gpl-binary-proof.json) preserves
complete strings/configurations, wheel hash and every file hash.

| Exact filename | Evidence / stop | SHA-256 |
| --- | --- | --- |
| `libavcodec.61.19.101.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `20825ad7fa755087b533595d24ebb7d5424114bef428032b233537c4e7f1e09f` |
| `libavdevice.61.3.100.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `3d779f3daa08b5144f545baade5f078c55fb918ca98341751760ab39043de2e5` |
| `libavfilter.10.4.100.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `c6c84f1d3bd90f2d270b4908d4a4469eca62828f3dfd4517c8837c3658406d80` |
| `libavformat.61.7.100.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `d670552654be11579f2fc31d66b994d7adbaaea9ef2bb5b5599b52c892c92033` |
| `libswresample.5.3.100.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `b47b866d5a394eadcb0b0edb7e9481f9a76e78922c9170b36edd5146eb7bea1c` |
| `libavutil.59.39.100.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `f38ed445f8b47a91f26f6bb03c16794d357c6ff72382d9f7ffca145c49ae4a58` |
| `libswscale.8.3.100.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `d85a25897775fa901806148b25f00a8bc0d8b38063fd68f102c9c4b06e7e5fe3` |
| `libpostproc.58.3.100.dylib` | FFmpeg: embedded GPL-3.0-or-later declaration | `09e09ff09cf250933fddb18ab0e1cb36775d1dc25e0f81b1334df58d78642cd5` |
| `libx264.164.dylib` | x264: upstream GPL-2.0; commercial entitlement not established | `7f02ead8b5987d7441c56521a80a3e894b44fa5f49a57f8dd515ac3b3f733564` |
| `libx265.215.dylib` | x265: upstream GPL-2.0; commercial entitlement not established | `860a3c77010264c498e781b599980f6bb8cdaab989b05ea1522844bab961bbac` |
| `librubberband.3.dylib` | Rubber Band: upstream GPL-2.0-or-later; commercial entitlement not established | `ac8aeb49e8c6a77c29086fb0696ee88faaa3f6a20363477a015c228fad5d5686` |

The three additional candidates use upstream
[x264 COPYING](https://github.com/mirror/x264/blob/master/COPYING),
[x265 COPYING](https://github.com/videolan/x265/blob/master/COPYING) and
[Rubber Band](https://github.com/breakfastquay/rubberband).
Their ABI filenames do not prove exact upstream release versions. No commercial
licence entitlement or matching alternative wheel notice was found; they remain
refused pending the Q/input replacement. libvidstab is not included in this GPL
list: its current upstream notice differs from older GPL releases, so an old
project-level GPL claim alone would not establish this wheel's licence.

Remaining foreign findings are fully listed in the CSV/JSON and platform summary:
unnamed OpenCV companions; raw torch helpers/libraries; platform notice hash
changes; NumPy metadata/native archive cases; onnxruntime/PDFium/TorchVision
foreign paths; and Windows ownership/extension/native helpers. GNU runtime files
on other platforms require their own byte/format/provenance review; the seven
approved Linux ELF hash bindings and all A-0700 source/offer texts remain intact.
No generic copyleft, filename, hash, static-object or notice exception was added.
A native file with no sufficient upstream licence/notice or static/modified
payload remains a stop candidate; the complete table marks its missing evidence.

**Required follow-up:** replace or rebuild the macOS OpenCV input without its
GPL-enabled FFmpeg and GPL codec payloads, and finish the remaining exact native
licence/notice/source reviews on the selected platform inputs. Then rerun the
complete inventories and pack builds. The task's zero-unreviewed requirement is
not met, and this report does not request a blanket GPL approval.

## Exact-head local pack gate

The complete scoped gate passed through `gate-slot.sh 1`, nice 19, with one
Cargo/test/BLAS/Rayon job. The separate diagnostic census used slot 2. Owned
obsolete diagnostic jobs were cancelled with SIGTERM; no lock holder was paused.

| Check | Result |
| --- | --- |
| Five focused suites / whole Docling suite | 192 passed / 283 passed, 4 skipped |
| Three complete pywin32 wheels and uv installations | Passed; altered source/RECORD refused |
| Actual Linux native fixture | 256 files: 52 named objects, 204 extensions; all seven GNU hashes/ELF checks pass |
| Complete aarch64 torch archive scan | Passed; 146 native files detected; this is an archive scan, not the full foreign licence gate |
| All-wheel ZIP payloads / locked hashes | 148 accepted payloads / 216 wheel hashes match |
| CP1252 and C-locale checks | 5 passed each |
| uv lock --check / uv audit --frozen | Passed; lock unchanged / no vulnerabilities found |
| UTF-8 audit / Ruff check and format | 19 files / all 9 selected modules pass |
| Current-head CLI build / bench tests | Passed / 75 passed |
| All fixture generators | Two runs byte-identical; 135 stored files and manifest hashes match |
| typos / just lint-commits / tracked diff / exact-head assertion | Passed |

Exact launch commands:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 bash /tmp/t7-rework6-final-pack-gate.sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /tmp/t7-rework6-native-final.sh
```

The [preserved full gate script](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-final-pack-gate.sh)
contains every command and environment variable, including both pytest scopes,
real-wheel checks, lock/audit, Ruff, CLI/bench, generators, typos and commit lint.
Helpers and both diagnostic harnesses are preserved beside it. No full Rust
matrix or new native source pack build was run. Local review covered the entire
six-file source diff, exact matching, notice bytes and caller paths; no additional
finding was found within the five-library change. The wider acceptance gaps above
remain blockers.

## Remote CI

At **2026-10-28 08:19:11 UTC**, `gh pr checks 95` reports the
[x86_64 pack job](https://github.com/bavanchun/AriadShift/actions/runs/40644845744/job/122671647410)
**IN_PROGRESS**, at `Build the pack`; macOS and ARM Linux pack jobs have failed.
The downloaded x86_64 job log is available despite the running state: its latest
available line is **2026-10-28 07:06:53 UTC**, OpenBLAS compile step
`12819/14175`. This is a partial server log, with no success/failure conclusion;
it is not evidence that the new head passes CI. The source-matched pack wait is
also in progress. No Windows pack job is present in this workflow run.

[PR checks JSON](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-pr-checks.json),
[job JSON](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-x86-ci-job.json) and
[normalized partial log](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework6-x86-ci.log) preserve the findings.
The remote PR head remains `ce0d878978a13c00f9325dbd29806d1271c1f441`;
the new source and report commits are local only. Nothing was pushed.

Status: BLOCKED
