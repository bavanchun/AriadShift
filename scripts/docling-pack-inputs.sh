#!/usr/bin/env bash
# Prints PACK_INPUT_HASH: a SHA-256 over every file the Docling pack archives are built from,
# read from the git tree of a revision (default HEAD), so any commit can be hashed without a
# checkout. Two commits with equal hashes build the same pack. Options:
#   --list   print the hashed tree entries instead of their hash
#   --paths  print the input paths (the pack workflow's path filters must cover them)
# Fails closed: an unknown revision or an empty input set exits non-zero with no output.
# See docs/ci.md ("Cloud E2E pack dependency") for what is and is not an input.
set -euo pipefail

# The engine and pack scripts, the shared licence code the pack scripts execute, the lock and
# project files that pin the wheels, the justfile and Pandoc installer the pack job runs, the
# licence files copied into the pack notices, the fixtures the pack job converts, the pack
# workflow, and this script (so changing the set forces a rebuild).
inputs=(
    engines/docling
    engines/common/licenses
    uv.lock
    pyproject.toml
    justfile
    scripts/install-pandoc.sh
    LICENSE
    NOTICE
    fixtures/pdf/en-map-room.pdf
    fixtures/scan/vi-garden-300dpi.png
    .github/workflows/pack-docling.yml
    scripts/docling-pack-inputs.sh
)

mode=digest
case "${1:-}" in
    --list) mode=list; shift ;;
    --paths) printf '%s\n' "${inputs[@]}"; exit 0 ;;
esac
rev=${1:-HEAD}

cd "$(git rev-parse --show-toplevel)"
git rev-parse --verify --quiet "$rev^{commit}" >/dev/null ||
    { echo "docling-pack-inputs: unknown revision: $rev" >&2; exit 1; }
listing=$(git ls-tree -r "$rev" -- "${inputs[@]}")
[[ -n "$listing" ]] || { echo "docling-pack-inputs: no pack inputs in $rev" >&2; exit 1; }

if [[ "$mode" == list ]]; then
    printf '%s\n' "$listing"
else
    printf '%s\n' "$listing" | sha256sum | cut -d' ' -f1
fi
