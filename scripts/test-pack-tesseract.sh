#!/usr/bin/env bash
# Check source pin rejection and bundled notices without network or compilation.
set -euo pipefail
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf '%s\n' 'not a source archive' >"$work/wrong.tar.gz"
if bash "$repo/engines/tesseract/pack/compile-tesseract.sh" \
    "$work/wrong.tar.gz" "$work/wrong.tar.gz" "$work/out" >"$work/log" 2>&1; then
    echo 'incorrect source hash was accepted' >&2
    exit 1
fi
grep -q 'hash mismatch:' "$work/log"
[ ! -e "$work/out/bin/tesseract" ]
checker="$repo/engines/tesseract/pack/check-linux-dependencies.sh"
printf '%s\n' ' (NEEDED) Shared library: [ld-linux-aarch64.so.1]' | bash "$checker" aarch64
if printf '%s\n' ' (NEEDED) Shared library: [ld-linux-aarch64.so.1]' | bash "$checker" x86_64; then
    echo 'foreign loader was accepted' >&2; exit 1
fi
if printf '%s\n' ' (NEEDED) Shared library: [libunexpected.so]' | bash "$checker" aarch64; then
    echo 'unexpected dependency was accepted' >&2; exit 1
fi
for notice in tesseract-LICENSE.txt leptonica-LICENSE.txt tessdata-LICENSE.txt; do
    [ -s "$repo/engines/tesseract/pack/notices/$notice" ] || {
        echo "missing notice: $notice" >&2
        exit 1
    }
done
if [ -n "${TESSERACT_TEST_PACK:-}" ]; then
    bash "$repo/engines/tesseract/pack/make.sh" verify "$TESSERACT_TEST_PACK"
    cp -R "$TESSERACT_TEST_PACK" "$work/pack"
    printf '%s\n' 'changed' >"$work/pack/tessdata/vie.traineddata"
    if bash "$repo/engines/tesseract/pack/make.sh" verify "$work/pack" >"$work/tamper-log" 2>&1; then
        echo 'changed model was accepted' >&2
        exit 1
    fi
    grep -q 'model hash mismatch' "$work/tamper-log"
    rm "$work/pack/notices/leptonica-LICENSE.txt"
    if bash "$repo/engines/tesseract/pack/make.sh" verify "$work/pack" >"$work/notice-log" 2>&1; then
        echo 'missing notice was accepted' >&2
        exit 1
    fi
    grep -q 'missing notice:' "$work/notice-log"
fi
echo 'Tesseract source pin and notice checks passed.'
