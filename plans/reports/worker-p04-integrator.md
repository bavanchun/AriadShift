# Phase 4 Integration Report (`feat/pdfium-engine`)

## 1. Branch Merges and Cleanup

Starting from `feat/pdfium-engine` (`5d0c0f95`), the following branches were merged in order:

| Branch | Tip Merged | Merge Commit | Notes |
| --- | --- | --- | --- |
| `origin/dev` | `cd689488` | `719abdec` | Brought Phase 3 structural PDF tools, `ariad-pdf`, `ariad-engine-io`, and host/CLI/WASM tool infrastructure into `feat/pdfium-engine`. |
| `p04/s1` | `152ae5f7` / `e7f08c23` | `fbf9491a` | Merged `pdf.render`, `pdf.to_images`, `pdf.thumbnails` implementation, golden PNG fixtures, and tests. |
| `p04/s2` | `99d29540` / `94c78fb4` | `44c46882` | Merged `pdf.text_layout`, `pdf.extract_images`, and `pdf.detect_scanned` implementation and tests. |
| `p04/s3` | `843f1b42` | `59ed9dee` | Merged `pdf.remove_blank`, `pdf.crop_whitespace`, `pdf.split` (`detect = "pixel"`), and `image.to_pdf`. |
| `p04/s4` | `f274b012` / `1c210938` | `a3773562` | Merged shared `scripts/pack/build-pack.sh`, `engines/pdfium/pack/make.sh`, and `.github/workflows/pack-pdfium.yml`. |
| `p04/s5` | `a78fe652` / `046b4aeb` | `ffe0fdbf` | Merged host/server `pdfium` pack registration, `engine_protocol = "ariad-engine/2"`, and sandbox tests. |
| `origin/fix/pack-exec-signed-bound` | `e5c3796e` | `db0cda81` | Merged signed-manifest executable allowlist hardening (`#145`). |

Local slice branches `p04/s1`, `p04/s2`, `p04/s3`, `p04/s4`, and `p04/s5` were deleted after merging.

## 2. Approved Contracts and Integration Decisions

### 2.1 Extra Approved Contract: Signed Pack Manifest `executables` (`fe259201`)

- **`crates/ariad-host/src/packs/manifest.rs` & `schemas/pack.v1.json`**:
  - Added `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub executables: Vec<String>` to `Target`.
  - Validated every path in `Target::validate` via `relative_path(..., "target executable")` so empty paths, absolute paths, `..`, and backslashes are rejected before installation or resolution.
  - Regenerated `schemas/pack.v1.json` and updated `crates/ariad-host/tests/pack_schema.rs`.
- **`crates/ariad-host/src/packs/extract.rs` & `crates/ariad-host/src/packs/author.rs` & `crates/ariad-cli/src/main.rs`**:
  - Added `scan_with_executables` to record regular files inside `.tar.zst` and `.zip` components that have executable permissions or Windows executable extensions (`.exe`, `.dll`, `.pyd`, `.bat`, `.cmd`, `.com`), including hardlink targets that inherit executable status.
  - Added `--executable <PATH>` to `ashift __pack manifest` and `executables: Vec<String>` to `ManifestRequest`.
  - `generate_manifest` populates `target.executables` from scanned archive executables, `--executable` flags, and any `--base` target `executables`, filtering out `manifest.notices` (unless equal to `entrypoint[0]`), sorted and deduplicated.
- **`crates/ariad-host/src/packs/install.rs` & `crates/ariad-host/tests/pack_resolve_verify.rs`**:
  - Tightened `manifest_allowed_executables(manifest, target)` to admit only `target.entrypoint[0]`, `target.executables` (excluding `manifest.notices` unless `entrypoint[0]`), and `Unpack::File` components with `component.executable == true`.
  - Removed `record.executables` and `receipt.files` from `allowed_executables` in `manifest_verified_entries`: unsigned local `receipt.toml` and `.locks/<name>-<version>.manifest` records are retained for integrity/cache diagnostics only and never widen the executable allowlist.
  - Updated `tampered_manifest_record_executables_cannot_widen_allowed_set` to tamper with both `receipt.toml` and `.locks/<name>-<version>.manifest` simultaneously and to verify that legacy manifests without `executables` reject any non-entrypoint executable.
