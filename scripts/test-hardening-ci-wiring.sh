#!/usr/bin/env bash
# Executable regression for mandatory database proof and CI publication policy.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 - <<'PY'
from pathlib import Path
import re

def read(path):
    return Path(path).read_text()

recipes = read('justfile')
assert re.search(r'test-auth-db:\n\s+ARIAD_TEST_DATABASE_URL="\$\{ARIAD_TEST_DATABASE_URL:\?[^}]+\}" ARIAD_TEST_REQUIRE_DATABASE=1 pnpm .*oauth-postgres.test.ts', recipes), 'database recipe must require execution'
ci = read('.github/workflows/ci.yml')
server = ci.split('\n  server:\n')[1].split('\n  fuzz:\n')[0]
assert 'ARIAD_TEST_DATABASE_URL:' in server and 'ARIAD_TEST_REQUIRE_DATABASE: "1"' in server
server_recipe = recipes.split('\ntest-server:\n')[1].split('\n# Build/check')[0]
assert re.search(r'ARIAD_TEST_DATABASE_URL=\S+ \\\n\s+ARIAD_TEST_REQUIRE_DATABASE=1 \\\n\s+ARIAD_TEST_S3_ENDPOINT=\S+ \\\n\s+cargo nextest run -p ariad-server', server_recipe), 'server recipe must require database execution'
for step in ('Set up pnpm', 'Set up Node.js', 'Install JavaScript dependencies', 'Install the PostgreSQL 18 client'):
    assert server.index(step) < server.index('run: just test-auth-db'), step
assert "pg_dump --version" in server and "postgresql-client-18" in server
web = ci.split('\n  web:\n')[1].split('\n  ci-passed:\n')[0]
# Auth-only paths intentionally skip Rust/server jobs. The existing web Vitest
# pass must also have a database, or such PRs would still skip the lock suite.
assert 'postgres: *auth-test-postgres' in web
assert 'ARIAD_TEST_DATABASE_URL:' in web and 'ARIAD_TEST_REQUIRE_DATABASE: "1"' in web
assert web.index('Install the PostgreSQL 18 client') < web.index('run: just web-ci')
assert 'pg_dump --version' in web
cloud = read('.github/workflows/cloud-e2e.yml')
oauth = cloud.split('\n  mcp-oauth:\n')[1].split('\n  status:\n')[0]
assert 'needs: scope' in oauth and "needs.scope.outputs.run == 'true'" in oauth
assert 'ref: *source-ref' in oauth and 'EXPECTED_SOURCE_SHA:' in oauth
assert 'git rev-parse HEAD' in oauth and 'persist-credentials: false' in oauth
assert 'contents: read' in oauth and 'actions: write' not in oauth
assert 'cancel-in-progress:' in oauth and "github.event.action != 'labeled'" in oauth
assert 'bash infra/test/e2e/mcp-oauth.sh' in oauth
assert 'pack-wait' not in oauth and 'actions/cache' not in oauth
assert 'needs: [scope, e2e]' in cloud, 'preserve required Compose status contract'
assert 'cargo fuzz list' in recipes and '-timeout=10 -rss_limit_mb=2048' in recipes
assert 'pdf_reader) build_options=(--dev --features pdf-reader)' in recipes
assert 'typst_writer) build_options=(--dev --features typst-writer)' in recipes
nightly = read('.github/workflows/fuzz-nightly.yml')
assert 'uses: actions/cache@' not in nightly, 'combined cache action publishes on PR/dispatch'
binary_savers = 0
corpus_savers = 0
for block in nightly.split('      - name: ')[1:]:
    if 'uses: actions/cache/save@' in block:
        assert "github.event_name == 'schedule'" in block
        if 'path: ~/.cargo/bin/cargo-fuzz' in block:
            assert "matrix.target == 'markdown_reader'" in block
            binary_savers += 1
        if 'path: fuzz/corpus/${{ matrix.target }}' in block:
            corpus_savers += 1
assert binary_savers == 1 and corpus_savers == 1
assert 'path: fuzz/corpus/${{ matrix.target }}' in nightly
print('CI wiring regression passed')
PY
