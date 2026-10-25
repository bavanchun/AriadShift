# AriadShift

![AriadShift logo](brand/png/ariadshift-lockup-1200.png)

AriadShift is an open-source, local-first document transformation platform. It
turns documents into clean Markdown, HTML, DOCX, EPUB, PDF and structured JSON
through one shared Rust core, used by every surface:

| Surface | Runs on | Guide |
|---|---|---|
| CLI `ashift` | Linux, macOS, Windows | [CLI reference](docs/cli.md) |
| MCP server | Local over stdio (`ashift mcp`), or remote over Streamable HTTP from the API server | [MCP reference](docs/mcp.md) |
| Web app, local mode | The browser, through WebAssembly; files stay on the device | [Web app](docs/web.md) |
| Web app, cloud mode | An AriadShift server, opt-in per file | [Web app](docs/web.md#cloud-mode) |
| Desktop app | macOS (Apple silicon), Windows, Linux | [Desktop app](docs/desktop.md) |
| Self-hosted cloud | One Linux host with Docker Compose | [Self-hosting](docs/self-hosting.md) |
| REST API `/v1` and `@ariadshift/sdk` | The self-hosted server | [Cloud API](docs/api.md) |

The base install converts Markdown, HTML, DOCX and EPUB, extracts digital PDF
text, writes PDF with embedded Vietnamese fonts, and exports IR JSON. A system
LibreOffice adds legacy Office and ODF routes. The signed `docling` engine pack
adds OCR for scans and layout-aware PDF, Office and image extraction.

## Installation

[docs/install.md](docs/install.md) covers every channel, checksum and Minisign
verification, the first launch of the unsigned desktop builds, and the runtime
prerequisites. The short version:

```sh
# CLI, Linux and macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/bavanchun/AriadShift/releases/latest/download/ariad-cli-installer.sh | sh

# CLI, Homebrew (macOS and Linux; installs Pandoc as a dependency)
brew install bavanchun/tap/ashift
```

```powershell
# CLI, Windows
irm https://github.com/bavanchun/AriadShift/releases/latest/download/ariad-cli-installer.ps1 | iex
```

[GitHub Releases](https://github.com/bavanchun/AriadShift/releases) also carry
the standalone CLI archives with `.sha256` files, the desktop installers
(`.dmg`, `-setup.exe`, `.msi`, `.deb`, `.rpm`, `.AppImage`) with `SHA256SUMS`
and Minisign signatures, and the Homebrew formula. Desktop builds are not
code-signed: macOS Gatekeeper and Windows SmartScreen ask for confirmation on
first launch ([steps](docs/install.md#3-desktop-installation-and-first-run-guide)).
Container images for the cloud stack are published to GHCR.

> [!NOTE]
> DOCX and EPUB routes run Pandoc (`>= 3.12, < 4`) as a separate process.
> Install it from the [official release](https://github.com/jgm/pandoc/releases)
> (distribution packages are often older) or set `ASHIFT_PANDOC` to its path.
> Homebrew installs it for you. `ashift doctor` reports what is missing.

## CLI quick start

```sh
ashift convert notes.md --to docx
ashift convert report.docx --to md
ashift convert book.epub --to html
ashift convert draft.md notes.md --to docx -o output/  # existing directory
ashift convert report.pdf --to md
ashift convert notes.md --to pdf
ashift convert notes.md --to json
# Requires system LibreOffice >= 24.2 (or set ASHIFT_SOFFICE)
ashift convert legacy.doc --to md
ashift convert slides.pptx --to pdf
```

The output defaults to `<input stem>.<target extension>` beside the input. Use
`-o` to choose another path, `--overwrite` to replace an existing file, and
`--profile <editable|faithful|fast|private>` to pick the routing goal
(`editable` by default). With several inputs, conversion continues after a
failure and returns the first failure's exit code. Ctrl-C (or SIGTERM/SIGHUP)
cancels a conversion and cleans up its workspace.

Engines run out of process inside a sandbox: Landlock and seccomp on Linux, and
Seatbelt on macOS when `sandbox-exec` is present. `ASHIFT_SANDBOX` selects
`best-effort` (default), `required` or `off`.

```sh
ashift inspect report.docx         # format, structure, reachable targets
ashift plan report.docx --to md    # the chosen route and its scores
ashift engines                     # engines, versions, licenses, packs
ashift engines install docling     # download and verify a signed engine pack
ashift doctor                      # check Pandoc, sandbox, packs and Tesseract
```

Add `--json` to any command for machine-readable output. The docling pack runs
offline once installed (`--from <dir>` installs it without network access); OCR
also needs a system Tesseract 5. See [docs/cli.md](docs/cli.md),
[docs/packs.md](docs/packs.md) and [docs/docling.md](docs/docling.md).

## MCP

Give an AI assistant (Claude Code, Claude Desktop, Cursor and other MCP clients)
local conversion tools confined to the directories you allow:

```sh
ashift mcp --allow-dir ~/Documents
```

The stdio server exposes `list_engines`, `inspect`, `plan` and `convert`.
A self-hosted API server also serves remote MCP at `/mcp` over Streamable HTTP,
authenticated with OAuth 2.1 (dynamic client registration, PKCE) or API keys,
with the same quotas as the REST API. See [docs/mcp.md](docs/mcp.md) for client
configuration and both tool sets.

## Web app

The Next.js app in [`apps/web`](apps/web) runs the core compiled to
WebAssembly. Local mode converts Markdown, HTML, DOCX, EPUB and IR JSON, reads
digital PDF text and writes PDF in the browser; files never leave the device,
and downloaded routes keep working offline. When the build points at an
AriadShift server, cloud mode offers server conversion for Office formats,
scans and large files, plus sign-in, API keys and a jobs page. Nothing is
uploaded until the user chooses the server for a file.

```sh
just js
just web-dev   # http://localhost:3200
```

See [docs/web.md](docs/web.md) for cloud configuration, building and
deploying.

## Desktop app

The Tauri app in [`apps/desktop`](apps/desktop) converts files and batches,
watches folders, manages signed engine packs, opens `ariadshift://` links from
the web app, adds "Convert with AriadShift" to Finder, Explorer and Linux file
managers, and updates itself. Install it from GitHub Releases, or build it with
`just desktop-dev` (development) and `just desktop-build` (local installers).
See [docs/desktop.md](docs/desktop.md).

## Self-hosted cloud

The cloud stack (web app, Better Auth service, `/v1` API, conversion workers,
PostgreSQL with pgmq, SeaweedFS) runs on one Linux host with no external
account:

```sh
cp .env.example infra/compose/.env   # then replace every "replace-with-…" value
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
```

The web app is then at `http://localhost:3000` and the API at
`http://localhost:8080`. Add `--profile office` for LibreOffice routes or
`--profile docling` for OCR. [docs/self-hosting.md](docs/self-hosting.md)
lists the prerequisites (Landlock and seccomp are required for the workers),
profiles, sizing and reverse-proxy rules; [docs/server.md](docs/server.md) is
the reference for every server setting.

## API and SDK

The `/v1` REST API is stable: `schemas/openapi.v1.json` is the contract, and a
snapshot test refuses breaking changes. Callers authenticate with a 15-minute
JWT from the auth service, an API key (`x-api-key`), or anonymously on small
files when the server allows it. A conversion uploads the file through one
presigned `PUT`, creates a job, follows its Server-Sent Events and downloads
the outputs.

[`@ariadshift/sdk`](packages/sdk) is the TypeScript client, generated from the
OpenAPI document with helpers for uploads, events and downloads. It is a
workspace package, not published to npm:

```ts
import { openAsBlob } from "node:fs";
import { createAriadClient } from "@ariadshift/sdk";

const ariad = createAriadClient({
  baseUrl: "http://localhost:8080",
  credentials: { apiKey: process.env.ASHIFT_API_KEY ?? "" },
});
const file = await openAsBlob("report.docx");
const upload = await ariad.upload(file, { filename: "report.docx" });
const job = await ariad.createJob({ uploadId: upload.id, to: "md" });
for await (const event of ariad.events(job.id)) console.log(event.kind);
```

See [docs/api.md](docs/api.md) for the full flow, every endpoint, limits,
errors and the versioning policy.

## Development

Install Rust 1.99.0 with the `rustfmt`, `clippy`, and
`wasm32-unknown-unknown` components, `just` 1.58.0, Node.js 24.21.0, pnpm
12.9.1, Python 3.14 or newer, and uv 0.12.23. The Rust and Node versions are
recorded in [`rust-toolchain.toml`](rust-toolchain.toml) and
[`.node-version`](.node-version); uv's required version is in
[`pyproject.toml`](pyproject.toml).

Install the tools used by the quality gate and snapshot development:

```sh
cargo install --locked just --version 1.58.0
cargo install --locked cargo-nextest --version 0.9.146
cargo install --locked cargo-deny --version 0.20.2
cargo install --locked cargo-insta --version 1.49.0
cargo install --locked typos-cli --version 1.50.3
```

The quality gate uses `just`, `cargo-nextest`, `cargo-deny`, and `typos-cli`;
`cargo-insta` is for snapshot work.

```sh
just pandoc               # download Pandoc 3.12 and verify its checksum
cargo build --workspace
cargo run -p ariad-cli -- --version
just ci                   # the repository quality gate
just fixtures             # regenerate the fixture corpus and its hashes
```

`just test-server` runs the `ariad-server` database and S3 tests against
throwaway PostgreSQL (with pgmq) and SeaweedFS containers; it needs Docker.

Install JavaScript workspace dependencies with `just js`; then build the brand
with `pnpm --dir brand build`. The root `package.json`,
`pnpm-workspace.yaml`, and `pnpm-lock.yaml` own the JavaScript workspace
(`apps/*`, `packages/*`, `services/*` and `brand`). `just ci` also builds the
wasm modules, the web app and the desktop checks; `just web-browsers` and
`just web-e2e` run the browser tests. [docs/ci.md](docs/ci.md) describes the
CI jobs and [docs/git-workflow.md](docs/git-workflow.md) how changes land.

## Project documents

- [Installation and first run](docs/install.md)
- [CLI reference](docs/cli.md), [engine packs](docs/packs.md) and [Docling](docs/docling.md)
- [MCP server reference](docs/mcp.md)
- [Web app](docs/web.md)
- [Desktop app](docs/desktop.md)
- [Self-hosting](docs/self-hosting.md), [server operation](docs/server.md) and [cloud API](docs/api.md)
- [Benchmarks](docs/bench.md) and the [benchmark report](docs/bench-report.md)
- [System architecture](ARCHITECTURE.md)
- [Security reporting](docs/SECURITY.md)
- [Git workflow](docs/git-workflow.md) and [CI](docs/ci.md)
- [Release procedure](docs/releasing.md)
- [License](LICENSE) and [third-party notices](NOTICE)
