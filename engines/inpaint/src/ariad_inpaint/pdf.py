"""Edit raster XObject instances without flattening or replacing PDF vector content."""

from __future__ import annotations

import io
import math
from typing import Any

from pypdf import PdfReader, PdfWriter
from pypdf.generic import (
    ContentStream,
    DecodedStreamObject,
    DictionaryObject,
    NameObject,
    NumberObject,
)

from ariad_inpaint.pixels import Image, detect, inpaint
from ariad_inpaint.protocol import EngineError

MAX_PIXELS = 16_000_000
IDENTITY = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


def multiply(left: tuple, right: tuple) -> tuple:
    a, b, c, d, e, f = left
    g, h, i, j, k, offset_y = right
    return (
        a * g + c * h,
        b * g + d * h,
        a * i + c * j,
        b * i + d * j,
        a * k + c * offset_y + e,
        b * k + d * offset_y + f,
    )


def decode(stream: Any) -> Image:
    width, height = int(stream["/Width"]), int(stream["/Height"])
    if width < 1 or height < 1 or width * height > MAX_PIXELS:
        raise EngineError("limit_exceeded", "raster dimensions exceed 16 million pixels")
    filters = stream.get("/Filter", [])
    if isinstance(filters, str):
        filters = [filters]
    if "/DCTDecode" in filters or "/JPXDecode" in filters:
        try:
            from PIL import Image as PillowImage
        except ImportError:
            raise EngineError(
                "tool_missing", "JPEG/JPX images require local Pillow or Flate PDF input"
            ) from None
        with PillowImage.open(io.BytesIO(stream.get_data())) as source:
            return Image(width, height, bytearray(source.convert("RGB").tobytes()))
    space = stream.get("/ColorSpace", "/DeviceGray")
    if not isinstance(space, str) or space not in {"/DeviceGray", "/DeviceRGB", "/DeviceCMYK"}:
        raise EngineError("unsupported_route", "raster color space requires an external renderer")
    channels = {"/DeviceGray": 1, "/DeviceRGB": 3, "/DeviceCMYK": 4}[space]
    bits = int(stream.get("/BitsPerComponent", 8))
    data = stream.get_data()
    if bits == 1 and channels == 1:
        stride = (width + 7) // 8
        data = bytes(
            255 if data[y * stride + x // 8] & (128 >> (x % 8)) else 0
            for y in range(height)
            for x in range(width)
        )
    elif bits != 8:
        raise EngineError("unsupported_route", "only 1-bit gray and 8-bit rasters are supported")
    if len(data) != width * height * channels:
        raise EngineError("invalid_request", "raster pixel count does not match dimensions")
    decoded = stream.get("/Decode")
    if decoded:
        if len(decoded) != channels * 2:
            raise EngineError("invalid_request", "invalid raster Decode array")
        data = bytes(
            round(
                max(
                    0,
                    min(
                        1,
                        float(decoded[(i % channels) * 2])
                        + value
                        / 255
                        * (
                            float(decoded[(i % channels) * 2 + 1])
                            - float(decoded[(i % channels) * 2])
                        ),
                    ),
                )
                * 255
            )
            for i, value in enumerate(data)
        )
    if channels == 1:
        rgb = bytearray(value for gray in data for value in (gray, gray, gray))
    elif channels == 4:
        rgb = bytearray()
        for i in range(0, len(data), 4):
            c, m, y, k = data[i : i + 4]
            rgb.extend(round(255 * (1 - color / 255) * (1 - k / 255)) for color in (c, m, y))
    else:
        rgb = bytearray(data)
    return Image(width, height, rgb)


def encode(image: Image, original: Any) -> DecodedStreamObject:
    stream = DecodedStreamObject()
    # Preserve image interpolation and masks; pixel data is now normalized RGB.
    for key in ("/Interpolate", "/SMask", "/Mask", "/Intent"):
        if key in original:
            stream[NameObject(key)] = original[key]
    stream.update(
        {
            NameObject("/Type"): NameObject("/XObject"),
            NameObject("/Subtype"): NameObject("/Image"),
            NameObject("/Width"): NumberObject(image.width),
            NameObject("/Height"): NumberObject(image.height),
            NameObject("/ColorSpace"): NameObject("/DeviceRGB"),
            NameObject("/BitsPerComponent"): NumberObject(8),
        }
    )
    stream.set_data(bytes(image.rgb))
    return stream.flate_encode()


def explicit_mask(
    image: Image, matrix: tuple, rectangles: list[dict[str, Any]], crop: tuple
) -> tuple[bytearray, int]:
    mask = bytearray(image.width * image.height)
    regions = 0
    a, b, c, d, e, f = matrix
    left, _bottom, _right, top = crop
    for rect in rectangles:
        count = 0
        for y in range(image.height):
            v = 1 - (y + 0.5) / image.height
            for x in range(image.width):
                u = (x + 0.5) / image.width
                px, py = a * u + c * v + e - left, top - (b * u + d * v + f)
                if rect["x"] <= px < rect["x"] + rect["width"] and (
                    rect["y"] <= py < rect["y"] + rect["height"]
                ):
                    mask[y * image.width + x] = 1
                    count += 1
        regions += bool(count)
    return mask, regions


def composite(canvas: Image, raster: Image, matrix: tuple, crop: tuple) -> None:
    a, b, c, d, e, f = matrix
    det = a * d - b * c
    if abs(det) < 1e-9:
        return
    left, _bottom, _right, top = crop
    corners = [
        (a * u + c * v + e - left, top - (b * u + d * v + f))
        for u, v in ((0, 0), (1, 0), (0, 1), (1, 1))
    ]
    low_x = max(0, math.floor(min(x for x, _ in corners)))
    high_x = min(canvas.width, math.ceil(max(x for x, _ in corners)))
    low_y = max(0, math.floor(min(y for _, y in corners)))
    high_y = min(canvas.height, math.ceil(max(y for _, y in corners)))
    for y in range(low_y, high_y):
        for x in range(low_x, high_x):
            px, py = x + 0.5 + left - e, top - y - 0.5 - f
            u, v = (d * px - c * py) / det, (-b * px + a * py) / det
            if 0 <= u < 1 and 0 <= v < 1:
                rx, ry = int(u * raster.width), min(raster.height - 1, int((1 - v) * raster.height))
                src, dst = (ry * raster.width + rx) * 3, (y * canvas.width + x) * 3
                canvas.rgb[dst : dst + 3] = raster.rgb[src : src + 3]


def clean_page(
    writer: PdfWriter,
    page: Any,
    rectangles: str | list[dict[str, Any]],
    method: str,
    radius: float,
    backend: str,
) -> tuple[Image, Image, int, str, bool]:
    crop = tuple(map(float, page.cropbox))
    width, height = math.ceil(crop[2] - crop[0]), math.ceil(crop[3] - crop[1])
    if width < 1 or height < 1 or width * height > MAX_PIXELS:
        raise EngineError("limit_exceeded", "page preview exceeds 16 million pixels at 72 dpi")
    if isinstance(rectangles, list):
        for rect in rectangles:
            if rect["x"] + rect["width"] > width or rect["y"] + rect["height"] > height:
                raise EngineError("invalid_request", "mask rectangle is outside the page CropBox")
    before = Image(width, height, bytearray([255]) * (width * height * 3))
    after = before.copy()
    regions = 0
    used_backend = "none"
    raster_count = 0
    has_vector = False

    def process(source: Any, resources: Any, initial: tuple, depth: int = 0) -> tuple[Any, Any]:
        nonlocal regions, used_backend, raster_count, has_vector
        if depth > 32:
            raise EngineError("limit_exceeded", "nested PDF forms exceed 32 levels")
        resources = DictionaryObject(resources.get_object())
        xobjects = DictionaryObject(resources.get("/XObject", DictionaryObject()).get_object())
        resources[NameObject("/XObject")] = xobjects
        content = ContentStream(source, writer)
        matrix = initial
        stack = []
        operations = []
        for operands, operator in content.operations:
            if operator == b"q":
                stack.append(matrix)
            elif operator == b"Q":
                matrix = stack.pop() if stack else initial
            elif operator == b"cm":
                matrix = multiply(matrix, tuple(map(float, operands)))
            elif operator == b"Do":
                name = operands[0]
                obj = xobjects[name].get_object()
                if obj.get("/Subtype") == "/Image":
                    image = decode(obj)
                    raster_count += 1
                    if rectangles == "auto":
                        mask, detected = detect(image)
                    else:
                        mask, detected = explicit_mask(image, matrix, rectangles, crop)
                    cleaned, selected_backend = inpaint(
                        image, mask, method, radius, backend=backend
                    )
                    if any(mask):
                        used_backend = selected_backend
                    regions += detected
                    composite(before, image, matrix, crop)
                    composite(after, cleaned, matrix, crop)
                    new_name = NameObject(f"/AriadInpaint{len(operations)}")
                    while new_name in xobjects:
                        new_name = NameObject(str(new_name) + "_")
                    xobjects[new_name] = writer._add_object(encode(cleaned, obj))
                    operands = [new_name]
                elif obj.get("/Subtype") == "/Form":
                    transformed = multiply(matrix, tuple(map(float, obj.get("/Matrix", IDENTITY))))
                    processed, form_resources = process(
                        obj, obj.get("/Resources", resources), transformed, depth + 1
                    )
                    form = DecodedStreamObject()
                    form.update(
                        {
                            key: value
                            for key, value in obj.items()
                            if key not in {"/Length", "/Filter", "/DecodeParms", "/Resources"}
                        }
                    )
                    form.set_data(processed.get_data())
                    form[NameObject("/Resources")] = form_resources
                    new_name = NameObject(f"/AriadForm{len(operations)}")
                    while new_name in xobjects:
                        new_name = NameObject(str(new_name) + "_")
                    xobjects[new_name] = writer._add_object(form)
                    operands = [new_name]
            elif operator in {b"Tj", b"TJ", b"S", b"f", b"f*", b"B", b"BI"}:
                has_vector = True
            operations.append((operands, operator))
        used_names = {operands[0] for operands, operator in operations if operator == b"Do"}
        resources[NameObject("/XObject")] = DictionaryObject(
            {name: reference for name, reference in xobjects.items() if name in used_names}
        )
        content.operations = operations
        return content, resources

    contents, resources = process(
        page.get_contents(), page.get("/Resources", DictionaryObject()), IDENTITY
    )
    page[NameObject("/Resources")] = resources
    page[NameObject("/Contents")] = writer._add_object(contents)
    if not raster_count:
        raise EngineError("unsupported_route", "page has no supported raster XObjects")
    return before, after, regions, used_backend, has_vector


def open_pdf(path: str, limits: dict[str, Any]) -> PdfWriter:
    reader = PdfReader(path)
    if reader.is_encrypted:
        raise EngineError("encrypted_input", "decrypt the PDF before raster inpainting")
    if limits.get("max_pages") and len(reader.pages) > limits["max_pages"]:
        raise EngineError("limit_exceeded", "PDF exceeds max_pages")
    return PdfWriter(clone_from=reader)
