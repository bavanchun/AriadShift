"""Real PDF raster edits and pixel-level verification, without native wheels."""

from __future__ import annotations

import json
import os
import struct
import subprocess
import sys
import zlib
from pathlib import Path

import pytest
from pypdf import PdfReader, PdfWriter
from pypdf.generic import (
    ArrayObject,
    DecodedStreamObject,
    DictionaryObject,
    NameObject,
    NumberObject,
)

from ariad_inpaint.engine import handle
from ariad_inpaint.pdf import decode
from ariad_inpaint.pixels import Image, inpaint
from ariad_inpaint.protocol import RunToolRequest

ROOT = Path(__file__).resolve().parents[1]


class Events:
    def __init__(self):
        self.events = []

    def emit(self, event):
        self.events.append(event)

    def progress(self, *args):
        pass

    def artifact(self, path, fmt):
        self.emit({"type": "artifact", "path": path, "format": fmt})

    def warning(self, code, message):
        self.emit({"type": "warning", "code": code, "message": message})

    def ok(self, data=None):
        self.emit({"type": "result", "ok": True, "data": data})

    def error(self, error):
        self.emit(
            {"type": "result", "ok": False, "error": {"code": error.code, "message": str(error)}}
        )


def source_pdf(path, count=1, *, shared=False, form=False, offset=False):
    writer = PdfWriter()
    width = height = 64
    pixels = bytearray([255]) * (width * height * 3)
    for y in range(25, 36):
        for x in range(20, 44):
            pixels[(y * width + x) * 3 : (y * width + x) * 3 + 3] = bytes([180]) * 3
    # Dark original document content must remain intact in automatic mode.
    for y in range(5, 9):
        for x in range(5, 50):
            pixels[(y * width + x) * 3 : (y * width + x) * 3 + 3] = bytes([20]) * 3
    image = DecodedStreamObject()
    image.update(
        {
            NameObject("/Type"): NameObject("/XObject"),
            NameObject("/Subtype"): NameObject("/Image"),
            NameObject("/Width"): NumberObject(width),
            NameObject("/Height"): NumberObject(height),
            NameObject("/BitsPerComponent"): NumberObject(8),
            NameObject("/ColorSpace"): NameObject("/DeviceRGB"),
        }
    )
    image.set_data(bytes(pixels))
    reference = writer._add_object(image.flate_encode())
    for _ in range(count):
        page = writer.add_blank_page(64 if not offset else 100, 64 if not offset else 100)
        if offset:
            page.cropbox.lower_left = (10, 20)
            page.cropbox.upper_right = (74, 84)
        resources = DictionaryObject(
            {NameObject("/XObject"): DictionaryObject({NameObject("/Image0"): reference})}
        )
        content = b"q 64 0 0 64 0 0 cm /Image0 Do Q"
        if offset:
            content = b"q 64 0 0 64 10 20 cm /Image0 Do Q"
        if form:
            wrapper = DecodedStreamObject()
            wrapper[NameObject("/Type")] = NameObject("/XObject")
            wrapper[NameObject("/Subtype")] = NameObject("/Form")
            wrapper[NameObject("/BBox")] = ArrayObject([NumberObject(v) for v in [0, 0, 64, 64]])
            wrapper[NameObject("/Resources")] = resources
            wrapper.set_data(content)
            resources = DictionaryObject(
                {
                    NameObject("/XObject"): DictionaryObject(
                        {
                            NameObject("/Form0"): writer._add_object(wrapper),
                        }
                    )
                }
            )
            content = b"q /Form0 Do Q"
        page[NameObject("/Resources")] = resources
        stream = DecodedStreamObject()
        stream.set_data(content)
        page[NameObject("/Contents")] = writer._add_object(stream)
        if not shared:
            reference = writer._add_object(image.flate_encode())
    writer.add_metadata({"/Title": "Original generated raster example"})
    writer.write(path)
    return pixels


