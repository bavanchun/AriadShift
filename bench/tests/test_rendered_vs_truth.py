"""Structural parity tests comparing rendered fixture files to truth companions.

Validates that rendered documents (HTML, DOCX, EPUB) match their authored truth
companions across all structural elements (titles, headings, paragraphs, lists,
tables, figures, and footnotes) using independent parsers (lxml for HTML/EPUB,
python-docx for DOCX). Also tests mutation detection to ensure structural drift
causes test failures.
"""

from __future__ import annotations

import copy
import io
import json
from pathlib import Path
import tomllib
import unittest
import zipfile

import docx
import lxml.html
from lxml import etree

from ariad_fixture_gen import docx as gen_docx, epub as gen_epub, html as gen_html
from ariad_fixture_gen.truth import (
    Cell,
    FootnoteBlock,
    HeadingBlock,
    ListBlock,
    ParagraphBlock,
    TableBlock,
)


def _extract_flat_headings(heading_nodes: list[dict]) -> list[tuple[int, str]]:
    flat: list[tuple[int, str]] = []

    def walk(nodes: list[dict]) -> None:
        for n in nodes:
            flat.append((n["level"], " ".join(n["text"].split())))
            walk(n.get("children", []))

    walk(heading_nodes)
    return flat


def verify_html_matches_truth(html_bytes: bytes, truth: dict) -> None:
    tree = lxml.html.fromstring(html_bytes)

    # 1. Title
    titles = tree.xpath("//title/text()")
    if titles and "meta" in truth.get("ir", {}):
        expected_title = truth["ir"]["meta"].get("title")
        if expected_title:
            assert titles[0].strip() == expected_title, f"HTML title mismatch: {titles[0].strip()} != {expected_title}"

    # 2. Headings
    h_rendered = [
        (int(el.tag[1]), " ".join(el.text_content().split()))
        for el in tree.xpath("//*[self::h1 or self::h2 or self::h3 or self::h4 or self::h5 or self::h6]")
    ]
    h_truth = _extract_flat_headings(truth["canonical"].get("headings", []))
    assert h_rendered == h_truth, f"HTML heading mismatch: {h_rendered} != {h_truth}"

    # 3. Paragraphs and block text
    p_rendered = [
        " ".join(p.text_content().split())
        for p in tree.xpath(
            "//main/p | //article/p | //main/blockquote/p | //article/blockquote/p | //main/a | //article/a"
        )
    ]
    p_truth = [" ".join(b["text"].split()) for b in truth.get("blocks", []) if b.get("type") == "paragraph" and b.get("text")]
    assert p_rendered == p_truth, f"HTML paragraph mismatch: {p_rendered} != {p_truth}"

    # 4. Tables
    t_rendered = tree.xpath("//table")
    t_truth = truth["canonical"].get("tables", [])
    assert len(t_rendered) == len(t_truth), f"Table count mismatch: {len(t_rendered)} != {len(t_truth)}"

    for tbl_el, tbl_canon in zip(t_rendered, t_truth, strict=False):
        cap = tbl_el.xpath(".//caption/text()")
        r_cap = " ".join(cap[0].split()) if cap else ""
        exp_cap = tbl_canon.get("caption", "")
        assert r_cap == exp_cap, f"HTML table caption mismatch: {r_cap!r} != {exp_cap!r}"

        rendered_rows = []
        for tr in tbl_el.xpath(".//tr"):
            cells = [" ".join(td.text_content().split()) for td in tr.xpath("./th | ./td")]
            if cells:
                rendered_rows.append(cells)

        canon_rows = [[c["text"] for c in row] for row in tbl_canon.get("rows", [])]
        assert rendered_rows == canon_rows, f"Table content mismatch: {rendered_rows} != {canon_rows}"

        tr_els = tbl_el.xpath(".//tr")
        canon_grid = tbl_canon.get("rows", [])
        assert len(tr_els) == len(canon_grid), f"Table row count mismatch: {len(tr_els)} != {len(canon_grid)}"
        for tr, c_row in zip(tr_els, canon_grid, strict=False):
            cell_els = tr.xpath("./th | ./td")
            assert len(cell_els) == len(c_row), f"Table cell count mismatch: {len(cell_els)} != {len(c_row)}"
            for cell_el, c in zip(cell_els, c_row, strict=False):
                is_th = (cell_el.tag.lower() == "th")
                exp_header = c.get("is_header", False)
                assert is_th == exp_header, f"HTML cell header mismatch: {is_th} != {exp_header}"
                colspan = int(cell_el.get("colspan", "1"))
                exp_colspan = c.get("colspan", 1)
                assert colspan == exp_colspan, f"HTML colspan mismatch: {colspan} != {exp_colspan}"
                rowspan = int(cell_el.get("rowspan", "1"))
                exp_rowspan = c.get("rowspan", 1)
                assert rowspan == exp_rowspan, f"HTML rowspan mismatch: {rowspan} != {exp_rowspan}"


