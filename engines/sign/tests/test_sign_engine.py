"""Tests for the digital signature engine."""

from __future__ import annotations

import io
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

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

    import shutil
    import subprocess

    if not shutil.which("qpdf"):
        pytest.skip("qpdf command not available")

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


def test_sign_token_tool(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
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

    # 1. Without ARIAD_TEST_SOFTWARE_TOKEN=1, mock software token is forbidden (deferred)
    writer = InMemoryWriter()
    handle(token_req, writer)
    res = next(e for e in writer.events if e["type"] == "result")
    assert res["ok"] is False
    assert res["error"]["code"] == "tool_missing"

    # 2. With ARIAD_TEST_SOFTWARE_TOKEN=1, software test backend succeeds
    monkeypatch.setenv("ARIAD_TEST_SOFTWARE_TOKEN", "1")
    writer2 = InMemoryWriter()
    handle(token_req, writer2)
    res2 = next(e for e in writer2.events if e["type"] == "result")
    assert res2["ok"] is True
    art = next(e for e in writer2.events if e["type"] == "artifact")
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


def test_pdf_timestamp_rejects_unbound_token(tmp_path: Path):
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

    # Token with mismatched imprint (b"0"*32) must be rejected with invalid_request
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
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_request"
    assert "imprint mismatch" in res["error"]["message"].lower()


class AutoRespondingWriter(Writer):
    """Writer that intercepts 'tsa_request' and writes valid TimeStampResp to work_dir."""

    def __init__(self, work_dir: Path, dummy_tsa: Any) -> None:
        self.work_dir = work_dir
        self.dummy_tsa = dummy_tsa
        self.events: list[dict] = []

    def emit(self, event: dict) -> None:
        self.events.append(event)
        if event.get("type") == "progress" and event.get("stage") == "tsa_request":
            from asn1crypto import tsp

            req_path = self.work_dir / "tsa_req.bin"
            resp_path = self.work_dir / "tsa_resp.bin"
            if req_path.is_file():
                req_bytes = req_path.read_bytes()
                req = tsp.TimeStampReq.load(req_bytes)
                resp = self.dummy_tsa.request_tsa_response(req)
                resp_path.write_bytes(resp.dump())


def create_dummy_tsa():
    from asn1crypto import keys as asn1_keys
    from asn1crypto import x509 as asn1_x509
    from cryptography.hazmat.primitives import serialization
    from pyhanko.sign.timestamps import DummyTimeStamper

    key = rsa.generate_private_key(65537, 2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Test TSA")])
    now = datetime.now(UTC)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(100)
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
    return DummyTimeStamper(tsa_cert=cert_asn1, tsa_key=key_asn1)


def generate_ca_and_evidence(tmp_path: Path):
    from asn1crypto import keys as asn1_keys
    from asn1crypto import x509 as asn1_x509
    from cryptography.hazmat.primitives import serialization
    from cryptography.x509 import ocsp
    from pyhanko.sign.timestamps import DummyTimeStamper

    now = datetime.now(UTC)
    ca_key = rsa.generate_private_key(65537, 2048)
    ca_name = x509.Name([
        x509.NameAttribute(NameOID.COUNTRY_NAME, "VN"),
        x509.NameAttribute(NameOID.ORGANIZATION_NAME, "AriadShift CA"),
        x509.NameAttribute(NameOID.COMMON_NAME, "Test CA Root"),
    ])
    ca_cert = (
        x509.CertificateBuilder()
        .subject_name(ca_name)
        .issuer_name(ca_name)
        .public_key(ca_key.public_key())
        .serial_number(1)
        .not_valid_before(now - timedelta(days=10))
        .not_valid_after(now + timedelta(days=365))
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
        .sign(ca_key, hashes.SHA256())
    )

    signer_key = rsa.generate_private_key(65537, 2048)
    signer_name = x509.Name([
        x509.NameAttribute(NameOID.COUNTRY_NAME, "VN"),
        x509.NameAttribute(NameOID.ORGANIZATION_NAME, "AriadShift Signer"),
        x509.NameAttribute(NameOID.COMMON_NAME, "Signer Under CA"),
    ])
    signer_cert = (
        x509.CertificateBuilder()
        .subject_name(signer_name)
        .issuer_name(ca_name)
        .public_key(signer_key.public_key())
        .serial_number(2)
        .not_valid_before(now - timedelta(days=1))
        .not_valid_after(now + timedelta(days=365))
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
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
        .sign(ca_key, hashes.SHA256())
    )

    tsa_key = rsa.generate_private_key(65537, 2048)
    tsa_name = x509.Name([
        x509.NameAttribute(NameOID.COUNTRY_NAME, "VN"),
        x509.NameAttribute(NameOID.ORGANIZATION_NAME, "AriadShift TSA"),
        x509.NameAttribute(NameOID.COMMON_NAME, "TSA Under CA"),
    ])
    tsa_cert = (
        x509.CertificateBuilder()
        .subject_name(tsa_name)
        .issuer_name(ca_name)
        .public_key(tsa_key.public_key())
        .serial_number(3)
        .not_valid_before(now - timedelta(days=1))
        .not_valid_after(now + timedelta(days=365))
        .add_extension(
            x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.TIME_STAMPING]),
            critical=True,
        )
        .sign(ca_key, hashes.SHA256())
    )

    tsa_cert_asn1 = asn1_x509.Certificate.load(tsa_cert.public_bytes(serialization.Encoding.DER))
    tsa_key_der = tsa_key.private_bytes(
        serialization.Encoding.DER,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )
    tsa_key_asn1 = asn1_keys.PrivateKeyInfo.load(tsa_key_der)
    dummy_tsa = DummyTimeStamper(tsa_cert=tsa_cert_asn1, tsa_key=tsa_key_asn1)

    ca_path = tmp_path / "ca.crt"
    ca_path.write_bytes(ca_cert.public_bytes(serialization.Encoding.DER))

    p12_bytes = pkcs12.serialize_key_and_certificates(
        name=b"test-signer",
        key=signer_key,
        cert=signer_cert,
        cas=[ca_cert],
        encryption_algorithm=BestAvailableEncryption(b"pass"),
    )
    p12_path = tmp_path / "signer.p12"
    p12_path.write_bytes(p12_bytes)

    crl = (
        x509.CertificateRevocationListBuilder()
        .issuer_name(ca_name)
        .last_update(now - timedelta(hours=1))
        .next_update(now + timedelta(days=30))
        .sign(ca_key, hashes.SHA256())
    )
    crl_path = tmp_path / "ca.crl"
    crl_path.write_bytes(crl.public_bytes(serialization.Encoding.DER))

    ocsp_signer = (
        ocsp.OCSPResponseBuilder()
        .add_response(
            cert=signer_cert,
            issuer=ca_cert,
            algorithm=hashes.SHA256(),
            cert_status=ocsp.OCSPCertStatus.GOOD,
            this_update=now - timedelta(hours=1),
            next_update=now + timedelta(days=7),
            revocation_time=None,
            revocation_reason=None,
        )
        .responder_id(ocsp.OCSPResponderEncoding.HASH, ca_cert)
        .sign(ca_key, hashes.SHA256())
    )
    signer_ocsp_path = tmp_path / "signer.ocsp"
    signer_ocsp_path.write_bytes(ocsp_signer.public_bytes(serialization.Encoding.DER))

    ocsp_tsa = (
        ocsp.OCSPResponseBuilder()
        .add_response(
            cert=tsa_cert,
            issuer=ca_cert,
            algorithm=hashes.SHA256(),
            cert_status=ocsp.OCSPCertStatus.GOOD,
            this_update=now - timedelta(hours=1),
            next_update=now + timedelta(days=7),
            revocation_time=None,
            revocation_reason=None,
        )
        .responder_id(ocsp.OCSPResponderEncoding.HASH, ca_cert)
        .sign(ca_key, hashes.SHA256())
    )
    tsa_ocsp_path = tmp_path / "tsa.ocsp"
    tsa_ocsp_path.write_bytes(ocsp_tsa.public_bytes(serialization.Encoding.DER))

    return ca_path, p12_path, crl_path, [str(signer_ocsp_path), str(tsa_ocsp_path)], dummy_tsa


