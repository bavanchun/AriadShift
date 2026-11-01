# Worker report: shared contract for the PDFium engine tools (c0)

Branch `p04/c0`. Status: complete. No tool logic was written; every engine tool is a stub that
returns a typed "not implemented yet" failure.

## What was implemented

1. Params types for all phase-4 tools in `ariad-core` (serde + schemars, `deny_unknown_fields`,
   the existing `PageRange`).
2. Tool specs in `specs/pdfium.rs`, not yet registered, with a `pub const` output-format list per
   tool.
3. The `ariad-pdfium-engine` crate: `[lib]` + `[[bin]]`, `#![forbid(unsafe_code)]`, `serve_v2`
   main, dispatch, libpdfium locator, a shared `ToolContext`, and eight stub modules.

## Final contract

### Params (`ariad_core::tools::params`, re-exported from `params/mod.rs`)

| Tool | Type | Fields (default, range) |
|---|---|---|
| `pdf.render` | `RenderParams` | `pages` (all), `dpi` (150, 36..=600), `format` (`RasterFormat`: png, jpeg, webp, bmp, tiff; png), `quality` (`Option<u8>`, 1..=100, jpeg/webp only), `background` (`Background`: white, transparent; white), `annotations` (true), `combine` (`Combine`: none, vertical; none) |
| `pdf.to_images` | `ToImagesParams` | same fields; `dpi` defaults to 300 (Stirling "PDF to image"). `From<ToImagesParams> for RenderParams` |
| `pdf.thumbnails` | `ThumbnailsParams` | `pages` (all), `max_px` (256, 64..=512) |
| `image.to_pdf` | `ImageToPdfParams` | `page` (`PageSize`: `fit-image`, `a3`, `a4`, `a5`, `letter`, `legal`; `fit-image`), `margin_pt` (0, 0..=144), `orientation` (`Orientation`: auto, portrait, landscape; auto) |
| `pdf.extract_images` | `ExtractImagesParams` | `pages` (all), `format` (`ExtractFormat`: original, png; original), `min_px` (0, 0..=16384) |
| `pdf.text_layout` | `TextLayoutParams` | `pages` (all) |
| `pdf.remove_blank` | `RemoveBlankParams` | `threshold` (0.99, 0..=1), `dpi` (50, 36..=600) |
| `pdf.crop_whitespace` | `CropWhitespaceParams` | `pages` (all), `padding_pt` (0, 0..=144) |
| `pdf.detect_scanned` | `DetectScannedParams` | none (`{}`) |

Every type except `DetectScannedParams` and `TextLayoutParams` has `validate(&self) -> Result<(),
ParamsError>` for the ranges and combinations serde cannot check (dpi and size ranges,
`quality` on a lossless format, `transparent` with jpeg, non-finite floats). `ParamsError` holds a
`field` and a fixed `reason`, so it is safe to relay as an engine failure message. The engine's
dispatch calls `validate()` before a tool runs. The JSON schemas carry the same ranges and
defaults.

`image.to_pdf` accepts png, jpeg, webp, tiff and bmp. HEIC is excluded because no permissive
decoder is verified.

### Specs (`ariad_core::tools::specs::pdfium`)

`pub fn specs() -> Vec<ToolSpec>` returns nine specs, all `Tier::Free`, version `"1"`:

| Id | Group | Engine | Runtimes | Primary output | Declared list |
|---|---|---|---|---|---|
| `pdf.render` | convert | pdfium | Local | png | `RENDER_OUTPUT_FORMATS` = png, jpeg, webp, bmp, tiff, zip |
| `pdf.to_images` | convert | pdfium | Local | png | `TO_IMAGES_OUTPUT_FORMATS` (same) |
| `pdf.thumbnails` | convert | pdfium | Local | png | `THUMBNAILS_OUTPUT_FORMATS` = png, json |
| `image.to_pdf` | convert | ariad-pdf | Wasm, Local | pdf | `IMAGE_TO_PDF_OUTPUT_FORMATS` = pdf; inputs `IMAGE_TO_PDF_INPUT_FORMATS`, arity `many(1, 500)` |
| `pdf.extract_images` | convert | pdfium | Local | png | `EXTRACT_IMAGES_OUTPUT_FORMATS` = png, jpeg, zip |
| `pdf.text_layout` | organize | pdfium | Local | json | `TEXT_LAYOUT_OUTPUT_FORMATS` = json |
| `pdf.remove_blank` | organize | pdfium | Local | pdf | `REMOVE_BLANK_OUTPUT_FORMATS` = pdf |
| `pdf.crop_whitespace` | organize | pdfium | Local | pdf | `CROP_WHITESPACE_OUTPUT_FORMATS` = pdf |
| `pdf.detect_scanned` | organize | pdfium | Local | json | `DETECT_SCANNED_OUTPUT_FORMATS` = json |

