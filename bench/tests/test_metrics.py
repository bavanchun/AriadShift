"""Unit tests with hand-computed expected values for benchmark metrics and TEDS."""

from __future__ import annotations

import sys
import unittest
from unittest.mock import MagicMock, patch

from ariad_bench.canonical import (
    CanonicalDoc,
    HeadingNode,
    ListStructure,
    TableCell,
    TableGrid,
)
from ariad_bench.metrics import (
    _is_post_exec,
    aggregate_edge_metrics,
    compute_editability,
    compute_heading_ted,
    compute_text_cer,
    measure_execution,
    score_fixture,
)
from ariad_bench.teds import (
    MAX_CELLS_PER_TABLE,
    compute_document_teds,
    compute_table_teds,
)


class TestMetrics(unittest.TestCase):
    def test_text_cer_hand_computed(self) -> None:
        """Verify Character Error Rate on known hand-computed cases."""
        self.assertEqual(compute_text_cer("abcd", "abcd"), 0.0)
        # 1 substitution out of 4 characters: 1/4 = 0.25
        self.assertAlmostEqual(compute_text_cer("abcd", "abed"), 0.25)
        # Both empty
        self.assertEqual(compute_text_cer("", ""), 0.0)
        # Empty reference with non-empty candidate
        self.assertEqual(compute_text_cer("", "abc"), 1.0)

    def test_heading_ted_hand_computed(self) -> None:
        """Verify heading tree edit distance on hand-computed tree structures."""
        # 1. Identical trees
        h_ref = [HeadingNode(level=1, text="Main", children=[HeadingNode(level=2, text="Sub")])]
        h_hyp = [HeadingNode(level=1, text="Main", children=[HeadingNode(level=2, text="Sub")])]
        self.assertEqual(compute_heading_ted(h_ref, h_hyp), 0.0)

        # 2. Deletion of child heading
        # Ref tree has: 2 real headings (Main, Sub)
        # Hyp tree has: 1 real heading (Main)
        # Edit operations: delete Sub = 1 deletion
        # Max real headings = 2
        # Expected normalized TED = 1 / 2 = 0.5
        h_hyp_deleted = [HeadingNode(level=1, text="Main")]
        self.assertAlmostEqual(compute_heading_ted(h_ref, h_hyp_deleted), 0.5)

        # 3. Complete loss of headings gives 1.0
        self.assertEqual(compute_heading_ted(h_ref, []), 1.0)
        self.assertEqual(compute_heading_ted([], h_ref), 1.0)

        # 4. Both empty heading lists
        self.assertEqual(compute_heading_ted([], []), 0.0)

    def test_teds_hand_computed(self) -> None:
        """Verify table tree edit distance on hand-computed tables."""
        # 1. Identical 1x1 table
        # Tree has: table (1), tr (1), td (1), text:Apple (1) = 4 nodes
        t_ref = TableGrid(rows=[[TableCell(text="Apple")]])
        t_hyp = TableGrid(rows=[[TableCell(text="Apple")]])
        score, skipped = compute_table_teds(t_ref, t_hyp)
        self.assertFalse(skipped)
        self.assertEqual(score, 1.0)

        # 2. Cell text edit: 'Apple' -> 'Appel'
        # Levenshtein distance: 2 edits out of 5 chars = 0.4
        # Tree edit distance = 0.4
        # Max nodes = 4
        # Expected TEDS = 1 - 0.4/4 = 1 - 0.1 = 0.90
        t_hyp_typo = TableGrid(rows=[[TableCell(text="Appel")]])
        score, skipped = compute_table_teds(t_ref, t_hyp_typo)
        self.assertFalse(skipped)
        self.assertIsNotNone(score)
        self.assertAlmostEqual(score, 0.90, places=4)

        # 3. Structural span change: colspan 2 -> colspan 1
        # Ref has tag td with colspan 2, Hyp has tag td with colspan 1
        # Tag/span rename cost = 1.0
        # Tree edit distance = 1.0
        # Max nodes = 4
        # Expected TEDS = 1 - 1.0/4 = 0.75
        t_ref_span = TableGrid(rows=[[TableCell(text="Apple", colspan=2)]])
        score, skipped = compute_table_teds(t_ref_span, t_hyp)
        self.assertFalse(skipped)
        self.assertIsNotNone(score)
        self.assertAlmostEqual(score, 0.75, places=4)

        # 4. Zhong et al. 2019 test: [[a, b]] vs [[a]]
        # Ref: table -> tbody -> tr -> td(a), td(b) = 5 nodes
        # Hyp: table -> tbody -> tr -> td(a) = 4 nodes
        # Edit operations: delete td(b) = 1 deletion
        # Max nodes = 5
        # Expected TEDS = 1 - 1/5 = 0.8
        t_ab = TableGrid(rows=[[TableCell(text="a"), TableCell(text="b")]])
        t_a = TableGrid(rows=[[TableCell(text="a")]])
        score, skipped = compute_table_teds(t_ab, t_a)
        self.assertFalse(skipped)
        self.assertIsNotNone(score)
        self.assertAlmostEqual(score, 0.8, places=4)

    def test_teds_500_cell_cap(self) -> None:
        """Verify that tables exceeding 500 cells are skipped and recorded."""
        # Create a table with 501 cells (e.g. 501 rows of 1 cell)
        huge_rows = [[TableCell(text=f"c{i}")] for i in range(MAX_CELLS_PER_TABLE + 1)]
        huge_table = TableGrid(rows=huge_rows)
        small_table = TableGrid(rows=[[TableCell(text="A")]])

        score, skipped = compute_table_teds(huge_table, small_table)
        self.assertTrue(skipped)
        self.assertIsNone(score)

        avg_score, skipped_count = compute_document_teds([huge_table], [small_table])
        self.assertEqual(skipped_count, 1)
        self.assertIsNone(avg_score)

    def test_editability_hand_computed(self) -> None:
        """Verify editability computation: surviving share of headings, lists, and tables."""
        # Reference has: 2 headings, 1 list, 1 table -> Total 4 structural items
        ref = CanonicalDoc(
            headings=[HeadingNode(level=1, text="H1"), HeadingNode(level=2, text="H2")],
            lists=[ListStructure(items=["i1", "i2"])],
            tables=[TableGrid(rows=[[TableCell(text="T")]])],
        )

        # Candidate has: 1 heading, 1 list, 0 tables
        # Survived: min(2,1)=1 heading + min(1,1)=1 list + min(1,0)=0 table = 2
        # Expected editability: 2 / 4 = 0.5
        hyp = CanonicalDoc(
            headings=[HeadingNode(level=1, text="H1")],
            lists=[ListStructure(items=["i1", "i2"])],
            tables=[],
        )

        self.assertAlmostEqual(compute_editability(ref, hyp), 0.5)

    def test_fidelity_formula(self) -> None:
        """Verify fidelity is the mean of (1 - text_cer), (1 - heading_ted), and teds."""
        # Doc without tables: fidelity is mean of text and headings
        ref_no_tbl = CanonicalDoc(
            text="abcd",
            headings=[HeadingNode(level=1, text="Main")],
        )
        hyp_no_tbl = CanonicalDoc(
            text="abed",  # cer = 0.25 -> 1 - cer = 0.75
            headings=[HeadingNode(level=1, text="Main")],  # h_ted = 0.0 -> 1 - ted = 1.0
        )
        score = score_fixture(ref_no_tbl, hyp_no_tbl)
        self.assertIsNone(score.teds)
        # Expected fidelity: (0.75 + 1.0) / 2 = 0.875
        self.assertAlmostEqual(score.fidelity, 0.875)

    def test_edge_aggregation_rounding(self) -> None:
        """Verify aggregation and rounding constraints:

        - scores rounded to 3 decimals
        - p50_ms rounded to 10 ms
        - peak_mem_mb rounded to 1 MB
        """
        ref = CanonicalDoc(text="hello")
        hyp = CanonicalDoc(text="hello")

        # 3 repeats with noisy timings and memory
        s1 = score_fixture(ref, hyp, wall_ms=23.4, peak_rss_bytes=14_800_000)
        s2 = score_fixture(ref, hyp, wall_ms=27.1, peak_rss_bytes=15_800_000)
        s3 = score_fixture(ref, hyp, wall_ms=31.8, peak_rss_bytes=14_900_000)

        edge = aggregate_edge_metrics([s1, s2, s3])
        self.assertEqual(edge.samples, 3)
        self.assertEqual(edge.fidelity, 1.0)
        self.assertEqual(edge.editability, 1.0)
        # Median wall time: 27.1 ms -> rounded to nearest 10 ms = 30.0 ms
        self.assertEqual(edge.p50_ms, 30.0)
        # Peak memory: 15.2 MB -> rounded to 1 MB = 15.0 MB
        self.assertEqual(edge.peak_mem_mb, 15.0)

    def test_teds_normalization_uses_max_not_min(self) -> None:
        """Verify TEDS normalizes by max(nodes_ref, nodes_hyp), not min."""
        t1 = TableGrid(rows=[[TableCell(text="a")]])
        t5 = TableGrid(rows=[[TableCell(text=f"c{i}") for i in range(5)]])
        score, _ = compute_table_teds(t1, t5)
        self.assertIsNotNone(score)
        # With min(nodes_ref, nodes_hyp) == 4, TED=5.0 gives 1 - 5/4 = -0.25 (clamped to 0.0).
        # With max(nodes_ref, nodes_hyp) == 8, TED=5.0 gives 1 - 5/8 = 0.375.
        self.assertEqual(score, 0.375)

    def test_teds_distinguishes_th_and_td(self) -> None:
        """Verify TEDS distinguishes header cell (th) from data cell (td)."""
        t_header = TableGrid(rows=[[TableCell(text="Title", is_header=True)]])
        t_data = TableGrid(rows=[[TableCell(text="Title", is_header=False)]])
        t_ref = TableGrid(rows=[[TableCell(text="Title", is_header=True)]])

        score_identical, _ = compute_table_teds(t_ref, t_header)
        score_diff_tag, _ = compute_table_teds(t_ref, t_data)
        self.assertEqual(score_identical, 1.0)
        # Header cell is thead -> tr -> th, data cell is tbody -> tr -> td (2 edits out of 4 nodes: 0.5)
        self.assertEqual(score_diff_tag, 0.5)

    def test_fidelity_includes_teds_when_tables_present(self) -> None:
        """Verify fidelity incorporates table TEDS in the 3-term average."""
        ref = CanonicalDoc(
            text="hello",
            headings=[HeadingNode(level=1, text="H")],
            tables=[TableGrid(rows=[[TableCell(text="A")]])],
        )
        hyp = CanonicalDoc(
            text="hello",
            headings=[HeadingNode(level=1, text="H")],
            tables=[TableGrid(rows=[[TableCell(text="B")]])],
        )
        score = score_fixture(ref, hyp)
        self.assertIsNotNone(score.teds)
        self.assertAlmostEqual(score.fidelity, 2.75 / 3.0, places=4)
        self.assertNotEqual(score.fidelity, 1.0)

    def test_editability_table_overlap_threshold(self) -> None:
        """Verify that tables with under 50% cell overlap do not count as survived."""
        ref = CanonicalDoc(
            tables=[TableGrid(rows=[[TableCell(text="a"), TableCell(text="b"), TableCell(text="c")]])],
        )
        # 1 match out of 3 cells = 33% overlap (< 50% threshold)
        hyp_sub_threshold = CanonicalDoc(
            tables=[TableGrid(rows=[[TableCell(text="a"), TableCell(text="x"), TableCell(text="y")]])],
        )
        self.assertEqual(compute_editability(ref, hyp_sub_threshold), 0.0)

        # 2 matches out of 3 cells = 67% overlap (>= 50% threshold)
        hyp_above_threshold = CanonicalDoc(
            tables=[TableGrid(rows=[[TableCell(text="a"), TableCell(text="b"), TableCell(text="z")]])],
        )
        self.assertEqual(compute_editability(ref, hyp_above_threshold), 1.0)

    def test_latency_aggregation_uses_median_not_max_or_mean(self) -> None:
        """Verify edge p50_ms uses median of wall times, not max or mean."""
        ref = CanonicalDoc(text="x")
        hyp = CanonicalDoc(text="x")
        s1 = score_fixture(ref, hyp, wall_ms=10.0)
        s2 = score_fixture(ref, hyp, wall_ms=20.0)
        s3 = score_fixture(ref, hyp, wall_ms=120.0)
        edge = aggregate_edge_metrics([s1, s2, s3])
        self.assertEqual(edge.p50_ms, 20.0)

    def test_measure_execution_captures_child_rss(self) -> None:
        """Verify measure_execution captures child process memory in the process tree."""
        cmd = [
            sys.executable,
            "-c",
            "import subprocess, sys; p = subprocess.Popen([sys.executable, '-c', 'b = bytearray(40 * 1024 * 1024); import time; time.sleep(0.15)']); p.wait()",
        ]
        rc, stdout, stderr, wall_ms, peak_rss = measure_execution(cmd, sample_interval_s=0.005)
        self.assertEqual(rc, 0)
        self.assertIsNotNone(peak_rss)
        # Parent RSS is ~15 MB; child RSS adds 40 MB -> combined tree peak exceeds 35 MB.
        self.assertGreater(peak_rss, 35 * 1024 * 1024)

    def test_measure_execution_captures_thread_spawned_child_rss(self) -> None:
        """Verify measure_execution captures child process spawned from a non-main thread."""
        cmd = [
            sys.executable,
            "-c",
            (
                "import threading, subprocess, sys\n"
                "def run_child():\n"
                "    subprocess.run([sys.executable, '-c', 'b = bytearray(50 * 1024 * 1024); import time; time.sleep(0.15)'])\n"
                "t = threading.Thread(target=run_child)\n"
                "t.start()\n"
                "t.join()\n"
            ),
        ]
        rc, stdout, stderr, wall_ms, peak_rss = measure_execution(cmd, sample_interval_s=0.005)
        self.assertEqual(rc, 0)
        self.assertIsNotNone(peak_rss)
        # Parent RSS is ~15 MB; thread-spawned child allocates 50 MB -> combined tree peak exceeds 45 MB.
        self.assertGreater(peak_rss, 45 * 1024 * 1024)

    def test_measure_execution_footprint_discrimination(self) -> None:
        """Verify measure_execution clearly distinguishes different memory allocations."""
        cmd_small = [
            sys.executable,
            "-c",
            "import time; b = bytearray(4 * 1024 * 1024); time.sleep(0.15)",
        ]
        cmd_large = [
            sys.executable,
            "-c",
            "import time; b = bytearray(40 * 1024 * 1024); time.sleep(0.15)",
        ]
        rc1, _, _, _, rss_small = measure_execution(cmd_small, sample_interval_s=0.005)
        rc2, _, _, _, rss_large = measure_execution(cmd_large, sample_interval_s=0.005)
        self.assertEqual(rc1, 0)
        self.assertEqual(rc2, 0)
        self.assertIsNotNone(rss_small)
        self.assertIsNotNone(rss_large)
        # Large allocation must exceed small allocation by at least 25 MB
        self.assertGreater(rss_large - rss_small, 25 * 1024 * 1024)

    def test_harness_200mb_rss_isolation(self) -> None:
        """Verify harness memory footprint does not leak into child process measurement."""
        bloat = bytearray(200 * 1024 * 1024)
        try:
            cmd = [
                sys.executable,
                "-c",
                "import time; time.sleep(0.15)",
            ]
            rc, _, _, _, peak_rss = measure_execution(cmd, sample_interval_s=0.005)
            self.assertEqual(rc, 0)
            self.assertIsNotNone(peak_rss)
            # The lightweight child process should consume < 60 MB, completely isolated from harness 200 MB
            self.assertLess(peak_rss, 60 * 1024 * 1024)
        finally:
            del bloat

    def test_memory_misses_handling(self) -> None:
        """Verify aggregate_edge_metrics handles sampling misses and sets peak_mem_mb to None."""
        ref = CanonicalDoc(text="hello")
        hyp = CanonicalDoc(text="hello")

        # All scores have None peak_rss_bytes
        s1 = score_fixture(ref, hyp, wall_ms=10.0, peak_rss_bytes=None)
        s2 = score_fixture(ref, hyp, wall_ms=20.0, peak_rss_bytes=None)
        summary = aggregate_edge_metrics([s1, s2])
        self.assertEqual(summary.samples, 2)
        self.assertEqual(summary.memory_misses, 2)
        self.assertIsNone(summary.peak_mem_mb)
        self.assertIsNone(summary.to_dict()["peak_mem_mb"])

        # Partial misses: valid sample is preserved
        s3 = score_fixture(ref, hyp, wall_ms=30.0, peak_rss_bytes=10 * 1024 * 1024)
        summary_partial = aggregate_edge_metrics([s1, s3])
        self.assertEqual(summary_partial.samples, 2)
        self.assertEqual(summary_partial.memory_misses, 1)
        self.assertEqual(summary_partial.peak_mem_mb, 10.0)

    def test_measure_execution_respects_post_exec_guard(self) -> None:
        """Verify measure_execution strictly enforces the _is_post_exec guard."""
        with patch("ariad_bench.metrics._is_post_exec", return_value=False):
            cmd = [sys.executable, "-c", "import time; time.sleep(0.05)"]
            rc, _, _, _, peak_rss = measure_execution(cmd, sample_interval_s=0.001)
            self.assertEqual(rc, 0)
            self.assertIsNone(peak_rss)

    def test_is_post_exec_pre_exec_detection(self) -> None:
        """Verify _is_post_exec detects pre-exec fork states and avoids false directory matches."""
        # 1. Path substring match in directory name (e.g. /home/user/ashift-repo/bin/python)
        p_sub = MagicMock()
        p_sub.cmdline.return_value = ["/home/user/ashift-repo/bin/python3", "-m", "runner"]
        p_sub.exe.return_value = "/usr/bin/python3"
        self.assertFalse(_is_post_exec(p_sub, ["ashift", "__ir", "input.md"]))

        # 2. Post-exec state with matching binary and arguments
        p_post = MagicMock()
        p_post.cmdline.return_value = ["/path/to/target/release/ashift", "__ir", "input.md"]
        p_post.exe.return_value = "/path/to/target/release/ashift"
        self.assertTrue(_is_post_exec(p_post, ["ashift", "__ir", "input.md"]))

    def test_is_post_exec_fork_clone_same_binary_rejected(self) -> None:
        """Verify _is_post_exec rejects a fork clone when target binary matches harness binary.

        Kills the mutant: removing `if p_cmdline == sys.argv: return False`.
        """
        p_fork = MagicMock()
        p_fork.cmdline.return_value = list(sys.argv)
        p_fork.exe.return_value = sys.executable
        # Target binary is sys.argv[0] without additional arguments
        self.assertFalse(_is_post_exec(p_fork, [sys.argv[0]]))

    def test_is_post_exec_unrelated_arguments_rejected(self) -> None:
        """Verify _is_post_exec rejects process with matching binary but unrelated arguments.

        Kills the mutant: removing argument matching requirement (`any(arg in p_cmdline[1:]...)`).
        """
        p_unrelated = MagicMock()
        p_unrelated.cmdline.return_value = ["/path/to/target/release/ashift", "__write", "different.json"]
        p_unrelated.exe.return_value = "/path/to/target/release/ashift"
        self.assertFalse(_is_post_exec(p_unrelated, ["ashift", "__ir", "target.md"]))


if __name__ == "__main__":
    unittest.main()
