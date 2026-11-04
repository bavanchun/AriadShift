"""Tests for the digital signature engine."""

from __future__ import annotations

import io
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest
from cryptography import x509
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.serialization import BestAvailableEncryption, pkcs12
from cryptography.x509.oid import NameOID

from ariad_sign.describe import capabilities
from ariad_sign.engine import handle
from ariad_sign.protocol import (
    RunToolRequest,
    Writer,
)


def generate_test_p12(password: str = "secret") -> bytes:
    """Generates an in-memory test PKCS#12 bundle."""
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    subject = issuer = x509.Name(
        [
            x509.NameAttribute(NameOID.COUNTRY_NAME, "VN"),
            x509.NameAttribute(NameOID.ORGANIZATION_NAME, "AriadShift Test"),
            x509.NameAttribute(NameOID.COMMON_NAME, "Test Signer"),
        ]
    )
    now = datetime.now(UTC)
    cert = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(issuer)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(days=1))
        .not_valid_after(now + timedelta(days=365))
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .sign(key, hashes.SHA256())
    )
    return pkcs12.serialize_key_and_certificates(
        name=b"test-signer",
        key=key,
        cert=cert,
        cas=None,
        encryption_algorithm=BestAvailableEncryption(password.encode("utf-8")),
    )


REPO_ROOT = Path(__file__).resolve().parents[3]
FIXTURE_PDF = REPO_ROOT / "fixtures" / "pdf-tools" / "annotations.pdf"


def get_sample_pdf_bytes() -> bytes:
    """Returns sample PDF bytes from fixtures."""
    if FIXTURE_PDF.is_file():
        return FIXTURE_PDF.read_bytes()
    # Fallback to minimal PDF
    from pyhanko.pdf_utils.generic import ArrayObject, DictionaryObject, NameObject, NumberObject
    from pyhanko.pdf_utils.writer import PdfFileWriter

    w = PdfFileWriter()
    page = DictionaryObject(
        {
            NameObject("/Type"): NameObject("/Page"),
            NameObject("/MediaBox"): ArrayObject(
                [NumberObject(0), NumberObject(0), NumberObject(612), NumberObject(792)]
            ),
        }
    )
    w.add_page(page)
    bio = io.BytesIO()
    w.write(bio)
    return bio.getvalue()


class InMemoryWriter(Writer):
    """Writer recording emitted events in memory for test assertions."""

    def __init__(self) -> None:
        self.events: list[dict] = []

    def emit(self, event: dict) -> None:
        self.events.append(event)


def test_describe_capabilities():
    caps = capabilities()
    assert caps["type"] == "capabilities"
    assert caps["protocol"] == "ariad-engine/2"
    assert caps["engine"] == "sign"
    tool_ids = [t["id"] for t in caps["tools"]]
    assert "pdf.sign" in tool_ids
    assert "pdf.sign_verify" in tool_ids
    assert "pdf.sign_remove" in tool_ids


