"""The ``convert`` operation: input document to DoclingDocument JSON."""

from __future__ import annotations

import errno
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from ariad_docling.describe import (
    INPUT_FORMATS,
    OUTPUT_FORMAT,
    tessdata_languages,
    tesseract_status,
)
from ariad_docling.environment import (
    Assets,
    apply_memory_limit,
    prepare_job,
    resolve_assets,
    thread_count,
)
from ariad_docling.ocr import OcrPlan, normalize_hints, plan_ocr
from ariad_docling.protocol import ConvertRequest, EngineError, Writer, optional_int

if TYPE_CHECKING:
    from docling.document_converter import DocumentConverter

OUTPUT_NAME = "document.docling.json"
PAGED_FORMATS = frozenset({"pdf", "png", "jpeg", "tiff", "webp"})
IMAGE_FORMATS = frozenset({"png", "jpeg", "tiff", "webp"})
# Pictures are rendered at twice the 72 dpi page scale.
PICTURE_SCALE = 2.0


@dataclass(frozen=True)
class PipelineKey:
    """Everything that shapes a converter; equal keys can share a warm converter."""

    input_format: str
    artifacts: str | None
    tessdata: str | None
    ocr_engine: str | None
    ocr_languages: tuple[str, ...]
    full_page_ocr: bool


class Converters:
    """Warm converters kept across the jobs of a session."""

    def __init__(self) -> None:
        self._cache: dict[PipelineKey, DocumentConverter] = {}

    def get(self, key: PipelineKey) -> DocumentConverter:
        converter = self._cache.get(key)
        if converter is None:
            converter = build_converter(key)
            self._cache[key] = converter
        return converter


@dataclass(frozen=True)
class Job:
    """A validated conversion job."""

    request: ConvertRequest
    source: Path
    output_dir: Path
    assets: Assets
    max_pages: int | None
    max_memory_mb: int | None
    started: float


def run(request: ConvertRequest, writer: Writer, converters: Converters) -> None:
    """Runs one job and emits its events; raises :class:`EngineError` on typed failures."""
    started = time.monotonic()
    if request.output_format != OUTPUT_FORMAT:
        raise EngineError("unsupported_route", f"output format must be {OUTPUT_FORMAT}")
    if request.input_format not in INPUT_FORMATS:
        raise EngineError(
            "unsupported_route", "input format is not supported by the docling engine"
        )
    max_input_bytes = optional_int(request.limits, "max_input_bytes")
    job = Job(
        request=request,
        source=Path(request.input_path),
        output_dir=Path(request.output_dir),
        assets=resolve_assets(request.options),
        max_pages=optional_int(request.limits, "max_pages"),
        max_memory_mb=optional_int(request.limits, "max_memory_mb"),
        started=started,
    )
    if not job.source.is_file():
        raise EngineError("io", "input file does not exist")
    if not job.output_dir.is_dir():
        raise EngineError("io", "output directory does not exist")
    if max_input_bytes is not None and job.source.stat().st_size > max_input_bytes:
        raise EngineError("limit_exceeded", f"input exceeds the limit of {max_input_bytes} bytes")

    prepare_job(request.work_dir, job.assets)
    # Each job starts unbounded so a session can load what it needs; the limit returns below.
    apply_memory_limit(None)
    try:
        _convert(job, writer, converters)
    except MemoryError as error:
        raise _memory_error(job.max_memory_mb) from error
    except OSError as error:
        if job.max_memory_mb is not None and error.errno == errno.ENOMEM:
            raise _memory_error(job.max_memory_mb) from error
        raise


def _memory_error(max_memory_mb: int | None) -> EngineError:
    if max_memory_mb is None:
        return EngineError("engine_failure", "the engine ran out of memory")
    return EngineError("limit_exceeded", f"the memory limit of {max_memory_mb} MB was exceeded")


def _convert(job: Job, writer: Writer, converters: Converters) -> None:
    request, assets = job.request, job.assets
    input_format = request.input_format
    # Looking for a text layer opens the untrusted PDF before any model is loaded, so the limit
    # already applies; it is lifted again while the models load.
    apply_memory_limit(job.max_memory_mb)
    needs_ocr, full_page = _ocr_need(input_format, job.source, job.max_pages)
    apply_memory_limit(None)
    plan = OcrPlan(engine=None)
    if input_format in PAGED_FORMATS:
        if assets.artifacts is None:
            raise EngineError(
                "tool_missing", "the Docling layout and table models are not installed"
            )
        plan = plan_ocr(
            request.options.get("ocr"),
            normalize_hints(request.options.get("lang")),
            needed=needs_ocr,
            tesseract=tesseract_status()["status"] == "found",
            tessdata_languages=tessdata_languages(assets.tessdata),
            rapidocr=(assets.artifacts / "RapidOcr").is_dir(),
        )
    for code, message in plan.warnings:
        writer.warning(code, message)

    key = PipelineKey(
        input_format=input_format,
        artifacts=str(assets.artifacts) if assets.artifacts else None,
        tessdata=str(assets.tessdata) if assets.tessdata else None,
        ocr_engine=plan.engine,
        ocr_languages=plan.languages,
        full_page_ocr=full_page and plan.engine is not None,
    )
    try:
        converter = converters.get(key)
    except ImportError as error:
        raise EngineError("engine_failure", "the Docling runtime could not be loaded") from error
    apply_memory_limit(job.max_memory_mb)

    from docling.datamodel.base_models import ConversionStatus, FailureCategory
    from docling.datamodel.progress import PageCompletedProgress
    from docling_core.types.doc import ImageRefMode

    def on_progress(event: Any) -> None:
        if isinstance(event, PageCompletedProgress):
            writer.progress(event.completed_pages, event.total_pages)

    result = converter.convert(
        job.source,
        raises_on_error=False,
        max_num_pages=job.max_pages if job.max_pages is not None else sys.maxsize,
        progress_callback=on_progress,
    )
    if result.has_errors(FailureCategory.POLICY):
        raise EngineError("limit_exceeded", "the document exceeds a configured limit")
    if result.status not in (ConversionStatus.SUCCESS, ConversionStatus.PARTIAL_SUCCESS):
        raise EngineError("engine_failure", "Docling could not convert the document")
    if result.status == ConversionStatus.PARTIAL_SUCCESS:
        writer.warning("partial_conversion", "some pages or elements could not be converted")

    pages = len(result.pages) or len(result.document.pages)
    if pages:
        writer.progress(pages, pages)
    target = job.output_dir / OUTPUT_NAME
    result.document.save_as_json(target, image_mode=ImageRefMode.EMBEDDED)
    writer.artifact(str(target), OUTPUT_FORMAT)
    elapsed_ms = int((time.monotonic() - job.started) * 1000)
    writer.ok({"pages": pages, "elapsed_ms": elapsed_ms})


