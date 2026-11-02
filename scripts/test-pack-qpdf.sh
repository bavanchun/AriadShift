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
sed -i 's/pending_owner_approval = true/pending_owner_approval = false/' "$tmp/licence-review.toml"
bash "$tmp/check-release.sh"
sed -i 's/version = "3.2.1"/version = "3.2.0"/' "$tmp/licence-review.toml"
if bash "$tmp/check-release.sh" >/dev/null 2>&1; then
    echo 'mismatched component review was accepted' >&2; exit 1
fi
if [ -n "${QPDF_MAC_ARM64_ARCHIVE:-}" ]; then bash "$script" verify-archive mac-arm64 "$QPDF_MAC_ARM64_ARCHIVE"; fi
if [ -n "${QPDF_MAC_X64_ARCHIVE:-}" ]; then bash "$script" verify-archive mac-x64 "$QPDF_MAC_X64_ARCHIVE"; fi
echo 'qpdf pack checks passed'
