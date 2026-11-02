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
if bash "$repo/engines/qpdf/pack/check-release.sh" >/dev/null 2>&1; then
    echo 'pending approval was accepted' >&2; exit 1
fi
cp "$repo/engines/qpdf/pack/check-release.sh" "$tmp/check-release.sh"
cp "$repo/engines/qpdf/pack/licence-review.toml" "$tmp/licence-review.toml"
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('pending_owner_approval = true', 'pending_owner_approval = false'))
PY
if bash "$tmp/check-release.sh" >/dev/null 2>&1; then
    echo 'incomplete runtime review was accepted' >&2; exit 1
fi
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('runtime_review_complete = false', 'runtime_review_complete = true'))
PY
bash "$tmp/check-release.sh"
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('version = "3.2.1"', 'version = "3.2.0"'))
PY
if bash "$tmp/check-release.sh" >/dev/null 2>&1; then
    echo 'mismatched component review was accepted' >&2; exit 1
fi
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
