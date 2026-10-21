"""Benchmark metrics computation for AriadShift.

Implements:
- text_cer: Character Error Rate via jiwer
- heading_ted: Normalized Tree Edit Distance on heading hierarchy via apted
- teds: Table Tree Edit Distance-based Similarity via teds.py
- fidelity: mean(1 - text_cer, 1 - heading_ted, teds_if_any)
- editability: surviving share of headings, lists, and tables
- p50_ms: median latency across repeats rounded to 10 ms
- peak_mem_mb: peak RSS across process tree sampled every 10 ms rounded to 1 MB
"""

from __future__ import annotations

from dataclasses import dataclass
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Any

from apted import APTED, Config
from apted.helpers import Tree
import jiwer
import psutil
import rapidfuzz.distance.Levenshtein as lev

from ariad_bench.canonical import CanonicalDoc, HeadingNode
from ariad_bench.teds import compute_document_teds


class HeadingConfig(Config):
    """Cost model for heading tree edit distance."""

    def rename(self, node1: Tree, node2: Tree) -> float:
        n1 = str(node1.name)
        n2 = str(node2.name)
        if n1 == n2:
            return 0.0

        if n1 == "root" or n2 == "root":
            return 1.0

        p1 = n1.split(":", 1)
        p2 = n2.split(":", 1)
        h1, text1 = p1[0], p1[1] if len(p1) > 1 else ""
        h2, text2 = p2[0], p2[1] if len(p2) > 1 else ""

        if h1 != h2:
            return 1.0

        if not text1 and not text2:
            return 0.0
        return float(lev.normalized_distance(text1, text2))


def heading_to_tree(node: HeadingNode) -> Tree:
    """Convert a HeadingNode and its children to an APTED Tree."""
    child_trees = [heading_to_tree(c) for c in node.children]
    return Tree(f"h{node.level}:{node.text}", *child_trees)


def count_tree_nodes(tree: Tree) -> int:
    """Count the total number of nodes in a Tree."""
    return 1 + sum(count_tree_nodes(c) for c in tree.children)


def compute_text_cer(ref_text: str, hyp_text: str) -> float:
    """Compute Character Error Rate (CER) using jiwer."""
    if not ref_text and not hyp_text:
        return 0.0
    if not ref_text and hyp_text:
        return 1.0
    val = jiwer.cer(ref_text, hyp_text)
    return float(val)


def count_total_headings(headings: list[HeadingNode]) -> int:
    """Count total heading nodes recursively."""
    return len(headings) + sum(count_total_headings(h.children) for h in headings)


def compute_heading_ted(
    ref_headings: list[HeadingNode],
    hyp_headings: list[HeadingNode],
) -> float:
    """Compute normalized heading tree edit distance in [0.0, 1.0]."""
    real_ref = count_total_headings(ref_headings)
    real_hyp = count_total_headings(hyp_headings)

    if real_ref == 0 and real_hyp == 0:
        return 0.0
    if real_ref == 0 or real_hyp == 0:
        return 1.0

    tree_ref = Tree("root", *[heading_to_tree(h) for h in ref_headings])
    tree_hyp = Tree("root", *[heading_to_tree(h) for h in hyp_headings])

    max_nodes = max(real_ref, real_hyp)
    apted = APTED(tree_ref, tree_hyp, HeadingConfig())
    dist = float(apted.compute_edit_distance())
    return max(0.0, min(1.0, dist / max_nodes))


def _flatten_headings(headings: list[HeadingNode]) -> list[HeadingNode]:
    res: list[HeadingNode] = []
    for h in headings:
        res.append(h)
        res.extend(_flatten_headings(h.children))
    return res


