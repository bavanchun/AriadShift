# Research: Phase 0 toolchain and technical choices

Date: 2026-10-06 (Asia/Saigon). Scope: the concrete choices needed to plan Phase 0 · Foundations (see ARCHITECTURE.md sections 2, 5, 6, 7, 14, 16, 17). All versions were read from the registries on 2026-10-06. Items marked **[tested]** were run locally against the official Pandoc 3.12 linux-amd64 binary.

## Recommendations table

| Area | Choice | Version | Verified source |
|---|---|---|---|
| Rust toolchain | stable, edition 2024, `rust-toolchain.toml` with `rustfmt`, `clippy`, target `wasm32-unknown-unknown` | 1.99.0 (2026-10-01) | https://static.rust-lang.org/dist/channel-rust-stable.toml, https://endoflife.date/rust |
| Node.js | 24 LTS now, switch `.node-version` to 26 after 2026-10-28 | 24.21.0 (26 LTS from 2026-10-28) | https://nodejs.org/dist/index.json, https://endoflife.date/nodejs |
| pnpm | `packageManager` field in root `package.json` | 12.9.1 | https://registry.npmjs.org/pnpm |
| uv | `required-version` in root `pyproject.toml` | 0.12.23 | https://github.com/astral-sh/uv/releases |
| Python (uv workspace only) | `requires-python = ">=3.14"` | 3.14.8 | https://endoflife.date/python |
| just | task runner | 1.58.0 | https://github.com/casey/just/releases |
| Pandoc | official release binaries, SHA-256 pinned | 3.12 (2026-09-29) | https://github.com/jgm/pandoc/releases/tag/3.12 |
| Pandoc AST JSON | own serde types in `ariad-core`, emit `[1,23]`, accept any `1.23.*` | pandoc-types 1.23.1.2 (Pandoc emits `[1,23,1,2]`) | https://hackage.haskell.org/package/pandoc-types, **[tested]** |
| Markdown reader | comrak, `default-features = false` | 0.55.0 (BSD-2-Clause) | https://crates.io/crates/comrak |
| Child processes | tokio::process + process-wrap (`tokio1`, `process-group`, `job-object`, `kill-on-drop`) | tokio ~1.53 (1.53.2), process-wrap 10.0.1 | https://crates.io/crates/process-wrap, https://github.com/tokio-rs/tokio#lts-releases |
| NDJSON framing | tokio-util `LinesCodec::new_with_max_length` | 0.7.19 | https://crates.io/crates/tokio-util |
| Serialization | serde / serde_json | 1.0.229 / 1.0.151 | https://crates.io/crates/serde |
| Schema | schemars (draft 2020-12 is the default) | 1.2.2 (MIT) | https://crates.io/crates/schemars, generate.rs source |
| Schema validation in tests | jsonschema (dev-dep) | 0.58.5 | https://crates.io/crates/jsonschema |
| CLI | clap (derive) | 4.6.7 | https://crates.io/crates/clap |
| Errors | thiserror | 2.0.21 | https://crates.io/crates/thiserror |
| Test runner | cargo-nextest (+ `cargo test --doc`) | 0.9.146 | https://github.com/nextest-rs/nextest/releases |
| Snapshots / golden | insta (+ cargo-insta locally) | 1.49.0 | https://crates.io/crates/insta |
| DOCX reading in tests | zip, `default-features = false, features = ["deflate"]` | 8.6.0 | https://crates.io/crates/zip |
| CLI integration tests | assert_cmd (dev-dep) | 2.2.2 | https://crates.io/crates/assert_cmd |
| Temp workspaces | tempfile | 3.27.0 | https://crates.io/crates/tempfile |
| License/advisory gate | cargo-deny | 0.20.2 | https://github.com/EmbarkStudios/cargo-deny/releases |
| Spell lint | typos-cli | 1.50.3 | https://github.com/crate-ci/typos/releases |
| JS license check (later) | built-in `pnpm licenses list --prod --json` | pnpm 12.9.1 | local `pnpm licenses --help` |
| Python license check (later) | `uvx pip-licenses --allow-only ... --partial-match` | 5.5.5 (MIT) | https://pypi.org/project/pip-licenses/ |
| GH Action: checkout | `actions/checkout` | v7 (7.0.1) | https://github.com/actions/checkout/releases |
| GH Action: Rust + cache | `actions-rust-lang/setup-rust-toolchain` (reads `rust-toolchain.toml`, wraps Swatinem/rust-cache) | v2 (2.0.0) | https://github.com/actions-rust-lang/setup-rust-toolchain/releases |
| GH Action: binary tools | `taiki-e/install-action` for just, cargo-nextest, cargo-deny, typos (checksum-verified prebuilt binaries) | v2 (2.87.25) | https://github.com/taiki-e/install-action/releases |
| GH Action: Node + pnpm | `pnpm/action-setup` + `actions/setup-node` (`node-version-file: .node-version`) | v6 (6.1.0) / v7 (7.0.0) | https://github.com/pnpm/action-setup/releases, https://github.com/actions/setup-node/releases |
| GH Action: uv | `astral-sh/setup-uv` | v10 (10.2.0) | https://github.com/astral-sh/setup-uv/releases |
| Runners | `ubuntu-26.04`, `macos-26` (arm64), `windows-2025`; pin labels, not `-latest` | GA | https://github.com/actions/runner-images |

