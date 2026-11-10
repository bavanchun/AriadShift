"""Fifty bilingual workflow examples with explicit semantic and parameter expectations."""

from __future__ import annotations

import json

import pytest

from ariad_ai.recipe import recipe_from_text


def tool(name, **params):
    return {"tool": "pdf." + name, "params": params}


def convert(name):
    return {"convert": {"to": name}}


# Each language covers every supported operation, composition, conditions and naming.
ENGLISH = [
    ("Merge the PDF files", [tool("merge")], None),
    ("Join these documents", [tool("merge")], None),
    ("Split into individual pages", [tool("split", mode="every", every=1)], None),
    ("Split every 5 pages", [tool("split", mode="every", every=5)], None),
    ("Rotate 90 degrees", [tool("rotate", angle=90, pages="all")], None),
    ("Rotate pages 2-4 by 180 degrees", [tool("rotate", angle=180, pages="2-4")], None),
    ("Delete pages 1,3-5", [tool("delete_pages", pages="1,3-5")], None),
    ("Extract pages 2-6", [tool("extract_pages", pages="2-6")], None),
    ("Compress using lossless preset", [tool("compress", preset="lossless")], None),
    ("OCR in English", [tool("ocr", languages=["eng"])], None),
    ("Deskew the scanned pages", [tool("deskew")], None),
    ('Add watermark "DRAFT"', [tool("watermark", kind="text", text="DRAFT")], None),
    ("Remove watermark candidate mark-1", [tool("watermark_remove", candidates=["mark-1"])], None),
    ("Remove raster watermark", [tool("watermark_remove_raster", mask="auto")], None),
    (
        'Redact "secret@example.org"',
        [tool("redact", targets=[{"kind": "exact", "text": "secret@example.org"}])],
        None,
    ),
    ("Convert to PDF/A-2u", [tool("to_pdfa", level="2u")], None),
    ("Grayscale the document", [tool("grayscale")], None),
    ("Add page numbers starting at 3", [tool("page_numbers", start=3)], None),
    ("Convert to Word", [convert("docx")], None),
    ("Convert to Markdown", [convert("markdown")], None),
    ("Convert to HTML then convert to PDF", [convert("html"), convert("pdf")], None),
    ("Convert to EPUB", [convert("epub")], None),
    (
        "If page_count > 10 compress then rotate 270 degrees",
        [{**tool("compress"), "if": "page_count > 10"}, tool("rotate", angle=270, pages="all")],
        None,
    ),
    (
        "Merge, OCR, then compress; name {stem}-{date}",
        [tool("merge"), tool("ocr"), tool("compress")],
        "{stem}-{date}",
    ),
    (
        "Split every 2 pages then convert to Word, naming {title}",
        [tool("split", mode="every", every=2), convert("docx")],
        "{title}",
    ),
]
VIETNAMESE = [
    ("Gộp các tệp PDF", [tool("merge")], None),
    ("Nối các tài liệu", [tool("merge")], None),
    ("Tách thành từng trang", [tool("split", mode="every", every=1)], None),
    ("Chia mỗi 4 trang", [tool("split", mode="every", every=4)], None),
    ("Xoay 90 độ", [tool("rotate", angle=90, pages="all")], None),
    ("Xoay trang 3-7 một góc 270 độ", [tool("rotate", angle=270, pages="3-7")], None),
    ("Xóa trang 2,5-8", [tool("delete_pages", pages="2,5-8")], None),
    ("Trích xuất trang 1-3", [tool("extract_pages", pages="1-3")], None),
    ("Nén mức 7", [tool("compress", level=7)], None),
    ("Nhận diện chữ tiếng Việt", [tool("ocr", languages=["vie"])], None),
    ("Căn thẳng tài liệu quét", [tool("deskew")], None),
    ('Đóng dấu "BẢN NHÁP"', [tool("watermark", kind="text", text="BẢN NHÁP")], None),
    ("Xóa thủy vân mã wm-2", [tool("watermark_remove", candidates=["wm-2"])], None),
    ("Xóa thủy vân ảnh", [tool("watermark_remove_raster", mask="auto")], None),
    (
        'Che thông tin "Nguyễn Văn An"',
        [tool("redact", targets=[{"kind": "exact", "text": "Nguyễn Văn An"}])],
        None,
    ),
    ("Chuyển sang PDF/A-3b", [tool("to_pdfa", level="3b")], None),
    ("Đen trắng tài liệu", [tool("grayscale")], None),
    ("Đánh số trang bắt đầu từ 5", [tool("page_numbers", start=5)], None),
    ("Chuyển sang Word", [convert("docx")], None),
    ("Chuyển sang Markdown", [convert("markdown")], None),
    ("Chuyển sang HTML rồi chuyển sang PDF", [convert("html"), convert("pdf")], None),
    ("Chuyển sang EPUB", [convert("epub")], None),
    (
        "Nếu số trang > 20 nén rồi xoay 180 độ",
        [{**tool("compress"), "if": "page_count > 20"}, tool("rotate", angle=180, pages="all")],
        None,
    ),
    (
        "Gộp rồi OCR rồi nén, đặt tên {stem}-{date}",
        [tool("merge"), tool("ocr"), tool("compress")],
        "{stem}-{date}",
    ),
    (
        "Tách mỗi 3 trang rồi chuyển sang Word, đặt tên {title}",
        [tool("split", mode="every", every=3), convert("docx")],
        "{title}",
    ),
]
CASES = ENGLISH + VIETNAMESE


