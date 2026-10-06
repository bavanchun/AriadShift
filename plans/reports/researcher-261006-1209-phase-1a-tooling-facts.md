# Phase 1a tooling facts (verified 2026-10-06)

Scope: rmcp, cargo-fuzz, dist (cargo-dist), Pandoc readers and writers, native HTML parsing, bench metrics.
Method: registry APIs (crates.io, PyPI, formulae.brew.sh, GitHub Releases API), source checkouts at release tags, and local experiments on Linux x86_64 with Rust 1.99.0 and the repo's Pandoc 3.12 binary (`.tools/pandoc/bin/pandoc`).
Tags: **[tested]** = run locally; **[source]** = read in a tagged source tree; **[doc]** = official docs only; **[unverified]** = could not confirm.

## Outcome

1. **rmcp 3.5.1** works with tokio 1.53.2 and schemars 1.2.2 as the repo pins them. A stdio server that returns `resource_link` content compiled and answered `tools/call` **[tested]**. Bump the ARCHITECTURE pin from 3.5 to 3.5.1.
2. **cargo-fuzz 0.13.2 runs on stable 1.99 with `-s none`.** A 5-second libFuzzer run completed **[tested]**. Only AddressSanitizer needs nightly, so fuzzing in CI needs no nightly toolchain. Run fuzzing on Linux only.
3. **dist 0.33.0** is the latest stable release on GitHub (2026-09-11). crates.io still stops at 0.32.0. dist handles SHA pinning through `[dist.github-action-commits]`. Two of its defaults must change: it uses `ubuntu-22.04` runners, which are being deprecated, and it does not pin the dist installer to a checksum.
4. **Homebrew core `pandoc` is 3.12**, and dist can declare it with `[dist.dependencies.homebrew] pandoc = { stage = ["run"] }`. Homebrew has no Intel-macOS pandoc bottle.
5. Pandoc 3.12 emits `pandoc-api-version [1,23,1,2]`. `--extract-media` **works under `--sandbox`** for DOCX and EPUB input **[tested]**.
6. Under `--sandbox`, EPUB output has a **fixed UUID shared across all documents** and `--epub-cover-image` **fails with exit 99** **[tested]**. Without the sandbox, the UUID is random even when `SOURCE_DATE_EPOCH` is set. Always pass `identifier` explicitly.
7. For the HTML reader, use **html5ever 0.40.1 directly** with our own `TreeSink`. It builds for wasm32. Tree building slows quadratically with nesting depth (20k nested `<div>` takes 1.0 s) **[tested]**, so we must enforce a depth cap ourselves.
8. No permissively licensed TEDS package exists. PubTabNet, OmniDocBench and `table-recognition-metric` all depend on GPL edit-distance libraries. Write about 100 lines of TEDS on top of **apted (MIT) + rapidfuzz (MIT)**, and use **jiwer 4.0.0 (Apache-2.0)** for CER and WER.

---

## 1. rmcp (official Rust MCP SDK)

| Item | Fact | Source |
|---|---|---|
| Latest | `rmcp` / `rmcp-macros` **3.5.1** (2026-10-05), Apache-2.0, MSRV 1.88 | https://crates.io/crates/rmcp, https://github.com/modelcontextprotocol/rust-sdk/releases/tag/rmcp-v3.5.1 |
| Deps | `tokio ^1` (sync, macros, rt, time); `schemars ^1.0` optional (`chrono04`); `process-wrap ^10.0` optional | crates.io dependency API |
| Resolution with repo pins | tokio **1.53.2**, schemars **1.2.2**; no duplicates **[tested]** | `cargo tree -i` |
| Default features | `base64`, `macros`, `server` (`server` pulls in `schemars`) | crates/rmcp/Cargo.toml @ rmcp-v3.5.1 |
| Minimal features for `ashift mcp` | `default-features = false, features = ["server", "macros", "transport-io"]` **[tested]** | — |
| MCP spec | Latest revision **2026-07-28** (released 2026-07-28). Previous: 2025-11-25 | https://github.com/modelcontextprotocol/modelcontextprotocol/releases |
| SDK protocol constants | `ProtocolVersion::LATEST = 2026-07-28`. Revision 2026-07-28 replaces the `initialize` handshake with per-request `_meta` (SEP-2567). `LATEST_WITH_INITIALIZE = 2025-11-25`. A client that sends `initialize` gets 2025-11-25 back **[tested]** | crates/rmcp/src/model.rs |

