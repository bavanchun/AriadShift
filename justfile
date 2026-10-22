export ASHIFT_PANDOC := justfile_directory() / ".tools" / "pandoc" / "bin" / ("pandoc" + if os() == "windows" { ".exe" } else { "" })
export CARGO_BUILD_WARNINGS := "deny"
set positional-arguments

default: ci

fmt:
    cargo fmt --all

lint: fmt-check clippy spell

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-targets --features ariad-host/test-probe

spell:
    typos

lint-tools:
    sh scripts/install-lint-tools.sh

lint-workflows: lint-tools
    .tools/bin/actionlint
    uvx zizmor@1.30.1 .github

lint-commits range="origin/dev..HEAD": lint-tools
    sh scripts/check-commits.sh {{range}}

wasm:
    cargo check -p ariad-core -p ariad-wasm --target wasm32-unknown-unknown

wasm-bindgen-tool:
    sh scripts/install-wasm-bindgen.sh

# Builds the browser module into packages/ariad-wasm/pkg (not committed).
wasm-build: wasm-bindgen-tool
    cargo build -p ariad-wasm --target wasm32-unknown-unknown --profile wasm-release
    .tools/bin/wasm-bindgen --target web --out-dir packages/ariad-wasm/pkg target/wasm32-unknown-unknown/wasm-release/ariad_wasm.wasm

test:
    cargo nextest run --workspace --features ariad-host/test-probe
    cargo test --workspace --doc

deny:
    cargo deny check

deny-advisories:
    cargo deny check advisories

js:
    pnpm install --frozen-lockfile

# Biome lint and format check of the JavaScript workspace.
web-lint:
    pnpm exec biome check

# Vitest across the JavaScript packages; the wasm bindings must be built first.
web-test: wasm-build
    pnpm exec vitest run

# TypeScript 7 checks of every JavaScript package.
web-typecheck: wasm-build
    pnpm -r --if-present typecheck

# Development server for the web app on http://localhost:3000.
web-dev: wasm-build
    pnpm --filter @ariadshift/web dev

# Production build; every route is statically generated. Serve it with `just web-start`.
web-build: wasm-build
    pnpm --filter @ariadshift/web build

# Serves the production build on http://localhost:3100.
web-start:
    pnpm --filter @ariadshift/web start

# Static export into apps/web/out, with out/_headers for the converter's headers.
web-export: wasm-build
    pnpm --filter @ariadshift/web export

# Installs the browsers the end-to-end tests use.
web-browsers:
    pnpm --filter @ariadshift/web exec playwright install chromium firefox webkit

# Playwright on Chromium, Firefox and WebKit against `next start`; run `just web-build` first.
web-e2e:
    pnpm --filter @ariadshift/web e2e

# Lighthouse on / and /app, mobile and desktop, against `next start`; run `just web-build` first.
web-lighthouse:
    pnpm --filter @ariadshift/web lighthouse

# The JavaScript checks of `just ci`, apart from Biome in `just static`.
web-ci: web-typecheck web-test web-build

py:
    uv lock --check

fixtures:
    uv run --package ariad-fixture-gen python -m ariad_fixture_gen

bench *args:
    uv run --package ariad-bench python -m ariad_bench run "$@"

bench-check:
    uv run --package ariad-bench python -m ariad_bench check

bench-test:
    uv run --package ariad-bench pytest bench/tests

bench-mutation:
    uv run --package ariad-bench pytest bench/tests/test_rendered_vs_truth.py -k test_structural_drift_mutations_detected

# Pinned version of cargo-about used for license inventory checks.
# .github/workflows/ci.yml pins the same version in the static job install-action step.
cargo_about_version := "0.9.2"

check-release-workflow:
    sh scripts/check-release-workflow.sh
    sh scripts/test-check-release-workflow.sh

test-winget-manifest:
    sh scripts/test-winget-manifest.sh

licenses-check:
    #!/usr/bin/env bash
    set -euo pipefail
    actual=$(cargo about --version 2>/dev/null || true)
    if [ "$actual" != "cargo-about {{cargo_about_version}}" ]; then
        echo "ERROR: cargo-about {{cargo_about_version}} required, found '$actual'" >&2
        exit 1
    fi
    tmp=$(mktemp)
    trap 'rm -f "$tmp"' EXIT
    cargo about generate about.hbs > "$tmp"
    if ! cmp -s "$tmp" THIRD_PARTY_LICENSES; then
        echo "ERROR: THIRD_PARTY_LICENSES is out of date. Run 'cargo about generate about.hbs > THIRD_PARTY_LICENSES' to update." >&2
        diff -u THIRD_PARTY_LICENSES "$tmp" | head -n 30 || true
        exit 1
    fi

# Checks that do not depend on the OS; CI runs them once.
static: fmt-check spell lint-workflows deny js web-lint py check-release-workflow licenses-check test-winget-manifest

package-check:
    cargo package --workspace --allow-dirty

ci: static clippy wasm test bench-test bench-check package-check web-ci

pandoc:
    bash scripts/install-pandoc.sh

# Run fuzz targets on Linux (default: all targets, 60 s each)
[linux]
fuzz target="" seconds="60":
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "{{target}}" ]; then
        targets=("{{target}}")
    else
        targets=("markdown_reader" "front_matter" "html_reader" "pandoc_ast_to_ir" "ir_json" "limits_validate")
    fi
    for t in "${targets[@]}"; do
        echo "=== Running fuzz target: $t ({{seconds}}s) ==="
        mkdir -p "fuzz/corpus/$t"
        cargo fuzz run -s none "$t" "fuzz/corpus/$t" "fuzz/seeds/$t" -- -max_total_time="{{seconds}}" -timeout=10 -rss_limit_mb=2048
    done

[macos]
[windows]
fuzz target="" seconds="60":
    @echo "fuzzing runs on Linux only" && exit 1
