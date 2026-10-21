#!/bin/sh
# Structural YAML security and supply-chain verifier for .github/workflows/release.yml.
# Parses workflow structure using PyYAML via uv to assert strict security invariants:
# 1. secrets.HOMEBREW_TAP_TOKEN appears exactly once in the file, strictly inside
#    jobs.publish-homebrew-formula.steps[name=="Push formula"].env.
# 2. jobs.prepare-homebrew-formula has no secret references and permissions: {contents: read}.
# 3. Every job's permissions map equals its minimal expected map; no extra writes.
# 4. Every actions/checkout step enforces persist-credentials: false (boolean).
# 5. Every uses: action reference ends with @<40 hex commit SHA> (case-insensitive).
# 6. Every tool: in install-action specifies an exact @x.y.z version.
# 7. Plan, Unix, Windows dist install steps and CycloneDX step verify expected SHA-256
#    hashes inside their own step run text (comments stripped) before extracting or executing,
#    with no masking (||, &&, ;) or set +e.
# 8. Windows install step verifies with Get-FileHash, -ne check, and throw, with no hash overwrite.
# 9. No shell piping (| sh, | bash, | iex), no irm/iwr piping, no unverified script execution.
# 10. No matrix.install_* expressions of any spelling.
# 11. Build artifacts step in build-local-artifacts explicitly specifies shell: bash.
# 12. publish-homebrew-formula declares environment: release and contains no brew execution.
# 13. Rejects duplicate step names within any job, and rejects if: / continue-on-error on security steps.
# 14. Validates pwsh syntax for PowerShell steps via Parser::ParseInput when pwsh is on PATH.
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
workflow="${1:-$repo_root/.github/workflows/release.yml}"
publish_crates="${2:-$repo_root/.github/workflows/publish-crates.yml}"

if [ ! -f "$workflow" ]; then
  echo "ERROR: Workflow file not found: $workflow" >&2
  exit 1
fi

uv run --no-project --with pyyaml==6.0.3 python -I - "$workflow" "$publish_crates" << 'PYEOF'
import sys, re, os, shutil, subprocess, yaml

def fail(msg):
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)

def _no_traceback(exc_type, exc, tb):
    print(f"ERROR: unexpected workflow structure ({exc_type.__name__}: {exc})", file=sys.stderr)
sys.excepthook = _no_traceback

workflow_path = sys.argv[1]
publish_crates_path = sys.argv[2] if len(sys.argv) > 2 else ""

try:
    with open(workflow_path, 'r', encoding='utf-8') as f:
        raw_text = f.read()
except Exception as e:
    fail(f"Could not read workflow file {workflow_path}: {e}")

try:
    data = yaml.safe_load(raw_text)
except Exception as e:
    fail(f"Failed to parse YAML in {workflow_path}: {e}")

if not isinstance(data, dict):
    fail(f"Top-level YAML in {workflow_path} is not a mapping")

jobs = data.get('jobs')
if not isinstance(jobs, dict):
    fail("'jobs' section missing or not a mapping")

# --- 1. Walk every string value in parsed YAML to audit secrets & tokens ---
# Allowed occurrences of strings referencing secrets (case-insensitive):
# 1. GH_TOKEN in job-level env of plan, build-local-artifacts, build-global-artifacts, host, prepare-homebrew-formula, announce
# 2. HOMEBREW_TAP_TOKEN in publish-homebrew-formula Push formula step env
# 3. CARGO_REGISTRY_TOKEN in custom-publish-crates secrets
def walk_elements(obj, path=()):
    if isinstance(obj, dict):
        for k, v in obj.items():
            yield (str(k), path + (f"key:{k}",))
            yield from walk_elements(v, path + (str(k),))
    elif isinstance(obj, list):
        for idx, item in enumerate(obj):
            yield from walk_elements(item, path + (str(idx),))
    elif isinstance(obj, str):
        yield (obj, path)