def _ocr_need(input_format: str, source: Path, max_pages: int | None) -> tuple[bool, bool]:
    """Returns (OCR is needed, OCR must cover whole pages).

    Images always need full-page OCR. A PDF needs OCR when a page has no text layer, and whole
    pages when none of them has one.
    """
    if input_format in IMAGE_FORMATS:
        return True, True
    if input_format != "pdf":
        return False, False
    try:
        import pypdfium2
    except ImportError as error:
        raise EngineError("engine_failure", "the Docling runtime could not be loaded") from error
    try:
        document = pypdfium2.PdfDocument(source)
    except pypdfium2.PdfiumError as error:
        raise EngineError("engine_failure", "the PDF could not be opened") from error
    try:
        page_count = len(document)
        if max_pages is not None and page_count > max_pages:
            raise EngineError("limit_exceeded", f"the document has more than {max_pages} pages")
        without_text = 0
        for index in range(page_count):
            page = document[index]
            text_page = page.get_textpage()
            try:
                if text_page.count_chars() == 0:
                    without_text += 1
            finally:
                text_page.close()
                page.close()
    finally:
        document.close()
    return without_text > 0, page_count > 0 and without_text == page_count


def build_converter(key: PipelineKey) -> DocumentConverter:
    from docling.datamodel.accelerator_options import AcceleratorOptions
    from docling.datamodel.base_models import InputFormat
    from docling.datamodel.pipeline_options import (
        PdfPipelineOptions,
        RapidOcrOptions,
        TableFormerMode,
        TableStructureOptions,
        TesseractCliOcrOptions,
    )
    from docling.document_converter import (
        DocumentConverter,
        ExcelFormatOption,
        ImageFormatOption,
        PdfFormatOption,
        PowerpointFormatOption,
        WordFormatOption,
    )

    if key.input_format in ("docx", "pptx", "xlsx"):
        input_format, option = {
            "docx": (InputFormat.DOCX, WordFormatOption()),
            "pptx": (InputFormat.PPTX, PowerpointFormatOption()),
            "xlsx": (InputFormat.XLSX, ExcelFormatOption()),
        }[key.input_format]
        return DocumentConverter(
            allowed_formats=[input_format], format_options={input_format: option}
        )

    ocr_options: Any = None
    if key.ocr_engine == "tesseract":
        ocr_options = TesseractCliOcrOptions(
            lang=list(key.ocr_languages),
            path=key.tessdata,
            force_full_page_ocr=key.full_page_ocr,
        )
    elif key.ocr_engine == "rapidocr":
        ocr_options = RapidOcrOptions(
            lang=list(key.ocr_languages),
            backend="onnxruntime",
            force_full_page_ocr=key.full_page_ocr,
        )
    pipeline = PdfPipelineOptions(
        artifacts_path=key.artifacts,
        enable_remote_services=False,
        allow_external_plugins=False,
        do_ocr=ocr_options is not None,
        do_table_structure=True,
        table_structure_options=TableStructureOptions(mode=TableFormerMode.ACCURATE),
        generate_picture_images=True,
        images_scale=PICTURE_SCALE,
        accelerator_options=AcceleratorOptions(num_threads=thread_count(), device="cpu"),
    )
    if ocr_options is not None:
        pipeline.ocr_options = ocr_options
    if key.input_format == "pdf":
        input_format, option = InputFormat.PDF, PdfFormatOption(pipeline_options=pipeline)
    else:
        input_format, option = InputFormat.IMAGE, ImageFormatOption(pipeline_options=pipeline)
    return _initialized(DocumentConverter, input_format, option)


def _initialized(converter_type: Any, input_format: Any, option: Any) -> DocumentConverter:
    """Builds a converter and loads its models now, before a memory limit applies."""
    converter = converter_type(
        allowed_formats=[input_format], format_options={input_format: option}
    )
    try:
        converter.initialize_pipeline(input_format)
    except FileNotFoundError as error:
        raise EngineError("tool_missing", "the Docling models are incomplete") from error
    return converter
