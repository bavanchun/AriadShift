"""Determinism test for benchmark harness.

Asserts that two benchmark runs over identical inputs yield byte-identical
fidelity and editability scores.
"""

from __future__ import annotations

import os
import json
from pathlib import Path
import tempfile
import unittest

from ariad_bench.run import find_binary, run_benchmark


class TestDeterminism(unittest.TestCase):
    def test_fidelity_and_editability_determinism(self) -> None:
        root = Path(__file__).resolve().parents[2]
        ashift_bin = find_binary(
            "ashift",
            None,
            [root / "target" / "release" / "ashift", root / "target" / "debug" / "ashift"],
        )
        pandoc_bin = find_binary(
            "pandoc",
            os.environ.get("ASHIFT_PANDOC"),
            [root / ".tools" / "pandoc" / "bin" / "pandoc"],
        )

        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp = Path(tmp_dir)
            out1 = tmp / "cap1.json"
            out2 = tmp / "cap2.json"

            # Benchmark reader edge and writer edge
            test_edges = ["html->ariad-ir+json", "ariad-ir+json->html"]
            test_fixtures = ["vi-garden-notice", "vi-styled-report"]

            cap1 = run_benchmark(
                root=root,
                ashift_bin=ashift_bin,
                pandoc_bin=pandoc_bin,
                edges_filter=test_edges,
                fixtures_filter=test_fixtures,
                repeat_count=1,
                out_file=out1,
                fidelity_out=tmp / "fidelity1.json",
            )

            cap2 = run_benchmark(
                root=root,
                ashift_bin=ashift_bin,
                pandoc_bin=pandoc_bin,
                edges_filter=test_edges,
                fixtures_filter=test_fixtures,
                repeat_count=1,
                out_file=out2,
                fidelity_out=tmp / "fidelity2.json",
            )

            evidence1 = json.loads((tmp / "fidelity1.json").read_text())
            evidence2 = json.loads((tmp / "fidelity2.json").read_text())
            self.assertEqual(evidence1["metric_version"], "fidelity/2")
            self.assertEqual(evidence1["basis"], "independent_reference")
            self.assertTrue(evidence1["samples"])
            self.assertEqual(
                [sample["fidelity"] for sample in evidence1["samples"]],
                [sample["fidelity"] for sample in evidence2["samples"]],
            )

            edges1 = {
                f"{e['from']}->{e['to']}": e["metrics"]
                for e in cap1.get("edges", [])
                if e.get("metrics") is not None
            }
            edges2 = {
                f"{e['from']}->{e['to']}": e["metrics"]
                for e in cap2.get("edges", [])
                if e.get("metrics") is not None
            }

            self.assertEqual(set(edges1.keys()), set(edges2.keys()))
            for edge_name in edges1:
                m1 = edges1[edge_name]
                m2 = edges2[edge_name]
                self.assertEqual(
                    m1["fidelity"],
                    m2["fidelity"],
                    f"Fidelity diverged across runs for {edge_name}",
                )
                self.assertEqual(
                    m1["editability"],
                    m2["editability"],
                    f"Editability diverged across runs for {edge_name}",
                )
                self.assertEqual(
                    m1["samples"],
                    m2["samples"],
                    f"Sample count diverged across runs for {edge_name}",
                )


if __name__ == "__main__":
    unittest.main()
