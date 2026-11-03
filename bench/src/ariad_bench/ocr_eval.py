"""Compare OCR transcripts with licensed reference text using normalized CER and WER."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import unicodedata

import jiwer


def normalize(text: str) -> str:
    """Normalize Unicode and whitespace equally for both OCR routes and truth."""
    return " ".join(unicodedata.normalize("NFC", text).split())


def score(truth: str, hypothesis: str) -> dict[str, float]:
    reference = normalize(truth)
    actual = normalize(hypothesis)
    return {"cer": float(jiwer.cer(reference, actual)), "wer": float(jiwer.wer(reference, actual))}


def evidence(path: Path) -> dict[str, str]:
    data = path.read_bytes()
    return {"path": str(path), "sha256": hashlib.sha256(data).hexdigest()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--truth", type=Path, required=True)
    parser.add_argument("--tesseract", type=Path, required=True)
    parser.add_argument("--docling", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--language", required=True)
    parser.add_argument("--docling-version", required=True)
    parser.add_argument("--docling-settings", required=True)
    args = parser.parse_args()
    truth = args.truth.read_text(encoding="utf-8")
    tesseract = score(truth, args.tesseract.read_text(encoding="utf-8"))
    docling = score(truth, args.docling.read_text(encoding="utf-8"))
    result = {
        "language": args.language,
        "normalization": "Unicode NFC and collapsed whitespace; case preserved",
        "fixture": evidence(args.fixture),
        "truth": evidence(args.truth),
        "tesseract": {"version": "5.5.4", "transcript": evidence(args.tesseract), **tesseract},
        "docling": {
            "version": args.docling_version,
            "settings": args.docling_settings,
            "transcript": evidence(args.docling),
            **docling,
        },
        "cer_gap_percentage_points": 100 * (tesseract["cer"] - docling["cer"]),
    }
    args.output.write_text(
        json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
