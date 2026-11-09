#!/usr/bin/env bash
# Test veraPDF pack builder, pins, notices, and release verification.
set -euo pipefail
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
script=$repo/engines/verapdf/pack/make.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

printf 'invalid archive\n' >"$tmp/bad"
if bash "$script" verify-archive mac-arm64 "$tmp/bad" >/dev/null 2>&1; then
    echo 'hash mismatch was accepted' >&2; exit 1
fi
if bash "$script" verify-archive invalid "$tmp/bad" >/dev/null 2>&1; then
    echo 'unknown target was accepted' >&2; exit 1
fi

# Verify targets.txt structure and non-empty entries
while read -r slug triple asset sha; do
    case $slug in ''|'#'*) continue ;; esac
    [ -n "$triple" ] || { echo "missing triple for $slug" >&2; exit 1; }
    [ -n "$asset" ] || { echo "missing asset for $slug" >&2; exit 1; }
    [[ "$sha" =~ ^[0-9a-f]{64}$ ]] || { echo "invalid sha256 for $slug" >&2; exit 1; }
done < "$repo/engines/verapdf/pack/targets.txt"

# Verify licence review file
python3 - "$repo/engines/verapdf/pack/licence-review.toml" <<'PY'
import pathlib, sys, tomllib
review = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
assert review['pack'] == 'verapdf'
assert review['version'] == '0.1.0'
assert 'MPL-2.0' in review['license']
assert 'GPL' not in review['license']
names = {c['name'] for c in review['components']}
assert 'verapdf' in names
assert 'eclipse-temurin-jdk' not in names
PY

# Verify notice files exist and are non-empty
notices_dir="$repo/engines/verapdf/pack/notices"
required_notices=(
    THIRD_PARTY_NOTICES.txt
    apache-LICENSE-2.0.txt
    jakarta-EDL-1.0.txt
    jakarta-EPL-2.0.txt
    rhino-NOTICE.txt
    verapdf-MPL-2.0.txt
)

for notice in "${required_notices[@]}"; do
    [ -s "$notices_dir/$notice" ] || { echo "missing or empty notice: $notice" >&2; exit 1; }
done

# If a test pack is provided, run make.sh verify and check tampering detection
if [ -n "${VERAPDF_TEST_PACK:-}" ]; then
    bash "$script" verify "$VERAPDF_TEST_PACK"
    cp -R "$VERAPDF_TEST_PACK" "$tmp/pack"
    for notice in verapdf-MPL-2.0.txt apache-LICENSE-2.0.txt THIRD_PARTY_NOTICES.txt; do
        mv "$tmp/pack/notices/$notice" "$tmp/notice"
        if bash "$script" verify "$tmp/pack" >/dev/null 2>&1; then
            echo "missing notice $notice was accepted" >&2; exit 1
        fi
        mv "$tmp/notice" "$tmp/pack/notices/$notice"
    done
fi

echo 'veraPDF pack checks passed'
