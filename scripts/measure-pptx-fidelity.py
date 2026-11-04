#!/usr/bin/env python3
"""Measures text recall fidelity of PDF -> PPTX compared with PDF -> DOCX.

Uses multiset word recall (collections.Counter) to avoid set-membership bias.
"""

from collections import Counter
import os
import re
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile

FIXTURES = [
    "fixtures/pdf-forms-detect/table.pdf",
    "fixtures/pdf-forms-detect/prose.pdf",
    "fixtures/pdf-forms-detect/boxes.pdf",
    "fixtures/heavy/en-map.pdf",
    "fixtures/heavy/vi-garden.pdf",
    "fixtures/pdf/pd-en-gao-08-35-highlights.pdf",
]


def find_ashift():
    candidates = [
        os.path.abspath("target/debug/ashift"),
        os.path.expanduser("~/.cache/ariadshift-target-p10-t2/debug/ashift"),
        os.path.abspath("../target/debug/ashift"),
    ]
    for c in candidates:
        if os.path.isfile(c) and os.access(c, os.X_OK):
            return c
    return "ashift"


def get_pdf_text_and_pages(pdf_path):
    proc = subprocess.run(
        ["pdftotext", pdf_path, "-"],
        capture_output=True,
        text=True,
        check=True,
    )
    text = proc.stdout

    info = subprocess.run(
        ["pdfinfo", pdf_path],
        capture_output=True,
        text=True,
        check=True,
    )
    pages = 1
    for line in info.stdout.splitlines():
        if line.startswith("Pages:"):
            pages = int(line.split()[1])
    return text, pages


def tokenize(text):
    return re.findall(r"\b\w+\b", text.lower())


def multiset_recall(ref_tokens, cand_tokens):
    ref_c = Counter(ref_tokens)
    cand_c = Counter(cand_tokens)
    if not ref_c:
        return 1.0
    matched = sum(min(count, cand_c[word]) for word, count in ref_c.items())
    return matched / len(ref_tokens)


def extract_pptx(pptx_path):
    slide_count = 0
    shape_count = 0
    text_pieces = []
    with zipfile.ZipFile(pptx_path, "r") as z:
        for name in sorted(z.namelist()):
            if re.match(r"ppt/slides/slide\d+\.xml", name):
                slide_count += 1
                xml_data = z.read(name)
                root = ET.fromstring(xml_data)
                for elem in root.iter():
                    tag = elem.tag.split("}")[-1]
                    if tag in ("sp", "pic", "cxnSp", "grpSp"):
                        shape_count += 1
                    elif tag == "t" and elem.text:
                        text_pieces.append(elem.text)
    return " ".join(text_pieces), slide_count, shape_count


def extract_docx(docx_path):
    text_pieces = []
    with zipfile.ZipFile(docx_path, "r") as z:
        if "word/document.xml" in z.namelist():
            xml_data = z.read("word/document.xml")
            root = ET.fromstring(xml_data)
            for elem in root.iter():
                tag = elem.tag.split("}")[-1]
                if tag == "t" and elem.text:
                    text_pieces.append(elem.text)
    return " ".join(text_pieces)


def main():
    ashift = find_ashift()
    pandoc_bin = os.path.abspath(".tools/pandoc/bin/pandoc")
    if os.path.isfile(pandoc_bin):
        os.environ["ASHIFT_PANDOC"] = pandoc_bin

    print(
        "| Fixture | PDF Pages | PDF Words | PPTX Slides | PPTX Shapes | PPTX Recall | DOCX Recall | Delta (PPTX - DOCX) |"
    )
    print(
        "|---|---|---|---|---|---|---|---|"
    )

    with tempfile.TemporaryDirectory() as tmpdir:
        for fix in FIXTURES:
            pdf_path = os.path.abspath(fix)
            if not os.path.exists(pdf_path):
                print(f"| {fix} | missing | - | - | - | - | - | - |")
                continue

            base = os.path.basename(fix)
            pdf_text, pdf_pages = get_pdf_text_and_pages(pdf_path)
            pdf_words = tokenize(pdf_text)

            # Convert to PPTX
            pptx_path = os.path.join(tmpdir, f"{base}.pptx")
            res_pptx = subprocess.run(
                [ashift, "convert", pdf_path, "--to", "pptx", "-o", pptx_path],
                capture_output=True,
            )
            if res_pptx.returncode != 0:
                print(
                    f"| {base} | {pdf_pages} | {len(pdf_words)} | Error: {res_pptx.returncode} | - | - | - | - |"
                )
                continue

            pptx_text, pptx_slides, pptx_shapes = extract_pptx(pptx_path)
            pptx_words = tokenize(pptx_text)
            pptx_rec = multiset_recall(pdf_words, pptx_words)

            # Convert to DOCX
            docx_path = os.path.join(tmpdir, f"{base}.docx")
            res_docx = subprocess.run(
                [ashift, "convert", pdf_path, "--to", "docx", "-o", docx_path],
                capture_output=True,
            )
            docx_rec = 0.0
            if res_docx.returncode == 0:
                docx_text = extract_docx(docx_path)
                docx_words = tokenize(docx_text)
                docx_rec = multiset_recall(pdf_words, docx_words)

            delta = (pptx_rec - docx_rec) * 100
            delta_str = f"{delta:+.1f}%" if delta != 0.0 else "0.0%"

            print(
                f"| `{base}` | {pdf_pages} | {len(pdf_words)} | {pptx_slides} | {pptx_shapes} | {pptx_rec*100:.1f}% | {docx_rec*100:.1f}% | {delta_str} |"
            )


if __name__ == "__main__":
    main()
