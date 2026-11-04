"""PAdES digital signing implementation using pyHanko."""

from __future__ import annotations

import base64
from pathlib import Path
from typing import Any

from ariad_sign.protocol import EngineError, RunToolRequest, Writer

try:
    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.sign import fields, signers, timestamps
    from pyhanko.sign.fields import SigFieldSpec
    from pyhanko.sign.signers.pdf_signer import PdfSignatureMetadata, PdfSigner
    from pyhanko.stamp import TextStampStyle

    PYHANKO_AVAILABLE = True
except ImportError:
    PYHANKO_AVAILABLE = False


class PreComputedTimeStamper(timestamps.TimeStamper):
    """Timestamper that uses a pre-fetched RFC 3161 timestamp response token."""

    def __init__(self, token_bytes: bytes) -> None:
        super().__init__()
        self.token_bytes = token_bytes

    async def async_timestamp(self, message_digest: bytes, md_algorithm: str) -> bytes:
        return self.token_bytes

    def timestamp(self, message_digest: bytes, md_algorithm: str) -> bytes:
        return self.token_bytes


def run_sign(request: RunToolRequest, writer: Writer) -> None:
    """Executes a `pdf.sign` request."""
    if not PYHANKO_AVAILABLE:
        raise EngineError("tool_missing", "pyHanko is not installed or available")

    in_path = Path(request.inputs[0]["path"])
    if not in_path.is_file():
        raise EngineError("io", f"input file does not exist: {in_path}")

    # Check for encrypted PDF
    try:
        with open(in_path, "rb") as f:
            reader = PdfFileReader(f, strict=False)
            if reader.encrypted:
                raise EngineError(
                    "encrypted_input",
                    "input PDF is encrypted; decrypt explicitly before signing",
                )
    except EngineError:
        raise
    except Exception as exc:
        raise EngineError("io", f"failed to open input PDF: {exc}") from exc

    params = request.params
    level = str(params.get("level", "B-B")).upper()
    valid_levels = {"B-B", "B-T", "B-LT", "B-LTA"}
    if level not in valid_levels:
        raise EngineError("invalid_request", f"invalid signature level: '{level}'")

    field_name = params.get("field_name") or "Signature1"
    reason = params.get("reason")
    location = params.get("location")
    contact = params.get("contact")
    tsa_url = params.get("tsa_url")
    offline_evidence = params.get("offline_evidence") or {}

    # Resolve certificate and key
    signer = _resolve_signer(params, request.work_dir)

    # Resolve timestamping for B-T, B-LT, B-LTA (no silent downgrade)
    timestamper = None
    if level in {"B-T", "B-LT", "B-LTA"}:
        timestamper = _resolve_timestamper(offline_evidence, request.work_dir, tsa_url)
        if timestamper is None:
            raise EngineError(
                "invalid_request",
                f"signature level '{level}' requires a TSA URL or "
                "pre-fetched timestamp token; silent downgrade is prohibited",
            )

    # Signature metadata
    subfilter = fields.SigSeedSubFilter.PADES
    meta = PdfSignatureMetadata(
        field_name=field_name,
        reason=reason,
        location=location,
        contact_info=contact,
        subfilter=subfilter,
    )

    # Visual appearance setup
    new_field_spec = None
    visible = params.get("visible")
    stamp_style = None
    if visible:
        page_idx = visible.get("page", 1) - 1
        if page_idx < 0:
            raise EngineError("invalid_request", "visible.page must be >= 1")
        rect = visible.get("rect")
        if not rect or len(rect) != 4:
            raise EngineError("invalid_request", "visible.rect must be [x1, y1, x2, y2]")
        box = (rect[0], rect[1], rect[2], rect[3])
        new_field_spec = SigFieldSpec(sig_field_name=field_name, on_page=page_idx, box=box)

        appearance_text = visible.get("text") or visible.get("appearance")
        if appearance_text:
            stamp_style = TextStampStyle(
                stamp_text=appearance_text,
                border_width=1,
            )

    # Perform incremental signing
    stem = in_path.stem
    out_filename = f"{stem}.signed.pdf"
    out_dir = Path(request.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / out_filename

    writer.progress("pdf.sign", 0, 1)

    try:
        with open(in_path, "rb") as inf:
            pdf_writer = IncrementalPdfFileWriter(inf, strict=False)

            # Ensure signature field exists
            if new_field_spec is not None:
                fields.append_signature_field(pdf_writer, new_field_spec)

            pdf_signer = PdfSigner(
                meta,
                signer=signer,
                timestamper=timestamper,
                stamp_style=stamp_style,
            )

            with open(out_path, "wb") as outf:
                pdf_signer.sign_pdf(pdf_writer, output=outf)
    except EngineError:
        raise
    except Exception as exc:
        raise EngineError("engine_failure", f"signing failed: {exc}") from exc

    writer.progress("pdf.sign", 1, 1)
    writer.artifact(str(out_path), "pdf")
    writer.ok({"level": level, "field_name": field_name})


def _resolve_signer(params: dict[str, Any], work_dir: str) -> signers.Signer:
    """Loads cryptographic credentials into a pyHanko Signer."""
    cert_val = params.get("certificate") or params.get("certificate_file")
    password = params.get("password") or params.get("key_password")
    passphrase = password.encode("utf-8") if password else None

    # Check for binary secret file staged in work_dir (D6)
    staged_p12 = Path(work_dir) / "cert.p12"
    if staged_p12.is_file():
        try:
            return signers.SimpleSigner.load_pkcs12(str(staged_p12), passphrase=passphrase)
        except Exception as exc:
            raise EngineError("invalid_request", f"failed to load staged PKCS#12: {exc}") from exc

    if not cert_val:
        token = params.get("token")
        if token:
            raise EngineError(
                "tool_missing",
                "hardware token signing is not supported in this environment",
            )
        keychain = params.get("keychain")
        if keychain:
            raise EngineError(
                "tool_missing",
                "macOS Keychain signing is not supported on this platform",
            )
        raise EngineError(
            "invalid_request",
            "missing signing certificate; provide certificate or staged cert.p12",
        )

    # Could be a direct path or base64 bytes
    cert_path = Path(cert_val)
    if cert_path.is_file():
        try:
            return signers.SimpleSigner.load_pkcs12(str(cert_path), passphrase=passphrase)
        except Exception as exc:
            raise EngineError("invalid_request", f"failed to load PKCS#12 file: {exc}") from exc

    # Try base64 decoding
    try:
        raw_bytes = base64.b64decode(cert_val)
        return signers.SimpleSigner.load_pkcs12_data(raw_bytes, passphrase=passphrase)
    except Exception as exc:
        raise EngineError("invalid_request", f"invalid certificate data: {exc}") from exc


def _resolve_timestamper(
    offline_evidence: dict[str, Any], work_dir: str, tsa_url: str | None
) -> timestamps.TimeStamper | None:
    """Resolves timestamper from offline evidence or staged host broker response."""
    # Check offline evidence timestamp token
    token_path_str = offline_evidence.get("timestamp_token")
    if token_path_str:
        token_path = Path(token_path_str)
        if token_path.is_file():
            return PreComputedTimeStamper(token_path.read_bytes())

    # Check staged response from host broker (D7)
    staged_resp = Path(work_dir) / "tsa_response.bin"
    if staged_resp.is_file():
        return PreComputedTimeStamper(staged_resp.read_bytes())

    return None
