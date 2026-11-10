"""Expand the host PageRange grammar for page-filtered analysis."""

from __future__ import annotations

import re
from typing import Any

from ariad_inpaint.protocol import EngineError

REFERENCE = r"(?:last(?:-[1-9]\d*)?|[1-9]\d*)"


def selected_pages(value: Any, total: int) -> set[int]:
    if value is None:
        value = "all"
    if not isinstance(value, str):
        raise EngineError("invalid_request", "pages must be a page range string")

    def resolve(reference: str) -> int:
        if reference == "last":
            number = total
        elif reference.startswith("last-"):
            number = total - int(reference[5:])
        else:
            number = int(reference)
        if not 1 <= number <= total:
            raise EngineError("invalid_request", "page is outside the document")
        return number

    selected: set[int] = set()
    for part in value.split(","):
        selector = part.strip()
        if selector in {"all", "odd", "even"}:
            pages = set(
                range(2 if selector == "even" else 1, total + 1, 1 if selector == "all" else 2)
            )
        elif re.fullmatch(REFERENCE, selector):
            pages = {resolve(selector)}
        else:
            match = re.fullmatch(f"({REFERENCE})?-({REFERENCE})?", selector)
            if not match or not (match[1] or match[2]):
                raise EngineError("invalid_request", "invalid page range")
            first = resolve(match[1]) if match[1] else 1
            last = resolve(match[2]) if match[2] else total
            pages = set(range(min(first, last), max(first, last) + 1))
        if selected.intersection(pages):
            raise EngineError("invalid_request", "page selection contains duplicates")
        selected.update(pages)
    return selected
