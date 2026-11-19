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

test-github-token:
    uv run --no-project python scripts/test-github-token.py

lint-workflows: lint-tools test-github-token
    .tools/bin/actionlint
    uv run --no-project python scripts/with-github-token.py uvx zizmor@1.30.1 .github

lint-commits range="origin/dev..HEAD": lint-tools
    sh scripts/check-commits.sh {{range}}

wasm:
    cargo check -p ariad-core -p ariad-wasm -p ariad-typst --target wasm32-unknown-unknown

wasm-bindgen-tool:
    sh scripts/install-wasm-bindgen.sh

# Builds the browser module into packages/ariad-wasm/pkg (not committed).
wasm-build: wasm-bindgen-tool
    cargo build -p ariad-wasm -p ariad-typst --target wasm32-unknown-unknown --profile wasm-release
    .tools/bin/wasm-bindgen --target web --out-dir packages/ariad-wasm/pkg "${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/wasm32-unknown-unknown/wasm-release/ariad_wasm.wasm"
    .tools/bin/wasm-bindgen --target web --out-dir packages/ariad-typst/pkg "${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/wasm32-unknown-unknown/wasm-release/ariad_typst.wasm"

test:
    cargo nextest run --workspace --features ariad-host/test-probe
    cargo test --workspace --doc

# One slice of the nextest run in `test`, split by test count (index counts from 1); the Linux
# Verify parts in ci.yml run the slices in parallel jobs. Keep the arguments in step with `test`.
test-shard index count:
    cargo nextest run --workspace --features ariad-host/test-probe --partition count:{{index}}/{{count}}

# The doc tests in `test`; CI runs them next to the last shard.
test-doc:
    cargo test --workspace --doc

deny:
    node --test scripts/check-crypto-bigint.test.mjs
    node scripts/check-crypto-bigint.mjs
    cargo deny check

deny-advisories:
    cargo deny check advisories

js:
    pnpm install --frozen-lockfile

# License check of the JavaScript dependencies that ship in the website.
js-licenses:
    node scripts/check-js-licenses.mjs

# Catalogue gate for locales/: ICU syntax, argument parity, completeness, and the macOS
# Localizable.xcstrings (fresh generation, every mac key used in Swift); see docs/i18n.md.
i18n-check:
    node --test scripts/i18n-lib.test.mjs scripts/i18n-xcstrings.test.mjs
    node scripts/i18n-check.mjs

# Biome lint and format check of the JavaScript workspace.
web-lint:
    pnpm exec biome check

# Vitest across the JavaScript packages; the wasm bindings and the SDK client come first.
web-test: wasm-build sdk-generate
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

# Playwright on Chromium, Firefox and WebKit; run `just web-ci` first for build and export.
web-e2e:
    pnpm --filter @ariadshift/web e2e

# Lighthouse on / and /app, mobile and desktop, against `next start`; run `just web-build` first.
web-lighthouse:
    pnpm --filter @ariadshift/web lighthouse

# The JavaScript checks of `just ci`, apart from Biome and the SDK drift check in `just static`.
web-ci: web-typecheck web-test web-export web-build web-licenses-check

# Regenerates the committed OpenAPI document, schemas/openapi.v1.json, from the server code.
openapi:
    cargo run --locked -p ariad-server --bin openapi

# Generates the SDK client into packages/sdk/src/generated (not committed).
sdk-generate:
    pnpm --filter @ariadshift/sdk generate

# Fails when the SDK's generator, OpenAPI document or config drifted from packages/sdk/generator.lock.json.
sdk-check:
    pnpm --filter @ariadshift/sdk check

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
    sh scripts/check-dist-includes.sh

test-winget-manifest:
    sh scripts/test-winget-manifest.sh

test-fuzz-gate:
    uv run --no-project --with pyyaml==6.0.3 python scripts/test-fuzz-gate.py

test-ci-scope:
    sh scripts/test-ci-scope.sh
    uv run --no-project --with pyyaml==6.0.3 python scripts/test-fuzz-gate.py

