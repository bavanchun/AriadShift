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
