"""Hardware token / PKCS#11 signing backend implementation."""

from __future__ import annotations

import abc
import datetime
from pathlib import Path
from typing import Any

from ariad_sign.protocol import EngineError, RunToolRequest, Writer

try:
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes
    from cryptography.hazmat.primitives.asymmetric import rsa
    from cryptography.hazmat.primitives.serialization import NoEncryption, pkcs12
    from cryptography.x509.oid import NameOID
    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.sign import fields, signers
    from pyhanko.sign.signers.pdf_signer import PdfSignatureMetadata, PdfSigner

    PYHANKO_AVAILABLE = True
except ImportError:
    PYHANKO_AVAILABLE = False


class TokenSignerBackend(abc.ABC):
    """Abstract interface for hardware/software cryptographic token signers."""

    @abc.abstractmethod
    def create_signer(self, params: dict[str, Any]) -> signers.Signer:
        """Instantiates a pyHanko-compatible Signer from token parameters."""
        raise NotImplementedError


class MockSoftwareTokenBackend(TokenSignerBackend):
    """Software-backed token implementation for test execution with generated keys."""

    def create_signer(self, params: dict[str, Any]) -> signers.Signer:
        key_label = params.get("key_label") or "Mock Token Key"
        private_key = rsa.generate_private_key(
            public_exponent=65537,
            key_size=2048,
        )
        subject = issuer = x509.Name(
            [
                x509.NameAttribute(NameOID.COMMON_NAME, key_label),
            ]
        )
        now = datetime.datetime.now(datetime.UTC)
        cert = (
            x509.CertificateBuilder()
            .subject_name(subject)
            .issuer_name(issuer)
            .public_key(private_key.public_key())
            .serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(days=1))
            .not_valid_after(now + datetime.timedelta(days=365))
            .add_extension(
                x509.KeyUsage(
                    digital_signature=True,
                    content_commitment=True,
                    key_encipherment=False,
                    data_encipherment=False,
                    key_agreement=False,
                    key_cert_sign=False,
                    crl_sign=False,
                    encipher_only=False,
                    decipher_only=False,
                ),
                critical=True,
            )
            .sign(private_key, hashes.SHA256())
        )
        p12_data = pkcs12.serialize_key_and_certificates(
            name=key_label.encode("utf-8"),
            key=private_key,
            cert=cert,
            cas=None,
            encryption_algorithm=NoEncryption(),
        )
        return signers.SimpleSigner.load_pkcs12_data(
            p12_data,
            other_certs=(),
            passphrase=None,
        )


class PKCS11TokenBackend(TokenSignerBackend):
    """PKCS#11 hardware token backend."""

    def create_signer(self, params: dict[str, Any]) -> signers.Signer:
        try:
            import pkcs11  # noqa: F401
            from pyhanko.sign import pkcs11 as pyh_pkcs11
        except ImportError as exc:
            raise EngineError(
                "tool_missing",
                "PKCS#11 token signing requires python-pkcs11 library",
            ) from exc

        module_path = params.get("module")
        if not module_path or not Path(module_path).exists():
            raise EngineError(
                "tool_missing",
                f"PKCS#11 module not found at: {module_path}",
            )

        pin = params.get("pin")
        slot_no = params.get("slot")
        token_label = params.get("token_label")
        key_label = params.get("key_label")

        try:
            return pyh_pkcs11.open_pkcs11_session(
                lib_location=module_path,
                slot_no=slot_no,
                token_label=token_label,
                user_pin=pin,
                key_label=key_label,
            )
        except Exception as exc:
            raise EngineError("engine_failure", f"PKCS#11 session creation failed: {exc}") from exc


def load_token_signer(params: dict[str, Any]) -> signers.Signer:
    """Loads a token signer using either test software backend or PKCS#11 backend."""
    import os

    # Software mock backend is strictly test-only behind explicit environment variable
    if os.environ.get("ARIAD_TEST_SOFTWARE_TOKEN") == "1":
        return MockSoftwareTokenBackend().create_signer(params)

    # In production, PKCS#11 extra is deferred; hardware token signing is not available
    raise EngineError(
        "tool_missing",
        "PKCS#11 hardware token signing is not available in this release (deferred)",
    )


def run_sign_token(request: RunToolRequest, writer: Writer) -> None:
    """Executes a `pdf.sign_token` request."""
    if not PYHANKO_AVAILABLE:
        raise EngineError("tool_missing", "pyHanko is not installed or available")

    in_path = Path(request.inputs[0]["path"])
    if not in_path.is_file():
        raise EngineError("io", f"input file does not exist: {in_path}")

    params = request.params
    signer = load_token_signer(params)

    # Use sign module's helper logic for PAdES metadata, timestamper, and writing
    from ariad_sign.sign import (
        _build_validation_context,
        _resolve_timestamper,
    )

    level = (params.get("level") or "b_b").lower().replace("-", "_")
    needs_ts = level in ("b_t", "b_lt", "b_lta")
    needs_lt = level in ("b_lt", "b_lta")
    work_dir = Path(request.work_dir)
    tsa_url = params.get("tsa_url")
    offline_evidence = params.get("offline_evidence") or {}

    timestamper = (
        _resolve_timestamper(offline_evidence, str(work_dir), tsa_url, writer)
        if needs_ts
        else None
    )
    if needs_ts and timestamper is None:
        raise EngineError(
            "invalid_request",
            f"level '{level}' requires tsa_url or timestamp_token; silent downgrade is prohibited",
        )

    vc = None
    if needs_lt:
        vc = _build_validation_context(offline_evidence, signer)

    field_name = params.get("field_name") or "TokenSignature"
    reason = params.get("reason")
    location = params.get("location")
    contact_info = params.get("contact_info")

    meta = PdfSignatureMetadata(
        field_name=field_name,
        reason=reason,
        location=location,
        contact_info=contact_info,
        subfilter=fields.SigSeedSubFilter.PADES,
        validation_context=vc if needs_lt else None,
        embed_validation_info=needs_lt,
        use_pades_lta=(level == "b_lta"),
    )

    stem = in_path.stem
    out_filename = f"{stem}.signed.pdf"
    out_dir = Path(request.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / out_filename

    writer.progress("pdf.sign_token", 0, 1)

    try:
        with open(in_path, "rb") as inf:
            reader = PdfFileReader(inf, strict=False)
            if reader.encrypted:
                raise EngineError("encrypted_input", "cannot sign encrypted PDF")

            pdf_writer = IncrementalPdfFileWriter(inf, strict=False)
            pdf_signer = PdfSigner(
                meta,
                signer=signer,
                timestamper=timestamper,
            )

            with open(out_path, "wb") as outf:
                pdf_signer.sign_pdf(pdf_writer, output=outf)
    except EngineError:
        raise
    except Exception as exc:
        raise EngineError("engine_failure", f"token signing failed: {exc}") from exc

    writer.progress("pdf.sign_token", 1, 1)
    writer.artifact(str(out_path), "pdf")
    writer.ok({"level": level, "field_name": field_name})