licenses:
    #!/usr/bin/env bash
    set -euo pipefail
    root="${ARIADSHIFT_ROOT_DIR:-{{justfile_directory()}}}"
    node scripts/generate-desktop-licenses.mjs --check-preconditions
    node apps/web/scripts/sync-notices.mjs --check-preconditions
    node scripts/generate-licenses.mjs --check-preconditions --cargo-about-version "{{cargo_about_version}}"
    tmp_rust=$(mktemp "$root/.THIRD_PARTY_LICENSES.XXXXXX")
    tmp_desktop=$(mktemp "$root/apps/desktop/.THIRD_PARTY_JS_LICENSES.XXXXXX")
    trap 'rm -f "$tmp_rust" "$tmp_desktop"' EXIT
    node scripts/generate-licenses.mjs --cargo-about-version "{{cargo_about_version}}" > "$tmp_rust"
    test -s "$tmp_rust"
    ARIADSHIFT_DESKTOP_LICENSES_OUT="$tmp_desktop" just desktop-licenses
    test -s "$tmp_desktop"
    ARIADSHIFT_THIRD_PARTY_LICENSES="$tmp_rust" just web-licenses
    chmod 0644 "$tmp_rust" "$tmp_desktop"
    mv -f "$tmp_desktop" "$root/apps/desktop/THIRD_PARTY_JS_LICENSES"
    mv -f "$tmp_rust" "$root/THIRD_PARTY_LICENSES"

web-licenses:
    node apps/web/scripts/sync-notices.mjs

web-licenses-check:
    node apps/web/scripts/sync-notices.mjs --check

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
    node scripts/generate-licenses.mjs > "$tmp"
    if ! cmp -s "$tmp" THIRD_PARTY_LICENSES; then
        echo "ERROR: THIRD_PARTY_LICENSES is out of date. Run 'just licenses' to update." >&2
        diff -u THIRD_PARTY_LICENSES "$tmp" | head -n 30 || true
        exit 1
    fi
    node scripts/generate-desktop-licenses.mjs > "$tmp"
    if ! cmp -s "$tmp" apps/desktop/THIRD_PARTY_JS_LICENSES; then
        echo "ERROR: desktop JavaScript notices are out of date. Run 'just desktop-licenses'." >&2
        exit 1
    fi
    if [ -d apps/web/public/notices ]; then
        node apps/web/scripts/sync-notices.mjs --check
    else
        node apps/web/scripts/sync-notices.mjs --check-graph
    fi

test-licenses-recipes:
    node --test scripts/licenses-recipes.test.mjs

# Checks that do not depend on the OS; CI runs them once.
static: fmt-check spell lint-workflows deny js js-licenses i18n-check web-lint sdk-check py check-release-workflow licenses-check test-licenses-recipes test-winget-manifest test-ci-scope test-ci-sccache test-docling-pack-inputs test-ci-apt test-macos-release-scripts test-hardening-ci-wiring

test-ci-apt:
    shellcheck scripts/ci-apt.sh scripts/test-ci-apt.sh
    bash scripts/test-ci-apt.sh

test-docling-pack-inputs:
    shellcheck scripts/docling-pack-inputs.sh scripts/test-docling-pack-inputs.sh scripts/docling-engine.sh
    bash scripts/test-docling-pack-inputs.sh

# The macOS release scripts that need no macOS tool: the release mode decision (Developer ID, ad-hoc, unsigned), the
# ad-hoc and Developer ID signing order and flags (one shared bundle walk, scripts/macos-signing-lib.sh) and gates
# against a stub codesign, the Pandoc fetch mode and embed phase
# against stub lipo and codesign, and the appcast script's key handling against stub child commands. macos.yml runs
# the same tests on the macOS runner.
test-macos-release-scripts:
    shellcheck scripts/macos-check-secrets.sh scripts/macos-check-secrets.test.sh scripts/macos-check-secrets-workflow.test.sh scripts/macos-adhoc-sign.sh scripts/macos-adhoc-sign.test.sh scripts/macos-appcast.sh scripts/macos-appcast.test.sh scripts/macos-embed-pandoc.test.sh apps/macos/scripts/embed-pandoc.sh scripts/macos-helpers-lib.sh scripts/macos-signing-lib.sh scripts/macos-sign-notarize.sh scripts/macos-sign-notarize.test.sh
    shellcheck scripts/install-pandoc.test.sh
    bash scripts/install-pandoc.test.sh
    bash scripts/macos-embed-pandoc.test.sh
    bash scripts/macos-check-secrets.test.sh
    bash scripts/macos-check-secrets-workflow.test.sh
    bash scripts/macos-adhoc-sign.test.sh
    bash scripts/macos-sign-notarize.test.sh
    bash scripts/macos-appcast.test.sh

test-ci-sccache:
    shellcheck -x scripts/ci-sccache.sh
    uv run --no-project --with-requirements scripts/r2-cache-requirements.txt python scripts/test-r2-cache-prune.py

# Keep mandatory database proof and workflow publication policy executable.
test-hardening-ci-wiring:
    bash scripts/test-hardening-ci-wiring.sh

