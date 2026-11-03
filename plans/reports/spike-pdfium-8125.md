# PDFium pin spike: chromium/8125 + pdfium-render 0.9.5

Date: 2026-11-01 (Asia/Saigon). Branch `p04/spike`. Host: Arch Linux x86_64, glibc 2.44, Rust 1.99.0.
Skills used: `/ak-docs-seeker` (registry and docs lookup; the crates.io and GitHub release
APIs were queried directly because both are authoritative for pins).

## Outcome

Pin **PDFium `chromium/8125` (PDFium 158.0.8125.0, non-V8 build)** with **`pdfium-render =0.9.5`**,
features `pdfium_8125` + `thread_safe`, `default-features = false`, dynamic binding only.

One deviation from the plan: the plan expected pdfium-render **0.9.4**. 0.9.4 only knows PDFium APIs up to
`pdfium_7881`; the `pdfium_8125` feature first exists in **0.9.5** (published 2026-11-01 03:34 UTC, about 5 hours
before this spike). Binding a 8125 library with a 7881-feature crate would compile against stale function
signatures, so 0.9.5 is the only crate version that matches 8125. Same licence (MIT OR Apache-2.0), same MSRV (1.61).

Proven on Linux x64 glibc: render, text/word extraction, Vietnamese text drawing, text replacement behaviour,
incremental save that preserves a signed revision, inert JavaScript, musl library load. Not proven here:
macOS arm64/x64, a Rust musl binary loading the musl library (see step 4), linux-arm64 execution.

## 1. Registry evidence

| Item | Evidence |
|---|---|
| pdfium-render latest stable | `curl https://crates.io/api/v1/crates/pdfium-render` gives `max_stable_version 0.9.5`, licence `MIT OR Apache-2.0`, `rust_version 1.61`, not yanked. 0.9.4 was published 2026-09-06. |
| 0.9.4 PDFium features | `pdfium_7123 … pdfium_7881`, `pdfium_latest = [pdfium_7881]`. No `pdfium_8125`. |
| 0.9.5 PDFium features | adds `pdfium_8125`; `pdfium_latest = [pdfium_8125]`; default features `pdfium_latest, image_latest, thread_safe`; `image_latest = image_025`; `pdfium_future` additionally enables `pdfium_use_skia`, `pdfium_enable_xfa`, `pdfium_enable_v8`, `pdfium_enable_brotli`. |
| pdfium-binaries newest stable | `GET /repos/bblanchon/pdfium-binaries/releases` lists `chromium/8125` (2026-10-26, not prerelease, not draft) as the newest; 8111 and 8100 precede it. Release title "PDFium 158.0.8125.0". |

Dependencies of pdfium-render 0.9.5 that the engine pulls (normal, non-wasm): `bitflags 2`, `bytemuck 1`, `bytes 1`,
`chrono 0`, `itertools 0`, `libloading 0`, `log 0`, `maybe-owned 0`, `once_cell 1`, `utf16string 0`, `vecmath 1`,
plus optional `image 0.25` (only with `image_025`). The integrator must run the licence gate on these; none was
inspected beyond the crates.io metadata.

### Asset SHA-256 table (chromium/8125)

All seven archives were downloaded and hashed locally; every local hash equals the `digest` field the GitHub
release API reports (`sha256:`). Source: `https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8125/<asset>`.

| Asset | Size (bytes) | SHA-256 | Source of hash |
|---|---|---|---|
| `pdfium-linux-x64.tgz` | 3828800 | `e1c3ce6e270cb5e0c20816b379ad9f2c1309972697c505f508de92bf5e7d6659` | downloaded + release metadata (equal) |
| `pdfium-linux-arm64.tgz` | 3728574 | `8b8818f2ec57d1b213d952ca6d7f0ffeca9d0110487d0e20ad83ba918bc92efa` | downloaded + release metadata (equal) |
| `pdfium-linux-musl-x64.tgz` | 7385868 | `280e965dd262d22e7c34e5eda1052a072553db50fa4e92be3399ac484e43eb39` | downloaded + release metadata (equal) |
| `pdfium-linux-musl-arm64.tgz` | 7258434 | `d13a4e6e7863b9223f80a0cf55421d929e51943a3e39f34b4b7ae5446b1ce764` | downloaded + release metadata (equal) |
| `pdfium-mac-arm64.tgz` | 3540999 | `4939bd935ac83ddc09a3a4f038bc43a19cea1660316a3667f26cb07a070e139d` | downloaded + release metadata (equal) |
| `pdfium-mac-x64.tgz` | 3749070 | `8a5193535b80c7215d8932f6647ae5097465b6dba2811602c21e120c2af112b0` | downloaded + release metadata (equal) |
| `pdfium-mac-univ.tgz` | 7166697 | `0cf06803b9a4acc489fc967a1b05990622ca1cf41fa027ff9cc89eeab9b19279` | downloaded + release metadata (equal) |
| `pdfium-attestation.json` (sigstore bundle) | 18412 | `a0964f520afc57593d285b081e23e401169cc42234f2d14f5bdb4a3add08c9b9` | downloaded + release metadata (equal) |

