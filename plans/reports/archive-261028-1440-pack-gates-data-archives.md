# Pack gates: reviewed PyTorch model archives

Twelve exact PyTorch regression-model archives are accepted by distribution,
version, relative path and SHA-256, with one matching installed RECORD owner.
Other ZIP payloads retain the fail-closed gate. No broad .pt/.ptl exception,
licensing exception or dependency/lock change is added.

- Starting head: `ce0d878978a13c00f9325dbd29806d1271c1f441`.
- Code/gate head: `ea43f406da7ae5aa2602b47a49ebe9372cc11232`.
- Code commit: `fix(docling): bind reviewed model data archives`.
- Gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework5-final-gate.log`.
- Last gate line: `=== PACK GATE ALL GREEN HEAD=ea43f406da7ae5aa2602b47a49ebe9372cc11232`.
- Nothing is pushed; a report-only successor follows the tested code head.
- [Previous result](/home/vchun/Codes/01-AriadShift-wt-t7-pack/plans/reports/archive-261028-1349-pack-gates-merge-dev.md)
  preserves the earlier licence approvals, GNU pointers, pywin32 binding,
  Windows repair and fixture evidence.

## Reproduction and repair

[Pack aarch64 Linux](https://github.com/bavanchun/AriadShift/actions/runs/40644845744/job/122671647465)
at ce0d878 failed at 06:42:44 UTC on 2026-10-28 with
`uninspected site-packages ZIP archive: torch/bin/script_module_v4.ptl`.
The ZIP guard detected model data outside its document/array suffix classes.
A complete extraction of the locked aarch64 torch wheel reproduces that exact
error before editing production code; the repaired scan passes and finds 146
native files. This is a real wheel check, not a full foreign-architecture pack.

The twelve exact entries in `engines/docling/pack/reviewed-data-archives.toml`
cover the torch 2.14.1+cpu Linux model bundles. Changed hash, version, owner,
path or installed RECORD, and duplicate ownership fail. Unknown ZIP/JAR/zipapp,
.zip/.egg/.whl suffixes and archive path references retain their refusal checks.
ARCHITECTURE.md, both owning Decision Log rows, AGENTS Licensing and the module
threat-model docstring describe this bounded exception.

## Complete locked-wheel ZIP inventory

The scanner exports the pack builder's frozen, no-dev ariad-docling requirements
and checks every compatible locked wheel for those package names, including
compatible variants rather than only one resolver choice. All **216 wheels**
match uv.lock SHA-256 values. Every regular wheel member is checked for PK
local-header magic: **148 payload occurrences**, grouped below into **37 rows**
with identical owner/version/path/size/hash. The complete wheel mapping is in
the preserved JSON; the table retains platform unions and occurrence counts.

Coverage is normal CPython 3.14, compatible ABI3 and pure wheels for Linux
x86_64/aarch64, macOS arm64 at deployment version 14.0 and Windows x86_64, plus
locked Windows x86/arm64 wheels where available. Free-threaded, musl, iOS and
unrelated platforms are outside the pack configuration. Extra Windows
architectures are audited wheels, not complete supported pack resolutions.

LX = x86_64 Linux; LA = aarch64 Linux; MA = arm64 macOS; WX = x86_64 Windows;
W32 = x86 Windows; WA = arm64 Windows.

| Owner/version | Relative wheel path | Bytes | SHA-256 | Platforms / occurrences |
| --- | --- | ---: | --- | --- |
| numpy 2.5.4 | `numpy/lib/tests/data/py2-objarr.npz` | 366 | `c68d771c14f415b159daabd9cf42d61836f74ae40049269787baca7d57098f1e` | LA,LX,MA,W32,WA,WX / 7 |
| numpy 2.5.4 | `numpy/lib/tests/data/py3-objarr.npz` | 453 | `bd5465f7f359effabe86376e52bf185a7cd1cbc1123659af30f95a4920baf5f9` | LA,LX,MA,W32,WA,WX / 7 |
| python-docx 1.2.0 | `docx/templates/default.docx` | 38116 | `2094b5bddffe9cf973d61fe03388413804f034160718494a65db7e98da40d35d` | LA,LX,MA,W32,WA,WX / 1 |
| python-pptx 1.0.2 | `pptx/templates/default.pptx` | 34030 | `e10cc9e120961f6bd4074a373c9c80d2a06c497157e8f4972977b7bea83a8f34` | LA,LX,MA,W32,WA,WX / 1 |
| scipy 1.18.1 | `scipy/fftpack/tests/fftw_double_ref.npz` | 162120 | `a60c649415b645223924d8342ccc5c097801c86901287a369e53fc9259f5ec4e` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/fftpack/tests/fftw_longdouble_ref.npz` | 296072 | `a406cbd4dad04d0c59dd38f54416fb49424c82229c1a074b6a44ec0cde2000e3` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/fftpack/tests/fftw_single_ref.npz` | 95144 | `276a9141318e6fc36e4ab6ff54a61b64054ef8849b660f17359e5f541b43c526` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/fftpack/tests/test.npz` | 11968 | `36de804a22d8fdea054590ce49ddf3c859838b7d89193c56b3bcb660cbf43797` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/interpolate/tests/data/bug-1310.npz` | 2648 | `8d6803c0b398f2704c236f1d1b9e8e5ede06bd165a0abb0f228281abbd455ae9` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/interpolate/tests/data/gcvspl.npz` | 3138 | `03ce8155a6cba0c1bf0a2441a10c228191f916dec36cb820723429811296bba8` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/linalg/tests/data/carex_15_data.npz` | 34462 | `13f3e1491a876bbf59d7ea10ad29c1f9b5996a2ab99216f31d5bfcd659012c1e` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/linalg/tests/data/carex_18_data.npz` | 161487 | `59f839467f2752b7df6fb6d4094396edd32a5929b764f7ffa1e6666431e6cac6` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/linalg/tests/data/carex_19_data.npz` | 34050 | `38e8fc7b041df0b23d7e5ca15ead1a065e6467611ef9a848cc7db93f80adfd87` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/linalg/tests/data/carex_20_data.npz` | 31231 | `14e222d34a7118c7284a1675c6feceee77b84df951a5c6ba2a5ee9ff3054fa1d` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/linalg/tests/data/carex_6_data.npz` | 15878 | `1b2a0736b541ebf5c4b9b4c00d6dab281e73c9fb9913c6e2581a781b37b602f9` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/linalg/tests/data/gendare_20170120_data.npz` | 2164 | `a3dfab451d9d5c20243e0ed85cd8b6c9657669fb9a0f83b5be165585783d55b5` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/sparse/linalg/tests/propack_test_data.npz` | 600259 | `25d42e05bb6cb5672db22ebf8df7f113e4cb21b1817491fadc996341a542346d` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/spatial/tests/data/degenerate_pointset.npz` | 22548 | `048abc1ddd924bf2d4d1f216015552ed9431f9e99546fbf382768eda58788175` | LA,LX,MA,WA,WX / 6 |
| scipy 1.18.1 | `scipy/special/tests/data/boost.npz` | 1270643 | `57b5c2b67ee01c741536aae6ad9f8f128106b7fdff152afcf3d8f774b0645844` | LA,LX,MA / 4 |
| scipy 1.18.1 | `scipy/special/tests/data/boost.npz` | 1273269 | `c37a7db057c98c228459f560fa2b1246789859cec97ae1550d6840d6f0f164b4` | WA,WX / 2 |
| scipy 1.18.1 | `scipy/special/tests/data/gsl.npz` | 51433 | `cbf1afdd27999806a710279910a2eb2f9f7f559033c7ecb796deaa10c44feb31` | LA,LX,MA / 4 |
| scipy 1.18.1 | `scipy/special/tests/data/gsl.npz` | 51453 | `3ed9f083ac91ea7190b593775114584875160ddf886d13a28e2d371f22c8f173` | WA,WX / 2 |
| scipy 1.18.1 | `scipy/special/tests/data/local.npz` | 203438 | `6c29e58ce827084f84db9f1bba96045a61ce69f1d3ea3d7c828a79c133cf88f2` | LA,LX,MA / 4 |
| scipy 1.18.1 | `scipy/special/tests/data/local.npz` | 203667 | `9e35e6df1806a464d555b14b0e3ccba5bf6b53dd0e2eda77e97e460986a48362` | WA,WX / 2 |
| scipy 1.18.1 | `scipy/stats/_sobol_direction_numbers.npz` | 589334 | `4859931147d42ce465b8605cb277f957d98b839d03194fdf06579357906d193b` | LA,LX,MA,WA,WX / 6 |
| torch 2.14.1+cpu | `torch/bin/script_module_v4.ptl` | 2573 | `dead49900e21bb04339bf06c14968de7d2a651a3739ccb1c8e88a9209f6e82ad` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/test_interpreter_async.pt` | 2840 | `549d04b98a683b79f1c5831a5ad922b563311a4216b5a2c28226182790368b21` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_scalar_float_v2.ptl` | 2160 | `91360dfdc505de05fac69b971ef8a06a2b75b468e0a66cf2db404fc50f210281` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_scalar_inplace_float_v2.ptl` | 2273 | `4f4711f0639598ebd2275ffd98a32ddd0108bc4cc85f46c133957ab77f8faa29` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_scalar_inplace_int_v2.ptl` | 2259 | `5013d856a255346ad71c4b462be3ab05cccd9443c6978819624197300be8220b` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_scalar_int_v2.ptl` | 2146 | `b1d850caddbab0c8804529321ffb93fd8a5a63637304d85634c94befde8cb094` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_scalar_reciprocal_float_v2.ptl` | 2904 | `7eeb7829a439737b2a09d21a8e18fb32cc0fd0fcce77b652eec352f7c603abcb` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_scalar_reciprocal_int_v2.ptl` | 2890 | `64b56ee0a2c5efeb6744a731286006adae747d6e9b54d65e701c292ad17ff3a0` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_scalar_scalar_v2.ptl` | 2672 | `321da0d43e96138be4bd102b2bd5fde6b836c0814ca754533f0246e2f030a74f` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_tensor_inplace_v2.ptl` | 2168 | `87a7c59d41d047fa1bc45578143f6c6a1577cee159a9094cc2bb80e12603017c` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_tensor_out_v2.ptl` | 2211 | `ccbcc0954174e817a7789194ea30e99b09e17de14d089ffed7e67c3e883195fd` | LA,LX / 2 |
| torch 2.14.1+cpu | `torch/bin/upgrader_models/test_versioned_div_tensor_v2.ptl` | 2463 | `3cbb7834ee5e0479468a29d91189e6413c81186e25d03c1210d8cfb16db77c74` | LA,LX / 2 |

