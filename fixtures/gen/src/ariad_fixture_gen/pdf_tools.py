"""Generate deterministic PDF tool test corpus fixtures and truth companions.

Encrypted fixtures (AES-256 user+owner, AES-256 owner-only, and RC4-40) are not
committed in encrypted form because AES uses random IVs per save. Their
deterministic plaintext source PDFs and truth companions (without passwords) are
committed, and test-time helpers in this module materialize the encrypted bytes
from the committed plaintext sources.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from io import BytesIO
import json
from pathlib import Path
import re
from typing import Any

import pikepdf
from pikepdf import Array, Dictionary, Name, ObjectStreamMode, Permissions, String
import typst

from ariad_fixture_gen import scan
from ariad_fixture_gen.manifest import update_generated_hashes

_AUTHOR = "AriadShift contributors"
_PRODUCER = "AriadShift fixture generator"
_CREATOR = "AriadShift fixture generator"
_CREATION_DATE = "D:20231114221320Z"
_MOD_DATE = "D:20231114221320Z"
_MOD_DATE_REV2 = "D:20231115000000Z"
_TIMESTAMP = 1_700_000_000

A4_PORTRAIT: tuple[float, float] = (595.28, 841.89)
LETTER_PORTRAIT: tuple[float, float] = (612.0, 792.0)
A3_LANDSCAPE: tuple[float, float] = (1190.55, 841.89)

PERSONAL_DATA_SAMPLES: dict[str, str] = {
    "name": "Nguyen Van An",
    "national_id": "CCCD-079203001234",
    "email": "nguyen.van.an@example.vi",
    "phone": "+84-912-345-678",
}


@dataclass(frozen=True)
class EncryptedFixtureSpec:
    """Test-time encryption specification; passwords live here, never in truth files."""

    stem: str
    plaintext_rel_path: str
    target_rel_path: str
    truth_rel_path: str
    title: str
    algorithm: str
    revision: int
    version: int
    user_password: str
    owner_password: str
    allow_print: bool
    allow_extract: bool


# Passwords live exclusively in this test helper, never in committed *.truth.json files.
ENCRYPTED_FIXTURE_SPECS: dict[str, EncryptedFixtureSpec] = {
    "encrypted-aes256": EncryptedFixtureSpec(
        stem="encrypted-aes256",
        plaintext_rel_path="fixtures/pdf-tools/encrypted-aes256.plaintext.pdf",
        target_rel_path="fixtures/pdf-tools/encrypted-aes256.pdf",
        truth_rel_path="fixtures/pdf-tools/encrypted-aes256.truth.json",
        title="Encrypted Document (AES-256 User and Owner)",
        algorithm="AES-256",
        revision=6,
        version=5,
        user_password="ariad-test-user-aes256",
        owner_password="ariad-test-owner-aes256",
        allow_print=True,
        allow_extract=True,
    ),
    "encrypted-owner-only": EncryptedFixtureSpec(
        stem="encrypted-owner-only",
        plaintext_rel_path="fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf",
        target_rel_path="fixtures/pdf-tools/encrypted-owner-only.pdf",
        truth_rel_path="fixtures/pdf-tools/encrypted-owner-only.truth.json",
        title="Encrypted Document (Owner Password Only)",
        algorithm="AES-256",
        revision=6,
        version=5,
        user_password="",
        owner_password="ariad-test-owner-only",
        allow_print=False,
        allow_extract=False,
    ),
    "encrypted-rc4-40": EncryptedFixtureSpec(
        stem="encrypted-rc4-40",
        plaintext_rel_path="fixtures/pdf-tools/encrypted-rc4-40.plaintext.pdf",
        target_rel_path="fixtures/pdf-tools/encrypted-rc4-40.pdf",
        truth_rel_path="fixtures/pdf-tools/encrypted-rc4-40.truth.json",
        title="Encrypted Document (RC4-40 Legacy)",
        algorithm="RC4-40",
        revision=2,
        version=1,
        user_password="ariad-user-rc4",
        owner_password="ariad-owner-rc4",
        allow_print=True,
        allow_extract=True,
    ),
}


def _pdf_escape(text: str) -> str:
    return (
        text.replace("\\", "\\\\")
        .replace("(", "\\(")
        .replace(")", "\\)")
    )


def _new_pdf(title: str, *, language: str = "en") -> tuple[pikepdf.Pdf, pikepdf.Object]:
    pdf = pikepdf.Pdf.new()
    if Name.Metadata in pdf.Root:
        del pdf.Root[Name.Metadata]
    pdf.Root[Name.Lang] = String(language)
    pdf.docinfo[Name.Title] = String(title)
    pdf.docinfo[Name.Author] = String(_AUTHOR)
    pdf.docinfo[Name.Creator] = String(_CREATOR)
    pdf.docinfo[Name.Producer] = String(_PRODUCER)
    pdf.docinfo[Name.CreationDate] = String(_CREATION_DATE)
    pdf.docinfo[Name.ModDate] = String(_MOD_DATE)
    font = pdf.make_indirect(
        Dictionary(
            Type=Name.Font,
            Subtype=Name.Type1,
            BaseFont=Name.Helvetica,
        )
    )
    resources = pdf.make_indirect(Dictionary(Font=Dictionary(F1=font)))
    return pdf, resources


def _save_deterministic(
    pdf: pikepdf.Pdf,
    *,
    force_version: str = "1.5",
    object_stream_mode: ObjectStreamMode = ObjectStreamMode.disable,
    compress_streams: bool = True,
    linearize: bool = False,
) -> bytes:
    if Name.Metadata in pdf.Root:
        del pdf.Root[Name.Metadata]
    pdf.docinfo[Name.Producer] = String(_PRODUCER)
    bio = BytesIO()
    pdf.save(
        bio,
        force_version=force_version,
        object_stream_mode=object_stream_mode,
        compress_streams=compress_streams,
        linearize=linearize,
        deterministic_id=True,
        fix_metadata_version=False,
    )
    return bio.getvalue()


def _page_stream(lines: list[str], *, x: float = 72.0, y_start: float = 760.0, leading: float = 18.0) -> bytes:
    ops = ["BT", "/F1 12 Tf"]
    for idx, line in enumerate(lines):
        y = y_start - idx * leading
        ops.append(f"1 0 0 1 {x:.2f} {y:.2f} Tm ({_pdf_escape(line)}) Tj")
    ops.append("ET")
    return ("\n".join(ops) + "\n").encode("ascii")


def _truth_envelope(
    *,
    doc_id: str,
    title: str,
    language: str = "en",
    pdf_version: str = "1.5",
    page_count: int,
    pages: list[dict[str, Any]],
    encrypted: bool = False,
    linearized: bool = False,
    has_forms: bool = False,
    field_names: list[str] | None = None,
    form_fields: list[dict[str, Any]] | None = None,
    has_annotations: bool = False,
    annotations: list[dict[str, Any]] | None = None,
    has_attachments: bool = False,
    attachments: list[dict[str, Any]] | None = None,
    has_javascript: bool = False,
    javascript: dict[str, Any] | None = None,
    outline_count: int = 0,
    outlines: list[dict[str, Any]] | None = None,
    page_labels: list[dict[str, Any]] | None = None,
    watermark_kind: str | None = None,
    watermark: dict[str, Any] | None = None,
    text_lines: list[str] | None = None,
    extra: dict[str, Any] | None = None,
) -> bytes:
    sizes = [
        {
            "page": p["page"],
            "width_pt": p["width_pt"],
            "height_pt": p["height_pt"],
            "paper": p.get("paper", "A4"),
            "orientation": p.get("orientation", "portrait"),
        }
        for p in pages
    ]
    rotations = [int(p.get("rotation", 0)) for p in pages]
    metadata = {
        "title": title,
        "author": _AUTHOR,
        "creator": _CREATOR,
        "producer": _PRODUCER,
        "creation_date": _CREATION_DATE,
        "mod_date": _MOD_DATE,
        "language": language,
    }
    pdf_info = {
        "version": pdf_version,
        "page_count": page_count,
        "encrypted": encrypted,
        "linearized": linearized,
        "pages": pages,
        "metadata": metadata,
        "has_forms": has_forms,
        "has_annotations": has_annotations,
        "has_attachments": has_attachments,
        "has_javascript": has_javascript,
        "outline_count": outline_count,
    }
    payload: dict[str, Any] = {
        "version": "ariad-truth/0",
        "id": doc_id,
        "unscored": True,
        "unscored_reason": "PDF tools test corpus fixture (routes = [])",
        "pdf_version": pdf_version,
        "page_count": page_count,
        "encrypted": encrypted,
        "linearized": linearized,
        "pages": pages,
        "sizes": sizes,
        "rotations": rotations,
        "metadata": metadata,
        "has_forms": has_forms,
        "field_names": field_names or [],
        "form_fields": form_fields or [],
        "has_annotations": has_annotations,
        "annotations": annotations or [],
        "has_attachments": has_attachments,
        "attachments": attachments or [],
        "has_javascript": has_javascript,
        "javascript": javascript,
        "outline_count": outline_count,
        "outlines": outlines or [],
        "page_labels": page_labels or [],
        "watermark_kind": watermark_kind,
        "watermark": watermark,
        "pdf_info": pdf_info,
        "canonical": {
            "text": "\n".join(text_lines if text_lines is not None else [title]),
            "headings": [],
            "tables": [],
            "lists": [],
            "image_count": 0,
            "link_count": 0,
        },
    }
    if extra:
        payload.update(extra)
    return (json.dumps(payload, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def _default_page_info(page_no: int, *, width: float = 595.28, height: float = 841.89, paper: str = "A4", orientation: str = "portrait", rotation: int = 0, inherited_rotation: bool = False, blank: bool = False) -> dict[str, Any]:
    info: dict[str, Any] = {
        "page": page_no,
        "width_pt": width,
        "height_pt": height,
        "paper": paper,
        "orientation": orientation,
        "rotation": rotation,
        "blank": blank,
    }
    if inherited_rotation:
        info["inherited_rotation"] = True
    return info


# ---------------------------------------------------------------------------
# 1. multipage-20.pdf
# ---------------------------------------------------------------------------
def _build_multipage_20() -> tuple[bytes, bytes]:
    title = "Twenty-Page Survey Manual"
    pdf, resources = _new_pdf(title)
    text_lines = [title]
    pages_truth: list[dict[str, Any]] = []

    chapter_starts = {
        1: "Chapter 1: Orientation",
        2: "Section 1.1: Benchmarks",
        6: "Chapter 2: River Crossings",
        8: "Section 2.1: Bridge Soundings",
        11: "Chapter 3: Terrace Contours",
        16: "Chapter 4: Appendix Tables",
    }

    for i in range(1, 21):
        lines = [title, f"Page {i} of 20"]
        if i in chapter_starts:
            lines.append(chapter_starts[i])
        lines.append(f"Survey station record #{i:02d} along the river corridor.")
        text_lines.extend(lines[1:])
        stream = pdf.make_stream(_page_stream(lines))
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                    Resources=resources,
                    Contents=stream,
                )
            )
        )
        pages_truth.append(_default_page_info(i))

    # Build /Outlines (4 top-level chapters, 2 nested sections = 6 outline entries)
    with pdf.open_outline() as outline:
        ch1 = pikepdf.OutlineItem("Chapter 1: Orientation", 0)
        ch1.children.append(pikepdf.OutlineItem("Section 1.1: Benchmarks", 1))
        ch2 = pikepdf.OutlineItem("Chapter 2: River Crossings", 5)
        ch2.children.append(pikepdf.OutlineItem("Section 2.1: Bridge Soundings", 7))
        ch3 = pikepdf.OutlineItem("Chapter 3: Terrace Contours", 10)
        ch4 = pikepdf.OutlineItem("Chapter 4: Appendix Tables", 15)
        outline.root.extend([ch1, ch2, ch3, ch4])

    # Build /PageLabels number tree:
    # 0..1 -> i, ii (roman-lower); 2..17 -> 1..16 (decimal); 18..19 -> A-A, A-B (alpha-upper with prefix A-)
    pdf.Root[Name.PageLabels] = Dictionary(
        Nums=Array(
            [
                0,
                Dictionary(S=Name.r, St=1),
                2,
                Dictionary(S=Name.D, St=1),
                18,
                Dictionary(S=Name.A, P=String("A-"), St=1),
            ]
        )
    )

    outlines_truth = [
        {
            "title": "Chapter 1: Orientation",
            "page": 1,
            "children": [{"title": "Section 1.1: Benchmarks", "page": 2, "children": []}],
        },
        {
            "title": "Chapter 2: River Crossings",
            "page": 6,
            "children": [{"title": "Section 2.1: Bridge Soundings", "page": 8, "children": []}],
        },
        {"title": "Chapter 3: Terrace Contours", "page": 11, "children": []},
        {"title": "Chapter 4: Appendix Tables", "page": 16, "children": []},
    ]
    page_labels_truth = [
        {"start_page": 1, "style": "roman-lower", "prefix": "", "start": 1},
        {"start_page": 3, "style": "decimal", "prefix": "", "start": 1},
        {"start_page": 19, "style": "alpha-upper", "prefix": "A-", "start": 1},
    ]

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="multipage-20",
        title=title,
        page_count=20,
        pages=pages_truth,
        outline_count=6,
        outlines=outlines_truth,
        page_labels=page_labels_truth,
        text_lines=text_lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 2. mixed-sizes.pdf
# ---------------------------------------------------------------------------
def _build_mixed_sizes() -> tuple[bytes, bytes]:
    title = "Mixed Paper Sizes Portfolio"
    pdf, resources = _new_pdf(title)
    specs = [
        (1, A4_PORTRAIT[0], A4_PORTRAIT[1], "A4", "portrait", "Page 1: ISO A4 Portrait (595.28 x 841.89 pt)"),
        (2, LETTER_PORTRAIT[0], LETTER_PORTRAIT[1], "Letter", "portrait", "Page 2: US Letter Portrait (612 x 792 pt)"),
        (3, A3_LANDSCAPE[0], A3_LANDSCAPE[1], "A3", "landscape", "Page 3: ISO A3 Landscape (1190.55 x 841.89 pt)"),
    ]
    pages_truth = []
    text_lines = [title]
    for page_no, w, h, paper, orient, label in specs:
        stream = pdf.make_stream(_page_stream([title, label], y_start=h - 72.0))
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, w, h]),
                    Resources=resources,
                    Contents=stream,
                )
            )
        )
        pages_truth.append(_default_page_info(page_no, width=w, height=h, paper=paper, orientation=orient))
        text_lines.append(label)

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="mixed-sizes",
        title=title,
        page_count=3,
        pages=pages_truth,
        text_lines=text_lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 3. rotated-pages.pdf
# ---------------------------------------------------------------------------
def _build_rotated_pages() -> tuple[bytes, bytes]:
    title = "Rotated Pages Inspection Sheet"
    pdf, resources = _new_pdf(title)
    # Set /Rotate 90 on the parent /Pages node so page 2 inherits 90 without its own /Rotate key
    pdf.Root.Pages[Name.Rotate] = 90
    rotations = [
        (1, 0, False, "Page 1: Explicit /Rotate 0"),
        (2, 90, True, "Page 2: Inherited /Rotate 90 from /Pages parent"),
        (3, 180, False, "Page 3: Explicit /Rotate 180"),
        (4, 270, False, "Page 4: Explicit /Rotate 270"),
    ]
    pages_truth = []
    text_lines = [title]
    for page_no, rot, inherited, label in rotations:
        stream = pdf.make_stream(_page_stream([title, label]))
        page_dict = Dictionary(
            Type=Name.Page,
            MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
            Resources=resources,
            Contents=stream,
        )
        if not inherited:
            page_dict[Name.Rotate] = rot
        pdf.pages.append(pikepdf.Page(page_dict))
        pages_truth.append(
            _default_page_info(page_no, rotation=rot, inherited_rotation=inherited)
        )
        text_lines.append(label)

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="rotated-pages",
        title=title,
        page_count=4,
        pages=pages_truth,
        text_lines=text_lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 4, 5, 6. Encrypted fixtures (plaintext committed; encrypted generated at test time)
# ---------------------------------------------------------------------------
def _build_encrypted_plaintext(spec: EncryptedFixtureSpec) -> tuple[bytes, bytes]:
    pdf, resources = _new_pdf(spec.title)
    body_line = f"Plaintext source for test-time {spec.stem} ({spec.algorithm}, R={spec.revision})."
    stream = pdf.make_stream(_page_stream([spec.title, body_line]))
    pdf.pages.append(
        pikepdf.Page(
            Dictionary(
                Type=Name.Page,
                MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                Resources=resources,
                Contents=stream,
            )
        )
    )
    plaintext_bytes = _save_deterministic(
        pdf,
        force_version="1.7" if spec.revision >= 6 else "1.4",
    )
    plaintext_sha256 = hashlib.sha256(plaintext_bytes).hexdigest()
    pages_truth = [_default_page_info(1)]
    truth_bytes = _truth_envelope(
        doc_id=spec.stem,
        title=spec.title,
        pdf_version="1.7" if spec.revision >= 6 else "1.4",
        page_count=1,
        pages=pages_truth,
        encrypted=True,
        text_lines=[spec.title, body_line],
        extra={
            "generated_at_test_time": True,
            "plaintext_source": spec.plaintext_rel_path,
            "plaintext_sha256": plaintext_sha256,
            "target_path": spec.target_rel_path,
            "encryption": {
                "algorithm": spec.algorithm,
                "revision": spec.revision,
                "version": spec.version,
                "needs_user_password": bool(spec.user_password),
                "has_owner_password": True,
                "empty_user_password": spec.user_password == "",
                "permissions": {
                    "print": spec.allow_print,
                    "extract": spec.allow_extract,
                },
            },
        },
    )
    return plaintext_bytes, truth_bytes


def generate_encrypted_fixture(stem: str, root: Path | None = None) -> bytes:
    """Generate an encrypted PDF at test time from its committed plaintext source."""
    if stem not in ENCRYPTED_FIXTURE_SPECS:
        raise KeyError(f"Unknown encrypted fixture stem: {stem!r}")
    spec = ENCRYPTED_FIXTURE_SPECS[stem]
    repo_root = root or Path(__file__).resolve().parents[4]
    plaintext_path = repo_root / spec.plaintext_rel_path
    plaintext_bytes = plaintext_path.read_bytes()

    allow = Permissions(
        print_lowres=spec.allow_print,
        print_highres=spec.allow_print,
        extract=spec.allow_extract,
        modify_annotation=spec.allow_extract,
        modify_form=spec.allow_extract,
        modify_other=spec.allow_extract,
        modify_assembly=spec.allow_extract,
    )
    if spec.revision == 2:
        encryption = pikepdf.Encryption(
            user=spec.user_password,
            owner=spec.owner_password,
            R=2,
            aes=False,
            metadata=False,
            allow=allow,
        )
    else:
        encryption = pikepdf.Encryption(
            user=spec.user_password,
            owner=spec.owner_password,
            R=spec.revision,
            allow=allow,
        )

    with pikepdf.open(BytesIO(plaintext_bytes)) as pdf:
        out = BytesIO()
        pdf.save(
            out,
            encryption=encryption,
            fix_metadata_version=False,
        )
        return out.getvalue()


def materialize_encrypted_fixtures(out_dir: Path, root: Path | None = None) -> dict[str, Path]:
    """Write all three test-time encrypted fixtures into ``out_dir`` and return their paths."""
    out_dir.mkdir(parents=True, exist_ok=True)
    written: dict[str, Path] = {}
    for stem in ENCRYPTED_FIXTURE_SPECS:
        dest = out_dir / f"{stem}.pdf"
        dest.write_bytes(generate_encrypted_fixture(stem, root=root))
        written[stem] = dest
    return written


# ---------------------------------------------------------------------------
# 7. form-acroform.pdf (text, checkbox, radio, combo, signature field)
# ---------------------------------------------------------------------------
def _build_form_acroform() -> tuple[bytes, bytes]:
    title = "AcroForm Interactive Registration Sheet"
    pdf, resources = _new_pdf(title)
    font = resources[Name.Font][Name.F1]
    zapf = pdf.make_indirect(
        Dictionary(Type=Name.Font, Subtype=Name.Type1, BaseFont=Name.ZapfDingbats)
    )
    dr_resources = pdf.make_indirect(
        Dictionary(Font=Dictionary(F1=font, Helv=font, ZaDb=zapf))
    )

    lines = [
        title,
        "Full Name:",
        "Subscribe to bulletin:",
        "Delivery mode:",
        "Department:",
        "Approver signature:",
    ]
    stream = pdf.make_stream(_page_stream(lines, y_start=760.0, leading=42.0))

    # 1. Text field (full_name)
    tx_ap = pdf.make_stream(b"q 0.95 g 0 0 260 24 re f 0 G 1 w 0.5 0.5 259 23 re s Q\n")
    tx_ap[Name.Type] = Name.XObject
    tx_ap[Name.Subtype] = Name.Form
    tx_ap[Name.BBox] = Array([0, 0, 260, 24])
    tx_field = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            FT=Name.Tx,
            T=String("full_name"),
            V=String(""),
            DV=String(""),
            DA=String("/Helv 11 Tf 0 g"),
            Rect=Array([180, 706, 440, 730]),
            F=4,
            AP=Dictionary(N=tx_ap),
        )
    )

    # 2. Checkbox field (subscribe_news)
    cb_off = pdf.make_stream(b"q 1 g 0 0 18 18 re f 0 G 1 w 0.5 0.5 17 17 re s Q\n")
    cb_off[Name.Type] = Name.XObject
    cb_off[Name.Subtype] = Name.Form
    cb_off[Name.BBox] = Array([0, 0, 18, 18])
    cb_yes = pdf.make_stream(b"q 1 g 0 0 18 18 re f 0 G 1 w 0.5 0.5 17 17 re s 3 3 m 15 15 l 3 15 m 15 3 l S Q\n")
    cb_yes[Name.Type] = Name.XObject
    cb_yes[Name.Subtype] = Name.Form
    cb_yes[Name.BBox] = Array([0, 0, 18, 18])
    cb_field = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            FT=Name.Btn,
            T=String("subscribe_news"),
            V=Name.Off,
            AS=Name.Off,
            Rect=Array([180, 664, 198, 682]),
            F=4,
            AP=Dictionary(N=Dictionary(Yes=cb_yes, Off=cb_off)),
        )
    )

    # 3. Radio button group (delivery_mode) with two kids (/standard, /express)
    radio_parent = pdf.make_indirect(
        Dictionary(
            FT=Name.Btn,
            Ff=49152,  # Radio (32768) | NoToggleToOff (16384)
            T=String("delivery_mode"),
            V=Name.standard,
        )
    )
    radio_kid_1 = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            Parent=radio_parent,
            Rect=Array([180, 622, 198, 640]),
            F=4,
            AS=Name.standard,
            AP=Dictionary(N=Dictionary(standard=cb_yes, Off=cb_off)),
        )
    )
    radio_kid_2 = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            Parent=radio_parent,
            Rect=Array([260, 622, 278, 640]),
            F=4,
            AS=Name.Off,
            AP=Dictionary(N=Dictionary(express=cb_yes, Off=cb_off)),
        )
    )
    radio_parent[Name.Kids] = Array([radio_kid_1, radio_kid_2])

    # 4. Combo box field (department)
    combo_field = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            FT=Name.Ch,
            Ff=131072,  # Combo bit 18
            T=String("department"),
            Opt=Array([String("Archive"), String("Cartography"), String("Conservation")]),
            V=String("Cartography"),
            DV=String("Cartography"),
            DA=String("/Helv 11 Tf 0 g"),
            Rect=Array([180, 578, 360, 602]),
            F=4,
            AP=Dictionary(N=tx_ap),
        )
    )

    # 5. Signature field (approver_signature)
    sig_ap = pdf.make_stream(b"q 0.98 g 0 0 220 44 re f 0.4 G 1 w 0.5 0.5 219 43 re s Q\n")
    sig_ap[Name.Type] = Name.XObject
    sig_ap[Name.Subtype] = Name.Form
    sig_ap[Name.BBox] = Array([0, 0, 220, 44])
    sig_field = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            FT=Name.Sig,
            T=String("approver_signature"),
            Rect=Array([180, 514, 400, 558]),
            F=4,
            AP=Dictionary(N=sig_ap),
        )
    )

    page_dict = Dictionary(
        Type=Name.Page,
        MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
        Resources=resources,
        Contents=stream,
        Annots=Array([tx_field, cb_field, radio_kid_1, radio_kid_2, combo_field, sig_field]),
    )
    pdf.pages.append(pikepdf.Page(page_dict))
    page_ref = pdf.pages[0].obj
    for widget in (tx_field, cb_field, radio_kid_1, radio_kid_2, combo_field, sig_field):
        widget[Name.P] = page_ref

    pdf.Root[Name.AcroForm] = Dictionary(
        Fields=Array([tx_field, cb_field, radio_parent, combo_field, sig_field]),
        DA=String("/Helv 11 Tf 0 g"),
        DR=dr_resources,
    )

    form_fields_truth = [
        {"name": "full_name", "type": "text", "value": "", "page": 1, "rect": [180, 706, 440, 730]},
        {"name": "subscribe_news", "type": "checkbox", "value": "Off", "page": 1, "rect": [180, 664, 198, 682]},
        {
            "name": "delivery_mode",
            "type": "radio",
            "value": "standard",
            "options": ["standard", "express"],
            "page": 1,
            "rects": [[180, 622, 198, 640], [260, 622, 278, 640]],
        },
        {
            "name": "department",
            "type": "combo",
            "value": "Cartography",
            "options": ["Archive", "Cartography", "Conservation"],
            "page": 1,
            "rect": [180, 578, 360, 602],
        },
        {"name": "approver_signature", "type": "signature", "value": None, "signed": False, "page": 1, "rect": [180, 514, 400, 558]},
    ]
    field_names = [f["name"] for f in form_fields_truth]

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="form-acroform",
        title=title,
        page_count=1,
        pages=[_default_page_info(1)],
        has_forms=True,
        field_names=field_names,
        form_fields=form_fields_truth,
        has_annotations=True,
        text_lines=lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 8. annotations.pdf (highlight, note, ink, link)
# ---------------------------------------------------------------------------
def _build_annotations() -> tuple[bytes, bytes]:
    title = "Annotated Archive Review Sheet"
    pdf, resources = _new_pdf(title)
    lines = [
        title,
        "Highlighted key term on this line.",
        "Margin note attached to this observation.",
        "Freehand ink mark below.",
        "External reference link: https://example.org/map-room",
    ]
    stream = pdf.make_stream(_page_stream(lines, y_start=740.0, leading=36.0))

    hl_annot = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Highlight,
            Rect=Array([72, 696, 265, 714]),
            QuadPoints=Array([72, 714, 265, 714, 72, 696, 265, 696]),
            C=Array([1.0, 0.92, 0.23]),
            T=String("Reviewer"),
            Contents=String("Highlighted key term"),
            F=4,
        )
    )
    note_annot = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Text,
            Rect=Array([320, 660, 340, 680]),
            Name=Name.Comment,
            C=Array([1.0, 0.8, 0.2]),
            T=String("Reviewer"),
            Contents=String("Sticky note comment on the second paragraph"),
            Open=False,
            F=4,
        )
    )
    ink_annot = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Ink,
            Rect=Array([72, 610, 240, 645]),
            InkList=Array([Array([75, 620, 120, 640, 175, 618, 235, 638])]),
            C=Array([0.1, 0.35, 0.85]),
            T=String("Reviewer"),
            Contents=String("Freehand ink stroke"),
            F=4,
        )
    )
    link_annot = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Link,
            Rect=Array([72, 580, 340, 602]),
            Border=Array([0, 0, 0]),
            A=Dictionary(
                Type=Name.Action,
                S=Name.URI,
                URI=String("https://example.org/map-room"),
            ),
            F=4,
        )
    )

    pdf.pages.append(
        pikepdf.Page(
            Dictionary(
                Type=Name.Page,
                MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                Resources=resources,
                Contents=stream,
                Annots=Array([hl_annot, note_annot, ink_annot, link_annot]),
            )
        )
    )

    annotations_truth = [
        {"subtype": "Highlight", "page": 1, "rect": [72, 696, 265, 714], "contents": "Highlighted key term"},
        {"subtype": "Text", "page": 1, "rect": [320, 660, 340, 680], "contents": "Sticky note comment on the second paragraph"},
        {"subtype": "Ink", "page": 1, "rect": [72, 610, 240, 645], "contents": "Freehand ink stroke"},
        {"subtype": "Link", "page": 1, "rect": [72, 580, 340, 602], "uri": "https://example.org/map-room"},
    ]

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="annotations",
        title=title,
        page_count=1,
        pages=[_default_page_info(1)],
        has_annotations=True,
        annotations=annotations_truth,
        text_lines=lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 9. with-attachments.pdf
# ---------------------------------------------------------------------------
def _build_with_attachments() -> tuple[bytes, bytes]:
    title = "Expedition Dossier with Embedded Attachments"
    pdf, resources = _new_pdf(title)
    lines = [
        title,
        "This PDF container carries two embedded file attachments in /Names/EmbeddedFiles.",
    ]
    stream = pdf.make_stream(_page_stream(lines))
    pdf.pages.append(
        pikepdf.Page(
            Dictionary(
                Type=Name.Page,
                MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                Resources=resources,
                Contents=stream,
            )
        )
    )

    attachments_spec = [
        ("inventory.csv", "text/csv", b"item,quantity\ncompass,2\nledger,5\n"),
        ("notes.txt", "text/plain", b"Field notes attached to the PDF container.\n"),
    ]
    names_array = Array()
    af_array = Array()
    attachments_truth = []

    for fname, mime, payload in attachments_spec:
        ef_stream = pdf.make_stream(payload)
        ef_stream[Name.Type] = Name.EmbeddedFile
        ef_stream[Name.Subtype] = Name("/" + mime.replace("/", "#2F"))
        ef_stream[Name.Params] = Dictionary(
            Size=len(payload),
            CreationDate=String(_CREATION_DATE),
            ModDate=String(_MOD_DATE),
        )
        filespec = pdf.make_indirect(
            Dictionary(
                Type=Name.Filespec,
                F=String(fname),
                UF=String(fname),
                Desc=String(f"Embedded attachment {fname}"),
                EF=Dictionary(F=ef_stream, UF=ef_stream),
            )
        )
        names_array.append(String(fname))
        names_array.append(filespec)
        af_array.append(filespec)
        attachments_truth.append(
            {
                "name": fname,
                "media_type": mime,
                "size": len(payload),
                "sha256": hashlib.sha256(payload).hexdigest(),
            }
        )

    pdf.Root[Name.Names] = Dictionary(
        EmbeddedFiles=Dictionary(Names=names_array)
    )
    pdf.Root[Name.AF] = af_array

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="with-attachments",
        title=title,
        page_count=1,
        pages=[_default_page_info(1)],
        has_attachments=True,
        attachments=attachments_truth,
        text_lines=lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 10. incremental-update.pdf (two revisions)
# ---------------------------------------------------------------------------
def _build_incremental_update() -> tuple[bytes, bytes]:
    title_rev1 = "Archive Ledger (Revision 1)"
    title_rev2 = "Archive Ledger (Revision 2)"
    pdf, resources = _new_pdf(title_rev1)
    lines = [
        title_rev1,
        "Initial revision entry recorded before incremental save.",
    ]
    stream = pdf.make_stream(_page_stream(lines), compress=False)
    pdf.pages.append(
        pikepdf.Page(
            Dictionary(
                Type=Name.Page,
                MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                Resources=resources,
                Contents=stream,
            )
        )
    )
    rev1_bytes = _save_deterministic(
        pdf,
        force_version="1.4",
        object_stream_mode=ObjectStreamMode.disable,
        compress_streams=False,
    )

    match = re.search(rb"startxref\n(\d+)\n%%EOF", rev1_bytes)
    if match is None:
        raise RuntimeError("Revision 1 PDF is missing startxref marker")
    prev_xref = int(match.group(1))

    with pikepdf.open(BytesIO(rev1_bytes)) as p1:
        root_num = p1.Root.objgen[0]
        next_obj_num = len(p1.objects) + 1
        id_hex = bytes(p1.trailer[Name.ID][0]).hex().upper()

    new_obj_offset = len(rev1_bytes)
    new_info_obj = (
        f"{next_obj_num} 0 obj\n"
        f"<< /Title ({title_rev2}) /Author ({_AUTHOR}) "
        f"/Creator ({_CREATOR}) /Producer ({_PRODUCER}) "
        f"/CreationDate ({_CREATION_DATE}) /ModDate ({_MOD_DATE_REV2}) >>\n"
        f"endobj\n"
    ).encode("ascii")
    xref2_offset = new_obj_offset + len(new_info_obj)
    xref2 = (
        f"xref\n"
        f"0 1\n"
        f"0000000000 65535 f \n"
        f"{next_obj_num} 1\n"
        f"{new_obj_offset:010d} 00000 n \n"
        f"trailer\n"
        f"<< /Size {next_obj_num + 1} /Root {root_num} 0 R /Info {next_obj_num} 0 R "
        f"/Prev {prev_xref} /ID [<{id_hex}> <{id_hex}>] >>\n"
        f"startxref\n"
        f"{xref2_offset}\n"
        f"%%EOF\n"
    ).encode("ascii")

    inc_bytes = rev1_bytes + new_info_obj + xref2
    truth_bytes = _truth_envelope(
        doc_id="incremental-update",
        title=title_rev2,
        pdf_version="1.4",
        page_count=1,
        pages=[_default_page_info(1)],
        text_lines=lines,
        extra={
            "revisions": 2,
            "eof_markers": 2,
            "has_prev_xref": True,
            "revision_titles": [title_rev1, title_rev2],
        },
    )
    return inc_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 11. broken-xref.pdf (repairable)
# ---------------------------------------------------------------------------
def _build_broken_xref() -> tuple[bytes, bytes]:
    title = "Repairable Cross-Reference Document"
    pdf, resources = _new_pdf(title)
    lines = [
        title,
        "This PDF has corrupted xref byte offsets that qpdf can reconstruct.",
    ]
    stream = pdf.make_stream(_page_stream(lines), compress=False)
    pdf.pages.append(
        pikepdf.Page(
            Dictionary(
                Type=Name.Page,
                MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                Resources=resources,
                Contents=stream,
            )
        )
    )
    valid_bytes = _save_deterministic(
        pdf,
        force_version="1.4",
        object_stream_mode=ObjectStreamMode.disable,
        compress_streams=False,
    )
    xref_pos = valid_bytes.rfind(b"\nxref\n")
    if xref_pos < 0:
        raise RuntimeError("Expected classic xref table in broken-xref source")
    head = valid_bytes[:xref_pos]
    tail = valid_bytes[xref_pos:]
    tail_corrupted = re.sub(rb"\b\d{10} 00000 n ", b"0000999999 00000 n ", tail)
    tail_corrupted = re.sub(rb"startxref\n\d+\n", b"startxref\n999999\n", tail_corrupted)
    broken_bytes = head + tail_corrupted

    truth_bytes = _truth_envelope(
        doc_id="broken-xref",
        title=title,
        pdf_version="1.4",
        page_count=1,
        pages=[_default_page_info(1)],
        text_lines=lines,
        extra={
            "broken_xref": True,
            "repairable": True,
            "repaired_sha256_source": hashlib.sha256(valid_bytes).hexdigest(),
        },
    )
    return broken_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 12. object-streams.pdf (PDF 1.5 xref streams)
# ---------------------------------------------------------------------------
def _build_object_streams() -> tuple[bytes, bytes]:
    title = "PDF 1.5 Compressed Object and XRef Streams"
    pdf, resources = _new_pdf(title)
    pages_truth = []
    text_lines = [title]
    for page_no in (1, 2):
        line = f"Page {page_no}: indirect dictionaries are stored in /ObjStm with a /XRef stream."
        stream = pdf.make_stream(_page_stream([title, line]))
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                    Resources=resources,
                    Contents=stream,
                )
            )
        )
        pages_truth.append(_default_page_info(page_no))
        text_lines.append(line)

    pdf_bytes = _save_deterministic(
        pdf,
        force_version="1.5",
        object_stream_mode=ObjectStreamMode.generate,
        compress_streams=True,
    )
    if b"/ObjStm" not in pdf_bytes or b"/XRef" not in pdf_bytes:
        raise RuntimeError("object-streams.pdf is missing /ObjStm or /XRef stream markers")

    truth_bytes = _truth_envelope(
        doc_id="object-streams",
        title=title,
        pdf_version="1.5",
        page_count=2,
        pages=pages_truth,
        text_lines=text_lines,
        extra={
            "object_streams": True,
            "xref_stream": True,
        },
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 13. vi-diacritics-subset-font.pdf (Vietnamese text with subset TrueType)
# ---------------------------------------------------------------------------
def _build_vi_diacritics_subset_font() -> tuple[bytes, bytes]:
    title = "Khu vườn đọc sách bên bờ sông Hương"
    paragraphs = [
        "Những buổi sáng mùa thu, ánh nắng dịu nhẹ chiếu qua tán lá phượng vĩ.",
        "Người thủ thư kiểm tra bản đồ cổ, đối chiếu từng địa danh và dấu thanh tiếng Việt.",
        "Đường phố yên tĩnh mở ra góc nhìn thoáng đãng về phía cầu Trường Tiền.",
    ]
    typst_source = "\n".join(
        [
            f'#set document(title: "{title}", author: "{_AUTHOR}", date: none)',
            '#set page(paper: "a4", margin: 22mm)',
            '#set text(font: "DejaVu Sans Mono", size: 11pt, lang: "vi")',
            '#set par(justify: false, leading: 0.75em)',
            f"= {title}",
            *paragraphs,
        ]
    ).encode("utf-8")
    raw_pdf = typst.compile(
        typst_source,
        format="pdf",
        font_paths=[],
        ignore_system_fonts=True,
        timestamp=_TIMESTAMP,
    )
    with pikepdf.open(BytesIO(raw_pdf)) as pdf:
        pdf.docinfo[Name.CreationDate] = String(_CREATION_DATE)
        pdf.docinfo[Name.ModDate] = String(_MOD_DATE)
        pdf.docinfo[Name.Creator] = String(_CREATOR)
        pdf.docinfo[Name.Producer] = String(_PRODUCER)
        font_dict = pdf.pages[0].resources[Name.Font]
        first_font = next(iter(font_dict.values()))
        descendant = first_font[Name.DescendantFonts][0]
        base_font = str(descendant[Name.BaseFont]).lstrip("/")
        subtype = str(descendant[Name.Subtype]).lstrip("/")
        has_font_file2 = Name.FontFile2 in descendant[Name.FontDescriptor]
        if not has_font_file2 or "+" not in base_font:
            raise RuntimeError(f"Expected subset TrueType FontFile2, got {base_font} ({subtype})")
        pdf_bytes = _save_deterministic(pdf, force_version="1.5")

    truth_bytes = _truth_envelope(
        doc_id="vi-diacritics-subset-font",
        title=title,
        language="vi",
        page_count=1,
        pages=[_default_page_info(1)],
        text_lines=[title, *paragraphs],
        extra={
            "subset_font": {
                "base_font": base_font,
                "cid_subtype": subtype,
                "truetype_font_file2": True,
                "subset_tag": base_font.split("+", 1)[0],
                "family": "DejaVu Sans Mono",
            }
        },
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 14. watermark-layer.pdf (OCG watermark)
# ---------------------------------------------------------------------------
def _build_watermark_layer() -> tuple[bytes, bytes]:
    title = " Engineering Specification with OCG Watermark"
    title = title.strip()
    watermark_text = "CONFIDENTIAL DRAFT"
    pdf, base_resources = _new_pdf(title)
    font = base_resources[Name.Font][Name.F1]

    ocg = pdf.make_indirect(
        Dictionary(
            Type=Name.OCG,
            Name=String("AriadShift Watermark"),
            Usage=Dictionary(
                PageElement=Dictionary(Subtype=Name.Watermark),
            ),
        )
    )
    pdf.Root[Name.OCProperties] = Dictionary(
        OCGs=Array([ocg]),
        D=Dictionary(
            Name=String("Default"),
            BaseState=Name.ON,
            ON=Array([ocg]),
        ),
    )
    gs_wm = pdf.make_indirect(
        Dictionary(
            Type=Name.ExtGState,
            ca=0.35,
            CA=0.35,
        )
    )
    resources = pdf.make_indirect(
        Dictionary(
            Font=Dictionary(F1=font),
            Properties=Dictionary(WM1=ocg),
            ExtGState=Dictionary(GSWM=gs_wm),
        )
    )

    pages_truth = []
    text_lines = [title]
    for page_no in (1, 2, 3):
        body_line = f"Page {page_no}: Primary specification body paragraph that must remain intact."
        content = (
            f"BT\n/F1 12 Tf\n1 0 0 1 72.00 760.00 Tm ({_pdf_escape(title)}) Tj\n"
            f"1 0 0 1 72.00 736.00 Tm ({_pdf_escape(body_line)}) Tj\nET\n"
            f"/OC /WM1 BDC\n"
            f"q\n/GSWM gs\n0.75 0.75 0.75 rg\n"
            f"BT\n/F1 36 Tf\n0.7071 0.7071 -0.7071 0.7071 140.00 320.00 Tm ({watermark_text}) Tj\nET\n"
            f"Q\nEMC\n"
        ).encode("ascii")
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                    Resources=resources,
                    Contents=pdf.make_stream(content),
                )
            )
        )
        pages_truth.append(_default_page_info(page_no))
        text_lines.append(body_line)

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="watermark-layer",
        title=title,
        page_count=3,
        pages=pages_truth,
        watermark_kind="ocg",
        watermark={
            "kind": "ocg",
            "layer_name": "AriadShift Watermark",
            "text": watermark_text,
            "pages": [1, 2, 3],
            "opacity": 0.35,
        },
        text_lines=text_lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 15. watermark-artifact.pdf (/Artifact /Watermark marked content)
# ---------------------------------------------------------------------------
def _build_watermark_artifact() -> tuple[bytes, bytes]:
    title = "Operations Procedure with Artifact Watermark"
    watermark_text = "INTERNAL REVIEW ONLY"
    pdf, base_resources = _new_pdf(title)
    font = base_resources[Name.Font][Name.F1]
    gs_wm = pdf.make_indirect(Dictionary(Type=Name.ExtGState, ca=0.35, CA=0.35))
    resources = pdf.make_indirect(
        Dictionary(
            Font=Dictionary(F1=font),
            ExtGState=Dictionary(GSWM=gs_wm),
        )
    )

    pages_truth = []
    text_lines = [title]
    for page_no in (1, 2, 3):
        body_line = f"Page {page_no}: Standard operating procedure content outside the artifact block."
        content = (
            f"BT\n/F1 12 Tf\n1 0 0 1 72.00 760.00 Tm ({_pdf_escape(title)}) Tj\n"
            f"1 0 0 1 72.00 736.00 Tm ({_pdf_escape(body_line)}) Tj\nET\n"
            f"/Artifact << /Type /Pagination /Subtype /Watermark >> BDC\n"
            f"q\n/GSWM gs\n0.70 0.70 0.70 rg\n"
            f"BT\n/F1 32 Tf\n0.7071 0.7071 -0.7071 0.7071 130.00 310.00 Tm ({watermark_text}) Tj\nET\n"
            f"Q\nEMC\n"
        ).encode("ascii")
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                    Resources=resources,
                    Contents=pdf.make_stream(content),
                )
            )
        )
        pages_truth.append(_default_page_info(page_no))
        text_lines.append(body_line)

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="watermark-artifact",
        title=title,
        page_count=3,
        pages=pages_truth,
        watermark_kind="artifact",
        watermark={
            "kind": "artifact",
            "subtype": "Watermark",
            "text": watermark_text,
            "pages": [1, 2, 3],
            "opacity": 0.35,
        },
        text_lines=text_lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 16. watermark-repeated-xobject.pdf
# ---------------------------------------------------------------------------
def _build_watermark_repeated_xobject() -> tuple[bytes, bytes]:
    title = "Field Ledger with Repeated Form XObject Watermark"
    watermark_text = "SAMPLE COPY"
    pdf, base_resources = _new_pdf(title)
    font = base_resources[Name.Font][Name.F1]
    gs_wm = pdf.make_indirect(Dictionary(Type=Name.ExtGState, ca=0.30, CA=0.30))

    wm_xobj = pdf.make_stream(
        f"q\n/GSWM gs\n0.72 0.72 0.72 rg\nBT\n/F1 34 Tf\n1 0 0 1 10.00 20.00 Tm ({watermark_text}) Tj\nET\nQ\n".encode(
            "ascii"
        )
    )
    wm_xobj[Name.Type] = Name.XObject
    wm_xobj[Name.Subtype] = Name.Form
    wm_xobj[Name.BBox] = Array([0, 0, 320, 80])
    wm_xobj[Name.Resources] = Dictionary(
        Font=Dictionary(F1=font),
        ExtGState=Dictionary(GSWM=gs_wm),
    )

    resources = pdf.make_indirect(
        Dictionary(
            Font=Dictionary(F1=font),
            XObject=Dictionary(WMForm1=wm_xobj),
        )
    )

    pages_truth = []
    text_lines = [title]
    for page_no in (1, 2, 3):
        body_line = f"Page {page_no}: Ledger entry preserved when removing /WMForm1."
        content = (
            f"BT\n/F1 12 Tf\n1 0 0 1 72.00 760.00 Tm ({_pdf_escape(title)}) Tj\n"
            f"1 0 0 1 72.00 736.00 Tm ({_pdf_escape(body_line)}) Tj\nET\n"
            f"q\n1 0 0 1 150.00 380.00 cm\n/WMForm1 Do\nQ\n"
        ).encode("ascii")
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                    Resources=resources,
                    Contents=pdf.make_stream(content),
                )
            )
        )
        pages_truth.append(_default_page_info(page_no))
        text_lines.append(body_line)

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="watermark-repeated-xobject",
        title=title,
        page_count=3,
        pages=pages_truth,
        watermark_kind="repeated_xobject",
        watermark={
            "kind": "repeated_xobject",
            "xobject_name": "/WMForm1",
            "text": watermark_text,
            "pages": [1, 2, 3],
            "page_ratio": 1.0,
        },
        text_lines=text_lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 17. scanned-skewed-vi.pdf (raster page with 3° skew)
# ---------------------------------------------------------------------------
def _build_scanned_skewed_vi(root: Path) -> tuple[bytes, bytes]:
    title = "Khu vườn đọc sách"
    paragraphs = scan._source(root, "vietnamese-garden.md")[:1]
    rendered = scan.render_page(
        title=title,
        paragraphs=paragraphs,
        language="vi",
        ppi=200,
    )
    degraded_png = scan._degrade_png(
        rendered,
        seed=40301,
        ppi=200,
        angle=3.0,
        blur_radius=0.18,
        jpeg_quality=90,
    )
    raw_scan_pdf = scan._scan_pdf(degraded_png, ppi=200)
    with pikepdf.open(BytesIO(raw_scan_pdf)) as pdf:
        pdf.docinfo[Name.Title] = String(title)
        pdf.docinfo[Name.Author] = String(_AUTHOR)
        pdf.docinfo[Name.Creator] = String(_CREATOR)
        pdf.docinfo[Name.Producer] = String(_PRODUCER)
        pdf.docinfo[Name.CreationDate] = String(_CREATION_DATE)
        pdf.docinfo[Name.ModDate] = String(_MOD_DATE)
        page_box = pdf.pages[0].MediaBox
        w = round(float(page_box[2]) - float(page_box[0]), 2)
        h = round(float(page_box[3]) - float(page_box[1]), 2)
        pdf_bytes = _save_deterministic(pdf, force_version="1.5")

    page_info = _default_page_info(1, width=w, height=h)
    page_info["has_text_layer"] = False
    page_info["image_only"] = True
    truth_bytes = _truth_envelope(
        doc_id="scanned-skewed-vi",
        title=title,
        language="vi",
        page_count=1,
        pages=[page_info],
        text_lines=[title, *paragraphs],
        extra={
            "scan": True,
            "has_text_layer": False,
            "skew_degrees": 3.0,
            "ppi": 200,
        },
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 18. blank-separators.pdf
# ---------------------------------------------------------------------------
def _build_blank_separators() -> tuple[bytes, bytes]:
    title = "Batch Scan with Blank Separator Pages"
    pdf, resources = _new_pdf(title)
    page_specs = [
        (1, False, "Part 1, Page 1: Opening memorandum."),
        (2, False, "Part 1, Page 2: Supporting figures."),
        (3, True, ""),
        (4, False, "Part 2, Page 1: Annex cover."),
        (5, False, "Part 2, Page 2: Annex schedule."),
        (6, True, "   "),
    ]
    pages_truth = []
    text_lines = [title]
    for page_no, is_blank, text in page_specs:
        if page_no == 3:
            stream = pdf.make_stream(b"")
        elif page_no == 6:
            stream = pdf.make_stream(b"BT /F1 12 Tf 72 750 Td (   ) Tj ET\n")
        else:
            stream = pdf.make_stream(_page_stream([title, text]))
            text_lines.append(text)
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                    Resources=resources,
                    Contents=stream,
                )
            )
        )
        pages_truth.append(_default_page_info(page_no, blank=is_blank))

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="blank-separators",
        title=title,
        page_count=6,
        pages=pages_truth,
        text_lines=text_lines,
        extra={
            "blank_pages": [3, 6],
            "non_blank_segments": [[1, 2], [4, 5]],
        },
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 19. javascript-action.pdf (OpenAction JS — must never execute)
# ---------------------------------------------------------------------------
def _build_javascript_action() -> tuple[bytes, bytes]:
    title = "Document with OpenAction JavaScript"
    pdf, resources = _new_pdf(title)
    lines = [
        title,
        "Safety probe: embedded /OpenAction and /Names /JavaScript must never execute.",
    ]
    stream = pdf.make_stream(_page_stream(lines))
    pdf.pages.append(
        pikepdf.Page(
            Dictionary(
                Type=Name.Page,
                MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                Resources=resources,
                Contents=stream,
            )
        )
    )

    open_js = pdf.make_indirect(
        Dictionary(
            Type=Name.Action,
            S=Name.JavaScript,
            JS=String('app.alert("UNSAFE_OPENACTION_JS");'),
        )
    )
    named_js = pdf.make_indirect(
        Dictionary(
            Type=Name.Action,
            S=Name.JavaScript,
            JS=String("app.beep(0);"),
        )
    )
    pdf.Root[Name.OpenAction] = open_js
    pdf.Root[Name.Names] = Dictionary(
        JavaScript=Dictionary(
            Names=Array([String("init_script"), named_js]),
        )
    )

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="javascript-action",
        title=title,
        page_count=1,
        pages=[_default_page_info(1)],
        has_javascript=True,
        javascript={
            "open_action": 'app.alert("UNSAFE_OPENACTION_JS");',
            "named_scripts": ["init_script"],
            "must_never_execute": True,
        },
        text_lines=lines,
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 20. tagged-actualtext.pdf (structure tree with /ActualText and /Alt)
# ---------------------------------------------------------------------------
def _build_tagged_actualtext() -> tuple[bytes, bytes]:
    title = "Tagged Document with Personal Data in Structure Attributes"
    pdf, resources = _new_pdf(title)
    pii_id = PERSONAL_DATA_SAMPLES["national_id"]
    pii_email = PERSONAL_DATA_SAMPLES["email"]
    pii_phone = PERSONAL_DATA_SAMPLES["phone"]
    pii_name = PERSONAL_DATA_SAMPLES["name"]

    visible_line = (
        f"Applicant: {pii_name} | ID: {pii_id} | Email: {pii_email} | Phone: {pii_phone}"
    )
    content = (
        f"BT\n/F1 12 Tf\n1 0 0 1 72.00 760.00 Tm ({_pdf_escape(title)}) Tj\nET\n"
        f"/P << /MCID 0 >> BDC\n"
        f"BT\n/F1 11 Tf\n1 0 0 1 72.00 730.00 Tm ({_pdf_escape(visible_line)}) Tj\nET\n"
        f"EMC\n"
    ).encode("ascii")

    page_dict = Dictionary(
        Type=Name.Page,
        MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
        Resources=resources,
        Contents=pdf.make_stream(content),
        StructParents=0,
    )
    pdf.pages.append(pikepdf.Page(page_dict))
    page_ref = pdf.pages[0].obj

    struct_root = pdf.make_indirect(Dictionary(Type=Name.StructTreeRoot))
    struct_elem = pdf.make_indirect(
        Dictionary(
            Type=Name.StructElem,
            S=Name.P,
            P=struct_root,
            Pg=page_ref,
            K=0,
            ActualText=String(f"{pii_name} ({pii_id}) — {pii_email}"),
            Alt=String(f"Identity record for {pii_name}, ID {pii_id}, phone {pii_phone}"),
            E=String(pii_id),
        )
    )
    struct_root[Name.K] = Array([struct_elem])
    pdf.Root[Name.StructTreeRoot] = struct_root
    pdf.Root[Name.MarkInfo] = Dictionary(Marked=True)

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="tagged-actualtext",
        title=title,
        page_count=1,
        pages=[_default_page_info(1)],
        text_lines=[title, visible_line],
        extra={
            "tagged_pdf": True,
            "redaction_targets": [pii_name, pii_id, pii_email, pii_phone],
            "structure_pii_locations": ["/ActualText", "/Alt", "/E", "content_stream"],
        },
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 21. form-with-values.pdf (field values containing a personal-data string)
# ---------------------------------------------------------------------------
def _build_form_with_values() -> tuple[bytes, bytes]:
    title = "Filled Form Carrying Personal Data"
    pdf, resources = _new_pdf(title)
    font = resources[Name.Font][Name.F1]
    dr_resources = pdf.make_indirect(Dictionary(Font=Dictionary(F1=font, Helv=font)))

    pii_name = PERSONAL_DATA_SAMPLES["name"]
    pii_id = PERSONAL_DATA_SAMPLES["national_id"]
    pii_email = PERSONAL_DATA_SAMPLES["email"]
    pii_phone = PERSONAL_DATA_SAMPLES["phone"]

    lines = [
        title,
        f"Summary line: {pii_name} ({pii_id})",
    ]
    stream = pdf.make_stream(_page_stream(lines, y_start=760.0))

    field_specs = [
        ("applicant_name", pii_name, [180, 680, 440, 704]),
        ("national_id", pii_id, [180, 640, 440, 664]),
        ("contact_email", pii_email, [180, 600, 440, 624]),
        ("contact_phone", pii_phone, [180, 560, 440, 584]),
    ]
    widgets = []
    form_fields_truth = []
    for fname, fval, rect in field_specs:
        ap_stream = pdf.make_stream(
            f"q 0.96 g 0 0 260 24 re f 0 G 1 w 0.5 0.5 259 23 re s BT /F1 11 Tf 6 7 Td ({_pdf_escape(fval)}) Tj ET Q\n".encode(
                "ascii"
            )
        )
        ap_stream[Name.Type] = Name.XObject
        ap_stream[Name.Subtype] = Name.Form
        ap_stream[Name.BBox] = Array([0, 0, 260, 24])
        ap_stream[Name.Resources] = Dictionary(Font=Dictionary(F1=font))
        widget = pdf.make_indirect(
            Dictionary(
                Type=Name.Annot,
                Subtype=Name.Widget,
                FT=Name.Tx,
                T=String(fname),
                V=String(fval),
                DV=String(fval),
                TU=String(f"Field {fname}: {fval}"),
                DA=String("/Helv 11 Tf 0 g"),
                Rect=Array(rect),
                F=4,
                AP=Dictionary(N=ap_stream),
            )
        )
        widgets.append(widget)
        form_fields_truth.append(
            {
                "name": fname,
                "type": "text",
                "value": fval,
                "default_value": fval,
                "tooltip": f"Field {fname}: {fval}",
                "page": 1,
                "rect": rect,
            }
        )

    page_dict = Dictionary(
        Type=Name.Page,
        MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
        Resources=resources,
        Contents=stream,
        Annots=Array(widgets),
    )
    pdf.pages.append(pikepdf.Page(page_dict))
    page_ref = pdf.pages[0].obj
    for w in widgets:
        w[Name.P] = page_ref

    pdf.Root[Name.AcroForm] = Dictionary(
        Fields=Array(widgets),
        DA=String("/Helv 11 Tf 0 g"),
        DR=dr_resources,
    )

    pdf_bytes = _save_deterministic(pdf)
    truth_bytes = _truth_envelope(
        doc_id="form-with-values",
        title=title,
        page_count=1,
        pages=[_default_page_info(1)],
        has_forms=True,
        field_names=[f["name"] for f in form_fields_truth],
        form_fields=form_fields_truth,
        has_annotations=True,
        text_lines=lines,
        extra={
            "redaction_targets": [pii_name, pii_id, pii_email, pii_phone],
            "form_pii_locations": ["/V", "/DV", "/TU", "/AP/N", "content_stream"],
        },
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# 22. large-1000-pages.pdf
# ---------------------------------------------------------------------------
def generate_large_1000_pages() -> bytes:
    """Build the 1,000-page PDF deterministically (also usable as a test-time helper)."""
    title = "Thousand-Page Throughput Benchmark Document"
    pdf, resources = _new_pdf(title)
    for i in range(1, 1001):
        stream = pdf.make_stream(
            f"BT /F1 12 Tf 72 760 Td ({title} - Page {i} of 1000) Tj ET\n".encode("ascii")
        )
        pdf.pages.append(
            pikepdf.Page(
                Dictionary(
                    Type=Name.Page,
                    MediaBox=Array([0, 0, A4_PORTRAIT[0], A4_PORTRAIT[1]]),
                    Resources=resources,
                    Contents=stream,
                )
            )
        )
    return _save_deterministic(
        pdf,
        force_version="1.5",
        object_stream_mode=ObjectStreamMode.generate,
        compress_streams=True,
    )


def _build_large_1000_pages() -> tuple[bytes, bytes]:
    title = "Thousand-Page Throughput Benchmark Document"
    pdf_bytes = generate_large_1000_pages()
    if len(pdf_bytes) > 5 * 1024 * 1024:
        raise ValueError(f"large-1000-pages.pdf exceeds 5 MB limit: {len(pdf_bytes)} bytes")
    pages_truth = [_default_page_info(i) for i in range(1, 1001)]
    truth_bytes = _truth_envelope(
        doc_id="large-1000-pages",
        title=title,
        page_count=1000,
        pages=pages_truth,
        text_lines=[title, "Thousand-Page Throughput Benchmark Document - Page 1 of 1000"],
        extra={
            "object_streams": True,
            "helper": "ariad_fixture_gen.pdf_tools:generate_large_1000_pages",
        },
    )
    return pdf_bytes, truth_bytes


# ---------------------------------------------------------------------------
# Public generator entry point
# ---------------------------------------------------------------------------
def generate(root: Path) -> dict[str, bytes]:
    """Generate all committed PDF tool corpus files and their *.truth.json companions."""
    outputs: dict[str, bytes] = {}

    builders = [
        ("multipage-20", _build_multipage_20),
        ("mixed-sizes", _build_mixed_sizes),
        ("rotated-pages", _build_rotated_pages),
        ("form-acroform", _build_form_acroform),
        ("annotations", _build_annotations),
        ("with-attachments", _build_with_attachments),
        ("incremental-update", _build_incremental_update),
        ("broken-xref", _build_broken_xref),
        ("object-streams", _build_object_streams),
        ("vi-diacritics-subset-font", _build_vi_diacritics_subset_font),
        ("watermark-layer", _build_watermark_layer),
        ("watermark-artifact", _build_watermark_artifact),
        ("watermark-repeated-xobject", _build_watermark_repeated_xobject),
        ("scanned-skewed-vi", lambda: _build_scanned_skewed_vi(root)),
        ("blank-separators", _build_blank_separators),
        ("javascript-action", _build_javascript_action),
        ("tagged-actualtext", _build_tagged_actualtext),
        ("form-with-values", _build_form_with_values),
        ("large-1000-pages", _build_large_1000_pages),
    ]

    for stem, builder in builders:
        pdf_bytes, truth_bytes = builder()
        outputs[f"fixtures/pdf-tools/{stem}.pdf"] = pdf_bytes
        outputs[f"fixtures/pdf-tools/{stem}.truth.json"] = truth_bytes

    for spec in ENCRYPTED_FIXTURE_SPECS.values():
        plaintext_bytes, truth_bytes = _build_encrypted_plaintext(spec)
        outputs[spec.plaintext_rel_path] = plaintext_bytes
        outputs[spec.truth_rel_path] = truth_bytes

    return outputs


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--emit-encrypted",
        type=Path,
        default=None,
        help="Materialize test-time encrypted PDFs into the given directory instead of updating committed fixtures.",
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[4]

    if args.emit_encrypted is not None:
        written = materialize_encrypted_fixtures(args.emit_encrypted, root=root)
        print(f"Materialized {len(written)} encrypted PDFs into {args.emit_encrypted}")
        return

    outputs = generate(root)
    for relative_path, content in sorted(outputs.items()):
        output_path = root / relative_path
        output_path.parent.mkdir(parents=True, exist_ok=True)
        if not output_path.exists() or output_path.read_bytes() != content:
            output_path.write_bytes(content)

    update_generated_hashes(root, set(outputs))
    print(f"Generated {len(outputs)} files for pdf-tools fixtures and updated their manifest hashes.")


if __name__ == "__main__":
    main()
