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
static = ci.split('\n  static:\n')[1].split('\n  lint-native:\n')[0]
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
assert 'pdf_reader|pdf_open_bounded|pdf_tools_structural|pdf_draw_params|pdf_form_interchange|pdf_form_structure|pdf_redact_params|type1_to_cff|reader_cbz) build_options=(--dev --features pdf-reader)' in recipes
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

# 4. macos.yml: the ad-hoc release mode keeps the signed path's gates, stays tag-only and keeps every secret scoped.
macos = load_yaml('.github/workflows/macos.yml')
m_on = macos.get('on') if 'on' in macos else macos.get(True)
assert m_on['push'].get('tags') == ['v*'], (
    f"macos.yml on.push.tags must be ['v*'], got {m_on['push'].get('tags')!r}"
)
APPLE_SECRETS = [
    'APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY',
    'APPLE_API_KEY_ID', 'APPLE_API_ISSUER', 'APPLE_API_KEY',
]
SPARKLE_SECRET = 'SPARKLE_ED_PRIVATE_KEY'
# The credential check learns only whether the Sparkle key is set, through a boolean; the key itself goes to the appcast steps.
SPARKLE_FLAG = 'SPARKLE_ED_KEY_PRESENT'
SPARKLE_FLAG_VALUE = "${{ secrets.SPARKLE_ED_PRIVATE_KEY != '' }}"
RAW_SPARKLE_KEY = re.compile(r'^\s*\$\{\{\s*secrets\.SPARKLE_ED_PRIVATE_KEY\s*\}\}\s*$')
APPCAST_STEPS = ('Generate Sparkle release appcast', 'Generate Sparkle appcast (ad-hoc)')

def secrets_in(obj):
    # Secret names referenced inside ${{ }} expressions. Any other use of the secrets context (secrets[...],
    # toJSON(secrets)) reports as '<dynamic>', which no step is allowed.
    found = set()
    for text in step_strings(obj):
        for expression in re.findall(r'\$\{\{(.*?)\}\}', text, re.DOTALL):
            for match in re.finditer(r'\bsecrets\b(?:\s*\.\s*([A-Za-z0-9_]+))?', expression):
                found.add(match.group(1) or '<dynamic>')
    return found

def step_named(steps, name):
    found = [step for step in steps if step.get('name') == name]
    assert len(found) == 1, f"macos.yml: expected exactly one step named {name!r}, found {len(found)}"
    return found[0]

m_sign = macos['jobs']['release-sign']
assert m_sign.get('environment') == 'release', 'macos.yml release-sign must run in the release environment'
assert "github.event_name == 'push'" in str(m_sign.get('if')) and "startsWith(github.ref, 'refs/tags/v')" in str(m_sign.get('if')), (
    'macos.yml release-sign must stay restricted to v* tag pushes'
)
assert m_sign['outputs']['mode'] == '${{ steps.mode.outputs.mode }}', 'macos.yml release-sign must export the mode'
ms = m_sign['steps']
ms_names = [step.get('name') for step in ms if step.get('name')]
assert len(ms_names) == len(set(ms_names)), 'macos.yml release-sign has duplicate step names'
ms_checkout_idx, ms_checkout_with = find_checkout(ms, 'macos.yml release-sign')
assert ms_checkout_with.get('fetch-depth') == 0, 'macos.yml release-sign must fetch full history for the ancestry guard'
ms_guard_idx = find_ancestry_guard(ms, 'macos.yml release-sign')
assert (ms_checkout_idx, ms_guard_idx) == (0, 1), 'macos.yml release-sign: checkout then the ancestry guard must be the first two steps'
for step in ms[: ms_guard_idx + 1]:
    assert not secrets_in(step), 'macos.yml release-sign: no step before or at the ancestry guard may reference a secret'

mode_idx = next(idx for idx, step in enumerate(ms) if step.get('id') == 'mode')
assert mode_idx > ms_guard_idx, 'macos.yml release-sign: the credential check must follow the ancestry guard'
mode_step = ms[mode_idx]
assert mode_step.get('env') == {
    **{name: '${{ secrets.%s }}' % name for name in APPLE_SECRETS},
    SPARKLE_FLAG: SPARKLE_FLAG_VALUE,
}, 'macos.yml mode step must receive exactly the six Apple secrets and the Sparkle boolean, never the Sparkle key'
assert 'bash scripts/macos-check-secrets.sh' in mode_step['run']
assert SPARKLE_SECRET not in mode_step['run'], 'macos.yml mode step must not read the Sparkle key'
assert mode_step.get('if') is None and not mode_step.get('continue-on-error')