Alternatives that were checked and ranked lower: `dtolnay/rust-toolchain` (no releases, branch refs only) with `Swatinem/rust-cache@v2` (2.9.2) is the fallback for Rust setup. `extractions/setup-just@v4` is redundant once taiki-e installs just. `pnpm/setup@v3` (2026-09-20) can replace two Node steps but is a young major. `pandoc/actions/setup@v1` is rejected (see Q5).

## Q1. Versions and discrepancies with ARCHITECTURE.md

Everything in section 2 that Phase 0 touches matches the registries today: Rust 1.99, Node 24.21 with Node 26 entering LTS on 2026-10-28 (endoflife.date confirms; EOL 2029-04-30), Python 3.14 (3.14.8), tokio `~1.53` (README lists 1.53.x as LTS until September 2027), serde 1, thiserror 2, clap 4.6, schemars 1.2, pnpm 12, uv 0.12, ruff 0.16 (0.16.10), Pandoc 3.12.

Discrepancies and gaps to fix in ARCHITECTURE.md during Phase 0 (AGENTS.md requires the update, plus a Decision Log row, in the same change):

1. **Binary name.** Sections 5 (`ariad` binary), 6.3 (`$ ariad plan`), 7 (`ariad __engine pdfium`) and 7.1 (`core` pack contains `ariad`) still say `ariad`. The decided name is `ashift`.
2. **New crates not in 2.3.** Phase 0 adds comrak, process-wrap, tokio-util, tempfile, and dev-only zip, insta, jsonschema, assert_cmd. Add at least the runtime ones (comrak, process-wrap, tokio-util) to the table.
3. **CI base image.** Section 2.1 says Ubuntu 26.04 for CI. `ubuntu-latest` still means 24.04 until November 2026 (runner-images announcement #14748), so the workflow must use the explicit `ubuntu-26.04` label.
4. **Section 14.** cargo-nextest does not run doctests; `just test` needs `cargo test --doc` as well.
5. **Node 26.** It enters LTS 22 days from now. Pin 24 for Phase 0 and plan the bump; do not pin 26 before 2026-10-28 (it is still "Current", not LTS).

## Q2. Markdown reader for ariad-core

| Criterion | comrak 0.55.0 | pulldown-cmark 0.13.4 | markdown-rs 1.0.0 |
|---|---|---|---|
| License | BSD-2-Clause (OSI, permissive) | MIT | MIT |
| Maintenance | Active (pushed 2026-10-05, 14 open issues) | Active (release 2026-05-20, pushed 2026-09-30) | **Stale**: last commit and release 2025-04-23, 91 open issues |
| Spec conformance | CommonMark 652/652, GFM 670/670 (port of cmark-gfm, the GFM reference) | CommonMark yes; GFM partial (no extended `www.` autolinks: no option exists) | CommonMark + GFM, 100% claimed |
| Output shape | Arena AST, `sourcepos` on every node | Event stream; tree must be rebuilt; byte ranges via `into_offset_iter` | mdast tree with positions |
| GFM tables, footnotes, task lists | Yes | Yes | Yes |
| Math | `math_dollars`, `math_code` | `ENABLE_MATH` | Yes |
| CommonMark writer (useful for IR → MD in 1a) | Built in (`format_commonmark`) | No (third-party crate) | Separate crate, also stale |
| wasm32 | Pure Rust once `default-features = false` (defaults pull `cli` and `syntect-onig`, a C library) | Pure Rust | Pure Rust |
| Breaking-change rate | High (0.x, frequent minors) | Low | n/a |

**Recommendation: comrak**, with `default-features = false`. It is the only option that is both actively maintained and a faithful GFM reference port, it hands us a tree with source positions (exactly what an IR builder wants), and it gives the Phase 1a Markdown writer for free. pulldown-cmark is the runner-up: smaller and very stable, but GFM gaps and tree reconstruction cost more code. markdown-rs is out because it has been idle for 18 months. The cost of comrak is its churny 0.x API; the lockfile and a thin `reader::markdown` module contain that. Add `BSD-2-Clause` to the cargo-deny allow list. I did not compile comrak to wasm32 myself; the CI wasm check (Q9) proves it on day one.

## Q3. Pandoc AST JSON from Rust

- **pandoc_ast 0.8.6**: last release 2024-01-03, the GitHub repo has no license file (crate metadata says MIT), and it stores the version as a free `Vec<u32>`.
- **pandoc_types 0.6.0**: Apache-2.0, last release 2023-02-11, hard-codes `[1, 23]` and checks only major.minor.
- Pandoc 3.12 depends on `pandoc-types >= 1.23.1.2 && < 1.24` and emits `"pandoc-api-version":[1,23,1,2]` **[tested]**. Its reader accepts any document whose first two components match: `[1,23]` is accepted, `[1,22]` is rejected with "Incompatible API versions" **[tested]**.

The 1.23 types have been stable since 2022, so both crates would technically work, but both are effectively abandoned single-maintainer projects and neither is designed for wasm or for our own mapping tests.

**Recommendation: define our own serde types** in an `ariad-core` module (`pandoc_ast`). It is roughly 300 lines: `Pandoc { pandoc_api_version, meta, blocks }` plus `Block`/`Inline`/`MetaValue` enums with `#[serde(tag = "t", content = "c")]`. That matches Pandoc's `{"t":…,"c":…}` encoding, including unit variants such as `{"t":"Space"}`. Emit `[1,23]` and accept any `1.23.*` on read, mirroring Pandoc's own check. Keep it in `ariad-core` (pure, wasm-compilable) rather than `ariad-host`, because Phase 3 feeds the same JSON to `pandoc.wasm` in the browser. Pandoc's own output is the test oracle: snapshot `pandoc -f gfm -t json` for fixtures and round-trip it through our types.

## Q4. Deterministic DOCX golden tests

Findings, all **[tested]** with Pandoc 3.12:

| Source of variance | Behaviour |
|---|---|
| `docProps/core.xml` created/modified and zip entry mtimes | Without `--sandbox`: wall-clock time. With `SOURCE_DATE_EPOCH=1700000000` (no sandbox): honoured in both core.xml and zip mtimes. **With `--sandbox`: always epoch 0** (core.xml `1970-01-01T00:00:00Z`, zip DOS floor `1980-01-01`), and `SOURCE_DATE_EPOCH` is ignored. |
| Repeat runs | Byte-identical with `--sandbox`, including across stdin vs file input and with `+RTS -M512M -RTS`. |
| Zip entry order | Fixed (16 entries, same order every run). |
| rsid | `<w:rsids>` is emitted empty; there are no per-run rsids. |
| Image names | Sequential relationship IDs (`word/media/rId9.png`), deterministic for a given AST. |
| `w:id` / `docPr id` / bookmarks | Sequential, deterministic. |
| Images under `--sandbox` | Relative image paths are **not readable** ("Could not fetch resource", exit 0, image silently dropped). `data:` URIs work and embed correctly. |

So the XML parts are deterministic for a pinned Pandoc version. The zip bytes are deterministic on one platform, but I could not prove byte equality across the Linux/macOS/Windows builds, which may link different zlib/deflate implementations.

**Recommended golden strategy (stable on all three OSes):**

1. Always run Pandoc with `--sandbox`. That makes the output timestamp-free without relying on `SOURCE_DATE_EPOCH`.
2. Never compare `.docx` bytes. In the test, open the docx with the `zip` crate. Assert the sorted entry list, then compare each XML part's text (`[Content_Types].xml`, `word/document.xml`, `word/footnotes.xml`, `word/numbering.xml`, `word/_rels/*.rels`) through insta snapshots stored under `fixtures/golden/` (`insta::Settings::set_snapshot_path`). That gives one review flow (`cargo insta review`) for both IR snapshots and DOCX goldens (DRY). For `styles.xml`, `theme1.xml` and `fontTable.xml`, which come from Pandoc's built-in reference.docx, snapshot a SHA-256 rather than 27 KB of XML: it still detects a Pandoc upgrade without noisy diffs. Media files: compare a SHA-256 per entry.
3. Also apply a defensive normaliser that blanks `dcterms:created`/`dcterms:modified` before comparing. It costs one regex and protects the test if the sandbox time behaviour ever changes.
4. Make the test assert `pandoc --version` equals the pinned `3.12` and fail with a clear message otherwise. Golden XML is only valid per Pandoc version.
5. Optional secondary semantic check: `pandoc -f docx -t json` on the output, compared against the input AST's blocks. It catches content loss that XML snapshots would make easy to bless by accident. Treat it as a nice-to-have, not a Phase 0 requirement.

Consequence for the engine: the IR → Pandoc AST conversion must inline assets as `data:` URIs, or the engine must pass them another way. Phase 0 fixtures may avoid images, but the IR mapping should do data URIs from the start.

## Q5. Installing pinned Pandoc; `--sandbox` semantics

Pandoc publishes no checksum file, but the GitHub release API exposes an asset `digest`, and it matches the SHA-256 I computed from the downloads:

| Asset (3.12) | SHA-256 |
|---|---|
| `pandoc-3.12-linux-amd64.tar.gz` | `67d7d011fed8c8543306022b985b9b2499ab9b74818df91d8727c7e9ebc5ba06` |
| `pandoc-3.12-linux-arm64.tar.gz` | `6cefcf7100e23a99447c26f89d1ff5b253f3407fcef99a9e27ae06f3ed16cb82` |
| `pandoc-3.12-arm64-macOS.zip` | `f148ca09c9f36594db527a9fc988ad736290ce428f79594c50208cd1ec58b3c0` |
| `pandoc-3.12-x86_64-macOS.zip` | `18577f9460c3dc5d2651ad3bab37d513bc2034a5a777fbe18fa0a5acf2e936ea` |
| `pandoc-3.12-windows-x86_64.zip` | `2a77ebc2517d13e95056e76b1cd5b574cfe958ac61aa6058117d80c22ca19b79` |

The planner should re-verify these with `gh api repos/jgm/pandoc/releases/tags/3.12 --jq '.assets[]|.name+" "+.digest'` when writing the script. No Windows arm64 build exists.

**Recommendation:** one repo script, `scripts/install-pandoc.sh`, run as `just pandoc`. It maps `uname -s`/`uname -m` to an asset, downloads it with `curl -fsSL`, verifies against the pinned table, and extracts into a gitignored `.tools/pandoc-3.12/`. The same path serves CI on all three OSes and local Arch. `ariad-host` resolves Pandoc as `ARIAD_PANDOC` env var, then `PATH`. CI either exports `ARIAD_PANDOC` or appends to `GITHUB_PATH`. Script constraints: macOS runners ship **bash 3.2**, so no associative arrays (use `case`). macOS needs `shasum -a 256` as the fallback for `sha256sum`. For Windows zips, use `unzip` if Git Bash has it, otherwise `7z x` (7-Zip 26.03 is on `windows-2025`).

- **Rejected: `pandoc/actions/setup@v1`.** Last release was 2025-07-17. It does no checksum verification, installs the x86_64 `.pkg` on arm64 macOS (Rosetta), and only covers CI, not local setup.
- **Arch Linux:** `extra/pandoc-cli` is still **3.11** and dynamically linked Haskell, while AUR `pandoc-bin` is 3.12. Use the repo script locally too, so local goldens match CI.

**`--sandbox` semantics** (MANUAL 3.12, §options, plus tests): it limits reader and writer IO to files named on the command line. It does not restrict filters or PDF production. Network resource fetching is blocked, `--reference-doc` given on the command line is allowed **[tested]**, and stdin input works **[tested]**. Under the sandbox the clock reads epoch 0 (see Q4). It is not an OS sandbox: OS-level network and filesystem isolation stays the host's job (ARCHITECTURE §11, later phases). Two related safety knobs, both **[tested]**:
- `+RTS -M<n>M -RTS` caps the Haskell heap portably. Exceeding it gives "Heap exhausted" and exit 251. Map `limits.max_memory_mb` to it.
- `--log=<file>` writes structured JSON warnings (`type`, `verbosity`, `message`) even under `--sandbox`. Map them 1:1 to protocol `{"type":"warning"}` events instead of parsing stderr.

The command line the engine should run: `pandoc +RTS -M{mem}M -RTS --sandbox --log={work}/pandoc-log.json -f json -t docx -o {out}/document.docx` with the AST on stdin.

## Q6. Cross-platform engine runner

| Option | Verdict |
|---|---|
| **process-wrap 10.0.1** (watchexec, Apache-2.0 OR MIT, successor to command-group) | **Recommended.** Composable wrappers: `ProcessGroup::leader()` (Unix), `JobObject` (Windows; kills the whole tree including the grandchild `pandoc`), `KillOnDrop`, with a `tokio1` frontend. Active (10.0.1 on 2026-09-23, with 8.x/9.x backports the same day). README says only the latest stable rustc is supported, which is fine with Rust 1.99. |
| command-group 5.0.1 | Superseded; last release 2023-11. |
| Raw tokio::process + `nix` / `windows-sys` | Reinvents job-object and process-group code; more unsafe code to own. |
| wait-timeout | Sync only; no tree kill. |

Runner design for `ariad-host` (Phase 0 needs only the timeout and tree kill):
- Re-exec: `std::env::current_exe()` with args `["__engine", "pandoc"]`, a `#[command(hide = true)]` clap subcommand in `ashift`. The engine binary then spawns `pandoc`. That grandchild is exactly why tree kill matters: a process group on Unix (unless pandoc calls setsid, which it does not), a job object on Windows.
- stdin: write the single request line, then **drop stdin** so the engine sees EOF.
- stdout: `FramedRead::new(stdout, LinesCodec::new_with_max_length(1 << 20))`, then `serde_json::from_str::<Event>`. The max length prevents a malicious or buggy engine from exhausting host memory, which tokio's `lines()` does not.
- stderr: drain **concurrently** (otherwise a full pipe deadlocks the child) into a bounded buffer that keeps the last 64 KiB, for crash reports.
- Timeout: `tokio::time::timeout(limits.timeout_s, …)`. On expiry, call `child.start_kill()` through the wrapper (kills the group or job), then `wait()` to reap.
- Exit: a non-zero exit, or EOF with no `result` event, is a crash (§7 rule). On Windows, make sure the child has exited before deleting the tempdir, because open handles block deletion.
- Paths in requests: require UTF-8 paths (reject otherwise). JSON escaping of `\` is automatic.
- Memory limits beyond Pandoc's `+RTS -M` (job-object memory caps, Unix `setrlimit`) are not needed for Phase 0 acceptance; leave them to the sandbox work.

## Q7. JSON Schema with schemars and drift check

- schemars 1.x defaults to **draft 2020-12** (`SchemaSettings::default()` returns `draft2020_12()`; the source comment advises calling `draft2020_12()` explicitly if you depend on it, so do that). Default features `derive`, `std` are enough. The output is deterministic (serde_json `Map` is sorted unless `preserve_order` is enabled). Leave `preserve_order` off, so field reordering in Rust does not churn the schema.
- Use `#[serde(tag = "type")]` on protocol events so the schema yields a clean `oneOf` discriminated by `type`. Put `$id` values such as `https://ariadshift.dev/schemas/ir.v0.json` (domain TBD) via `#[schemars(extend("$id" = …))]` or by post-processing the root.
- **Drift check, recommended:** a test, `schemas_are_up_to_date`, that generates `schema_for!(Document)` and the engine-protocol schemas and compares them to `schemas/*.json`. When `ARIAD_BLESS=1` it rewrites the files instead. This runs inside `cargo nextest` on all three OSes, needs no git state, and gives a clear failure message. The alternative (a generator binary plus `git diff --exit-code schemas/`) works too, but adds a bin target and depends on line-ending config. Use whichever you prefer, but do not do both.
- Validate the example messages from ARCHITECTURE §7 and every fixture's IR snapshot against the generated schema with `jsonschema` (dev-dependency). That gives the conformance-suite seed for free.
- Schema file names: §7 already fixes `schemas/engine-protocol.v1.json`; use `schemas/ir.v0.json` for IR v0. Note that §6.2 calls the IR `ariad-ir/1`, but Phase 0 says "IR v0". Decide one (see Unresolved).

## Q8. Licensing gate

Minimal `deny.toml` for cargo-deny 0.20 (the `version` field is obsolete; vulnerabilities and unsound advisories always error):

```toml
[graph]
targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-pc-windows-msvc", "wasm32-unknown-unknown"]
[advisories]
yanked = "deny"
unmaintained = "workspace"
[licenses]
allow = ["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib"]
confidence-threshold = 0.9
[bans]
multiple-versions = "warn"
wildcards = "deny"
[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

- All licenses are denied unless allowed (0.20 removed `copyleft`/`deny`), so AGPL and non-OSI licenses are blocked by construction. Only add MPL-2.0 or others when a real dependency needs them, with a comment.
- Set `license = "Apache-2.0"` in `[workspace.package]` so workspace crates pass the check themselves.
- With `[graph] targets` set, one Linux run covers all platforms' dependency graphs. Running it on all three OSes is harmless but triples advisory-DB fetches.
- Later (not Phase 0, since no JS or Python product deps exist yet): `pnpm licenses list --prod --json` filtered by a small allow-list check in a just recipe, and `uvx pip-licenses --python <venv>/python --allow-only "MIT;Apache-2.0;BSD-3-Clause;..." --partial-match`. Phase 1b adds `cargo-about` 0.9.2 for `THIRD_PARTY_LICENSES`.

## Q9. Windows/macOS CI pitfalls

- **`.gitattributes`** (new file, needed before any fixture lands):
  ```gitattributes
  * text=auto eol=lf
  fixtures/** -text
  *.snap text eol=lf
  *.docx binary
  *.png binary
  *.pdf binary
  *.zip binary
  ```
  `fixtures/** -text` keeps fixture bytes exact on every OS, so a CRLF test case stays CRLF and an LF one stays LF. Golden snapshots stay LF so insta compares the same text everywhere.
- **just shell on Windows:** just 1.58 deprecates `windows-shell`/`windows-powershell` in favour of a `[windows]` attribute on `set shell`. Recommendation: **set nothing** and write POSIX-sh one-liners. just's default `sh` resolves to Git's `sh.exe` on `windows-2025`. Never set `shell := ["bash", …]`: on Windows, `bash` can resolve to `C:\Windows\System32\bash.exe` (WSL), and the runner README lists `wslbash.exe` separately. Avoid shebang recipes; call `bash scripts/x.sh` explicitly when a script is needed.
- **Workflow shell:** set `defaults: run: shell: bash` in the workflow. The Windows runner default is pwsh, and that is a classic source of "works on Linux" failures.
- **Matrix:** `fail-fast: false`; `ubuntu-26.04`, `macos-26`, `windows-2025`. All three are free for a public repo.
- **wasm invariant:** `cargo check -p ariad-core --target wasm32-unknown-unknown` in `just ci`. This enforces "I/O-free, WASM-compilable" without scaffolding `ariad-wasm`.
- **Warnings:** setup-rust-toolchain v2 sets `CARGO_BUILD_WARNINGS=deny` by default (cargo 1.97+), which replaces `RUSTFLAGS=-D warnings` and avoids cache invalidation. Keep the default.
- **insta in CI:** `CI=true` is set by Actions, so insta fails on mismatch and does not write `.snap.new`. Use insta redactions or filters for absolute temp paths, which differ by OS and contain `\` on Windows.
- **Path length:** keep fixture paths short (`fixtures/md/0001-heading.md`) and avoid deep nested golden directories. Windows MAX_PATH bites in `target/` plus long test names.
- **Executable names:** `ashift.exe`/`pandoc.exe`. Use `current_exe()` and `which`-style lookup, never a hard-coded name.
- **pnpm on Windows:** set `packageManager: "pnpm@12.9.1"` in the root `package.json`. Moving `brand/pnpm-lock.yaml` to the root workspace lockfile is part of creating the pnpm workspace. Run `pnpm install --frozen-lockfile` in CI.
- **uv:** an empty workspace locks fine **[tested]** (`[tool.uv.workspace] members = ["engines/*"]` plus a non-package root project with `requires-python = ">=3.14"`). CI runs `uv lock --check`.

### Proposed `just ci` content (for the planner)

`fmt --check`, `clippy --workspace --all-targets`, `typos`, `cargo check -p ariad-core --target wasm32-unknown-unknown`, `cargo nextest run --workspace` (includes the schema drift and MD → DOCX golden tests), `cargo test --doc`, `cargo deny check`, `pnpm install --frozen-lockfile`, `uv lock --check`. The brand build is not in `just ci`: resvg PNG output may differ across OSes and is not a Phase 0 acceptance item.

## Scope opinion (KISS)

I agree with not scaffolding `apps/`, `packages/`, `ariad-wasm`, `ariad-server`, `bench/`, `infra/` or `engines/docling` in Phase 0. The wasm check above covers the only invariant those would protect. The pnpm workspace has one member (`brand`) and the uv workspace has none yet. Both exist only to fix the layout and lockfiles. The one item I would push back on is the "IR v0" vs `ariad-ir/1` naming, which should be settled before the schema file name is committed.

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| DOCX `core.xml` created/modified always `1970-01-01` under `--sandbox` (user-visible in Word's file properties) | Certain | Low/Med (product polish) | Accept for Phase 0. Later, the host can patch `docProps/core.xml` after Pandoc, or drop `--sandbox` in favour of an OS sandbox plus `SOURCE_DATE_EPOCH`. This is a product decision. |
| Images silently dropped under `--sandbox` when the AST references file paths (exit 0) | High if overlooked | High (silent data loss) | IR → AST emits `data:` URIs. Treat `CouldNotFetchResource` from `--log` as an error. |
| Pandoc zip bytes differ across OS builds | Unknown | Med (flaky goldens) | Compare extracted XML, never zip bytes. |
| comrak 0.x API churn | High | Low | Lockfile pin; isolate behind one reader module. |
| Pandoc 3.12 is 7 days old; a 3.12.x patch may follow | Med | Low | Golden test pins and asserts the version; bumping regenerates snapshots in one PR. |
| Git Bash `unzip` presence on Windows runners not verified | Low | Low | `7z` fallback in the script. |
| process-wrap is a small project (48 stars), though watchexec-maintained with 17M downloads | Low | Med | Small API surface; replaceable with `nix` plus `windows-sys` if needed. |
| Node 26 LTS bump lands mid-Phase 0 | Certain (2026-10-28) | Low | Single `.node-version` edit; Renovate. |

## Limitations

The cross-OS behaviour of Pandoc (byte equality, Windows path handling under `--sandbox`) was tested on Linux only. comrak's wasm32 build, process-wrap's job-object behaviour and Git Bash's `unzip` were not executed here: no Rust toolchain is installed locally and no Windows/macOS host was available. The first CI run is the verification. Fixture sourcing (≥50 documents with licenses) was out of scope for this report.

## Unresolved questions

1. Is IR v0 called `ariad-ir/0` (schema `ir.v0.json`), or should Phase 0 ship `ariad-ir/1` as §6.2 states?
2. Is a 1970 created date in DOCX acceptable for v0.1, or should the host rewrite `core.xml` (requires a zip rewrite in `ariad-host`)?
3. Should the schema drift check be test-based (recommended) or a `git diff --exit-code` step?
4. What `$id` base URL should the schemas use (project domain)?
5. Should cargo-deny run on one OS with `[graph] targets` (recommended) or on all three?

Status: DONE_WITH_CONCERNS
Summary: All nine questions answered with registry-verified versions and local Pandoc 3.12 tests. Key findings: `--sandbox` makes DOCX output deterministic (epoch-0 timestamps) but silently drops file-path images, Pandoc emits `pandoc-api-version` `[1,23,1,2]` and accepts any `1.23.*`, and comrak plus process-wrap plus own Pandoc AST types are recommended.
Concerns/Blockers: Cross-OS determinism and Windows tooling (unzip, job objects, comrak wasm) are unverified until the first 3-OS CI run. ARCHITECTURE.md needs updates for the `ashift` binary name, the new crates and the explicit Ubuntu 26.04 runner label.
