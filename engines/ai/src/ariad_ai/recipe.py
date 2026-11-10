"""Conservative bilingual intent parsing into imported, unexecuted recipes."""

from __future__ import annotations

import re
import unicodedata
from typing import Any

from ariad_ai.protocol import EngineError


def fold(text: str) -> str:
    return "".join(
        c
        for c in unicodedata.normalize("NFD", text.lower().replace("đ", "d"))
        if not unicodedata.combining(c)
    )


# Match actions before extracting arguments. Longer, more specific verbs take priority.
ACTIONS = re.compile(
    r"\b(?P<raster>(?:remove|erase)\s+(?:raster|burned[- ]in)\s+watermarks?"
    r"|xoa\s+(?:thuy van|watermark)\s+(?:raster|anh))\b"
    r"|\b(?P<remove>(?:remove|erase)\s+watermarks?|xoa\s+(?:thuy van|watermark))\b"
    r"|\b(?P<extract>(?:extract|keep)\s+pages?|trich xuat\s+trang|giu\s+trang)\b"
    r"|\b(?P<delete>(?:delete|remove)\s+pages?|xoa\s+trang)\b"
    r"|\b(?P<merge>merge|combine|join|gop|noi)\b"
    r"|\b(?P<split>split|tach|chia)\b"
    r"|\b(?P<rotate>rotate|xoay)\b"
    r"|\b(?P<compress>compress|shrink|nen)\b"
    r"|\b(?P<ocr>ocr|nhan dien\s+(?:chu|van ban))\b"
    r"|\b(?P<deskew>deskew|can thang)\b"
    r"|\b(?P<watermark>(?:add|apply)\s+(?:a\s+)?watermark|watermark|dong dau)\b"
    r"|\b(?P<redact>redact|che\s+(?:thong tin|van ban))\b"
    r"|\b(?P<pdfa>(?:convert\s+to\s+|chuyen\s+sang\s+)?pdf/a(?:[- ]?(?:2b|2u|3b))?)\b"
    r"|\b(?P<grayscale>grayscale|greyscale|den trang)\b"
    r"|\b(?P<numbers>(?:add\s+)?page numbers|number\s+(?:the\s+)?pages|danh so trang)\b"
    r"|\b(?P<convert>convert\s+(?:(?:it|them|the\s+(?:file|document|pdf))\s+)?"
    r"(?:to|into)|chuyen\s+(?:(?:doi|tep|tai lieu|pdf)\s+)?sang)\b"
)
CONDITION = re.compile(
    r"(?:if\s+(?:the\s+)?(?:page_count|page count)|neu\s+so trang)\s*"
    r"(>=|<=|==|!=|>|<|exceeds|vuot qua|lon hon)\s*(\d+)"
)
PAGE_RANGE = re.compile(
    r"\b(?:pages?|trang)\s+([1-9]\d*(?:\s*-\s*[1-9]\d*)?"
    r"(?:\s*,\s*[1-9]\d*(?:\s*-\s*[1-9]\d*)?)*)"
)
QUOTED = re.compile(r"""["“']([^"”']+)["”']""")


def invalid(message: str) -> EngineError:
    return EngineError("invalid_request", message)


def pages_in(clause: str, *, required: bool = False) -> str:
    match = PAGE_RANGE.search(clause)
    if match:
        pages = re.sub(r"\s+", "", match[1])
        selected: set[int] = set()
        for part in pages.split(","):
            ends = list(map(int, part.split("-")))
            low, high = ends[0], ends[-1]
            if low > high or high > 1_000_000 or selected.intersection(range(low, high + 1)):
                raise invalid("page ranges must be ordered and unique")
            selected.update(range(low, high + 1))
        return pages
    if required:
        raise invalid("specify explicit page numbers for this operation")
    return "all"


