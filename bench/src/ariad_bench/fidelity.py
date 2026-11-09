"""Versioned, evidence-based fidelity scoring shared with ariad-core::fidelity.

Alignment uses exact normalized semantic keys, consuming each hypothesis once.
This conservative rule never attributes style or geometry from unrelated text.
The original conformance vectors are data, not corpus measurements.
"""

from __future__ import annotations

from collections import Counter, defaultdict, deque
import json
import math
from pathlib import Path
from typing import Any
import unicodedata

import rapidfuzz.distance.Levenshtein as lev

METRIC_VERSION = "fidelity/2"
MAX_ITEMS = 4096
MAX_TEXT_CHARS = 100_000
MAX_CER_CELLS = 4_000_000
STYLE_FIELDS = ("size_pt", "weight", "color", "font_family", "italic", "underline", "strike")


def clean(text: str) -> str:
    return " ".join(unicodedata.normalize("NFC", text).split())


def key(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def unavailable(reason: str, basis: str = "independent_reference") -> dict:
    return dict(metric_version=METRIC_VERSION, status="unavailable", basis=basis,
                score=None, text_cer=None, structure_f1=None, structure_by_kind={},
                style_accuracy=None, layout_iou=None, coverage={},
                unavailable_reasons={"report": reason})


def _pairs(reference: list, hypothesis: list, identity) -> list[tuple[int, int]]:
    indexes: dict[str, deque] = defaultdict(deque)
    for i, item in enumerate(hypothesis):
        indexes[identity(item)].append(i)
    return [(i, indexes[identity(item)].popleft()) for i, item in enumerate(reference)
            if indexes[identity(item)]]


def _style_equal(field: str, a: Any, b: Any) -> bool:
    if b is None:
        return False
    if field == "size_pt":
        return abs(a - b) <= 0.5
    if field == "color":
        if not all(isinstance(c, str) and len(c) == 7 and c[0] == "#" and all(v in "0123456789abcdefABCDEF" for v in c[1:]) for c in (a, b)):
            return False
        return max(abs(int(a[i:i+2], 16) - int(b[i:i+2], 16)) for i in (1, 3, 5)) <= 2
    return a == b


def _area(box: dict) -> float:
    if not all(math.isfinite(box[field]) for field in ("left", "top", "right", "bottom")):
        return 0.0
    area = max(0, box["right"] - box["left"]) * max(0, box["bottom"] - box["top"])
    return area if math.isfinite(area) else 0.0


def _coalesced(runs: list[dict]) -> list[dict]:
    merged: list[dict] = []
    for run in runs:
        if merged and merged[-1]["style"] == run["style"]:
            merged[-1]["text"] += run["text"]
        else:
            merged.append(dict(run))
    return [dict(text=clean(run["text"]), style=run["style"]) for run in merged if clean(run["text"])]


def _iou(a: dict, b: dict) -> float:
    if a["page_no"] != b["page_no"]:
        return 0.0
    x, y = a["bbox"], b["bbox"]
    area_a, area_b = _area(x), _area(y)
    if not area_a or not area_b:
        return 0.0
    intersection = max(0, min(x["right"], y["right"]) - max(x["left"], y["left"])) * max(0, min(x["bottom"], y["bottom"]) - max(x["top"], y["top"]))
    return intersection / (area_a + area_b - intersection)


def _layout_pairs(reference, hypothesis):
    matched = _pairs(reference, hypothesis, lambda b: key([b["text"], b["page_no"]]))
    used_r, used_h = {i for i, _ in matched}, {j for _, j in matched}
    missing = [i for i in range(len(reference)) if i not in used_r]
    remaining = [j for j in range(len(hypothesis)) if j not in used_h]
    fallback = _pairs([reference[i] for i in missing], [hypothesis[j] for j in remaining], lambda b: b["text"])
    return matched + [(missing[i], remaining[j]) for i, j in fallback]


def score(reference: dict, hypothesis: dict, basis: str = "independent_reference") -> dict:
    """Score observations. Raw CER retains the legacy empty-reference rules."""
    r, h = reference, hypothesis
    if any(len(d.get(k, [])) > MAX_ITEMS for d in (r, h) for k in ("structures", "runs", "boxes")):
        return unavailable("observation_limit", basis)
    rt, ht = r.get("text", ""), h.get("text", "")
    if max(len(rt), len(ht)) > MAX_TEXT_CHARS:
        return unavailable("text_work_limit", basis)
    prefix = 0
    while prefix < min(len(rt), len(ht)) and rt[prefix] == ht[prefix]:
        prefix += 1
    ra, ha = rt[prefix:], ht[prefix:]
    suffix = 0
    while suffix < min(len(ra), len(ha)) and ra[-1-suffix] == ha[-1-suffix]:
        suffix += 1
    if suffix:
        ra, ha = ra[:-suffix], ha[:-suffix]
    if max(len(rt), len(ht)) > MAX_TEXT_CHARS or len(ra) * len(ha) > MAX_CER_CELLS:
        return unavailable("text_work_limit", basis)
    cer = (float(lev.distance(ra, ha)) / len(rt)) if rt else float(bool(ht))
    result = unavailable("", basis)
    result.update(status="measured", text_cer=cer, unavailable_reasons={})
    components = [max(0.0, 1.0 - cer)]
    coverage = result["coverage"]
    coverage["text"] = dict(reference=len(rt), hypothesis=len(ht), matched=max(0, len(rt) - int(round(cer * len(rt)))), observed=len(ht))
    f1s = []
    for kind in ("heading", "list", "table"):
        rs = Counter(x["key"] for x in r.get("structures", []) if x["kind"] == kind)
        hs = Counter(x["key"] for x in h.get("structures", []) if x["kind"] == kind)
        rn, hn, matched = sum(rs.values()), sum(hs.values()), sum((rs & hs).values())
        counts = dict(reference=rn, hypothesis=hn, matched=matched, observed=hn)
        f1 = 2 * matched / (rn + hn) if rn + hn else None
        result["structure_by_kind"][kind] = dict(**counts, precision=matched / hn if hn else None,
                                                recall=matched / rn if rn else None, f1=f1)
        if f1 is not None:
            f1s.append(f1)
    if f1s:
        result["structure_f1"] = sum(f1s) / len(f1s)
        components.append(result["structure_f1"])
    else:
        result["unavailable_reasons"]["structure"] = "no_structure_observations"
    rr, hr = r.get("runs", []), h.get("runs", [])
    run_pairs = dict(_pairs(rr, hr, lambda x: x["text"]))
    total = matched = observed = 0
    for i, run in enumerate(rr):
        other = hr[run_pairs[i]]["style"] if i in run_pairs else {}
        for field in STYLE_FIELDS:
            value = run["style"].get(field)
            if value is not None:
                total += 1
                observed += other.get(field) is not None
                matched += _style_equal(field, value, other.get(field))
    coverage["style"] = dict(reference=total, hypothesis=sum(x["style"].get(f) is not None for x in hr for f in STYLE_FIELDS), matched=matched, observed=observed)
    if total:
        result["style_accuracy"] = matched / total
        components.append(result["style_accuracy"])
    else:
        result["unavailable_reasons"]["style"] = "no_reference_style_observations"
    rb, hb = r.get("boxes", []), h.get("boxes", [])
    box_pairs = _layout_pairs(rb, hb)
    coverage["layout"] = dict(reference=len(rb), hypothesis=len(hb), matched=sum(rb[i]["page_no"] == hb[j]["page_no"] for i, j in box_pairs), observed=len(box_pairs))
    if rb:
        result["layout_iou"] = sum(_iou(rb[i], hb[j]) for i, j in box_pairs) / len(rb)
        components.append(result["layout_iou"])
        if any(_area(b["bbox"]) == 0.0 for b in rb + hb):
            result["unavailable_reasons"]["layout_geometry"] = "degenerate_box_scores_zero"
    else:
        result["unavailable_reasons"]["layout"] = "no_reference_layout_observations"
    result["score"] = sum(components) / len(components)
    return result


def observe_ir(document: dict) -> dict:
    """Capture effective styles and point geometry from IR without using IDs for alignment."""
    if len(document.get("body", [])) > MAX_ITEMS:
        raise ValueError("observation_limit")
    pending = [(node, 0) for node in document.get("body", [])]
    nodes = characters = 0
    while pending:
        node, depth = pending.pop()
        if depth > 64:
            raise ValueError("observation_limit")
        nodes += 1
        characters += sum(len(node.get(field) or "") for field in ("text", "tex", "alt"))
        if characters > MAX_TEXT_CHARS:
            raise ValueError("text_work_limit")
        nested = []
        for field in ("blocks", "content", "caption", "footnotes", "items", "head", "body"):
            for value in node.get(field) or []:
                values = value if isinstance(value, list) else [value]
                if nodes + len(pending) + len(nested) + len(values) > MAX_ITEMS:
                    raise ValueError("observation_limit")
                nested.extend(values)
        if nodes + len(pending) + len(nested) > MAX_ITEMS:
            raise ValueError("observation_limit")
        pending.extend((node, depth + 1) for node in nested)
    out: dict = dict(text="", structures=[], runs=[], boxes=[])
    sheet = document.get("styles") or {}
    defaults = sheet.get("defaults") or {}
    pieces: list[str] = []
    heading_levels: list[int] = []

    def paragraph(named, direct):
        result = {}
        def apply(style, seen):
            if len(seen) >= 64:
                raise ValueError("style inheritance exceeds nesting limit")
            if parent := style.get("based_on"):
                if parent in seen or parent not in sheet.get("paragraph", {}):
                    raise ValueError("invalid paragraph inheritance")
                apply(sheet["paragraph"][parent], seen + [parent])
            result.update({k: v for k, v in style.items() if v is not None})
        apply(defaults.get("paragraph", {}), [])
        if named:
            apply(sheet["paragraph"][named], [named])
        for layer in direct if isinstance(direct, list) else [direct]:
            apply(layer, [])
        return result

    def char_style(parent, named=None, direct=None):
        result = {k: v for k, v in defaults.get("character", {}).items() if v is not None}
        result.update({k: v for k, v in parent.items() if v is not None})
        if named:
            result.update({k: v for k, v in sheet["character"][named].items() if v is not None})
        result.update({k: v for k, v in (direct or {}).items() if v is not None})
        return result

    def inlines(nodes, parent, capture=False):
        parts = []
        for node in nodes:
            kind = node["type"]
            style = dict(parent)
            if kind == "strong":
                style["weight"] = 700
            elif kind == "emph":
                style["italic"] = True
            elif kind == "strikeout":
                style["strike"] = True
            elif kind == "styled":
                style = char_style(parent, node.get("style"), node.get("direct"))
            if "content" in node:
                parts.append(inlines(node["content"], style, capture))
                continue
            text = node.get("text", node.get("tex", node.get("alt", "")))
            if kind in ("soft_break", "line_break"):
                text = " "
            parts.append(text)
            if capture:
                effective = {k: style[k] for k in STYLE_FIELDS if k in style}
                if "font" in style:
                    effective["font_family"] = style["font"]["family"]
                out["runs"].append(dict(text=text, style=effective))
        return "".join(parts)

    def block_text(block):
        if block["type"] == "table":
            return clean(" ".join([inlines(block.get("caption") or [], {})] + [block_text(b) for b in children(block)]))
        if "content" in block or "caption" in block:
            return clean(inlines(block.get("content", block.get("caption")) or [], {}))
        if "text" in block or "tex" in block:
            return clean(block.get("text", block.get("tex", "")))
        return clean(" ".join(block_text(b) for b in children(block)))

    def children(block):
        if block["type"] == "table":
            return [b for row in block.get("head", []) + block.get("body", []) for cell in row for b in cell.get("blocks", [])]
        if block["type"] == "list":
            return [b for item in block.get("items", []) for b in item.get("blocks", [])]
        return block.get("blocks", [])

    def walk(blocks, parent, path):
        for i, block in enumerate(blocks):
            kind = block["type"]
            if kind == "table":
                table_defaults = defaults.get("table", {})
                table = sheet.get("table", {}).get(block.get("style"), {})
                para = paragraph(None, [table_defaults.get("paragraph", {}), table.get("paragraph", {}), block.get("direct", {})])
                style = char_style(char_style(parent, direct=table_defaults.get("character", {})), direct=table.get("character", {}))
            else:
                para = paragraph(block.get("style"), block.get("direct", {}))
                style = char_style(parent)
            style.update({k: para[k] for k in ("font", "size_pt", "color") if k in para})
            text = block_text(block)
            signature = None
            if kind == "heading":
                level = block["level"]
                while heading_levels and heading_levels[-1] >= level:
                    heading_levels.pop()
                heading_levels.append(level)
                signature = [list(heading_levels), text]
            elif kind == "list":
                signature = [block.get("ordered", False), [clean(" ".join(block_text(b) for b in item.get("blocks", []))) for item in block.get("items", [])]]
            elif kind == "table":
                signature = [[[cell.get("rowspan", 1), cell.get("colspan", 1), cell.get("header") if cell.get("header") is not None else head, clean(" ".join(block_text(b) for b in cell.get("blocks", [])))] for cell in row] for head, rows in ((True, block.get("head", [])), (False, block.get("body", []))) for row in rows]
            if signature is not None:
                out["structures"].append(dict(kind=kind, key=key(signature)))
            positions = document.get("source_map", {}).get(block.get("id"))
            if not positions:
                layout = (document.get("layout") or {}).get("blocks", {})
                position = layout.get(block.get("id")) or layout.get(f"{path}/{i}")
                positions = [position] if position else []
            out["boxes"].extend(dict(text=text, **position) for position in positions)
            start = len(out["runs"])
            if "content" in block or "caption" in block:
                value = clean(inlines(block.get("content", block.get("caption")) or [], style, True))
                if value:
                    pieces.append(value)
            elif "text" in block or "tex" in block:
                if text:
                    pieces.append(text)
            # Coalesce only within this block, preserving textual whitespace boundaries.
            out["runs"][start:] = _coalesced(out["runs"][start:])
            if kind == "list":
                for item_index, item in enumerate(block.get("items", [])):
                    walk(item.get("blocks", []), style, f"{path}/{i}/items/{item_index}/blocks")
            elif kind == "table":
                for part in ("head", "body"):
                    for row_index, row in enumerate(block.get(part, [])):
                        for cell_index, cell in enumerate(row):
                            walk(cell.get("blocks", []), style, f"{path}/{i}/{part}/{row_index}/cells/{cell_index}/blocks")
            else:
                walk(children(block), style, f"{path}/{i}/blocks")
            if kind == "table" and block.get("footnotes"):
                start = len(out["runs"])
                text = clean(inlines(block["footnotes"], style, True))
                if text:
                    pieces.append(text)
                out["runs"][start:] = _coalesced(out["runs"][start:])
    walk(document.get("body", []), {}, "body")
    # Furniture is deliberately outside semantic body fidelity, as in canonical v1.
    out["text"] = "\n".join(pieces)
    return out


def observe(source: Any) -> dict:
    """Accept IR/truth, canonical documents, JSON paths, or observations."""
    if isinstance(source, (str, Path)):
        source = json.loads(Path(source).read_text(encoding="utf-8"))
    if isinstance(source, dict):
        if "structures" in source and "runs" in source:
            return source
        if "ir" in source:
            return observe_ir(source["ir"])
        if "body" in source:
            return observe_ir(source)
    from ariad_bench.canonical import normalize
    doc = normalize(source)
    if doc.observations is not None:
        if "unavailable_reason" in doc.observations:
            raise ValueError(doc.observations["unavailable_reason"])
        return doc.observations
    from ariad_bench.canonical import canonical_to_ir
    out = observe_ir(canonical_to_ir(doc))
    out["text"] = doc.text
    return out


def compare(reference: Any, hypothesis: Any, basis: str = "independent_reference") -> dict:
    try:
        return score(observe(reference), observe(hypothesis), basis)
    except (ValueError, KeyError, TypeError, RecursionError) as error:
        return unavailable(str(error), basis)