def compute_editability(ref_doc: CanonicalDoc, hyp_doc: CanonicalDoc) -> float:
    """Compute editability: share of headings, lists, and tables in ref that survive as same structure kind."""
    from collections import Counter
    from ariad_bench.canonical import clean_text

    # 1. Headings: matched by text
    h_ref_texts = Counter(clean_text(h.text) for h in _flatten_headings(ref_doc.headings) if clean_text(h.text))
    h_hyp_texts = Counter(clean_text(h.text) for h in _flatten_headings(hyp_doc.headings) if clean_text(h.text))
    survived_headings = sum((h_ref_texts & h_hyp_texts).values())
    total_headings = sum(h_ref_texts.values())

    # 2. Lists: matched by item content (>= 50% item overlap)
    total_lists = len(ref_doc.lists)
    survived_lists = 0
    hyp_list_items = [set(clean_text(it) for it in l.items if clean_text(it)) for l in hyp_doc.lists]
    used_hyp_lists: set[int] = set()
    for l_ref in ref_doc.lists:
        ref_items = set(clean_text(it) for it in l_ref.items if clean_text(it))
        if not ref_items:
            continue
        best_idx = None
        best_overlap = 0.0
        for idx, h_items in enumerate(hyp_list_items):
            if idx in used_hyp_lists:
                continue
            overlap = len(ref_items & h_items) / len(ref_items)
            if overlap > best_overlap:
                best_overlap = overlap
                best_idx = idx
        if best_idx is not None and best_overlap >= 0.5:
            survived_lists += 1
            used_hyp_lists.add(best_idx)

    # 3. Tables: matched by cell content (>= 50% cell overlap)
    total_tables = len(ref_doc.tables)
    survived_tables = 0
    hyp_table_cells: list[set[str]] = []
    for tbl in hyp_doc.tables:
        cells: set[str] = set()
        for row in tbl.rows:
            for cell in row:
                t = clean_text(cell.text)
                if t:
                    cells.add(t)
        hyp_table_cells.append(cells)

    used_hyp_tables: set[int] = set()
    for tbl in ref_doc.tables:
        ref_cells: set[str] = set()
        for row in tbl.rows:
            for cell in row:
                t = clean_text(cell.text)
                if t:
                    ref_cells.add(t)
        if not ref_cells:
            continue
        best_idx = None
        best_overlap = 0.0
        for idx, h_cells in enumerate(hyp_table_cells):
            if idx in used_hyp_tables:
                continue
            overlap = len(ref_cells & h_cells) / len(ref_cells)
            if overlap > best_overlap:
                best_overlap = overlap
                best_idx = idx
        if best_idx is not None and best_overlap >= 0.5:
            survived_tables += 1
            used_hyp_tables.add(best_idx)

    total_ref = total_headings + total_lists + total_tables
    if total_ref == 0:
        hyp_total = len(_flatten_headings(hyp_doc.headings)) + len(hyp_doc.lists) + len(hyp_doc.tables)
        return 1.0 if hyp_total == 0 else 0.0

    survived = survived_headings + survived_lists + survived_tables
    return max(0.0, min(1.0, survived / total_ref))


@dataclass
class SingleFixtureScore:
    text_cer: float
    heading_ted: float
    teds: float | None
    fidelity: float
    editability: float
    wall_ms: float
    peak_rss_bytes: int | None
    tables_skipped: int = 0


def score_fixture(
    ref_doc: CanonicalDoc,
    hyp_doc: CanonicalDoc,
    wall_ms: float = 0.0,
    peak_rss_bytes: int | None = None,
) -> SingleFixtureScore:
    """Compute all quality metrics for a single (reference, hypothesis) fixture pair."""
    cer = compute_text_cer(ref_doc.text, hyp_doc.text)
    h_ted = compute_heading_ted(ref_doc.headings, hyp_doc.headings)
    doc_teds, skipped_tables = compute_document_teds(ref_doc.tables, hyp_doc.tables)

    components = [
        max(0.0, min(1.0, 1.0 - cer)),
        max(0.0, min(1.0, 1.0 - h_ted)),
    ]
    if doc_teds is not None:
        components.append(doc_teds)

    fidelity = sum(components) / len(components)
    editability = compute_editability(ref_doc, hyp_doc)

    return SingleFixtureScore(
        text_cer=cer,
        heading_ted=h_ted,
        teds=doc_teds,
        fidelity=fidelity,
        editability=editability,
        wall_ms=wall_ms,
        peak_rss_bytes=peak_rss_bytes,
        tables_skipped=skipped_tables,
    )