def parse_step(action: str, clause: str, original: str) -> dict[str, Any]:
    params: dict[str, Any] = {}
    tool = "pdf." + action
    if action == "convert":
        targets = {
            "word": "docx",
            "docx": "docx",
            "markdown": "markdown",
            "md": "markdown",
            "html": "html",
            "pdf": "pdf",
            "epub": "epub",
        }
        match = re.search(r"\b(word|docx|markdown|md|html|pdf|epub)\b", clause)
        if not match:
            raise invalid("specify a supported conversion target")
        return {"convert": {"to": targets[match[1]]}}
    if action == "split":
        match = re.search(r"(?:every|moi)\s+(\d+)", clause)
        if match:
            if int(match[1]) < 1:
                raise invalid("split interval must be positive")
            params = {"mode": "every", "every": int(match[1])}
        elif re.search(r"bookmark|dau trang", clause):
            params = {"mode": "bookmarks"}
        elif re.search(r"(?:pages?|trang)\s+\d", clause):
            params = {"mode": "ranges", "ranges": pages_in(clause, required=True).split(",")}
        else:
            params = {"mode": "every", "every": 1}
    elif action == "rotate":
        angle = re.search(r"(?<!\d)(-90|90|180|270)(?!\d)", PAGE_RANGE.sub("pages", clause))
        if not angle:
            raise invalid("specify a rotation angle: 90, 180 or 270")
        params = {"angle": int(angle[1]), "pages": pages_in(clause)}
    elif action in {"delete", "extract"}:
        tool = "pdf.delete_pages" if action == "delete" else "pdf.extract_pages"
        params = {"pages": pages_in(clause, required=True)}
    elif action == "compress":
        preset = re.search(r"\b(screen|ebook|print|lossless)\b", clause)
        if preset:
            params["preset"] = preset[1]
        level = re.search(r"(?:level|muc)\s+([1-9])\b", clause)
        if level:
            params = {"level": int(level[1])}
    elif action == "ocr":
        languages = []
        if re.search(r"vietnamese|tieng viet|\bvie\b", clause):
            languages.append("vie")
        if re.search(r"english|tieng anh|\beng\b", clause):
            languages.append("eng")
        if languages:
            params["languages"] = languages
    elif action == "watermark":
        quoted = QUOTED.search(original)
        if not quoted:
            raise invalid("quote the watermark text")
        params = {"kind": "text", "text": quoted[1]}
    elif action in {"remove", "raster"}:
        candidates = re.search(
            r"\b(?:candidates?|ma|ids?)\b\s*[:=]?\s*([a-z0-9_.-]+"
            r"(?:\s*,\s*[a-z0-9_.-]+)*)",
            clause,
        )
        if action == "remove" and candidates:
            tool = "pdf.watermark_remove"
            params = {
                "candidates": [
                    c.strip() for c in original[candidates.start(1) : candidates.end(1)].split(",")
                ]
            }
        else:
            tool = "pdf.watermark_remove_raster"
            params = {"mask": "auto"}
    elif action == "redact":
        quoted = QUOTED.search(original)
        if quoted:
            targets = [{"kind": "exact", "text": quoted[1]}]
        elif re.search(r"emails?|email", clause):
            targets = [{"kind": "preset", "preset": "email"}]
        elif re.search(r"phone|dien thoai", clause):
            targets = [{"kind": "preset", "preset": "phone"}]
        else:
            raise invalid("quote redaction text or specify an email/phone preset")
        params = {"targets": targets}
    elif action == "pdfa":
        tool = "pdf.to_pdfa"
        level = re.search(r"\b(2b|2u|3b)\b", clause)
        params = {"level": level[1] if level else "2b"}
    elif action == "numbers":
        tool = "pdf.page_numbers"
        start = re.search(r"(?:start(?:ing)?\s+(?:at|from)|bat dau\s+tu)\s+(\d+)", clause)
        if start:
            params["start"] = int(start[1])
    return {"tool": tool, "params": params}


def recipe_from_text(prompt: str) -> dict[str, Any]:
    if not isinstance(prompt, str) or not prompt.strip() or len(prompt) > 32_768:
        raise invalid("prompt must contain 1 to 32768 characters")
    # Mask quoted literals before looking for verbs, preserving character positions.
    masked = QUOTED.sub(lambda m: " " * len(m[0]), prompt)
    normalized = fold(masked)
    if re.search(
        r"\b(?:curl|wget|upload|download|encrypt|decrypt|shell|bash|watch)\b"
        r"|https?://|ma hoa|tai len|gui den",
        normalized,
    ):
        raise invalid("prompt contains an unsupported or remote operation")
    if len(re.findall(r"\b(?:if|neu)\b", normalized)) != len(list(CONDITION.finditer(normalized))):
        raise invalid("specify conditions as if page_count > N")
    actions = list(ACTIONS.finditer(normalized))
    if not actions:
        raise invalid("no supported document operation was found")
    if len(actions) > 32:
        raise invalid("recipe exceeds 32 steps")
    steps = []
    for index, match in enumerate(actions):
        end = actions[index + 1].start() if index + 1 < len(actions) else len(normalized)
        clause = normalized[match.start() : end]
        # Folding diacritics preserves character count for NFC EN/VI prompts.
        original = prompt[match.start() : end]
        step = parse_step(match.lastgroup or "", CONDITION.sub("", clause), original)
        steps.append(step)
    for condition in CONDITION.finditer(normalized):
        previous = [i for i, action in enumerate(actions) if action.end() <= condition.start()]
        following = [i for i, action in enumerate(actions) if action.start() >= condition.end()]
        if not previous:
            owner = following[0]
        elif not following:
            owner = previous[-1]
        else:
            gap = normalized[actions[previous[-1]].end() : condition.start()]
            separator = re.search(r"(?:\bthen\b|\broi\b|\band\b|\bva\b|[,;])\s*$", gap)
            owner = following[0] if separator else previous[-1]
        if "if" in steps[owner] or int(condition[2]) > 2**64 - 1:
            raise invalid("step conditions must contain one bounded integer comparison")
        operator = condition[1] if condition[1] in {">", "<", ">=", "<=", "==", "!="} else ">"
        steps[owner]["if"] = f"page_count {operator} {int(condition[2])}"
    recipe: dict[str, Any] = {"version": "ariad-recipe/1", "steps": steps}
    naming = re.search(
        r"(?:nam(?:e|ing)|filename|ten(?: tep)?|dat ten)\s*(?:as|theo|la|=|:)?\s*"
        r"""["']?((?:\{(?:stem|date|title|n)\}|[A-Za-z0-9_. -])+\})""",
        masked,
        re.IGNORECASE,
    )
    if not naming:
        naming = re.search(r"(\{(?:stem|title)\}(?:[-_ ]\{(?:date|n)\})*)", prompt)
    if naming:
        template = naming[1].strip()
        if template.startswith(".") or ".." in template:
            raise invalid("unsafe naming template")
        recipe["naming"] = template
    return recipe