# Images for the ariad-server integration tests; ci.yml pins the same digests.
pgmq_image := "ghcr.io/pgmq/pg18-pgmq:v1.13.1@sha256:aa3030fdacf5c199338bfe5c893fb8101e8233687026c4803e1fde61f520dac7"
seaweedfs_image := "chrislusf/seaweedfs:4.48@sha256:4e61d15fd35994cb1e43e1e553dff106794841fd9a99ade2fc8c8bfce4d7872d"

# Run ariad-server's database and S3 tests against throwaway Docker containers.
[unix]
test-server:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
        echo "skipping server integration tests: Docker is unavailable"
        exit 0
    fi
    pg=ariadshift-test-pg
    s3=ariadshift-test-s3
    for container in "$pg" "$s3"; do
        if docker inspect "$container" >/dev/null 2>&1; then
            owner=$(docker inspect -f '{{'{{'}} index .Config.Labels "ariadshift.test-worktree" {{'}}'}}' "$container")
            if [ "$owner" != "{{justfile_directory()}}" ]; then
                echo "refusing to remove $container: it belongs to another worktree" >&2
                exit 1
            fi
        fi
    done
    cleanup() { docker rm -f "$pg" "$s3" >/dev/null 2>&1 || true; }
    trap cleanup EXIT
    cleanup
    docker run -d --name "$pg" --label "ariadshift.test-worktree={{justfile_directory()}}" -p 127.0.0.1:55442:5432 -e POSTGRES_PASSWORD=ariad -e PGCTLTIMEOUT=180 "{{pgmq_image}}" >/dev/null
    docker run -d --name "$s3" --label "ariadshift.test-worktree={{justfile_directory()}}" -p 127.0.0.1:58342:8333 \
        -v "{{justfile_directory()}}/infra/test/seaweedfs-s3.json:/etc/seaweedfs/s3.json:ro" \
        "{{seaweedfs_image}}" server -s3 -s3.config=/etc/seaweedfs/s3.json -dir=/data \
        -volume.max=32 -master.volumeSizeLimitMB=16 >/dev/null
    # TCP opens only after the image's init scripts finish.
    for _ in $(seq 1 180); do
        docker exec "$pg" psql -h 127.0.0.1 -U postgres -qtc 'select 1' >/dev/null 2>&1 && break
        sleep 1
    done
    if ! docker exec "$pg" psql -h 127.0.0.1 -U postgres -qtc 'select 1' >/dev/null 2>&1; then
        docker logs "$pg" >&2
        echo "test PostgreSQL did not become ready within 180 seconds" >&2
        exit 1
    fi
    for _ in $(seq 1 60); do
        docker exec "$s3" sh -c 'echo "s3.bucket.create -name ariad-test" | weed shell' 2>/dev/null \
            | grep -q 'created bucket' && break
        sleep 1
    done
    if ! docker exec "$s3" sh -c 'echo "s3.bucket.list" | weed shell' 2>/dev/null \
        | grep -Eq '(^|[[:space:]>])ariad-test([[:space:]]|$)'; then
        docker logs "$s3" >&2
        echo "test SeaweedFS did not become ready within 60 seconds" >&2
        exit 1
    fi
    cargo build -p ariad-cli --bin ashift
    ARIAD_TEST_DATABASE_URL=postgres://postgres:ariad@127.0.0.1:55442/postgres \
    ARIAD_TEST_REQUIRE_DATABASE=1 \
    ARIAD_TEST_S3_ENDPOINT=http://127.0.0.1:58342 \
        cargo nextest run -p ariad-server
    if [ "${ARIAD_TEST_CONTAINER_SMOKE:-0}" = 1 ]; then ARIAD_TEST_DATABASE_URL=postgres://postgres:ariad@127.0.0.1:55442/postgres just test-server-container; fi

# Build/check the Dockerfiles and run a required-mode worker on the test database.
[linux]
test-server-container:
    bash scripts/test-server-container.sh

# Auth PostgreSQL serialization suite; requires its disposable DB URL and pg_dump 18.
test-auth-db:
    ARIAD_TEST_DATABASE_URL="${ARIAD_TEST_DATABASE_URL:?set a disposable PostgreSQL URL}" ARIAD_TEST_REQUIRE_DATABASE=1 pnpm --filter @ariadshift/auth exec vitest run test/oauth-postgres.test.ts

# Interactive OAuth for remote MCP on the compose core profile: discovery, registration, PKCE, consent, tools/list.
mcp-oauth-e2e:
    bash infra/test/e2e/mcp-oauth.sh

package-check:
    CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/package-check" cargo package --workspace --exclude ariad-server --exclude ariad-desktop --allow-dirty --config 'patch.crates-io.ariad-core.path="crates/ariad-core"' --config 'patch.crates-io.ariad-host.path="crates/ariad-host"' --config 'patch.crates-io.ariad-typst.path="crates/ariad-typst"'

