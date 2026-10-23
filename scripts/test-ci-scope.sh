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

# 1. core
assert_classification "core (ariad-core)" "crates/ariad-core/src/lib.rs" "true" "true" "code change"

# 2. host
assert_classification "host (ariad-host)" "crates/ariad-host/src/main.rs" "true" "false" "code change"

# 3. apps/
assert_classification "apps/ (web app)" "apps/web/src/converter/worker.ts" "false" "true" "JavaScript-only change"

# 4. packages/
assert_classification "packages/ (ui package)" "packages/ui/src/button.tsx" "false" "true" "JavaScript-only change"

# 5. brand/
assert_classification "brand/ (brand assets)" "brand/src/logo.mjs" "false" "true" "JavaScript-only change"

# 6. root manifests (JS and Rust)
assert_classification "root manifest (package.json)" "package.json" "false" "true" "JavaScript-only change"
assert_classification "root manifest (pnpm-lock)" "pnpm-lock.yaml" "false" "true" "JavaScript-only change"
assert_classification "root manifest (biome.json)" "biome.json" "false" "true" "JavaScript-only change"
assert_classification "root manifest (Cargo.toml)" "Cargo.toml" "true" "true" "code change"
assert_classification "root manifest (Cargo.lock)" "Cargo.lock" "true" "true" "code change"
assert_classification "root manifest (justfile)" "justfile" "true" "true" "code change"

# 7. ci.yml
assert_classification "workflow (.github ci.yml)" ".github/workflows/ci.yml" "true" "true" "code change"

# 8. docs-only
assert_classification "docs-only (docs/)" "docs/cli.md" "false" "false" "documentation-only change"
assert_classification "docs-only (README.md)" "README.md" "false" "false" "documentation-only change"
assert_classification "docs-only (plans/)" "plans/design.md" "false" "false" "documentation-only change"

# 9. mixed
assert_classification "mixed (docs + host)" "$(printf 'docs/cli.md\ncrates/ariad-host/src/main.rs')" "true" "false" "code change"
assert_classification "mixed (docs + apps)" "$(printf 'docs/web.md\napps/web/src/converter/worker.ts')" "false" "true" "JavaScript-only change"
assert_classification "mixed (apps + host)" "$(printf 'apps/web/src/index.ts\ncrates/ariad-host/src/main.rs')" "true" "true" "code change"
assert_classification "mixed (core + docs)" "$(printf 'crates/ariad-core/src/lib.rs\nREADME.md')" "true" "true" "code change"

echo "=== Summary ==="
echo "Passed: $passed_count, Failed: $failed_count"
if [ "$failed_count" -gt 0 ]; then
  exit 1
fi