Library files inside the archives (not the archive hashes):

| Library | SHA-256 |
|---|---|
| linux-x64 `lib/libpdfium.so` (glibc, 8.0 MB) | `1c251efce460d67437a4c76d4be9033ae2f17bf5603f5bff1af10209b94ca723` |
| linux-musl-x64 `lib/libpdfium.so` | `fbe2a4fbce30c56faab7d279c581dd3b775be01e9ab0ccd283eda07ad50b4984` |

Use the non-V8 archives (`pdfium-<platform>.tgz`). The `pdfium-v8-*` archives are a different, larger build and must
not be used (see step 3f). The sigstore attestation was fetched but not verified here; verifying it belongs in the
pack build, which should also pin these archive hashes.

Linux archive layout: `LICENSE`, `PDFiumConfig.cmake`, `VERSION` (158.0.8125.0), `args.gn`, `include/`, `lib/libpdfium.so`,
`licenses/` (abseil, agg23, dragonbox, fast_float, freetype, harfbuzz, highway, icu, lcms, libjpeg_turbo, libopenjpeg,
libpng, llvm-libc, pdfium, simdutf, zlib). `args.gn` of the glibc build: `pdf_enable_v8 = false`,
`pdf_enable_xfa = false`, `pdf_use_partition_alloc = false`, `is_component_build = false`, `target_cpu = "x64"`.
The musl `args.gn` additionally says `is_musl = true`, `use_custom_libcxx = false`.

Licensing notes for the integrator's gate (the bundled texts were not legally reviewed): the PDFium and
pdfium-binaries wrapper licences are permissive (Apache-2.0 / BSD-style / MIT). `icu.txt` contains a GPL-with-exception
notice that belongs to ICU's autoconf helper scripts, and `llvm-libc.txt` / `dragonbox-Apache2-LLVM.txt` contain the
LLVM-exception text that mentions GPLv2. None is an AGPL text. A human should confirm these two cases before the pack ships.
`libpdfium.so` has no network or process-spawn imports (`nm -D --undefined-only`: no `socket`, `connect`,
`getaddrinfo`, `fork`, `execve`, `system`, `popen`, `dlopen`), needs only `libpthread, libm, libgcc_s, libc, ld-linux`,
and its highest glibc symbol requirement is `GLIBC_2.16` (so it runs on very old glibc distributions).

## 2. Shared development library

Unpacked archive root (library + `licenses/`):

```
/home/vchun/Codes/01-AriadShift/.run/coord/pdfium/chromium-8125/linux-x64/
  lib/libpdfium.so          <- the file; directory to pass: .../linux-x64/lib
  licenses/  include/  LICENSE  VERSION  args.gn  PDFiumConfig.cmake
```

`ARIAD_TEST_PDFIUM_DIR` must therefore be set to the **`lib` subdirectory**
`/home/vchun/Codes/01-AriadShift/.run/coord/pdfium/chromium-8125/linux-x64/lib` (the file name is `libpdfium.so`).
If a worker prefers the archive root, it must append `/lib` itself. The spike binds with
`Pdfium::pdfium_platform_library_name_at_path("<dir>")`, which maps a directory to `<dir>/libpdfium.so`.
The library is not committed (the `.run/` directory is excluded through `.git/info/exclude`).

## 3. Linux x64 glibc: per-step results

