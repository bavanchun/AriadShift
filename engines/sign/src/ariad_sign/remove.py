"""Digital signature removal implementation."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from ariad_sign.protocol import EngineError, RunToolRequest, Writer

try:
    from pyhanko.pdf_utils import generic
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.pdf_utils.writer import copy_into_new_writer
    PYHANKO_AVAILABLE = True
except ImportError:
    PYHANKO_AVAILABLE = False


def run_remove(request: RunToolRequest, writer: Writer) -> None:
    """Executes a `pdf.sign_remove` request."""
    if not PYHANKO_AVAILABLE:
        raise EngineError("tool_missing", "pyHanko is not installed or available")

    in_path = Path(request.inputs[0]["path"])
    if not in_path.is_file():
        raise EngineError("io", f"input file does not exist: {in_path}")

    params = request.params
    confirm = bool(params.get("confirm_invalidation", False))
    if not confirm:
        raise EngineError(
            "invalid_request",
            "confirm_invalidation must be true to remove digital signatures (D8)",
        )

    target_field = params.get("field_name")

    writer.progress("pdf.sign_remove", 0, 1)

    stem = in_path.stem
    out_filename = f"{stem}.unsigned.pdf"
    out_dir = Path(request.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / out_filename

    removed_count = 0
    try:
        with open(in_path, "rb") as inf:
            reader = PdfFileReader(inf, strict=False)
            pdf_writer = copy_into_new_writer(reader)

            root = pdf_writer.root
            acro_form = root.get("/AcroForm")

            if acro_form is not None:
                form_obj = acro_form.get_object() if hasattr(acro_form, "get_object") else acro_form
                fields = form_obj.get("/Fields")
                if fields is not None:
                    fields_obj = fields.get_object() if hasattr(fields, "get_object") else fields
                    new_fields = generic.ArrayObject()
                    for f_ref in fields_obj:
                        field_obj = f_ref.get_object() if hasattr(f_ref, "get_object") else f_ref
                        field_name = str(field_obj.get("/T", ""))
                        ft = field_obj.get("/FT")
                        is_sig = (ft == generic.pdf_name("/Sig"))
                        if is_sig and (target_field is None or field_name == target_field):
                            removed_count += 1
                        else:
                            new_fields.append(f_ref)

                    if len(new_fields) > 0:
                        form_obj[generic.pdf_name("/Fields")] = new_fields
                    else:
                        if generic.pdf_name("/AcroForm") in root:
                            del root[generic.pdf_name("/AcroForm")]

            page_count = int(reader.root["/Pages"]["/Count"])
            for page_ix in range(page_count):
                p_ref, _ = pdf_writer.find_page_for_modification(page_ix)
                page_obj = p_ref.get_object() if hasattr(p_ref, "get_object") else p_ref
                annots = page_obj.get("/Annots")
                if annots is not None:
                    annots_obj = annots.get_object() if hasattr(annots, "get_object") else annots
                    new_annots = generic.ArrayObject()
                    for annot_ref in annots_obj:
                        annot_obj = annot_ref.get_object() if hasattr(annot_ref, "get_object") else annot_ref
                        if annot_obj.get("/Subtype") == generic.pdf_name("/Widget") and annot_obj.get("/FT") == generic.pdf_name("/Sig"):
                            name = str(annot_obj.get("/T", ""))
                            if target_field is None or name == target_field:
                                continue
                        new_annots.append(annot_ref)
                    page_obj[generic.pdf_name("/Annots")] = new_annots

            # Full rewrite (drops prior revisions and garbage collects)
            with open(out_path, "wb") as outf:
                pdf_writer.write(outf)
    except Exception as exc:
        raise EngineError("engine_failure", f"signature removal failed: {exc}") from exc

    writer.progress("pdf.sign_remove", 1, 1)
    writer.artifact(str(out_path), "pdf")
    writer.ok({
        "removed_signatures_count": removed_count,
        "invalidated": True,
    })
