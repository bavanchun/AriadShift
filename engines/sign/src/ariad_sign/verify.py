"""Digital signature verification implementation using pyHanko."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from ariad_sign.protocol import EngineError, RunToolRequest, Writer

try:
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.sign.validation import (
        ValidationContext,
        validate_pdf_signature,
    )
    from pyhanko.sign.validation.status import SignatureCoverageLevel

    PYHANKO_AVAILABLE = True
except ImportError:
    PYHANKO_AVAILABLE = False


def run_verify(request: RunToolRequest, writer: Writer) -> None:
    """Executes a `pdf.sign_verify` request."""
    if not PYHANKO_AVAILABLE:
        raise EngineError("tool_missing", "pyHanko is not installed or available")

    in_path = Path(request.inputs[0]["path"])
    if not in_path.is_file():
        raise EngineError("io", f"input file does not exist: {in_path}")

    params = request.params
    fetch_revocation = bool(params.get("fetch_revocation", False))
    trust_anchors = params.get("trust_anchors") or []
    offline_evidence = params.get("offline_evidence") or []

    # Build validation context
    vc = _build_validation_context(
        trust_anchors=trust_anchors,
        offline_evidence=offline_evidence,
        allow_fetching=fetch_revocation,
    )

    writer.progress("pdf.sign_verify", 0, 1)

    signatures_report: list[dict[str, Any]] = []
    try:
        with open(in_path, "rb") as f:
            reader = PdfFileReader(f, strict=False)
            embedded_sigs = reader.embedded_signatures

            for sig_field in embedded_sigs:
                status = validate_pdf_signature(sig_field, vc)

                # Extract signer info
                signer_info = {}
                signing_cert = status.signing_cert
                if signing_cert is not None:
                    subject = signing_cert.subject
                    signer_info = {
                        "subject": subject.human_friendly,
                        "serial_number": str(signing_cert.serial_number),
                        "issuer": signing_cert.issuer.human_friendly,
                        "not_valid_before": signing_cert.not_valid_before.isoformat(),
                        "not_valid_after": signing_cert.not_valid_after.isoformat(),
                    }

                # Extract coverage
                coverage_str = "unknown"
                if status.coverage == SignatureCoverageLevel.ENTIRE_FILE:
                    coverage_str = "entire_file"
                elif status.coverage == SignatureCoverageLevel.ENTIRE_REVISION:
                    coverage_str = "entire_revision"
                else:
                    coverage_str = "partial"

                # Extract timestamp info
                timestamp_info = None
                if status.timestamp_validity is not None:
                    ts_val = status.timestamp_validity
                    ts = (
                        ts_val.timestamp.isoformat() if getattr(ts_val, "timestamp", None) else None
                    )
                    timestamp_info = {
                        "timestamp": ts,
                        "integrity": ts_val.intact,
                        "valid": ts_val.valid,
                    }

                # Certificate status
                cert_status = "unknown"
                if status.valid:
                    cert_status = "valid"
                elif not status.intact:
                    cert_status = "corrupted"
                else:
                    cert_status = "untrusted_or_expired"

                # Modifications summary
                modifications = {
                    "docmdp_ok": status.docmdp_ok,
                    "bottom_line": status.bottom_line,
                }

                sig_entry = {
                    "field_name": sig_field.field_name,
                    "signer": signer_info,
                    "integrity": status.intact,
                    "valid": status.valid,
                    "coverage": coverage_str,
                    "certificate_status": cert_status,
                    "timestamp": timestamp_info,
                    "modifications": modifications,
                }
                signatures_report.append(sig_entry)
    except Exception as exc:
        raise EngineError("engine_failure", f"signature verification failed: {exc}") from exc

    stem = in_path.stem
    out_filename = f"{stem}.sign-verify.json"
    out_dir = Path(request.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / out_filename

    all_intact = len(signatures_report) == 0 or all(s["integrity"] for s in signatures_report)
    all_valid = len(signatures_report) == 0 or all(s["valid"] for s in signatures_report)

    report_data = {
        "file": in_path.name,
        "valid": all_valid,
        "integrity": all_intact,
        "signature_count": len(signatures_report),
        "signatures_count": len(signatures_report),
        "signatures": signatures_report,
    }

    out_path.write_text(json.dumps(report_data, indent=2), encoding="utf-8")

    writer.progress("pdf.sign_verify", 1, 1)
    writer.artifact(str(out_path), "json")
    all_valid = len(signatures_report) > 0 and all(s["valid"] for s in signatures_report)
    writer.ok(
        {
            "signatures_count": len(signatures_report),
            "all_valid": all_valid,
        }
    )


def _build_validation_context(
    trust_anchors: list[str],
    offline_evidence: list[str],
    allow_fetching: bool,
) -> ValidationContext:
    """Constructs a pyHanko ValidationContext with offline-by-default rules."""
    from asn1crypto import x509
    from cryptography.hazmat.primitives import serialization

    trust_roots = []

    for anchor_path_str in trust_anchors:
        anchor_path = Path(anchor_path_str)
        if anchor_path.is_file():
            data = anchor_path.read_bytes()
            try:
                # Attempt PEM decode
                if b"-----BEGIN CERTIFICATE-----" in data:
                    from cryptography.x509 import load_pem_x509_certificates

                    certs = load_pem_x509_certificates(data)
                    for c in certs:
                        cert_obj = x509.Certificate.load(c.public_bytes(serialization.Encoding.DER))
                        trust_roots.append(cert_obj)
                else:
                    cert_obj = x509.Certificate.load(data)
                    trust_roots.append(cert_obj)
            except Exception:  # noqa: S110
                pass

    return ValidationContext(
        trust_roots=trust_roots if trust_roots else None,
        allow_fetching=allow_fetching,
    )
