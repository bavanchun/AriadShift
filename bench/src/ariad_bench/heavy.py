"""Measure heavy engine edges and complete PDF/OCR/Office routes against authored truth."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shlex
import shutil
import statistics
import subprocess
import tempfile
import time
import tomllib
from dataclasses import asdict, replace
from datetime import datetime, timezone
from pathlib import Path

from ariad_bench.canonical import CanonicalDoc, normalize
from ariad_bench.metrics import aggregate_edge_metrics, measure_execution, score_fixture_v2 as score_fixture
from ariad_bench.references import INDEPENDENT_READBACK_FORMATS, editor_assertions_only, read_writer_output
from ariad_bench.fidelity import METRIC_VERSION, unavailable
from ariad_bench.run import find_binary, public_paths
from ariad_bench.visual import pdf_ssim

IR = "ariad-ir+json"
DOCLING = "docling+json"
OFFICE_PDF_INPUTS = {
    "docx", "xlsx", "pptx", "odt", "rtf", "doc", "xls", "ppt", "ods", "odp",
    "odg", "wpd", "wps", "pub", "vsd",
}


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
        self.attempted: list[dict] = []

    def execute(
        self, command: list[str], stdin: Path | None = None
    ) -> tuple[str, float, int]:
        self.attempted.append(dict(command=command))
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

    def convert(
        self, source: Path, target: str, engine: str | None = None
    ) -> tuple[Path, float, int]:
        """Measure a complete direct route, including the planner and host startup."""
        self.serial += 1
        output = self.scratch / f"convert-{self.serial}.{target}"
        command = [
            str(self.ashift), "convert", str(source), "--to", target,
            "-o", str(output), "--overwrite",
        ]
        if engine is not None:
            command.extend(["--engine", engine])
        _, wall, rss = self.execute(command)
        return output, wall, rss

    def readback(self, artifact: Path, target: str) -> Path | CanonicalDoc:
        if target in INDEPENDENT_READBACK_FORMATS:
            return read_writer_output(artifact, target, self.pandoc)
        if target == "pdf":
            return self.engine("pdf", artifact, target, IR)[0]
        if target in ("docx", "epub"):
            return self.engine("pandoc", artifact, target, IR)[0]
        if target == IR:
            return artifact
        return self.hook("__ir", artifact)[0]

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
        if target in OFFICE_PDF_INPUTS:
            return self.engine("libreoffice", artifact, target, "pdf")[0], "libreoffice"
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
        self.attempted.append(dict(fixture=fixture["id"], engine=engine, target=target, ocr=ocr, route=route))
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
        # Plain text and CSV do not carry a faithful page layout. Do not create
        # a misleading SSIM score by forcing them through an unavailable reader.
        if (target == "pdf" or route) and target not in ("txt", "csv"):
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
        path = self.root / fixture["path"]
        if engine == "ariad-pdf":
            if (source, target) != ("cbz", "pdf"):
                raise ValueError(f"Unsupported in-process PDF edge: {source}->{target}")
            runs = [self.convert(path, target) for _ in range(self.repeat)]
        elif engine == "ariad-core":
            runs = [
                self.hook("__ir" if target == IR else "__write", path, target)
                for _ in range(self.repeat)
            ]
        else:
            runs = [
                self.engine(engine, path, source, target, ocr)
                for _ in range(self.repeat)
            ]
        artifact = runs[-1][0]
        mapper_runs = None
        if target == DOCLING:
            mapper_runs = [self.hook("__ir", artifact) for _ in range(self.repeat)]
            ir = mapper_runs[-1][0]
        else:
            ir = self.readback(artifact, target)
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
        if engine == "ariad-pdf" or (
            engine == "libreoffice"
            and (fixture["format"] in OFFICE_PDF_INPUTS or fixture["format"] == "pdf")
        ):
            if engine == "ariad-pdf" and (fixture["format"], target) != ("cbz", "pdf"):
                raise ValueError("Only cbz->pdf is an in-process PDF route")
            runs = [
                self.convert(
                    self.root / fixture["path"], target,
                    "libreoffice" if engine == "libreoffice" else None,
                )
                for _ in range(self.repeat)
            ]
            self.record(fixture, engine, target, ocr, runs,
                        self.readback(runs[-1][0], target), route=True)
            return
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
            elif target in ("docx", "epub", "odt", "rtf"):
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
        ir = self.readback(artifact, target)
        self.record(fixture, engine, target, ocr, runs, ir, route=True)


def run(root: Path, ashift: Path, pandoc: Path, repeat: int, out: Path, evidence: Path, fidelity_out: Path | None = None):
    if repeat < 1:
        raise ValueError("--repeat must be positive")
    if (fidelity_out is not None
        and fidelity_out.resolve() != (root / "bench/results/fidelity-v2/heavy.json").resolve()
        and evidence.resolve() == (root / "bench/results/heavy-routes.json").resolve()):
        evidence = fidelity_out
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
        try:
            for fixture in fixtures:
                fmt, scan = fixture["format"], "scan" in fixture["tags"]
                if fmt in ("pdf", "png"):
                    if fmt == "pdf" and not scan:
                        bench.measure(fixture, "pdf", IR)
                        bench.route(fixture, "pdf", "markdown")
                        for target in ("odt", "rtf", "txt", "xlsx"):
                            bench.route(fixture, "pdf", target)
                        bench.measure(fixture, "libreoffice", "pptx")
                        bench.route(fixture, "libreoffice", "pptx")
                    for ocr in ["tesseract", "rapidocr"] if scan else ["off"]:
                        bench.measure(fixture, "docling", DOCLING, ocr)
                        bench.route(fixture, "docling", "markdown", ocr)
                        if fmt == "pdf":
                            bench.route(fixture, "docling", "xlsx", ocr)
                        if scan:
                            bench.route(fixture, "docling", "docx", ocr)
                elif fmt in ("pptx", "xlsx", "docx"):
                    bench.measure(fixture, "docling", DOCLING)
                    if fmt != "docx":
                        bench.route(fixture, "docling", "markdown")
                    bench.measure(fixture, "libreoffice", "pdf")
                    if fmt == "docx":
                        bench.route(fixture, "typst", "pdf")
                elif fmt in OFFICE_PDF_INPUTS:
                    bench.measure(fixture, "libreoffice", "pdf")
                    if fmt in ("odt", "doc", "rtf"):
                        bench.measure(fixture, "libreoffice", "docx")
                elif fmt in ("txt", "csv", "eml", "msg", "svg"):
                    bench.measure(fixture, "ariad-core", IR)
                    bench.route(fixture, "typst", "pdf")
                elif fmt == "cbz":
                    bench.measure(fixture, "ariad-pdf", "pdf")
                    bench.route(fixture, "ariad-pdf", "pdf")
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
        except (RuntimeError, ValueError, OSError) as error:
            detailed_path = fidelity_out or root / "bench/results/fidelity-v2/heavy.json"
            detailed_path.parent.mkdir(parents=True, exist_ok=True)
            detailed_path.write_text(json.dumps(public_paths(dict(metric_version=METRIC_VERSION,
                attempted=bench.attempted, samples=bench.samples,
                failure=unavailable(str(error))), root, ashift), indent=2) + "\n", encoding="utf-8")
            raise

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
                truth = json.loads(
                    (root / companions[0]["path"]).read_text(encoding="utf-8")
                )
                if not truth.get("unscored", False) and not editor_assertions_only(truth):
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
            "metric_version": METRIC_VERSION,
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
            "attempted": bench.attempted,
            "samples": bench.samples,
        }
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.write_text(json.dumps(public_paths(measured, root, ashift), indent=2) + "\n", encoding="utf-8")
        detailed_path = fidelity_out or root / "bench/results/fidelity-v2/heavy.json"
        detailed_path.parent.mkdir(parents=True, exist_ok=True)
        detailed_path.write_text(json.dumps(public_paths(measured, root, ashift), indent=2) + "\n", encoding="utf-8")
        out.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        return data


def build_parser(parser: argparse.ArgumentParser):
    parser.add_argument("--ashift", type=Path)
    parser.add_argument("--pandoc", type=Path)
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--fidelity-out", type=Path, default=Path(__file__).resolve().parents[3] / "bench/results/fidelity-v2/heavy.json")
    parser.add_argument(
        "--out", type=Path, default=Path(__file__).resolve().parents[3] / "crates/ariad-core/data/capabilities.json"
    )
    parser.add_argument(
        "--evidence", type=Path, default=Path(__file__).resolve().parents[3] / "bench/results/heavy-routes.json"
    )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    build_parser(parser)
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[3]
    try:
        ashift = find_binary(
            "ashift",
            args.ashift,
            [root / "target" / "release" / "ashift", root / "target" / "debug" / "ashift"],
        )
        pandoc = find_binary("pandoc", args.pandoc, [root / ".tools/pandoc/bin/pandoc"])
        run(root, ashift, pandoc, args.repeat, args.out, args.evidence, args.fidelity_out)
    except (RuntimeError, ValueError, OSError) as error:
        parser.exit(1, f"{error}\n")
    return 0


if __name__ == "__main__":
    import sys
    raise SystemExit(main(sys.argv[1:]))
