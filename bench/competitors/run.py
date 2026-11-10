"""Bounded, local-only paired document conversions against pinned Stirling-PDF."""

from __future__ import annotations

import _thread
import argparse
from contextlib import contextmanager
from dataclasses import asdict, is_dataclass
from datetime import datetime, timezone
import hashlib
import importlib
import inspect
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener
from uuid import uuid4
import xml.etree.ElementTree as ET
from zipfile import ZipFile

if __package__:
    from .loopback import LoopbackRelay
    from .report import PRODUCTS, ROUTES, SCHEMA, compare, overall_status, validate_evidence, validate_score
else:
    from loopback import LoopbackRelay
    from report import PRODUCTS, ROUTES, SCHEMA, compare, overall_status, validate_evidence, validate_score

ROOT = Path(__file__).resolve().parents[2]
LIMIT = 32 * 1024 * 1024
LABEL = "org.ariadshift.benchmark.owner"


class HarnessError(RuntimeError):
    """A concrete prerequisite, execution or evaluation failure."""


class HarnessAbort(BaseException):
    """Fatal deadline or signal: never turn cancellation into an ordinary attempt."""


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def load_config(path):
    data = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    image = data.get("image", "")
    if not re.fullmatch(r"stirlingtools/stirling-pdf:[0-9]+\.[0-9]+\.[0-9]+@sha256:[0-9a-f]{64}", image):
        raise ValueError("Use an official stable tag plus immutable SHA-256 digest")
    if data.get("endpoint") != "http://127.0.0.1:18711":
        raise ValueError("Only the dedicated loopback endpoint is allowed")
    if data.get("openapi_path") != "/v1/api-docs":
        raise ValueError("Use the installed image's verified OpenAPI endpoint")
    if data.get("platform") not in ("linux/amd64", "linux/arm64"):
        raise ValueError("An exact image platform is required")
    if data.get("routes") != list(ROUTES) or data.get("repeat") != 3:
        raise ValueError("Both routes and three repetitions are required")
    if data.get("ocr") != "off" or data.get("profile") != "editable":
        raise ValueError("Both products use digital documents with OCR off and editable conversion")
    for name, upper in (("total_timeout_s", 570), ("request_timeout_s", 120), ("startup_timeout_s", 180)):
        value = data.get(name)
        if type(value) is not int or not 0 < value <= upper:
            raise ValueError(f"Invalid bounded timeout: {name}")
    for route, endpoint in (("pdf->docx", "/api/v1/convert/pdf/word"), ("docx->pdf", "/api/v1/convert/file/pdf")):
        operation = data.get("operations", {}).get(route, {})
        if operation.get("path") != endpoint:
            raise ValueError("Only verified local conversion endpoints are allowed")
        expected = {"outputFormat": "docx"} if route == "pdf->docx" else {}
        if operation.get("options") != expected or operation.get("query") != {"useStirlingOfficeConvert": "false"}:
            raise ValueError("Explicit verified conversion options are required")
    if not data.get("verification", {}).get("source_url") or not data["verification"].get("license"):
        raise ValueError("Upstream release and licence provenance are required")
    return data


def terminate_process_tree(process):
    """Stop the process and every descendant, including grandchildren that ignore termination."""
    if hasattr(os, "killpg"):
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass
        # The leader can exit before a grandchild that ignores SIGTERM.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=2)
        return
    import psutil

    try:
        parent = psutil.Process(process.pid)
        victims = [*parent.children(recursive=True), parent]
    except psutil.NoSuchProcess:
        victims = []
    for victim in victims:
        try:
            victim.terminate()
        except psutil.NoSuchProcess:
            pass
    _, alive = psutil.wait_procs(victims, timeout=2)
    for victim in alive:
        try:
            victim.kill()
        except psutil.NoSuchProcess:
            pass
    psutil.wait_procs(alive, timeout=2)
    process.wait(timeout=2)


