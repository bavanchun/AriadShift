"""Isolate stdout and serve one offline engine request."""

import os
import sys

_PROTOCOL_FD = os.dup(int(os.environ.get("ARIAD_PROTOCOL_FD", "1")))
os.dup2(2, 1)
sys.stdout = sys.stderr

from ariad_inpaint.engine import main  # noqa: E402

raise SystemExit(main(_PROTOCOL_FD, sys.stdin.buffer))
