"""Determinism test for benchmark harness.

Asserts that two benchmark runs over identical inputs yield byte-identical
fidelity and editability scores.
"""

from __future__ import annotations

import os
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
            )

            cap2 = run_benchmark(
                root=root,
                ashift_bin=ashift_bin,
                pandoc_bin=pandoc_bin,
                edges_filter=test_edges,
                fixtures_filter=test_fixtures,
                repeat_count=1,
                out_file=out2,
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
