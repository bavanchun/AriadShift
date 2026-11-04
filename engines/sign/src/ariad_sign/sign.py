"""PAdES digital signing implementation using pyHanko."""

from __future__ import annotations

import asyncio
import base64
from pathlib import Path
from typing import Any

from ariad_sign.protocol import EngineError, RunToolRequest, Writer

try:
    from asn1crypto import cms, crl, ocsp, tsp, x509
    from cryptography.hazmat.primitives import serialization
    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.sign import fields, signers, timestamps
    from pyhanko.sign.fields import SigFieldSpec
    from pyhanko.sign.signers.pdf_signer import PdfSignatureMetadata, PdfSigner
    from pyhanko.stamp import TextStampStyle
    from pyhanko_certvalidator import ValidationContext

    PYHANKO_AVAILABLE = True
except ImportError:
    PYHANKO_AVAILABLE = False


class PreComputedTimeStamper(timestamps.TimeStamper):
    """Timestamper that uses a pre-fetched RFC 3161 timestamp response token."""

    def __init__(self, token_bytes: bytes) -> None:
        super().__init__()
        self.token_bytes = token_bytes
        try:
            content_info = cms.ContentInfo.load(token_bytes)
            signed_data = content_info["content"]
            encap = signed_data["encap_content_info"]
            content = encap["content"]
            raw_tst = (
                content.parsed
                if hasattr(content, "parsed")
                else tsp.TSTInfo.load(content.native)
            )
            if not isinstance(raw_tst, tsp.TSTInfo):
                raw_tst = tsp.TSTInfo.load(content.native)
            self._tst_info = raw_tst
            self._content_info = content_info
        except Exception as exc:
            raise EngineError(
                "invalid_request",
                f"failed to parse timestamp token: {exc}",
            ) from exc

    def _verify_imprint(self, message_digest: bytes, md_algorithm: str) -> None:
        imprint = self._tst_info["message_imprint"]
        token_digest = imprint["hashed_message"].native
        token_algo = imprint["hash_algorithm"]["algorithm"].native

        if token_digest != message_digest:
            raise EngineError(
                "invalid_request",
                f"RFC 3161 timestamp token imprint mismatch: expected {message_digest.hex()}, got {token_digest.hex()}",
            )
        norm_req_algo = md_algorithm.lower().replace("-", "")
        norm_tok_algo = token_algo.lower().replace("-", "")
        if norm_req_algo != norm_tok_algo:
            raise EngineError(
                "invalid_request",
                f"RFC 3161 timestamp token hash algorithm mismatch: expected {md_algorithm}, got {token_algo}",
            )

    async def async_timestamp(
        self, message_digest: bytes, md_algorithm: str
    ) -> cms.ContentInfo:
        self._verify_imprint(message_digest, md_algorithm)
        return self._content_info

    def timestamp(
        self, message_digest: bytes, md_algorithm: str
    ) -> cms.ContentInfo:
        self._verify_imprint(message_digest, md_algorithm)
        return self._content_info


