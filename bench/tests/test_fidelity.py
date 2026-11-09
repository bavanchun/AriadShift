"""Original shared vectors exercise the portable fidelity contract."""
import json
from pathlib import Path

import pytest

from ariad_bench.fidelity import METRIC_VERSION, compare, observe_ir, score
from ariad_bench.canonical import normalize

VECTORS = json.loads((Path(__file__).parents[1] / "results/fidelity-v2/conformance.json").read_text())


def assert_subset(actual, expected):
    if isinstance(expected, dict):
        for field, value in expected.items():
            assert_subset(actual[field], value)
    elif isinstance(expected, float):
        assert actual == pytest.approx(expected, abs=VECTORS["tolerance"])
    else:
        assert actual == expected


def check_named_vectors(names):
    for vector in VECTORS["vectors"]:
        if vector["name"] in names:
            assert_subset(score(vector["reference"], vector["hypothesis"]), vector["expected"])


def test_fidelity_v2_cer_preserves_insertions_and_empty_reference_rules():
    check_named_vectors({"insertions", "empty-reference", "empty-both", "unicode"})


def test_fidelity_v2_structure_f1_penalizes_missing_and_duplicate_nodes():
    check_named_vectors({"duplicate-heading", "missing-structures"})


def test_fidelity_v2_layout_iou_handles_missing_and_degenerate_boxes():
    check_named_vectors({"box-intersection", "wrong-page", "missing-box", "degenerate-box", "reordered-page-fragments"})


@pytest.mark.parametrize("vector", VECTORS["vectors"], ids=lambda v: v["name"])
def test_fidelity_v2_python_rust_vectors_agree(vector):
    result = score(vector["reference"], vector["hypothesis"])
    assert result["metric_version"] == METRIC_VERSION
    assert_subset(result, vector["expected"])


@pytest.mark.parametrize("vector", VECTORS["ir_vectors"], ids=lambda v: v["name"])
def test_fidelity_v2_style_tolerances_and_explicit_resets(vector):
    assert_subset(compare(vector["reference"], vector["hypothesis"]), vector["expected"])


def test_fidelity_v2_missing_evidence_is_not_perfect_accuracy():
    report = compare(normalize({"text":"abc", "headings":[], "tables":[]}), normalize({"text":"abc", "headings":[], "tables":[]}))
    assert report["style_accuracy"] is None
    assert report["layout_iou"] is None
    assert report["coverage"]["style"]["reference"] == 0
    assert report["unavailable_reasons"]["layout"]


def test_canonical_preserves_style_observations_and_independent_truth():
    reference = VECTORS["ir_vectors"][0]["reference"]
    canonical = normalize(dict(ir=reference, canonical=dict(text="abc", headings=[], tables=[])))
    assert canonical.observations == observe_ir(reference)
    assert normalize(canonical.to_dict()).observations == canonical.observations


def test_fidelity_work_limits_are_explicit():
    report = score(dict(text="a" * 100_001), {})
    assert report["status"] == "unavailable"
    assert report["score"] is None
    assert report["unavailable_reasons"]["report"] == "text_work_limit"


def test_observation_extraction_bounds_depth_before_semantic_walk():
    inline = dict(type="text", text="abc")
    for _ in range(70):
        inline = dict(type="strong", content=[inline])
    document = dict(body=[dict(type="paragraph", id="a", content=[inline])])
    report = compare(document, document)
    assert report["status"] == "unavailable"
    assert report["unavailable_reasons"]["report"] == "observation_limit"


def test_fidelity_v2_legacy_nested_layout_paths_are_observed():
    import copy
    reference = dict(version="ariad-ir/0", body=[dict(type="list", ordered=False, items=[dict(blocks=[dict(type="paragraph", content=[dict(type="text", text="abc")])])])], layout=dict(blocks={"body/0/items/0/blocks/0":dict(page_no=1, bbox=dict(left=0, top=0, right=10, bottom=10))}))
    hypothesis = copy.deepcopy(reference)
    hypothesis["layout"]["blocks"]["body/0/items/0/blocks/0"]["bbox"].update(left=5, right=15)
    report = compare(reference, hypothesis)
    assert report["layout_iou"] == pytest.approx(1 / 3)
    assert report["coverage"]["layout"]["reference"] == 1


def test_legacy_style_blindness_has_measured_v2_regression_coverage():
    from ariad_bench.metrics import score_fixture, score_fixture_v2
    reference = normalize(dict(version="ariad-ir/2", **VECTORS["ir_vectors"][0]["reference"]))
    hypothesis = normalize(dict(version="ariad-ir/2", body=[dict(type="paragraph", id="b", content=[dict(type="text", text="abc")])]))
    assert score_fixture(reference, hypothesis).fidelity == 1.0
    assert score_fixture_v2(reference, hypothesis).fidelity == 0.5


def test_benchmark_parsers_accept_isolated_fidelity_evidence_paths():
    import argparse
    from ariad_bench import heavy, run
    assert run.build_parser().parse_args(["--fidelity-out", "/tmp/base.json"]).fidelity_out == Path("/tmp/base.json")
    parser = argparse.ArgumentParser()
    heavy.build_parser(parser)
    assert parser.parse_args(["--fidelity-out", "/tmp/heavy.json"]).fidelity_out == Path("/tmp/heavy.json")
