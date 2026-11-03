"""Generate deterministic scan-tool fixtures from the in-repository garden scan."""
from __future__ import annotations

import hashlib
import json
import random
from io import BytesIO
from pathlib import Path

import pikepdf
from PIL import Image, ImageDraw

from ariad_fixture_gen.pdf_tools import _save_deterministic

ROOT = Path(__file__).resolve().parents[4]
OUT = ROOT / "fixtures" / "pdf-tools"


def image_pdf(image: Image.Image) -> bytes:
    stream = BytesIO()
    image.convert("RGB").save(stream, format="PDF", resolution=150, quality=82)
    return stream.getvalue()


def document(images: list[Image.Image], *, mixed: bool = False) -> bytes:
    pdf = pikepdf.Pdf.new()
    for image in images:
        with pikepdf.open(BytesIO(image_pdf(image))) as source:
            pdf.pages.append(source.pages[0])
    if mixed:
        first = pdf.add_blank_page(page_size=(595, 842))
        pdf.pages.insert(0, first)
        del pdf.pages[-1]
        font = pdf.make_indirect(pikepdf.Dictionary(Type=pikepdf.Name.Font, Subtype=pikepdf.Name.Type1, BaseFont=pikepdf.Name.Helvetica))
        for page, mode in ((pdf.pages[0], 0), (pdf.pages[2], 3)):
            resources = page.obj.get('/Resources', pikepdf.Dictionary())
            fonts = resources.get('/Font', pikepdf.Dictionary())
            fonts['/FScan'] = font
            resources['/Font'] = fonts
            page.obj['/Resources'] = resources
            text = pdf.make_stream(f'BT /FScan 12 Tf {mode} Tr 72 750 Td (Garden scan) Tj ET\n'.encode())
            old = page.obj.get('/Contents')
            page.obj['/Contents'] = pikepdf.Array([old, text]) if old is not None else text
    return _save_deterministic(pdf)


def main() -> None:
    source = Image.open(ROOT / 'fixtures/scan/vi-garden-300dpi.png').convert('L')
    source.thumbnail((1240, 1754), Image.Resampling.LANCZOS)
    rotated = source.transpose(Image.Transpose.ROTATE_270)
    dirty = source.point(lambda x: max(0, min(255, int(x * .85 + 25))))
    draw = ImageDraw.Draw(dirty)
    draw.rectangle((0, 0, dirty.width - 1, dirty.height - 1), outline=0, width=12)
    rng = random.Random(8472)
    for _ in range(3500):
        draw.point((rng.randrange(dirty.width), rng.randrange(dirty.height)), fill=rng.choice((0, 255)))
    bed = Image.new('L', (1500, 1100), 170)
    boxes = []
    for x, y, angle in ((80, 100, -3), (580, 160, 2), (1050, 100, -2)):
        photo = source.copy()
        photo.thumbnail((350, 760), Image.Resampling.LANCZOS)
        photo = photo.rotate(angle, expand=True, fillcolor=170)
        bed.paste(photo, (x, y))
        boxes.append({'bbox_pt': [x * 72 / 150, y * 72 / 150, (x + photo.width) * 72 / 150, (y + photo.height) * 72 / 150], 'angle_deg': angle})
    fixtures = {
        'scan-photos-3': (document([bed]), {'count': 3, 'regions': boxes}),
        'scan-rotated-90-vi': (document([rotated]), {'correction_cw': 270}),
        'scan-mixed-text-vi': (document([source, source], mixed=True), {'pages': [{'visible_text': True, 'invisible_text': False}, {'visible_text': False, 'invisible_text': False}, {'visible_text': False, 'invisible_text': True}]}),
        'scan-dirty-vi': (document([dirty]), {'grey_background': True, 'speckles': True, 'black_border': True}),
    }
    OUT.mkdir(parents=True, exist_ok=True)
    for name, (pdf, truth) in fixtures.items():
        if len(pdf) >= 600_000:
            raise ValueError(f'{name} exceeds 600 KB: {len(pdf)}')
        (OUT / f'{name}.pdf').write_bytes(pdf)
        companion = {
            'version': 'ariad-truth/0',
            'id': name,
            'unscored': True,
            'unscored_reason': 'PDF scan tools test corpus fixture (routes = [])',
            **truth,
        }
        (OUT / f'{name}.truth.json').write_text(json.dumps(companion, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
        print(name, len(pdf), hashlib.sha256(pdf).hexdigest())


if __name__ == '__main__':
    main()