def test_sign_pades_b_t_lt_lta(tmp_path: Path):
    ca_path, p12_path, crl_path, ocsp_paths, dummy_tsa = generate_ca_and_evidence(tmp_path)

    # 1. PAdES B-T
    work1 = tmp_path / "work_bt"
    work1.mkdir()
    out1 = tmp_path / "out_bt"
    out1.mkdir()
    req_bt = RunToolRequest(
        job="job-bt",
        tool="pdf.sign",
        inputs=[{"path": str(tmp_path / "doc.pdf"), "format": "pdf"}],
        output_dir=str(out1),
        work_dir=str(work1),
        params={
            "certificate": str(p12_path),
            "password": "pass",
            "level": "B-T",
            "tsa_url": "http://127.0.0.1:8080/tsa",
        },
    )
    (tmp_path / "doc.pdf").write_bytes(get_sample_pdf_bytes())
    w1 = AutoRespondingWriter(work1, dummy_tsa)
    handle(req_bt, w1)
    res1 = next(e for e in w1.events if e["type"] == "result")
    assert res1["ok"] is True
    bt_pdf = Path(next(e for e in w1.events if e["type"] == "artifact")["path"])
    assert bt_pdf.is_file()

    # Verify B-T
    v1_out = tmp_path / "v1_out"
    v1_out.mkdir()
    v_req1 = RunToolRequest(
        job="job-v-bt",
        tool="pdf.sign_verify",
        inputs=[{"path": str(bt_pdf), "format": "pdf"}],
        output_dir=str(v1_out),
        work_dir=str(work1),
        params={"trust_anchors": [str(ca_path)]},
    )
    vw1 = InMemoryWriter()
    handle(v_req1, vw1)
    rep1 = json.loads(Path(next(e for e in vw1.events if e["type"] == "artifact")["path"]).read_text("utf-8"))
    assert rep1["integrity"] is True
    assert rep1["signatures"][0]["integrity"] is True

    # 2. PAdES B-LT
    work2 = tmp_path / "work_blt"
    work2.mkdir()
    out2 = tmp_path / "out_blt"
    out2.mkdir()
    req_blt = RunToolRequest(
        job="job-blt",
        tool="pdf.sign",
        inputs=[{"path": str(tmp_path / "doc.pdf"), "format": "pdf"}],
        output_dir=str(out2),
        work_dir=str(work2),
        params={
            "certificate": str(p12_path),
            "password": "pass",
            "level": "B-LT",
            "tsa_url": "http://127.0.0.1:8080/tsa",
            "offline_evidence": {
                "trust_anchors": [str(ca_path)],
                "crl_files": [str(crl_path)],
                "ocsp_responses": ocsp_paths,
            },
        },
    )
    w2 = AutoRespondingWriter(work2, dummy_tsa)
    handle(req_blt, w2)
    res2 = next(e for e in w2.events if e["type"] == "result")
    assert res2["ok"] is True
    blt_pdf = Path(next(e for e in w2.events if e["type"] == "artifact")["path"])

    # Verify B-LT
    v2_out = tmp_path / "v2_out"
    v2_out.mkdir()
    v_req2 = RunToolRequest(
        job="job-v-blt",
        tool="pdf.sign_verify",
        inputs=[{"path": str(blt_pdf), "format": "pdf"}],
        output_dir=str(v2_out),
        work_dir=str(work2),
        params={
            "trust_anchors": [str(ca_path)],
            "offline_evidence": [str(crl_path)] + ocsp_paths,
        },
    )
    vw2 = InMemoryWriter()
    handle(v_req2, vw2)
    rep2 = json.loads(Path(next(e for e in vw2.events if e["type"] == "artifact")["path"]).read_text("utf-8"))
    assert rep2["integrity"] is True
    assert rep2["signatures"][0]["integrity"] is True

    # 3. PAdES B-LTA
    work3 = tmp_path / "work_blta"
    work3.mkdir()
    out3 = tmp_path / "out_blta"
    out3.mkdir()
    req_blta = RunToolRequest(
        job="job-blta",
        tool="pdf.sign",
        inputs=[{"path": str(tmp_path / "doc.pdf"), "format": "pdf"}],
        output_dir=str(out3),
        work_dir=str(work3),
        params={
            "certificate": str(p12_path),
            "password": "pass",
            "level": "B-LTA",
            "tsa_url": "http://127.0.0.1:8080/tsa",
            "offline_evidence": {
                "trust_anchors": [str(ca_path)],
                "crl_files": [str(crl_path)],
                "ocsp_responses": ocsp_paths,
            },
        },
    )
    w3 = AutoRespondingWriter(work3, dummy_tsa)
    handle(req_blta, w3)
    res3 = next(e for e in w3.events if e["type"] == "result")
    assert res3["ok"] is True
    blta_pdf = Path(next(e for e in w3.events if e["type"] == "artifact")["path"])

    # Verify B-LTA
    v3_out = tmp_path / "v3_out"
    v3_out.mkdir()
    v_req3 = RunToolRequest(
        job="job-v-blta",
        tool="pdf.sign_verify",
        inputs=[{"path": str(blta_pdf), "format": "pdf"}],
        output_dir=str(v3_out),
        work_dir=str(work3),
        params={
            "trust_anchors": [str(ca_path)],
            "offline_evidence": [str(crl_path)] + ocsp_paths,
        },
    )
    vw3 = InMemoryWriter()
    handle(v_req3, vw3)
    rep3 = json.loads(Path(next(e for e in vw3.events if e["type"] == "artifact")["path"]).read_text("utf-8"))
    assert rep3["integrity"] is True
    assert rep3["signatures"][0]["integrity"] is True