def verify_docx_matches_truth(docx_bytes: bytes, truth: dict) -> None:
    doc = docx.Document(io.BytesIO(docx_bytes))

    # 1. Title
    expected_title = truth.get("ir", {}).get("meta", {}).get("title")
    if expected_title and doc.core_properties.title:
        assert doc.core_properties.title == expected_title, (
            f"DOCX title mismatch: {doc.core_properties.title} != {expected_title}"
        )

    # 2. Headings
    h_rendered = []
    for p in doc.paragraphs:
        if "Heading" in p.style.name:
            level_str = p.style.name.replace("Heading", "").strip()
            if level_str.isdigit():
                h_rendered.append((int(level_str), " ".join(p.text.split())))

    h_truth = _extract_flat_headings(truth["canonical"].get("headings", []))
    assert h_rendered == h_truth, f"DOCX heading mismatch: {h_rendered} != {h_truth}"

    # 3. Paragraphs
    p_rendered = [
        " ".join(p.text.split())
        for p in doc.paragraphs
        if "Heading" not in p.style.name and not p.style.name.startswith("List") and p.text.strip()
    ]
    p_truth = [" ".join(b["text"].split()) for b in truth.get("blocks", []) if b.get("type") == "paragraph" and b.get("text")]
    assert p_rendered == p_truth, f"DOCX paragraph mismatch: {p_rendered} != {p_truth}"

    # 4. Lists
    l_rendered = [" ".join(p.text.split()) for p in doc.paragraphs if p.style.name.startswith("List")]
    l_truth = [it for b in truth.get("blocks", []) if b.get("type") == "list" for it in b.get("items", [])]
    assert l_rendered == l_truth, f"DOCX list mismatch: {l_rendered} != {l_truth}"

    # 4b. List kind (ordered vs bullet styles)
    list_blocks = [blk for blk in truth.get("blocks", []) if blk.get("type") == "list"]
    if list_blocks:
        list_paras = [p for p in doc.paragraphs if p.style.name.startswith("List")]
        p_idx = 0
        for l_blk in list_blocks:
            for item in l_blk.get("items", []):
                assert p_idx < len(list_paras), f"Missing list paragraph for item '{item}'"
                p = list_paras[p_idx]
                if l_blk.get("ordered"):
                    assert "Number" in p.style.name, f"Expected ordered list style (Number), got '{p.style.name}' for item '{item}'"
                else:
                    assert "Bullet" in p.style.name, f"Expected bullet list style (Bullet), got '{p.style.name}' for item '{item}'"
                p_idx += 1

    # 4c. Footnotes
    fn_truth = [" ".join(blk["text"].split()) for blk in truth.get("blocks", []) if blk.get("type") == "footnote"]
    if fn_truth:
        with zipfile.ZipFile(io.BytesIO(docx_bytes)) as z:
            assert "word/footnotes.xml" in z.namelist(), "Missing word/footnotes.xml in DOCX"
            fn_tree = etree.fromstring(z.read("word/footnotes.xml"))
            ns = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}
            fn_rendered = [
                " ".join("".join(p.xpath(".//w:t/text()", namespaces=ns)).split())
                for p in fn_tree.xpath(".//w:footnote[@w:id > 0]//w:p", namespaces=ns)
            ]
            assert fn_rendered == fn_truth, f"DOCX footnote mismatch: {fn_rendered} != {fn_truth}"

    # 5. Tables
    t_rendered = doc.tables
    t_truth = truth.get("canonical", {}).get("tables", [])
    assert len(t_rendered) == len(t_truth), f"Table count mismatch: {len(t_rendered)} != {len(t_truth)}"

    for tbl_el, tbl_canon in zip(t_rendered, t_truth, strict=False):
        canon_rows = [[c["text"] for c in row] for row in tbl_canon.get("rows", [])]
        assert len(tbl_el.rows) == len(canon_rows), (
            f"Row count mismatch: {len(tbl_el.rows)} != {len(canon_rows)}"
        )

        for r_el, c_row in zip(tbl_el.rows, canon_rows, strict=False):
            cell_texts = [" ".join(c.text.split()) for c in r_el.cells]
            for expected_cell in c_row:
                assert expected_cell in cell_texts, f"Missing cell text '{expected_cell}' in DOCX table"

        # Check is_header and gridSpan from underlying oxml
        for tr, c_row in zip(tbl_el._element.xpath("./w:tr"), tbl_canon.get("rows", [])):
            tc_els = tr.xpath("./w:tc")
            assert len(tc_els) == len(c_row), f"DOCX tc count mismatch: {len(tc_els)} != {len(c_row)}"
            for tc, c in zip(tc_els, c_row, strict=False):
                is_bold = bool(tc.xpath('.//w:rPr/w:b[not(@w:val="0" or @w:val="false")]'))
                grid_span = tc.xpath("./w:tcPr/w:gridSpan/@w:val")
                span = int(grid_span[0]) if grid_span else 1
                exp_header = c.get("is_header", False)
                exp_span = c.get("colspan", 1)
                assert is_bold == exp_header, f"DOCX header bold mismatch: {is_bold} != {exp_header} for cell '{c['text']}'"
                assert span == exp_span, f"DOCX gridSpan mismatch: {span} != {exp_span} for cell '{c['text']}'"