# Every secret is confined to the steps that need it. Ad-hoc steps never see an Apple secret.
allowed_secrets = {
    # The credential check names the Sparkle secret only inside the `!= ''` comparison that builds the boolean (pinned
    # by the exact env equality above), so the key value itself never enters this step.
    mode_step['name']: set(APPLE_SECRETS + [SPARKLE_SECRET]),
    'Sign, notarize and package release artifacts': set(APPLE_SECRETS),
    'Generate Sparkle release appcast': {SPARKLE_SECRET},
    'Generate Sparkle appcast (ad-hoc)': {SPARKLE_SECRET},
}
for step in ms:
    extra = secrets_in(step) - allowed_secrets.get(step.get('name'), set())
    assert not extra, f"macos.yml release-sign step {step.get('name')!r} references unexpected secrets {sorted(extra)}"

# The real Sparkle key (the bare secret, not a boolean built from it) reaches only the appcast steps, as an env value.
for step in ms:
    holders = [key for key, value in (step.get('env') or {}).items() if isinstance(value, str) and RAW_SPARKLE_KEY.match(value)]
    if step.get('name') in APPCAST_STEPS:
        assert holders == [SPARKLE_SECRET], f"macos.yml step {step.get('name')!r} must receive the Sparkle key as {SPARKLE_SECRET}"
    else:
        assert not holders, f"macos.yml step {step.get('name')!r} must not receive the Sparkle key itself, found it in {holders}"
    assert not any(RAW_SPARKLE_KEY.match(text) for text in step_strings(step.get('run') or '')), (
        f"macos.yml step {step.get('name')!r} must not interpolate the Sparkle key into a script"
    )
# The runner masks secrets itself. An explicit ::add-mask:: of a multi-line key would register each line on its own and
# leak the others, so no step may add one.
assert 'add-mask' not in '\n'.join(step_strings(macos)), 'macos.yml must not call ::add-mask:: on a secret'
adhoc_appcast_run = step_named(ms, 'Generate Sparkle appcast (ad-hoc)')['run']
assert '[[:space:]]' in adhoc_appcast_run, 'the ad-hoc appcast step must reject a whitespace-only Sparkle key, which the boolean cannot reveal'

# Each mode runs only its own steps; a widened condition would let one mode do another's work.
mode_gate = lambda *modes: ' || '.join(f"steps.mode.outputs.mode == '{mode}'" for mode in modes)
for name, condition in {
    'Sign, notarize and package release artifacts': mode_gate('signed'),
    'Generate Sparkle release appcast': mode_gate('signed'),
    'Build unsigned DMG (workflow artifact only)': mode_gate('unsigned'),
    'Upload unsigned DMG as workflow artifact': mode_gate('unsigned'),
    'Sign the application ad-hoc and verify it': mode_gate('adhoc'),
    'Build ad-hoc DMG': mode_gate('adhoc'),
    'Generate Sparkle appcast (ad-hoc)': mode_gate('adhoc'),
    'Record SHA-256 digests of the release assets': mode_gate('signed', 'adhoc'),
    'Upload signed release assets as workflow artifact': mode_gate('signed', 'adhoc'),
}.items():
    assert step_named(ms, name).get('if') == condition, f"macos.yml step {name!r} must run only when {condition}"
assert 'macos-adhoc-sign.sh app' in step_named(ms, 'Sign the application ad-hoc and verify it')['run']
assert 'macos-dmg.sh' in step_named(ms, 'Build ad-hoc DMG')['run']
upload = step_named(ms, 'Upload signed release assets as workflow artifact')
upload_paths = upload['with']['path']
assert upload['with']['name'] == 'AriadShift-release-assets' and upload['with']['if-no-files-found'] == 'error'
assert '*' not in upload_paths and 'universal.dmg' in upload_paths and 'appcast.xml' in upload_paths, (
    'macos.yml release asset artifact must list explicit file names'
)

