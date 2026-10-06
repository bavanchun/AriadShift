# AriadShift

![AriadShift logo](brand/png/ariadshift-lockup-1200.png)

AriadShift is an open-source, local-first document transformation platform in
early development. Its goal is to turn documents into clean Markdown and
structured data through a shared engine used by local and hosted interfaces.

The current repository ships one local conversion route: Markdown to DOCX.

## Convert Markdown to DOCX

Convert `.md` or `.markdown` files with Pandoc 3.12 or newer in the supported
3.x series:

```sh
just pandoc
ashift convert notes.md --to docx
```

The default output is `notes.docx` beside the input. Use `-o` to choose another
path and `--overwrite` to replace an existing destination. Stdout contains the
output path; warnings are written to stderr. Press Ctrl-C to cancel a running
conversion and clean up its temporary workspace. Set `ASHIFT_PANDOC` to select
an existing Pandoc executable instead of installing the repository-pinned one.

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
`pnpm-workspace.yaml`, and `pnpm-lock.yaml` own the JavaScript workspace.

## Project documents

- [System architecture](ARCHITECTURE.md)
- [Security reporting](docs/SECURITY.md)
- [Git workflow](docs/git-workflow.md)
- [License](LICENSE) and [third-party notices](NOTICE)