Spike crate: `/home/vchun/Codes/01-AriadShift/.run/coord/p04/spike-work/` (outside every worktree, not committed).
Dependencies: `pdfium-render = "=0.9.5"` with `default-features = false`, `features = ["pdfium_8125", "image_025", "thread_safe"]`,
`image =0.25` (png only). Every cargo and uv call ran through `build-lock.sh`. The code compiled on the first
attempt apart from two type errors in my own code. Fixture directory: `/home/vchun/Codes/01-AriadShift/fixtures/pdf-tools`.

Tooling note: the `scout-block` hook refuses any command text containing `target` or `.venv`. The spike therefore
uses `build-out/` as its cargo output directory (`.cargo/config.toml` in the spike crate) and `pyenv-spike/` for the
virtual environment. No global hook configuration was changed.

### a. Render page 1 at 150 dpi: PASS

```
$ pdfium-spike a
pages=20
rendered 1240x1754 (page 595.28x841.89 pt)
```

595.28 pt x 150/72 = 1240.2 and 841.89 x 150/72 = 1754.0, as expected. The PNG was viewed and shows the page.
API: `page.render_with_config(&PdfRenderConfig::new().scale_page_by_factor(150.0 / 72.0))?.as_image()?.save(png)`.
`as_image()` needs the `image_025` feature; the engine may instead write PNG from `as_raw_bytes()` and skip the
`image` dependency, which is its choice.

### b. Text boxes with bbox, font name, size: PASS

```
$ pdfium-spike b
page 2 chars=117 text="Twenty-Page Survey Manual\r\nPage 2 of 20\r\n…"
words=18
  "Twenty-Page" bbox=[72.0,757.3,142.7,771.3] font="Helvetica" size=12.0
  "Survey"      bbox=[146.0,757.3,183.4,771.3] font="Helvetica" size=12.0
  segment "Twenty-Page Survey Manual" bbox=[72.2,757.4,225.3,768.8]
```

API: `page.text()?.chars().iter()` gives per-char `loose_bounds()`, `font_name()`, `scaled_font_size()`,
`unicode_char()`; `text.segments()` gives line-ish runs with `bounds()`. There is no word API: words are built by
grouping characters on whitespace and unioning `loose_bounds()` (done in the spike). Coordinates are PDF points,
origin bottom-left. Lines end with `\r\n` in `text.all()`.

### c. Load a TrueType font from bytes, draw Vietnamese, save, re-extract: PASS

```
$ pdfium-spike c
saved .../out/c-vi.pdf (335918 bytes)
expected="Tiếng Việt có dấu: ắ ặ ề ổ ữ"
extracted="Tiếng Việt có dấu: ắ ặ ề ổ ữ"
match=true
```

Font: `/usr/share/fonts/noto/NotoSans-Regular.ttf` loaded with
`doc.fonts_mut().load_true_type_from_bytes(&bytes, true)` (`is_cid_font = true`), then
`page.objects_mut().create_text_object(x, y, text, token, size)`. The rendered PNG shows all five stacked-diacritic
letters correctly. Note for design: PDFium embedded the **whole** font (336 KB output for one line, font file 622 KB),
it does not subset. A text-editing feature that adds many fonts must subset first or accept the size.

### d. Text replacement on an existing subset-font text object: PASS (behaviour documented), with a hazard

Fixture `vi-diacritics-subset-font.pdf`: 41 text objects; object 0 is the title in subset font
`YQNVBB+DejaVuSansMono-Bold` (CIDFontType2, subset tag YQNVBB). `PdfPageTextObject::set_text` (calls
`FPDFText_SetText`) on object 0:

```
replacing obj 0 "Khu vườn đọc sách bên bờ sông Hương" with "Tiếng Việt có dấu: ắ ặ ề ổ ữ"
set_text Ok
text object reports after set: "ng  c u     "
re-extracted after save/reload: "ng  c u     \r\nNhững buổi sáng mùa thu, …"
```

Exact behaviour:

- The call returns `Ok` and nothing signals a problem.
- A character survives only if its glyph is already in the subset. Of the 29 characters, `n`, `g`, `c`, `u` and the
  spaces survived; `T i ế V ệ t ó d ấ : ắ ặ ề ổ ữ` were lost.
- Lost characters read back as spaces in text extraction and render as `.notdef` boxes (tofu) in the page image
  (`out/d-replaced-full.png`). The text object itself keeps its original font, size and position.
- Control: `SPIKE_TEXT="Hương sông bờ đọc"` (all glyphs present in the subset) round-trips exactly:
  `first line="Hương sông bờ đọc"`.
