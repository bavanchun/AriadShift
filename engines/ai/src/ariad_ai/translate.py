"""Deterministic EN/VI glossary translation that leaves structural data untouched."""

from __future__ import annotations

import copy
import re
from typing import Any

from ariad_ai.protocol import EngineError

# This is a deliberately bounded offline glossary, not a claim of model fluency.
PHRASES = {
    "hello world": "xin chào thế giới",
    "thank you": "cảm ơn",
    "good morning": "chào buổi sáng",
    "good afternoon": "chào buổi chiều",
    "good night": "chúc ngủ ngon",
    "please": "vui lòng",
    "welcome": "chào mừng",
    "hello": "xin chào",
    "document": "tài liệu",
    "documents": "các tài liệu",
    "report": "báo cáo",
    "reports": "các báo cáo",
    "summary": "tóm tắt",
    "annual report": "báo cáo thường niên",
    "table of contents": "mục lục",
    "page number": "số trang",
    "page numbers": "các số trang",
    "first name": "tên",
    "last name": "họ",
    "full name": "họ và tên",
    "customer": "khách hàng",
    "customers": "các khách hàng",
    "company": "công ty",
    "companies": "các công ty",
    "project": "dự án",
    "projects": "các dự án",
    "meeting": "cuộc họp",
    "meeting notes": "ghi chú cuộc họp",
    "notes": "ghi chú",
    "data": "dữ liệu",
    "privacy": "quyền riêng tư",
    "security": "bảo mật",
    "confidential": "bảo mật",
    "public": "công khai",
    "private": "riêng tư",
    "information": "thông tin",
    "personal information": "thông tin cá nhân",
    "total": "tổng",
    "subtotal": "tổng phụ",
    "price": "giá",
    "quantity": "số lượng",
    "amount": "số tiền",
    "invoice": "hóa đơn",
    "payment": "thanh toán",
    "revenue": "doanh thu",
    "expenses": "chi phí",
    "profit": "lợi nhuận",
    "budget": "ngân sách",
    "date": "ngày",
    "time": "thời gian",
    "year": "năm",
    "month": "tháng",
    "day": "ngày",
    "today": "hôm nay",
    "tomorrow": "ngày mai",
    "yesterday": "hôm qua",
    "name": "tên",
    "address": "địa chỉ",
    "phone": "điện thoại",
    "email": "email",
    "title": "tiêu đề",
    "heading": "tiêu đề",
    "introduction": "giới thiệu",
    "conclusion": "kết luận",
    "results": "kết quả",
    "result": "kết quả",
    "description": "mô tả",
    "example": "ví dụ",
    "examples": "các ví dụ",
    "chapter": "chương",
    "section": "phần",
    "appendix": "phụ lục",
    "table": "bảng",
    "list": "danh sách",
    "image": "hình ảnh",
    "text": "văn bản",
    "page": "trang",
    "pages": "các trang",
    "file": "tệp",
    "files": "các tệp",
    "open": "mở",
    "close": "đóng",
    "save": "lưu",
    "delete": "xóa",
    "add": "thêm",
    "edit": "chỉnh sửa",
    "search": "tìm kiếm",
    "translate": "dịch",
    "download": "tải xuống",
    "read": "đọc",
    "write": "viết",
    "create": "tạo",
    "update": "cập nhật",
    "new": "mới",
    "old": "cũ",
    "important": "quan trọng",
    "complete": "hoàn thành",
    "successful": "thành công",
    "error": "lỗi",
    "warning": "cảnh báo",
    "yes": "có",
    "no": "không",
    "and": "và",
    "or": "hoặc",
    "with": "với",
    "without": "không có",
    "for": "cho",
    "from": "từ",
    "to": "đến",
    "in": "trong",
    "on": "trên",
    "of": "của",
    "is": "là",
    "are": "là",
    "this": "này",
    "that": "đó",
    "these": "những",
    "those": "những",
    "we": "chúng tôi",
    "you": "bạn",
    "our": "của chúng tôi",
    "your": "của bạn",
    "the": "",
    "a": "một",
    "an": "một",
    "one": "một",
    "two": "hai",
    "three": "ba",
    "four": "bốn",
    "five": "năm",
    "book": "sách",
    "books": "các sách",
    "school": "trường học",
    "student": "học sinh",
    "teacher": "giáo viên",
    "language": "ngôn ngữ",
    "english": "tiếng Anh",
    "vietnamese": "tiếng Việt",
    "world": "thế giới",
    "test": "kiểm tra",
}


