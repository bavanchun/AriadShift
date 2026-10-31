#!/bin/sh
# Table tests for scripts/ci-scope.sh path classification.
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
ci_scope="$script_dir/ci-scope.sh"

failed_count=0
passed_count=0

assert_classification() {
  desc="$1"
  paths="$2"
  expected_heavy="$3"
  expected_web="$4"
  expected_reason="$5"

  output=$(printf '%s\n' "$paths" | sh "$ci_scope" --classify)
  actual_heavy=$(printf '%s' "$output" | cut -d'|' -f1)
  actual_web=$(printf '%s' "$output" | cut -d'|' -f2)
  actual_reason=$(printf '%s' "$output" | cut -d'|' -f3)

  if [ "$actual_heavy" != "$expected_heavy" ] || \
     [ "$actual_web" != "$expected_web" ] || \
     [ "$actual_reason" != "$expected_reason" ]; then
    printf 'FAIL | %-32s | expected %s|%s|%s, got %s\n' \
      "$desc" "$expected_heavy" "$expected_web" "$expected_reason" "$output" >&2
    failed_count=$((failed_count + 1))
  else
    printf 'PASS | %-32s | %s\n' "$desc" "$output"
    passed_count=$((passed_count + 1))
  fi
}

echo "=== Table tests: scripts/ci-scope.sh classification ==="

# 1. core and wasm
assert_classification "core (ariad-core)" "crates/ariad-core/src/lib.rs" "true" "true" "code change"
assert_classification "pdf (ariad-pdf)" "crates/ariad-pdf/src/lib.rs" "true" "true" "code change"
assert_classification "engine-io (ariad-engine-io)" "crates/ariad-engine-io/src/lib.rs" "true" "true" "code change"
assert_classification "wasm (ariad-wasm)" "crates/ariad-wasm/src/lib.rs" "true" "true" "code change"

assert_classification "typst (ariad-typst)" "crates/ariad-typst/src/lib.rs" "true" "true" "code change"

# 2. host
assert_classification "host (ariad-host)" "crates/ariad-host/src/main.rs" "true" "false" "code change"

# 3. apps/
assert_classification "apps/ (web app)" "apps/web/src/converter/worker.ts" "false" "true" "JavaScript-only change"
assert_classification "desktop (native command)" "apps/desktop/src-tauri/src/lib.rs" "true" "true" "code change"
assert_classification "desktop (React UI)" "apps/desktop/src/app.tsx" "false" "true" "JavaScript-only change"

# 4. packages/
assert_classification "packages/ (ui package)" "packages/ui/src/button.tsx" "false" "true" "JavaScript-only change"
assert_classification "packages/ (generated SDK)" "packages/sdk/src/client.ts" "false" "true" "JavaScript-only change"

# 4b. services/ and the OpenAPI document the SDK is generated from
assert_classification "services/ (auth service)" "services/auth/src/auth.ts" "false" "true" "JavaScript-only change"
assert_classification "services/ (auth image)" "services/auth/Dockerfile" "false" "true" "JavaScript-only change"
assert_classification "schemas (OpenAPI document)" "schemas/openapi.v1.json" "true" "true" "code change"

# 5. brand/
assert_classification "brand/ (brand assets)" "brand/src/logo.mjs" "false" "true" "JavaScript-only change"

# 6. root manifests and tool configs (JS and Rust)
assert_classification "root manifest (package.json)" "package.json" "false" "true" "JavaScript-only change"
assert_classification "root manifest (pnpm-lock)" "pnpm-lock.yaml" "false" "true" "JavaScript-only change"
assert_classification "root manifest (biome.json)" "biome.json" "false" "true" "JavaScript-only change"
assert_classification "root config (.node-version)" ".node-version" "false" "true" "JavaScript-only change"
assert_classification "root config (vitest.config.ts)" "vitest.config.ts" "false" "true" "JavaScript-only change"
assert_classification "root manifest (Cargo.toml)" "Cargo.toml" "true" "true" "code change"
assert_classification "root manifest (Cargo.lock)" "Cargo.lock" "true" "true" "code change"
assert_classification "root config (rust-toolchain.toml)" "rust-toolchain.toml" "true" "true" "code change"
assert_classification "root manifest (justfile)" "justfile" "true" "true" "code change"

# Fuzz crate tests are outside the root workspace and need the fuzz job.
assert_classification "fuzz oracle tests" "fuzz/fuzz_targets/docx_meta_tests.rs" "true" "false" "code change"
assert_classification "fuzz crate manifest" "fuzz/Cargo.toml" "true" "false" "code change"