- **Pack builders & docs**:
  - Updated `scripts/pack/build-pack.sh` (`check_manifest_output` requires `^executables = \[`) and `scripts/pack/test-pack-tooling.sh` (58/58 assertions pass).
  - Updated `engines/docling/pack/build.py` (`archive_executables`, `target_block`, `write_manifest`) and added `test_archive_executables_and_target_block_emit_declared_executables` in `engines/docling/tests/test_pack.py`.
  - Updated `engines/pdfium/pack/make.sh` so `verify` parses both inline and multi-line `notices` arrays in `pack.toml`.
  - Documented `targets[].executables` in `docs/packs.md` and added the `Signed pack manifest executables` entry to the `ARCHITECTURE.md` Decision Log.

### 2.2 Multi-Format `ToolSpec` / `ToolDescriptor`, `pdf.split` Pixel Routing, and `ariad-wasm` Early Refusal (`2917d1f8`)

- **Multi-format tool specs (`crates/ariad-core`, `crates/ariad-host`, `schemas/tools.v1.json`)**:
  - Extended `ToolSpec` with `output_formats: &'static [&'static str]` and `ToolDescriptor` with `output_formats: Vec<String>` (omitted on the wire when empty).
  - `pdf.render`, `pdf.to_images`, `pdf.extract_images`: `input_formats = ["pdf"]`, `output_format = "png"`, `output_formats = ["png", "jpeg", "webp", "bmp", "tiff", "zip"]`.
  - `pdf.thumbnails`: `input_formats = ["pdf"]`, `output_format = "png"`, `output_formats = ["png", "json"]`.
  - `image.to_pdf`: `input_formats = ["png", "jpeg", "webp", "bmp", "tiff"]`, `output_format = "pdf"`.
  - Updated `ariad-host::tools::publish` and `ariad-host::tools::engine` to validate artifact formats and extensions against `spec.allowed_output_formats()`.
  - Regenerated `schemas/tools.v1.json`.
- **`pdf.split` pixel blank-separator routing**:
  - Added `SplitDetectMode` (`Content` default vs `Pixel`) to `SplitParams` in `crates/ariad-core/src/tools/params/split.rs` (`detect = "pixel"` requires `mode == "blank-separator"`).
  - `ariad-host::tools::run::run_tool` inspects `pdf.split` parameters after JSON Schema validation: when `mode == "blank-separator"` and `detect == "pixel"`, it dispatches out of process to the `pdfium` engine (`ariad-engine/2`); otherwise it runs in-process in `ariad-pdf`.
- **`ariad-wasm` early refusal (Mac PM condition 5)**:
  - In `crates/ariad-wasm/src/api.rs`, `run_tool` checks `pdf.split` with `detect == "pixel"` and any tool without `ToolRuntime::Wasm` (all 8 `pdfium`-engine tools), returning `tool_missing` (`exit_code = 5`) before opening or parsing input bytes.
  - `image.to_pdf` runs in-process in `ariad-wasm` across `png`, `jpeg`, `webp`, `bmp`, and `tiff`.

### 2.3 `image.to_pdf`, Shared Blank-Page Helpers, and `ariad-pdfium-engine` Wiring (`6c1def61`)

- **`image.to_pdf` (`crates/ariad-pdf/src/image_to_pdf.rs`)**:
  - Enabled `bmp` and `tiff` features on the `image` crate in `crates/ariad-pdf/Cargo.toml` alongside `png`, `jpeg`, and `webp`.
  - Embeds JPEG streams directly with `/DCTDecode`, reconstructs RGB/grayscale/alpha `/FlateDecode` streams from PNGs without double-decoding when possible, and decodes WebP/BMP/TIFF/indexed/16-bit images with bounded pixel (`Limits.max_image_pixels`, default 100 MP) and byte budgets (`Limits.max_decompressed_bytes`).
