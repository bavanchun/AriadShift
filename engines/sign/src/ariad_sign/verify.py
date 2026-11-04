"""Digital signature verification implementation using pyHanko."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from ariad_sign.protocol import EngineError, RunToolRequest, Writer

try:
    from asn1crypto import crl, ocsp, x509
    from cryptography.hazmat.primitives import serialization
    from cryptography.x509 import load_pem_x509_certificates
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.sign.validation import (
        ValidationContext,
        validate_pdf_signature,
        validate_pdf_timestamp,
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
    if params.get("fetch_revocation"):
        raise EngineError(
            "invalid_request",
            "network revocation fetching is disabled; supply offline_evidence or trust_anchors",
        )

    trust_anchors = params.get("trust_anchors") or []
    offline_evidence = params.get("offline_evidence") or []

    # Build validation context (offline-only, OS trust store default if no anchors)
    vc = _build_validation_context(
        trust_anchors=trust_anchors,
        offline_evidence=offline_evidence,
    )

    writer.progress("pdf.sign_verify", 0, 1)

    signatures_report: list[dict[str, Any]] = []
    try:
        with open(in_path, "rb") as f:
            reader = PdfFileReader(f, strict=False)
            embedded_sigs = reader.embedded_signatures

            for sig_field in embedded_sigs:
                is_timestamp = (sig_field.sig_object_type == "/DocTimeStamp")
                try:
                    if is_timestamp:
                        status = validate_pdf_timestamp(
                            sig_field, validation_context=vc
                        )
                    else:
                        status = validate_pdf_signature(sig_field, vc)
                except Exception as sig_exc:
                    signatures_report.append(
                        {
                            "field_name": sig_field.field_name,
                            "signer": {},
                            "integrity": False,
                            "valid": False,
                            "coverage": "unknown",
                            "certificate_status": "error",
                            "timestamp": None,
                            "signing_time": None,
                            "revocation_status": "unknown",
                            "modifications": {
                                "docmdp_ok": False,
                                "bottom_line": False,
                                "modification_level": None,
                                "diff_summary": None,
                            },
                            "error": str(sig_exc),
                        }
                    )
                    continue

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
                if is_timestamp and getattr(status, "timestamp", None) is not None:
                    timestamp_info = {
                        "timestamp": status.timestamp.isoformat(),
                        "integrity": status.intact,
                        "valid": status.valid,
                    }
                elif getattr(status, "timestamp_validity", None) is not None:
                    ts_val = status.timestamp_validity
                    ts = (
                        ts_val.timestamp.isoformat()
                        if getattr(ts_val, "timestamp", None)
                        else None
                    )
                    timestamp_info = {
                        "timestamp": ts,
                        "integrity": ts_val.intact,
                        "valid": ts_val.valid,
                    }

                # Signing time
                signing_time = None
                if is_timestamp and getattr(status, "timestamp", None) is not None:
                    signing_time = status.timestamp.isoformat()
                elif getattr(status, "signer_reported_dt", None) is not None:
                    signing_time = status.signer_reported_dt.isoformat()

                # Revocation status
                revocation_status = "unknown"
                if status.revoked is True:
                    revocation_status = "revoked"
                elif status.revoked is False:
                    revocation_status = "not_revoked"

                # Certificate status
                cert_status = "unknown"
                if status.valid:
                    cert_status = "valid"
                elif not status.intact:
                    cert_status = "corrupted"
                else:
                    cert_status = "untrusted_or_expired"

                # Modifications summary
                mod_level_str = None
                if status.modification_level is not None:
                    mod_level_str = (
                        status.modification_level.name.lower()
                        if hasattr(status.modification_level, "name")
                        else str(status.modification_level)
                    )

                diff_summary = None
                if status.diff_result is not None:
                    diff_summary = {
                        "modification_level": mod_level_str,
                        "changed_form_fields": list(status.diff_result.changed_form_fields)
                        if status.diff_result.changed_form_fields
                        else [],
                    }

                bottom_line = getattr(status, "bottom_line", None)
                if bottom_line is None and is_timestamp:
                    bottom_line = bool(
                        status.intact
                        and status.valid
                        and getattr(status, "docmdp_ok", True)
                    )
                modifications = {
                    "docmdp_ok": getattr(status, "docmdp_ok", True),
                    "bottom_line": bottom_line,
                    "modification_level": mod_level_str,
                    "diff_summary": diff_summary,
                }

                is_valid = bool(
                    status.intact
                    and status.valid
                    and (bottom_line if bottom_line is not None else True)
                )
                sig_entry = {
                    "field_name": sig_field.field_name,
                    "signer": signer_info,
                    "integrity": status.intact,
                    "valid": is_valid,
                    "coverage": coverage_str,
                    "certificate_status": cert_status,
                    "timestamp": timestamp_info,
                    "signing_time": signing_time,
                    "revocation_status": revocation_status,
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

    # If zero signatures are present, valid and integrity are false
    sig_count = len(signatures_report)
    all_intact = sig_count > 0 and all(s["integrity"] for s in signatures_report)
    all_valid = sig_count > 0 and all(s["valid"] for s in signatures_report)

    report_data = {
        "file": in_path.name,
        "valid": all_valid,
        "integrity": all_intact,
        "signature_count": sig_count,
        "signatures_count": sig_count,
        "signatures": signatures_report,
    }

    out_path.write_text(json.dumps(report_data, indent=2), encoding="utf-8")

    writer.progress("pdf.sign_verify", 1, 1)
    writer.artifact(str(out_path), "json")
    writer.ok(
        {
            "signatures_count": sig_count,
            "all_valid": all_valid,
        }
    )


def _build_validation_context(
    trust_anchors: list[str],
    offline_evidence: list[str],
) -> ValidationContext:
    """Constructs a pyHanko ValidationContext with strict offline rules."""
    trust_roots = []
    for anchor_path_str in trust_anchors:
        anchor_path = Path(anchor_path_str)
        if not anchor_path.is_file():
            raise EngineError("invalid_request", f"trust anchor file not found: {anchor_path}")
        data = anchor_path.read_bytes()
        try:
            if b"-----BEGIN CERTIFICATE-----" in data:
                certs = load_pem_x509_certificates(data)
                for c in certs:
                    cert_obj = x509.Certificate.load(c.public_bytes(serialization.Encoding.DER))
                    trust_roots.append(cert_obj)
            else:
                cert_obj = x509.Certificate.load(data)
                trust_roots.append(cert_obj)
        except Exception as exc:
            raise EngineError(
                "invalid_request",
                f"failed to parse trust anchor at {anchor_path}: {exc}",
            ) from exc

    other_certs = []
    crls = []
    ocsps = []

    for ev_path_str in offline_evidence:
        ev_path = Path(ev_path_str)
        if not ev_path.is_file():
            raise EngineError("invalid_request", f"offline evidence file not found: {ev_path}")
        data = ev_path.read_bytes()
        parsed = False

        if b"-----BEGIN CERTIFICATE-----" in data:
            try:
                certs = load_pem_x509_certificates(data)
                for c in certs:
                    other_certs.append(
                        x509.Certificate.load(c.public_bytes(serialization.Encoding.DER))
                    )
                parsed = True
            except Exception:  # noqa: S110
                pass

        if not parsed:
            try:
                crl_obj = crl.CertificateList.load(data)
                _ = crl_obj["tbs_cert_list"]
                _ = crl_obj["signature_algorithm"]
                _ = crl_obj["signature"]
                crls.append(crl_obj)
                parsed = True
            except Exception:  # noqa: S110
                pass

        if not parsed:
            try:
                ocsp_obj = ocsp.OCSPResponse.load(data)
                _ = ocsp_obj["response_status"]
                ocsps.append(ocsp_obj)
                parsed = True
            except Exception:  # noqa: S110
                pass

        if not parsed:
            try:
                cert_obj = x509.Certificate.load(data)
                other_certs.append(cert_obj)
                parsed = True
            except Exception:  # noqa: S110
                pass

        if not parsed:
            raise EngineError(
                "invalid_request",
                f"unrecognized or unparsable offline evidence file: {ev_path}",
            )

    return ValidationContext(
        trust_roots=trust_roots if trust_roots else None,
        other_certs=other_certs if other_certs else None,
        crls=crls if crls else None,
        ocsps=ocsps if ocsps else None,
        allow_fetching=False,
    )