`pub fn output_formats(tool_id) -> Option<&'static [&'static str]>` maps an id to its list; the
engine's `ToolContext::artifact` uses it to refuse an undeclared format. `PDFIUM_ENGINE` is the
engine id constant.

### Engine crate (`crates/ariad-pdfium-engine`)

Context type (`output.rs`):

```rust
pub type Metrics = BTreeMap<String, Value>;
pub type ToolResult = Result<Option<Metrics>, EngineFailure>;
pub const MAX_IMAGE_PIXELS: u64 = 16_384 * 16_384;
pub struct CancelFlag(..);            // new(), cancel(), is_cancelled(); Clone
pub struct ToolContext<'a> { .. }     // holds &mut dyn Write for events
```

`ToolContext` methods:

- `new(job, tool, RunToolPaths, Limits, &mut dyn Write)`, `with_library_dir(dir)`, `with_cancel(flag)`
- `job()`, `tool()`, `inputs() -> &[PathBuf]`, `input() -> &Path`, `input_stem() -> String`,
  `output_dir()`, `work_dir()`, `limits()`
- `check()`: fails on cancel (`engine_failure`) or past `limits.timeout_s` (`limit_exceeded`)
- `check_page_count(n)`: fails above `limits.max_pages`
- `check_pixels(w, h)`: fails above `MAX_IMAGE_PIXELS`
- `pdfium() -> Result<Pdfium, EngineFailure>`: loads libpdfium
- `progress(stage, done, total)`, `warn(code, message)`
- `artifact_path(name) -> Result<PathBuf>`: plain file name inside the output directory
- `artifact(path, format)`: the file must be a regular file directly in the output directory, the
  format must be declared for the tool, and the count is bounded by `limits.max_archive_entries`
- `artifact_count()`

Free function: `page_artifact_name(stem, page, ext)` returns `<stem>.page-001.<ext>`.

How a tool reports: it calls `ctx.progress(..)` and `ctx.warn(..)` while working, writes each
finished file into `ctx.output_dir()` (named through `artifact_path`) and calls `ctx.artifact(..)`,
then returns `Ok(Some(metrics))` or `Ok(None)`. Dispatch emits the successful `result` (with the
metrics) after the tool returns. A failure is returned as an `EngineFailure`, which `serve_v2`
reports as the failed `result`. A tool never writes to stdout and never emits `result` itself.

Stub signatures (each currently returns `Err(not_implemented(..))`: code `engine_failure`,
message `The <tool> tool is not implemented yet.`):

```rust
render::run(ctx: &mut ToolContext<'_>, params: RenderParams) -> ToolResult            // also serves pdf.to_images
thumbnails::run(ctx, ThumbnailsParams) -> ToolResult
extract_images::run(ctx, ExtractImagesParams) -> ToolResult
text_layout::run(ctx, TextLayoutParams) -> ToolResult
detect_scanned::run(ctx, DetectScannedParams) -> ToolResult
blank::run(ctx, RemoveBlankParams) -> ToolResult                                       // pdf.remove_blank
blank::split_pixel(ctx, params: &serde_json::Value) -> ToolResult                      // pdf.split, detect = "pixel"
crop_whitespace::run(ctx, CropWhitespaceParams) -> ToolResult
```

`dispatch.rs`: `TOOL_IDS` (nine ids, including `pdf.split`), `capabilities()`, `handle(request,
out)`, `handle_with(request, out, library_dir)` and `run_tool(ctx, params)`. `describe` reports
engine `pdfium`, protocol `ariad-engine/2`, `supports_session: false`,
`enforces_memory_limit: false`, license `Apache-2.0`, tool status `libpdfium` found or missing, and
each tool with the version from its spec (`"1"` for `pdf.split`). `convert` requests fail with
`unsupported_route`. `run_tool` requests go through `validate_run_tool`, then `pdf.to_images`
parameters are converted to `RenderParams` and passed to `render::run`.

`library.rs`: `library_file_name()`, `search_dirs(exe_dir)` (the directory, then `../lib/`),
`find_in(dir)`, `locate_default()`, `bind_in(dir)` (explicit directory, for tests) and
`bind_default()`. No environment variable is read. PDFium binds once per process; a second
`bind_in` returns a handle to the first binding, so all calls in one test process must name the
same directory.

## Dependencies added (`crates/ariad-pdfium-engine/Cargo.toml`)

