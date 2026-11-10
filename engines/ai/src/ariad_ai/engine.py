"""One-shot, offline ariad-engine/2 tool dispatcher."""

from __future__ import annotations

import json
import math
import re
from collections import Counter
from pathlib import Path
from typing import IO, Any

from ariad_ai import __version__
from ariad_ai import artifacts as out
from ariad_ai.config import pack_options
from ariad_ai.document import Document, read_document
from ariad_ai.model import LocalModel
from ariad_ai.pages import selected_pages as select_pages
from ariad_ai.protocol import (
    DescribeRequest,
    EngineError,
    RunToolRequest,
    Writer,
    parse_request,
    read_line,
)
from ariad_ai.recipe import fold, recipe_from_text
from ariad_ai.translate import Translator, detect_language

TOOLS = ("ai.summarize", "ai.ask", "ai.translate", "ai.extract_tables", "ai.recipe_from_text")
STOP_WORDS = set(
    "the a an is are was were what when where how why does do of in to and or "
    "for with from this that document please la cua va cac nhung gi nao o cho".split()
)


def capabilities() -> dict[str, Any]:
    return {
        "type": "capabilities",
        "protocol": "ariad-engine/2",
        "engine": "ai",
        "version": __version__,
        "tool": {"name": "ai", "version": __version__, "status": "found"},
        "license": "Apache-2.0 AND MIT",
        "routes": [],
        "tools": [{"id": name, "version": "1"} for name in TOOLS],
        "enforces_memory_limit": False,
        "supports_session": False,
    }


def tokens(text: str) -> list[str]:
    return [word for word in re.findall(r"\w+", fold(text)) if word not in STOP_WORDS]


def summarize(doc: Document, params: dict[str, Any], model: LocalModel | None) -> dict[str, Any]:
    length = params.get("length", "standard")
    if length not in {"brief", "standard", "detailed"}:
        raise EngineError("invalid_request", "length must be brief, standard or detailed")
    sentences = [s.strip() for s in re.split(r"(?<=[.!?])\s+|\n+", doc.text) if s.strip()]
    if not sentences:
        raise EngineError("invalid_request", "document has no text; run OCR first")
    frequency = Counter(tokens(doc.text))
    count = {"brief": 1, "standard": 3, "detailed": 6}[length]
    ranked = sorted(
        range(len(sentences)),
        key=lambda i: (
            sum(frequency[t] for t in tokens(sentences[i]))
            / max(1, math.sqrt(len(tokens(sentences[i]))))
        ),
        reverse=True,
    )
    bullet_points = [sentences[i][:1000] for i in sorted(ranked[:count])]
    summary = " ".join(bullet_points)
    language = params.get("language") or detect_language(doc.text)
    if model:
        summary = model.generate(
            f"Summarize the following document in {language}, length {length}. "
            "Use only facts in the document. Return only the summary.\n" + doc.text
        )
    elif language != detect_language(doc.text):
        translator = Translator(language, detect_language(doc.text))
        summary = translator.text(summary)
        bullet_points = [translator.text(item) for item in bullet_points]
    return {
        "summary": summary,
        "bullet_points": bullet_points,
        "word_count": len(summary.split()),
        "language": language,
        "length": length,
    }


def ask(doc: Document, params: dict[str, Any], model: LocalModel | None) -> dict[str, Any]:
    question = params.get("question")
    if not isinstance(question, str) or not question.strip():
        raise EngineError("invalid_request", "question must be nonempty")
    maximum = params.get("max_citations") if params.get("max_citations") is not None else 5
    if type(maximum) is not int or not 1 <= maximum <= 20:
        raise EngineError("invalid_request", "max_citations must be between 1 and 20")
    wanted = set(tokens(question))
    matches = []
    for number, page in enumerate(doc.pages, 1):
        # Quotes are sliced directly from the page, never generated or normalized.
        for passage in re.finditer(r"[^\n.!?]{1,2000}[.!?]?", page):
            quote = passage[0].strip()
            words = set(tokens(quote))
            overlap = wanted & words
            if overlap:
                score = len(overlap) / max(1, len(wanted))
                matches.append({"page": number, "quote": quote, "score": float(score)})
    matches.sort(key=lambda item: -item["score"])
    citations = matches[:maximum]
    answer = (
        " ".join(c["quote"] for c in citations) if citations else "No supporting passage found."
    )
    if model and citations:
        answer = model.generate(
            "Answer only using these exact source passages. If insufficient, "
            "say so.\nQuestion: " + question + "\n" + answer
        )
    return {"question": question, "answer": answer, "citations": citations}


