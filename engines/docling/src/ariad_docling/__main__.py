"""Run the engine: ``python -I -m ariad_docling``.

Protocol lines go to a duplicate of the original stdout. File descriptor 1 is then pointed at
stderr before anything else is imported, so native libraries and stray prints that write to
stdout cannot corrupt the protocol stream.
"""

import os
import sys

_PROTOCOL_FD = os.dup(1)
os.dup2(2, 1)
sys.stdout = sys.stderr

from ariad_docling.engine import main  # noqa: E402

raise SystemExit(main(_PROTOCOL_FD, sys.stdin.buffer))