def run(tmp_path, source, params=None):
    events = Events()
    handle(
        RunToolRequest(
            "test",
            "pdf.watermark_remove_raster",
            [{"path": str(source), "format": "pdf"}],
            str(tmp_path / "out"),
            str(tmp_path / "work"),
            params or {},
            options={"backend": "python"},
        ),
        events,
    )
    return events


def png_pixels(path):
    raw = path.read_bytes()
    assert raw.startswith(b"\x89PNG\r\n\x1a\n")
    position = 8
    compressed = b""
    width = height = 0
    while position < len(raw):
        size = struct.unpack(">I", raw[position : position + 4])[0]
        kind = raw[position + 4 : position + 8]
        data = raw[position + 8 : position + 8 + size]
        assert (
            zlib.crc32(kind + data) & 0xFFFFFFFF
            == struct.unpack(">I", raw[position + 8 + size : position + 12 + size])[0]
        )
        if kind == b"IHDR":
            width, height = struct.unpack(">II", data[:8])
        elif kind == b"IDAT":
            compressed += data
        position += size + 12
    scanlines = zlib.decompress(compressed)
    stride = width * 3
    return (
        width,
        height,
        b"".join(scanlines[y * (stride + 1) + 1 : (y + 1) * (stride + 1)] for y in range(height)),
    )


def first_image(page):
    for reference in page["/Resources"]["/XObject"].values():
        obj = reference.get_object()
        if obj.get("/Subtype") == "/Image":
            return decode(obj)
        if obj.get("/Subtype") == "/Form":
            return first_image(obj)
    raise AssertionError("no image found")


@pytest.mark.parametrize("method", ["telea", "ns"])
@pytest.mark.parametrize(
    "mask", ["auto", [{"page": 1, "x": 20, "y": 25, "width": 24, "height": 11}]]
)
def test_pixel_removal_and_previews(tmp_path, method, mask):
    source = tmp_path / "input.pdf"
    original = source_pdf(source)
    events = run(tmp_path, source, {"mask": mask, "method": method})
    result = events.events[-1]
    assert result["ok"], events.events
    assert result["data"]["pages_inpainted"] == [1]
    assert result["data"]["regions_detected"] == 1
    assert result["data"]["method"] == method
    assert result["data"]["previews"][0]["page"] == 1
    before = Path(result["data"]["previews"][0]["before"])
    after = Path(result["data"]["previews"][0]["after"])
    width, height, pre_pixels = png_pixels(before)
    assert (width, height) == (64, 64) and pre_pixels == original
    _, _, post_pixels = png_pixels(after)
    cleaned = PdfReader(tmp_path / "out/cleaned.pdf")
    actual = first_image(cleaned.pages[0])
    assert actual.rgb == post_pixels
    assert cleaned.metadata.title == "Original generated raster example"
    for y in range(64):
        for x in range(64):
            offset = (y * 64 + x) * 3
            if 20 <= x < 44 and 25 <= y < 36:
                assert min(post_pixels[offset : offset + 3]) > 250
            else:
                assert post_pixels[offset : offset + 3] == original[offset : offset + 3]
    assert source.read_bytes() != (tmp_path / "out/cleaned.pdf").read_bytes()
    assert first_image(PdfReader(source).pages[0]).rgb == original


def test_selected_page_shared_image_and_no_previews(tmp_path):
    source = tmp_path / "input.pdf"
    original = source_pdf(source, count=2, shared=True)
    events = run(tmp_path, source, {"pages": "2", "preview": False})
    assert events.events[-1]["ok"], events.events
    assert events.events[-1]["data"]["pages_inpainted"] == [2]
    assert events.events[-1]["data"]["previews"] == []
    document = PdfReader(tmp_path / "out/cleaned.pdf")
    assert first_image(document.pages[0]).rgb == original
    assert first_image(document.pages[1]).rgb != original
    assert not list((tmp_path / "out").glob("*.png"))


