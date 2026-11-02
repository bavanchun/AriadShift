#!/usr/bin/env bash
set -euo pipefail
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
script=$repo/engines/qpdf/pack/make.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf 'invalid archive\n' >"$tmp/bad"
if bash "$script" verify-archive mac-arm64 "$tmp/bad" >/dev/null 2>&1; then
    echo 'hash mismatch was accepted' >&2; exit 1
fi
if bash "$script" verify-archive invalid "$tmp/bad" >/dev/null 2>&1; then
    echo 'unknown target was accepted' >&2; exit 1
fi
# The PR completing the GNU runtime review must flip or remove this assertion.
if bash "$repo/engines/qpdf/pack/check-release.sh" >/dev/null 2>&1; then
    echo 'committed incomplete runtime review was accepted' >&2; exit 1
fi
cp "$repo/engines/qpdf/pack/check-release.sh" "$tmp/check-release.sh"
python3 - "$repo/engines/qpdf/pack/licence-review.toml" "$tmp" <<'PYTEST'
import pathlib, subprocess, sys, tomllib

source, directory = map(pathlib.Path, sys.argv[1:])
review_path = directory / 'licence-review.toml'
original = source.read_text()

def check(text, accepted, label):
    review_path.write_text(text)
    result = subprocess.run(['bash', str(directory / 'check-release.sh')], capture_output=True)
    if (result.returncode == 0) != accepted:
        sys.exit(f'{label}: unexpected release gate result')

approved = original.replace('pending_owner_approval = true', 'pending_owner_approval = false')
approved = approved.replace('runtime_review_complete = false', 'runtime_review_complete = true')
check(approved.replace('runtime_review_complete = true', 'runtime_review_complete = false'), False, 'runtime pending')
check(approved.replace('pending_owner_approval = false', 'pending_owner_approval = true'), False, 'owner approval pending')
check(approved, True, 'both reviews complete')
for key, value in tomllib.loads(approved).items():
    lines = approved.splitlines(keepends=True)
    replacement = 'true' if value is False else 'false' if value is True else '"mismatch"'
    changed = ''.join(f'{key} = {replacement}\n' if line.startswith(f'{key} = ') else line for line in lines)
    check(changed, False, f'mismatched {key}')
PYTEST
if [ -n "${QPDF_SOURCE_ARCHIVE:-}" ]; then bash "$script" verify-archive mac-arm64 "$QPDF_SOURCE_ARCHIVE"; fi
if [ -n "${QPDF_TEST_PACK:-}" ]; then
    bash "$script" verify "$QPDF_TEST_PACK"
    cp -R "$QPDF_TEST_PACK" "$tmp/pack"
    rm "$tmp/pack/notices/qpdf-LICENSE.txt"
    if bash "$script" verify "$tmp/pack" >/dev/null 2>&1; then
        echo 'missing notice was accepted' >&2; exit 1
    fi
fi
echo 'qpdf pack checks passed'