| Crate | Pin | Features | Licence |
|---|---|---|---|
| `pdfium-render` | `=0.9.5` (newest stable, published 2026-11-01) | `pdfium_8125`, `thread_safe`; default features off; no `static`, no `image_*` | MIT OR Apache-2.0 |
| `image` | `=0.25.10` (newest stable) | `png`, `jpeg`, `webp`, `bmp`, `tiff`; default features off | MIT OR Apache-2.0 |
| `zip` | `=9.0.5` (the workspace pin; 9.2.0 exists) | `deflate`; default features off | MIT |
| `ariad-core`, `ariad-engine-io`, `ariad-pdf`, `serde`, `serde_json` | workspace | | workspace |
| `tempfile` (dev) | workspace | | MIT OR Apache-2.0 |

`pdfium-render`'s `image_025` feature depends on `image` with default features, which would pull
every codec into the tree and break the "five codecs only" requirement, so no `image_*` feature is
enabled. Slices turn the PDFium bitmap into an `image::RgbaImage` themselves
(`PdfBitmap::as_rgba_bytes()`).

New transitive crates from `pdfium-render` and `image` (for example `libloading`, `moxcms`,
`zune-jpeg`, `image-webp`, `tiff`, `png`, `fax`, `bytes`, `chrono`, `itertools`) were not
licence-checked: `just deny` and `just licenses-check` are the integrator's step.

## Version pin

The spike report `plans/reports/spike-pdfium-8125.md` was still missing on `p04/spike` when this
slice finished. As the task allowed, `pdfium-render 0.9.5` was used with the `pdfium_8125` feature
(its newest PDFium feature, matching `chromium/8125`). Confirmed with the spike's Linux x64 build:
a throwaway test (not committed) loaded `libpdfium.so` through `library::bind_in`, bound a second
time, and opened a one-page PDF.

## Commands run

| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy -p ariad-core -p ariad-pdfium-engine --all-targets -- -D warnings` | PASS |
| `cargo test -p ariad-core --lib` | PASS, 239 passed (includes the new params and spec tests) |
| `cargo test -p ariad-pdfium-engine` | PASS, 17 passed |
| throwaway real-library bind test with `ARIAD_TEST_PDFIUM_DIR` | PASS (deleted, not committed) |

All commands ran through the build lock, without `--locked`.

## Changes the integrator must make to shared files

1. `crates/ariad-core/src/tools/specs/mod.rs`: add `pub mod pdfium;`. The engine crate does not
   compile without it (it imports `specs::pdfium`). Slices add this line locally.
2. `crates/ariad-core/src/tools/registry.rs`, in the `REGISTRY` initializer: add
   `list.extend(specs::pdfium::specs());` after the `organize_info` line.
3. Root `Cargo.toml`: the workspace picks up the crate through `members = ["crates/*"]`. The
   integrator may move the three pinned dependencies to `[workspace.dependencies]`. `Cargo.lock`
   changed locally (new crates) and is not committed.
4. `fuzz/**`, `THIRD_PARTY_LICENSES`, `about.toml`, `deny.toml`: the new dependencies need the
   usual licence entries.
5. `docs/parity-matrix.md`: the runtime cells of the nine tools are `local` (and `wasm, local` for
   `image.to_pdf`), per slice-map decision 3; `docs/tools.md` needs the new tools and the output
   naming below.
6. `schemas/*.json`: regenerate the tools schema once the registry change lands.

### Recommended multi-format `ToolSpec` change (slice-map decision 1)

Today `ToolSpec.output_format: &'static str` is checked in `crates/ariad-host/src/tools/run.rs`
(preflight at line 611, explicit output path at 1004, artifact check at 1115) and
`tools/publish.rs` (`validate_output_path_extension`, `validate_artifact_extensions`), and used for
inline JSON capture in `tools/engine.rs:137` and in the test probe `engines/mod.rs:239`.

- `registry.rs`: replace the field with `pub output_formats: &'static [&'static str]`; the first
  entry is the primary format. Add `ToolSpec::primary_output_format()`. Keep
  `ToolDescriptor.output_format: String` (the primary, so `ashift tools --json` stays compatible)
  and add `output_formats: Vec<String>` with `#[serde(default)]`.
- `validate_specs`: the list must be non-empty, each entry non-blank, lowercase and unique.
  `organize_info.rs` and the other specs become `output_formats: &["json"]` and so on. In
  `specs/pdfium.rs` set `output_formats: RENDER_OUTPUT_FORMATS` etc. and drop the `output_format`
  field plus the `output_formats()` lookup; the engine then reads `ariad_core::tools::find(tool)`.
- `core::format::Format`: add `Bmp` and `Zip` variants (`bmp`, `zip` extensions and media types).
  `Png`, `Jpeg`, `Webp` and `Tiff` already exist; `json` already resolves.
- `publish.rs`: `validate_output_path_extension(path, formats)` accepts an extension whose format
  is in the list. `validate_artifact_extensions(formats, staged)` requires, per artifact, that the
  reported format is in the list and that the staged name and the target name carry the extension
  of that same reported format (so a `zip` artifact must be named `*.zip`).
- `run.rs`: pass `spec.output_formats` to the three call sites. A multi-artifact run (one image
  per page) already requires an output directory; with an explicit single `-o file.png` and
  several pages the existing "requires an output directory" error applies unchanged.
- `engine.rs:137`: capture inline JSON when the single artifact's reported format is `json`
  instead of testing `spec.output_format`.

### Recommended `pdf.split detect: "pixel"` hook (slice-map decision 4)

Add an optional per-spec engine override and route on the validated parameters.

- `ToolSpec` gains `engine_for: Option<fn(&serde_json::Value) -> Option<&'static str>>` (`None` for
  every spec but `pdf.split`; the `pdf.split` spec sets it to return `Some("pdfium")` when
  `params["detect"] == "pixel"`). Add `ToolSpec::engine_for_params(&self, params) -> &'static str`.
- Host: in `tools/run.rs`, after parameter validation and before dispatch, compute
  `let engine = spec.engine_for_params(&params)` and use it where `spec.engine` picks between
  `inprocess` (engine `ariad-pdf`) and `engine::run_engine_tool` (`tools/engine.rs:58`
  `resolve_engine_command`). The tier, runtimes and quota stay those of the `pdf.split` spec.
  `pixel` on a `Wasm` runtime must fail with a clear "requires the local runtime" error. The
  in-process `pdf.split` must reject `detect: "pixel"` so it can never be silently ignored.
- `pdf.split` params (phase 3) gain `detect: "content" | "pixel"`, default `content`, in the
  schema and the descriptor.
- Engine side is already in place: `pdf.split` is in `TOOL_IDS` and routed to
  `blank::split_pixel(ctx, &Value)`. Its body renders the pages, finds the blank separators, and
  reuses `ariad-pdf`'s split (linked) for the structural cut, so phase 3's split must expose a
  function that takes explicit separator pages.

## Deviations and decisions

- `pdf.to_images` has its own params type (`ToImagesParams`) instead of literally sharing
  `RenderParams`. A shared type cannot give the two tools different `dpi` defaults, and the
  task asked for Stirling's defaults on the alias (300 dpi). Both types are generated by one
  `macro_rules!` so the fields cannot drift, a test asserts that the two JSON schemas differ only
  in the `dpi` default, and `From<ToImagesParams> for RenderParams` lets the engine run one code
  path.
- `quality` is rejected for lossless formats and `background: transparent` is rejected for jpeg,
  by `validate()`, instead of being ignored silently.
- Page artifacts are named `<stem>.page-001.<ext>` as in `docs/tools.md` §6, not `<stem>.p001.png`
  as in the phase table; `output::page_artifact_name` implements this.
- `pdf.thumbnails` lists `png` and `json` only; it follows the phase table (no zip).
- `pdf.extract_images` lists `png`, `jpeg` and `zip`; a pass-through of JPEG 2000 or CCITT data
  would need `Format` variants that do not exist.
- `zip` is pinned to the workspace's 9.0.5 rather than the newest 9.2.0 so the lock keeps one zip
  version; the integrator can raise both together.
- `dispatch::handle_with` takes an optional library directory so engine tests can run a whole
  request against a real libpdfium without an environment variable.
- Cancellation: the host cancels by terminating the engine process; `CancelFlag` exists for
  in-process callers and tests, and is not wired to a signal handler (that would need `unsafe`
  or a new dependency).

## Skills used

/ak-scenario (edge cases for the params), /ak-cook (implementation), /ak-docs-seeker (crate
versions and features checked on crates.io and in the vendored `pdfium-render` source), /ak-test
(tests), /ak-git (commits).

## Open questions

- Should `pdf.thumbnails` also produce a `zip` above 50 pages like `pdf.render`? The phase table
  says no; a 500-page job would emit 501 artifacts.
- Is a pass-through of non-JPEG original encodings wanted for `pdf.extract_images`? That needs new
  `Format` variants and a longer declared list.
- Is the default of 0 for `pdf.extract_images` `min_px` right, or should tiny decorations be
  skipped by default?