def translate(
    doc: Document, params: dict[str, Any], model: LocalModel | None, directory: Path, writer: Writer
) -> dict[str, Any]:
    target = params.get("target_lang")
    if not isinstance(target, str) or not target.strip():
        raise EngineError("invalid_request", "target_lang must be nonempty")
    translator = Translator(target, params.get("source_lang") or detect_language(doc.text), model)
    fmt = params.get("format") or "pdf"
    if fmt not in {"json", "md", "html", "docx", "pdf"}:
        raise EngineError("invalid_request", "unsupported translated output format")
    path = directory / f"translated.{fmt}"
    translated_ir = translator.ir(doc.ir) if doc.ir else None
    if translated_ir:
        text = out.ir_markdown(translated_ir)
    elif doc.format in {"md", "markdown", "html", "htm"} and fmt in {"md", "html"}:
        text = translator.markup(Path(doc.source).read_text(encoding="utf-8"), doc.format)
    else:
        text = translator.text(doc.text)
    if fmt == "json":
        value = translated_ir or {
            "version": "ariad-ir/2",
            "meta": {"language": translator.target},
            "assets": {},
            "body": [
                {
                    "type": "paragraph",
                    "id": f"p{index}",
                    "direct": {},
                    "content": [{"type": "text", "text": paragraph}],
                }
                for index, paragraph in enumerate(text.split("\n\n"), 1)
            ],
        }
        out.json_file(path, value)
    elif fmt == "docx" and doc.format == "docx":
        out.translate_docx(Path(doc.source), path, translator)
    elif fmt == "docx":
        out.docx_file(path, text)
    elif fmt == "pdf":
        out.pdf_file(path, text)
    elif fmt == "html" and doc.format in {"html", "htm"}:
        path.write_text(text, encoding="utf-8")
    elif fmt == "html":
        out.html_file(path, text)
    else:
        path.write_text(text, encoding="utf-8")
    if translator.unknown:
        writer.warning(
            "offline_glossary_partial",
            "Offline glossary preserved unknown terms; configure GGUF for full translation.",
        )
    writer.artifact(str(path), fmt)
    return {
        "target_lang": translator.target,
        "format": fmt,
        "backend": "llama.cpp" if model else "offline_glossary",
        "untranslated_terms": len(translator.unknown),
    }


def handle(request: RunToolRequest | DescribeRequest, writer: Writer) -> None:
    try:
        if isinstance(request, DescribeRequest):
            writer.emit(capabilities())
            writer.ok()
            return
        if request.tool not in TOOLS:
            raise EngineError("unsupported_route", "unsupported AI tool")
        if request.params.get("provider", "local") != "local":
            raise EngineError("invalid_request", "cloud providers belong to the consent-gated host")
        recipe_tool = request.tool == "ai.recipe_from_text"
        if (recipe_tool and len(request.inputs) > 1) or (
            not recipe_tool and len(request.inputs) != 1
        ):
            raise EngineError(
                "invalid_request", "tool requires one input (recipe accepts zero or one)"
            )
        directory = Path(request.output_dir).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        Path(request.work_dir).mkdir(parents=True, exist_ok=True)
        options = pack_options(request.options)
        model = (
            LocalModel(options, request.limits)
            if request.tool in {"ai.summarize", "ai.ask", "ai.translate"}
            and any(key in options for key in ("model_path", "gguf_model", "llama_cli"))
            else None
        )
        writer.progress("analyze")
        if request.tool == "ai.recipe_from_text":
            prompt = request.params.get("prompt")
            if prompt is None and request.inputs:
                prompt_path = Path(request.inputs[0]["path"])
                if prompt_path.stat().st_size > 131_072:
                    raise EngineError("limit_exceeded", "staged prompt exceeds 128 KiB")
                prompt = prompt_path.read_text(encoding="utf-8")
                if prompt_path.suffix == ".json":
                    value = json.loads(prompt)
                    prompt = value.get("prompt") if isinstance(value, dict) else value
            recipe = recipe_from_text(prompt or "")
            path = directory / "recipe.json"
            out.json_file(path, recipe)
            writer.artifact(str(path), "json")
            data = {"recipe": recipe, "origin": "imported", "requires_confirmation": True}
        else:
            doc = read_document(request.inputs[0], request.limits)
            if request.tool == "ai.translate":
                data = translate(doc, request.params, model, directory, writer)
            elif request.tool == "ai.extract_tables":
                fmt = request.params.get("format") or "csv"
                if fmt not in {"csv", "json", "xlsx"}:
                    raise EngineError("invalid_request", "table format must be csv, json or xlsx")
                selected = select_pages(request.params.get("pages"), len(doc.pages))
                tables = [table for table in doc.tables if table.page in selected]
                if len(tables) > 1000:
                    raise EngineError("limit_exceeded", "table extraction exceeds 1000 tables")
                metadata = []
                for index, table in enumerate(tables, 1):
                    path = directory / f"table_{index:03}.{fmt}"
                    out.table_file(path, table, fmt)
                    writer.artifact(str(path), fmt)
                    metadata.append(
                        {
                            "page": table.page,
                            "rows": len(table.rows),
                            "columns": max(map(len, table.rows), default=0),
                            "path": str(path),
                        }
                    )
                data = {"tables": metadata, "table_count": len(tables)}
                if not tables:
                    writer.warning("no_tables", "No structured tables were detected.")
            else:
                data = (
                    summarize(doc, request.params, model)
                    if request.tool == "ai.summarize"
                    else ask(doc, request.params, model)
                )
                fmt = request.params.get("format") or "json"
                if (request.tool == "ai.ask" and fmt != "json") or fmt not in {"json", "md"}:
                    raise EngineError("invalid_request", "unsupported output format")
                path = directory / f"{request.tool.split('.')[1]}.{fmt}"
                if fmt == "json":
                    out.json_file(path, data)
                else:
                    path.write_text(
                        data["summary"]
                        + "\n\n"
                        + "\n".join("- " + item for item in data["bullet_points"])
                        + "\n",
                        encoding="utf-8",
                    )
                writer.artifact(str(path), fmt)
        writer.ok(data)
    except EngineError as error:
        writer.error(error)
    except OSError:
        writer.error(EngineError("io", "could not read or write a local engine file"))
    except ValueError, TypeError, KeyError, json.JSONDecodeError:
        writer.error(EngineError("invalid_request", "invalid document or tool parameters"))
    except Exception:
        writer.error(EngineError("engine_failure", "local document analysis failed"))


def main(protocol_fd: int, stdin: IO[bytes]) -> int:
    writer = Writer(protocol_fd)
    try:
        line = read_line(stdin)
        if line is not None:
            handle(parse_request(line), writer)
    except EngineError as error:
        writer.error(error)
    return 0