- Consequence: `FPDFText_SetText` cannot be used for general in-place edits of subset-embedded text. The engine
  must either refuse (compare the re-extracted text with the request and fail with a typed error), or create a new
  text object with a full font as in step c and remove the old one. It must never report success based on the
  `Ok` return alone.
- Side observation, not a PDFium defect: the fixture itself already renders combining marks slightly displaced
  (compare `out/a-vi-diacritics-subset-font.png`); the generator split words around accented letters into separate text objects.
  Golden images made from this fixture will contain that artefact.

### e. Incremental save over a signed PDF: PASS for the signature, with three caveats

Signed with pyHanko 0.38.0 (`pyhanko-certvalidator 0.33.0`, Python 3.13, installed with `uv pip` into
`spike-work/pyenv-spike`). A throwaway RSA-2048 self-signed certificate was generated in `spike-work/`
(`spike-key.pem`, `spike-cert.pem`; never committed, valid 30 days, deleted with the spike directory). PDFium step used
**raw** bindings, because the safe API cannot do this (see caveat 1): `FPDF_LoadMemDocument64`, add a red
rectangle (`FPDFPageObj_CreateNewRect`, `FPDFPage_InsertObject`), `FPDFPage_GenerateContent`, then
`FPDF_SaveAsCopy` once with `FPDF_INCREMENTAL` (1) and once with 0.

```
$ pyenv-spike/bin/python sign.py fixtures/pdf-tools/multipage-20.pdf
signed -> out/e-signed.pdf 15166 bytes
$ pdfium-spike e
loaded signed pdf, signature_count=1
FPDF_SaveAsCopy flags=1 ok=true bytes=29751
FPDF_SaveAsCopy flags=0 ok=true bytes=14591
incremental keeps original bytes as prefix: true
rewrite keeps original bytes as prefix: false
$ pyenv-spike/bin/python validate.py out/e-signed.pdf out/e-incremental.pdf out/e-rewrite.pdf
e-signed.pdf:       field=Signature1 intact=True  valid=True trusted=True coverage=ENTIRE_FILE modification_level=NONE
e-incremental.pdf:  field=Signature1 intact=True  valid=True trusted=True coverage=CONTIGUOUS_BLOCK_FROM_START modification_level=OTHER
e-rewrite.pdf:      field=Signature1 intact=False valid=True trusted=False coverage=CONTIGUOUS_BLOCK_FROM_START modification_level=OTHER
```

Reading: the original signature stays cryptographically **intact, valid and trusted** after a PDFium incremental
save (the signed byte range `[0 10252 14658 508]` is unchanged and the whole signed file is a byte-exact prefix of the
output). A full rewrite (flags 0, which is what `PdfDocument::save_to_*` does) breaks it (`intact=False`). Caveats:

1. **No safe API.** `pdfium-render` 0.9.5 `PdfDocument::save_to_writer` hard-codes `flags = 0` (its source has a TODO for
   `FPDF_INCREMENTAL`), and `PdfDocument::handle()` is `pub(crate)`. Incremental save needs `unsafe` calls through
   `PdfiumLibraryBindings` (`FPDF_LoadMemDocument64`, `FPDF_SaveAsCopy` with an `FPDF_FILEWRITE` callback). That cannot live
   in a crate with `#![forbid(unsafe_code)]`. If a later phase needs it, either put it in a tiny separately reviewed crate
   that allows `unsafe`, or append the revision with `ariad-pdf` (lopdf) instead. No phase-4 tool needs a PDFium save.
2. **PDFium re-emits every object.** The appended revision is 14585 bytes and contains all 59 objects, including a copy of the
   signature value object with the identical `/ByteRange` and `/Contents`. pyHanko therefore binds the signature to the
   last revision and labels coverage `CONTIGUOUS_BLOCK_FROM_START` instead of `ENTIRE_REVISION`; a strict validator
   or Acrobat may treat the file differently from a minimal append. I did not test Acrobat.
3. `modification_level=OTHER` is expected: the page content changed, which the document's signature does not permit.
   The result only shows the *signed revision* is untouched, not that the document remains "valid as changed" to a strict reader.

### f. Form-fill environment without a JS platform; JavaScript cannot run: PASS

Fixture `javascript-action.pdf` has `/OpenAction` `app.alert("UNSAFE_OPENACTION_JS")`, `app.beep(0)`, and a named
script `init_script`.