class Runner:
    def __init__(self, seconds):
        self.deadline = time.monotonic() + seconds
        self.commands = []

    def remaining(self, cap=120):
        remaining = min(cap, self.deadline - time.monotonic())
        if remaining <= 0:
            raise HarnessAbort("Harness total time limit exceeded")
        return remaining

    def run(self, command, timeout=120):
        """Terminate our process group, including grandchildren, on every abnormal exit."""
        limit = self.remaining(timeout)
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            started = time.monotonic()
            process = subprocess.Popen(command, stdout=stdout, stderr=stderr, start_new_session=True)
            record = {"command": [str(value) for value in command], "pid": process.pid, "timeout_s": limit}
            self.commands.append(record)
            try:
                if hasattr(signal, "setitimer"):
                    process.wait(timeout=limit)
                else:
                    # Short slices let the interpreter deliver interrupt_main on Windows.
                    while True:
                        remaining = (started + limit) - time.monotonic()
                        if remaining <= 0:
                            raise subprocess.TimeoutExpired(command, limit)
                        try:
                            process.wait(timeout=min(0.05, remaining))
                            break
                        except subprocess.TimeoutExpired:
                            if (started + limit) - time.monotonic() <= 0:
                                raise
            except BaseException as exc:
                terminate_process_tree(process)
                if isinstance(exc, subprocess.TimeoutExpired):
                    raise HarnessError(f"Timed out after {limit:.2f}s: {' '.join(map(str, command[:4]))}") from exc
                raise
            finally:
                record.update(elapsed_ms=(time.monotonic() - started) * 1000, exit_code=process.returncode)
            stdout.seek(0)
            stderr.seek(0, 2)
            length = stderr.tell()
            stderr.seek(max(0, length - 2000))
            error = stderr.read().decode("utf-8", "replace").strip()
            if process.returncode:
                raise HarnessError(f"{command[0]} exited {process.returncode}: {error}")
            result = stdout.read(LIMIT + 1)
            if len(result) > LIMIT:
                raise HarnessError("Command output exceeds evidence limit")
            return result


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise HarnessError("Local conversion redirects are forbidden")


def request(config, runner, path, body=None, content_type=None):
    """Ignore proxy variables and reject redirects, including redirects off loopback."""
    headers = {"Content-Type": content_type} if content_type else {}
    req = Request(config["endpoint"] + path, data=body, headers=headers)
    try:
        with build_opener(ProxyHandler({}), NoRedirect()).open(req, timeout=runner.remaining(config["request_timeout_s"])) as response:
            content = response.read(LIMIT + 1)
            if len(content) > LIMIT:
                raise HarnessError("HTTP artifact exceeds limit")
            return content
    except HTTPError as exc:
        with exc:
            body = exc.read(8192).decode("utf-8", "replace")
        try:
            details = json.loads(body)
            reason = "; ".join(str(details[k]) for k in ("error", "message", "detail") if details.get(k))
        except (ValueError, AttributeError):
            reason = body[:1000]
        raise HarnessError(f"Stirling HTTP {exc.code} on {path}: {reason or 'no error detail'}") from exc


def validate_artifact(path, fmt):
    if not path.is_file() or not 0 < path.stat().st_size <= LIMIT:
        raise HarnessError("Missing, empty or oversized conversion output")
    if fmt == "pdf":
        import pypdfium2 as pdfium

        if not path.read_bytes().startswith(b"%PDF-"):
            raise HarnessError("Conversion did not return a PDF")
        with pdfium.PdfDocument(path) as document:
            if len(document) < 1:
                raise HarnessError("PDF has no pages")
    elif fmt == "docx":
        with ZipFile(path) as archive:
            entries = archive.infolist()
            if sum(entry.file_size for entry in entries) > LIMIT or len(entries) > 4096:
                raise HarnessError("DOCX expanded package exceeds limits")
            if archive.testzip() is not None:
                raise HarnessError("DOCX checksum failure")
            archive.read("[Content_Types].xml")
            tree = ET.fromstring(archive.read("word/document.xml"))
            if tree.tag != "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}document":
                raise HarnessError("Output is not a Word document")


