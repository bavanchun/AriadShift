#!/usr/bin/env python3
"""Keep package-oracle unit tests in the automatic and local fuzz gates."""

from pathlib import Path
import re
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]
UNIT_COMMAND = "cargo test --locked --manifest-path fuzz/Cargo.toml --bin docx_meta"
GUARD_COMMAND = (
    "uv run --no-project --with pyyaml==6.0.3 python scripts/test-fuzz-gate.py"
)


class FuzzGateTests(unittest.TestCase):
    def test_ci_runs_units_unconditionally_before_campaigns(self):
        workflow = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text())
        job = workflow["jobs"]["fuzz"]
        steps = job["steps"]
        unit_steps = [
            i for i, step in enumerate(steps)
            if step.get("run", "").strip() == UNIT_COMMAND
        ]
        self.assertEqual(len(unit_steps), 1, "CI fuzz gate must run the oracle unit suite")
        unit_index = unit_steps[0]
        unit_step = steps[unit_index]
        self.assertNotIn("if", unit_step)
        self.assertFalse(unit_step.get("continue-on-error", False))
        self.assertFalse(job.get("continue-on-error", False))
        campaign_index = next(
            i for i, step in enumerate(steps)
            if step.get("name") == "Run fuzz targets"
        )
        self.assertLess(unit_index, campaign_index)
        campaign = steps[campaign_index]["run"]
        for required in [
            "cargo fuzz run", '"fuzz/corpus/$t"', '"fuzz/seeds/$t"',
            "-max_total_time=60",
        ]:
            self.assertIn(required, campaign)

    def test_local_fuzz_runs_units_before_selecting_targets(self):
        justfile = (ROOT / "justfile").read_text()
        recipe = re.search(
            r"(?ms)^\[linux\]\nfuzz [^\n]+:\n(.*?)(?=^\[macos\])", justfile
        )
        self.assertIsNotNone(recipe)
        lines = [line.strip() for line in recipe.group(1).splitlines()]
        self.assertIn(UNIT_COMMAND, lines, "local fuzz gate must run the oracle unit suite")
        index = lines.index(UNIT_COMMAND)
        self.assertLess(lines.index("set -euo pipefail"), index)
        selection = next(i for i, line in enumerate(lines) if line.startswith("if "))
        self.assertLess(index, selection)
        self.assertIn("cargo fuzz run", recipe.group(1))
        self.assertIn('"fuzz/seeds/$t"', recipe.group(1))

    def test_gate_guard_runs_in_static_checks(self):
        justfile = (ROOT / "justfile").read_text()
        recipe = re.search(r"(?ms)^test-ci-scope:\n(.*?)(?=^\S)", justfile)
        self.assertIsNotNone(recipe)
        self.assertIn(GUARD_COMMAND, [line.strip() for line in recipe.group(1).splitlines()])
        static = next(line for line in justfile.splitlines() if line.startswith("static:"))
        self.assertIn("test-ci-scope", static.split())


if __name__ == "__main__":
    unittest.main(verbosity=2)
