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

# License check of the JavaScript dependencies that ship in the website.
js-licenses:
    node scripts/check-js-licenses.mjs

# Biome lint and format check of the JavaScript workspace.
web-lint:
    pnpm exec biome check

# Vitest across the JavaScript packages; the wasm bindings must be built first.
web-test: wasm-build
    pnpm exec vitest run

# TypeScript 7 checks of every JavaScript package.
web-typecheck: wasm-build
    pnpm -r --if-present typecheck

# Development server for the web app on http://localhost:3200.
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

# Installs the browsers the end-to-end tests use; CI adds --with-deps for system packages.
web-browsers *flags:
    pnpm --filter @ariadshift/web exec playwright install {{flags}} chromium firefox webkit

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

test-ci-scope:
    sh scripts/test-ci-scope.sh

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
static: fmt-check spell lint-workflows deny js js-licenses web-lint py check-release-workflow licenses-check test-winget-manifest test-ci-scope

# Images for the ariad-server integration tests; ci.yml pins the same digests.
pgmq_image := "ghcr.io/pgmq/pg18-pgmq:v1.13.1@sha256:aa3030fdacf5c199338bfe5c893fb8101e8233687026c4803e1fde61f520dac7"
seaweedfs_image := "chrislusf/seaweedfs:4.48@sha256:4e61d15fd35994cb1e43e1e553dff106794841fd9a99ade2fc8c8bfce4d7872d"

# Run ariad-server's database and S3 tests against throwaway Docker containers.
[unix]
test-server:
    #!/usr/bin/env bash
    set -euo pipefail
    pg=ariadshift-test-pg
    s3=ariadshift-test-s3
    cleanup() { docker rm -f "$pg" "$s3" >/dev/null 2>&1 || true; }
    trap cleanup EXIT
    cleanup
    docker run -d --name "$pg" -p 127.0.0.1:55442:5432 -e POSTGRES_PASSWORD=ariad "{{pgmq_image}}" >/dev/null
    docker run -d --name "$s3" -p 127.0.0.1:58342:8333 \
        -v "{{justfile_directory()}}/infra/test/seaweedfs-s3.json:/etc/seaweedfs/s3.json:ro" \
        "{{seaweedfs_image}}" server -s3 -s3.config=/etc/seaweedfs/s3.json -dir=/data \
        -volume.max=32 -master.volumeSizeLimitMB=16 >/dev/null
    # TCP opens only after the image's init scripts finish.
    for _ in $(seq 1 60); do
        docker exec "$pg" psql -h 127.0.0.1 -U postgres -qtc 'select 1' >/dev/null 2>&1 && break
        sleep 1
    done
    for _ in $(seq 1 60); do
        docker exec "$s3" sh -c 'echo "s3.bucket.create -name ariad-test" | weed shell' 2>/dev/null \
            | grep -q 'created bucket' && break
        sleep 1
    done
    cargo build -p ariad-cli --bin ashift
    ARIAD_TEST_DATABASE_URL=postgres://postgres:ariad@127.0.0.1:55442/postgres \
    ARIAD_TEST_S3_ENDPOINT=http://127.0.0.1:58342 \
        cargo nextest run -p ariad-server

package-check:
    cargo package --workspace --exclude ariad-server --allow-dirty

ci: static clippy wasm test bench-test bench-check package-check web-ci

pandoc:
    bash scripts/install-pandoc.sh

# The docling engine's development environment, models and OCR data live here.
docling_root := justfile_directory() / ".tools" / "docling"
docling_python := docling_root / "python-env" / if os() == "windows" { "Scripts/python.exe" } else { "bin/python" }

# Sync the docling engine environment and fetch its pinned models and OCR data (about 1.6 GiB).
docling-dev:
    UV_PROJECT_ENVIRONMENT="{{docling_root}}/python-env" uv sync --package ariad-docling --frozen
    "{{docling_python}}" engines/docling/pack/fetch.py assets --dest "{{docling_root}}"

# Lint and test the docling engine (needs `just docling-dev`, or only its uv sync for pytest).
docling-test:
    UV_PROJECT_ENVIRONMENT="{{docling_root}}/python-env" uv run --package ariad-docling --frozen ruff check engines/docling
    UV_PROJECT_ENVIRONMENT="{{docling_root}}/python-env" uv run --package ariad-docling --frozen ruff format --check engines/docling
    UV_PROJECT_ENVIRONMENT="{{docling_root}}/python-env" uv run --package ariad-docling --frozen pytest engines/docling/tests

# Run the CLI tests that drive the real docling engine (needs `just docling-dev` and Tesseract).
docling-engine-test:
    ASHIFT_DOCLING_ENGINE='"{{docling_python}}" -I -m ariad_docling' cargo nextest run -p ariad-cli --features ariad-host/test-probe --run-ignored ignored-only --test-threads 2 -E 'binary(docling_engine)'

# Build the unsigned docling engine pack for this host into target/pack-docling (needs uv).
pack-docling out="target/pack-docling":
    cargo build -p ariad-cli --bin ashift
    uv run --no-project --python 3.14 python engines/docling/pack/build.py build --out "{{out}}" --ashift "{{justfile_directory()}}/target/debug/ashift{{ if os() == "windows" { ".exe" } else { "" } }}"

# Convert a document with the pack that `just pack-docling` built, offline on Linux.
pack-docling-verify out="target/pack-docling" input="fixtures/scan/vi-garden-300dpi.png" expect="Khu vườn đọc sách":
    uv run --no-project --python 3.14 python engines/docling/pack/build.py verify --out "{{out}}" --input "{{input}}" --expect "{{expect}}" {{ if os() == "linux" { "--offline" } else { "" } }} --ashift "{{justfile_directory()}}/target/debug/ashift{{ if os() == "windows" { ".exe" } else { "" } }}"

# Run fuzz targets on Linux (default: all targets, 60 s each)
[linux]
fuzz target="" seconds="60":
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "{{target}}" ]; then
        targets=("{{target}}")
    else
        targets=("markdown_reader" "front_matter" "html_reader" "pandoc_ast_to_ir" "docling_to_ir" "ir_json" "limits_validate")
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