ci: static clippy wasm test bench-test bench-check package-check web-ci desktop-ci

pandoc:
    bash scripts/install-pandoc.sh

# Sync the docling engine environment (.tools/docling) and fetch its pinned models and OCR data (about 1.6 GiB).
# The three docling recipes run scripts/docling-engine.sh, which pack-docling.yml calls too.
docling-dev:
    bash scripts/docling-engine.sh dev

# Lint and test the docling engine (needs `just docling-dev`, or only its uv sync for pytest).
docling-test:
    bash scripts/docling-engine.sh test

# Run the CLI tests that drive the real docling engine (needs `just docling-dev` and Tesseract).
docling-engine-test:
    bash scripts/docling-engine.sh engine-test

# Build MKL-free CPU torch on a matching native x86_64 host (Linux uses Docker).
docling-torch-build target out="torch-out":
    uv run {{ if os() == "windows" { "--package ariad-docling --group torch-build --frozen" } else { "--no-project --python 3.14" } }} python engines/docling/pack/torch_build.py build --target "{{target}}" --out "{{out}}"

# Build the unsigned docling engine pack for this host into target/pack-docling (needs uv).
pack-docling out="target/pack-docling":
    cargo build -p ariad-cli --bin ashift
    uv run --no-project --python 3.14 python engines/docling/pack/build.py build --out "{{out}}" --ashift "{{justfile_directory()}}/target/debug/ashift{{ if os() == "windows" { ".exe" } else { "" } }}"

# Convert a document with the pack that `just pack-docling` built, offline on Linux.
pack-docling-verify out="target/pack-docling" input="fixtures/scan/vi-garden-300dpi.png" expect="Khu vườn đọc sách":
    uv run --no-project --python 3.14 python engines/docling/pack/build.py verify --out "{{out}}" --input "{{input}}" --expect "{{expect}}" {{ if os() == "linux" { "--offline" } else { "" } }} --ashift "{{justfile_directory()}}/target/debug/ashift{{ if os() == "windows" { ".exe" } else { "" } }}"

# Run real-engine protocol, table and OCR tests against the built pack (needs Tesseract).
pack-docling-test out="target/pack-docling":
    uv run --no-project --python 3.14 python engines/docling/pack/build.py conformance --out "{{out}}"

# Sign engine virtual environment and tools.
sign_root := justfile_directory() / "engines" / "sign"
sign_python := sign_root / ".venv" / if os() == "windows" { "Scripts/python.exe" } else { "bin/python" }

# Sync the sign engine environment.
sign-dev:
    uv sync --directory engines/sign --frozen

# Lint and test the sign engine and shared common licenses.
sign-test:
    uv run --directory engines/sign --frozen ruff check src
    uv run --directory engines/sign --frozen pytest tests ../common/tests

# Build the unsigned sign engine pack for this host into target/pack-sign.
pack-sign out="target/pack-sign":
    python3 engines/sign/pack/build.py build --out "{{out}}"

# Build the unsigned pdfium engine pack for this host into target/packs/pdfium, an unpacked
# development pack: `export ASHIFT_PACK_PDFIUM=$PWD/target/packs/pdfium`. The components go to
# target/packs/pdfium-dist. Downloads the pinned PDFium archive unless PDFIUM_ARCHIVE names a copy.
pack-pdfium out="target/packs/pdfium":
    cargo build -p ariad-pdfium-engine -p ariad-cli --bin ariad-pdfium-engine --bin ashift
    bash engines/pdfium/pack/make.sh dev --engine "{{justfile_directory()}}/target/debug/ariad-pdfium-engine" --ashift "{{justfile_directory()}}/target/debug/ashift" --out "{{out}}-dist" --unpack-dir "{{out}}"
    bash engines/pdfium/pack/make.sh verify --pack "{{out}}"

# Check the layout, manifest and notices of the pack that `just pack-pdfium` built.
pack-pdfium-verify out="target/packs/pdfium":
    bash engines/pdfium/pack/make.sh verify --pack "{{out}}"

# Assemble and check an unsigned qpdf development pack for this host.
pack-qpdf qpdf_dir out="target/packs/qpdf":
    cargo build -p ariad-cli --bin ashift
    bash engines/qpdf/pack/make.sh dev "{{qpdf_dir}}" "${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/debug/ashift" "{{out}}"
    bash engines/qpdf/pack/make.sh verify "{{out}}"

pack-qpdf-verify out="target/packs/qpdf":
    bash engines/qpdf/pack/make.sh verify "{{out}}"