- **Shared blank-page helpers (`crates/ariad-pdf` & `crates/ariad-pdfium-engine/src/blank.rs`)**:
  - Exposed `ariad_pdf::tools::delete_pages::delete_page_numbers` and `ariad_pdf::tools::split::split_non_blank_runs` so `pdf.remove_blank` and `pdf.split` (`detect = "pixel"`) in `ariad-pdfium-engine` share exact structural page deletion, bookmark/named-destination pruning, and `.part-NNN.pdf` naming with `ariad-pdf`.
- **Third-party licences (Mac PM condition 4)**:
  - Regenerated `THIRD_PARTY_LICENSES` via `just licenses` and updated `engines/pdfium/pack/notices/PDFIUM-PACK.txt` to list the newly enabled `fax` (`0.2.7`, MIT) and `tiff` (`0.11.3`, MIT) crates.

### 2.4 Protocol v2 Conformance, Malicious PDF Corpus, Fuzz Target, and Docs (`460e743f`)

- **Protocol v2 conformance & malicious corpus (Mac PM conditions 2 & 3)**:
  - Added `crates/ariad-pdfium-engine/tests/conformance_and_malicious.rs` and extended `crates/ariad-cli/tests/engine_conformance_v2.rs` to validate every `RequestV2` and `EventV2` message emitted by the real `ariad-pdfium-engine` binary against `schemas/engine-protocol.v2.json`.
  - Tested `javascript-action.pdf` (no JS execution), oversized MediaBox (30,000 x 30,000 pt at 300 DPI rejected with `limit_exceeded`), cyclic `/Pages` tree, truncated streams, and password-encrypted PDFs across `ariad-pdfium-engine`.
  - Added `fuzz/fuzz_targets/pdfium_engine_request.rs` and seeds in `fuzz/seeds/pdfium_engine_request/`, registered in `fixtures/manifest.toml`.
- **CLI integration & docs (`crates/ariad-cli/tests/pdfium_tools_cli.rs`, `docs/tools.md`, `docs/parity-matrix.md`)**:
  - Added end-to-end CLI tests in `crates/ariad-cli/tests/pdfium_tools_cli.rs` covering missing-pack exit code `5` + install hint and live execution of all 9 PDFium/image tools (`pdf.render`, `pdf.to_images`, `pdf.thumbnails`, `pdf.extract_images`, `pdf.text_layout`, `pdf.detect_scanned`, `pdf.remove_blank`, `pdf.split` with `detect=pixel`, `pdf.crop_whitespace`).
  - Updated CLI snapshots (`commands_cli` and `tools_cli`) and documented all 9 Phase 4 tools and `pdf.split` `detect=pixel` in `docs/tools.md` and `docs/parity-matrix.md`.

## 3. Rendered Sample Artifacts

Generated via `ashift tool pdf.render fixtures/pdf-tools/multipage-20.pdf --param pages=1-2 --param dpi=150 -o /tmp/agent-herdr/runs/p04-int/rendered-samples --overwrite`:

- `/tmp/agent-herdr/runs/p04-int/rendered-samples/multipage-20.page-001.png` (`87,854` bytes)
- `/tmp/agent-herdr/runs/p04-int/rendered-samples/multipage-20.page-002.png` (`89,292` bytes)

## 4. macOS Binding and Sandbox Verification Commands (Mac PM Condition 6)

On the macOS arm64 host (`aarch64-apple-darwin`), run:

