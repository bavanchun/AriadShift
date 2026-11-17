#!/bin/sh
# Prints PACK_INPUT_HASH: a SHA-256 over every file the Docling pack archives are built from,
# read from the git tree of a revision (default HEAD), so any commit can be hashed without a
# checkout. A pack built from one commit can stand in for another commit exactly when the two
# hashes are equal. `--list` prints the hashed tree entries instead. See docs/ci.md ("Cloud E2E pack dependency").
#
# The set: the engine and pack scripts (engines/docling), the lock file that pins the wheels,
# the licence files copied into the pack notices, the fixtures the build converts for its proof
# and checks, the pack workflow, and this script (so changing the set forces a rebuild).
# The workspace version is not an input: the E2E job regenerates the manifest with the current
# ashift, which re-checks `requires_ashift`.
set -eu

list=false
if [ "${1:-}" = --list ]; then
    list=true
    shift
fi
rev=${1:-HEAD}

entries() {
    git ls-tree -r "$rev" -- \
        engines/docling \
        uv.lock \
        LICENSE \
        NOTICE \
        fixtures/pdf/en-map-room.pdf \
        fixtures/scan/vi-garden-300dpi.png \
        .github/workflows/pack-docling.yml \
        scripts/docling-pack-inputs.sh
}

if $list; then
    entries
else
    entries | sha256sum | cut -d' ' -f1
fi
