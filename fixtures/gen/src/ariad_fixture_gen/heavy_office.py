"""Authored ODF inputs and frozen LibreOffice exports for Office-to-PDF measurement."""

from pathlib import Path
from xml.sax.saxutils import escape

from ariad_fixture_gen import heavy, office
from ariad_fixture_gen.truth import HeadingBlock, ParagraphBlock, TableBlock


def _text(blocks: list) -> str:
    body = ["<office:text>"]
    for block in blocks:
        if isinstance(block, HeadingBlock):
            body.append(
                f'<text:h text:style-name="Heading_20_{block.level}" text:outline-level="{block.level}">{escape(block.text)}</text:h>'
            )
        elif isinstance(block, ParagraphBlock):
            body.append(office._paragraph(block.text))
        elif isinstance(block, TableBlock):
            body.append(
                '<table:table table:name="Records"><table:table-column table:number-columns-repeated="2"/>'
            )
            for row in block.rows:
                body.append(
                    "<table:table-row>"
                    + "".join(office._cell(cell.text) for cell in row)
                    + "</table:table-row>"
                )
            body.append("</table:table>")
    return "".join([*body, "</office:text>"])


def _sheet(case: dict) -> str:
    rows = "".join(
        "<table:table-row>"
        + "".join(office._cell(value) for value in row)
        + "</table:table-row>"
        for row in case["rows"]
    )
    return (
        '<office:spreadsheet><table:table table:name="Records">'
        + rows
        + "</table:table></office:spreadsheet>"
    )


def _slides(case: dict) -> str:
    return (
        '<office:presentation><draw:page draw:name="page1">'
        + office._frame("title", 1.0, 3.0, [case["title"]])
        + office._frame("outline", 5.0, 12.0, case["paragraphs"])
        + "</draw:page></office:presentation>"
    )


def generate(root: Path) -> dict[str, bytes]:
    outputs = {}
    for case in heavy.cases(root):
        stem = "fixtures/heavy/" + case["id"]
        source_blocks = heavy.blocks(case)
        sources = {
            "odt": _text(source_blocks),
            "ods": _sheet(case),
            "odp": _slides(case),
        }
        for kind, content in sources.items():
            outputs[stem + "." + kind] = office._package(
                kind, case["title"], case["lang"], content
            )
        slide_blocks = [
            HeadingBlock(1, case["title"]),
            *(ParagraphBlock(text) for text in case["paragraphs"]),
        ]
        for extension in ["odt", "doc", "rtf", "ods", "xls", "odp", "ppt"]:
            content = (
                source_blocks
                if extension in ["odt", "doc", "rtf"]
                else [source_blocks[-1]]
                if extension in ["ods", "xls"]
                else slide_blocks
            )
            outputs[stem + "." + extension + ".truth.json"] = heavy._truth(
                case, content
            )
        for source, extensions in [
            ("odt", ["doc", "rtf"]),
            ("ods", ["xls"]),
            ("odp", ["ppt"]),
        ]:
            for extension in extensions:
                path = stem + "." + extension
                existing = root / path
                if existing.is_file():
                    # Legacy export metadata is not deterministic; preserve the reviewed fixture.
                    outputs[path] = existing.read_bytes()
                else:
                    soffice = office._soffice()
                    if soffice is None:
                        raise RuntimeError(
                            "LibreOffice is required to create the legacy heavy fixtures"
                        )
                    outputs[path] = office._convert(
                        soffice,
                        case["id"] + "." + source,
                        outputs[stem + "." + source],
                        extension,
                    )
    return outputs
