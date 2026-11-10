"""Offline raster watermark removal through ariad-engine/2."""

from __future__ import annotations

import math
from pathlib import Path
from typing import IO, Any

from ariad_inpaint import __version__
from ariad_inpaint.config import pack_options
from ariad_inpaint.pages import selected_pages
from ariad_inpaint.pdf import clean_page, open_pdf
from ariad_inpaint.pixels import png
from ariad_inpaint.protocol import (
    DescribeRequest,
    EngineError,
    RunToolRequest,
    Writer,
    parse_request,
    read_line,
)


def capabilities() -> dict[str, Any]:
    return {
        "type": "capabilities",
        "protocol": "ariad-engine/2",
        "engine": "inpaint",
        "version": __version__,
        "tool": {"name": "inpaint", "version": __version__, "status": "found"},
        "license": "Apache-2.0 AND MIT",
        "routes": [],
        "tools": [{"id": "pdf.watermark_remove_raster", "version": "1"}],
        "enforces_memory_limit": False,
        "supports_session": False,
    }


def validate(params: dict[str, Any], total: int) -> tuple[str, float, Any, bool]:
    method = params.get("method", "telea")
    radius = params.get("radius") if params.get("radius") is not None else 3.0
    preview = params.get("preview", True)
    if method not in {"telea", "ns"}:
        raise EngineError("invalid_request", "method must be telea or ns")
    if type(radius) not in {int, float} or not math.isfinite(radius) or not 1 <= radius <= 25:
        raise EngineError("invalid_request", "radius must be finite and between 1 and 25")
    if not isinstance(preview, bool):
        raise EngineError("invalid_request", "preview must be boolean")
    mask = params.get("mask")
    if mask is None:
        mask = "auto"
    if mask != "auto":
        if not isinstance(mask, list) or not mask or len(mask) > 10_000:
            raise EngineError("invalid_request", "mask must be auto or 1 to 10000 rectangles")
        for rect in mask:
            if not isinstance(rect, dict) or set(rect) - {"page", "x", "y", "width", "height"}:
                raise EngineError("invalid_request", "invalid mask rectangle")
            values = [rect.get(key) for key in ("x", "y", "width", "height")]
            if any(type(v) not in {int, float} or not math.isfinite(v) for v in values):
                raise EngineError("invalid_request", "rectangle coordinates must be finite")
            if any(v < 0 for v in values[:2]) or any(v <= 0 for v in values[2:]):
                raise EngineError("invalid_request", "rectangle size must be positive")
            page = rect.get("page")
            if page is not None and (type(page) is not int or not 1 <= page <= total):
                raise EngineError("invalid_request", "mask page is out of bounds")
    return method, float(radius), mask, preview


def handle(request: RunToolRequest | DescribeRequest, writer: Writer) -> None:
    try:
        if isinstance(request, DescribeRequest):
            writer.emit(capabilities())
            writer.ok()
            return
        if request.tool != "pdf.watermark_remove_raster":
            raise EngineError("unsupported_route", "unsupported inpaint tool")
        if len(request.inputs) != 1 or request.inputs[0].get("format", "pdf") != "pdf":
            raise EngineError("invalid_request", "raster inpainting requires one PDF input")
        source = Path(request.inputs[0]["path"])
        if source.stat().st_size > 256 * 1024 * 1024:
            raise EngineError("limit_exceeded", "PDF input exceeds 256 MiB")
        document = open_pdf(str(source), request.limits)
        selected = selected_pages(request.params.get("pages"), len(document.pages))
        method, radius, mask, preview = validate(request.params, len(document.pages))
        directory = Path(request.output_dir).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        options = pack_options(request.options)
        regions = 0
        completed = []
        previews = []
        backends = set()
        for number in sorted(selected):
            rectangles = (
                mask
                if mask == "auto"
                else [rect for rect in mask if rect.get("page") in {None, number}]
            )
            if not rectangles:
                continue
            writer.progress("inpaint", number, len(document.pages))
            try:
                before, after, detected, backend, vector = clean_page(
                    document,
                    document.pages[number - 1],
                    rectangles,
                    method,
                    radius,
                    options.get("backend", "auto"),
                )
            except EngineError as error:
                if (
                    error.code == "unsupported_route"
                    and str(error) == "page has no supported raster XObjects"
                ):
                    writer.warning("no_raster", f"Page {number} contains no raster XObjects.")
                    continue
                raise
            regions += detected
            if detected:
                completed.append(number)
            backends.add(backend)
            if preview:
                before_path = directory / f"before_page_{number:03}.png"
                after_path = directory / f"after_page_{number:03}.png"
                png(before_path, before)
                png(after_path, after)
                writer.artifact(str(before_path), "png")
                writer.artifact(str(after_path), "png")
                previews.append(
                    {"page": number, "before": str(before_path), "after": str(after_path)}
                )
                if vector:
                    writer.warning(
                        "raster_layer_preview",
                        f"Page {number} previews show raster layers; "
                        "vector content is preserved in PDF.",
                    )
        path = directory / "cleaned.pdf"
        document.compress_identical_objects(remove_duplicates=True, remove_unreferenced=True)
        document.write(path)
        writer.artifact(str(path), "pdf")
        writer.ok(
            {
                "pages_inpainted": completed,
                "regions_detected": regions,
                "method": method,
                "previews": previews,
                "backends": sorted(backends),
            }
        )
    except EngineError as error:
        writer.error(error)
    except OSError:
        writer.error(EngineError("io", "could not read or write a local engine file"))
    except ValueError, TypeError, KeyError:
        writer.error(EngineError("invalid_request", "invalid PDF or inpainting parameters"))
    except Exception:
        writer.error(EngineError("engine_failure", "local raster inpainting failed"))


def main(protocol_fd: int, stdin: IO[bytes]) -> int:
    writer = Writer(protocol_fd)
    try:
        line = read_line(stdin)
        if line is not None:
            handle(parse_request(line), writer)
    except EngineError as error:
        writer.error(error)
    return 0