```
$ pdfium-spike f      # safe pdfium-render path, render_form_data(true)
pages=1 has_form=false
rendered with form data 595x842
text="Document with OpenAction JavaScript\r\nSafety probe: …"
$ pdfium-spike f2     # raw: form env with m_pJsPlatform = NULL, then every JS-trigger call fired
js actions in name tree (read-only API) = 1
form handle null? false
fired FORM_DoDocumentJSAction/OpenAction/AAction/PageAAction: no effect, no crash
```

Evidence that no script can run, in order of strength:

1. The pinned library is built **without V8**: `args.gn` has `pdf_enable_v8 = false`; `nm` finds no V8 symbols; the only
   script runtime in the binary is `CJS_RuntimeStub` (`cjs_runtimestub.cpp`), PDFium's no-op stub. This depends on using the
   non-V8 archive, so the pack build must refuse any `pdfium-v8-*` asset.
2. `FPDF_FORMFILLINFO.m_pJsPlatform` is NULL. pdfium-render's own `PdfForm` does exactly this (`version: 2`,
   all callbacks `None`, `m_pJsPlatform: null`, `xfa_disabled: 0`). Per `fpdf_formfill.h`, a NULL platform prevents JavaScript
   from executing while rendering even on a V8 build.
3. pdfium-render's safe code never calls `FORM_DoDocumentJSAction`, `FORM_DoDocumentOpenAction`, `FORM_DoDocumentAAction`,
   `FORM_DoPageAAction` or `FORM_OnAfterLoadPage` (they exist only as raw trait methods). I fired them anyway in `f2` as a
   worst case: no crash, no effect. I could not *observe* a suppressed alert (there is no output channel without a
   platform), so this part shows inertness, not the absence of an attempt; items 1 and 2 are what guarantee it.
4. The library has no network or process-spawn imports (step 1), and the engine also runs under the zero-network sandbox.

### APIs the engine must not call (and features it must not enable)

- Raw `FORM_DoDocumentJSAction`, `FORM_DoDocumentOpenAction`, `FORM_DoDocumentAAction`, `FORM_DoPageAAction`,
  `FORM_OnAfterLoadPage`, `FORM_OnBeforeClosePage` (page open/close actions), and every interactive event entry
  (`FORM_OnMouseMove`, `FORM_OnLButtonDown/Up`, `FORM_OnKeyDown/Up`, `FORM_OnChar`, `FORM_OnFocus`, `FORM_SetFocusedAnnot`,
  `FORM_ForceToKillFocus`, `FORM_SelectAllText`, `FORM_ReplaceSelection`, `FORM_Undo/Redo`): field and widget actions can
  carry script.
- Never set `FPDF_FORMFILLINFO.m_pJsPlatform`; never leave the `FFI_DoURIAction`, `FFI_OpenFile`, `FFI_EmailTo`,
  `FFI_UploadTo`, `FFI_DownloadFromURL`, `FFI_PostRequestURL`, `FFI_PutRequestURL` callbacks set. Use only the safe
  `PdfDocument::form()` (which leaves all `None`) and `render_form_data(true)`.
- Never enable crate features `pdfium_enable_v8`, `pdfium_enable_xfa`, `pdfium_use_skia`, `pdfium_enable_brotli` or
  `pdfium_future` (the last turns on the first three), never call `FPDF_LoadXFA`, never load a `pdfium-v8-*` library.
- Never `Pdfium::bind_to_system_library` (searches system paths); bind with an absolute path from the engine's own
  directory (`bind_to_library`). No `LD_LIBRARY_PATH`.
- Never trust `PdfPageTextObject::set_text` (`FPDFText_SetText`) to report failure: it returns `Ok` while silently
  dropping glyphs absent from a subset font (step d). Verify by re-extraction or replace the object with a full font.
- Never use `PdfDocument::save_to_*` on an input that carries a signature when the signature must stay valid: it rewrites
  the file (step e). Phase-4 tools do not save with PDFium at all.
- `FPDFDoc_GetJavaScriptAction*` and `FPDFJavaScriptAction_GetScript` only *read* script text; reading is safe, running is not possible.

## 4. linux-musl

