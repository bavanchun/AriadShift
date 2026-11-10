"""Make the isolated engine source available to pytest and protocol children."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