@pytest.mark.parametrize(
    ("prompt", "steps", "naming"), CASES, ids=[f"prompt-{i:02}" for i in range(1, 51)]
)
def test_nl_recipe_semantics(prompt, steps, naming):
    actual = recipe_from_text(prompt)
    expected = {"version": "ariad-recipe/1", "steps": steps}
    if naming:
        expected["naming"] = naming
    assert actual == expected
    assert json.loads(json.dumps(actual)) == expected
    assert set(actual) <= {"version", "steps", "naming"}
    for step in actual["steps"]:
        assert set(step) <= {"tool", "params", "convert", "if"}
        assert "watch" not in step


def test_eval_threshold():
    assert len(ENGLISH) == len(VIETNAMESE) == 25
    accurate = 0
    for prompt, steps, naming in CASES:
        expected = {"version": "ariad-recipe/1", "steps": steps}
        if naming:
            expected["naming"] = naming
        accurate += recipe_from_text(prompt) == expected
    assert accurate / 50 >= 0.9, f"semantic accuracy: {accurate}/50"


@pytest.mark.parametrize(
    "prompt, owner",
    [
        ("Merge then if page_count >= 5 compress", 1),
        ("Merge, nếu số trang > 10 nén", 1),
        ("Compress if page_count > 10 then rotate 90 degrees", 0),
        ("Nén nếu số trang > 10 rồi xoay 90 độ", 0),
    ],
)
def test_condition_belongs_to_only_the_requested_step(prompt, owner):
    recipe = recipe_from_text(prompt)
    assert len(recipe["steps"]) == 2
    assert "if" in recipe["steps"][owner]
    assert "if" not in recipe["steps"][1 - owner]


def test_angle_does_not_use_the_page_number():
    assert recipe_from_text("Rotate pages 90 by 270 degrees")["steps"] == [
        tool("rotate", angle=270, pages="90")
    ]
    assert recipe_from_text("Rotate -90 degrees")["steps"] == [
        tool("rotate", angle=-90, pages="all")
    ]


def test_watermark_candidate_identifiers_remain_case_sensitive():
    assert recipe_from_text("Remove watermark candidate WM-AbC")["steps"] == [
        tool("watermark_remove", candidates=["WM-AbC"])
    ]
