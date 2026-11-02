#!/usr/bin/env bash
# Fail closed unless the exact component review explicitly records approval.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
python3 - "$here/licence-review.toml" <<'PY'
import pathlib, sys, tomllib
p = pathlib.Path(sys.argv[1])
try:
    review = tomllib.loads(p.read_text())
    expected = {
        'pack': 'qpdf', 'component': 'libjpeg-turbo', 'version': '3.2.1',
        'modified': False, 'license': 'IJG AND BSD-3-Clause AND Zlib',
        'source': 'https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/3.2.1/libjpeg-turbo-3.2.1.tar.gz',
        'sha256': 'c27ad72a68da151177929aa969ae773a2da5ac2af9ce946079314fdac8cece7f',
        'pending_owner_approval': False,
    }
    if review != expected:
        raise ValueError('review is pending or does not match the approved component')
except (OSError, ValueError, tomllib.TOMLDecodeError) as exc:
    sys.exit(f'qpdf release refused: {exc}')
PY