tap_token_secret_count = 0
for text, path in walk_elements(data):
    if re.search(r'secrets', text, re.IGNORECASE):
        # Validate that this secret reference is at an authorized path
        is_authorized = False
        if len(path) == 4 and path[0] == 'jobs' and path[2] == 'env' and path[3] == 'GH_TOKEN':
            if path[1] in ('plan', 'build-local-artifacts', 'build-global-artifacts', 'host', 'prepare-homebrew-formula', 'publish-homebrew-formula', 'announce'):
                if re.match(r'^\$\{\{\s*secrets\.GITHUB_TOKEN\s*\}\}$', text.strip()):
                    is_authorized = True
        elif len(path) >= 5 and path[0] == 'jobs' and path[1] == 'publish-homebrew-formula' and path[2] == 'steps':
            # Must be in step env for HOMEBREW_TAP_TOKEN
            step_idx = int(path[3]) if path[3].isdigit() else -1
            steps = jobs.get('publish-homebrew-formula', {}).get('steps') or []
            if 0 <= step_idx < len(steps):
                step = steps[step_idx]
                if isinstance(step, dict) and step.get('name') == 'Push formula' and len(path) == 6 and path[4] == 'env' and path[5] == 'HOMEBREW_TAP_TOKEN':
                    if re.match(r'^\$\{\{\s*secrets(\.HOMEBREW_TAP_TOKEN|\s*\[\s*[\'"]HOMEBREW_TAP_TOKEN[\'"]\s*\])\s*\}\}$', text.strip()):
                        is_authorized = True
                        tap_token_secret_count += 1
        elif len(path) == 3 and path[0] == 'jobs' and path[1] == 'custom-publish-crates' and path[2] == 'key:secrets':
            if text == 'secrets':
                is_authorized = True
        elif len(path) == 4 and path[0] == 'jobs' and path[1] == 'custom-publish-crates' and path[2] == 'secrets' and path[3] == 'CARGO_REGISTRY_TOKEN':
            if re.match(r'^\$\{\{\s*secrets(\.CARGO_REGISTRY_TOKEN|\s*\[\s*[\'"]CARGO_REGISTRY_TOKEN[\'"]\s*\])\s*\}\}$', text.strip()):
                is_authorized = True

        if not is_authorized:
            fail(f"Unauthorized secret reference '{text}' at path {' -> '.join(path)}")

    # Check for HOMEBREW_TAP_TOKEN token name outside authorized locations
    if 'HOMEBREW_TAP_TOKEN' in text:
        is_token_path = False
        if len(path) >= 5 and path[0] == 'jobs' and path[1] == 'publish-homebrew-formula' and path[2] == 'steps':
            step_idx = int(path[3]) if path[3].isdigit() else -1
            steps = jobs.get('publish-homebrew-formula', {}).get('steps') or []
            if 0 <= step_idx < len(steps):
                step = steps[step_idx]
                if isinstance(step, dict) and step.get('name') == 'Push formula':
                    # Allowed in env key/value or run text
                    if (len(path) == 6 and path[4] == 'env' and (path[5] == 'HOMEBREW_TAP_TOKEN' or path[5] == 'key:HOMEBREW_TAP_TOKEN')) or (len(path) == 5 and path[4] == 'run'):
                        is_token_path = True
        if not is_token_path:
            fail(f"HOMEBREW_TAP_TOKEN referenced outside Push formula step at {' -> '.join(path)}")

if tap_token_secret_count != 1:
    fail(f"secrets.HOMEBREW_TAP_TOKEN must occur exactly once in workflow structure, found {tap_token_secret_count}")

# Check that HOMEBREW_TAP_TOKEN env definition is not in job-level env of publish-homebrew-formula
pub_job = jobs.get('publish-homebrew-formula')
if not isinstance(pub_job, dict):
    fail("Job 'publish-homebrew-formula' missing or not a mapping")
if 'HOMEBREW_TAP_TOKEN' in (pub_job.get('env') or {}):
    fail("HOMEBREW_TAP_TOKEN must not be in job-level env of publish-homebrew-formula")

# --- 2. Top-level and job-level permissions ---
if data.get('permissions') != {'contents': 'read'}:
    fail(f"Top-level permissions must be strictly 'contents: read', got {data.get('permissions')}")

expected_job_perms = {
    'plan': None,
    'build-local-artifacts': None,
    'build-global-artifacts': None,
    'host': {'contents': 'write'},
    'prepare-homebrew-formula': {'contents': 'read'},
    'publish-homebrew-formula': {'contents': 'read'},
    'custom-publish-crates': {'contents': 'read'},
    'announce': {'contents': 'read'},
}