# Publishing: tag-only, signed or ad-hoc only, the workflow token and nothing else.
m_pub = macos['jobs']['release-publish']
assert m_pub.get('needs') == ['release-sign'] and m_pub.get('permissions') == {'contents': 'write'}
assert 'environment' not in m_pub, 'macos.yml release-publish must not use an environment with secrets'
for needle in (
    "github.event_name == 'push'",
    "startsWith(github.ref, 'refs/tags/v')",
    "needs.release-sign.outputs.mode == 'signed'",
    "needs.release-sign.outputs.mode == 'adhoc'",
):
    assert needle in str(m_pub.get('if')), f"macos.yml release-publish condition must contain {needle}"
assert 'unsigned' not in str(m_pub.get('if')), 'macos.yml release-publish must never run for an unsigned build'
assert secrets_in(m_pub) == {'GITHUB_TOKEN'}, f"macos.yml release-publish may use only GITHUB_TOKEN, found {sorted(secrets_in(m_pub))}"
m_pub_text = '\n'.join(step_strings(m_pub))
assert SPARKLE_SECRET not in m_pub_text and 'APPLE_' not in m_pub_text
m_build_text = '\n'.join(step_strings(macos['jobs']['build']))
assert SPARKLE_SECRET not in m_build_text and 'APPLE_' not in m_build_text, 'macos.yml build job must not touch signing credentials'

# Every PR and branch build proves the real ad-hoc signing layout on the Release app, off the tag path, without a secret,
# and on a copy so the build output stays what the tests and the unsigned artifact expect.
mb = macos['jobs']['build']['steps']
proof_name = 'Sign a copy of the Release app ad-hoc and verify it'
proof = step_named(mb, proof_name)
build_idx = mb.index(step_named(mb, 'Build macOS application bundle'))
proof_idx = mb.index(proof)
assert build_idx < proof_idx, 'macos.yml build job must sign after the Release app is built'
assert proof_idx < mb.index(step_named(mb, 'Compress application bundle')), 'macos.yml build job must sign before the app is packaged'
assert proof.get('if') == "${{ !startsWith(github.ref, 'refs/tags/') }}", 'macos.yml PR signing proof must stay off the tag path'
assert not proof.get('continue-on-error'), 'macos.yml PR signing proof must fail the job'
assert not secrets_in(proof) and 'env' not in proof and 'uses' not in proof, 'macos.yml PR signing proof must use no secret or env'
proof_run = proof['run']
sign_lines = [line for line in proof_run.splitlines() if 'macos-adhoc-sign.sh' in line]
assert len(sign_lines) == 1 and 'bash scripts/macos-adhoc-sign.sh app ' in sign_lines[0], 'macos.yml must run the real macos-adhoc-sign.sh app'
assert 'Build/Products' not in sign_lines[0] and 'APP_PATH' not in sign_lines[0], (
    'macos.yml PR signing proof must sign the copy, never the build output in place'
)
assert re.search(r'^\s*ditto "\$\{APP_PATH\}" "\$\{COPY_DIR\}/AriadShift\.app"\s*$', proof_run, re.MULTILINE), 'macos.yml must ditto the Release app before signing'
assert 'trap ' in proof_run and 'rm -rf' in proof_run, 'macos.yml PR signing proof must remove its copy'
# The release-sign job still signs for a tag.
assert 'macos-adhoc-sign.sh app' in step_named(ms, 'Sign the application ad-hoc and verify it')['run']

# The release helper tests run on the macOS job and, where they need no macOS tool, in `just static` on Linux.
m_tests = step_named(macos['jobs']['build']['steps'], 'Test release helper scripts')['run']
justfile_text = Path('justfile').read_text(encoding='utf-8')
assert re.search(r'^static:.*\btest-macos-release-scripts\b', justfile_text, re.MULTILINE), 'just static must run test-macos-release-scripts'
macos_recipe = justfile_text.split('\ntest-macos-release-scripts:\n')[1].split('\n\n')[0]
for test_script in ('macos-check-secrets.test.sh', 'macos-check-secrets-workflow.test.sh', 'macos-adhoc-sign.test.sh', 'macos-appcast.test.sh'):
    assert f'bash scripts/{test_script}' in m_tests, f"macos.yml build job must run {test_script}"
    assert f'bash scripts/{test_script}' in macos_recipe, f"test-macos-release-scripts must run {test_script}"
assert 'bash scripts/macos-check-release-plan.test.sh' in m_tests