class LocalStirling:
    def __init__(self, config, runner):
        self.config = config
        self.runner = runner
        self.token = uuid4().hex
        self.container = f"ariad-bench-stirling-{self.token[:12]}"
        self.network = f"ariad-bench-network-{self.token[:12]}"
        self.resources = []
        self.relay = None
        self.metadata = {}

    def docker(self, *args, timeout=120):
        return self.runner.run(["docker", *args], timeout=timeout)

    def __enter__(self):
        try:
            if not shutil.which("docker"):
                raise HarnessError("Docker is not installed")
            docker_host = os.environ.get("DOCKER_HOST") if not os.environ.get("DOCKER_CONTEXT") else None
            docker_host = docker_host or json.loads(self.docker("context", "inspect", "--format", "{{json .Endpoints.docker.Host}}"))
            if not docker_host.startswith("unix://"):
                raise HarnessError("Competitor evaluation requires a local Docker Unix socket")
            with socket.socket() as port:
                port.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                port.bind(("127.0.0.1", 18711))
            runtime = json.loads(self.docker("version", "--format", "{{json .Server}}"))
            image = json.loads(self.docker("image", "inspect", self.config["image"]))[0]
            actual_platform = f"{image['Os']}/{image['Architecture']}"
            if actual_platform != self.config["platform"]:
                raise HarnessError("Installed image platform differs from the pin")
            self.metadata = {
                "reference": self.config["image"],
                "id": image["Id"],
                "platform": actual_platform,
                "repo_digests": image.get("RepoDigests", []),
                "docker_version": runtime["Version"],
                "license_labels": {
                    key: value
                    for key, value in (image["Config"].get("Labels") or {}).items()
                    if "license" in key.lower()
                },
            }
            self.resources.append(("network", self.network))
            self.docker("network", "create", "--internal", "--driver", "bridge", "--label", f"{LABEL}={self.token}", self.network)
            network = json.loads(self.docker("network", "inspect", self.network))[0]
            if network.get("Internal") is not True:
                raise HarnessError("Stirling network must block external egress")
            self.metadata["network_internal"] = True
            self.resources.append(("container", self.container))
            self.docker("create", "--name", self.container, "--label", f"{LABEL}={self.token}",
                        "--network", self.network,
                        "--memory", "3g", "--cpus", "2", "--pids-limit", "512", "--security-opt", "no-new-privileges",
                        "--env", "SECURITY_ENABLELOGIN=false", "--env", "SYSTEM_ENABLEANALYTICS=false",
                        "--env", "SYSTEM_ENABLEPOSTHOG=false", "--env", "SYSTEM_ENABLESCARF=false",
                        "--env", "SYSTEM_DEFAULTLOCALE=en-US", self.config["image"])
            self.docker("start", self.container)
            container = json.loads(self.docker("container", "inspect", self.container))[0]
            networks = container["NetworkSettings"]["Networks"]
            if set(networks) != {self.network}:
                raise HarnessError("Stirling must have exactly one isolated network")
            self.relay = LoopbackRelay((networks[self.network]["IPAddress"], 8080), self.config["request_timeout_s"])
            startup = time.monotonic() + self.config["startup_timeout_s"]
            last_error = "not ready"
            while time.monotonic() < startup:
                try:
                    api = request(self.config, self.runner, self.config["openapi_path"])
                    spec = json.loads(api)
                    for operation in self.config["operations"].values():
                        if "post" not in spec.get("paths", {}).get(operation["path"], {}):
                            raise HarnessError(f"Installed image does not expose {operation['path']}")
                    self.metadata["openapi_sha256"] = hashlib.sha256(api).hexdigest()
                    self.metadata["openapi_version"] = spec.get("info", {}).get("version")
                    return self
                except (URLError, TimeoutError, ConnectionError, json.JSONDecodeError) as exc:
                    last_error = str(exc)
                    time.sleep(min(1, self.runner.remaining(1)))
            raise HarnessError(f"Stirling startup failed: {last_error}")
        except BaseException as exc:
            try:
                self.close()
            except HarnessError as cleanup_error:
                raise HarnessError(f"{exc}; {cleanup_error}") from exc
            raise

    def convert(self, source, target, output):
        operation = self.config["operations"][f"{source.suffix[1:]}->{target}"]
        boundary = uuid4().hex
        chunks = []
        for name, value in operation["options"].items():
            chunks.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode())
        # Only generated names enter MIME headers, never an arbitrary fixture filename.
        media_type = "application/pdf" if source.suffix == ".pdf" else "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        chunks += [f'--{boundary}\r\nContent-Disposition: form-data; name="fileInput"; filename="source{source.suffix}"\r\nContent-Type: {media_type}\r\n\r\n'.encode(),
                   source.read_bytes(), f"\r\n--{boundary}--\r\n".encode()]
        path = operation["path"] + "?" + urlencode(operation["query"])
        content = request(self.config, self.runner, path, b"".join(chunks), f"multipart/form-data; boundary={boundary}")
        output.write_bytes(content)

    def close(self):
        """Use a fresh small cleanup budget even after the measurement deadline expires."""
        failures = []
        if self.relay is not None:
            self.relay.close()
            self.relay = None
        cleanup = Runner(25)
        cleanup.commands = self.runner.commands
        for kind, name in reversed(self.resources):
            try:
                records = json.loads(cleanup.run(["docker", kind, "inspect", name], timeout=5))
                record = records[0]
                labels = record.get("Config", {}).get("Labels", {}) if kind == "container" else record.get("Labels", {})
                if (labels or {}).get(LABEL) != self.token:
                    raise HarnessError(f"Refusing cleanup of resource not owned by this run: {name}")
                if kind == "container":
                    try:
                        cleanup.run(["docker", "stop", "--time", "3", name], timeout=10)
                    except HarnessError:
                        cleanup.run(["docker", "rm", "--force", name], timeout=5)
                    else:
                        cleanup.run(["docker", "rm", name], timeout=5)
                else:
                    cleanup.run(["docker", "network", "rm", name], timeout=5)
            except (HarnessError, HarnessAbort, OSError) as exc:
                if "No such" not in str(exc):
                    failures.append(str(exc))
        self.resources.clear()
        if failures:
            raise HarnessError("Resource cleanup failed: " + "; ".join(failures))

    def __exit__(self, kind, error, traceback):
        try:
            self.close()
        except HarnessError as cleanup_error:
            if error is not None:
                raise HarnessError(f"{error}; {cleanup_error}") from error
            raise