def _read_linux_vm_hwm(pid: int) -> int | None:
    """Read peak resident set size (VmHWM) in bytes from Linux /proc/<pid>/status."""
    try:
        with open(f"/proc/{pid}/status", "r", encoding="ascii", errors="replace") as f:
            for line in f:
                if line.startswith("VmHWM:"):
                    parts = line.split()
                    if len(parts) >= 2:
                        return int(parts[1]) * 1024
    except (OSError, ValueError):
        pass
    return None


def _get_linux_child_pids(pid: int) -> list[int]:
    """Recursively collect child PIDs for a process across all threads on Linux.

    Reads /proc/<curr>/task/*/children for every thread in the process tree,
    ensuring subprocesses spawned by non-main threads (such as external engines
    launched from worker threads) are discovered.
    """
    result: list[int] = []
    seen: set[int] = set()
    stack = [pid]
    while stack:
        curr = stack.pop()
        task_dir = f"/proc/{curr}/task"
        try:
            tids = os.listdir(task_dir)
        except OSError:
            tids = [str(curr)]
        for tid in tids:
            path = f"{task_dir}/{tid}/children"
            try:
                with open(path, "r", encoding="ascii") as f:
                    for p_str in f.read().split():
                        c_pid = int(p_str)
                        if c_pid not in seen:
                            seen.add(c_pid)
                            result.append(c_pid)
                            stack.append(c_pid)
            except (OSError, ValueError):
                pass
    return result


def _get_process_peak_rss(p: psutil.Process) -> int:
    """Get peak resident memory in bytes for process p using platform-appropriate mechanism."""
    hwm = _read_linux_vm_hwm(p.pid)
    if hwm is not None and hwm > 0:
        return hwm
    mem = p.memory_info()
    peak_wset = getattr(mem, "peak_wset", None)
    if peak_wset is not None and peak_wset > 0:
        return int(peak_wset)
    return int(mem.rss)


def _is_post_exec(p: psutil.Process, cmd: list[str]) -> bool:
    """Verify that process p has executed the target command and is not in pre-exec fork state.

    Prevents capturing pre-execve fork memory which inherits the harness memory footprint.
    Does not use directory substring matching to avoid false positives when repository path
    contains the binary name.
    """
    if not cmd:
        return False
    try:
        p_cmdline = p.cmdline()
        if not p_cmdline:
            return False

        # If cmdline matches the current running harness process, it's still in pre-exec fork state
        if p_cmdline == sys.argv:
            return False

        target_name = Path(cmd[0]).name
        p_exe_name = ""
        try:
            p_exe_name = Path(p.exe()).name
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass

        cmdline_name = Path(p_cmdline[0]).name

        # If target has arguments, verify at least one non-trivial argument is present in cmdline
        if len(cmd) > 1:
            matching_args = any(arg in p_cmdline[1:] for arg in cmd[1:] if len(arg) > 1)
            if not matching_args:
                return False

        # Verify executable or argv[0] basename matches target executable basename
        if (p_exe_name and p_exe_name == target_name) or cmdline_name == target_name:
            return True

        # Or if executed via shebang interpreter (/bin/sh, python), check argv[1] basename
        if len(p_cmdline) > 1 and Path(p_cmdline[1]).name == target_name:
            return True

        return False
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        return False