The existing data formats account for 124 occurrences: NumPy/SciPy NPZ arrays
(all members are NPY files), the python-docx DOCX template and python-pptx PPTX
template. No new suffix class is introduced. The other 24 occurrences are the
same twelve torch model bundles in both Linux wheels; torch macOS/Windows
contain no ZIP payloads.

The model bundles contain serialized TorchScript code, tensors and state, as
shown by the [upstream format](https://github.com/pytorch/pytorch/blob/main/torch/csrc/jit/docs/serialization.md)
and the reviewed wheel's `torch/jit/mobile/__init__.py` file-taking loader.
They are inputs to explicit model-load calls. The four .pth files across the
inspected wheels do not reference these models. No JAR, zipapp, egg/wheel import
archive or archive loader path requiring escalation was found. Model contents,
source and loader evidence are preserved; archive contents are not parsed at
runtime. The existing default notice-content and other-archive-magic limits,
and deferred Windows RECORD refresh, remain unchanged.

## Tests and gate

Baseline tests: **1 failed, 6 passed**. The valid reviewed archive is refused
before the repair; every negative case already passes. Synthetic tests cover
exact bytes, changed bytes with a rewritten RECORD, owner/version/RECORD,
duplicate owner and unreviewed paths inside/outside torch. JAR and zipapp cases
extend the existing renamed-archive test. The first focused run passed the
inventory tests and real wheel but stopped at two Ruff regex warnings; the
patterns were corrected. The final focused run passes all **48 tests**, the
real aarch64 wheel, Ruff check/format and the tracked diff check.

The exact-head full scoped gate completed through `gate-slot.sh 2`, nice 19,
with one Cargo/test/BLAS/Rayon job. All selected checks pass:

| Check | Result |
| --- | --- |
| Five focused suites / whole engine suite | 182 passed / 273 passed, 4 skipped |
| Three complete locked pywin32 wheels and uv installations | Passed; modified source and forged RECORD refused |
| Real Linux native inventory | 256 files: 52 named objects, 204 extensions |
| Complete aarch64 torch wheel archive scan | Passed; 146 native files detected |
| All real-wheel ZIP payloads / locked hashes | 148 payloads pass / 216 wheel hashes match |
| CP1252 and C-locale probes | 5 passed each |
| uv lock --check / uv audit --frozen | 113 packages, lock unchanged / 109 packages, no adverse findings |
| UTF-8 audit / Ruff check and format | 19 files, no implicit encodings / all 9 selected modules pass |
| Current-head CLI build / just bench-test | Passed / 75 passed |
| Fixture generators | 135 files match stored bytes and manifest hashes in two runs |
| typos / just lint-commits / tracked diff / exact-head assertion | Passed |

Exact launch command:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 /bin/bash /tmp/t7-rework5-final-pack-gate.sh
```

The [preserved gate script](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework5-final-pack-gate.sh)
contains every command and environment variable: both pytest scopes, the real
wheel and archive checks, lock/audit, text/Ruff checks, CLI/bench, all generators,
typos and commit lint. Helpers are preserved beside it. No full Rust matrix,
native source pack build or publishing is added. Local review of the complete
source/TOML/docs diff found no open finding within this ZIP repair.

## CI and evidence

At 06:49:52 UTC on ce0d878, the aarch64 pack was the only failed check. The
x86_64 pack and CPU torch build were running; macOS pack was queued. Engine
tests Windows/Linux, real-engine conformance and CPU torch arm64 Linux/macOS
passed. At 07:19:24 UTC, macOS had also failed; x86_64 pack and CPU torch
were still running. The complete macOS log ends its build at 07:01:25 UTC with
`review vendored library licences for opencv-python-headless: cv2/.dylibs/libIex-3_3.32.3.3.4.dylib`.
[macOS pack job](https://github.com/bavanchun/AriadShift/actions/runs/40644845744/job/122671647402)
failed on a missing named native-library review, not ZIP data. This rework keeps
that object refused and records it as a separate remaining pack issue. The
final snapshot at **07:31:36 UTC** still has exactly these two failed pack jobs;
[x86_64 pack](https://github.com/bavanchun/AriadShift/actions/runs/40644845744/job/122671647410)
remains in progress at `Build the pack`, with no completed job log available.
Its step/status JSON is saved. No Windows pack job is present in this run;
Windows engine tests pass. The remote PR head is still ce0d878.

Evidence: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework5-*`: CI JSON/logs,
wheel receipts and complete ZIP inventory, model/archive members and loader
source, download/scan/reproduction logs, baseline/focused tests and gate helpers.
Wheels remain in `/home/vchun/Codes/t7-rework5-wheel-review/`; the full aarch64
extraction is `/home/vchun/Codes/t7-rework5-aarch64-site/`.

Foreign wheel checks execute no ARM/macOS/Windows code. Full-pack CI at the new
head requires an authorized push. No runnable archive requiring a Q was found.
The macOS libIex native-library review remains unresolved outside this ZIP repair.

Status: DONE_WITH_CONCERNS
