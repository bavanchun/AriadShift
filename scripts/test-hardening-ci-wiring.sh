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
static = ci.split('\n  static:\n')[1].split('\n  lint-windows:\n')[0]
assert 'cargo check --locked -p ariad-pdf --no-default-features\n' in static
assert 'cargo check --locked -p ariad-pdf --no-default-features --features codecs' in static
fuzz_targets = re.search(r'targets="([^"]+)"', ci)
assert fuzz_targets, 'CI fuzz target list is missing'
for target in fuzz_targets.group(1).split():
    assert any(Path('fuzz/seeds', target).iterdir()), f'missing fuzz seed for {target}'
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
assert 'pdf_reader|pdf_open_bounded|pdf_tools_structural|pdf_draw_params|pdf_form_interchange|pdf_form_structure|pdf_redact_params|reader_cbz) build_options=(--dev --features pdf-reader)' in recipes
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

uv run --no-project --with pyyaml==6.0.3 python -I - <<'PY'
from pathlib import Path
import re
import yaml

def load_yaml(path):
    return yaml.safe_load(Path(path).read_text(encoding='utf-8'))

def step_strings(obj):
    if isinstance(obj, dict):
        for k, v in obj.items():
            yield str(k)
            yield from step_strings(v)
    elif isinstance(obj, list):
        for item in obj:
            yield from step_strings(item)
    elif isinstance(obj, str):
        yield obj

def find_checkout(steps, job_label):
    for idx, step in enumerate(steps):
        uses = step.get('uses') or ''
        if uses.lower().startswith('actions/checkout@'):
            assert re.fullmatch(r'actions/checkout@[0-9a-f]{40}', uses, re.IGNORECASE), (
                f"{job_label}: actions/checkout must be pinned to 40-hex SHA, got {uses!r}"
            )
            with_cfg = step.get('with') or {}
            assert with_cfg.get('persist-credentials') is False, (
                f"{job_label}: checkout must set persist-credentials: false"
            )
            return idx, with_cfg
    raise AssertionError(f"{job_label}: missing actions/checkout step")

def find_ancestry_guard(steps, job_label, *, allowed_ifs=(None,), require_bash=False):
    needle = 'git merge-base --is-ancestor "$GITHUB_SHA" refs/remotes/origin/main'
    for idx, step in enumerate(steps):
        run = step.get('run') or ''
        if needle in run:
            assert not step.get('continue-on-error'), (
                f"{job_label}: ancestry guard must not set continue-on-error"
            )
            assert step.get('if') in allowed_ifs, (
                f"{job_label}: unexpected if condition on ancestry guard: {step.get('if')!r}"
            )
            assert 'set +e' not in run and '|| true' not in run, (
                f"{job_label}: ancestry guard must fail closed"
            )
            if require_bash:
                assert step.get('shell') == 'bash', (
                    f"{job_label}: ancestry guard must set shell: bash"
                )
            return idx
    raise AssertionError(f"{job_label}: missing ancestry guard step ({needle})")

# 1. images.yml: build & publish must fail closed unless commit is an ancestor of origin/main.
images = load_yaml('.github/workflows/images.yml')
img_build = images['jobs']['build']
build_steps = img_build['steps']
b_checkout_idx, b_checkout_with = find_checkout(build_steps, 'images.yml build')
b_depth = b_checkout_with.get('fetch-depth')
assert b_depth == 0 or (
    isinstance(b_depth, str)
    and re.fullmatch(
        r"\$\{\{\s*github\.event_name\s*!=\s*'push'\s*&&\s*1\s*\|\|\s*0\s*\}\}",
        b_depth.strip(),
    )
), f"images.yml build fetch-depth must fetch full history on push (avoiding falsy && 0 || 1), got {b_depth!r}"
b_guard_idx = find_ancestry_guard(
    build_steps,
    'images.yml build',
    allowed_ifs=(None, "github.event_name == 'push'", "${{ github.event_name == 'push' }}"),
)
assert b_checkout_idx < b_guard_idx, 'images.yml build guard must follow checkout'
for idx, step in enumerate(build_steps):
    text = '\n'.join(step_strings(step))
    if any(tok in text for tok in ('docker/login-action', 'docker/setup-buildx-action', 'docker buildx', 'cosign sign')):
        assert b_guard_idx < idx, f"images.yml build step {idx} ({step.get('name')!r}) runs before ancestry guard"