def test_sign_tampered_pdf(tmp_path: Path):
    pdf_path = tmp_path / "sample.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    p12_bytes = generate_test_p12("secret")
    p12_path = tmp_path / "cert.p12"
    p12_path.write_bytes(p12_bytes)

    out_dir = tmp_path / "out"
    work_dir = tmp_path / "work"
    out_dir.mkdir()
    work_dir.mkdir()

    sign_req = RunToolRequest(
        job="job-sign",
        tool="pdf.sign",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(out_dir),
        work_dir=str(work_dir),
        params={"certificate": str(p12_path), "password": "secret", "level": "B-B"},
    )
    w = InMemoryWriter()
    handle(sign_req, w)
    art = next(e for e in w.events if e["type"] == "artifact")
    signed_path = Path(art["path"])

    # Tamper with the signed PDF by flipping bytes in page content
    content = bytearray(signed_path.read_bytes())
    tamper_idx = content.find(b"/MediaBox")
    assert tamper_idx > 0
    content[tamper_idx] ^= 0xFF
    tampered_path = tmp_path / "tampered.pdf"
    tampered_path.write_bytes(content)

    v_dir = tmp_path / "v_out"
    v_dir.mkdir()
    verify_req = RunToolRequest(
        job="job-v-tamper",
        tool="pdf.sign_verify",
        inputs=[{"path": str(tampered_path), "format": "pdf"}],
        output_dir=str(v_dir),
        work_dir=str(work_dir),
        params={},
    )
    vw = InMemoryWriter()
    handle(verify_req, vw)
    v_res = next(e for e in vw.events if e["type"] == "result")
    assert v_res["ok"] is True
    report_file = Path(next(e for e in vw.events if e["type"] == "artifact")["path"])
    report = json.loads(report_file.read_text("utf-8"))
    assert report["integrity"] is False
    assert report["valid"] is False
    assert report["signatures"][0]["integrity"] is False