# Build a host Tesseract development pack from verified source and model inputs.
pack-tesseract out="target/packs/tesseract":
    #!/usr/bin/env bash
    set -euo pipefail
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    inputs=${TESSERACT_INPUT_DIR:-$work/input}
    if [ -z "${TESSERACT_INPUT_DIR:-}" ]; then
        python3 engines/tesseract/pack/fetch-inputs.py "$inputs"
    fi
    bash engines/tesseract/pack/compile-tesseract.sh "$inputs/tesseract.tar.gz" "$inputs/leptonica.tar.gz" "$work/runtime"
    cargo build -p ariad-cli --bin ashift
    PACK_REQUIRES_ASHIFT='>=0.2.0, <2.0.0' bash engines/tesseract/pack/make.sh pack "$(bash engines/tesseract/pack/make.sh slug)" "$work/runtime" "$inputs/tessdata" "${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/debug/ashift" "{{out}}"
    bash engines/tesseract/pack/make.sh verify "{{out}}"

pack-tesseract-verify out="target/packs/tesseract":
    #!/usr/bin/env bash
    set -euo pipefail
    bash engines/tesseract/pack/make.sh verify "{{out}}"
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    export XDG_DATA_HOME="$work"
    export ASHIFT_PACK_TESSERACT="$(cd "{{out}}" && pwd -P)"
    export ARIAD_TEST_TESSERACT_PACK="$ASHIFT_PACK_TESSERACT"
    cargo test -p ariad-host --features test-probe --test tesseract_adapter

# Build a host veraPDF development pack from verified installer inputs.
pack-verapdf out="target/packs/verapdf":
    #!/usr/bin/env bash
    set -euo pipefail
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    slug=$(bash engines/verapdf/pack/make.sh slug)
    installer_zip=${VERAPDF_INSTALLER_ZIP:-}
    if [ -z "$installer_zip" ]; then
        installer_zip=$work/verapdf-installer.zip
        curl -fsSL --retry 3 --max-time 180 https://software.verapdf.org/rel/1.30/verapdf-greenfield-1.30.3-installer.zip -o "$installer_zip"
    fi
    cargo build -p ariad-cli --bin ashift
    bash engines/verapdf/pack/make.sh pack "$slug" "$installer_zip" "${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/debug/ashift" "{{out}}"
    bash engines/verapdf/pack/make.sh verify "{{out}}"

pack-verapdf-verify out="target/packs/verapdf":
    #!/usr/bin/env bash
    set -euo pipefail
    bash engines/verapdf/pack/make.sh verify "{{out}}"
    VERAPDF_TEST_PACK="{{out}}" bash scripts/test-pack-verapdf.sh

# Test the shared pack builder, the pdfium wrapper and the signing guard (offline).
pack-tooling-test:
    bash scripts/pack/test-pack-tooling.sh

# Run fuzz targets on Linux (default: all targets, 60 s each)
[linux]
fuzz target="" seconds="60":
    #!/usr/bin/env bash
    set -euo pipefail
    cargo test --locked --manifest-path fuzz/Cargo.toml --bin docx_meta
    if [ -n "{{target}}" ]; then
        targets=("{{target}}")
    else
        # Capture first so a discovery failure cannot silently run zero targets.
        discovered=$(cargo fuzz list)
        [ -n "$discovered" ] || { echo "no fuzz targets discovered" >&2; exit 1; }
        mapfile -t targets <<<"$discovered"
    fi
    for t in "${targets[@]}"; do
        echo "=== Running fuzz target: $t ({{seconds}}s) ==="
        mkdir -p "fuzz/corpus/$t"
        build_options=()
        case "$t" in
            pdf_reader|pdf_open_bounded|pdf_tools_structural|pdf_draw_params|pdf_form_interchange|pdf_form_structure|pdf_redact_params|type1_to_cff|reader_cbz) build_options=(--dev --features pdf-reader) ;;
            typst_writer) build_options=(--dev --features typst-writer) ;;
        esac
        cargo fuzz run -s none "${build_options[@]}" "$t" "fuzz/corpus/$t" "fuzz/seeds/$t" -- -max_total_time="{{seconds}}" -timeout=10 -rss_limit_mb=2048
    done

[macos]
[windows]
fuzz target="" seconds="60":
    @echo "fuzzing runs on Linux only" && exit 1

# Build the ariad-ffi universal XCFramework and Swift bindings.
[macos]
macos-ffi *args:
    bash scripts/build-xcframework.sh "$@"

