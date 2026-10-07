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

# Checks that do not depend on the OS; CI runs them once.
static: fmt-check spell lint-workflows deny js py

ci: static clippy wasm test bench-test bench-check

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