def measure_execution(
    cmd: list[str],
    cwd: Path | str | None = None,
    sample_interval_s: float = 0.0005,
    env: dict[str, str] | None = None,
    timeout_s: float = 60.0,
) -> tuple[int, str, str, float, int | None]:
    """Execute a command, tracking wall-clock time and sampling process tree peak RSS.

    Uses temporary files for stdout/stderr to prevent OS pipe buffer deadlocks on large outputs.
    Samples the child process tree peak RSS using psutil only once post-exec. Reports None if
    the process exited before any memory sample could be obtained (sampling miss).

    Returns:
        (returncode, stdout, stderr, wall_ms, peak_rss_bytes)
    """
    start_time = time.perf_counter()

    with tempfile.TemporaryFile(mode="w+b") as out_f, \
         tempfile.TemporaryFile(mode="w+b") as err_f:
        proc = subprocess.Popen(
            cmd,
            cwd=cwd,
            stdout=out_f,
            stderr=err_f,
            env=env,
        )

        peak_rss: int | None = None
        timed_out = False
        try:
            p = psutil.Process(proc.pid)
            while proc.poll() is None:
                # 1. Sample process tree RSS only when verified post-exec
                try:
                    if _is_post_exec(p, cmd):
                        total_rss = _get_process_peak_rss(p)
                        if sys.platform.startswith("linux"):
                            child_pids = _get_linux_child_pids(p.pid)
                            for c_pid in child_pids:
                                c_hwm = _read_linux_vm_hwm(c_pid)
                                if c_hwm:
                                    total_rss += c_hwm
                                else:
                                    try:
                                        total_rss += psutil.Process(c_pid).memory_info().rss
                                    except Exception:
                                        pass
                        else:
                            for child in p.children(recursive=True):
                                try:
                                    total_rss += _get_process_peak_rss(child)
                                except (psutil.NoSuchProcess, psutil.AccessDenied):
                                    pass
                        if peak_rss is None or total_rss > peak_rss:
                            peak_rss = total_rss
                except (psutil.NoSuchProcess, psutil.AccessDenied):
                    break

                # 2. Check timeout
                if (time.perf_counter() - start_time) > timeout_s:
                    timed_out = True
                    try:
                        for child in p.children(recursive=True):
                            try:
                                child.kill()
                            except (psutil.NoSuchProcess, psutil.AccessDenied):
                                pass
                        p.kill()
                    except (psutil.NoSuchProcess, psutil.AccessDenied):
                        pass
                    break

                time.sleep(sample_interval_s)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass

        try:
            proc.wait(timeout=1.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()

        wall_ms = (time.perf_counter() - start_time) * 1000.0

        out_f.seek(0)
        stdout = out_f.read().decode("utf-8", errors="replace")
        err_f.seek(0)
        stderr = err_f.read().decode("utf-8", errors="replace")

        if timed_out:
            stderr += f"\nProcess timed out after {timeout_s:.1f} s."
            return -1, stdout, stderr, wall_ms, peak_rss

        return proc.returncode, stdout, stderr, wall_ms, peak_rss


@dataclass
class EdgeSummary:
    fidelity: float
    editability: float
    p50_ms: float
    peak_mem_mb: float | None
    samples: int
    memory_misses: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {
            "fidelity": self.fidelity,
            "editability": self.editability,
            "p50_ms": self.p50_ms,
            "peak_mem_mb": self.peak_mem_mb,
            "samples": self.samples,
        }


def aggregate_edge_metrics(scores: list[SingleFixtureScore]) -> EdgeSummary:
    """Aggregate fixture scores into EdgeMetrics rounded per specification:

    - fidelity, editability: rounded to 3 decimal places
    - p50_ms: median of wall times rounded to 10 ms
    - peak_mem_mb: peak RSS across process tree rounded to 1 MB (None if all samples miss)
    - samples: fixture count
    """
    if not scores:
        return EdgeSummary(
            fidelity=0.0,
            editability=0.0,
            p50_ms=0.0,
            peak_mem_mb=None,
            samples=0,
            memory_misses=0,
        )

    mean_fidelity = statistics.fmean(s.fidelity for s in scores)
    mean_editability = statistics.fmean(s.editability for s in scores)

    wall_times = [s.wall_ms for s in scores]
    median_wall_ms = statistics.median(wall_times)

    valid_rss = [s.peak_rss_bytes for s in scores if s.peak_rss_bytes is not None]
    if valid_rss:
        max_rss_bytes = max(valid_rss)
        peak_mb = max_rss_bytes / (1024.0 * 1024.0)
        rounded_peak_mb: float | None = float(round(peak_mb))
    else:
        rounded_peak_mb = None

    memory_misses = sum(1 for s in scores if s.peak_rss_bytes is None)

    # Rounding specifications:
    # - scores rounded to 3 decimals
    # - p50_ms rounded to 10 ms
    # - peak_mem_mb rounded to 1 MB (or None if all samples miss)
    rounded_fidelity = round(mean_fidelity, 3)
    rounded_editability = round(mean_editability, 3)
    rounded_p50 = float(round(median_wall_ms / 10.0) * 10)

    return EdgeSummary(
        fidelity=rounded_fidelity,
        editability=rounded_editability,
        p50_ms=rounded_p50,
        peak_mem_mb=rounded_peak_mb,
        samples=len(scores),
        memory_misses=memory_misses,
    )
