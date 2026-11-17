#!/bin/sh
# Checks that PACK_INPUT_HASH moves with the pack inputs and only with them.
set -eu

script=$(CDPATH="" cd "$(dirname "$0")" && pwd)/docling-pack-inputs.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
git init -q .
git config user.email test@example.test
git config user.name test
mkdir -p engines/docling/pack fixtures/pdf fixtures/scan .github/workflows scripts crates
for file in engines/docling/pack/build.py uv.lock LICENSE NOTICE fixtures/pdf/en-map-room.pdf \
    fixtures/scan/vi-garden-300dpi.png .github/workflows/pack-docling.yml scripts/docling-pack-inputs.sh; do
    echo "$file" >"$file"
done
echo base >crates/other.rs
git add . && git commit -q -m base
base=$(sh "$script" HEAD)

failed=0
expect() {
    # expect <same|different> <description>
    now=$(sh "$script" HEAD)
    if [ "$1" = same ] && [ "$now" = "$base" ]; then
        echo "PASS | $2"
    elif [ "$1" = different ] && [ "$now" != "$base" ]; then
        echo "PASS | $2"
    else
        echo "FAIL | $2 (expected $1)" >&2
        failed=1
    fi
    git reset -q --hard "$base_commit"
}
base_commit=$(git rev-parse HEAD)

change() {
    echo changed >>"$1"
    git add "$1" && git commit -q -m change
}

change crates/other.rs && expect same "an unrelated Rust file keeps the hash"
change README.md && expect same "a prose file keeps the hash"
change engines/docling/pack/build.py && expect different "a pack script changes the hash"
change uv.lock && expect different "the lock file changes the hash"
change fixtures/pdf/en-map-room.pdf && expect different "a proof fixture changes the hash"
change .github/workflows/pack-docling.yml && expect different "the pack workflow changes the hash"
change scripts/docling-pack-inputs.sh && expect different "this script changes the hash"

# The hash of an older revision does not need a checkout of it.
change engines/docling/pack/build.py
if [ "$(sh "$script" "$base_commit")" = "$base" ]; then
    echo "PASS | an older revision hashes from its tree"
else
    echo "FAIL | older revision hash" >&2
    failed=1
fi

exit "$failed"