def ashift_convert(runner, binary, source, target, output):
    command = [str(binary), "convert", str(source), "--to", target, "--profile", "editable", "-o", str(output), "--overwrite"]
    if source.suffix == ".pdf":
        command += ["--engine", "pdf"]  # Digital-only policy, shared with Stirling's Office conversion.
    runner.run(command)


def readback(runner, binary, source, output):
    runner.run([str(binary), "__ir", str(source), "-o", str(output), "--overwrite"])
    return json.loads(output.read_text(encoding="utf-8"))


def load_scorer(spec):
    """Late-bind the accepted scorer; an adapter may translate its API, never its metric."""
    module_name, separator, function_name = spec.partition(":")
    if not separator or (
        module_name != "ariad_bench.fidelity" and not module_name.startswith("ariad_bench.")
    ):
        raise HarnessError("Scorer must be an installed ariad_bench module:function")
    try:
        module = importlib.import_module(module_name)
        function = getattr(module, function_name)
        inspect.signature(function).bind({}, {})
    except (ImportError, AttributeError, TypeError, ValueError) as exc:
        raise HarnessError(f"Accepted fidelity-v2 scorer unavailable ({spec}): {exc}") from exc

    def score(reference, hypothesis):
        value = function(reference, hypothesis)
        if is_dataclass(value):
            value = asdict(value)
        validate_score(value)
        return value

    return score, {"callable": spec, "sha256": sha256(module.__file__)}


def corpus(root):
    """Consume manifest-owned original PDF/DOCX pairs without regenerating fixtures."""
    manifest = tomllib.loads((root / "fixtures/manifest.toml").read_text(encoding="utf-8"))
    groups = {}
    for fixture in manifest["fixture"]:
        path = Path(fixture["path"])
        if not path.as_posix().startswith("fixtures/editor/") or path.suffix not in (".pdf", ".docx"):
            continue
        source = root / path
        if fixture.get("license") != "Apache-2.0" or sha256(source) != fixture["sha256"]:
            raise HarnessError(f"Authored corpus licence/hash mismatch: {path}")
        companions = [c for c in fixture.get("companions", []) if c["path"].endswith(".truth.json")]
        if len(companions) != 1:
            raise HarnessError(f"Exactly one independent truth companion required: {path}")
        truth = companions[0]
        if sha256(root / truth["path"]) != truth["sha256"]:
            raise HarnessError(f"Authored truth hash mismatch: {truth['path']}")
        groups.setdefault(path.with_suffix("").as_posix(), {})[path.suffix[1:]] = {
            "path": path.as_posix(), "sha256": fixture["sha256"], "truth": truth["path"], "truth_sha256": truth["sha256"],
        }
    if not groups:
        raise HarnessError("Accepted authored PDF/DOCX corpus under fixtures/editor is not available")
    for name, sources in groups.items():
        if set(sources) != {"pdf", "docx"}:
            raise HarnessError(f"Unpaired authored fixture: {name}; PDF and DOCX are both required")
    return [{"id": name, "sources": sources} for name, sources in sorted(groups.items())]