```bash
# 1. Build ashift + ariad-pdfium-engine and assemble/verify the local macOS arm64 pdfium pack
#    (fetches pinned pdfium-mac-arm64.tgz sha256=3dc8ef588fd1e54ccfbbe93fe43cd1ec71cd61fbccaa62bb5b15ca32ca6d0ef7,
#     bundles notices, and ad-hoc codesigns bin/ariad-pdfium-engine and bin/libpdfium.dylib):
just pack-pdfium
export ASHIFT_PACK_PDFIUM="$PWD/target/packs/pdfium"
export ARIAD_TEST_PDFIUM_DIR="$PWD/target/packs/pdfium/bin"

# 2. Verify pack tooling (including macOS codesign and manifest executables checks):
ASHIFT_BIN="$PWD/target/debug/ashift" just pack-tooling-test

# 3. Verify live libpdfium.dylib binding, protocol v2 conformance, malicious corpus, and all 9 tools:
cargo test --locked -p ariad-pdfium-engine

# 4. Verify macOS Seatbelt sandbox confinement, pack resolution, and signed-manifest executable allowlist:
cargo test --locked -p ariad-host --features test-probe --test pdfium_pack --test pdfium_sandbox --test sandbox_macos --test sandbox_refusal_macos --test pack_resolve_verify

# 5. Verify end-to-end CLI execution and doctor round-trip on macOS:
cargo test --locked -p ariad-cli --features ariad-host/test-probe --test pdfium_tools_cli --test engine_conformance_v2
cargo run --locked -p ariad-cli -- doctor
```

## 5. Gate Command Results

All heavy commands were executed under `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

| Gate Command | Status | Details |
| --- | --- | --- |
| `cargo fmt --all --check` | PASS | 0 formatting diffs |
| `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-pdfium-engine -p ariad-host -p ariad-server -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` | PASS | 0 warnings |
| `cargo check --locked -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown` | PASS | Compiles cleanly for `wasm32-unknown-unknown` |
| `cargo test --locked -p ariad-wasm` | PASS | 15 passed, 0 failed |
| `just pack-pdfium` | PASS | Built `pdfium-0.1.0-x86_64-unknown-linux-gnu-engine.tar.zst` and verified `target/packs/pdfium` (24 notices, `executables = ["bin/ariad-pdfium-engine"]`) |
| `ASHIFT_BIN="$PWD/target/debug/ashift" just pack-tooling-test` | PASS | 58 passed, 0 failed |
| `cargo test --locked -p ariad-core -p ariad-pdf -p ariad-engine-io -p ariad-pdfium-engine` | PASS | `ariad-core`: 320 passed; `ariad-pdf`: 125 passed; `ariad-engine-io`: 5 passed; `ariad-pdfium-engine`: 53 passed (0 failed) |
| `cargo test --locked -p ariad-host --features test-probe` | PASS | 421 passed, 0 failed (including `pack_resolve_verify`: 9, `pack_schema`: 2, `packs`: 33, `pdfium_pack`: 11, `pdfium_sandbox`: 3) |
| `cargo test --locked -p ariad-server` | PASS | 202 passed, 0 failed |
| `cargo test --locked -p ariad-cli --features ariad-host/test-probe` | PASS | 190 passed, 0 failed, 11 ignored |
| `MISE_UV_VERSION=0.13.1 UV_NO_CONFIG=1 uv run --no-project --python 3.14 --with pytest pytest engines/docling/tests/test_pack.py -k test_archive_executables_and_target_block_emit_declared_executables` | PASS | 1 passed, 0 failed |
| `cargo run --locked -p ariad-cli -- tool pdf.render fixtures/pdf-tools/multipage-20.pdf --param pages=1-2 --param dpi=150 -o /tmp/agent-herdr/runs/p04-int/rendered-samples --overwrite` | PASS | Wrote `multipage-20.page-001.png` and `multipage-20.page-002.png` |
| `cargo run --locked -p ariad-cli -- doctor` | PASS | `✓ pdfium engine 0.2.0 passes describe round trip; protocol ariad-engine/2` |
| `just licenses-check` | PASS | Rust and JS third-party licences verified |
| `just deny` | PASS | `advisories ok, bans ok, licenses ok, sources ok` |
| `just spell` | PASS | `typos` 0 errors |
| `bash scripts/test-ci-scope.sh` | PASS | 90 passed, 0 failed |
| `(cd fuzz && cargo check --locked)` | PASS | All fuzz targets compile cleanly |
| `sh scripts/check-commits.sh origin/dev..HEAD` | PASS | All commits conform to Conventional Commits with no AI/tool references |