The pattern below compiled and ran, and it passes `#![forbid(unsafe_code)]` in the binary crate **[tested]**:

```rust
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ConvertArgs { /// Input path
    input: String, to: String }
#[derive(Clone)] struct S;
#[tool_router(server_handler)]
impl S {
    #[tool(description = "Convert a document")]
    fn convert(&self, Parameters(a): Parameters<ConvertArgs>) -> Result<CallToolResult, rmcp::ErrorData> {
        let link = Resource::new("file:///…/out.docx", "out").with_mime_type("application/vnd.openxmlformats-officedocument.wordprocessingml.document");
        Ok(CallToolResult::success(vec![ContentBlock::text("converted"), ContentBlock::resource_link(link)]))
    }
}
// main: S.serve(rmcp::transport::stdio()).await?.waiting().await?;
```

- `tools/list` returned an `inputSchema` with `$schema` set to draft 2020-12, and the doc comments became `description` fields. This matches the default dialect of MCP 2025-11-25 and later.
- **API rename:** the content type is now `ContentBlock` (`Text | Image | Audio | Resource | ResourceLink`). Older examples on the web use `Content::…` and no longer compile.
- **Returning files** has three options:
  - `ContentBlock::resource_link(Resource)` returns a URI with a name, MIME type and size. This is preferred for local DOCX/EPUB outputs because it avoids base64-encoding megabytes.
  - `ContentBlock::resource(ResourceContents::BlobResourceContents{uri, mime_type, blob})` embeds the base64 bytes.
  - `Json<T>` (with `schemars::JsonSchema`) adds `structuredContent` plus an `outputSchema`. It suits a plan or inspect report.
- To make `resources/read` serve link targets, implement `ServerHandler::list_resources/read_resource` manually. The `#[tool_router(server_handler)]` shortcut covers tools only **[source]**.
- `serverInfo.name` defaults to `"rmcp"`. Override it in `get_info` **[tested]**.
- Logs must go to stderr, because stdout carries the protocol (official example).

## 2. cargo-fuzz

