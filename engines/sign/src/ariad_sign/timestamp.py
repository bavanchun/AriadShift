"""Document timestamping implementation using pyHanko and RFC 3161."""

from __future__ import annotations

import base64
from pathlib import Path

from ariad_sign.protocol import EngineError, RunToolRequest, Writer
from ariad_sign.sign import HostBrokeredTimeStamper, PreComputedTimeStamper

try:
    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.sign import PdfTimeStamper

    PYHANKO_AVAILABLE = True
except ImportError:
    PYHANKO_AVAILABLE = False


def run_timestamp(request: RunToolRequest, writer: Writer) -> None:
    """Executes a `pdf.timestamp` request."""
    if not PYHANKO_AVAILABLE:
        raise EngineError("tool_missing", "pyHanko is not installed or available")

    in_path = Path(request.inputs[0]["path"])
    if not in_path.is_file():
        raise EngineError("io", f"input file does not exist: {in_path}")

    params = request.params
    field_name = params.get("field_name")
    tsa_url = params.get("tsa_url")
    timestamp_token = params.get("timestamp_token")
    work_dir = Path(request.work_dir)

    if timestamp_token:
        try:
            token_bytes = base64.b64decode(timestamp_token)
        except Exception as exc:
            raise EngineError("invalid_request", f"invalid base64 timestamp_token: {exc}") from exc
        timestamper = PreComputedTimeStamper(token_bytes)
    elif tsa_url:
        timestamper = HostBrokeredTimeStamper(work_dir, tsa_url, writer)
    else:
        raise EngineError(
            "invalid_request",
            "either tsa_url or timestamp_token is required for pdf.timestamp",
        )

    writer.progress("pdf.timestamp", 0, 1)

    stem = in_path.stem
    out_filename = f"{stem}.timestamped.pdf"
    out_dir = Path(request.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / out_filename

    try:
        with open(in_path, "rb") as inf:
            reader = PdfFileReader(inf, strict=False)
            if reader.encrypted:
                raise EngineError("encrypted_input", "cannot timestamp encrypted PDF")

            pdf_out = IncrementalPdfFileWriter(inf)
            pdf_timestamper = PdfTimeStamper(
                timestamper=timestamper,
                field_name=field_name,
            )
            with open(out_path, "wb") as outf:
                pdf_timestamper.timestamp_pdf(pdf_out, md_algorithm="sha256", output=outf)
    except EngineError:
        raise
    except Exception as exc:
        raise EngineError("engine_failure", f"document timestamping failed: {exc}") from exc

    writer.progress("pdf.timestamp", 1, 1)
    writer.artifact(str(out_path), "pdf")
    writer.ok(
        {
            "field_name": field_name or "Timestamp",
        }
    )