def paired_attempt(runner, service, binary, source, target, output):
    started = time.monotonic()
    try:
        if service is None:
            ashift_convert(runner, binary, source, target, output)
        else:
            service.convert(source, target, output)
        elapsed = (time.monotonic() - started) * 1000
        validate_artifact(output, target)
        return {"status": "converted", "conversion_ms": elapsed, "output_sha256": sha256(output)}
    except Exception as exc:
        failed = {"status": "failed", "reason": f"{type(exc).__name__}: {exc}", "conversion_ms": (time.monotonic() - started) * 1000}
        if output.is_file() and output.stat().st_size <= LIMIT:
            failed["output_sha256"] = sha256(output)
        return failed


def probe_routes(runner, service, binary, scratch):
    samples = []
    for route in ROUTES:
        source_format, target = route.split("->")
        source = ROOT / f"fixtures/heavy/en-map.{source_format}"
        validate_artifact(source, source_format)
        for product in PRODUCTS:
            output = scratch / f"probe-{product}-{source_format}.{target}"
            result = paired_attempt(runner, service if product == "stirling" else None, binary, source, target, output)
            samples.append({"route": route, "product": product, "sha256": sha256(source), **result})
    return samples


def measure(config, runner, service, binary, fixtures, scorer, scratch, out):
    samples = []
    for fixture in fixtures:
        for route in ROUTES:
            source_format, target = route.split("->")
            record = fixture["sources"][source_format]
            source = ROOT / record["path"]
            validate_artifact(source, source_format)
            reference = json.loads((ROOT / record["truth"]).read_text(encoding="utf-8"))
            extraction = time.monotonic()
            extraction_error = None
            try:
                readback(runner, binary, source, scratch / "source.json")
            except Exception as exc:
                extraction_error = str(exc)
            extraction_ms = (time.monotonic() - extraction) * 1000
            for attempt in range(1, config["repeat"] + 1):
                # Alternate ordering to reduce systematic warm/load bias.
                products = PRODUCTS if attempt % 2 else reversed(PRODUCTS)
                for product in products:
                    number = len(samples) + 1
                    output = scratch / f"output-{number}.{target}"
                    result = paired_attempt(runner, service if product == "stirling" else None, binary, source, target, output)
                    row = {"fixture": fixture["id"], "route": route, "product": product, "attempt": attempt,
                           "sha256": record["sha256"], "truth_sha256": record["truth_sha256"],
                           "extraction_ms": extraction_ms, "evaluation_ms": 0.0, **result}
                    if extraction_error:
                        row["extraction_reason"] = extraction_error
                    if result["status"] == "converted":
                        evaluation = time.monotonic()
                        try:
                            hypothesis = readback(runner, binary, output, scratch / f"readback-{number}.json")
                            row.update(status="ok", score=scorer(reference, hypothesis))
                        except Exception as exc:
                            row.update(status="unavailable", reason=f"Read-back/scoring: {type(exc).__name__}: {exc}")
                        row["evaluation_ms"] = (time.monotonic() - evaluation) * 1000
                    samples.append(row)
                    write_json(out / f"attempt-{number:04}.json", public_evidence(row, binary))
    return samples


def write_json(path, data):
    content = json.dumps(data, indent=2, sort_keys=True, allow_nan=False) + "\n"
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=path.name, suffix=".tmp", encoding="utf-8", delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(content)
            stream.close()
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)


def public_evidence(data, binary):
    """Remove machine/user path identifiers without altering measured outcomes."""
    replacements = ((str(ROOT), "${WORKTREE}"), (str(binary), "${ASHIFT_BIN}"), (str(Path.home()), "${HOME}"))

    def redact(value):
        if isinstance(value, dict):
            return {key: redact(item) for key, item in value.items()}
        if isinstance(value, list):
            return [redact(item) for item in value]
        if isinstance(value, str):
            for original, placeholder in replacements:
                value = value.replace(original, placeholder)
                # Keep redacted paths POSIX-style on every platform.
                value = re.sub(re.escape(placeholder) + r"[^\s\"']*", lambda m: m.group(0).replace("\\", "/"), value)
        return value

    result = redact(data)
    result["metadata_redactions"] = {
        "WORKTREE": "repository checkout", "ASHIFT_BIN": "explicit hashed conversion binary", "HOME": "user home directory",
    }
    return result


def artifact_directory(out):
    directory = out / "runs" / uuid4().hex
    directory.mkdir(parents=True)
    return directory