def verify_epub_matches_truth(epub_bytes: bytes, truth: dict) -> None:
    with zipfile.ZipFile(io.BytesIO(epub_bytes)) as z:
        # 1. Title from package.opf
        opf = etree.fromstring(z.read("EPUB/package.opf"))
        ns = {"dc": "http://purl.org/dc/elements/1.1/", "opf": "http://www.idpf.org/2007/opf"}
        titles = opf.xpath("//dc:title/text()", namespaces=ns)
        expected_title = truth.get("ir", {}).get("meta", {}).get("title")
        if expected_title and titles:
            assert titles[0] == expected_title, f"EPUB title mismatch: {titles[0]} != {expected_title}"

        # 2. Read content documents in spine order
        item_map = {item.get("id"): item.get("href") for item in opf.xpath("//opf:item", namespaces=ns)}
        doc_hrefs = [
            item_map[ref.get("idref")]
            for ref in opf.xpath("//opf:itemref", namespaces=ns)
            if ref.get("idref") in item_map
        ]

        h_rendered = []
        p_rendered = []
        fn_rendered = []
        fig_rendered = []
        t_rendered_rows = []
        t_rendered_tables = []

        for href in doc_hrefs:
            tree = lxml.html.fromstring(z.read(f"EPUB/{href}"))
            for el in tree.xpath("//*[self::h1 or self::h2 or self::h3 or self::h4 or self::h5 or self::h6]"):
                h_rendered.append((int(el.tag[1]), " ".join(el.text_content().split())))
            for p in tree.xpath("//body/p"):
                p_text = " ".join(p.xpath("text()")).strip()
                p_rendered.append(p_text)
            for aside in tree.xpath('//*[local-name()="aside"]/p'):
                fn_rendered.append(" ".join(aside.text_content().split()))
            for fig in tree.xpath("//figure"):
                img_alt = fig.xpath(".//img/@alt")
                cap = fig.xpath(".//figcaption/text()")
                fig_rendered.append((img_alt[0] if img_alt else "", cap[0] if cap else ""))
            for tbl in tree.xpath("//table"):
                t_rendered_tables.append(tbl)
                for tr in tbl.xpath(".//tr"):
                    cells = [" ".join(td.text_content().split()) for td in tr.xpath("./th | ./td")]
                    if cells:
                        t_rendered_rows.append(cells)

        # 3. Compare headings
        h_truth = _extract_flat_headings(truth["canonical"].get("headings", []))
        assert h_rendered == h_truth, f"EPUB heading mismatch: {h_rendered} != {h_truth}"

        # 4. Compare paragraphs
        p_truth = [" ".join(b["text"].split()) for b in truth.get("blocks", []) if b.get("type") == "paragraph" and b.get("text")]
        assert p_rendered == p_truth, f"EPUB paragraph mismatch: {p_rendered} != {p_truth}"

        # 5. Compare footnotes
        fn_truth = [" ".join(b["text"].split()) for b in truth.get("blocks", []) if b.get("type") == "footnote"]
        assert fn_rendered == fn_truth, f"EPUB footnote mismatch: {fn_rendered} != {fn_truth}"

        # 6. Compare figures
        fig_truth = [
            (b.get("alt", ""), b.get("caption", ""))
            for b in truth.get("blocks", [])
            if b.get("type") == "figure"
        ]
        assert fig_rendered == fig_truth, f"EPUB figure mismatch: {fig_rendered} != {fig_truth}"

        # 7. Compare tables if present
        t_truth = truth["canonical"].get("tables", [])
        if t_truth:
            canon_rows = [
                [" ".join(c["text"].split()) for c in row]
                for tbl in t_truth
                for row in tbl.get("rows", [])
            ]
            assert t_rendered_rows == canon_rows, f"EPUB table content mismatch: {t_rendered_rows} != {canon_rows}"

            assert len(t_rendered_tables) == len(t_truth), f"EPUB table count mismatch: {len(t_rendered_tables)} != {len(t_truth)}"
            for tbl_el, tbl_canon in zip(t_rendered_tables, t_truth, strict=False):
                cap = tbl_el.xpath(".//caption/text()")
                r_cap = " ".join(cap[0].split()) if cap else ""
                exp_cap = tbl_canon.get("caption", "")
                assert r_cap == exp_cap, f"EPUB table caption mismatch: {r_cap!r} != {exp_cap!r}"

                tr_els = tbl_el.xpath(".//tr")
                canon_grid = tbl_canon.get("rows", [])
                assert len(tr_els) == len(canon_grid), f"EPUB row count mismatch: {len(tr_els)} != {len(canon_grid)}"
                for tr, c_row in zip(tr_els, canon_grid, strict=False):
                    cell_els = tr.xpath("./th | ./td")
                    assert len(cell_els) == len(c_row), f"EPUB cell count mismatch: {len(cell_els)} != {len(c_row)}"
                    for cell_el, c in zip(cell_els, c_row, strict=False):
                        is_th = (cell_el.tag.lower() == "th")
                        exp_header = c.get("is_header", False)
                        assert is_th == exp_header, f"EPUB cell header mismatch: {is_th} != {exp_header}"
                        colspan = int(cell_el.get("colspan", "1"))
                        exp_colspan = c.get("colspan", 1)
                        assert colspan == exp_colspan, f"EPUB colspan mismatch: {colspan} != {exp_colspan}"
                        rowspan = int(cell_el.get("rowspan", "1"))
                        exp_rowspan = c.get("rowspan", 1)
                        assert rowspan == exp_rowspan, f"EPUB rowspan mismatch: {rowspan} != {exp_rowspan}"