def test_sign_verify_remove_pipeline(tmp_path: Path):
    pdf_path = tmp_path / "sample.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    p12_bytes = generate_test_p12("test-password")
    p12_path = tmp_path / "cert.p12"
    p12_path.write_bytes(p12_bytes)

    out_dir = tmp_path / "out"
    work_dir = tmp_path / "work"
    out_dir.mkdir()
    work_dir.mkdir()

    # 1. Sign (PAdES B-B)
    sign_req = RunToolRequest(
        job="job-1",
        tool="pdf.sign",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(out_dir),
        work_dir=str(work_dir),
        params={
            "certificate": str(p12_path),
            "password": "test-password",
            "level": "B-B",
            "reason": "Test Approval",
            "location": "Hanoi",
        },
    )
    writer = InMemoryWriter()
    handle(sign_req, writer)

    result_event = next(e for e in writer.events if e["type"] == "result")
    assert result_event["ok"] is True
    artifact_event = next(e for e in writer.events if e["type"] == "artifact")
    signed_pdf_path = Path(artifact_event["path"])
    assert signed_pdf_path.is_file()

    # 2. Verify
    verify_out_dir = tmp_path / "verify_out"
    verify_out_dir.mkdir()
    verify_req = RunToolRequest(
        job="job-2",
        tool="pdf.sign_verify",
        inputs=[{"path": str(signed_pdf_path), "format": "pdf"}],
        output_dir=str(verify_out_dir),
        work_dir=str(work_dir),
        params={"fetch_revocation": False},
    )
    v_writer = InMemoryWriter()
    handle(verify_req, v_writer)

    v_result = next(e for e in v_writer.events if e["type"] == "result")
    assert v_result["ok"] is True
    assert v_result.get("metrics", {}).get("signatures_count") == 1

    v_artifact = next(e for e in v_writer.events if e["type"] == "artifact")
    report_file = Path(v_artifact["path"])
    report_json = json.loads(report_file.read_text("utf-8"))
    assert len(report_json["signatures"]) == 1
    assert report_json["signatures"][0]["integrity"] is True
    assert "Test Signer" in report_json["signatures"][0]["signer"]["subject"]

    # 3. Remove
    remove_out_dir = tmp_path / "remove_out"
    remove_out_dir.mkdir()
    remove_req = RunToolRequest(
        job="job-3",
        tool="pdf.sign_remove",
        inputs=[{"path": str(signed_pdf_path), "format": "pdf"}],
        output_dir=str(remove_out_dir),
        work_dir=str(work_dir),
        params={"confirm_invalidation": True},
    )
    r_writer = InMemoryWriter()
    handle(remove_req, r_writer)

    r_result = next(e for e in r_writer.events if e["type"] == "result")
    assert r_result["ok"] is True
    assert r_result.get("metrics", {}).get("removed_signatures_count") == 1
    r_artifact = next(e for e in r_writer.events if e["type"] == "artifact")
    unsigned_pdf = Path(r_artifact["path"])
    assert unsigned_pdf.is_file()