def test_sign_expired_cert_reports_invalid(tmp_path: Path):
    pdf_path = tmp_path / "sample.pdf"
    pdf_path.write_bytes(get_sample_pdf_bytes())

    # Expired cert (validity ended 30 days ago)
    now = datetime.now(UTC)
    key = rsa.generate_private_key(65537, 2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Expired Signer")])
    exp_cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(1)
        .not_valid_before(now - timedelta(days=400))
        .not_valid_after(now - timedelta(days=30))
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .sign(key, hashes.SHA256())
    )
    exp_p12 = pkcs12.serialize_key_and_certificates(
        name=b"expired",
        key=key,
        cert=exp_cert,
        cas=None,
        encryption_algorithm=BestAvailableEncryption(b"secret"),
    )
    exp_p12_path = tmp_path / "expired.p12"
    exp_p12_path.write_bytes(exp_p12)

    out_dir = tmp_path / "out_exp"
    out_dir.mkdir()
    work_dir = tmp_path / "work_exp"
    work_dir.mkdir()
    sign_req = RunToolRequest(
        job="job-exp",
        tool="pdf.sign",
        inputs=[{"path": str(pdf_path), "format": "pdf"}],
        output_dir=str(out_dir),
        work_dir=str(work_dir),
        params={"certificate": str(exp_p12_path), "password": "secret", "level": "B-B"},
    )
    w = InMemoryWriter()
    handle(sign_req, w)
    art = next(e for e in w.events if e["type"] == "artifact")
    signed_exp_path = Path(art["path"])

    v_dir = tmp_path / "v_exp"
    v_dir.mkdir()
    v_req = RunToolRequest(
        job="job-v-exp",
        tool="pdf.sign_verify",
        inputs=[{"path": str(signed_exp_path), "format": "pdf"}],
        output_dir=str(v_dir),
        work_dir=str(work_dir),
        params={},
    )
    vw = InMemoryWriter()
    handle(v_req, vw)
    rep = json.loads(Path(next(e for e in vw.events if e["type"] == "artifact")["path"]).read_text("utf-8"))
    assert rep["signatures"][0]["integrity"] is True
    assert rep["signatures"][0]["valid"] is False
    assert rep["valid"] is False


def test_second_incremental_signature_preserves_first(tmp_path: Path):
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

    # Second signature on top of first
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

    # Verify both signatures on the final document
    out_v = tmp_path / "out_v"
    out_v.mkdir()
    req_v = RunToolRequest(
        job="v2",
        tool="pdf.sign_verify",
        inputs=[{"path": str(sig2_pdf), "format": "pdf"}],
        output_dir=str(out_v),
        work_dir=str(work2),
        params={},
    )
    wv = InMemoryWriter()
    handle(req_v, wv)
    rep = json.loads(Path(next(e for e in wv.events if e["type"] == "artifact")["path"]).read_text("utf-8"))
    assert rep["signatures_count"] == 2
    assert rep["integrity"] is True
    assert all(s["integrity"] is True for s in rep["signatures"])