# Generate apps/macos/AriadShift.xcodeproj from project.yml (needs `brew install xcodegen`) and install the tracked
# apps/macos/Package.resolved, the one place Xcode reads SwiftPM pins from for the generated, gitignored project.
[macos]
macos-project:
    cd apps/macos && xcodegen generate
    mkdir -p apps/macos/AriadShift.xcodeproj/project.xcworkspace/xcshareddata/swiftpm
    cp apps/macos/Package.resolved apps/macos/AriadShift.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved

# Fetch the pinned Pandoc (version and SHA-256 from scripts/install-pandoc.sh) for both app architectures into .tools/.
# The Xcode phase apps/macos/scripts/embed-pandoc.sh joins them into Contents/Helpers/pandoc and uses no network itself.
# Skipped when ASHIFT_BUNDLED_PANDOC_PATH names a binary to embed instead.
[macos]
macos-pandoc:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "${ASHIFT_BUNDLED_PANDOC_PATH:-}" ]]; then
        echo "ASHIFT_BUNDLED_PANDOC_PATH is set; not fetching Pandoc."
        exit 0
    fi
    bash scripts/install-pandoc.sh --macos-arch arm64
    bash scripts/install-pandoc.sh --macos-arch x86_64

# Build the universal Release macOS application bundle. Extra xcodebuild settings may be passed as arguments
# (each stays one argument: the global `set positional-arguments` forwards them with "$@" instead of textual substitution).
[macos]
macos-build *args: macos-pandoc
    #!/usr/bin/env bash
    set -euo pipefail
    # The ashift helper is built in the same cargo invocation as the XCFramework slices and handed to the Xcode helper
    # phase through ASHIFT_HELPER_PATH, so the dependency graph compiles once per target. A caller-provided
    # ASHIFT_HELPER_PATH is kept and no helper is built.
    target_dir="${CARGO_TARGET_DIR:-target}"
    [[ "$target_dir" = /* ]] || target_dir="$PWD/$target_dir"
    if [[ -z "${ASHIFT_HELPER_PATH:-}" ]]; then
        ASHIFT_HELPER_OUT="$target_dir/universal/release/ashift" bash scripts/build-xcframework.sh
        export ASHIFT_HELPER_PATH="$target_dir/universal/release/ashift"
    else
        bash scripts/build-xcframework.sh
    fi
    bash apps/macos/scripts/sync-ffi.sh
    just macos-project
    derived_dir="${MACOS_DERIVED_DATA:-${DERIVED_DATA_DIR:-target/derived-data}}"
    mkdir -p "$derived_dir"
    # SWIFTPM_CACHE_PATH, when set, keeps SwiftPM's repository cache out of ~/Library/Caches.
    xcode_cache_args=()
    if [[ -n "${SWIFTPM_CACHE_PATH:-}" ]]; then
        xcode_cache_args=(-packageCachePath "${SWIFTPM_CACHE_PATH}")
    fi
    xcodebuild -project apps/macos/AriadShift.xcodeproj \
        -scheme AriadShift \
        -configuration Release \
        -destination 'generic/platform=macOS' \
        -derivedDataPath "$derived_dir" \
        ${xcode_cache_args[@]+"${xcode_cache_args[@]}"} \
        -onlyUsePackageVersionsFromResolvedFile \
        build "$@"

# Build the Release app and install it as "AriadShift Dev" (bundle id com.ariadnev.ariadshift.dev) beside the stable
# app, in ~/Applications or $ASHIFT_DEV_APP_DIR. The copy has Sparkle switched off, carries the checkout version, and is
# re-signed ad hoc as a whole: the embedded Sparkle.framework keeps Sparkle's Team ID, which dyld refuses to map into an
# ad-hoc signed process. The build output is deleted afterwards unless ASHIFT_KEEP_BUILD=1.
[macos]
macos-install-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    dev_id="com.ariadnev.ariadshift.dev"
    dev_name="AriadShift Dev"
    install_dir="${ASHIFT_DEV_APP_DIR:-$HOME/Applications}"
    sha="$(git rev-parse --short HEAD)"
    # Highest version tag, not `git describe`: release tags live on main and are not ancestors of dev.
    base="$(git tag --list 'v*' --sort=-v:refname | sed -n 1p)"
    base="${base#v}"
    version="${base:-0.0.0}-dev+${sha}"
    build="$(git rev-list --count HEAD)"
    just macos-build \
        "PRODUCT_BUNDLE_IDENTIFIER=$dev_id" \
        "MARKETING_VERSION=$version" \
        "CURRENT_PROJECT_VERSION=$build"
    derived_dir="${MACOS_DERIVED_DATA:-${DERIVED_DATA_DIR:-target/derived-data}}"
    built_app="$derived_dir/Build/Products/Release/AriadShift.app"
    [[ -d "$built_app" ]] || { echo "build product missing: $built_app" >&2; exit 1; }
    stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' EXIT
    # Staged copy: a launched bundle cannot be modified in place (App Management), so the install is replaced whole.
    ditto "$built_app" "$stage/$dev_name.app"
    app="$stage/$dev_name.app"
    plist="$app/Contents/Info.plist"
    pb=/usr/libexec/PlistBuddy
    "$pb" -c "Set :CFBundleDisplayName $dev_name" "$plist"
    "$pb" -c "Set :CFBundleName $dev_name" "$plist" 2>/dev/null || "$pb" -c "Add :CFBundleName string $dev_name" "$plist"
    "$pb" -c "Delete :SUFeedURL" "$plist"
    "$pb" -c "Set :SUEnableAutomaticChecks false" "$plist"
    "$pb" -c "Set :SUAutomaticallyUpdate false" "$plist"
    xattr -cr "$app"
    codesign --force --deep --sign - "$app"
    codesign --verify --deep --strict "$app"
    mkdir -p "$install_dir"
    rm -rf "${install_dir:?}/$dev_name.app"
    ditto "$app" "$install_dir/$dev_name.app"
    if [[ "${ASHIFT_KEEP_BUILD:-0}" != "1" ]]; then
        # Only directories inside this checkout and named target or derived-data are removed; anything else is left alone.
        target_dir="${CARGO_TARGET_DIR:-target}"
        for dir in "$derived_dir" "$target_dir"; do
            abs="$(cd "$dir" 2>/dev/null && pwd -P || true)"
            if [[ -n "$abs" && "$abs" == "$(pwd -P)"/* && ( "${abs##*/}" == target || "${abs##*/}" == derived-data ) ]]; then
                rm -rf "$abs"
            elif [[ -n "$abs" ]]; then
                echo "keeping build output outside the checkout or not named target or derived-data: $abs"
            fi
        done
    fi
    echo "Installed:  $install_dir/$dev_name.app"
    echo "Bundle id:  $("$pb" -c 'Print :CFBundleIdentifier' "$install_dir/$dev_name.app/Contents/Info.plist")"
    echo "Version:    $("$pb" -c 'Print :CFBundleShortVersionString' "$install_dir/$dev_name.app/Contents/Info.plist") ($build)"

# Run tests for AriadShiftCore and the AriadShift Xcode scheme.
[macos]
macos-test: macos-pandoc
    #!/usr/bin/env bash
    set -euo pipefail
    REPO_ROOT="$(pwd)"
    DERIVED_DATA="${MACOS_DERIVED_DATA:-${DERIVED_DATA_DIR:-${REPO_ROOT}/target/derived-data}}"
    mkdir -p "${DERIVED_DATA}"
    if [[ -z "${ASHIFT_HELPER_PATH:-}" ]]; then
        if [[ -x "${REPO_ROOT}/target/release/ashift" ]]; then
            export ASHIFT_HELPER_PATH="${REPO_ROOT}/target/release/ashift"
        elif [[ -x "${REPO_ROOT}/target/aarch64-apple-darwin/release/ashift" ]]; then
            export ASHIFT_HELPER_PATH="${REPO_ROOT}/target/aarch64-apple-darwin/release/ashift"
        elif [[ -x "${DERIVED_DATA}/Build/Products/Release/AriadShift.app/Contents/Helpers/ashift" ]]; then
            export ASHIFT_HELPER_PATH="${DERIVED_DATA}/Build/Products/Release/AriadShift.app/Contents/Helpers/ashift"
        elif [[ -x "${REPO_ROOT}/target/debug/ashift" ]]; then
            export ASHIFT_HELPER_PATH="${REPO_ROOT}/target/debug/ashift"
        fi
    fi
    # SWIFTPM_CACHE_PATH, when set, keeps SwiftPM's repository cache out of ~/Library/Caches.
    cache_args=()
    xcode_cache_args=()
    if [[ -n "${SWIFTPM_CACHE_PATH:-}" ]]; then
        cache_args=(--cache-path "${SWIFTPM_CACHE_PATH}")
        xcode_cache_args=(-packageCachePath "${SWIFTPM_CACHE_PATH}")
    fi
    swift test --package-path apps/macos/AriadShiftCore
    # Token, model and snapshot tests of the design system (reference images in Tests/**/__Snapshots__).
    swift test --package-path apps/macos/AriadDesignSystem ${cache_args[@]+"${cache_args[@]}"}
    # xcodebuild hands the test process only TEST_RUNNER_-prefixed variables (without the prefix), so the snapshot
    # gate and the failed-render folder are passed through that way, each only when set: an empty SNAPSHOT_ARTIFACTS
    # would make failed renders land in the working directory.
    runner_env=()
    for name in CI ARIAD_SNAPSHOTS SNAPSHOT_ARTIFACTS; do
        if [[ -n "${!name:-}" ]]; then runner_env+=("TEST_RUNNER_${name}=${!name}"); fi
    done
    # The result bundle names the failing tests even when xcodebuild's own failure list is empty; xcodebuild refuses
    # to overwrite an existing bundle, so a stale one is removed first.
    RESULT_BUNDLE="${MACOS_RESULT_BUNDLE:-${DERIVED_DATA}/AriadShift-tests.xcresult}"
    rm -rf "${RESULT_BUNDLE}"
    env ${runner_env[@]+"${runner_env[@]}"} xcodebuild test \
        -project apps/macos/AriadShift.xcodeproj \
        -scheme AriadShift \
        -destination 'platform=macOS' \
        -derivedDataPath "${DERIVED_DATA}" \
        -resultBundlePath "${RESULT_BUNDLE}" \
        ${xcode_cache_args[@]+"${xcode_cache_args[@]}"} \
        -onlyUsePackageVersionsFromResolvedFile

# Fail on literal spacing, frame, color and font values in the macOS app outside AriadDesignSystem (new ones only;
# design-literals-baseline.txt lists the old ones to drain), and on asset catalogs that differ from their sources.
[macos]
macos-lint:
    python3 apps/macos/scripts/test-check-design-literals.py
    python3 apps/macos/scripts/check-design-literals.py
    node apps/macos/scripts/generate-design-assets.mjs --check

# Regenerate the macOS color sets, AccentColor and AppIcon from tokens.json and brand/png.
[macos]
macos-assets:
    node apps/macos/scripts/generate-design-assets.mjs

# Generated IPC types and wrappers come only from Rust.
desktop-licenses:
    #!/usr/bin/env bash
    set -euo pipefail
    root="${ARIADSHIFT_ROOT_DIR:-{{justfile_directory()}}}"
    out="${ARIADSHIFT_DESKTOP_LICENSES_OUT:-$root/apps/desktop/THIRD_PARTY_JS_LICENSES}"
    node scripts/generate-desktop-licenses.mjs --check-preconditions
    tmp=$(mktemp "$(dirname "$out")/.THIRD_PARTY_JS_LICENSES.XXXXXX")
    trap 'rm -f "$tmp"' EXIT
    node scripts/generate-desktop-licenses.mjs > "$tmp"
    test -s "$tmp"
    chmod 0644 "$tmp"
    mv -f "$tmp" "$out"

desktop-bindings:
    cargo run -p ariad-desktop --bin export-bindings
    pnpm exec biome format --write apps/desktop/src/bindings

desktop-bindings-check: desktop-bindings
    git diff --exit-code -- apps/desktop/src/bindings apps/desktop/src-tauri/permissions/desktop-commands.toml

desktop-dev:
    pnpm --filter @ariadshift/desktop tauri dev

desktop-build:
    pnpm --filter @ariadshift/desktop tauri build

# Run after a desktop build on Linux, where WebKitGTK supports WebDriver.
[linux]
desktop-smoke:
    APPIMAGE_EXTRACT_AND_RUN=1 python3 scripts/desktop-smoke.py target/release/bundle/appimage/*.AppImage

# Run built-in engines from the extracted Linux bundle without FUSE.
[linux]
desktop-engine-smoke:
    APPIMAGE_EXTRACT_AND_RUN=1 bash scripts/desktop-engine-smoke.sh

# Shared dependency changes also trigger desktop checks locally.
desktop-ci:
    #!/usr/bin/env bash
    set -euo pipefail
    if git diff --name-only origin/dev HEAD -- apps/desktop packages Cargo.toml Cargo.lock pnpm-lock.yaml justfile .github/workflows/desktop.yml | grep -q .; then
        just desktop-bindings-check
        pnpm --filter @ariadshift/desktop build
        pnpm --filter @ariadshift/desktop test
        pnpm --filter @ariadshift/desktop test:tools
        cargo nextest run -p ariad-desktop
    fi

# Regenerate the committed desktop icons after changing brand sources.
desktop-icons:
    pnpm --filter @ariadshift/desktop tauri icon ../../brand/png/ariadshift-app-icon-1024.png --output src-tauri/icons

# Measure authored PDF, scan and Office edges and complete routes with a configured Docling runtime.
bench-heavy *args:
    uv run --package ariad-bench python -m ariad_bench heavy "$@"

# Regenerate the public benchmark report from committed measurements.
bench-report *args:
    uv run --package ariad-bench python -m ariad_bench report "$@"
