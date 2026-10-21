"""Unit tests for canonical document normalizer.

Tests:
- Vietnamese Unicode NFC/NFD equivalence and whitespace collapse.
- Tables with spans (rowspan, colspan) and header identification.
- Heading tree hierarchy construction.
- List structure extraction.
- Deterministic equivalence across IR, Pandoc, and truth sources.
"""

from __future__ import annotations

import unicodedata
import unittest

from ariad_bench.canonical import (
    CanonicalDoc,
    HeadingNode,
    TableCell,
    TableGrid,
    clean_text,
    from_ir,
    from_pandoc,
    nfc,
    normalize,
)


class TestCanonical(unittest.TestCase):
    def test_vietnamese_nfc_nfd_normalization(self) -> None:
        """Verify that decomposed NFD text is normalized to precomposed NFC."""
        # 'Tiếng Việt' decomposed into base letters and combining accents (NFD)
        composed = "Tiếng Việt có dấu hỏi ngã nặng sắc huyền"
        decomposed = unicodedata.normalize("NFD", composed)

        self.assertNotEqual(composed, decomposed, "NFD representation must differ in byte sequence")
        self.assertEqual(nfc(decomposed), composed, "nfc() must convert NFD to NFC")
        self.assertEqual(clean_text(decomposed), clean_text(composed))

        # Whitespace collapse test
        messy_vietnamese = "   Tiếng   Việt   \n\t  thân   yêu \r\n  "
        expected = "Tiếng Việt thân yêu"
        self.assertEqual(clean_text(messy_vietnamese), expected)

    def test_table_with_spans_ir(self) -> None:
        """Verify that IR tables with rowspan and colspan are properly extracted."""
        ir_doc = {
            "version": "ariad-ir/0",
            "meta": {"title": "Test Table"},
            "body": [
                {
                    "type": "table",
                    "caption": [{"type": "text", "text": "   Bảng dữ liệu   "}],
                    "columns": [{"align": "left"}, {"align": "center"}, {"align": "right"}],
                    "head": [
                        [
                            {
                                "rowspan": 1,
                                "colspan": 2,
                                "header": True,
                                "blocks": [
                                    {"type": "paragraph", "content": [{"type": "text", "text": "Header 1 & 2"}]}
                                ],
                            },
                            {
                                "rowspan": 2,
                                "colspan": 1,
                                "header": True,
                                "blocks": [
                                    {"type": "paragraph", "content": [{"type": "text", "text": "Header 3"}]}
                                ],
                            },
                        ]
                    ],
                    "body": [
                        [
                            {
                                "rowspan": 1,
                                "colspan": 1,
                                "header": False,
                                "blocks": [
                                    {"type": "paragraph", "content": [{"type": "text", "text": "Cell A"}]}
                                ],
                            },
                            {
                                "rowspan": 1,
                                "colspan": 1,
                                "header": False,
                                "blocks": [
                                    {"type": "paragraph", "content": [{"type": "text", "text": "Cell B"}]}
                                ],
                            },
                        ]
                    ],
                }
            ],
        }

        canonical = from_ir(ir_doc)
        self.assertEqual(len(canonical.tables), 1)
        table = canonical.tables[0]
        self.assertEqual(table.caption, "Bảng dữ liệu")
        self.assertEqual(len(table.rows), 2)

        # Row 0 (head)
        row0 = table.rows[0]
        self.assertEqual(len(row0), 2)
        self.assertEqual(row0[0].text, "Header 1 & 2")
        self.assertEqual(row0[0].colspan, 2)
        self.assertEqual(row0[0].rowspan, 1)
        self.assertTrue(row0[0].is_header)

        self.assertEqual(row0[1].text, "Header 3")
        self.assertEqual(row0[1].colspan, 1)
        self.assertEqual(row0[1].rowspan, 2)
        self.assertTrue(row0[1].is_header)

        # Row 1 (body)
        row1 = table.rows[1]
        self.assertEqual(len(row1), 2)
        self.assertEqual(row1[0].text, "Cell A")
        self.assertEqual(row1[0].colspan, 1)
        self.assertEqual(row1[0].rowspan, 1)
        self.assertFalse(row1[0].is_header)

    def test_table_with_spans_pandoc(self) -> None:
        """Verify that Pandoc AST tables with rowspan and colspan are properly extracted."""
        pandoc_doc = {
            "pandoc-api-version": [1, 23, 1, 2],
            "meta": {},
            "blocks": [
                {
                    "t": "Table",
                    "c": [
                        ["", [], []],  # attr
                        [None, [[{"t": "Para", "c": [{"t": "Str", "c": "Route"}, {"t": "Space"}, {"t": "Str", "c": "Table"}]}]]],  # caption
                        [{"t": "AlignDefault"}, {"t": "ColWidthDefault"}],  # colspecs
                        [
                            ["", [], []],  # table head attr
                            [
                                [
                                    ["", [], []],  # row attr
                                    [
                                        # cell: [attr, align, rowspan, colspan, blocks]
                                        [["", [], []], {"t": "AlignDefault"}, 1, 2, [{"t": "Plain", "c": [{"t": "Str", "c": "Tuyến"}, {"t": "Space"}, {"t": "Str", "c": "đường"}]}]]
                                    ]
                                ]
                            ]
                        ],
                        [
                            [
                                ["", [], []], 0, [],
                                [
                                    [
                                        ["", [], []],
                                        [
                                            [["", [], []], {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": [{"t": "Str", "c": "Bến"}, {"t": "Space"}, {"t": "Str", "c": "sông"}]}]],
                                            [["", [], []], {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": [{"t": "Str", "c": "8"}, {"t": "Space"}, {"t": "Str", "c": "phút"}]}]]
                                        ]
                                    ]
                                ]
                            ]
                        ],
                        ["", [], []]  # foot
                    ]
                }
            ]
        }

        canonical = from_pandoc(pandoc_doc)
        self.assertEqual(len(canonical.tables), 1)
        table = canonical.tables[0]
        self.assertEqual(table.caption, "Route Table")
        self.assertEqual(len(table.rows), 2)
        self.assertEqual(table.rows[0][0].text, "Tuyến đường")
        self.assertEqual(table.rows[0][0].colspan, 2)
        self.assertTrue(table.rows[0][0].is_header)
        self.assertEqual(table.rows[1][0].text, "Bến sông")
        self.assertEqual(table.rows[1][1].text, "8 phút")

    def test_table_rowspan_pandoc(self) -> None:
        """Verify Pandoc AST table cell rowspan is preserved."""
        pandoc_doc = {
            "pandoc-api-version": [1, 23, 1, 2],
            "meta": {},
            "blocks": [
                {
                    "t": "Table",
                    "c": [
                        ["", [], []],
                        [None, []],
                        [{"t": "AlignDefault"}, {"t": "ColWidthDefault"}],
                        ["", []],
                        [
                            [
                                ["", [], []], 0, [],
                                [
                                    [
                                        ["", [], []],
                                        [
                                            [["", [], []], {"t": "AlignDefault"}, 3, 2, [{"t": "Plain", "c": [{"t": "Str", "c": "Spanned"}]}]]
                                        ]
                                    ]
                                ]
                            ]
                        ],
                        ["", []]
                    ]
                }
            ]
        }
        canonical = from_pandoc(pandoc_doc)
        cell = canonical.tables[0].rows[0][0]
        self.assertEqual(cell.rowspan, 3)
        self.assertEqual(cell.colspan, 2)

    def test_heading_tree_hierarchy(self) -> None:
        """Verify that heading sequences form correct parent-child hierarchies."""
        ir_doc = {
            "version": "ariad-ir/0",
            "meta": {},
            "body": [
                {"type": "heading", "level": 1, "content": [{"type": "text", "text": "H1 Title"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "intro"}]},
                {"type": "heading", "level": 2, "content": [{"type": "text", "text": "H2 Sub 1"}]},
                {"type": "heading", "level": 3, "content": [{"type": "text", "text": "H3 Deep"}]},
                {"type": "heading", "level": 2, "content": [{"type": "text", "text": "H2 Sub 2"}]},
                {"type": "heading", "level": 1, "content": [{"type": "text", "text": "H1 Second"}]},
            ],
        }

        canonical = from_ir(ir_doc)
        self.assertEqual(len(canonical.headings), 2)
        h1_first = canonical.headings[0]
        self.assertEqual(h1_first.text, "H1 Title")
        self.assertEqual(len(h1_first.children), 2)

        h2_sub1 = h1_first.children[0]
        self.assertEqual(h2_sub1.text, "H2 Sub 1")
        self.assertEqual(len(h2_sub1.children), 1)
        self.assertEqual(h2_sub1.children[0].text, "H3 Deep")

        h2_sub2 = h1_first.children[1]
        self.assertEqual(h2_sub2.text, "H2 Sub 2")
        self.assertEqual(len(h2_sub2.children), 0)

        h1_second = canonical.headings[1]
        self.assertEqual(h1_second.text, "H1 Second")
        self.assertEqual(len(h1_second.children), 0)

    def test_truth_dict_roundtrip(self) -> None:
        """Verify CanonicalDoc serialization roundtrip."""
        doc = CanonicalDoc(
            text="Văn bản mẫu tiếng Việt",
            headings=[HeadingNode(level=1, text="Tiêu đề chính")],
            tables=[TableGrid(caption="Bảng", rows=[[TableCell(text="Ô 1", colspan=2)]])],
            image_count=2,
            link_count=3,
        )

        d = doc.to_dict()
        restored = normalize(d)
        self.assertEqual(restored.text, doc.text)
        self.assertEqual(len(restored.headings), 1)
        self.assertEqual(restored.headings[0].text, "Tiêu đề chính")
        self.assertEqual(restored.tables[0].rows[0][0].colspan, 2)
        self.assertEqual(restored.image_count, 2)
        self.assertEqual(restored.link_count, 3)

    def test_canonical_to_ir_structure(self) -> None:
        """Verify canonical_to_ir generates valid IR representation."""
        from ariad_bench.canonical import canonical_to_ir

        doc = CanonicalDoc(
            text="Tiêu đề chính\nNội dung đoạn văn\nMục danh sách",
            headings=[HeadingNode(level=1, text="Tiêu đề chính")],
            tables=[TableGrid(caption="Bảng", rows=[[TableCell(text="Ô 1", colspan=2, is_header=True)]])],
            lists=[from_ir({"body": [{"type": "list", "ordered": False, "items": [{"blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Mục danh sách"}]}]}]}]}).lists[0]],
        )

        ir = canonical_to_ir(doc, title="Test IR")
        self.assertEqual(ir.get("version"), "ariad-ir/0")
        self.assertEqual(ir.get("meta", {}).get("title"), "Test IR")
        self.assertIn("body", ir)
        self.assertTrue(len(ir["body"]) >= 3)

        # Check block types
        types = [b.get("type") for b in ir["body"]]
        self.assertIn("heading", types)
        self.assertIn("table", types)
        self.assertIn("list", types)

        # Ensure normalize can read it back
        recovered = normalize(ir)
        self.assertEqual(len(recovered.headings), 1)
        self.assertEqual(recovered.headings[0].text, "Tiêu đề chính")
        self.assertEqual(len(recovered.tables), 1)
        self.assertEqual(len(recovered.lists), 1)

    def test_cross_path_ir_and_pandoc_canonical_equality(self) -> None:
        """Verify cross-path canonical equality across IR JSON and Pandoc AST JSON."""
        # Equivalent document structure across IR and Pandoc AST:
        # Heading 1, paragraph with raw HTML and note, task list, table with header
        ir_doc = {
            "version": "ariad-ir/0",
            "meta": {"title": "Doc"},
            "body": [
                {"type": "heading", "level": 1, "content": [{"type": "text", "text": "Tiêu đề"}]},
                {"type": "paragraph", "content": [
                    {"type": "text", "text": "Đoạn văn có "},
                    {"type": "raw", "format": "html", "text": "<span>chú thích</span>"},
                    {"type": "text", "text": "."},
                ]},
                {"type": "list", "ordered": False, "tight": True, "items": [
                    {"checked": True, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Nhiệm vụ 1"}]}]},
                    {"checked": False, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Nhiệm vụ 2"}]}]},
                ]},
                {"type": "table", "caption": [], "columns": [{"align": "default"}], "head": [
                    [{"rowspan": 1, "colspan": 1, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Cột"}]}]}],
                ], "body": [
                    [{"rowspan": 1, "colspan": 1, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Ô"}]}]}],
                ]},
                {"type": "footnote", "id": "fn1", "blocks": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "Nội dung chú thích cuối trang"}]},
                ]},
            ],
        }

        pandoc_doc = {
            "pandoc-api-version": [1, 23, 1, 2],
            "meta": {},
            "blocks": [
                {"t": "Header", "c": [1, ["tieu-de", [], []], [{"t": "Str", "c": "Tiêu"}, {"t": "Space"}, {"t": "Str", "c": "đề"}]]},
                {"t": "Para", "c": [
                    {"t": "Str", "c": "Đoạn"}, {"t": "Space"}, {"t": "Str", "c": "văn"}, {"t": "Space"}, {"t": "Str", "c": "có"}, {"t": "Space"},
                    {"t": "RawInline", "c": ["html", "<span>"]}, {"t": "Str", "c": "chú"}, {"t": "Space"}, {"t": "Str", "c": "thích"}, {"t": "RawInline", "c": ["html", "</span>"]},
                    {"t": "Str", "c": "."},
                    {"t": "Note", "c": [
                        {"t": "Para", "c": [{"t": "Str", "c": "Nội"}, {"t": "Space"}, {"t": "Str", "c": "dung"}, {"t": "Space"}, {"t": "Str", "c": "chú"}, {"t": "Space"}, {"t": "Str", "c": "thích"}, {"t": "Space"}, {"t": "Str", "c": "cuối"}, {"t": "Space"}, {"t": "Str", "c": "trang"}]},
                    ]},
                ]},
                {"t": "BulletList", "c": [
                    [{"t": "Plain", "c": [{"t": "Str", "c": "☒"}, {"t": "Space"}, {"t": "Str", "c": "Nhiệm"}, {"t": "Space"}, {"t": "Str", "c": "vụ"}, {"t": "Space"}, {"t": "Str", "c": "1"}]}],
                    [{"t": "Plain", "c": [{"t": "Str", "c": "☐"}, {"t": "Space"}, {"t": "Str", "c": "Nhiệm"}, {"t": "Space"}, {"t": "Str", "c": "vụ"}, {"t": "Space"}, {"t": "Str", "c": "2"}]}],
                ]},
                {"t": "Table", "c": [
                    ["", [], []],
                    [None, []],
                    [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
                    ["", [
                        ["", [
                            ["", {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": [{"t": "Str", "c": "Cột"}]}]],
                        ]],
                    ]],
                    [
                        ["", 0, [], [
                            ["", [
                                ["", {"t": "AlignDefault"}, 1, 1, [{"t": "Plain", "c": [{"t": "Str", "c": "Ô"}]}]],
                            ]],
                        ]],
                    ],
                    ["", []],
                ]},
            ],
        }

        c_ir = from_ir(ir_doc)
        c_pandoc = from_pandoc(pandoc_doc)

        self.assertEqual(c_ir.text, c_pandoc.text)
        self.assertEqual(c_ir.headings, c_pandoc.headings)
        self.assertEqual(c_ir.lists, c_pandoc.lists)
        self.assertEqual(c_ir.tables, c_pandoc.tables)
        self.assertEqual(c_ir, c_pandoc)

    def test_canonical_doc_from_dict_newline_preservation(self) -> None:
        """Verify CanonicalDoc.from_dict joins lines with newline, not space."""
        data = {"text": "Dòng 1\nDòng 2\nDòng 3"}
        doc = CanonicalDoc.from_dict(data)
        self.assertEqual(doc.text, "Dòng 1\nDòng 2\nDòng 3")
        self.assertIn("\n", doc.text)
        self.assertNotIn("Dòng 1 Dòng 2", doc.text)

    def test_from_ir_excludes_furniture(self) -> None:
        """Verify from_ir ignores header and footer furniture blocks."""
        doc = {
            "version": "ariad-ir/0",
            "body": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Nội dung chính"}]},
            ],
            "furniture": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Tiêu đề đầu trang (header)"}]},
            ],
        }
        canonical = from_ir(doc)
        self.assertEqual(canonical.text, "Nội dung chính")
        self.assertNotIn("header", canonical.text)

    def test_from_pandoc_footnote_code_block(self) -> None:
        """Verify from_pandoc extracts CodeBlock in footnotes."""
        doc = {
            "pandoc-api-version": [1, 23, 1, 2],
            "meta": {},
            "blocks": [
                {
                    "t": "Para",
                    "c": [
                        {"t": "Str", "c": "Văn"},
                        {"t": "Space"},
                        {"t": "Str", "c": "bản"},
                        {
                            "t": "Note",
                            "c": [
                                {"t": "CodeBlock", "c": [["", [], []], "const x = 42;"]},
                            ],
                        },
                    ],
                },
            ],
        }
        canonical = from_pandoc(doc)
        self.assertIn("const x = 42;", canonical.text)

    def test_nested_list_handling_ir_and_pandoc(self) -> None:
        """Verify nested list structures are extracted in document order across both formats."""
        ir_doc = {
            "version": "ariad-ir/0",
            "body": [
                {
                    "type": "list",
                    "ordered": False,
                    "items": [
                        {
                            "blocks": [
                                {"type": "paragraph", "content": [{"type": "text", "text": "Mục cha"}]},
                                {
                                    "type": "list",
                                    "ordered": True,
                                    "items": [
                                        {"blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Mục con"}]}]},
                                    ],
                                },
                            ],
                        },
                    ],
                },
            ],
        }
        c_ir = from_ir(ir_doc)
        self.assertEqual(len(c_ir.lists), 2)
        self.assertFalse(c_ir.lists[0].ordered)
        self.assertEqual(c_ir.lists[0].items, ["Mục cha"])
        self.assertTrue(c_ir.lists[1].ordered)
        self.assertEqual(c_ir.lists[1].items, ["Mục con"])
        self.assertIn("Mục cha\nMục con", c_ir.text)

        pandoc_doc = {
            "pandoc-api-version": [1, 23, 1, 2],
            "meta": {},
            "blocks": [
                {
                    "t": "BulletList",
                    "c": [
                        [
                            {"t": "Plain", "c": [{"t": "Str", "c": "Mục"}, {"t": "Space"}, {"t": "Str", "c": "cha"}]},
                            {
                                "t": "OrderedList",
                                "c": [
                                    [1, {"t": "Decimal"}, {"t": "Period"}],
                                    [[{"t": "Plain", "c": [{"t": "Str", "c": "Mục"}, {"t": "Space"}, {"t": "Str", "c": "con"}]}]],
                                ],
                            },
                        ],
                    ],
                },
            ],
        }
        c_pandoc = from_pandoc(pandoc_doc)
        self.assertEqual(len(c_pandoc.lists), 2)
        self.assertFalse(c_pandoc.lists[0].ordered)
        self.assertEqual(c_pandoc.lists[0].items, ["Mục cha"])
        self.assertTrue(c_pandoc.lists[1].ordered)
        self.assertEqual(c_pandoc.lists[1].items, ["Mục con"])
        self.assertIn("Mục cha\nMục con", c_pandoc.text)

    def test_real_tool_cross_path_normalization(self) -> None:
        """Verify real Pandoc JSON and real ashift __ir outputs normalize to consistent canonical docs.

        Tests headings, paragraphs, nested lists, and tables with colspan and rowspan across both real tools.
        """
        import json
        from pathlib import Path
        import subprocess
        import tempfile
        from ariad_bench.run import find_binary

        root = Path(__file__).resolve().parents[2]
        try:
            pandoc_bin = find_binary("pandoc", None, [root / ".tools" / "pandoc" / "bin" / "pandoc"])
            ashift_bin = find_binary(
                "ashift",
                None,
                [root / "target" / "release" / "ashift", root / "target" / "debug" / "ashift"],
            )
        except FileNotFoundError as exc:
            self.skipTest(str(exc))

        html_sample = (
            "<!DOCTYPE html>\n"
            "<html>\n"
            "<head><title>Báo cáo kỹ thuật</title></head>\n"
            "<body>\n"
            "  <h1>Báo cáo kỹ thuật</h1>\n"
            "  <h2>1. Giới thiệu tổng quan</h2>\n"
            "  <p>Dự án AriadShift cung cấp công cụ chuyển đổi tài liệu chất lượng cao.</p>\n"
            "  <ul>\n"
            "    <li>Mục cấp 1\n"
            "      <ul>\n"
            "        <li>Mục lồng cấp 2</li>\n"
            "      </ul>\n"
            "    </li>\n"
            "  </ul>\n"
            "  <table>\n"
            "    <caption>Bảng tiêu chuẩn</caption>\n"
            "    <thead>\n"
            "      <tr>\n"
            "        <th colspan=\"2\">Chỉ số tổng hợp</th>\n"
            "      </tr>\n"
            "    </thead>\n"
            "    <tbody>\n"
            "      <tr>\n"
            "        <td rowspan=\"2\">Nhóm A</td>\n"
            "        <td>Giá trị 1</td>\n"
            "      </tr>\n"
            "      <tr>\n"
            "        <td>Giá trị 2</td>\n"
            "      </tr>\n"
            "    </tbody>\n"
            "  </table>\n"
            "</body>\n"
            "</html>\n"
        )

        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_path = Path(tmp_dir)
            html_file = tmp_path / "sample.html"
            html_file.write_text(html_sample, encoding="utf-8")

            # 1. Run real Pandoc to produce AST JSON
            pandoc_res = subprocess.run(
                [str(pandoc_bin), "-f", "html", "-t", "json", str(html_file)],
                capture_output=True,
                encoding="utf-8",
                check=True,
            )
            pandoc_data = json.loads(pandoc_res.stdout)
            canonical_pandoc = normalize(pandoc_data)

            # 2. Run real ashift __ir to produce Ariad IR JSON
            ir_file = tmp_path / "sample.ir.json"
            subprocess.run(
                [str(ashift_bin), "__ir", str(html_file), "-o", str(ir_file), "--overwrite"],
                capture_output=True,
                encoding="utf-8",
                check=True,
            )
            canonical_ir = normalize(ir_file)

            # 3. Verify cross-path structural consistency
            # Headings
            self.assertEqual(len(canonical_pandoc.headings), 1)
            self.assertEqual(len(canonical_ir.headings), 1)
            self.assertEqual(canonical_pandoc.headings[0].text, canonical_ir.headings[0].text)

            self.assertEqual(len(canonical_pandoc.headings[0].children), 1)
            self.assertEqual(len(canonical_ir.headings[0].children), 1)
            self.assertEqual(canonical_pandoc.headings[0].children[0].text, canonical_ir.headings[0].children[0].text)

            # Nested lists
            self.assertEqual(len(canonical_pandoc.lists), 2)
            self.assertEqual(len(canonical_ir.lists), 2)
            self.assertEqual(canonical_pandoc.lists[0].items, ["Mục cấp 1"])
            self.assertEqual(canonical_ir.lists[0].items, ["Mục cấp 1"])
            self.assertEqual(canonical_pandoc.lists[1].items, ["Mục lồng cấp 2"])
            self.assertEqual(canonical_ir.lists[1].items, ["Mục lồng cấp 2"])

            # Tables with spans
            self.assertEqual(len(canonical_pandoc.tables), 1)
            self.assertEqual(len(canonical_ir.tables), 1)
            self.assertEqual(canonical_pandoc.tables[0].caption, "Bảng tiêu chuẩn")
            self.assertEqual(canonical_ir.tables[0].caption, "Bảng tiêu chuẩn")
            self.assertEqual(len(canonical_pandoc.tables[0].rows), 3)
            self.assertEqual(len(canonical_ir.tables[0].rows), 3)

            # Cell spans: header colspan=2
            self.assertEqual(canonical_pandoc.tables[0].rows[0][0].colspan, 2)
            self.assertEqual(canonical_ir.tables[0].rows[0][0].colspan, 2)
            self.assertTrue(canonical_pandoc.tables[0].rows[0][0].is_header)
            self.assertTrue(canonical_ir.tables[0].rows[0][0].is_header)

            # Cell spans: body cell rowspan=2
            self.assertEqual(canonical_pandoc.tables[0].rows[1][0].rowspan, 2)
            self.assertEqual(canonical_ir.tables[0].rows[1][0].rowspan, 2)

            # Text content
            self.assertIn("Báo cáo kỹ thuật", canonical_pandoc.text)
            self.assertIn("Báo cáo kỹ thuật", canonical_ir.text)
            self.assertIn("AriadShift", canonical_pandoc.text)
            self.assertIn("AriadShift", canonical_ir.text)


if __name__ == "__main__":
    unittest.main()
