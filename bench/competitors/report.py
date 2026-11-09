"""Validate paired evidence and render it without implementing a fidelity metric."""

from collections import Counter
from decimal import Decimal
import math
import re
import statistics

SCHEMA = "ariad-competitors/1"
ROUTES = ("pdf->docx", "docx->pdf")
PRODUCTS = ("ariadshift", "stirling")
COMPONENTS = ("text_cer", "table_f1", "style_accuracy", "layout_iou")
HASH = re.compile(r"[0-9a-f]{64}\Z")


def require_hash(value):
    if not isinstance(value, str) or not HASH.fullmatch(value):
        raise ValueError("Evidence requires actual SHA-256 hashes")


def validate_score(score):
    if not isinstance(score, dict) or not score.get("metric_version"):
        raise ValueError("A versioned fidelity scorer report is required")
    if not isinstance(score.get("coverage"), dict):
        raise ValueError("Scorer component coverage/denominators are required")
    for name in COMPONENTS:
        coverage = score["coverage"].get(name, {})
        matched, reference = coverage.get("matched"), coverage.get("reference")
        if type(matched) is not int or type(reference) is not int or not 0 <= matched <= reference:
            raise ValueError(f"Matched/reference component denominators required: {name}")
        if name not in score:
            raise ValueError(f"Missing scorer component: {name}")
        value = score[name]
        if value is None:
            if not score.get("unavailable_reasons"):
                raise ValueError("Unavailable components require an explanation")
        elif (
            isinstance(value, bool)
            or not isinstance(value, (int, float))
            or not math.isfinite(value)
            or value < 0
            or (name != "text_cer" and value > 1)
        ):
            raise ValueError(f"Invalid scorer component: {name}")
        elif reference == 0:
            raise ValueError(f"A score needs a nonzero reference denominator: {name}")


def validate_evidence(data):
    """Reject selective samples; failures must occupy their expected matrix cells."""
    if data.get("schema") != SCHEMA or data.get("kind") != "corpus":
        raise ValueError("Only versioned corpus evidence can enter the public report")
    if data.get("cleanup") != "completed" or data.get("gaps"):
        raise ValueError("Accepted evidence requires completed cleanup and no unresolved prerequisite gaps")
    if data.get("routes") != list(ROUTES):
        raise ValueError("Both document routes are required")
    repeat = data.get("repeat")
    if type(repeat) is not int or repeat < 1:
        raise ValueError("Positive repeat count required")
    fixtures = data.get("fixtures", [])
    if not fixtures or len({f["id"] for f in fixtures}) != len(fixtures):
        raise ValueError("A nonempty unique authored corpus is required")
    expected = set()
    sources = {}
    for fixture in fixtures:
        for route in ROUTES:
            source = fixture["sources"][route.split("->")[0]]
            require_hash(source["sha256"])
            require_hash(source["truth_sha256"])
            sources[fixture["id"], route] = source
            for product in PRODUCTS:
                for attempt in range(1, repeat + 1):
                    expected.add((fixture["id"], route, product, attempt))
    seen = set()
    versions = set()
    for sample in data.get("samples", []):
        key = tuple(sample[k] for k in ("fixture", "route", "product", "attempt"))
        if key not in expected or key in seen:
            raise ValueError("Unexpected or duplicate paired attempt")
        seen.add(key)
        source = sources[key[:2]]
        if any(sample.get(k) != source[k] for k in ("sha256", "truth_sha256")):
            raise ValueError("Paired attempts must use the exact same source and truth hashes")
        if sample.get("status") not in ("ok", "failed", "unsupported", "unavailable"):
            raise ValueError("Every attempt requires an explicit outcome")
        if sample["status"] != "ok" and not sample.get("reason"):
            raise ValueError("Failed/missing attempts require a reason")
        for name in ("extraction_ms", "conversion_ms", "evaluation_ms"):
            value = sample.get(name)
            if not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
                raise ValueError("Separate nonnegative stage timings are required")
        if sample.get("output_sha256") is not None:
            require_hash(sample["output_sha256"])
        if sample["status"] == "ok":
            require_hash(sample.get("output_sha256"))
            validate_score(sample.get("score"))
            versions.add(sample["score"]["metric_version"])
        elif sample.get("score") is not None:
            raise ValueError("An unsuccessful attempt cannot carry a successful score")
    if seen != expected:
        raise ValueError("Missing paired attempts, including failures")
    if len(versions) > 1:
        raise ValueError("Metric versions cannot be mixed")
    for name in ("ashift_sha256", "config_sha256"):
        require_hash(data.get(name))
    if not data.get("environment") or not data.get("image", {}).get("id"):
        raise ValueError("Runtime and actual image identity are required")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", data["image"]["id"]):
        raise ValueError("Docker image identity requires a SHA-256 content hash")
    if not re.fullmatch(r".+:[0-9]+\.[0-9]+\.[0-9]+@sha256:[0-9a-f]{64}", data["image"].get("reference", "")):
        raise ValueError("Evidence requires the pinned stable image reference")
    if data.get("status") == "pass" and overall_status(data) != "pass":
        raise ValueError("Claimed acceptance does not match both routes and the competitive gates")
    return data