for jname, jdata in jobs.items():
    if not isinstance(jdata, dict):
        fail(f"Job '{jname}' is not a mapping")
    actual_perms = jdata.get('permissions')
    expected = expected_job_perms.get(jname, '__unexpected__')
    if expected == '__unexpected__':
        fail(f"Unexpected job '{jname}' found in workflow")
    if actual_perms != expected:
        fail(f"Job '{jname}' permissions mismatch: expected {expected}, got {actual_perms}")

# Job environment checks
if pub_job.get('environment') != 'release':
    fail(f"Job 'publish-homebrew-formula' must declare 'environment: release', got {pub_job.get('environment')!r}")

# prepare-homebrew-formula must not contain secrets or token references
prep_job = jobs.get('prepare-homebrew-formula')
if not isinstance(prep_job, dict):
    fail("Job 'prepare-homebrew-formula' missing or not a mapping")
for text, path in walk_elements(prep_job):
    if re.search(r'secrets\b', text, re.IGNORECASE) and not (len(path) == 2 and path[0] == 'env' and path[1] == 'GH_TOKEN'):
        fail(f"Job 'prepare-homebrew-formula' illegally references secrets: {text}")

# --- 3. Step names uniqueness and security/install step constraints ---
SECURITY_STEPS = {
    'Install dist',
    'Install dist (Unix)',
    'Install dist (Windows)',
    'Install cargo-auditable',
    'Build artifacts',
    'Install cargo-cyclonedx',
    'Push formula',
}

for jname, jdata in jobs.items():
    if not isinstance(jdata, dict):
        continue
    seen_step_names = set()
    steps = jdata.get('steps') or []
    if not isinstance(steps, list):
        fail(f"Steps in job '{jname}' is not a sequence")
    for step in steps:
        if not isinstance(step, dict):
            fail(f"A step in job '{jname}' is not a mapping")
        sname = step.get('name')
        if sname is not None and not isinstance(sname, str):
            fail(f"Step in job '{jname}' has non-string 'name'")
        if sname:
            if sname in seen_step_names:
                fail(f"Job '{jname}' has duplicate step name: '{sname}'")
            seen_step_names.add(sname)

        if sname in SECURITY_STEPS:
            # Reject continue-on-error
            if step.get('continue-on-error') is not None and step.get('continue-on-error') is not False:
                fail(f"Step '{sname}' in job '{jname}' must not declare continue-on-error")
            # Check if: conditions
            s_if = step.get('if')
            if sname == 'Install dist (Unix)':
                if s_if != "runner.os != 'Windows'":
                    fail(f"Step '{sname}' has unauthorized if condition: {s_if!r}")
            elif sname == 'Install dist (Windows)':
                if s_if != "runner.os == 'Windows'":
                    fail(f"Step '{sname}' has unauthorized if condition: {s_if!r}")
            else:
                if 'if' in step and s_if is not None:
                    fail(f"Step '{sname}' must not have an 'if:' condition, got {s_if!r}")

# --- 4. Global installer patterns & script execution bans ---
def strip_comments(text):
    if not isinstance(text, str):
        fail(f"Expected string for text with comments, got {type(text).__name__}")
    lines = []
    for l in text.splitlines():
        trimmed = l.strip()
        if trimmed.startswith('#'):
            continue
        # Strip trailing comments while handling strings simply
        if '#' in l:
            idx = l.find('#')
            l = l[:idx]
        lines.append(l)
    return '\n'.join(lines)