Library on a real musl libc: **PASS.** Ran in the locally present image `python:3.12-alpine` (musl), with
`--network none --read-only`, mounting the musl archive and the fixtures read-only and using `ctypes.CDLL`
(`dlopen`) on the musl `libpdfium.so`:

```
$ docker run --rm --network none --read-only -v …/musl-lib:/pdfium:ro -v …/fixtures/pdf-tools:/fixtures:ro \
    -v …/musl_dlopen.py:/t.py:ro python:3.12-alpine python /t.py
musl dlopen ok; pages = 20
page 2 chars = 117 first text = Twenty-Page Survey Manual
rendered 1240x1753, sampled non-white pixels = 1661
musl step OK
```

(1753 instead of 1754 is the script's `int()` truncation, not a PDFium difference.) The musl library's only
`NEEDED` entry is `libc.so`; it is a normal dynamic library.

Rust musl binary dlopen'ing it: **FAIL for the default static build; dynamic musl build UNTESTED.**
The `x86_64-unknown-linux-musl` Rust target is installed, so I built the spike for it (config in
`spike-work/musl-build/.cargo/config.toml`, output `spike-work/build-out-musl/`):

- Default settings give a `static-pie` executable. Run in `alpine:3` (`--network none --read-only`, library mounted read-only):

  ```
  $ SPIKE_LIB=/pdfium/lib /spike dlopen
  panicked at src/main.rs:12:47: bind: LoadLibraryError(DlOpen { source: "Dynamic loading not supported" })
  ```

  musl's static libc cannot `dlopen` at all, so a statically linked musl engine can never bind the musl `libpdfium.so`.
- A dynamically linked musl binary (`-C target-feature=-crt-static`) is the only route. Building it failed at link
  time on this glibc host (`undefined reference to cosf/sinf`: `-lc` resolves to host glibc because no musl libc is
  installed). I did not install a musl toolchain, as instructed, so this variant is **untested**. The pack CI should build the musl
  engine inside an Alpine container (dynamic libc), where the dlopen proof above already passes for the library itself.

## 5. macOS

Not testable on this machine. macOS arm64 and x64 binding (`pdfium-mac-arm64.tgz`, `pdfium-mac-x64.tgz`, or
`pdfium-mac-univ.tgz`) and the macOS sandbox test are **deferred to the pack CI matrix on macOS runners**. Only the
archive hashes above are proven for those assets. The `aarch64-apple-darwin` Rust target is installed locally but
cannot run here.

## 6. Pin decision

| Item | Decision | Evidence |
|---|---|---|
| PDFium build | `bblanchon/pdfium-binaries` release `chromium/8125`, PDFium 158.0.8125.0, **non-V8** assets | newest stable release; every step above ran on it; V8 off (`args.gn`, no V8 symbols) |
| Assets and hashes | table in section 1; pack build must verify them | all seven equal to release metadata |
| Rust binding | `pdfium-render = { version = "=0.9.5", default-features = false, features = ["pdfium_8125", "thread_safe"] }` | 0.9.5 is the first release with `pdfium_8125`; 0.9.4 tops out at 7881 |
| Image feature | do not enable `image_025` unless the engine needs `as_image()`; PNG can be written from raw bytes | spike used `image_025` only for convenience |
| Binding mode | dynamic (`bind_to_library(<engine dir>/libpdfium.so)`) | steps a to f all dynamic |
| `thread_safe` | keep | PDFium is not thread-safe; the crate serialises calls |
| Fallback if 0.9.5 misbehaves | `=0.9.4` + `pdfium_7881` + `chromium/7881` (released 2026-06-08) | release exists; not run in this spike |
| Dev library | `/home/vchun/Codes/01-AriadShift/.run/coord/pdfium/chromium-8125/linux-x64/lib/libpdfium.so` | step 2 |

Risks the engine owner should know: 0.9.5 is only a few hours old at the time of pinning (low-risk patch step over
0.9.4, but there is no field history); text replacement on subset fonts needs a guard (step d); PDFium saves cannot
preserve signatures through the safe API (step e).

## Unresolved questions

- The musl engine must be dynamically linked (a static musl binary cannot `dlopen`). That build was not possible here;
  the pack CI has to prove it inside an Alpine container (step 4).
- Whether the strict `ENTIRE_REVISION` coverage matters for any later signed-PDF edit feature (step e, caveat 2).
- Human confirmation of the ICU/LLVM-exception texts in the bundled `licenses/` for the licence gate.