class HostBrokeredTimeStamper(timestamps.TimeStamper):
    """Timestamper that coordinates imprint-bound RFC 3161 timestamping through the host broker."""

    def __init__(self, work_dir: Path, tsa_url: str, writer: Writer) -> None:
        super().__init__()
        self.work_dir = work_dir
        self.tsa_url = tsa_url
        self.writer = writer

    async def async_request_tsa_response(self, req: tsp.TimeStampReq) -> tsp.TimeStampResp:
        req_file = self.work_dir / "tsa_req.bin"
        resp_file = self.work_dir / "tsa_resp.bin"
        err_file = self.work_dir / "tsa_err.txt"

        if resp_file.exists():
            try:
                resp_file.unlink()
            except OSError:
                pass
        if err_file.exists():
            try:
                err_file.unlink()
            except OSError:
                pass

        req_file.write_bytes(req.dump())
        self.writer.progress("tsa_request", 0, 1)

        # Poll for host response with bounded timeout (30 seconds)
        for _ in range(600):
            if resp_file.is_file():
                resp_bytes = resp_file.read_bytes()
                try:
                    resp_file.unlink()
                except OSError:
                    pass
                return tsp.TimeStampResp.load(resp_bytes)
            if err_file.is_file():
                err_msg = err_file.read_text(encoding="utf-8")
                try:
                    err_file.unlink()
                except OSError:
                    pass
                raise EngineError("engine_failure", f"TSA broker failed: {err_msg}")
            await asyncio.sleep(0.05)

        raise EngineError(
            "engine_failure",
            "TSA broker request timed out waiting for host response",
        )

    def timestamp(self, message_digest: bytes, md_algorithm: str) -> cms.ContentInfo:
        return asyncio.run(self.async_timestamp(message_digest, md_algorithm))


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
        timestamper = _resolve_timestamper(offline_evidence, request.work_dir, tsa_url, writer)
        if timestamper is None:
            raise EngineError(
                "invalid_request",
                f"signature level '{level}' requires a TSA URL or "
                "pre-fetched timestamp token; silent downgrade is prohibited",
            )

    # Configure PAdES levels
    embed_validation_info = False
    use_pades_lta = False
    validation_context = None

    if level in {"B-LT", "B-LTA"}:
        embed_validation_info = True
        validation_context = _build_validation_context(offline_evidence, signer)
        if level == "B-LTA":
            use_pades_lta = True

    # Signature metadata
    subfilter = fields.SigSeedSubFilter.PADES
    meta = PdfSignatureMetadata(
        field_name=field_name,
        reason=reason,
        location=location,
        contact_info=contact,
        subfilter=subfilter,
        embed_validation_info=embed_validation_info,
        use_pades_lta=use_pades_lta,
        validation_context=validation_context,
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

    # Check for binary secret file staged in work_dir
    staged_p12 = Path(work_dir) / "cert.p12"
    if staged_p12.is_file():
        try:
            signer = signers.SimpleSigner.load_pkcs12(
                str(staged_p12), passphrase=passphrase
            )
            if signer is None:
                raise EngineError(
                    "invalid_request",
                    "failed to load staged PKCS#12 (check password or certificate format)",
                )
            return signer
        except EngineError:
            raise
        except Exception as exc:
            raise EngineError(
                "invalid_request",
                f"failed to load staged PKCS#12: {exc}",
            ) from exc

    if not cert_val:
        token = params.get("token")
        if token:
            from ariad_sign.token import load_token_signer
            return load_token_signer(token)
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

    # Could be a direct path
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
    offline_evidence: dict[str, Any], work_dir: str, tsa_url: str | None, writer: Writer
) -> timestamps.TimeStamper | None:
    """Resolves timestamper from offline evidence or host broker."""
    # Check offline evidence timestamp token
    token_path_str = offline_evidence.get("timestamp_token")
    if token_path_str:
        token_path = Path(token_path_str)
        if token_path.is_file():
            return PreComputedTimeStamper(token_path.read_bytes())

    # Host-brokered TSA exchange
    if tsa_url:
        return HostBrokeredTimeStamper(Path(work_dir), tsa_url, writer)

    return None


def _build_validation_context(
    offline_evidence: dict[str, Any],
    signer: signers.Signer | None = None,
) -> ValidationContext:
    """Builds validation context for PAdES B-LT/B-LTA embedding."""
    trust_roots = []
    crls = []
    ocsps = []

    # Trust anchors
    for anchor_path_str in offline_evidence.get("trust_anchors", []):
        p = Path(anchor_path_str)
        if not p.is_file():
            raise EngineError("invalid_request", f"trust anchor file not found: {p}")
        data = p.read_bytes()
        try:
            if b"-----BEGIN CERTIFICATE-----" in data:
                from cryptography.x509 import load_pem_x509_certificates
                for c in load_pem_x509_certificates(data):
                    der_bytes = c.public_bytes(serialization.Encoding.DER)
                    trust_roots.append(x509.Certificate.load(der_bytes))
            else:
                trust_roots.append(x509.Certificate.load(data))
        except Exception as exc:
            raise EngineError(
                "invalid_request",
                f"failed to parse trust anchor {p}: {exc}",
            ) from exc

    # CRLs
    for crl_path_str in offline_evidence.get("crl_files", []):
        p = Path(crl_path_str)
        if not p.is_file():
            raise EngineError("invalid_request", f"CRL file not found: {p}")
        data = p.read_bytes()
        try:
            crls.append(crl.CertificateList.load(data))
        except Exception as exc:
            raise EngineError("invalid_request", f"failed to parse CRL {p}: {exc}") from exc

    # OCSPs
    for ocsp_path_str in offline_evidence.get("ocsp_responses", []):
        p = Path(ocsp_path_str)
        if not p.is_file():
            raise EngineError("invalid_request", f"OCSP response file not found: {p}")
        data = p.read_bytes()
        try:
            ocsps.append(ocsp.OCSPResponse.load(data))
        except Exception as exc:
            raise EngineError(
                "invalid_request",
                f"failed to parse OCSP response {p}: {exc}",
            ) from exc

    return ValidationContext(
        trust_roots=trust_roots if trust_roots else None,
        crls=crls if crls else None,
        ocsps=ocsps if ocsps else None,
        allow_fetching=False,
    )
