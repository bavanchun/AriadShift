# Docs vs code audit — origin/dev (e6bd29b), 2026-10-27

Scope: `README.md`, `AGENTS.md`, `ARCHITECTURE.md` and all 17 files under `docs/` on `origin/dev` (e6bd29b), compared with the code on the same ref. The local `dev` checkout is two commits behind (missing #53 and #51) and has no tracked modifications, so every file the two commits did not touch was read from the working tree and every touched file through `git show`/`git grep origin/dev`. Every relative link and heading anchor in the 20 docs resolves. Claims that check out are omitted.

## 1. Verdict

Needs fixes. The docs are largely accurate, but six claims misstate a security boundary, the MCP and job-error contracts, or what the self-hosted stack delivers. About 30 further facts are stale (versions, counts, workflow tables, planned features described as shipped).

## 2. Findings

Line numbers are on `origin/dev`. "Verified" means the auditor read the cited code. The main reviewer re-read every high finding and most medium findings.

### High

| Doc:line | Wrong claim | What the code does | Failure scenario | Corrected wording |
|---|---|---|---|---|
| `ARCHITECTURE.md:32` | The DoclingDocument mapper "reads the JSON that the out-of-process Docling engine wrote, never the user's input file". | A user-supplied `.docling.json` file is parsed in-process. `crates/ariad-host/src/convert.rs:880-882` routes `(DoclingJson, AriadIrJson)` at `Stage::Source` to `read_docling_to_ir(&self.request.input, …)`. `crates/ariad-core/data/capabilities.json:78-94` has the `docling+json → ariad-ir+json` edge on `ariad-core`, and `ARCHITECTURE.md:354` already says this. | A security reviewer using Principle 3 assumes untrusted DoclingDocument JSON never reaches an in-process parser and skips threat-modelling that path. | "…reads the JSON the Docling engine wrote and user-supplied DoclingDocument JSON, in-process, under the same bounds, typed errors and fuzzing (`docling_to_ir`)." List this as a Principle 3 exception. |
| `ARCHITECTURE.md:401` (also `:130`) | Format matrix row: PNG/JPEG/WebP/TIFF are read and written by "image crate / libvips". | No `Cargo.toml` declares `image` or libvips. `capabilities.json` has no edge whose target is an image format. Image input exists only through the Docling pack (`capabilities.json:163-171, 206-220, 238-256`). | A user or integrator expects image output or native image reads and finds no route. `ashift plan --to png` fails. | Read: "Docling pack (layout + OCR)". Write: "none". Mark libvips at `:130` as planned. |
| `docs/mcp.md:85` (also `:123`, `:131`, `:143` per reviewer) | "Relative paths resolve against the server's working directory." | Tool `input`/`output` paths are refused unless absolute (`crates/ariad-host/src/confine.rs:311-318`, `:497-502`: "relative path … is not permitted"). `file://` URIs are accepted (`crates/ariad-cli/src/mcp/tools.rs:62,73,77`) but not documented. Only `--allow-dir` values resolve relative to the cwd (`mcp.md:11`). | An MCP client sends `input: "docs/a.md"` as documented and every call fails. | "Tool `input` and `output` must be absolute paths or `file://` URIs; relative paths are refused. Relative `--allow-dir` values resolve against the server's working directory." |
| `docs/mcp.md:103` | Under `2026-07-28`/stateless sessions, "roots are dynamically queried via `context.peer.list_roots()` upon each tool invocation." | `list_roots` is called only from `on_initialized` and `on_roots_list_changed` (`crates/ariad-cli/src/mcp/mod.rs:67-68, 130-135`). `tools.rs` never queries roots. | A stateless client that relies on roots and starts no `--allow-dir` gets every path rejected as outside the allowed roots. Whether rmcp calls `on_initialized` on stateless requests is unverified. | "Client roots are read after `initialize` and on `notifications/roots/list_changed`; there is no per-call query. Stateless clients must pass `--allow-dir`." |
| `docs/self-hosting.md:76`, `:85`, `:167` (and the browser framing at `:78`) | "The web image bakes the API and auth URLs in at build time, so rebuild `web` after changing them"; "Uploads fail in the browser" troubleshooting. | `infra/docker/web.Dockerfile:64-67` and `infra/compose/docker-compose.yml:376-381` set `NEXT_PUBLIC_ARIAD_API_URL`/`NEXT_PUBLIC_ARIAD_AUTH_URL`, but nothing in `apps/web` reads them. `apps/web` has no SDK import, sign-in UI or upload flow. | An operator rebuilds `web` and debugs browser uploads and sign-in that the shipped web app does not have. | "The `web` service serves the local (in-browser) converter only; no browser cloud client ships yet. Sign-in and uploads are available through the auth service and the `/v1` API and SDK." Drop the rebuild advice and the browser-upload troubleshooting until the client lands. |
| `docs/api.md:103` | A failed job's `error` is one of `limit_exceeded`, `unsupported_route`, `input_io`, `output_io`, `tool_missing`, `storage`/`database`, `failed`. | `sandbox_unavailable` is also a final job error (`crates/ariad-server/src/worker.rs:108`, transient then stored after retries; emitted by `crates/ariad-host/src/convert.rs:360`). `docs/server.md:167` and `self-hosting.md:166` already document it. | An API client with an exhaustive switch over job errors mis-handles a real failure code. | Add "`sandbox_unavailable` (engine isolation was unavailable or failed to launch; final after retries)". |

### Medium

| Doc:line | Wrong or stale claim | Code (evidence) | Corrected wording |
|---|---|---|---|
| `ARCHITECTURE.md:41`, `:677`, `:801` | Renovate opens weekly PRs, patch releases automerge. | No Renovate config. `.github/dependabot.yml` configures weekly, grouped updates for actions, cargo (root, `/fuzz`), npm and uv, with no automerge. | "Dependabot (`.github/dependabot.yml`) opens weekly grouped update PRs; nothing automerges." |
| `ARCHITECTURE.md:51` | Ubuntu is the "single base image for all containers". | The server and worker images use `ubuntu:26.04`. `infra/docker/web.Dockerfile:15-16` uses `node:24.21.0-trixie-slim` and `nginx-unprivileged:1.30.6-alpine-slim`, and `services/auth/Dockerfile` uses node trixie-slim. | "Ubuntu 26.04 for the server and worker images; Node trixie-slim for the web build and auth; nginx-unprivileged for static web." |
| `ARCHITECTURE.md:66-68` | TanStack Query 5, Zustand 5, Zod 4, React Hook Form 7, Motion 14 and Uppy 6 are listed as the web stack. | None of these appears in any `package.json` or the `pnpm-workspace.yaml` catalog. | Remove the rows, or mark them "planned". |
| `ARCHITECTURE.md:106` | serde_json `unbounded_depth` is enabled "in the host". | Root `Cargo.toml:63` enables it in `[workspace.dependencies]`, and `ariad-core` (which compiles to WASM) inherits it. | "enabled workspace-wide; `ariad-core` bounds depth with its own prescan (`json_depth.rs`)", or scope the feature to `ariad-host`. |
| `ARCHITECTURE.md:117` | quick-xml 0.42.0 and hex 0.4.3 are dev-only, "never distributed". | Both are under `[dependencies]` in `crates/ariad-core/Cargo.toml:19,22` and `crates/ariad-host/Cargo.toml:21,25`. | Move both to the runtime rows. |
| `ARCHITECTURE.md:135` | uv / ruff "0.12 / 0.16". | `uv.lock` and `engines/docling/pyproject.toml` pin `ruff==0.17.1`. | "0.12 / 0.17". |
| `ARCHITECTURE.md:213` | `ashift plan <INPUT> --to <FORMAT> [--profile] [--json]`. | `crates/ariad-cli/src/main.rs:186-188` also defines `--engine`. | Add `[--engine <ENGINE>]`. |
| `ARCHITECTURE.md:292` | "Only 5 crates exist initially." | Six crates exist under `crates/`, and the §5 tree lists six. | "Six crates exist…" or drop the count. |
| `ARCHITECTURE.md:359` | Planner cost uses "p50 time per page". | `crates/ariad-core/src/planner/cost.rs:72-78` normalises `p50_ms / 5000` per run, with no page term (`docs/bench.md:64-65`). | "p50 wall-clock time per conversion run". |
| `ARCHITECTURE.md:522`; `docs/docling.md:39` | The warning list includes `content_dropped`. | Nothing emits it. The engine emits `ocr_fallback`, `ocr_language_unsupported` (`engines/docling/src/ariad_docling/ocr.py:122,136,154`) and `partial_conversion` (`convert.py:180`). | Drop `content_dropped` from both docs, or implement it. |
| `ARCHITECTURE.md:660` | The desktop signs in with "System browser OAuth + PKCE…; tokens stored in OS keychain". | No OAuth, PKCE or keychain code exists. `apps/desktop/src-tauri/src/deep_link.rs:35,141` reserves `auth/callback` and the test asserts it produces no action. | "Planned. The `ariadshift://auth/callback` route is reserved." |
| `ARCHITECTURE.md:722` | Cloud mode has a "Delete Now" button and a "Delete after download" option. | No such UI exists. Only the 1-hour server retention is real (`crates/ariad-server/migrations/20261022000001_cloud_schema.sql:28`). | State the 1-hour retention and mark both UI controls as planned. |
| `ARCHITECTURE.md:726` | Desktop and CLI telemetry is opt-in. | No telemetry exists (`apps/desktop/src/app.tsx:865`: "No telemetry collection is implemented"). | "No telemetry is collected; any future collection must be opt-in and exclude content and file names." |
| `ARCHITECTURE.md:756` | "Seven targets" are fuzzed. | `fuzz/fuzz_targets/` has nine, adding `pdf_reader` and `typst_writer`. `fuzz-nightly.yml:65` runs `pdf_reader`. | "Nine targets…", and list both. |
| `ARCHITECTURE.md:878` | Biome 2.5. | `pnpm-workspace.yaml:12` pins `@biomejs/biome` 2.6.2. | "Biome 2.6". |
| `AGENTS.md:7`; `README.md:187-198` | "Current state" and the docs list omit the cloud foundation from #53. | `docs/self-hosting.md`, `services/auth`, `infra/compose` and the `/v1` server exist on origin/dev. | Add one sentence to AGENTS "Current state" linking `docs/server.md` and `docs/self-hosting.md`, and add a `[Self-hosting](docs/self-hosting.md)` link to the README list. |
| `README.md:183-184` | The JS workspace is `apps/*`, `packages/*`, `brand`. | `pnpm-workspace.yaml:5` also includes `services/*`. | Add `services/*`. |
| `docs/cli.md:317`, `:361` | Docling routes are shown as `docx -> ariad-ir+json`. | Production routes are `<pdf, docx, pptx, xlsx, png, jpeg, tiff, webp> -> docling+json` (`capabilities.json:113-115`, `engines/docling/src/ariad_docling/describe.py:16`). The doc copied the test fixture `crates/ariad-cli/tests/test_capabilities.json:92-95`. | List the eight `-> docling+json` routes. |
| `docs/cli.md:373` | `status` is one of `ready`, `missing`, `wrong version`, `not installed`. | Also `cannot start` (`crates/ariad-host/src/commands/engines.rs:43,55,157,384`). | Add `cannot start`. |
| `docs/cli.md:239`; `docs/mcp.md:89`, `:132`, `:142` | `to` accepts only md/docx/html/epub, with a fixed output-extension list. | `plan` accepts any user-facing format (`crates/ariad-host/src/commands/plan.rs:165`), and the MCP tools document eight targets (`tools.rs:64,75`). The output extension only has to match the target (`confine.rs:507-520`). | Use the eight target names from `cli.md:76`: "the output extension must match the target format." |
| `docs/api.md:88-101` | "Every error has the body `{"error": …}`"; the table has no 408 row. | `crates/ariad-server/src/api/mod.rs:265-268` adds tower-http `TimeoutLayer` with 408, which returns an empty body. OpenAPI lists 408 only for `/v1/blobs/{token}`. | Add "408, empty body: the request exceeded `ARIAD_REQUEST_TIMEOUT_S`", and qualify the body sentence. |
| `docs/api.md:48` | "Browsers upload with Uppy's `@uppy/aws-s3`…" | The repo has no Uppy. The SDK uploads with `fetch` (`packages/sdk/src/client.ts:235-238`). | "The planned browser client will use Uppy; the TypeScript SDK sends the presigned PUT with `fetch`." |
| `docs/api.md:46` | Bucket CORS must allow `PUT` and `x-amz-checksum-sha256`. | The self-hosted stack never sets bucket CORS (`infra/compose/seaweedfs/bucket-init.sh`, `start.sh`), and `self-hosting.md` does not mention it. SeaweedFS preflight behaviour was not verified. | Document that the Compose stack sets no bucket CORS, or add a rule in `bucket-init.sh`. |
| `docs/server.md:7` | "One binary, four subcommands", followed by a three-row table. | Public subcommands are `migrate`, `api` and `worker`, plus hidden `__convert` and `__check-helper-network` (`crates/ariad-server/src/main.rs:24-40`). | "One binary with three subcommands (plus hidden internal helpers)." |
| `docs/ci.md:7-21` | The workflow table omits `images.yml` and `toolchain-wheels.yml`. | `images.yml:3-13` builds and pushes GHCR images and signs them with cosign on `v*` (build-only on manual runs). `toolchain-wheels.yml:7-10` runs on dispatch and `toolchain-wheels-v*` tags. | Add both rows. |
| `docs/ci.md:15` | torch-docling builds wheels "on native Linux and Windows x86_64". | The matrix is `x86_64-unknown-linux-gnu` only (`torch-docling.yml:34`). The arm and mac jobs (`:212-213`) only verify upstream wheels. Windows is deferred to #83. | "Builds or reuses the MKL-free CPU wheels on Linux x86_64 (Windows deferred, #83); audits upstream wheels on Linux ARM64 and Apple silicon." |
| `docs/ci.md:21` | Publishes `ariad-core`, `ariad-host`, `ariad-cli`. | `publish-crates.yml:43` also publishes `ariad-typst`. | List all four, in publish order. |
| `docs/ci.md:38` | Jobs 3–6 must have "passed or were skipped". | `ci.yml:568-572`: when the scope is heavy all four must succeed; otherwise all must be skipped. | State that all-or-nothing rule. |
| `docs/ci.md:47`, `:49` | A push to `dev` (or a hotfix to `main`) always runs CI. | `scripts/ci-scope.sh:35-38,58-64` path-scopes pushes and skips docs-only ranges. Only manual runs or an unknown base force a full run (`:28-32`). | Describe the path-scoped behaviour. |
| `docs/ci.md:51`, `:55` | The web and JS-only path lists. | `ci-scope.sh:21,24` also match `services/`, `crates/ariad-typst/` and `schemas/openapi.v1.json`. | Add the three paths. |
| `docs/releasing.md:7-9` | Legacy `toolchain-torch-*` tags are built and published by `toolchain-wheels.yml`. | That workflow triggers and gates only on `toolchain-wheels-v*` (`:9-10`, `:334`). `toolchain-torch-*` appears only as an exclusion in `release.yml:55`. | "Legacy `toolchain-torch-*` tags predate `toolchain-wheels.yml`; no current workflow builds them." |
| `docs/web.md:50` | "Process in Cloud is hidden until the cloud UI is enabled." | No such control or flag exists. The e2e tests assert the text is absent (`apps/web/e2e/converter.spec.ts:128`). | "There is no cloud processing option; such inputs offer Open in Desktop only." |

### Low

| Doc:line | Issue → fix (code evidence) |
|---|---|
| `ARCHITECTURE.md:124-132`, `:783-784`, `:864` | Granite-Docling, PaddleOCR-VL, the `ocr-vlm` pack, llama.cpp, FFmpeg, libvips and NATS are listed with versions, but no pin, lock entry or code reference exists. Label them planned. |
| `ARCHITECTURE.md:269-273` | The §5 tree omits `packages/ariad-typst/`, `fuzz/`, `packaging/`, `vendor/`, `deny.toml`, `dist-workspace.toml`, `lychee.toml` and `typos.toml`. Add them or say the tree is partial. |
| `ARCHITECTURE.md:553` | The plugin list omits `opener` (`apps/desktop/src-tauri/Cargo.toml`: `tauri-plugin-opener`). |
| `ARCHITECTURE.md:607` | The route table omits `PUT`/`GET /v1/blobs/{token}` (`crates/ariad-server/src/api/mod.rs:277`). Add it, marked local-object-store only. |
| `ARCHITECTURE.md:625` | The core image is called `server`, but CI publishes `ariadshift-worker-core` (`images.yml:104,114`). |
| `ARCHITECTURE.md:908`, `:953` | `:908` cites `webpki-root-certs` (CDLA) as a reason to avoid a stack, but it is now in the lock through reqwest, with a wasm32-only exception (`deny.toml:60-64`). `:953` carries a bare tracker code "(A-1710)". Replace it with the invariant (the auth role lacks `CREATE SCHEMA`). |
| `README.md:80-81` | `ashift` never installs Pandoc. It reads `ASHIFT_PANDOC` or `PATH` (`crates/ariad-host/src/pandoc_bin.rs:128-131`), and `just pandoc` installs the pin. |
| `README.md:136-137` | `wasm32-unknown-unknown` is a toolchain target, not a component (`rust-toolchain.toml:3-4`). |
| `README.md:178-179`, `:185` | `just test-server` skips with exit 0 without Docker (`justfile:190-192`). `just web-browsers` only installs browsers (`justfile:94-95`). |
| `docs/cli.md:56`, `:67`, `:315` | Batch warnings are `warning[<code>]: <input>: <message>` (`crates/ariad-cli/src/batch.rs:141-146`). Docling also reads `docx`. `ariad-core` is version `0.1.0`, not `0.2.0` (`Cargo.toml:7`). |
| `docs/mcp.md:165` | Undocumented resource limits: 32 MiB per read and 1000 registered resources (`crates/ariad-cli/src/mcp/resources.rs:17,39,111-115`). |
| `docs/server.md:141` | The work directory is `ariad-job-<32 hex>`, the UUID simple form (`crates/ariad-server/src/storage.rs:674-675`). |
| `docs/self-hosting.md:10` | The default `mem_limit`s sum to 5/13/33 GiB (`docker-compose.yml:272,302,334,364`), not 6/14/34. Say what the extra GiB covers. |
| `docs/self-hosting.md:165`; `docs/server.md:151` | A failed startup probe exits with "ARIAD_SANDBOX=required, but this host cannot isolate engines" (`crates/ariad-server/src/sandbox.rs:140`), not with `sandbox_unavailable`. That code is per job. |
| `docs/ci.md:14`, `:32`, `:36`, `:86`, `:93`, `:108` | `:14`: pack-docling also builds on manual runs and has no Windows target. `:32`: the static list omits `sdk-check` and `check-crypto-bigint` (`justfile:50-52,117-118,179`). `:36`/`:93`: Rust is pinned to 1.99.0 and `just fuzz` is Linux-only (`justfile:283-307`). `:86`: `ariad-wasm` is `publish = false`. `:108`: actionlint declares `ubuntu-26.04` and `ubuntu-26.04-arm`. |
| `docs/releasing.md:204`, `:206` | `CARGO_REGISTRY_TOKEN` is also mapped in `release.yml:477-479` through the reusable workflow's `secrets:`. |
| `docs/git-workflow.md:77`, `:115` | Clippy needs `--features ariad-host/test-probe` (`justfile:16`), or point to `just clippy`. `release.yml` also runs in rehearsal mode on pull requests (`:38-56`, `:68-70`). |
| `docs/bench.md:217` | `pypdfium2` is a runtime dependency of the bench package (`bench/pyproject.toml:12`), not a dev dependency. |
| `docs/brand/design-direction.md:40` | Regenerate desktop icons with `just desktop-icons` (`justfile:348-350`), not `pnpm tauri icon`. |
| `docs/desktop.md:83-84`, `:182-183` | A directory maps to `--from` and anything else to `--manifest` (`apps/desktop/src-tauri/src/backend.rs:424-430`). `APPIMAGE_EXTRACT_AND_RUN` is set by the `just` recipe (`justfile:333-334`), not by the script. |
| `docs/docling.md:67`, `:121`, `:185`; `docs/packs.md:192` | `pins.toml` `[toolchain]` tables are empty placeholders (`:43-75`). Add "(Windows deferred, #83)" to the MKL sentences. Pack CI also triggers on `workflow_dispatch`, `engines/docling.rs` and `docling_pack.rs` (`pack-docling.yml:6-20`). |
| `docs/packs.md:36-46`, `:210` | The example manifest uses `dest = "runtime"` and `docling-runtime-<triple>.tar.zst`, while the real Docling pack uses `dest = "."` and `docling-<version>-<target>-runtime.tar.zst` (`engines/docling/pack/build.py:776`). Label the example schematic. The key file name is lower-case (`d87b0e33f55ea02b.pub`). |
| `docs/web.md:40` | The input cap is 16 MiB (`apps/web/src/converter/session.ts:20`), not 16 MB. |

### Unverifiable from the repository

- **Release state:** `README.md:16,20,25,36,40-41,100` and `AGENTS.md:7` ("v0.1.0 is shipped"). No tags or GitHub Releases are available offline; the config in `dist-workspace.toml` matches.
- **Performance figures with no committed measurement:** `docs/cli.md:179` and `ARCHITECTURE.md:212` (~3 GB peak, ~100×), `ARCHITECTURE.md:346` (230 B RSS per input byte), `docs/bench.md:68` (the max(2 MB, 25%) tolerance is not implemented in `bench/`), `docs/packs.md:175` component sizes, `docs/web.md:114-125` Lighthouse scores, `docs/releasing.md:22-44` rehearsal sizes.
- **External or vendor facts:** Node, Python, Postgres and Next support dates (`ARCHITECTURE.md:47-65`), Postgres 18.6 (the image is pinned by digest only), Better Auth defaults (`docs/api.md:9`, `docs/server.md:132`), SeaweedFS flags (`server.md:206`), the minisign prehash default (`packs.md:94,212`), `HTTPS_PROXY` handling via ureq defaults (`packs.md:103`), the zstd 128 MiB window (`packs.md:163`), the CycloneDX crate set (`releasing.md:170`).
- **GitHub repository settings:** branch protection, merge methods, the `release` environment, and private vulnerability reporting (`docs/SECURITY.md:3`).
- **Other:** `docs/git-workflow.md:125` (the herdr-cook-runs `.gitignore` comes from an external tool); `README.md:149` (cargo-insta 1.49.0 is not pinned in `ci.yml`); `docs/brand/design-direction.md:12,18` (no comparison or contrast artifacts).

## 3. Contract or architecture violations

No code change in scope contradicts AGENTS.md. Four places where the governing docs and the code disagree on a contract:

- **Principle 3 trust boundary** (`ARCHITECTURE.md:32` vs `:354` and `convert.rs:880-882`): user-supplied DoclingDocument JSON is parsed in-process, but Principle 3 says it never is. ARCHITECTURE contradicts itself. Either amend the Principle 3 exception list or route that input through isolation.
- **AGENTS.md "Current state" is stale after #53**: it does not mention the `/v1` server, `services/auth` or the Compose stack. AGENTS.md says it describes the shipped state.
- **Licensing exceptions**: AGENTS.md lists the allowed exceptions (MPL Rust crates, certifi/tqdm, caniuse-lite, OFL, CDLA for TableFormer weights). `deny.toml:60-64` also allows CDLA-Permissive-2.0 for `webpki-root-certs`. The comment says it is selected only on wasm32, which `ariad-server` never builds, so it is not distributed and is not a violation. Even so, neither AGENTS.md nor `ARCHITECTURE.md:908` reflects it.
- **serde_json `unbounded_depth`** is enabled workspace-wide (`Cargo.toml:63`), but `ARCHITECTURE.md:106` says host-only. `ariad-core` (WASM) therefore relies entirely on its `json_depth` prescan.

## 4. Test gaps that matter

- **MCP stateless roots:** `crates/ariad-cli/tests/mcp_stdio.rs` exercises roots only after `initialize`. No test covers a `2026-07-28` stateless call, which is the case `mcp.md:103` documents.
- **Engines output vs production registry:** the `ashift engines` snapshots load `crates/ariad-cli/tests/test_capabilities.json`, so no test renders engines output from the production `capabilities.json`. That is how the fixture-only `docx -> ariad-ir+json` route reached `cli.md`.
- **Job error codes:** the codes are free strings (`worker.rs`), not an OpenAPI enum, so `openapi_drift` cannot catch a doc or client list that misses `sandbox_unavailable`.
- **Docling warning codes:** no test ties emitted warning codes to the documented list (`content_dropped`).
- **Version tables:** no check compares ARCHITECTURE version tables with the lockfiles; ruff and Biome have already drifted.
- **Bench tolerance:** the documented max(2 MB, 25%) reproduction tolerance has no implementation or test in `bench/`.

## 5. Unresolved questions

1. Is in-process parsing of user-supplied `.docling.json` intended? If so, Principle 3 needs the exception; if not, the route should go through the isolated engine.
2. Does rmcp 3.5.1 run `on_initialized` for stateless `2026-07-28` requests? This decides whether `mcp.md:103` is merely wrong wording or a functional gap for stateless clients.
3. Is the browser cloud client still planned for v1 (`plans/261025-1651-ariadshift-v1-completion/phase-05-cloud-product.md`)? Should the unused `NEXT_PUBLIC_ARIAD_*` build args stay until then?
4. Should `content_dropped` be implemented, or removed from ARCHITECTURE and `docling.md`?
5. Is the extra ~1 GiB in the self-hosting memory figures intended headroom for PostgreSQL, SeaweedFS and auth?
6. Does the pinned SeaweedFS answer browser CORS preflights without a bucket rule?
7. Do legacy `toolchain-torch-*` tags exist on the remote? A tag listing was not possible offline.
