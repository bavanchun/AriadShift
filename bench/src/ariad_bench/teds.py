"""Table Tree Edit Distance-based Similarity (TEDS) implementation.

Follows Zhong et al. 2019 ("Image-based table recognition: data, code, and evaluation")
using APTED for tree edit distance and RapidFuzz for normalized cell-text Levenshtein distance.
"""

from __future__ import annotations

from apted import APTED, Config
from apted.helpers import Tree
import rapidfuzz.distance.Levenshtein as lev

from ariad_bench.canonical import TableCell, TableGrid

MAX_CELLS_PER_TABLE = 500


class TableConfig(Config):
    """Cost model for table tree edit distance per Zhong et al. 2019.

    - Tree insertions and deletions cost 1.0.
    - Tag renames cost 1.0 if structure, header tag, or span differs.
    - If tag and spans match, content uses normalized Levenshtein distance in [0.0, 1.0].
    """

    def rename(self, node1: Tree, node2: Tree) -> float:
        n1 = str(node1.name)
        n2 = str(node2.name)
        if n1 == n2:
            return 0.0

        is_cell1 = n1.startswith("th:") or n1.startswith("td:")
        is_cell2 = n2.startswith("th:") or n2.startswith("td:")

        if is_cell1 and is_cell2:
            p1 = n1.split(":", 3)
            p2 = n2.split(":", 3)
            tag1, cs1, rs1 = p1[0], p1[1], p1[2]
            text1 = p1[3] if len(p1) > 3 else ""
            tag2, cs2, rs2 = p2[0], p2[1], p2[2]
            text2 = p2[3] if len(p2) > 3 else ""

            if tag1 != tag2 or cs1 != cs2 or rs1 != rs2:
                return 1.0

            if not text1 and not text2:
                return 0.0
            return float(lev.normalized_distance(text1, text2))

        return 1.0


def count_tree_nodes(tree: Tree) -> int:
    """Count the total number of nodes in a Tree."""
    return 1 + sum(count_tree_nodes(c) for c in tree.children)


def table_to_tree(table: TableGrid) -> Tree:
    """Convert a TableGrid into an invariant HTML-like Tree per Zhong et al. 2019."""
    head_rows: list[Tree] = []
    body_rows: list[Tree] = []

    for row in table.rows:
        cell_trees: list[Tree] = []
        is_head_row = False
        for cell in row:
            tag = "th" if cell.is_header else "td"
            if cell.is_header:
                is_head_row = True
            cs = max(1, cell.colspan)
            rs = max(1, cell.rowspan)
            cell_tree = Tree(f"{tag}:cs={cs}:rs={rs}:{cell.text}")
            cell_trees.append(cell_tree)

        tr_tree = Tree("tr", *cell_trees)
        if is_head_row:
            head_rows.append(tr_tree)
        else:
            body_rows.append(tr_tree)

    table_children: list[Tree] = []
    if head_rows:
        table_children.append(Tree("thead", *head_rows))
    if body_rows or not head_rows:
        table_children.append(Tree("tbody", *body_rows))

    return Tree("table", *table_children)


def compute_table_teds(ref_table: TableGrid, hyp_table: TableGrid) -> tuple[float | None, bool]:
    """Compute TEDS between two tables.

    Returns:
        (teds_score, skipped)
        If either table exceeds MAX_CELLS_PER_TABLE, returns (None, True).
    """
    ref_cells = sum(len(r) for r in ref_table.rows)
    hyp_cells = sum(len(r) for r in hyp_table.rows)

    if ref_cells > MAX_CELLS_PER_TABLE or hyp_cells > MAX_CELLS_PER_TABLE:
        return None, True

    if ref_cells == 0 and hyp_cells == 0:
        return 1.0, False

    tree_ref = table_to_tree(ref_table)
    tree_hyp = table_to_tree(hyp_table)

    nodes_ref = count_tree_nodes(tree_ref)
    nodes_hyp = count_tree_nodes(tree_hyp)
    max_nodes = max(nodes_ref, nodes_hyp)

    if max_nodes == 0:
        return 1.0, False

    apted = APTED(tree_ref, tree_hyp, TableConfig())
    distance = float(apted.compute_edit_distance())

    teds = max(0.0, min(1.0, 1.0 - (distance / max_nodes)))
    return teds, False


def compute_document_teds(
    ref_tables: list[TableGrid],
    hyp_tables: list[TableGrid],
) -> tuple[float | None, int]:
    """Compute aggregated TEDS score across all tables in a document.

    Returns:
        (average_teds, skipped_count)
        If neither document has tables, returns (None, 0).
    """
    if not ref_tables and not hyp_tables:
        return None, 0

    if not ref_tables and hyp_tables:
        return 0.0, 0

    if ref_tables and not hyp_tables:
        return 0.0, 0

    pair_count = max(len(ref_tables), len(hyp_tables))
    scores: list[float] = []
    skipped_count = 0

    for i in range(pair_count):
        if i >= len(ref_tables):
            # Extra table in hypothesis
            scores.append(0.0)
            continue
        if i >= len(hyp_tables):
            # Missing table in hypothesis
            scores.append(0.0)
            continue

        score, skipped = compute_table_teds(ref_tables[i], hyp_tables[i])
        if skipped or score is None:
            skipped_count += 1
        else:
            scores.append(score)

    if not scores:
        return None, skipped_count

    return sum(scores) / len(scores), skipped_count
