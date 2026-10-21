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
    cargo check -p ariad-core --target wasm32-unknown-unknown

test:
    cargo nextest run --workspace --features ariad-host/test-probe
    cargo test --workspace --doc

deny:
    cargo deny check

deny-advisories:
    cargo deny check advisories

js:
    pnpm install --frozen-lockfile

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
static: fmt-check spell lint-workflows deny js py check-release-workflow licenses-check test-winget-manifest

package-check:
    cargo package --workspace --allow-dirty

ci: static clippy wasm test bench-test bench-check package-check

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