for jname, jdata in jobs.items():
    if not isinstance(jdata, dict):
        continue
    for step in (jdata.get('steps') or []):
        if not isinstance(step, dict):
            continue
        sname = step.get('name')
        if sname is not None and not isinstance(sname, str):
            fail(f"Step in job '{jname}' has non-string 'name'")
        sname = sname or 'unnamed'
        srun = step.get('run')
        if srun is not None and not isinstance(srun, str):
            fail(f"Step '{sname}' in job '{jname}' has non-string 'run'")
        clean_run = strip_comments(srun or '')

        # Ban piping to interpreters: interpreter word anywhere later in same pipeline segment, dot-sourcing, command substitution, eval
        if re.search(r'\|[^|;&\n]*?(?<![\w.-])(?:\S*/)?(sh|bash|zsh|dash|ksh|fish|pwsh|powershell|python[0-9.]*|perl|ruby|node)\b', clean_run, re.I) or re.search(r'(?m)^\s*(?:\.|&|source)\s+\S', clean_run) or re.search(r'\b(?:sh|bash|zsh|python[0-9.]*)\s+(?:-\S+\s+)*[<"\']*\$?\(', clean_run) or re.search(r'\beval\b', clean_run):
            fail(f"Piping to interpreter detected in step '{sname}' of job '{jname}'")

        # Ban Invoke-Expression and iex
        if re.search(r'\b(Invoke-Expression|iex)\b', clean_run, re.I):
            fail(f"Invoke-Expression / iex detected in step '{sname}' of job '{jname}'")

        # Ban Invoke-RestMethod/Invoke-WebRequest piped to anything
        if re.search(r'\b(Invoke-RestMethod|irm|Invoke-WebRequest|iwr)\b[^|\n]*\|', clean_run, re.I):
            fail(f"Invoke-RestMethod/Invoke-WebRequest piped to command detected in step '{sname}' of job '{jname}'")

        # Ban chmod +x except for verified unpacked binaries in ~/.cargo/bin
        if re.search(r'\bchmod\s+\+x\s+(?!~/\.cargo/bin/(?:dist|cargo-cyclonedx)\b)', clean_run):
            fail(f"chmod +x on unverified file detected in step '{sname}' of job '{jname}'")

        # Ban executing downloaded script/binary directly (e.g. ./x, bash x, sh /tmp/x, /bin/sh x)
        # Note: bash -ec "$PACKAGES_INSTALL" is authorized package installation
        if re.search(r'(?:^|[;&|\n])\s*\./[a-zA-Z0-9_.-]+', clean_run):
            fail(f"Direct execution of local file (./...) detected in step '{sname}' of job '{jname}'")
        for m in re.finditer(r'(?:^|[;&|\n])\s*(?:[a-zA-Z0-9_/.-]*/)?(?:bash|sh)\s+([a-zA-Z0-9_/.-]+)', clean_run):
            arg = m.group(1)
            if not arg.startswith('-'):
                fail(f"Executing script via '{m.group(0).strip()}' detected in step '{sname}' of job '{jname}'")

        # No brew execution in publish-homebrew-formula
        if jname == 'publish-homebrew-formula':
            suses = step.get('uses')
            if suses is not None and not isinstance(suses, str):
                fail(f"Step '{sname}' in job '{jname}' has non-string 'uses'")
            suses = suses or ''
            if re.search(r'\bbrew\b', srun or '') or re.search(r'\bbrew\b', suses):
                fail(f"Job 'publish-homebrew-formula' must not run 'brew' commands (found in step '{sname}')")

        # Check matrix.install_
        if re.search(r'matrix\.install_', srun or '') or re.search(r'matrix\[[\'"]install_', srun or ''):
            fail(f"Unverified matrix.install_* expression detected in step '{sname}' of job '{jname}'")

# --- 5. Action pins (@40-hex SHA) and checkout persist-credentials ---
for jname, jdata in jobs.items():
    if not isinstance(jdata, dict):
        continue
    uses = jdata.get('uses')
    if uses and isinstance(uses, str) and not uses.startswith('./'):
        if not re.match(r'^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+@[0-9a-f]{40}$', uses, re.IGNORECASE):
            fail(f"Job '{jname}' uses unpinned reusable workflow: {uses}")

    for step in (jdata.get('steps') or []):
        if not isinstance(step, dict):
            continue
        suses = step.get('uses')
        sname = step.get('name')
        if sname is not None and not isinstance(sname, str):
            fail(f"Step in job '{jname}' has non-string 'name'")
        sname = sname or 'unnamed'
        with_block = step.get('with')
        if with_block is not None and not isinstance(with_block, dict):
            fail(f"Job '{jname}' step '{sname}' has non-mapping 'with'")
        with_block = with_block or {}
        if suses and isinstance(suses, str) and not suses.startswith('./'):
            if not re.match(r'^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+@[0-9a-f]{40}$', suses, re.IGNORECASE):
                fail(f"Job '{jname}' step '{sname}' uses unpinned action: {suses}")

            if suses.lower().startswith('actions/checkout@'):
                val = with_block.get('persist-credentials')
                if val is not False:
                    fail(f"Job '{jname}' step '{sname}' must set 'persist-credentials: false' (boolean), got {val!r}")

        if suses and isinstance(suses, str) and 'install-action' in suses.lower():
            tool_val = with_block.get('tool')
            if tool_val is not None and not isinstance(tool_val, str):
                fail(f"Job '{jname}' step '{sname}' has non-string 'tool'")
            tool_str = tool_val or ''
            for t in tool_str.split(','):
                t = t.strip()
                if not re.match(r'^[^@]+@\d+\.\d+\.\d+$', t):
                    fail(f"Job '{jname}' step '{sname}' has unpinned tool '{t}'")

