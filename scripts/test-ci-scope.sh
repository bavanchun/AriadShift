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

# 7b. pdfium engine pack scripts and workflow
assert_classification "pack (pdfium wrapper)" "engines/pdfium/pack/make.sh" "false" "false" "pack-only change"
assert_classification "pack (pdfium pins)" "engines/pdfium/pack/targets.txt" "false" "false" "pack-only change"
assert_classification "pack (qpdf wrapper)" "engines/qpdf/pack/make.sh" "false" "false" "pack-only change"
assert_classification "pack (qpdf test)" "scripts/test-pack-qpdf.sh" "false" "false" "pack-only change"
assert_classification "pack (qpdf workflow)" ".github/workflows/pack-qpdf.yml" "false" "false" "pack-only change"
assert_classification "pack (tesseract wrapper)" "engines/tesseract/pack/make.sh" "false" "false" "pack-only change"
assert_classification "pack (tesseract test)" "scripts/test-pack-tesseract.sh" "false" "false" "pack-only change"
assert_classification "pack (tesseract workflow)" ".github/workflows/pack-tesseract.yml" "false" "false" "pack-only change"
assert_classification "pack (shared builder)" "scripts/pack/build-pack.sh" "false" "false" "pack-only change"
assert_classification "pack (workflow)" ".github/workflows/pack-pdfium.yml" "false" "false" "pack-only change"
assert_classification "mixed (pack + docs)" "$(printf 'docs/packs.md\nscripts/pack/sign-pack.sh')" "false" "false" "pack-only change"
assert_classification "mixed (pack + host)" "$(printf 'scripts/pack/sign-pack.sh\ncrates/ariad-host/src/main.rs')" "true" "false" "code change"
assert_classification "mixed (pack + web)" "$(printf 'engines/pdfium/pack/make.sh\napps/web/src/index.ts')" "false" "true" "JavaScript-only change"
assert_classification "pdfium engine crate" "crates/ariad-pdfium-engine/src/main.rs" "true" "false" "code change"

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

echo "=== Packaging scope ==="
# assert_package <description> <expected> <path>...  ("--added <path>" marks an added file)
assert_package() {
  desc=$1 expected=$2
  shift 2
  actual=$(sh "$ci_scope" --classify-package "$@" </dev/null)
  if [ "$actual" = "$expected" ]; then
    passed_count=$((passed_count + 1))
    printf 'PASS | package %-38s | %s\n' "$desc" "$actual"
  else
    failed_count=$((failed_count + 1))
    printf 'FAIL | package %-38s | expected %s, got %s\n' "$desc" "$expected" "$actual" >&2
  fi
}
# Inputs of `cargo package`.
assert_package "root Cargo.toml" true Cargo.toml
assert_package "Cargo.lock" true Cargo.lock
assert_package "rust-toolchain.toml" true rust-toolchain.toml
assert_package "justfile" true justfile
assert_package "root README.md (crate readme)" true README.md
assert_package "workflow ci.yml" true .github/workflows/ci.yml
assert_package "scope script" true scripts/ci-scope.sh
assert_package "crate manifest" true crates/ariad-core/Cargo.toml
assert_package "crate build script" true crates/ariad-host/build.rs
assert_package "crate data file" true crates/ariad-core/data/capabilities.json
assert_package "crate test fixture" true crates/ariad-cli/tests/fixtures/sample.md
assert_package "crate license file" true crates/ariad-core/LICENSE
assert_package "added crate source file" true --added crates/ariad-pdf/src/new_module.rs
assert_package "added crate test file" true --added crates/ariad-core/tests/new_case.rs
assert_package "added file among edits" true --added crates/ariad-core/src/new.rs crates/ariad-core/src/lib.rs
# A plain edit of existing Rust source, and files the package never reads.
assert_package "cargo config" true .cargo/config.toml
assert_package "Rust edit reaching outside" true --rs-risk crates/ariad-core/src/lib.rs
assert_package "edited crate source" false crates/ariad-core/src/lib.rs
assert_package "edited nested crate source" false crates/ariad-host/src/engine/mod.rs
assert_package "edited crate test source" false crates/ariad-cli/tests/cli.rs
assert_package "two edited sources" false crates/ariad-core/src/lib.rs crates/ariad-pdf/src/lib.rs
assert_package "docs" false docs/ci.md
assert_package "web app" false apps/web/src/converter/worker.ts
assert_package "other workflow" false .github/workflows/cloud-e2e.yml
assert_package "other script" false scripts/test-ci-scope.sh
assert_package "added file outside crates" false --added docs/new.md
assert_package "added web file" false --added apps/web/src/new.ts
assert_package "Cargo.lock lookalike" false Cargo.lock.bak
assert_package "nested build.rs lookalike" false crates/ariad-core/src/build.rs
assert_package "crates lookalike dir" false crates-old/ariad-core/Cargo.toml
# An unknown change list runs everything.
assert_package "empty list" true
[ "$(printf 'crates/ariad-core/src/lib.rs\n' | sh "$ci_scope" --classify-package)" = false ] || exit 1

echo "=== Scope outputs (event and git diff) ==="
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
repo="$tmp/repo"
git init -q "$repo"
git_in() {
  git -C "$repo" -c user.name=scope-test -c user.email=scope-test@example.invalid \
    -c commit.gpgsign=false -c core.hooksPath=/dev/null "$@"
}
mkdir -p "$repo/crates/ariad-core/src" "$repo/crates/ariad-core/assets" "$repo/docs"
printf '[package]\nname = "ariad-core"\n' > "$repo/crates/ariad-core/Cargo.toml"
printf 'pub fn one() {}\n' > "$repo/crates/ariad-core/src/lib.rs"
printf '{}\n' > "$repo/crates/ariad-core/assets/data.json"
printf '# readme\n' > "$repo/README.md"
printf 'notes\n' > "$repo/docs/notes.md"
git_in add crates README.md docs
git_in commit -q -m base
git_in tag base

