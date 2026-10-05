"""Generate deterministic degraded raster scans with exact text companions."""

from __future__ import annotations

import random
from io import BytesIO
from pathlib import Path

import typst
from PIL import Image, ImageChops, ImageFilter

_MAX_FILE_BYTES = 2 * 1024 * 1024


def _typst_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _source(root: Path, filename: str) -> list[str]:
    path = root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text" / filename
    return [part.strip() for part in path.read_text(encoding="utf-8").split("\n\n") if part.strip()]


def render_page(*, title: str, paragraphs: list[str], language: str, ppi: int) -> bytes:
    """Render a single A4 page directly to PNG using Typst's bundled fonts."""
    source = [
        f"#set document(title: {_typst_string(title)}, author: \"AriadShift contributors\", date: none)",
        '#set page(paper: "a4", margin: 22mm)',
        f'#set text(font: "Libertinus Serif", size: 11pt, lang: "{language}")',
        "#set par(justify: true, leading: 0.72em)",
        '#align(center)[#text(size: 17pt, weight: "bold")[#text(' + _typst_string(title) + ")]]",
        "#v(5mm)",
    ]
    for paragraph in paragraphs:
        source.extend((f"#text({_typst_string(paragraph)})", "#parbreak()"))

    rendered_page = typst.compile(
        "\n".join(source).encode("utf-8"),
        format="png",
        ppi=ppi,
        timestamp=1_700_000_000,
    )
    with Image.open(BytesIO(rendered_page)) as image:
        if getattr(image, "n_frames", 1) != 1:
            raise ValueError(f"Expected one page for {title!r}, got {image.n_frames}")
    return rendered_page


def _degrade_png(
    source: bytes,
    *,
    seed: int,
    ppi: int,
    angle: float,
    blur_radius: float,
    jpeg_quality: int,
) -> bytes:
    with Image.open(BytesIO(source)) as image_file:
        image = image_file.convert("L")

    image = image.filter(ImageFilter.GaussianBlur(radius=blur_radius))
    noise_bytes = random.Random(seed).randbytes(image.width * image.height)
    noise = Image.frombytes("L", image.size, noise_bytes).point([value // 64 for value in range(256)])
    image = ImageChops.add(image, noise, offset=-2)
    image = image.rotate(angle, resample=Image.Resampling.BICUBIC, fillcolor=255)

    compressed = BytesIO()
    image.save(compressed, format="JPEG", quality=jpeg_quality, optimize=True)
    with Image.open(BytesIO(compressed.getvalue())) as jpeg_image:
        degraded = jpeg_image.convert("L")

    output = BytesIO()
    degraded.save(output, format="PNG", dpi=(ppi, ppi), optimize=True, compress_level=9)
    result = output.getvalue()
    if len(result) > _MAX_FILE_BYTES:
        raise ValueError(f"Generated scan exceeds 2 MiB: {len(result)} bytes")
    return result


def generate(root: Path) -> dict[str, bytes]:
    garden = _source(root, "vietnamese-garden.md")
    river = _source(root, "vietnamese-river.md")
    map_room = _source(root, "english-map-room.md")

    cases = (
        {
            "id": "vi-garden-300dpi",
            "path": "fixtures/scan/vi-garden-300dpi.png",
            "truth": "fixtures/scan/truth/vi-garden-300dpi.txt",
            "title": "Khu vườn đọc sách",
            "paragraphs": garden[:1],
            "language": "vi",
            "ppi": 300,
            "seed": 31001,
            "angle": 0.15,
            "blur": 0.18,
            "jpeg_quality": 91,
        },
        {
            "id": "vi-river-200dpi",
            "path": "fixtures/scan/vi-river-200dpi.png",
            "truth": "fixtures/scan/truth/vi-river-200dpi.txt",
            "title": "Đường ven sông",
            "paragraphs": river[:1],
            "language": "vi",
            "ppi": 200,
            "seed": 21002,
            "angle": -0.4,
            "blur": 0.28,
            "jpeg_quality": 87,
        },
        {
            "id": "en-map-room-150dpi",
            "path": "fixtures/scan/en-map-room-150dpi.png",
            "truth": "fixtures/scan/truth/en-map-room-150dpi.txt",
            "title": "Notes from the Map Room",
            "paragraphs": map_room[:1],
            "language": "en",
            "ppi": 150,
            "seed": 15003,
            "angle": 0.65,
            "blur": 0.38,
            "jpeg_quality": 82,
        },
    )

    outputs: dict[str, bytes] = {}
    for case in cases:
        page = render_page(
            title=case["title"],
            paragraphs=case["paragraphs"],
            language=case["language"],
            ppi=case["ppi"],
        )
        outputs[case["path"]] = _degrade_png(
            page,
            seed=case["seed"],
            ppi=case["ppi"],
            angle=case["angle"],
            blur_radius=case["blur"],
            jpeg_quality=case["jpeg_quality"],
        )
        truth = "\n\n".join((case["title"], *case["paragraphs"])) + "\n"
        outputs[case["truth"]] = truth.encode("utf-8")
    return outputs
