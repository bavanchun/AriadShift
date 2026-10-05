export ASHIFT_PANDOC := justfile_directory() / ".tools" / "pandoc" / "bin" / ("pandoc" + if os() == "windows" { ".exe" } else { "" })
export CARGO_BUILD_WARNINGS := "deny"

default: ci

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets
    typos

wasm:
    cargo check -p ariad-core --target wasm32-unknown-unknown

test:
    cargo nextest run --workspace
    cargo test --workspace --doc

deny:
    cargo deny check

js:
    pnpm install --frozen-lockfile

py:
    uv lock --check

fixtures:
    uv run --package ariad-fixture-gen python -m ariad_fixture_gen

ci: lint wasm test deny js py

pandoc:
    bash scripts/install-pandoc.sh