# 5. ci.yml: the Linux Verify parts take their matrix from the scope job, and every job feeds the
# CI passed result, so a failed part can never leave the gate green.
ci_jobs = load_yaml('.github/workflows/ci.yml')['jobs']
ci_passed = ci_jobs['ci-passed']
assert set(ci_passed['needs']) == set(ci_jobs) - {'ci-passed'}, 'ci.yml: CI passed must wait for every other job'
linux = ci_jobs['verify-linux']
heavy_if = "needs.scope.outputs.heavy == 'true'"
os_if = f"{heavy_if} && needs.scope.outputs.os_verify == 'true'"
assert linux['needs'] == 'scope' and linux['if'] == heavy_if, 'ci.yml: verify-linux must follow the heavy scope alone'
for name in ('verify', 'lint-native'):
    assert ci_jobs[name]['needs'] == 'scope' and ci_jobs[name]['if'] == os_if, (
        f'ci.yml: {name} must follow the heavy scope and the os_verify decision'
    )
assert linux['strategy']['matrix']['include'] == '${{ fromJSON(needs.scope.outputs.linux_parts) }}', (
    'ci.yml: verify-linux must take its parts from the scope job'
)
assert linux['strategy']['fail-fast'] is False
for key in ('environment', 'permissions'):
    assert linux.get(key) == ci_jobs['verify'].get(key), f"ci.yml: verify-linux must keep the {key} of verify"
assert all(entry['os'] != 'ubuntu-26.04' for entry in ci_jobs['verify']['strategy']['matrix']['include']), (
    'ci.yml: Linux runs in verify-linux, not in the verify matrix'
)
assert {'package', 'linux_parts', 'os_verify'} <= set(ci_jobs['scope']['outputs']), (
    'ci.yml: scope must export package, linux_parts and os_verify'
)
gate = ci_passed['steps'][0]
assert gate['env']['VERIFY_LINUX'] == '${{ needs.verify-linux.result }}'
assert gate['run'].count('[ "$VERIFY_LINUX" = success ]') == 1 and gate['run'].count('[ "$VERIFY_LINUX" = skipped ]') == 1, (
    'ci.yml: CI passed must require verify-linux to succeed, or to be skipped when the heavy jobs are'
)
assert gate['env']['OS_VERIFY'] == '${{ needs.scope.outputs.os_verify }}'
assert gate['env']['LINT_NATIVE'] == '${{ needs.lint-native.result }}' and gate['env']['VERIFY'] == '${{ needs.verify.result }}'
assert 'os-verify=%s' in gate['run'] and '"$OS_VERIFY"' in gate['run'], 'ci.yml: CI passed must print the os_verify decision'
gate_lines = [line.strip() for line in gate['run'].split('\n') if line.strip() and not line.strip().startswith('#')]
gate_logic = '\n'.join(gate_lines)
heavy_gate = '\n'.join([
    'if [ "$HEAVY" = true ]; then',
    '[ "$VERIFY_LINUX" = success ] && [ "$SERVER" = success ] && [ "$FUZZ" = success ] || exit 1',
    'if [ "$OS_VERIFY" = true ]; then',
    '[ "$LINT_NATIVE" = success ] && [ "$VERIFY" = success ] || exit 1',
    'elif [ "$OS_VERIFY" = false ]; then',
    '[ "$LINT_NATIVE" = skipped ] && [ "$VERIFY" = skipped ] || exit 1',
    'else',
    'echo "os_verify is neither true nor false: \'$OS_VERIFY\'" >&2',
    'exit 1',
    'fi',
    'else',
    '[ "$LINT_NATIVE" = skipped ] && [ "$VERIFY" = skipped ] && [ "$VERIFY_LINUX" = skipped ] && '
    '[ "$SERVER" = skipped ] && [ "$FUZZ" = skipped ] || exit 1',
    'fi',
])
assert heavy_gate in gate_logic, (
    'ci.yml: with heavy jobs CI passed must require verify-linux, server and fuzz, require lint-native and '
    'verify to succeed when os_verify is true and to be skipped when it is false, and require every heavy '
    'job to be skipped otherwise'
)
assert not any(step.get('name') == 'Check packaging' for step in ci_jobs['verify']['steps']), (
    'ci.yml: the macOS and Windows verify job must not run the packaging check; the Linux package part does'
)
linux_packaging = step_named(ci_jobs['verify-linux']['steps'], 'Check packaging')
assert linux_packaging['if'] == "matrix.part == 'package'", 'ci.yml: the Linux package part runs the packaging check'

print('Release guard CI wiring assertions passed')
PY