# --- 6. Verified dist & CycloneDX install steps and expected hashes ---
EXPECTED_HASHES = {
    'dist_x86_musl': 'b8e95bc76c63375958173ef5ae2dbd8e9211cc1ed03cdee0899702766b4c2a2e',
    'dist_aarch64_musl': '4761cff5fc547ad66d1449abbf321380b0e6bd8093b1fe6593852a3314fd0c19',
    'dist_darwin': '7b3cbe25511de01d74c0f5fcb7909edabd379bea9cfa284d93af5a3cdfa3247c',
    'dist_win': '9a36d70795e14326a5ec4bf17aee085df00ab85a322739291ea5d4b1b5f693cd',
    'cyclonedx': '9bd3e599314f50810c9d98b8b68a617ff9d3cc20873968d90b29d121f6b226ff',
}

def get_step_by_name(job_data, step_name):
    for s in (job_data.get('steps') or []):
        if isinstance(s, dict) and s.get('name') == step_name:
            return s
    return None

# Plan job: Install dist
plan_job = jobs.get('plan') or {}
plan_dist_step = get_step_by_name(plan_job, 'Install dist')
if not plan_dist_step:
    fail("Plan job missing 'Install dist' step")
plan_run_raw = plan_dist_step.get('run')
if not isinstance(plan_run_raw, str):
    fail("Plan 'Install dist' step has non-string 'run'")
plan_run = strip_comments(plan_run_raw)
if EXPECTED_HASHES['dist_x86_musl'] not in plan_run:
    fail("Plan 'Install dist' step run does not contain expected x86_64 musl hash")
if re.search(r'set\s+\+(?:[a-z]*e|o\s+errexit)', plan_run):
    fail("Plan 'Install dist' step contains 'set +e'")
if not any(re.search(r'\|\s*shasum\s+-a\s+256\s+-c\s+-\s*$', l) for l in plan_run.splitlines()):
    fail("Plan 'Install dist' step missing unmasked '| shasum -a 256 -c -' verification")
tar_pos = plan_run.find('tar ')
shasum_pos = plan_run.find('shasum')
if tar_pos != -1 and tar_pos < shasum_pos:
    fail("Plan 'Install dist' step executes tar before shasum verification")
exec_match = re.search(r'(?:^|[;&|\n])\s*(?:~/\.cargo/bin/)?dist\b', plan_run)
if exec_match and exec_match.start() < shasum_pos:
    fail("Plan 'Install dist' step executes dist binary before checksum verification")

# Build-local-artifacts: Install dist (Unix)
local_job = jobs.get('build-local-artifacts') or {}
unix_dist_step = get_step_by_name(local_job, 'Install dist (Unix)')
if not unix_dist_step:
    fail("build-local-artifacts job missing 'Install dist (Unix)' step")
unix_run_raw = unix_dist_step.get('run')
if not isinstance(unix_run_raw, str):
    fail("build-local-artifacts 'Install dist (Unix)' step has non-string 'run'")
unix_run = strip_comments(unix_run_raw)
for hkey in ('dist_x86_musl', 'dist_aarch64_musl', 'dist_darwin'):
    if EXPECTED_HASHES[hkey] not in unix_run:
        fail(f"build-local-artifacts 'Install dist (Unix)' step run missing expected hash for {hkey}")
if re.search(r'set\s+\+(?:[a-z]*e|o\s+errexit)', unix_run):
    fail("build-local-artifacts 'Install dist (Unix)' step contains 'set +e'")
if not any(re.search(r'\|\s*shasum\s+-a\s+256\s+-c\s+-\s*$', l) for l in unix_run.splitlines()):
    fail("build-local-artifacts 'Install dist (Unix)' step missing unmasked '| shasum -a 256 -c -' verification")
tar_pos = unix_run.find('tar ')
shasum_pos = unix_run.find('shasum')
if tar_pos != -1 and tar_pos < shasum_pos:
    fail("build-local-artifacts 'Install dist (Unix)' step executes tar before shasum verification")
