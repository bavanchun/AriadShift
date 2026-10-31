"""Choosing the OCR engine and languages for a job.

Vietnamese and mixed Vietnamese-English text goes to Tesseract: the PP-OCR dictionaries behind
RapidOCR miss many Vietnamese letters. Other languages go to RapidOCR when its models are
installed and to Tesseract when the language data is.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from ariad_docling.protocol import EngineError

OCR_MODES = frozenset({"auto", "tesseract", "rapidocr", "off"})
DEFAULT_HINTS = ("vi", "en")

# ISO 639-1 code -> (Tesseract traineddata name, RapidOCR BCP-47 tag or None).
_LANGUAGES: dict[str, tuple[str, str | None]] = {
    "vi": ("vie", None),
    "en": ("eng", "en"),
    "fr": ("fra", "fr"),
    "de": ("deu", "de"),
    "es": ("spa", "es"),
    "it": ("ita", "it"),
    "pt": ("por", "pt"),
    "nl": ("nld", "nl"),
    "pl": ("pol", "pl"),
    "cs": ("ces", "cs"),
    "sv": ("swe", "sv"),
    "da": ("dan", "da"),
    "fi": ("fin", "fi"),
    "no": ("nor", "no"),
    "ro": ("ron", "ro"),
    "hu": ("hun", "hu"),
    "tr": ("tur", "tr"),
    "id": ("ind", "id"),
    "ms": ("msa", "ms"),
    "zh": ("chi_sim", "zh-Hans"),
    "ja": ("jpn", "ja"),
    "ko": ("kor", None),
    "ru": ("rus", None),
    "th": ("tha", None),
}
_FROM_TESSERACT = {tesseract: iso for iso, (tesseract, _) in _LANGUAGES.items()}


@dataclass(frozen=True)
class OcrPlan:
    """The OCR engine and languages for one job; ``engine`` is ``None`` when OCR is off."""

    engine: str | None
    languages: tuple[str, ...] = ()
    warnings: tuple[tuple[str, str], ...] = field(default_factory=tuple)


def normalize_hints(value: object) -> tuple[str, ...]:
    """Turns the ``lang`` option into ISO 639-1 codes, keeping unknown codes as given."""
    if value is None:
        return DEFAULT_HINTS
    items = [value] if isinstance(value, str) else value
    if not isinstance(items, list) or not all(isinstance(item, str) for item in items):
        raise EngineError("invalid_request", "options.lang must be a string or a list of strings")
    hints: list[str] = []
    for item in items:
        code = item.strip().lower().removeprefix("iso:").split("-")[0]
        code = _FROM_TESSERACT.get(code, code)
        if code and code not in hints:
            hints.append(code)
    return tuple(hints) or DEFAULT_HINTS


def plan_ocr(
    mode: object,
    hints: tuple[str, ...],
    *,
    needed: bool,
    tesseract: bool,
    tessdata_languages: frozenset[str],
    rapidocr: bool,
) -> OcrPlan:
    """Picks the OCR engine.

    ``needed`` says whether the input cannot be read without OCR (images, pages without a text
    layer). When OCR would only refine a digital document and the engine is missing, the job
    continues without OCR and says so.
    """
    if mode is None:
        mode = "auto"
    if mode not in OCR_MODES:
        raise EngineError("invalid_request", "options.ocr must be auto, tesseract, rapidocr or off")
    if mode == "off":
        return OcrPlan(engine=None)
    if mode == "tesseract":
        if not tesseract:
            return _unavailable(needed, "Tesseract 5 was not found on PATH")
        return _tesseract(hints, tessdata_languages)
    if mode == "rapidocr":
        if not rapidocr:
            return _unavailable(needed, "the RapidOCR models are not installed")
        return _rapidocr(hints)

    vietnamese = "vi" in hints
    if tesseract and vietnamese:
        return _tesseract(hints, tessdata_languages)
    rapidocr_covers = all(_LANGUAGES.get(hint, ("", None))[1] is not None for hint in hints)
    if rapidocr and not vietnamese and rapidocr_covers:
        return _rapidocr(hints)
    if tesseract:
        return _tesseract(hints, tessdata_languages)
    if vietnamese:
        return _unavailable(needed, "Tesseract 5 is required for Vietnamese OCR")
    if rapidocr:
        return _rapidocr(hints)
    return _unavailable(needed, "no OCR engine is installed")


def _unavailable(needed: bool, reason: str) -> OcrPlan:
    if needed:
        raise EngineError("tool_missing", f"OCR is required but {reason}")
    return OcrPlan(
        engine=None,
        warnings=(("ocr_fallback", f"OCR was skipped because {reason}"),),
    )


def _tesseract(hints: tuple[str, ...], available: frozenset[str]) -> OcrPlan:
    languages: list[str] = []
    warnings: list[tuple[str, str]] = []
    for hint in hints:
        code = _LANGUAGES.get(hint, (hint, None))[0]
        if code in available:
            languages.append(code)
        else:
            warnings.append(
                (
                    "ocr_language_unsupported",
                    f"Tesseract has no language data for '{hint}'; install it to OCR that language",
                )
            )
    if not languages:
        if "eng" not in available:
            raise EngineError("tool_missing", "no Tesseract language data is installed")
        languages.append("eng")
    return OcrPlan(engine="tesseract", languages=tuple(languages), warnings=tuple(warnings))


def _rapidocr(hints: tuple[str, ...]) -> OcrPlan:
    warnings: list[tuple[str, str]] = []
    for hint in hints:
        tag = _LANGUAGES.get(hint, ("", None))[1]
        if hint == "vi":
            warnings.append(
                (
                    "ocr_language_unsupported",
                    "RapidOCR models miss many Vietnamese letters; use ocr=tesseract instead",
                )
            )
            tag = "vi"
        if tag is not None:
            return OcrPlan(engine="rapidocr", languages=(f"iso:{tag}",), warnings=tuple(warnings))
    warnings.append(
        (
            "ocr_language_unsupported",
            "RapidOCR has no model for the requested languages; using English",
        )
    )
    return OcrPlan(engine="rapidocr", languages=("iso:en",), warnings=tuple(warnings))
