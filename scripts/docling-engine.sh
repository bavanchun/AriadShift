#!/usr/bin/env bash
# The docling engine's development environment, lint, tests and CLI engine tests. The justfile
# recipes `docling-dev`, `docling-test` and `docling-engine-test` and pack-docling.yml both call
# this script, so the workflow depends on these steps and not on the whole justfile.
#   dev          sync the engine environment and fetch its pinned models and OCR data (~1.6 GiB)
#   test         ruff check, ruff format --check and pytest for engines/docling
#   engine-test  CLI tests that drive the real engine (needs `dev` and Tesseract)
set -euo pipefail

cd "$(dirname "$0")/.."
root=$PWD/.tools/docling
export UV_PROJECT_ENVIRONMENT=$root/python-env

# The environment's interpreter: bin/ on Linux and macOS, Scripts/ on Windows.
env_python() {
    if [[ -x "$UV_PROJECT_ENVIRONMENT/bin/python" ]]; then
        echo "$UV_PROJECT_ENVIRONMENT/bin/python"
    else
        echo "$UV_PROJECT_ENVIRONMENT/Scripts/python.exe"
    fi
}

case "${1:-}" in
    dev)
        uv sync --package ariad-docling --frozen
        "$(env_python)" engines/docling/pack/fetch.py assets --dest "$root"
        ;;
    test)
        uv run --package ariad-docling --frozen ruff check engines/docling
        uv run --package ariad-docling --frozen ruff format --check engines/docling
        uv run --package ariad-docling --frozen pytest engines/docling/tests
        ;;
    engine-test)
        ASHIFT_DOCLING_ENGINE="\"$(env_python)\" -I -m ariad_docling" \
            cargo nextest run -p ariad-cli --features ariad-host/test-probe --run-ignored ignored-only \
            --test-threads 2 -E 'binary(it) & test(/^docling_engine::/)'
        ;;
    *)
        echo "usage: docling-engine.sh dev|test|engine-test" >&2
        exit 2
        ;;
esac
