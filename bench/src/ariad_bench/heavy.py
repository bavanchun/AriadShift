"""Measure heavy engine edges and complete PDF/OCR/Office routes against authored truth."""

from __future__ import annotations

import argparse
from dataclasses import asdict, replace
from datetime import datetime, timezone
import hashlib
import platform
import json
import os
from pathlib import Path
import shlex
import shutil
import statistics
import subprocess
import tempfile
import time
import tomllib

from ariad_bench.canonical import CanonicalDoc, normalize
from ariad_bench.metrics import aggregate_edge_metrics, measure_execution, score_fixture
from ariad_bench.run import find_binary
from ariad_bench.visual import pdf_ssim

IR = "ariad-ir+json"
DOCLING = "docling+json"


class Bench:
    def __init__(
        self, root: Path, ashift: Path, pandoc: Path, scratch: Path, repeat: int
    ):
        self.root, self.ashift, self.scratch, self.repeat = (
            root,
            ashift,
            scratch,
            repeat,
        )
        self.pandoc = pandoc
        self.env = os.environ.copy()
        self.env.update(ASHIFT_PANDOC=str(pandoc), SOURCE_DATE_EPOCH="1700000000")
        self.command = shlex.split(self.env.get("ASHIFT_DOCLING_ENGINE", ""))
        self.serial = 0
        self.edge_scores: dict[tuple[str, str, str], list] = {}
        self.samples: list[dict] = []

    def execute(
        self, command: list[str], stdin: Path | None = None
    ) -> tuple[str, float, int]:
        code, stdout, stderr, wall, rss = measure_execution(
            command,
            env=self.env,
            stdin_path=stdin,
            timeout_s=300,
            sample_interval_s=0.0005,
        )
        if code:
            detail = stderr.strip() or stdout.strip()
            raise RuntimeError(
                f"Command failed ({code}, {Path(command[0]).name} {' '.join(command[1:3])}): {detail[-4000:]}"
            )
        if rss is None:
            raise RuntimeError(f"No memory sample for {command[0]}")
        return stdout, wall, rss

    def engine(
        self, engine: str, path: Path, source: str, target: str, ocr: str = "off"
    ) -> tuple[Path, float, int]:
        self.serial += 1
        workspace = self.scratch / str(self.serial)
        for directory in ("in", "out", "tmp", "log"):
            (workspace / directory).mkdir(parents=True)
        copied = workspace / "in" / path.name
        shutil.copyfile(path, copied)
        options = {}
        if engine == "docling":
            if not self.command:
                raise ValueError(
                    "Set ASHIFT_DOCLING_ENGINE to the pack interpreter followed by -I -m ariad_docling"
                )
            options["ocr"] = ocr
            for key, variable in [
                ("artifacts_path", "ARIAD_BENCH_MODELS"),
                ("tessdata_path", "ARIAD_BENCH_TESSDATA"),
            ]:
                if variable in self.env:
                    options[key] = self.env[variable]
        request = {
            "protocol": "ariad-engine/1",
            "job": "benchmark",
            "op": "convert",
            "input": {"path": str(copied), "format": source},
            "output": {"dir": str(workspace / "out"), "format": target},
            "work_dir": str(workspace / "tmp"),
            "options": options,
            "limits": {"timeout_s": 300},
        }
        stdin = workspace / "request.jsonl"
        stdin.write_text(json.dumps(request) + "\n", encoding="utf-8")
        command = (
            self.command
            if engine == "docling"
            else [str(self.ashift), "__engine", engine]
        )
        stdout, wall, rss = self.execute(command, stdin)
        events = [json.loads(line) for line in stdout.splitlines()]
        results = [event for event in events if event.get("type") == "result"]
        if len(results) != 1 or events[-1] != results[0] or not results[0].get("ok"):
            raise RuntimeError(f"{engine} protocol failure: {events}")
        artifacts = [event for event in events if event.get("type") == "artifact"]
        if len(artifacts) != 1:
            raise RuntimeError(f"{engine} did not emit one artifact")
        artifact = Path(artifacts[0]["path"]).resolve()
        if (
            not artifact.is_relative_to((workspace / "out").resolve())
            or not artifact.is_file()
        ):
            raise RuntimeError(
                f"{engine} artifact escapes output directory or is absent"
            )
        return artifact, wall, rss

    def hook(
        self, verb: str, source: Path, target: str = IR
    ) -> tuple[Path, float, int]:
        self.serial += 1
        output = (
            self.scratch / f"hook-{self.serial}.{'json' if target == IR else target}"
        )
        command = [
            str(self.ashift),
            verb,
            str(source),
            "-o",
            str(output),
            "--overwrite",
        ]
        if verb == "__write":
            command.extend(["--to", target])
        _, wall, rss = self.execute(command)
        return output, wall, rss

    def docling_canonical(self, path: Path):
        """Interpret the source with Docling's own exporter, independently of our Rust mapper."""
        self.serial += 1
        markup = self.scratch / f"docling-source-{self.serial}.html"
        self.execute(
            [
                self.command[0],
                "-I",
                "-c",
                "from pathlib import Path; import sys; "
                "from docling_core.types.doc import DoclingDocument; "
                "doc = DoclingDocument.model_validate_json(Path(sys.argv[1]).read_bytes()); "
                "Path(sys.argv[2]).write_text(doc.export_to_html(), encoding='utf-8')",
                str(path),
                str(markup),
            ]
        )
        exported = subprocess.run(
            [str(self.pandoc), "-f", "html", "-t", "json", str(markup)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=60,
            check=True,
        )
        return normalize(json.loads(exported.stdout))

    def reference(self, fixture: dict):
        return normalize(
            self.root
            / next(
                c["path"]
                for c in fixture["companions"]
                if c["path"].endswith(".truth.json")
            )
        )

    def visual_pdf(self, artifact: Path, target: str) -> tuple[Path, str]:
        if target == "pdf":
            return artifact, "pdfium"
        if target == "docx":
            return self.engine("libreoffice", artifact, "docx", "pdf")[0], "libreoffice"
        ir = self.hook("__ir", artifact)[0]
        return self.engine("typst", ir, IR, "pdf")[0], "ariad-core+typst"

    def record(
        self,
        fixture: dict,
        engine: str,
        target: str,
        ocr: str,
        runs: list,
        ir: Path | CanonicalDoc,
        route: bool = False,
        reference: CanonicalDoc | None = None,
    ):
        score = score_fixture(
            self.reference(fixture) if reference is None else reference,
            normalize(ir),
            wall_ms=statistics.median(r[1] for r in runs),
            peak_rss_bytes=max(r[2] for r in runs),
        )
        sample = {
            "fixture": fixture["id"],
            "source": fixture["format"],
            "target": target,
            "engine": engine,
            "ocr": ocr,
            "route": route,
            "pages": fixture.get("pages", 1),
            **asdict(score),
        }
        if target == "pdf" or route:
            rendered, renderer = self.visual_pdf(runs[-1][0], target)
            sample["ssim"] = pdf_ssim(self.root / fixture["visual_reference"], rendered)
            sample["visual_profile"] = "faithful"
            sample["visual_renderer"] = renderer
        self.samples.append(sample)
        print(
            f"{fixture['id']} -> {target} ({engine}, {ocr}, route={route}): "
            f"CER={score.text_cer:.3f}, editability={score.editability:.3f}, {score.wall_ms:.0f}ms",
            flush=True,
        )
        return score

    def measure(self, fixture: dict, engine: str, target: str, ocr: str = "off"):
        source = fixture["format"]
        runs = [
            self.engine(engine, self.root / fixture["path"], source, target, ocr)
            for _ in range(self.repeat)
        ]
        artifact = runs[-1][0]
        mapper_runs = None
        if target == DOCLING:
            mapper_runs = [self.hook("__ir", artifact) for _ in range(self.repeat)]
            ir = mapper_runs[-1][0]
        elif target == "pdf":
            ir = self.engine("pdf", artifact, "pdf", IR)[0]
        elif target == "docx":
            ir = self.engine("pandoc", artifact, "docx", IR)[0]
        else:
            ir = artifact
        source_document = self.docling_canonical(artifact) if target == DOCLING else ir
        score = self.record(fixture, engine, target, ocr, runs, source_document)
        pages = max(1, fixture.get("pages", 1))
        # The graph describes default OCR (Tesseract without language hints).
        # RapidOCR is an explicit alternative and keeps its own route evidence.
        if ocr != "rapidocr":
            self.edge_scores.setdefault((source, target, engine), []).append(
                replace(score, wall_ms=score.wall_ms / pages)
            )
        if mapper_runs:
            mapped_score = self.record(
                {**fixture, "format": DOCLING},
                "ariad-core",
                IR,
                ocr,
                mapper_runs,
                ir,
                reference=source_document,
            )
            if ocr != "rapidocr":
                self.edge_scores.setdefault((DOCLING, IR, "ariad-core"), []).append(
                    replace(mapped_score, wall_ms=mapped_score.wall_ms / pages)
                )

    def route(self, fixture: dict, engine: str, target: str, ocr: str = "off"):
        """Cold complete route; exclude scoring read-back from job time and memory."""
        runs = []
        for _ in range(self.repeat):
            started = time.perf_counter()
            source = self.root / fixture["path"]
            stages = []
            if engine == "docling":
                stages.append(
                    self.engine(engine, source, fixture["format"], DOCLING, ocr)
                )
                stages.append(self.hook("__ir", stages[-1][0]))
            elif engine == "pdf":
                stages.append(self.engine(engine, source, "pdf", IR))
            elif fixture["format"] == "docx":
                stages.append(self.engine("pandoc", source, "docx", IR))
            else:
                stages.append(self.hook("__ir", source))
            if engine == "libreoffice":
                stages.append(self.engine("pandoc", stages[-1][0], IR, "docx"))
                stages.append(self.engine(engine, stages[-1][0], "docx", "pdf"))
            elif engine == "typst":
                stages.append(self.engine("typst", stages[-1][0], IR, "pdf"))
            elif target == "docx":
                stages.append(self.engine("pandoc", stages[-1][0], IR, target))
            else:
                stages.append(self.hook("__write", stages[-1][0], target))
            runs.append(
                (
                    stages[-1][0],
                    (time.perf_counter() - started) * 1000,
                    max(r[2] for r in stages),
                )
            )
        artifact = runs[-1][0]
        if target in ("pdf", "docx"):
            reader = "pdf" if target == "pdf" else "pandoc"
            ir = self.engine(reader, artifact, target, IR)[0]
        else:
            ir = self.hook("__ir", artifact)[0]
        self.record(fixture, engine, target, ocr, runs, ir, route=True)


def run(root: Path, ashift: Path, pandoc: Path, repeat: int, out: Path, evidence: Path):
    if repeat < 1:
        raise ValueError("--repeat must be positive")
    data = json.loads(out.read_text(encoding="utf-8"))
    records = tomllib.loads(
        (root / "fixtures/manifest.toml").read_text(encoding="utf-8")
    )["fixture"]
    fixtures = [record for record in records if "heavy-bench" in record.get("tags", [])]
    if not fixtures:
        raise ValueError("No authored heavy fixtures found")
    for fixture in fixtures:
        for item in [fixture, *fixture["companions"]]:
            path = root / item["path"]
            if hashlib.sha256(path.read_bytes()).hexdigest() != item["sha256"]:
                raise ValueError(f"Fixture hash mismatch: {item['path']}")
    with tempfile.TemporaryDirectory(
        prefix="ariad-heavy-", dir=os.environ.get("ARIAD_BENCH_TMPDIR")
    ) as scratch:
        bench = Bench(root, ashift, pandoc, Path(scratch).resolve(), repeat)
        for fixture in fixtures:
            fmt, scan = fixture["format"], "scan" in fixture["tags"]
            if fmt in ("pdf", "png"):
                if fmt == "pdf" and not scan:
                    bench.measure(fixture, "pdf", IR)
                    bench.route(fixture, "pdf", "markdown")
                for ocr in ["tesseract", "rapidocr"] if scan else ["off"]:
                    bench.measure(fixture, "docling", DOCLING, ocr)
                    bench.route(fixture, "docling", "markdown", ocr)
                    if scan:
                        bench.route(fixture, "docling", "docx", ocr)
            elif fmt in ("pptx", "xlsx", "docx"):
                bench.measure(fixture, "docling", DOCLING)
                if fmt != "docx":
                    bench.route(fixture, "docling", "markdown")
                bench.measure(fixture, "libreoffice", "pdf")
                if fmt == "docx":
                    bench.route(fixture, "typst", "pdf")
            elif fmt in ("odt", "ods", "odp", "doc", "rtf", "xls", "ppt"):
                bench.measure(fixture, "libreoffice", "pdf")
                if fmt in ("odt", "doc", "rtf"):
                    bench.measure(fixture, "libreoffice", "docx")
            elif fmt == "md":
                truth_path = root / next(
                    c["path"]
                    for c in fixture["companions"]
                    if c["path"].endswith(".truth.json")
                )
                path = Path(scratch) / f"{fixture['id']}.json"
                path.write_text(
                    json.dumps(
                        json.loads(truth_path.read_text(encoding="utf-8"))["ir"]
                    ),
                    encoding="utf-8",
                )
                bench.measure(
                    {**fixture, "format": IR, "path": str(path)}, "typst", "pdf"
                )
                bench.route(fixture, "typst", "pdf")
                bench.route(fixture, "libreoffice", "pdf")
        for edge in data["edges"]:
            key = (edge["from"], edge["to"], edge["engine"])
            if key in bench.edge_scores:
                edge["metrics"] = aggregate_edge_metrics(
                    bench.edge_scores[key]
                ).to_dict()
        data["generated_at"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        # Union counts avoid repeated heavy runs inflating the metadata.
        scored_ids = set()
        for fixture in records:
            companions = [
                c
                for c in fixture.get("companions", [])
                if c["path"].endswith(".truth.json")
            ]
            if companions:
                if not json.loads(
                    (root / companions[0]["path"]).read_text(encoding="utf-8")
                ).get("unscored", False):
                    scored_ids.add(fixture["id"])
            elif fixture["format"] in ("md", "markdown"):
                scored_ids.add(fixture["id"])
        data["bench"]["fixture_count"] = len(scored_ids)
        tools = {}
        for name, command in [
            (
                "docling",
                [
                    bench.command[0],
                    "-I",
                    "-c",
                    "import importlib.metadata; print(importlib.metadata.version('docling-slim'))",
                ],
            ),
            ("libreoffice", [os.environ.get("ASHIFT_SOFFICE", "soffice"), "--version"]),
        ]:
            tools[name] = subprocess.run(
                command, capture_output=True, text=True, check=True
            ).stdout.strip()
        with ashift.open("rb") as binary:
            binary_digest = hashlib.file_digest(binary, "sha256").hexdigest()
        pack = None
        if manifest_path := os.environ.get("ARIAD_BENCH_PACK_MANIFEST"):
            manifest = tomllib.loads(Path(manifest_path).read_text(encoding="utf-8"))
            pack = {
                "name": manifest["name"],
                "version": manifest["version"],
                "manifest_sha256": hashlib.sha256(
                    Path(manifest_path).read_bytes()
                ).hexdigest(),
                "components": [
                    {key: component[key] for key in ("name", "sha256", "size")}
                    for entry in manifest["targets"]
                    for component in entry["components"]
                ],
            }
        measured = {
            "generated_at": data["generated_at"],
            "pack": pack,
            "repeat": repeat,
            "tools": tools,
            "environment": {
                "platform": platform.system(),
                "architecture": platform.machine(),
                "cpu_count": os.cpu_count(),
                "python": platform.python_version(),
                "ashift_sha256": binary_digest,
            },
            "fixtures": [
                {
                    "id": f["id"],
                    "sha256": f["sha256"],
                    "truth": [
                        c for c in f["companions"] if c["path"].endswith(".truth.json")
                    ],
                }
                for f in fixtures
            ],
            "samples": bench.samples,
        }
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.write_text(json.dumps(measured, indent=2) + "\n", encoding="utf-8")
        out.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        return data


def build_parser(parser: argparse.ArgumentParser):
    parser.add_argument("--ashift", type=Path)
    parser.add_argument("--pandoc", type=Path)
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument(
        "--out", type=Path, default=Path("crates/ariad-core/data/capabilities.json")
    )
    parser.add_argument(
        "--evidence", type=Path, default=Path("bench/results/heavy-routes.json")
    )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    build_parser(parser)
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[3]
    ashift = find_binary(
        "ashift",
        args.ashift,
        [root / "target" / "release" / "ashift", root / "target" / "debug" / "ashift"],
    )
    pandoc = find_binary("pandoc", args.pandoc, [root / ".tools/pandoc/bin/pandoc"])
    try:
        run(root, ashift, pandoc, args.repeat, args.out, args.evidence)
    except (RuntimeError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    return 0