# 7. scripts and workflows
assert_classification "script (check-js-licenses.mjs)" "scripts/check-js-licenses.mjs" "false" "false" "JavaScript-only change"
assert_classification "script (install-wasm-bindgen.sh)" "scripts/install-wasm-bindgen.sh" "true" "true" "code change"
assert_classification "workflow (.github ci.yml)" ".github/workflows/ci.yml" "true" "true" "code change"
assert_classification "workflow (cloud-e2e.yml)" ".github/workflows/cloud-e2e.yml" "false" "false" "JavaScript-only change"
assert_classification "e2e test (infra/test/e2e)" "infra/test/e2e/run.sh" "false" "false" "JavaScript-only change"
assert_classification "compose (infra/compose)" "infra/compose/docker-compose.yml" "true" "false" "code change"

# 8. docs-only
assert_classification "docs-only (docs/)" "docs/cli.md" "false" "false" "documentation-only change"
assert_classification "docs-only (README.md)" "README.md" "false" "false" "documentation-only change"
assert_classification "docs-only (plans/)" "plans/design.md" "false" "false" "documentation-only change"

# 9. mixed
assert_classification "mixed (docs + host)" "$(printf 'docs/cli.md\ncrates/ariad-host/src/main.rs')" "true" "false" "code change"
assert_classification "mixed (docs + apps)" "$(printf 'docs/web.md\napps/web/src/converter/worker.ts')" "false" "true" "JavaScript-only change"
assert_classification "mixed (apps + host)" "$(printf 'apps/web/src/index.ts\ncrates/ariad-host/src/main.rs')" "true" "true" "code change"
assert_classification "mixed (e2e + host)" "$(printf 'infra/test/e2e/run.sh\ncrates/ariad-host/src/main.rs')" "true" "false" "code change"
assert_classification "mixed (core + docs)" "$(printf 'crates/ariad-core/src/lib.rs\nREADME.md')" "true" "true" "code change"

# 10. fallback
assert_classification "full-run fallback (empty list)" "" "true" "true" "full run"

echo "=== Cloud E2E scope ==="
assert_cloud() {
  paths=$1 expected=$2
  actual=$(printf '%s\n' "$paths" | sh "$ci_scope" --classify-cloud)
  if [ "$actual" = "$expected" ]; then
    passed_count=$((passed_count + 1))
    printf 'PASS | cloud %s | %s\n' "$paths" "$actual"
  else
    failed_count=$((failed_count + 1))
    printf 'FAIL | cloud %s | expected %s, got %s\n' "$paths" "$expected" "$actual" >&2
  fi
}
for path in infra/test/e2e/run.sh infra/docker/web.Dockerfile \
  apps/web/src/app/page.tsx crates/ariad-core/src/lib.rs \
  crates/ariad-pdf/src/lib.rs crates/ariad-engine-io/src/lib.rs \
  crates/ariad-host/src/lib.rs crates/ariad-cli/src/main.rs \
  crates/ariad-server/src/worker.rs crates/ariad-wasm/src/lib.rs \
  crates/ariad-typst/src/lib.rs services/auth/src/auth.ts \
  packages/sdk/src/client.ts packages/ui/src/index.ts brand/src/logo.mjs \
  Cargo.toml Cargo.lock rust-toolchain.toml pnpm-lock.yaml pnpm-workspace.yaml \
  package.json .node-version .env.example justfile scripts/install-wasm-bindgen.sh \
  scripts/ci-scope.sh scripts/test-ci-scope.sh fixtures/manifest.toml \
  fixtures/pdf/vi-garden-image-only.pdf .github/workflows/cloud-e2e.yml \
  .github/workflows/pack-docling.yml; do
  assert_cloud "$path" true
done
for path in docs/ci.md plans/design.md README.md fixtures/scan/en-alice-p3.pdf \
  apps/desktop/src/app.tsx Cargo.lock.bak fixtures/manifest.toml.bak; do
  assert_cloud "$path" false
done
assert_cloud "$(printf 'docs/ci.md\napps/web/src/app/page.tsx')" true
assert_cloud "$(printf 'docs/ci.md\nREADME.md')" false
assert_cloud "" true
[ "$(sh "$ci_scope" --classify-cloud Cargo.lock)" = true ] || exit 1

echo "=== Summary ==="
echo "Passed: $passed_count, Failed: $failed_count"
if [ "$failed_count" -gt 0 ]; then
  exit 1
fi
