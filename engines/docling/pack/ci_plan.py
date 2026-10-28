"""Plan pack CI omissions explicitly and assemble only its completed targets."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from pathlib import Path

import fetch
import native_build

LINUX = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")
MACOS = "aarch64-apple-darwin"
MISSING_MACOS_PIN = (
    "macOS pack requires the video-disabled OpenCV wheel from toolchain-wheels-v*; pin missing"
)
TARGETS = {
    LINUX[0]: dict(
        os="ubuntu-26.04",
        exe="",
        primary=True,
        input="fixtures/scan/vi-garden-300dpi.png",
        expect="Khu vườn đọc sách",
    ),
    LINUX[1]: dict(os="ubuntu-26.04", exe="", primary=False, input="", expect=""),
    MACOS: dict(
        os="macos-26",
        exe="",
        primary=False,
        input="fixtures/pdf/en-map-room.pdf",
        expect="Notes from the Map Room",
    ),
}


def validate_selection(result):
    targets = result["targets"]
    skipped = set(result["skipped"])
    allowed = set(result["allowed_skips"])
    if allowed - {MACOS}:
        raise SystemExit(
            "explicit omissions may name only aarch64-apple-darwin; Linux is mandatory"
        )
    if (
        len(targets) != len(set(targets))
        or not set(LINUX) <= set(targets)
        or set(targets) - set(TARGETS)
        or skipped != set(TARGETS) - set(targets)
    ):
        raise SystemExit(
            "pack plan must retain both Linux targets and declare every omitted target"
        )
    if result["release"] and skipped - allowed:
        raise SystemExit(f"{MISSING_MACOS_PIN}; release requires an explicit macOS omission")


def plan(pins, event, ref_name="", allowed_skips=""):
    if event not in {"pull_request", "push", "workflow_dispatch"}:
        raise SystemExit(f"unsupported pack CI event: {event}")
    if event == "push":
        if not re.fullmatch(r"pack-docling-v[0-9][A-Za-z0-9.+-]*", ref_name):
            raise SystemExit("release push must name an exact pack-docling-v* tag")
        allowed = pins.get("pack_release_skips", {}).get(ref_name, [])
        if not isinstance(allowed, list):
            raise SystemExit("release tag omissions must be a list of exact targets")
    else:
        allowed = [value for value in re.split(r"[\s,]+", allowed_skips) if value]
    if len(allowed) != len(set(allowed)):
        raise SystemExit("duplicate explicit pack omissions")
    selected = list(LINUX)
    inputs = native_build.release_inputs(
        "opencv", MACOS, pins, native_build.cache_key("opencv", MACOS, pins)
    )
    if inputs is not None:
        if not pins["toolchain"]["release"].startswith("toolchain-wheels-v"):
            raise SystemExit("macOS OpenCV requires a versioned toolchain-wheels-v* release")
        wheels = [name for name in inputs if name.endswith(".whl")]
        if len(wheels) != 1 or not wheels[0].startswith(
            f"opencv_python_headless-{pins['opencv']['version']}-"
        ):
            raise SystemExit("macOS OpenCV pin must contain exactly one matching headless wheel")
        selected.append(MACOS)
    result = dict(
        schema=1,
        release=event != "pull_request",
        targets=selected,
        skipped={} if MACOS in selected else {MACOS: MISSING_MACOS_PIN},
        allowed_skips=sorted(allowed),
    )
    validate_selection(result)
    return result


def assemble(out, selected, ashift):
    import build

    validate_selection(selected)
    targets = selected["targets"]
    expected = {f"build-{target}.json" for target in targets}
    if {p.name for p in out.glob("build-*.json")} != expected:
        raise SystemExit("pack artifacts do not exactly match the planned built targets")
    records = []
    for target in targets:
        record = build.load_build(out, target)
        if record.triple != target:
            raise SystemExit("pack record triple differs from its planned target")
        names = {
            "runtime": f"docling-{record.version}-{target}-runtime.tar.zst",
            "models": f"docling-{record.version}-models.tar.zst",
            "tessdata": f"docling-{record.version}-tessdata.tar.zst",
        }
        if record.files != names or any(not (out / name).is_file() for name in names.values()):
            raise SystemExit("pack record has missing or unexpected component files")
        records.append(record)
    build.write_manifest(ashift, out, records)
    files = {"pack.toml", *(name for record in records for name in record.files.values())}
    checksums = []
    for name in sorted(files):
        with (out / name).open("rb") as handle:
            checksums.append(f"{hashlib.file_digest(handle, 'sha256').hexdigest()}  {name}")
    (out / "SHA256SUMS").write_text("\n".join(checksums) + "\n", encoding="utf-8")
    (out / "pack-ci-plan.json").write_text(json.dumps(selected, indent=2) + "\n", encoding="utf-8")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["plan", "manifest"])
    parser.add_argument("--out", type=Path)
    parser.add_argument("--ashift", default=os.environ.get("ASHIFT_BIN", "ashift"))
    args = parser.parse_args(argv)
    try:
        selected = plan(
            fetch.load_pins(),
            os.environ.get("GITHUB_EVENT_NAME", "pull_request"),
            os.environ.get("GITHUB_REF_NAME", ""),
            os.environ.get("ALLOW_MISSING_TARGETS", ""),
        )
    except SystemExit as error:
        print(f"::error::{error}", flush=True)
        raise
    if args.command == "manifest":
        if args.out is None or json.loads(os.environ["PACK_CI_PLAN"]) != selected:
            raise SystemExit("manifest plan differs from the current CI policy")
        assemble(args.out, selected, args.ashift)
        return 0
    rows = ["| Target | Status | Reason |", "| --- | --- | --- |"]
    for target in TARGETS:
        reason = selected["skipped"].get(target, "")
        rows.append(f"| {target} | {'SKIPPED' if reason else 'BUILD'} | {reason} |")
        if reason:
            print(f"::warning title=Docling pack target skipped::{reason}", flush=True)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as handle:
            handle.write("\n".join(rows) + "\n")
    outputs = dict(
        plan=selected, matrix=[dict(target=t, **TARGETS[t]) for t in selected["targets"]]
    )
    if os.environ.get("GITHUB_OUTPUT"):
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as handle:
            for name, value in outputs.items():
                handle.write(f"{name}={json.dumps(value, separators=(',', ':'))}\n")
    print(json.dumps(selected, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
