# AriadShift

![AriadShift logo](brand/png/ariadshift-lockup-1200.png)

AriadShift is an open-source, local-first document transformation platform in
early development. Its goal is to turn documents into clean Markdown and
structured data through a shared engine used by local and hosted interfaces.

The repository provides local conversion across Markdown, HTML, DOCX, and EPUB.
The website converts Markdown, HTML, and AriadShift IR JSON entirely in the
browser; files never leave the device.

## Installation

Prebuilt standalone binaries are published on [GitHub Releases](https://github.com/bavanchun/AriadShift/releases) for Linux (`x86_64` and `aarch64` static musl), macOS (Apple Silicon and Intel), and Windows (`x86_64`).

> [!NOTE]
> AriadShift invokes Pandoc (`>= 3.12, < 4`) as an external, out-of-process executable for DOCX and EPUB conversions.
> Install Pandoc from the [official release](https://github.com/jgm/pandoc/releases) (distribution packages are frequently older than 3.12) or set `ASHIFT_PANDOC` to the executable path. The Homebrew formula, once published, installs Pandoc as a dependency; on Intel macOS (`x86_64-apple-darwin`) Homebrew builds Pandoc from source, so the official Pandoc installer package is significantly faster there.

### Shell Installer (Linux & macOS)

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/bavanchun/AriadShift/releases/latest/download/ariad-cli-installer.sh | sh
```

### PowerShell Installer (Windows)

```powershell
irm https://github.com/bavanchun/AriadShift/releases/latest/download/ariad-cli-installer.ps1 | iex
```

### Manual Download

Standalone archives (`.tar.xz` or `.zip`) can be downloaded directly from [GitHub Releases](https://github.com/bavanchun/AriadShift/releases). Each archive includes `ashift`, `LICENSE`, `NOTICE`, and `THIRD_PARTY_LICENSES`, and ships with a `.sha256` checksum.

### Package managers (not yet published)

The release pipeline is ready for Homebrew, crates.io and WinGet, but v0.1.0 has not reached them yet: the Homebrew formula and the crates were not published, and no WinGet submission exists. These commands work once each channel is published:

```sh
# Homebrew (macOS & Linux)
brew install bavanchun/tap/ashift

# crates.io: prebuilt binary through cargo-binstall, or a source build
cargo binstall ariad-cli
cargo install ariad-cli
```

```powershell
# Windows Package Manager
winget install VChun.AriadShift
```

## Convert Documents

Convert between supported formats (`md`/`markdown`, `html`/`htm`, `docx`, and `epub`) with `ashift convert`:

```sh
just pandoc
ashift convert notes.md --to docx
ashift convert notes.md --to html
ashift convert report.docx --to md
ashift convert book.epub --to md
ashift convert draft.md notes.md --to docx -o output/  # existing directory
```

The default output is `<input stem>.<target extension>` beside the input. Use `-o` to choose another
path and `--overwrite` to replace an existing destination. Select a routing goal with
`--profile <editable|faithful|fast|private>` (defaults to `editable`). Same-format conversions (such as `md -> md`)
are refused with exit code 3; destinations matching the input path are refused with exit code 2. Stdout
contains the output path; warnings are written to stderr. Press Ctrl-C (or send SIGTERM/SIGHUP) to cancel a running
conversion and clean up its temporary workspace. Set `ASHIFT_PANDOC` to select
an existing Pandoc executable instead of installing the repository-pinned one.

With several inputs, conversion continues after individual failures and returns the first
failure's exit code. `--json` returns a `results` array. Linux engines use a Landlock/seccomp
sandbox by default; `ashift doctor` reports kernel support and `ASHIFT_SANDBOX` controls the mode.

## Inspect, Plan, and Diagnose

Inspect document structure, preview planned conversion routes, and check local environment readiness:

```sh
ashift inspect report.docx
ashift plan report.docx --to md
ashift engines
ashift doctor
```

Add `--json` to any command for structured machine-readable output. See [`docs/cli.md`](docs/cli.md) for the complete reference.

Heavy engines such as Docling ship as signed engine packs that `ashift engines install <name>` downloads and verifies on demand (`--from <dir>` installs offline); the first published pack, docling, arrives with v0.2.0. See [`docs/packs.md`](docs/packs.md).

## Convert in the Browser

The web app in [`apps/web`](apps/web) runs the same core compiled to
WebAssembly. Files stay in the browser's private storage, the converter keeps
working offline after the first visit, and every route shows its measured
quality. Run it locally:

```sh
just js
just web-dev   # http://localhost:3200
```

See [`docs/web.md`](docs/web.md) for building, testing, and deploying it as a
static site or a Node.js server.

## Model Context Protocol (MCP)

Run an MCP server over standard I/O for AI assistants (such as Claude Code, Claude Desktop, or Cursor):

```sh
ashift mcp --allow-dir ~/Documents
```

The server exposes 4 tools (`list_engines`, `inspect`, `plan`, and `convert`) with strict path confinement, capability-based directory access, dynamic roots synchronization, and `resources/read` support. See [`docs/mcp.md`](docs/mcp.md) for configuration and tool details.

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

Build and inspect the current CLI:

```sh
cargo build --workspace
cargo run -p ariad-cli -- --version
```

Run the repository quality gate with:

```sh
just ci
```

Regenerate the deterministic fixture corpus and refresh its hashes with:

```sh
just fixtures
```

`just pandoc` downloads Pandoc 3.12 and verifies its archive checksum. Run it
before working on tasks that use Pandoc.

Install JavaScript workspace dependencies with `just js`; then build the brand
with `pnpm --dir brand build`. The root `package.json`,
`pnpm-workspace.yaml`, and `pnpm-lock.yaml` own the JavaScript workspace
(`apps/*`, `packages/*`, and `brand`). `just ci` also builds the wasm module
and the web app; `just web-browsers` and `just web-e2e` run its browser tests.

## Project documents

- [CLI reference](docs/cli.md)
- [MCP server reference](docs/mcp.md)
- [Web app](docs/web.md)
- [System architecture](ARCHITECTURE.md)
- [Security reporting](docs/SECURITY.md)
- [Git workflow](docs/git-workflow.md)
- [Release procedure](docs/releasing.md)
- [License](LICENSE) and [third-party notices](NOTICE)