exec_match = re.search(r'(?:^|[;&|\n])\s*(?:~/\.cargo/bin/)?dist\b', unix_run)
if exec_match and exec_match.start() < shasum_pos:
    fail("build-local-artifacts 'Install dist (Unix)' step executes dist binary before checksum verification")

# Build-local-artifacts: Install dist (Windows)
win_dist_step = get_step_by_name(local_job, 'Install dist (Windows)')
if not win_dist_step:
    fail("build-local-artifacts job missing 'Install dist (Windows)' step")
win_run_raw = win_dist_step.get('run')
if not isinstance(win_run_raw, str):
    fail("build-local-artifacts 'Install dist (Windows)' step has non-string 'run'")
win_run = strip_comments(win_run_raw)
if EXPECTED_HASHES['dist_win'] not in win_run:
    fail("build-local-artifacts 'Install dist (Windows)' step run missing expected Windows hash")
if 'Get-FileHash' not in win_run:
    fail("build-local-artifacts 'Install dist (Windows)' step missing Get-FileHash")
if re.search(r'\$actualHash\s*=\s*\$expectedHash', win_run):
    fail("build-local-artifacts 'Install dist (Windows)' step contains illicit hash overwrite")
if not re.search(r'if \(\$actualHash -ne \$expectedHash\) \{\s*throw ', win_run) or len(re.findall(r'\$(?:actualHash|expectedHash)\s*=', win_run)) != 2:
    fail("build-local-artifacts 'Install dist (Windows)' step missing '-ne' check or throw")
expand_pos = win_run.find('Expand-Archive')
throw_pos = win_run.find('throw ')
if expand_pos != -1 and throw_pos != -1 and throw_pos > expand_pos:
    fail("build-local-artifacts 'Install dist (Windows)' step executes Expand-Archive before verification throw")

# Build-global-artifacts: Install cargo-cyclonedx
global_job = jobs.get('build-global-artifacts') or {}
cyclonedx_step = get_step_by_name(global_job, 'Install cargo-cyclonedx')
if not cyclonedx_step:
    fail("build-global-artifacts job missing 'Install cargo-cyclonedx' step")
cyclonedx_run_raw = cyclonedx_step.get('run')
if not isinstance(cyclonedx_run_raw, str):
    fail("build-global-artifacts 'Install cargo-cyclonedx' step has non-string 'run'")
cyclonedx_run = strip_comments(cyclonedx_run_raw)
if EXPECTED_HASHES['cyclonedx'] not in cyclonedx_run:
    fail("build-global-artifacts 'Install cargo-cyclonedx' step run missing expected CycloneDX hash")
if re.search(r'set\s+\+(?:[a-z]*e|o\s+errexit)', cyclonedx_run):
    fail("build-global-artifacts 'Install cargo-cyclonedx' step contains 'set +e'")
if not any(re.search(r'\|\s*shasum\s+-a\s+256\s+-c\s+-\s*$', l) for l in cyclonedx_run.splitlines()):
    fail("build-global-artifacts 'Install cargo-cyclonedx' step missing unmasked '| shasum -a 256 -c -' verification")

# Build-local-artifacts: Build artifacts step has shell: bash
build_art_step = get_step_by_name(local_job, 'Build artifacts')
if not build_art_step:
    fail("build-local-artifacts missing 'Build artifacts' step")
if build_art_step.get('shell') != 'bash':
    fail(f"'Build artifacts' step must have 'shell: bash', got {build_art_step.get('shell')!r}")

# --- 7. PowerShell syntax validation when pwsh is available ---
pwsh_path = shutil.which("pwsh")
has_pwsh_steps = any(
    (step.get("shell") == "pwsh" or (step.get("shell") is None and 'run' in step and ('windows' in str(j.get('runs-on', '')).lower() or '${{' in str(j.get('runs-on', '')))))
    for j in jobs.values()
    if isinstance(j, dict)
    for step in (j.get("steps") or [])
    if isinstance(step, dict)
)