img_publish = images['jobs']['publish']
assert (img_publish.get('permissions') or {}).get('contents') == 'read', (
    'images.yml publish must declare contents: read for checkout'
)
assert "github.event_name == 'push'" in str(img_publish.get('if') or ''), (
    'images.yml publish must remain restricted to push events'
)
pub_steps = img_publish['steps']
p_checkout_idx, p_checkout_with = find_checkout(pub_steps, 'images.yml publish')
assert p_checkout_with.get('fetch-depth') == 0, (
    f"images.yml publish must set fetch-depth: 0, got {p_checkout_with.get('fetch-depth')!r}"
)
p_guard_idx = find_ancestry_guard(pub_steps, 'images.yml publish')
assert p_checkout_idx < p_guard_idx, 'images.yml publish guard must follow checkout'
for idx, step in enumerate(pub_steps):
    text = '\n'.join(step_strings(step))
    if any(tok in text for tok in ('docker/login-action', 'docker/setup-buildx-action', 'docker buildx', 'cosign sign', 'actions/download-artifact')):
        assert p_guard_idx < idx, f"images.yml publish step {idx} ({step.get('name')!r}) runs before ancestry guard"

# 2. desktop.yml: release & verify-release must fail closed before accessing release secrets or verifying.
desktop = load_yaml('.github/workflows/desktop.yml')
d_release = desktop['jobs']['release']
dr_steps = d_release['steps']
dr_checkout_idx, dr_checkout_with = find_checkout(dr_steps, 'desktop.yml release')
assert dr_checkout_with.get('fetch-depth') == 0, (
    f"desktop.yml release must set fetch-depth: 0, got {dr_checkout_with.get('fetch-depth')!r}"
)
dr_guard_idx = find_ancestry_guard(dr_steps, 'desktop.yml release', require_bash=True)
assert dr_checkout_idx < dr_guard_idx, 'desktop.yml release guard must follow checkout'

secret_pat = re.compile(r'secrets\.(?:TAURI_SIGNING_PRIVATE_KEY|WINDOWS_CERTIFICATE|APPLE_)')
sensitive_names = {
    'Require updater signing material',
    'Import Windows signing certificate when configured',
    'Wait for the CLI workflow to create the release',
    'Build and attach installers and signed updater metadata',
}
for idx, step in enumerate(dr_steps):
    text = '\n'.join(step_strings(step))
    if secret_pat.search(text) or step.get('name') in sensitive_names:
        assert dr_guard_idx < idx, (
            f"desktop.yml release step {idx} ({step.get('name')!r}) accesses secrets or release APIs before ancestry guard"
        )

d_verify = desktop['jobs']['verify-release']
dv_steps = d_verify['steps']
dv_checkout_idx, dv_checkout_with = find_checkout(dv_steps, 'desktop.yml verify-release')
assert dv_checkout_with.get('fetch-depth') == 0, (
    f"desktop.yml verify-release must set fetch-depth: 0, got {dv_checkout_with.get('fetch-depth')!r}"
)
dv_guard_idx = find_ancestry_guard(dv_steps, 'desktop.yml verify-release', require_bash=True)
assert dv_checkout_idx < dv_guard_idx, 'desktop.yml verify-release guard must follow checkout'
for idx, step in enumerate(dv_steps):
    if 'verify-release.mjs' in (step.get('run') or ''):
        assert dv_guard_idx < idx, 'desktop.yml verify-release must check ancestry before verify-release.mjs'

# 3. release.yml: push tags must use the v* allowlist.
release = load_yaml('.github/workflows/release.yml')
on_block = release.get('on') if 'on' in release else release.get(True)
assert isinstance(on_block, dict) and isinstance(on_block.get('push'), dict)
assert on_block['push'].get('tags') == ['v*'], (
    f"release.yml on.push.tags must be ['v*'], got {on_block['push'].get('tags')!r}"
)

print('Release guard CI wiring assertions passed')
PY