def language(value: str) -> str:
    aliases = {"english": "en", "eng": "en", "vietnamese": "vi", "vie": "vi"}
    return aliases.get(value.lower(), value.lower().split("-")[0])


def detect_language(text: str) -> str:
    return (
        "vi"
        if re.search(r"[àáảãạăâđèéẻẽẹêìíỉĩịòóỏõọôơùúủũụưỳýỷỹỵ\u1ea0-\u1ef9]", text.lower())
        else "en"
    )


class Translator:
    def __init__(self, target: str, source: str | None = None, model: Any = None) -> None:
        self.target = language(target)
        self.source = language(source) if source else None
        self.model = model
        self.unknown: set[str] = set()
        if self.target not in {"en", "vi"} and model is None:
            raise EngineError(
                "tool_missing", "this language requires a configured local GGUF model"
            )

    def text(self, text: str) -> str:
        if not text.strip():
            return text
        source = self.source or detect_language(text)
        if source == self.target:
            return text
        if self.model:
            return self.model.generate(
                f"Translate from {source} to {self.target}. Preserve all numbers, Markdown, "
                f"names and whitespace. Return only translated text.\n{text}"
            ).strip()
        mapping = (
            PHRASES if self.target == "vi" else {v.lower(): k for k, v in PHRASES.items() if v}
        )
        patterns = sorted(mapping, key=len, reverse=True)
        pattern = re.compile(r"(?<!\w)(?:" + "|".join(map(re.escape, patterns)) + r")(?!\w)", re.I)
        matches = list(pattern.finditer(text))
        covered = set()
        for match in matches:
            covered.update(range(match.start(), match.end()))
        for word in re.finditer(r"[^\W\d_]+", text):
            if not any(i in covered for i in range(word.start(), word.end())):
                self.unknown.add(word[0])

        def replace(match: re.Match[str]) -> str:
            value = mapping[match[0].lower()]
            if match[0].isupper():
                return value.upper()
            if match[0][0].isupper():
                return value[:1].upper() + value[1:]
            return value

        return pattern.sub(replace, text)

    def ir(self, document: dict[str, Any]) -> dict[str, Any]:
        result = copy.deepcopy(document)

        def visit(node: Any) -> None:
            if isinstance(node, list):
                for item in node:
                    visit(item)
            elif isinstance(node, dict):
                if node.get("type") == "text" and isinstance(node.get("text"), str):
                    node["text"] = self.text(node["text"])
                # Restrict traversal to document containers; never translate IDs, style
                # names, source maps, URLs, math, code, assets or provenance values.
                for key in ("body", "blocks", "content", "items", "head", "caption"):
                    visit(node.get(key))

        visit(result)
        result.setdefault("meta", {})["language"] = self.target
        for key in ("title", "subject"):
            if result["meta"].get(key):
                result["meta"][key] = self.text(result["meta"][key])
        return result

    def markup(self, text: str, fmt: str) -> str:
        # Keep tags, entities, code spans, fenced code, and link destinations byte-for-byte.
        protected = r"(<[^>]+>|&[\w#]+;|```[\s\S]*?```|`[^`]*`|\]\([^)]*\))"
        pieces = re.split(protected, text)
        result = []
        hidden = 0
        for index, piece in enumerate(pieces):
            if index % 2:
                if re.match(r"<(?:script|style)\b", piece, re.I):
                    hidden += 1
                elif re.match(r"</(?:script|style)\b", piece, re.I):
                    hidden = max(0, hidden - 1)
                result.append(piece)
            else:
                result.append(piece if hidden else self.text(piece))
        return "".join(result)