class TestRenderedVsTruth(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = Path(__file__).resolve().parents[2]
        manifest_path = cls.root / "fixtures" / "manifest.toml"
        cls.manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))

    def _get_truth_companion(self, fixture_record: dict) -> dict | None:
        for comp in fixture_record.get("companions", []):
            if comp.get("path", "").endswith(".truth.json"):
                truth_path = self.root / comp["path"]
                if truth_path.is_file():
                    return json.loads(truth_path.read_text(encoding="utf-8"))
        return None

    def test_html_fixtures_match_truth(self) -> None:
        """Verify rendered HTML files match their truth companions in headings, paragraphs, and tables."""
        tested_count = 0
        for f in self.manifest.get("fixture", []):
            if f.get("format") != "html":
                continue

            truth = self._get_truth_companion(f)
            if truth is None or truth.get("unscored"):
                continue

            html_path = self.root / f["path"]
            self.assertTrue(html_path.is_file(), f"Missing HTML file: {html_path}")
            verify_html_matches_truth(html_path.read_bytes(), truth)
            tested_count += 1

        self.assertGreaterEqual(tested_count, 3, "Expected at least 3 scored HTML fixtures")

    def test_docx_fixtures_match_truth(self) -> None:
        """Verify rendered DOCX files match their truth companions in headings, paragraphs, lists, and tables."""
        tested_count = 0
        for f in self.manifest.get("fixture", []):
            if f.get("format") != "docx":
                continue

            truth = self._get_truth_companion(f)
            if truth is None or truth.get("unscored"):
                continue

            docx_path = self.root / f["path"]
            self.assertTrue(docx_path.is_file(), f"Missing DOCX file: {docx_path}")
            verify_docx_matches_truth(docx_path.read_bytes(), truth)
            tested_count += 1

        self.assertGreaterEqual(tested_count, 3, "Expected at least 3 scored DOCX fixtures")

    def test_epub_fixtures_match_truth(self) -> None:
        """Verify rendered EPUB files match their truth companions in headings, paragraphs, figures, and footnotes."""
        tested_count = 0
        for f in self.manifest.get("fixture", []):
            if f.get("format") != "epub":
                continue

            truth = self._get_truth_companion(f)
            if truth is None or truth.get("unscored"):
                continue

            epub_path = self.root / f["path"]
            self.assertTrue(epub_path.is_file(), f"Missing EPUB file: {epub_path}")
            verify_epub_matches_truth(epub_path.read_bytes(), truth)
            tested_count += 1

        self.assertGreaterEqual(tested_count, 3, "Expected at least 3 scored EPUB fixtures")

    def test_structural_drift_mutations_detected(self) -> None:
        """Verify that nine specific mutations in rendered documents or generators cause parity test failures."""
        # Mutation 1: DOCX styled_report heading level 2 to 3
        spec1 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "vi-styled-report"][0]
        truth1 = json.loads((self.root / "fixtures" / "docx" / "vi-styled-report.truth.json").read_text(encoding="utf-8"))
        mut_blocks1 = copy.deepcopy(spec1.blocks)
        for b in mut_blocks1:
            if isinstance(b, HeadingBlock) and b.level == 2:
                b.level = 3
        spec1_mut = copy.deepcopy(spec1)
        spec1_mut.blocks = mut_blocks1
        data1 = gen_docx._render_docx(spec1_mut)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(data1, truth1)

        # Mutation 2: DOCX route_table drop a body row
        spec2 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "vi-route-data"][0]
        truth2 = json.loads((self.root / "fixtures" / "docx" / "vi-route-data.truth.json").read_text(encoding="utf-8"))
        mut_blocks2 = copy.deepcopy(spec2.blocks)
        for b in mut_blocks2:
            if isinstance(b, TableBlock):
                b.rows = [b.rows[0], b.rows[2], b.rows[3]]
        spec2_mut = copy.deepcopy(spec2)
        spec2_mut.blocks = mut_blocks2
        data2 = gen_docx._render_docx(spec2_mut)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(data2, truth2)

        # Mutation 3: HTML vi-route-table drop a <tr>
        raw_html3 = (self.root / "fixtures" / "html" / "vi-route-table.html").read_text(encoding="utf-8")
        truth3 = json.loads((self.root / "fixtures" / "html" / "vi-route-table.truth.json").read_text(encoding="utf-8"))
        mut_html3 = raw_html3.replace("<tr><td>Vườn đọc</td><td>14</td></tr>", "")
        with self.assertRaises(AssertionError):
            verify_html_matches_truth(mut_html3.encode("utf-8"), truth3)

        # Mutation 4: HTML _render_html <h{level}> to <h{level+1}>
        spec4 = [s for s in gen_html._get_specs(self.root) if s[1] == "vi-route-table"][0]
        truth4 = json.loads((self.root / "fixtures" / "html" / "vi-route-table.truth.json").read_text(encoding="utf-8"))
        mut_blocks4 = copy.deepcopy(spec4[4])
        for b in mut_blocks4:
            if isinstance(b, HeadingBlock):
                b.level += 1
        data4 = gen_html._render_html(spec4[1], spec4[2], spec4[3], mut_blocks4)
        with self.assertRaises(AssertionError):
            verify_html_matches_truth(data4, truth4)

        # Mutation 5: DOCX numbered_checklist drop item 'Check each lamp'
        spec5 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "en-numbered-checklist"][0]
        truth5 = json.loads((self.root / "fixtures" / "docx" / "en-numbered-checklist.truth.json").read_text(encoding="utf-8"))
        mut_blocks5 = copy.deepcopy(spec5.blocks)
        for b in mut_blocks5:
            if isinstance(b, ListBlock) and "Check each lamp" in b.items:
                b.items.remove("Check each lamp")
        spec5_mut = copy.deepcopy(spec5)
        spec5_mut.blocks = mut_blocks5
        data5 = gen_docx._render_docx(spec5_mut)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(data5, truth5)

        # Mutation 6: DOCX route_table closing paragraph text changed
        spec6 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "vi-route-data"][0]
        truth6 = json.loads((self.root / "fixtures" / "docx" / "vi-route-data.truth.json").read_text(encoding="utf-8"))
        mut_blocks6 = copy.deepcopy(spec6.blocks)
        for b in mut_blocks6:
            if isinstance(b, ParagraphBlock) and "ước lượng đi bộ" in b.text:
                b.text = "Thời gian đã biến đổi."
        spec6_mut = copy.deepcopy(spec6)
        spec6_mut.blocks = mut_blocks6
        data6 = gen_docx._render_docx(spec6_mut)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(data6, truth6)

        # Mutation 7: EPUB chapters heading <h2> to <h3>
        spec7 = [s for s in gen_epub._get_specs(self.root) if s.doc_id == "vi-epub-chapters"][0]
        truth7 = json.loads((self.root / "fixtures" / "epub" / "vi-epub-chapters.truth.json").read_text(encoding="utf-8"))
        mut_blocks7 = copy.deepcopy(spec7.blocks)
        for b in mut_blocks7:
            if isinstance(b, HeadingBlock) and b.text == "Sinh hoạt hằng tuần":
                b.level = 3
        spec7_mut = copy.deepcopy(spec7)
        spec7_mut.blocks = mut_blocks7
        data7 = gen_epub._render_epub(spec7_mut)
        with self.assertRaises(AssertionError):
            verify_epub_matches_truth(data7, truth7)

        # Mutation 8: EPUB minimal title text changed
        spec8 = [s for s in gen_epub._get_specs(self.root) if s.doc_id == "en-epub-minimal"][0]
        truth8 = json.loads((self.root / "fixtures" / "epub" / "en-epub-minimal.truth.json").read_text(encoding="utf-8"))
        spec8_mut = copy.deepcopy(spec8)
        spec8_mut.title = "Mutated Minimal Document"
        data8 = gen_epub._render_epub(spec8_mut)
        with self.assertRaises(AssertionError):
            verify_epub_matches_truth(data8, truth8)

        # Mutation 9: HTML _render_html paragraph text truncated to 20 chars
        spec9 = [s for s in gen_html._get_specs(self.root) if s[1] == "vi-garden-notice"][0]
        truth9 = json.loads((self.root / "fixtures" / "html" / "vi-garden-notice.truth.json").read_text(encoding="utf-8"))
        mut_blocks9 = copy.deepcopy(spec9[4])
        for b in mut_blocks9:
            if isinstance(b, ParagraphBlock):
                b.text = b.text[:20]
        data9 = gen_html._render_html(spec9[1], spec9[2], spec9[3], mut_blocks9)
        with self.assertRaises(AssertionError):
            verify_html_matches_truth(data9, truth9)

        # Mutation 10: HTML row 0 is_header=False (truth expects headers; renderer emits <td>)
        spec10 = [s for s in gen_html._get_specs(self.root) if s[1] == "vi-route-table"][0]
        truth10 = json.loads((self.root / "fixtures" / "html" / "vi-route-table.truth.json").read_text(encoding="utf-8"))
        mut_blocks10 = copy.deepcopy(spec10[4])
        for b in mut_blocks10:
            if isinstance(b, TableBlock):
                for cell in b.rows[0]:
                    cell.is_header = False
        data10 = gen_html._render_html(spec10[1], spec10[2], spec10[3], mut_blocks10)
        with self.assertRaises(AssertionError):
            verify_html_matches_truth(data10, truth10)

        # Mutation 11: EPUB drop colspan attribute
        spec11 = [s for s in gen_epub._get_specs(self.root) if s.doc_id == "vi-epub-table-image"][0]
        truth11 = json.loads((self.root / "fixtures" / "epub" / "vi-epub-table-image.truth.json").read_text(encoding="utf-8"))
        mut_blocks11 = copy.deepcopy(spec11.blocks)
        for b in mut_blocks11:
            if isinstance(b, TableBlock):
                for cell in b.rows[3]:
                    cell.colspan = 1
        spec11_mut = copy.deepcopy(spec11)
        spec11_mut.blocks = mut_blocks11
        data11 = gen_epub._render_epub(spec11_mut)
        with self.assertRaises(AssertionError):
            verify_epub_matches_truth(data11, truth11)

        # Mutation 12: DOCX colspan cells not merged (gridSpan removed / missing)
        spec12 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "vi-route-data"][0]
        truth12 = json.loads((self.root / "fixtures" / "docx" / "vi-route-data.truth.json").read_text(encoding="utf-8"))
        orig_docx12 = gen_docx._render_docx(spec12)
        with zipfile.ZipFile(io.BytesIO(orig_docx12)) as z_in:
            parts12 = {name: z_in.read(name) for name in z_in.namelist()}
        doc_xml12 = etree.fromstring(parts12["word/document.xml"])
        for gs in doc_xml12.xpath(".//w:gridSpan", namespaces={"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}):
            gs.getparent().remove(gs)
        parts12["word/document.xml"] = etree.tostring(doc_xml12, encoding="UTF-8", xml_declaration=True, standalone=True)
        buf12 = io.BytesIO()
        with zipfile.ZipFile(buf12, "w") as z_out:
            for name, content in parts12.items():
                z_out.writestr(name, content)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(buf12.getvalue(), truth12)

        # Mutation 13: DOCX header row not bold
        spec13 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "vi-route-data"][0]
        truth13 = json.loads((self.root / "fixtures" / "docx" / "vi-route-data.truth.json").read_text(encoding="utf-8"))
        orig_docx13 = gen_docx._render_docx(spec13)
        with zipfile.ZipFile(io.BytesIO(orig_docx13)) as z_in:
            parts13 = {name: z_in.read(name) for name in z_in.namelist()}
        doc_xml13 = etree.fromstring(parts13["word/document.xml"])
        ns13 = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}
        first_tr = doc_xml13.xpath(".//w:tbl/w:tr", namespaces=ns13)[0]
        for b_tag in first_tr.xpath(".//w:rPr/w:b", namespaces=ns13):
            b_tag.attrib[f"{{{ns13['w']}}}val"] = "0"
        parts13["word/document.xml"] = etree.tostring(doc_xml13, encoding="UTF-8", xml_declaration=True, standalone=True)
        buf13 = io.BytesIO()
        with zipfile.ZipFile(buf13, "w") as z_out:
            for name, content in parts13.items():
                z_out.writestr(name, content)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(buf13.getvalue(), truth13)

        # Mutation 14: DOCX footnote text truncated to [:10]
        spec14 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "en-footnotes-docx"][0]
        truth14 = json.loads((self.root / "fixtures" / "docx" / "en-footnotes-docx.truth.json").read_text(encoding="utf-8"))
        mut_blocks14 = copy.deepcopy(spec14.blocks)
        for b in mut_blocks14:
            if isinstance(b, FootnoteBlock):
                b.text = b.text[:10]
        spec14_mut = copy.deepcopy(spec14)
        spec14_mut.blocks = mut_blocks14
        data14 = gen_docx._render_docx(spec14_mut)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(data14, truth14)

        # Mutation 15: DOCX ordered list rendered as bullets
        spec15 = [s for s in gen_docx._get_specs(self.root) if s.doc_id == "en-numbered-checklist"][0]
        truth15 = json.loads((self.root / "fixtures" / "docx" / "en-numbered-checklist.truth.json").read_text(encoding="utf-8"))
        mut_blocks15 = copy.deepcopy(spec15.blocks)
        for b in mut_blocks15:
            if isinstance(b, ListBlock) and b.ordered:
                b.ordered = False
                b.style = "List Bullet"
        spec15_mut = copy.deepcopy(spec15)
        spec15_mut.blocks = mut_blocks15
        data15 = gen_docx._render_docx(spec15_mut)
        with self.assertRaises(AssertionError):
            verify_docx_matches_truth(data15, truth15)

        # Mutation 16: HTML caption text truncated to [:3]
        raw_html16 = (self.root / "fixtures" / "html" / "vi-route-table.html").read_text(encoding="utf-8")
        truth16 = json.loads((self.root / "fixtures" / "html" / "vi-route-table.truth.json").read_text(encoding="utf-8"))
        mut_html16 = raw_html16.replace("<caption>Thời gian đi bộ</caption>", "<caption>Thờ</caption>")
        with self.assertRaises(AssertionError):
            verify_html_matches_truth(mut_html16.encode("utf-8"), truth16)


if __name__ == "__main__":
    unittest.main()