| Item | Fact | Source |
|---|---|---|
| Latest | **cargo-fuzz 0.13.2** (2026-06-09), MIT OR Apache-2.0; `libfuzzer-sys 0.4.13` (2026-06-04), `(MIT OR Apache-2.0) AND NCSA` | crates.io; https://github.com/rust-fuzz/cargo-fuzz/releases/tag/0.13.2 |
| Prebuilt binaries | x86_64 linux-musl, x86_64 apple-darwin, x86_64 windows-msvc, each with an API sha256 digest (Linux: `sha256:b5b70401…931c`). Not in taiki-e/install-action's native manifest, so that action falls back to binstall | GitHub Releases API |
| Nightly requirement | Docs say nightly is required. In the code, nightly is needed only for `-Zsanitizer` (the default is ASan), `-Zbuild-std` (MSan or `--build-std`) and `--careful`. Sancov instrumentation uses stable `-C` flags | src/project.rs @ 0.13.2 |
| **Stable 1.99** | `cargo fuzz run -s none <t> -- -max_total_time=5` ran 14M execs in 6 s **[tested]**. The default (ASan) fails with "1 nightly option were parsed" **[tested]** | local |
| `#![deny(unsafe_code)]` in a fuzz target | Compiles with `fuzz_target!` **[tested]** | local |
| Platforms | README: "x86-64 and Aarch64 … only Unix-like (not Windows)". The book claims Windows works through MSVC ASan, and the maintainers do not test it (issues #450, #358 report a missing `clang_rt.asan_dynamic` DLL and broken crash reports). **Run fuzzing on Linux only.** | https://github.com/rust-fuzz/cargo-fuzz#installation, https://rust-fuzz.github.io/book/cargo-fuzz/setup.html, issues #358/#450 |
| Layout | `fuzz/fuzz_targets/<t>.rs`, `fuzz/corpus/<t>/`, `fuzz/artifacts/<t>/`. `cargo fuzz init` writes `fuzz/.gitignore` with `target corpus artifacts coverage` | src/project.rs, local |
| New in 0.13.2 | `cargo fuzz init --fuzz-engine libfuzzer|libafl` | CHANGELOG |

Recommended CI shape:
- **PR job (ubuntu-26.04, stable 1.99, no nightly):** `cargo fuzz run -s none <target> fuzz/seeds/<target> -- -max_total_time=60 -rss_limit_mb=2048 -max_len=<reader limit+1>` for each target. Upload `fuzz/artifacts/` when the job fails.
- **Nightly scheduled job (optional):** install a dated nightly (`rustup toolchain install nightly-YYYY-MM-DD`) and run `cargo +nightly-… fuzz run` with ASan for longer. `cargo +toolchain` overrides `rust-toolchain.toml`. ASan matters less here because our crates deny `unsafe`, but html5ever, ego-tree and comrak's dependencies contain `unsafe`.
- **Corpus:** commit small hand-picked seeds in a non-ignored directory such as `fuzz/seeds/<target>/`, or remove `corpus` from `fuzz/.gitignore`. Persist the grown corpus with `actions/cache`, not git. Commit minimized crashers (`cargo fuzz tmin`) as regression tests in the crate's normal test suite.
- **Workspace:** root `members = ["crates/*"]` does not include `fuzz/`. Use `cargo fuzz init --fuzzing-workspace=true` or add `exclude = ["fuzz"]`. Then the Windows `cargo build --workspace` never compiles libFuzzer's C++, and cargo-deny never sees the NCSA licence. The fuzz crate is dev-only and never distributed.
- **Alternatives:** I did not recommend them. cargo-bolero 0.13.5 (MIT) gives one harness for libFuzzer, AFL and Kani plus a `cargo test` fallback, but adds a dependency for no 1a need. afl.rs 0.18.2 (Apache-2.0) needs the AFL++ toolchain and is slower to set up in CI.

## 3. dist (cargo-dist)

| Item | Fact | Source |
|---|---|---|
| Latest stable | **v0.33.0** (2026-09-11, not a prerelease) on GitHub. **crates.io max is 0.32.0** (2026-05-22) | https://github.com/axodotdev/cargo-dist/releases/tag/v0.33.0, https://crates.io/crates/cargo-dist |
| 0.33 changes | Azure Artifact Signing (x86_64 Windows only); flat-layout `env` script moved to `~/.config/$APP`; `--repo` fix for `gh attestation` | release notes |
| Generates | `dist-workspace.toml` (config) and `.github/workflows/release.yml` (plan → build-local → build-global → host → publish-homebrew-formula → announce) driven by tag push; shell/PowerShell installers; `<formula>.rb`; `sha256.sum`; `dist-manifest.json` | templates @ v0.33.0 |
| Targets | `x86_64/aarch64-apple-darwin`, `x86_64/aarch64-unknown-linux-gnu`, `x86_64/aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc` (+ `aarch64-pc-windows-msvc` per runner table) | book/src/reference/config.md#targets |
| Homebrew tap | `tap = "bavanchun/homebrew-tap"`, `publish-jobs = ["homebrew"]`, `installers = ["shell","powershell","homebrew"]`, optional `formula = "ashift"`. The tap repo must already exist. Add a secret named **`HOMEBREW_TAP_TOKEN`** to `bavanchun/AriadShift`, with write access to the tap. The publish job checks out the tap with that token and runs `brew style --fix` from `/home/linuxbrew` (Homebrew 7.0.6 is on ubuntu-26.04 images) | book/src/installers/homebrew.md, partials/publish_homebrew.yml.j2, runner-images Ubuntu2604-Readme |
| Runtime dependency | `[dist.dependencies.homebrew] pandoc = { stage = ["run"] }` becomes `depends_on "pandoc"` in the formula. The default stage is `build` only. `version` is ignored on Homebrew | config.md#dependencies, installer/homebrew.rb.j2 |
| SHA pinning | Default refs are tags (`checkout@v6`, `upload-artifact@v7`, `download-artifact@v8`, `attest@v4`, `swatinem/rust-cache@v2`, `setup-node@v6`, `azure/login@v2`). Override each with `[dist.github-action-commits] "actions/checkout" = "<sha>"` (since 0.29). Hand edits to `release.yml` fail `dist plan`'s consistency check unless `allow-dirty = ["ci"]` | src/backend/ci/github.rs, config.md |
| Not pinnable | dist installs itself with `curl … cargo-dist-installer.sh \| sh` (version-pinned by `cargo-dist-version`, no checksum). `brew update` is also unpinned. Accept these or document them as an exception | src/backend/ci/mod.rs |
| Default runners | linux `ubuntu-22.04`, linux-arm `ubuntu-22.04-arm`, mac arm `macos-14`, mac intel `macos-15-intel`, windows `windows-2022`. Ubuntu 22 images began deprecation on 2026-09-17, and macos-14 is fully unsupported from 2026-11-02 (runner-images#13518, from earlier research). Override with `[dist.github-custom-runners]` including `global` | github.rs, https://github.com/actions/runner-images |

Homebrew `pandoc`: stable **3.12**, revision 0, GPL-2.0-or-later. Bottles exist for `arm64_golden_gate`, `arm64_tahoe`, `arm64_sequoia`, `arm64_linux` and `x86_64_linux`, with **none for Intel macOS**. Intel-mac users would build Pandoc from source with GHC, which is very slow. Source: https://formulae.brew.sh/api/formula/pandoc.json.

Notes for planning:
- `cargo-dist = "0.32"` in `[workspace.dependencies]` is misplaced. dist is a CLI, configured through `cargo-dist-version` in `dist-workspace.toml`, and should not be a Cargo dependency.
- Linux builds on ubuntu-26.04 raise the glibc floor of `-gnu` binaries. Choose the `*-unknown-linux-musl` targets, or set `github-custom-runners` to `ubuntu-24.04` for the gnu builds.

## 4. Pandoc readers → JSON AST (3.12)

- `pandoc-types` constraint: `>= 1.23.1.2 && < 1.24`, latest on Hackage 1.23.1.2. The JSON header is `"pandoc-api-version":[1,23,1,2]` **[tested]**, and readers accept any `[1,23,…]`. Sources: https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal, https://hackage.haskell.org/package/pandoc-types.
- `pandoc --sandbox -f docx|epub|html -t json` all worked **[tested]**:
  - DOCX: metadata comes from core.xml. Images have target `media/rIdN.png`, and the bytes are **not** in the JSON.
  - EPUB: the meta includes `identifier`, `date` and `language`.
  - HTML: the meta includes `generator` and `viewport` from `<meta>` tags, which should be filtered.
- `--extract-media=<dir>` **works under `--sandbox`** for DOCX (`<dir>/media/rId9.png`) and EPUB (`<dir>/media/file0.png`), and image targets are rewritten to those paths **[tested]**. The host reads each extracted file, hashes it into `AssetStore` and rewrites the reference to `Asset { id }`. Extract into the per-job tempdir only.
- The HTML reader does not fetch relative or remote images under the sandbox, which is the desired SSRF posture. `data:` URIs survive.
- Mapping: use our own serde types (`#[serde(tag = "t", content = "c")]`). The `pandoc_ast`/`pandoc_types` crates are stale (from earlier research).

## 5. Pandoc writers: HTML and EPUB3

HTML **[tested]**:
- `-s --embed-resources` under `--sandbox` keeps `data:` images. Relative paths stay unembedded, with a warning and exit 0.
- A plain `-s` writes no `<script>` or `<link>` tags for this input.
- `--self-contained` is the deprecated alias of `--embed-resources --standalone` **[doc]**.
- Because the IR already stores assets as bytes, emit `data:` URIs into the JSON AST, and embedding works under the sandbox.

EPUB3 **[tested]**:

| Mode | dc:date / dcterms:modified | dc:identifier | Byte-identical runs |
|---|---|---|---|
| `--sandbox` | 1970-01-01 | **constant `urn:uuid:42a14256-…` for every document** | yes |
| no sandbox + `SOURCE_DATE_EPOCH=1700000000` | 2023-11-14T22:13:20Z | random per run | **no** |
| no sandbox + SDE + `identifier:` in metadata | SDE | as given | yes |
| `--sandbox` + `identifier:` | 1970 | as given | yes (expected; 1970 date) |

- Under the sandbox, SDE is ignored, matching the DOCX behaviour from earlier research. Choose between "sandbox with a 1970 date" and "SDE with no sandbox", or post-process `content.opf` the same way the DOCX metadata rewrite works.
- **Always set `identifier`**, for example a UUIDv5 derived from the input hash. Without it, sandboxed EPUBs collide on the same identifier.
- If the title is missing, Pandoc writes **no `dc:title` and gives no warning**, so the output is invalid EPUB3 (dc:title is required). Default the title to the file stem. `lang` defaults to `en-US`, so set it explicitly.
- `--epub-cover-image=<file>` under the sandbox fails with **exit 99, "not found in resource path"**, even when the file is passed on the command line. The `cover-image` metadata with a `data:` URI also fails. Add the cover by post-processing the zip, or run the trusted IR-generated AST without the sandbox for that one route. The second option is a security trade-off that needs a decision.
- I did not run epubcheck on any of these outputs **[unverified]**.

## 6. Native HTML parsing in `ariad-core`

| Crate | Version / date | License | Fit | Notes |
|---|---|---|---|---|
| **html5ever** | 0.40.1 (2026-09-14), repo active 2026-10-05 | MIT OR Apache-2.0 | **Rank 1** | WHATWG-conformant tokenizer and tree builder from Servo. MSRV 1.85. Builds for wasm32-unknown-unknown **[tested]**. Implement `TreeSink` onto our own arena. `markup5ever_rcdom` is published as `0.39.0+unofficial`, so do not use it |
| scraper | 0.27.0 (2026-05-11) | ISC | Rank 2 | Arena DOM (`ego-tree`) and CSS selectors. Pins **html5ever 0.39**, so using both duplicates html5ever, and it pulls in cssparser/selectors we do not need. 20k-node trees drop without stack overflow because of the arena **[tested]** |
| lol_html | 3.0.1 (2026-07-29) | BSD-3-Clause | Reject | Cloudflare's streaming rewriter. It builds no document tree, which is the wrong shape for reading into the IR |
| tl | 0.7.8 (2024-01-29) | MIT | Reject | Last push 2024-08. Not spec-conformant |

- **Depth limits:** html5ever exposes no depth or size option **[source]**. The parse time measured through scraper on 1.99 release builds was:

  | Input | Time |
  |---|---|
  | 1k nested `<div>` | 2.3 ms |
  | 5k nested `<div>` | 66 ms |
  | 20k nested `<div>` | 1.03 s (quadratic) |
  | 5k misnested `<b><i>…<p>` | 6.6 ms |

  Enforce the limits in our `TreeSink`. Count open-element depth in `append`, and once it exceeds the limit, stop attaching or flatten and return a typed `LimitExceeded`. Also cap the input byte size before parsing. Chromium caps DOM depth at 512 **[unverified, from memory]**.
- `#![forbid(unsafe_code)]` applies to our crate only. Dependencies still contain `unsafe` (counted in sources: html5ever 6, ego-tree 47, lol_html 4 occurrences).

## 7. Bench metrics (permissive only)

| Need | Pick | Version / license | Notes |
|---|---|---|---|
| CER / WER (Python) | **jiwer** | 4.0.0 (2025-06-19), Apache-2.0; depends on `rapidfuzz>=3.9.7` (MIT) | jiwer releases before 3.x used GPL `python-Levenshtein`. Pin 4.x |
| Edit distance (Python) | **rapidfuzz** | 3.14.6 (2026-08-30), MIT | `rapidfuzz.distance.Levenshtein` is a drop-in for GPL `Levenshtein` |
| Tree edit distance | **apted** | 1.0.3 (2017), MIT | Algorithm is stable; repo dormant since 2017. Pure Python, slow on tables above about 500 cells |
| Tree edit distance (alt) | zss | 1.2.0 (2018), BSD-style (GitHub reports NOASSERTION) | Zhang–Shasha. Usable for heading trees |
| TEDS | **write our own (~100 LOC)**: APTED with a rapidfuzz cell-content cost, following Zhong et al. 2019 (arXiv:1911.10683) | ours (Apache-2.0) | Every packaged TEDS depends on GPL code. PubTabNet `metric.py` imports `distance` (GPL), and its repo has no licence file. OmniDocBench `table_metric.py` (Apache-2.0 repo) imports `Levenshtein` (GPL-2.0+). `table-recognition-metric` 0.0.6 requires `levenshtein` (GPL) |
| Rust options | strsim 0.11.1 (MIT, 2024), rapidfuzz 0.5.0 crate (MIT, 2023, stale), tree-edit-distance 0.4.0 (MIT, 2022, 5 stars) | — | Thin and stale. Keep the bench in Python under `uv` unless the planner must compute metrics in Rust |
| Avoid | `Levenshtein`, `python-Levenshtein`, `Distance` | GPL | — |

Sources: https://pypi.org/project/jiwer/, https://pypi.org/project/rapidfuzz/, https://pypi.org/project/apted/, https://github.com/timtadh/zhang-shasha/blob/master/LICENSE, https://github.com/ibm-aur-nlp/PubTabNet/blob/master/src/metric.py, https://github.com/opendatalab/OmniDocBench/blob/main/src/metrics/table_metric.py, https://pypi.org/project/table-recognition-metric/.

## Trade-offs and ranked decisions

| Decision | Rank 1 | Rank 2 | Why |
|---|---|---|---|
| Fuzz in CI | Stable `-s none`, Linux, 60 s per target on each PR | Plus a nightly ASan cron | No second toolchain on PRs, and the readers are safe Rust |
| HTML reader | html5ever + own TreeSink | scraper | Fewer dependencies, latest html5ever, and depth enforcement in the sink |
| EPUB determinism | Sandbox + explicit `identifier` + post-write `content.opf` date rewrite (reuse the DOCX rewrite) | SDE without the sandbox | Keeps the security boundary |
| dist version | 0.33.0 from the GitHub installer | 0.32.0 (crates.io parity) | Latest stable per policy. crates.io lag is irrelevant because dist is not a Cargo dependency |
| Linux artifacts | musl targets | gnu built on ubuntu-24.04 | Avoids the glibc floor set by 26.04 |
| TEDS | Own implementation on apted + rapidfuzz | — | No permissive package exists |

Adoption risk:
- rmcp: low. Releases are frequent (3.4.1 → 3.5.1 in 12 days), so expect API churn like the `Content` → `ContentBlock` rename.
- dist: medium. axodotdev ships about every 3–4 months, and crates.io publishing has lagged.
- cargo-fuzz: low. It is mature and the release cadence is slow.
- html5ever: low. Servo maintains it, though it still releases at 0.x.
- apted/zss: dormant but algorithmically complete.

## Limitations

- Not run on macOS or Windows, and no dist release dry run (`dist plan`/`dist build`) against this repo.
- epubcheck was not run.
- The cost of a GHC source build of Pandoc for Intel macOS was not measured.
- No benchmark of apted runtime on large tables.

## Unresolved questions

1. Should the EPUB cover route run without `--sandbox`, given the AST is trusted and IR-generated? The alternative is cover injection by post-processing the zip.
2. Should `x86_64-apple-darwin` stay a dist target, given Homebrew has no Intel pandoc bottle?
3. Are the unpinned dist self-install (`curl | sh`) and `brew update` in `release.yml` accepted as exceptions to the SHA-pin policy?
4. musl or gnu-on-24.04 for Linux artifacts?
5. Should ARCHITECTURE be updated to rmcp 3.5.1 and dist 0.33.0, and `cargo-dist` removed from `[workspace.dependencies]`?