def test_sign_remove_requires_confirmation(tmp_path: Path):
    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    remove_req = RunToolRequest(
        job="job-fail",
        tool="pdf.sign_remove",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(tmp_path),
        work_dir=str(tmp_path),
        params={"confirm_invalidation": False},
    )
    writer = InMemoryWriter()
    handle(remove_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_request"


def test_sign_visual(tmp_path: Path):
    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    p12_bytes = generate_test_p12("pass")
    p12_path = tmp_path / "cert.p12"
    p12_path.write_bytes(p12_bytes)

    out_dir = tmp_path / "out"
    work_dir = tmp_path / "work"
    out_dir.mkdir()
    work_dir.mkdir()

    sign_req = RunToolRequest(
        job="job-visual",
        tool="pdf.sign",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(out_dir),
        work_dir=str(work_dir),
        params={
            "certificate": str(p12_path),
            "password": "pass",
            "level": "B-B",
            "visible": {
                "page": 1,
                "rect": [50, 50, 200, 100],
                "appearance": "Signed by Test Signer",
            },
        },
    )
    writer = InMemoryWriter()
    handle(sign_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is True
    art = next(e for e in writer.events if e["type"] == "artifact")
    assert Path(art["path"]).is_file()


def test_sign_level_bt_requires_tsa_or_offline_token(tmp_path: Path):
    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    p12_bytes = generate_test_p12("pass")
    p12_path = tmp_path / "cert.p12"
    p12_path.write_bytes(p12_bytes)

    out_dir = tmp_path / "out"
    work_dir = tmp_path / "work"
    out_dir.mkdir()
    work_dir.mkdir()

    # B-T without TSA should fail closed (no silent downgrade)
    sign_req = RunToolRequest(
        job="job-bt-nodowngrade",
        tool="pdf.sign",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(out_dir),
        work_dir=str(work_dir),
        params={
            "certificate": str(p12_path),
            "password": "pass",
            "level": "B-T",
        },
    )
    writer = InMemoryWriter()
    handle(sign_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_request"
    assert "silent downgrade is prohibited" in res["error"]["message"]


def test_sign_encrypted_input_rejected(tmp_path: Path):
    plaintext_pdf = REPO_ROOT / "fixtures" / "pdf-tools" / "encrypted-aes256.plaintext.pdf"
    if not plaintext_pdf.is_file():
        pytest.skip("plaintext fixture not found")

    import subprocess

    encrypted_pdf = tmp_path / "encrypted.pdf"
    res_qpdf = subprocess.run(
        ["qpdf", "--encrypt", "user", "owner", "256", "--", str(plaintext_pdf), str(encrypted_pdf)],
        capture_output=True,
    )
    if res_qpdf.returncode != 0:
        pytest.skip(f"qpdf encrypt failed: {res_qpdf.stderr}")

    p12_bytes = generate_test_p12("pass")
    p12_path = tmp_path / "cert.p12"
    p12_path.write_bytes(p12_bytes)

    sign_req = RunToolRequest(
        job="job-enc",
        tool="pdf.sign",
        inputs=[{"path": str(encrypted_pdf), "format": "pdf"}],
        output_dir=str(tmp_path),
        work_dir=str(tmp_path),
        params={
            "certificate": str(p12_path),
            "password": "pass",
        },
    )
    writer = InMemoryWriter()
    handle(sign_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is False
    assert res["error"]["code"] == "encrypted_input"


def test_sign_verify_zero_signatures(tmp_path: Path):
    pdf_path = tmp_path / "unsigned.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    verify_req = RunToolRequest(
        job="job-zero-sigs",
        tool="pdf.sign_verify",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(tmp_path),
        work_dir=str(tmp_path),
        params={},
    )
    writer = InMemoryWriter()
    handle(verify_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is True
    assert res["metrics"]["signatures_count"] == 0
    assert res["metrics"]["all_valid"] is False

    art = next(e for e in writer.events if e["type"] == "artifact")
    report = json.loads(Path(art["path"]).read_text("utf-8"))
    assert report["signatures_count"] == 0
    assert report["valid"] is False
    assert report["integrity"] is False


def test_sign_verify_rejects_fetch_revocation(tmp_path: Path):
    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    verify_req = RunToolRequest(
        job="job-fetch-rej",
        tool="pdf.sign_verify",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(tmp_path),
        work_dir=str(tmp_path),
        params={"fetch_revocation": True},
    )
    writer = InMemoryWriter()
    handle(verify_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_request"
    assert "network revocation fetching is disabled" in res["error"]["message"]


def test_sign_verify_strict_offline_evidence_error(tmp_path: Path):
    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    # 1. Non-existent trust anchor
    verify_req1 = RunToolRequest(
        job="job-bad-anchor",
        tool="pdf.sign_verify",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(tmp_path),
        work_dir=str(tmp_path),
        params={"trust_anchors": ["/non/existent/anchor.crt"]},
    )
    writer1 = InMemoryWriter()
    handle(verify_req1, writer1)
    res1 = next(e for e in writer1.events if e["type"] == "result")
    assert res1["ok"] is False
    assert res1["error"]["code"] == "invalid_request"

    # 2. Corrupt offline evidence
    corrupt_file = tmp_path / "corrupt.crl"
    corrupt_file.write_bytes(b"not-a-valid-crl-or-ocsp")
    verify_req2 = RunToolRequest(
        job="job-bad-ev",
        tool="pdf.sign_verify",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(tmp_path),
        work_dir=str(tmp_path),
        params={"offline_evidence": [str(corrupt_file)]},
    )
    writer2 = InMemoryWriter()
    handle(verify_req2, writer2)
    res2 = next(e for e in writer2.events if e["type"] == "result")
    assert res2["ok"] is False
    assert res2["error"]["code"] == "invalid_request"


def test_sign_token_tool(tmp_path: Path):
    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    token_req = RunToolRequest(
        job="job-token",
        tool="pdf.sign_token",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(tmp_path / "out"),
        work_dir=str(tmp_path / "work"),
        params={
            "module": "mock",
            "key_label": "Hardware Token Key",
            "level": "B-B",
            "reason": "Token Signing Test",
        },
    )
    writer = InMemoryWriter()
    handle(token_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is True
    art = next(e for e in writer.events if e["type"] == "artifact")
    signed_pdf = Path(art["path"])
    assert signed_pdf.is_file()

    # Verify token signature
    verify_req = RunToolRequest(
        job="job-verify-token",
        tool="pdf.sign_verify",
        inputs=[{"path": str(signed_pdf), "format": "pdf"}],
        output_dir=str(tmp_path / "v_out"),
        work_dir=str(tmp_path / "work"),
        params={},
    )
    v_writer = InMemoryWriter()
    handle(verify_req, v_writer)
    v_res = next(e for e in v_writer.events if e["type"] == "result")
    assert v_res["ok"] is True
    assert v_res["metrics"]["signatures_count"] == 1


def test_sign_remove_multi_signature_warning(tmp_path: Path):
    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    p12_bytes = generate_test_p12("secret")
    p12_file = tmp_path / "cert.p12"
    p12_file.write_bytes(p12_bytes)

    # First signature
    out1 = tmp_path / "out1"
    work1 = tmp_path / "work1"
    out1.mkdir()
    work1.mkdir()
    req1 = RunToolRequest(
        job="sig1",
        tool="pdf.sign",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(out1),
        work_dir=str(work1),
        params={"certificate": str(p12_file), "password": "secret", "field_name": "Sig1"},
    )
    w1 = InMemoryWriter()
    handle(req1, w1)
    sig1_pdf = Path(next(e for e in w1.events if e["type"] == "artifact")["path"])

    # Second signature
    out2 = tmp_path / "out2"
    work2 = tmp_path / "work2"
    out2.mkdir()
    work2.mkdir()
    req2 = RunToolRequest(
        job="sig2",
        tool="pdf.sign",
        inputs=[{"path": str(sig1_pdf), "format": "pdf"}],
        output_dir=str(out2),
        work_dir=str(work2),
        params={"certificate": str(p12_file), "password": "secret", "field_name": "Sig2"},
    )
    w2 = InMemoryWriter()
    handle(req2, w2)
    sig2_pdf = Path(next(e for e in w2.events if e["type"] == "artifact")["path"])

    # Remove only Sig1 from 2-signature doc
    out3 = tmp_path / "out3"
    work3 = tmp_path / "work3"
    out3.mkdir()
    work3.mkdir()
    req3 = RunToolRequest(
        job="rem",
        tool="pdf.sign_remove",
        inputs=[{"path": str(sig2_pdf), "format": "pdf"}],
        output_dir=str(out3),
        work_dir=str(work3),
        params={"confirm_invalidation": True, "field_name": "Sig1"},
    )
    w3 = InMemoryWriter()
    handle(req3, w3)

    assert any(e.get("type") == "warning" and e.get("code") == "invalidated_signatures" for e in w3.events)
    res3 = next(e for e in w3.events if e["type"] == "result")
    assert res3["ok"] is True


def test_pdf_timestamp_tool(tmp_path: Path):
    from asn1crypto import keys as asn1_keys
    from asn1crypto import tsp
    from asn1crypto import x509 as asn1_x509
    from cryptography.hazmat.primitives import serialization
    from pyhanko.sign.timestamps import DummyTimeStamper
    import base64

    pdf_path = tmp_path / "doc.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    # Create test TSA
    key = rsa.generate_private_key(65537, 2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Test TSA")])
    now = datetime.now(UTC)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(1)
        .not_valid_before(now - timedelta(days=1))
        .not_valid_after(now + timedelta(days=365))
        .add_extension(
            x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.TIME_STAMPING]),
            critical=True,
        )
        .sign(key, hashes.SHA256())
    )
    cert_asn1 = asn1_x509.Certificate.load(cert.public_bytes(serialization.Encoding.DER))
    key_der = key.private_bytes(
        serialization.Encoding.DER,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )
    key_asn1 = asn1_keys.PrivateKeyInfo.load(key_der)
    d = DummyTimeStamper(tsa_cert=cert_asn1, tsa_key=key_asn1)

    req = tsp.TimeStampReq(
        {
            "version": 1,
            "message_imprint": tsp.MessageImprint(
                {
                    "hash_algorithm": {"algorithm": "sha256"},
                    "hashed_message": b"0" * 32,
                }
            ),
        }
    )
    resp = d.request_tsa_response(req)
    tok_b64 = base64.b64encode(resp["time_stamp_token"].dump()).decode("ascii")

    ts_req = RunToolRequest(
        job="job-ts",
        tool="pdf.timestamp",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(tmp_path / "ts_out"),
        work_dir=str(tmp_path / "work"),
        params={"timestamp_token": tok_b64},
    )
    writer = InMemoryWriter()
    handle(ts_req, writer)

    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is True
    art = next(e for e in writer.events if e["type"] == "artifact")
    assert Path(art["path"]).is_file()

