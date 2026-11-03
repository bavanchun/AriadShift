from ariad_bench.ocr_eval import normalize, score


def test_normalizes_unicode_and_whitespace_equally() -> None:
    assert normalize("Vie\u0323̂t\n  Nam") == "Việt Nam"
    assert score("Việt\nNam", "Vie\u0323̂t Nam") == {"cer": 0.0, "wer": 0.0}


def test_reports_character_and_word_errors() -> None:
    result = score("one two", "one too")
    assert result["cer"] == 1 / 7
    assert result["wer"] == 1 / 2