if has_pwsh_steps:
    if pwsh_path:
        for jname, jdata in jobs.items():
            if not isinstance(jdata, dict):
                continue
            ro = str(jdata.get('runs-on', ''))
            for step in (jdata.get("steps") or []):
                if not isinstance(step, dict):
                    continue
                if step.get("shell") == "pwsh" or (step.get("shell") is None and 'run' in step and ('windows' in ro.lower() or '${{' in ro)):
                    sname = step.get("name")
                    if sname is not None and not isinstance(sname, str):
                        fail(f"Step in job '{jname}' has non-string 'name'")
                    sname = sname or 'unnamed'
                    run_code = step.get("run")
                    if run_code is not None and not isinstance(run_code, str):
                        fail(f"Step '{sname}' in job '{jname}' has non-string 'run'")
                    run_code = run_code or ""
                    parse_cmd = """
$code = [Console]::In.ReadToEnd()
$tokens = $null
$errors = $null
[System.Management.Automation.Language.Parser]::ParseInput($code, [ref]$tokens, [ref]$errors) | Out-Null
if ($errors.Count -gt 0) {
    foreach ($e in $errors) {
        [Console]::Error.WriteLine("ParserError: " + $e.Message)
    }
    exit 1
}
exit 0
"""
                    proc = subprocess.run(
                        [pwsh_path, "-NoProfile", "-NonInteractive", "-Command", parse_cmd],
                        input=run_code,
                        text=True,
                        capture_output=True,
                    )
                    if proc.returncode != 0:
                        fail(f"PowerShell parse error in step '{sname}' of job '{jname}': {proc.stderr.strip()}")
    else:
        ci_env = os.environ.get("CI")
        if ci_env and ci_env.lower() not in ("false", "0"):
            fail("pwsh required in CI to parse PowerShell steps")
        else:
            print("WARNING: pwsh not found on PATH; skipping PowerShell step syntax validation", file=sys.stderr)

# --- 8. Check publish-crates.yml if present ---
if publish_crates_path and os.path.isfile(publish_crates_path):
    try:
        with open(publish_crates_path, 'r', encoding='utf-8') as pf:
            p_data = yaml.safe_load(pf)
    except Exception as e:
        fail(f"Failed to parse YAML in {publish_crates_path}: {e}")

    if not isinstance(p_data, dict):
        fail(f"Top-level YAML in {publish_crates_path} is not a mapping")
    if p_data.get('permissions') != {'contents': 'read'}:
        fail(f"{publish_crates_path} top permissions must be 'contents: read'")
    p_jobs = p_data.get('jobs')
    if not isinstance(p_jobs, dict):
        fail(f"{publish_crates_path} 'jobs' section missing or not a mapping")
    p_pub = p_jobs.get('publish')
    if not isinstance(p_pub, dict):
        fail(f"{publish_crates_path} missing 'publish' job")
    if p_pub.get('environment') != 'release':
        fail(f"{publish_crates_path} publish job must declare 'environment: release'")
    for s in (p_pub.get('steps') or []):
        if isinstance(s, dict) and str(s.get('uses') or '').lower().startswith('actions/checkout@'):
            if not isinstance(s.get('with'), dict) or s['with'].get('persist-credentials') is not False:
                fail(f"{publish_crates_path} checkout must have persist-credentials: false")

    # Assert CARGO_REGISTRY_TOKEN appears only in the env of its two authorized steps
    for text, path in walk_elements(p_data):
        if 'CARGO_REGISTRY_TOKEN' in text:
            is_valid = False
            if len(path) >= 3 and str(path[0]) in ('on', 'True') and path[1] == 'workflow_call' and path[2] in ('secrets', 'key:secrets'):
                is_valid = True
            elif len(path) >= 5 and path[0] == 'jobs' and path[1] == 'publish' and path[2] == 'steps':
                step_idx = int(path[3]) if path[3].isdigit() else -1
                p_steps = p_pub.get('steps') or []
                if 0 <= step_idx < len(p_steps):
                    s = p_steps[step_idx]
                    sname = s.get('name') if isinstance(s, dict) else None
                    if sname in ('Validate registry token', 'Publish crates in dependency order'):
                        if (len(path) == 6 and path[4] == 'env' and path[5] in ('CARGO_REGISTRY_TOKEN', 'key:CARGO_REGISTRY_TOKEN')) or (len(path) == 5 and path[4] == 'run'):
                            is_valid = True
            if not is_valid:
                fail(f"CARGO_REGISTRY_TOKEN illegally referenced in {publish_crates_path} at {' -> '.join(path)}")

print(f"OK: {workflow_path} passed all release security and structural checks.")
PYEOF