def compare(data, route):
    samples = [s for s in data["samples"] if s["route"] == route]
    products = {}
    for product in PRODUCTS:
        rows = [s for s in samples if s["product"] == product]
        values = {}
        for component in COMPONENTS:
            observed = [s["score"][component] for s in rows if s["status"] == "ok" and s["score"][component] is not None]
            values[component] = {
                "mean": float(sum(Decimal(str(v)) for v in observed) / len(observed)) if observed else None,
                "observed": len(observed), "expected": len(rows),
            }
        products[product] = values
    complete = all(v["observed"] == v["expected"] and v["expected"] > 0 for p in products.values() for v in p.values())
    a, s = products["ariadshift"], products["stirling"]
    style_a, style_s = a["style_accuracy"]["mean"], s["style_accuracy"]["mean"]
    delta = (style_a - style_s) * 100 if style_a is not None and style_s is not None else None
    relative = (style_a / style_s - 1) * 100 if delta is not None and style_s else None
    gates = {}
    for name in ("text_cer", "table_f1", "style_accuracy"):
        av, sv = a[name]["mean"], s[name]["mean"]
        gates[name] = None if av is None or sv is None else (
            av <= sv if name == "text_cer" else
            Decimal(str(av)) >= Decimal("1.10") * Decimal(str(sv)) if name == "style_accuracy" else av >= sv
        )
    status = "incomplete" if not complete else "pass" if all(gates.values()) else "fail"
    return {"products": products, "gates": gates, "status": status, "style_delta_pp": delta, "style_relative_percent": relative}


def overall_status(data):
    comparisons = {route: compare(data, route) for route in ROUTES}
    if any(value["status"] == "incomplete" for value in comparisons.values()):
        return "incomplete"
    # The competitive threshold is approved for PDF->DOCX; the reverse route
    # must still complete and disclose all metrics and diagnostic comparisons.
    return comparisons["pdf->docx"]["status"]


def render(data):
    validate_evidence(data)
    lines = [
        "## Local Stirling comparison", "",
        f"Measured: {data['generated_at']}. Repetitions: {data['repeat']} per product, route and fixture.", "",
        f"Image: `{data['image']['reference']}`; local image ID `{data['image']['id']}`.",
        f"AriadShift SHA-256: `{data['ashift_sha256']}`. Config SHA-256: `{data['config_sha256']}`.", "",
        "Both products use identical authored sources and truth, OCR policy and the same versioned scorer. "
        "Source extraction, conversion and output read-back/evaluation are timed separately. "
        "No auxiliary renderer is used; missing read-back layout stays unavailable. "
        "Means describe available observations; coverage and all failures remain visible and prevent acceptance.", "",
        "| Route | Product | CER ↓ | Table F1 ↑ | Style accuracy ↑ | Layout IoU ↑ | Conversion p50 ms | Gate |",
        "|---|---|---:|---:|---:|---:|---:|---|",
    ]
    for route in ROUTES:
        result = compare(data, route)
        for product in PRODUCTS:
            cells = []
            for name in COMPONENTS:
                v = result["products"][product][name]
                value = f"{v['mean']:.4f}" if v["mean"] is not None else "N/A"
                cells.append(f"{value} ({v['observed']}/{v['expected']})")
            times = [s["conversion_ms"] for s in data["samples"] if s["route"] == route and s["product"] == product and s["status"] in ("ok", "unavailable") and s.get("output_sha256")]
            timing = f"{statistics.median(times):.0f}" if times else "N/A"
            lines.append("| " + " | ".join([route, product, *cells, timing, result["status"]]) + " |")
        pp = result["style_delta_pp"]
        relative = result["style_relative_percent"]
        lines += ["", f"{route}: style delta {f'{pp:+.2f} percentage points' if pp is not None else 'N/A'}; "
                  f"relative gain {f'{relative:+.2f}%' if relative is not None else 'N/A (missing or zero competitor baseline)'}. "
                  f"Component gates: CER={result['gates']['text_cer']}, table={result['gates']['table_f1']}, style={result['gates']['style_accuracy']}.", ""]
    lines += [
        "The PDF->DOCX gate requires CER(A) <= CER(S), table F1(A) >= table F1(S), and "
        "style(A) >= 1.10 * style(S). For a zero competitor style baseline, the last inequality "
        "still applies (A >= 0); relative gain is undefined, including a zero/zero tie. "
        "No infinite improvement is claimed. Impossible or failed gates require the owner's recorded decision.", "",
        "### Attempt outcomes and component coverage", "",
    ]
    outcomes = Counter((s["route"], s["product"], s["status"]) for s in data["samples"])
    for (route, product, status), count in sorted(outcomes.items()):
        lines.append(f"- {route}, {product}, {status}: {count} attempts.")
    for sample in data["samples"]:
        reason = sample.get("reason") or (sample.get("score") or {}).get("unavailable_reasons")
        coverage = (sample.get("score") or {}).get("coverage")
        lines.append(f"- {sample['fixture']} / {sample['route']} / {sample['product']} / attempt {sample['attempt']}: "
                     f"{sample['status']}; coverage: {coverage or 'N/A'}; reason: {reason or 'none'}.")
    return "\n".join(lines) + "\n"