parts_all='[{"part":"lint"},{"part":"test-1","shard":1,"shards":2},{"part":"test-2","shard":2,"shards":2}'
parts_no_package="$parts_all]"
parts_package="$parts_all,{\"part\":\"package\"}]"

# assert_outputs <description> <package> <linux_parts> <env assignment>...
assert_outputs() {
  desc=$1 expected_package=$2 expected_parts=$3
  shift 3
  : > "$tmp/output"
  # The variables a real run reads are cleared first, and the step summary is never written.
  (cd "$repo" && env -u GITHUB_STEP_SUMMARY -u EVENT_NAME -u PUSH_BEFORE -u REF_NAME \
    -u PR_BASE_SHA -u PR_HEAD_SHA -u PR_BASE_REF "$@" GITHUB_OUTPUT="$tmp/output" sh "$ci_scope" >/dev/null)
  actual_package=$(sed -n 's/^package=//p' "$tmp/output")
  actual_parts=$(sed -n 's/^linux_parts=//p' "$tmp/output")
  if [ "$actual_package" = "$expected_package" ] && [ "$actual_parts" = "$expected_parts" ]; then
    passed_count=$((passed_count + 1))
    printf 'PASS | outputs %-34s | package=%s\n' "$desc" "$actual_package"
  else
    failed_count=$((failed_count + 1))
    printf 'FAIL | outputs %-34s | expected %s %s, got %s %s\n' \
      "$desc" "$expected_package" "$expected_parts" "$actual_package" "$actual_parts" >&2
  fi
}

# assert_pr <description> <package> <setup command>: commit the setup on top of base, then run
# the scope for a pull request into dev. The setup stages the files it creates.
assert_pr() {
  desc=$1 expected_package=$2 setup=$3
  git_in checkout -q -B change base
  (cd "$repo" && eval "$setup")
  git_in commit -q -a -m change
  expected_parts=$parts_no_package
  if [ "$expected_package" = true ]; then expected_parts=$parts_package; fi
  assert_outputs "$desc" "$expected_package" "$expected_parts" \
    EVENT_NAME=pull_request PR_BASE_SHA="$(git_in rev-parse base)" \
    PR_HEAD_SHA="$(git_in rev-parse HEAD)" PR_BASE_REF=dev
}

assert_outputs "manual run" true "$parts_package" EVENT_NAME=workflow_dispatch
assert_outputs "push without a previous commit" true "$parts_package" EVENT_NAME=push REF_NAME=dev
git_in checkout -q -B change base
printf 'pub fn two() {}\n' >> "$repo/crates/ariad-core/src/lib.rs"
git_in commit -q -a -m edit
assert_outputs "push of a Rust edit to dev" true "$parts_package" \
  EVENT_NAME=push PUSH_BEFORE="$(git_in rev-parse base)" REF_NAME=dev
assert_pr "edit of existing Rust source" false "printf 'pub fn two() {}\n' >> crates/ariad-core/src/lib.rs"
assert_pr "edit of the docs only" false "printf 'more\n' >> docs/notes.md"
assert_pr "Rust edit plus docs" false "printf 'pub fn two() {}\n' >> crates/ariad-core/src/lib.rs; printf 'more\n' >> docs/notes.md"
assert_pr "new Rust file in a crate" true "printf 'pub fn two() {}\n' > crates/ariad-core/src/two.rs; git add crates/ariad-core/src/two.rs"
assert_pr "Rust file renamed in a crate" true "git mv crates/ariad-core/src/lib.rs crates/ariad-core/src/lib_old.rs"
assert_pr "asset moved out of the crate" true "mkdir -p data; git mv crates/ariad-core/assets/data.json data/data.json"
assert_pr "crate asset edited" true "printf '{\"a\":1}\n' > crates/ariad-core/assets/data.json"
assert_pr "crate manifest edited" true "printf 'version = \"1\"\n' >> crates/ariad-core/Cargo.toml"
assert_pr "root README edited with code" true "printf 'x\n' >> README.md; printf 'pub fn two() {}\n' >> crates/ariad-core/src/lib.rs"
assert_pr "justfile added" true "printf 'default:\n' > justfile; git add justfile"
# Documentation-only changes skip the heavy jobs, so nothing packages.
assert_pr "include_str outside the crate" true "printf 'const A: &str = include_str!(\"../../../docs/notes.md\");\n' >> crates/ariad-core/src/lib.rs"
assert_pr "include_bytes outside the crate" true "printf 'static B: &[u8] = include_bytes!(\"../../../README.md\");\n' >> crates/ariad-core/src/lib.rs"
assert_pr "path attribute outside the crate" true "printf '#[path = \"../../other/mod.rs\"]\nmod other;\n' >> crates/ariad-core/src/lib.rs"
assert_pr "include_str inside the crate" false "printf 'const A: &str = include_str!(\"data.txt\");\n' >> crates/ariad-core/src/lib.rs"
assert_pr "docs-only addition" false "printf 'new\n' > docs/new.md; git add docs/new.md"
git_in checkout -q base

echo "=== Summary ==="
echo "Passed: $passed_count, Failed: $failed_count"
if [ "$failed_count" -gt 0 ]; then
  exit 1
fi