@pytest.mark.parametrize("form,offset", [(True, False), (False, True)])
def test_form_xobject_and_crop_coordinates(tmp_path, form, offset):
    source = tmp_path / "input.pdf"
    source_pdf(source, form=form, offset=offset)
    events = run(tmp_path, source, {"mask": [{"x": 20, "y": 25, "width": 24, "height": 11}]})
    assert events.events[-1]["ok"], events.events
    assert events.events[-1]["data"]["pages_inpainted"] == [1]
    actual = first_image(PdfReader(tmp_path / "out/cleaned.pdf").pages[0])
    assert min(actual.rgb[(30 * 64 + 25) * 3 : (30 * 64 + 25) * 3 + 3]) == 255


@pytest.mark.parametrize(
    "params",
    [
        {"radius": 0},
        {"radius": 26},
        {"radius": float("nan")},
        {"method": "lama"},
        {"pages": "0"},
        {"pages": "2"},
        {"pages": "1,1"},
        {"preview": "true"},
        {"mask": []},
        {"mask": [{"page": 0, "x": 1, "y": 2, "width": 3, "height": 4}]},
        {"mask": [{"x": 1, "y": 2, "width": -3, "height": 4}]},
    ],
)
def test_rejects_invalid_parameters(tmp_path, params):
    source = tmp_path / "input.pdf"
    source_pdf(source)
    events = run(tmp_path, source, params)
    assert events.events[-1]["ok"] is False
    assert events.events[-1]["error"]["code"] == "invalid_request"
    assert not any(e["type"] == "artifact" for e in events.events)


def test_mask_without_boundary_fails():
    with pytest.raises(Exception, match="unmasked background"):
        inpaint(
            Image(2, 2, bytearray([180]) * 12), bytearray([1]) * 4, "telea", 3, backend="python"
        )


def test_describe_and_run_protocol(tmp_path):
    source = tmp_path / "input.pdf"
    source_pdf(source)
    base = {"protocol": "ariad-engine/2", "job": "t"}
    for request in (
        {**base, "op": "describe"},
        {
            **base,
            "op": "run_tool",
            "tool": "pdf.watermark_remove_raster",
            "inputs": [{"path": str(source), "format": "pdf"}],
            "output": {"dir": str(tmp_path / "out")},
            "work_dir": str(tmp_path / "work"),
            "params": {"mask": "auto"},
        },
    ):
        result = subprocess.run(
            [sys.executable, "-m", "ariad_inpaint"],
            input=json.dumps(request).encode() + b"\n",
            capture_output=True,
            env={**os.environ, "PYTHONPATH": str(ROOT / "src")},
            timeout=20,
            check=True,
        )
        events = [json.loads(line) for line in result.stdout.splitlines()]
        assert events[-1]["ok"], events
        if request["op"] == "describe":
            assert events[0]["engine"] == "inpaint" and len(events[0]["tools"]) == 1
        else:
            assert events[-1]["data"]["regions_detected"] == 1


def test_auto_detects_repeated_marks_on_all_pages(tmp_path):
    source = tmp_path / "input.pdf"
    source_pdf(source, count=2, shared=True)
    events = run(tmp_path, source)
    assert events.events[-1]["ok"], events.events
    assert events.events[-1]["data"]["pages_inpainted"] == [1, 2]
    assert events.events[-1]["data"]["regions_detected"] == 2


def test_vector_content_is_preserved_and_preview_is_labelled(tmp_path):
    source = tmp_path / "input.pdf"
    source_pdf(source)
    writer = PdfWriter(clone_from=source)
    stream = DecodedStreamObject()
    original = writer.pages[0].get_contents().get_data()
    stream.set_data(original + b"\n0 0 0 RG 2 w 1 1 m 60 60 l S")
    writer.pages[0][NameObject("/Contents")] = writer._add_object(stream)
    writer.write(source)
    events = run(tmp_path, source)
    assert events.events[-1]["ok"], events.events
    assert any(event.get("code") == "raster_layer_preview" for event in events.events)
    assert b"60 60 l" in PdfReader(tmp_path / "out/cleaned.pdf").pages[0].get_contents().get_data()