@contextmanager
def interrupt_cleanup(seconds=None):
    def interrupt(signum, _):
        raise HarnessAbort(f"Interrupted by signal {signum}")
    signals = tuple(s for name in ("SIGTERM", "SIGINT", "SIGALRM") if (s := getattr(signal, name, None)) is not None)
    previous = {s: signal.signal(s, interrupt) for s in signals}
    timer = None
    if seconds is not None:
        if hasattr(signal, "setitimer"):
            signal.setitimer(signal.ITIMER_REAL, seconds)
        else:
            timer = threading.Timer(seconds, lambda: _thread.interrupt_main(signal.SIGINT))
            timer.daemon = True
            timer.start()
    try:
        yield
    finally:
        if seconds is not None:
            if timer is not None:
                timer.cancel()
            else:
                signal.setitimer(signal.ITIMER_REAL, 0)
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--ashift", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--preflight-only", action="store_true")
    parser.add_argument("--scorer", help="Accepted fidelity-v2 adapter as ariad_bench.module:function")
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--result-name", default="results.json", choices=("results.json", "stirling-v3.3.0.json"))
    args = parser.parse_args(argv)
    if args.repeat != 3:
        parser.error("The paired acceptance benchmark requires three repetitions")
    evidence = {"schema": SCHEMA, "kind": "preflight" if args.preflight_only else "corpus",
                "generated_at": datetime.now(timezone.utc).isoformat(), "status": "blocked", "gaps": [], "samples": []}
    args.out.mkdir(parents=True, exist_ok=True)
    destination = args.out / ("preflight.json" if args.preflight_only else args.result_name)
    try:
        config = load_config(args.config)
        binary = args.ashift.resolve()
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise HarnessError(f"Executable AriadShift binary missing: {binary}")
        evidence.update(config_sha256=sha256(args.config), ashift_sha256=sha256(binary), routes=list(ROUTES), repeat=config["repeat"],
                        policy={"ocr": config["ocr"], "profile": config["profile"], "auxiliary_renderer": None},
                        environment={"os": platform.platform(), "architecture": platform.machine(), "python": platform.python_version()},
                        worktree=str(ROOT), source_revision=Runner(10).run(["git", "-C", str(ROOT), "rev-parse", "HEAD"]).decode().strip())
        scorer, fixtures = None, None
        try:
            scorer, evidence["scorer"] = load_scorer(args.scorer or config["scorer"])
        except HarnessError as exc:
            evidence["gaps"].append(str(exc))
        try:
            fixtures = corpus(ROOT)
        except (HarnessError, OSError, ValueError) as exc:
            evidence["gaps"].append(str(exc))
        runner = Runner(config["total_timeout_s"])
        evidence["commands"] = runner.commands
        service = LocalStirling(config, runner)
        evidence["resources"] = {"container": service.container, "network": service.network, "owner": service.token,
                                 "pid": os.getpid(), "endpoint": config["endpoint"], "worktree": str(ROOT)}
        with interrupt_cleanup(config["total_timeout_s"]), tempfile.TemporaryDirectory(prefix="ariad-competitors-") as temporary:
            with service:
                evidence["image"] = service.metadata
                evidence["probe_samples"] = probe_routes(runner, service, binary, Path(temporary))
                for sample in evidence["probe_samples"]:
                    if sample["status"] != "converted":
                        evidence["gaps"].append(f"{sample['product']} {sample['route']}: {sample['reason']}")
                if not args.preflight_only and not evidence["gaps"]:
                    evidence["fixtures"] = fixtures
                    artifacts = artifact_directory(args.out)
                    evidence["artifact_directory"] = artifacts.relative_to(args.out).as_posix()
                    evidence["samples"] = measure(config, runner, service, binary, fixtures, scorer, Path(temporary), artifacts)
                    evidence["comparisons"] = {route: compare(evidence, route) for route in ROUTES}
                    evidence["status"] = overall_status(evidence)
                elif args.preflight_only and not evidence["gaps"]:
                    evidence["status"] = "ready"
        evidence["cleanup"] = "completed"
        if "fixtures" in evidence:
            validate_evidence(evidence)
    except (Exception, HarnessAbort) as exc:
        evidence["status"] = "blocked"
        if "service" in locals():
            evidence["image"] = service.metadata
        evidence["gaps"].append(f"{type(exc).__name__}: {exc}")
    write_json(destination, public_evidence(evidence, args.ashift.resolve()))
    print(f"{evidence['status']}: {destination}")
    for gap in evidence["gaps"]:
        print(gap, file=sys.stderr)
    return 0 if evidence["status"] in ("ready", "pass") else 2


if __name__ == "__main__":
    raise SystemExit(main())
