from __future__ import annotations

import pytest

from ariad_docling.ocr import DEFAULT_HINTS, normalize_hints, plan_ocr
from ariad_docling.protocol import EngineError

PACK = frozenset({"vie", "eng", "osd"})


def plan(mode, hints=DEFAULT_HINTS, *, needed=True, tesseract=True, rapidocr=True, data=PACK):
    return plan_ocr(
        mode,
        hints,
        needed=needed,
        tesseract=tesseract,
        tessdata_languages=data,
        rapidocr=rapidocr,
    )


def test_normalizes_language_hints():
    assert normalize_hints(None) == ("vi", "en")
    assert normalize_hints("vie") == ("vi",)
    assert normalize_hints(["iso:zh-Hans", "en", "eng", "VI"]) == ("zh", "en", "vi")
    with pytest.raises(EngineError):
        normalize_hints([1])


def test_auto_sends_vietnamese_and_mixed_english_to_tesseract():
    result = plan("auto")
    assert result.engine == "tesseract"
    assert result.languages == ("vie", "eng")


def test_auto_sends_other_languages_to_rapidocr():
    english = plan("auto", ("en",))
    assert english.engine == "rapidocr"
    assert english.languages == ("iso:en",)
    result = plan("auto", ("fr",))
    assert result.engine == "rapidocr"
    assert result.languages == ("iso:fr",)
    assert plan("auto", ("zh",)).languages == ("iso:zh-Hans",)


def test_auto_without_tesseract_refuses_vietnamese_ocr_but_not_optional_ocr():
    with pytest.raises(EngineError) as error:
        plan("auto", tesseract=False)
    assert error.value.code == "tool_missing"
    skipped = plan("auto", tesseract=False, needed=False)
    assert skipped.engine is None
    assert skipped.warnings[0][0] == "ocr_fallback"
    assert plan("auto", ("en",), tesseract=False).engine == "rapidocr"


def test_forced_engines_warn_about_unsupported_languages():
    tesseract = plan("tesseract", ("vi", "fr"))
    assert tesseract.languages == ("vie",)
    assert tesseract.warnings[0][0] == "ocr_language_unsupported"
    rapid = plan("rapidocr", ("vi",))
    assert rapid.engine == "rapidocr"
    assert rapid.warnings[0][0] == "ocr_language_unsupported"
    fallback = plan("rapidocr", ("ko",))
    assert fallback.languages == ("iso:en",)


def test_off_and_invalid_modes():
    assert plan("off").engine is None
    with pytest.raises(EngineError) as error:
        plan("sometimes")
    assert error.value.code == "invalid_request"
    with pytest.raises(EngineError):
        plan("tesseract", data=frozenset())
