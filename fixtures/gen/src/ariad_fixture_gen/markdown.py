"""Generate Markdown fixtures that exercise CommonMark and GFM behavior."""

from __future__ import annotations

import io
import unicodedata
from pathlib import Path

from PIL import Image, ImageDraw


def _source(root: Path, filename: str) -> str:
    return (root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text" / filename).read_text(
        encoding="utf-8"
    ).strip()


def _route_image() -> bytes:
    image = Image.new("RGB", (720, 300), "#f5f1e8")
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, 719, 299), outline="#385a64", width=4)
    draw.rectangle((0, 180, 719, 299), fill="#c8e0e2")
    route_points = [(40, 130), (160, 92), (265, 130), (380, 76), (520, 112), (665, 50)]
    draw.line(route_points, fill="#b65334", width=8)
    for x, y in route_points:
        draw.ellipse((x - 10, y - 10, x + 10, y + 10), fill="#f5f1e8", outline="#385a64", width=4)
    draw.rectangle((112, 151, 142, 180), fill="#6e8a58")
    draw.rectangle((456, 145, 490, 180), fill="#6e8a58")
    buffer = io.BytesIO()
    image.save(buffer, format="PNG", compress_level=9, optimize=False)
    return buffer.getvalue()


def generate(root: Path) -> dict[str, bytes]:
    river = _source(root, "vietnamese-river.md")
    garden = _source(root, "vietnamese-garden.md")
    map_room = _source(root, "english-map-room.md")
    repair_cafe = _source(root, "english-repair-cafe.md")

    entries: dict[str, str] = {
        "fixtures/md/vi-long-headings.md": "\n\n".join(
            f"## Chặng {index:03d}\n\nDấu mốc {index} giúp người đọc theo dõi tuyến đường."
            for index in range(1, 225)
        ),
        "fixtures/md/en-deep-list.md": "\n".join(f'{"  " * level}- Layer {level + 1}' for level in range(64)),
        "fixtures/md/vi-ordered-start.md": "# Trình tự kiểm tra bản đồ\n\n7. Ghi ngày khảo sát\n8. Đối chiếu tên đường\n9. Đánh dấu chỗ chưa chắc chắn",
        "fixtures/md/en-task-list.md": "# Before the Repair Café Opens\n\n- [x] Set out the work mats\n- [ ] Test the lamps\n- [ ] Leave a clear path to the door",
        "fixtures/md/vi-aligned-table.md": "# Bảng hành trình\n\n| Điểm dừng | Phút đi bộ | Ghi chú |\n|:----------|----------:|:-------|\n| Bến sông | 8 | Bóng mát |\n| Vườn sách | 14 | Có ghế dài |",
        "fixtures/md/vi-tone-dense-table.md": "# Bảng chữ có nhiều dấu\n\n| Địa điểm | Đường vòng | Ý kiến |\n|----------|------------|---------|\n| Phố cổ | Quãng ngắn | Cần rẽ sớm |\n| Bến chợ | Đường rộng | Dễ nhận ra |",
        "fixtures/md/en-footnotes.md": "# Notes from the Archive\n\nThe map was redrawn after the flood.[^date] A pencil mark near the bridge was kept.[^mark]\n\n[^date]: The river reached the lower stair in late May.\n[^mark]: The mark was not present on the earlier copy.",
        "fixtures/md/mixed-math.md": "# Measuring a Shared Path\n\nThe walking estimate is $d = vt$. Với từng đoạn đường, ta có thể tính tổng như sau:\n\n$$\nD = \\sum_{i=1}^{n} d_i\n$$\n\nA slower pace changes the total time, not the map scale.",
        "fixtures/md/en-fenced-code.md": "# A Small Distance Check\n\n```python\nsegments = [120, 85, 210]\nprint(sum(segments))\n```\n\nThe values are metres, not minutes.",
        "fixtures/md/vi-blockquote.md": f"# Lời nhắn bên cầu\n\n> {river.splitlines()[0]}\n>\n> Hãy để lối đi thông thoáng cho người qua đường.",
        "fixtures/md/vi-links-autolinks.md": "# Ghi chú nguồn bản đồ\n\nBản ghi mở tại <https://example.org/river-survey>. Tên tuyến được đối chiếu với [sổ địa danh](https://example.org/place-names).",
        "fixtures/md/en-emphasis-strike.md": "# A Reminder\n\n*Record* the bridge before the route **turns east**. ~~Ignore the old water mark~~; it still helps.",
        "fixtures/md/en-hard-breaks.md": "# Two Lines on the Noticeboard\n\nThe west door opens at nine.  \nBring a clean cloth for the workbench.",
        "fixtures/md/vi-raw-html.md": '# Bảng chỉ dẫn\n\n<span lang="vi">Lối xuống bến ở phía bên trái.</span>\n\n<div data-route="river">Đoạn đường này đang được sửa.</div>',
        "fixtures/md/vi-local-image.md": "# Sơ đồ lối ven sông\n\n" + garden + "\n\n![Sơ đồ tuyến đi bộ](assets/vietnamese-route.png \"Đường ven sông và các điểm dừng\")",
        "fixtures/md/en-emoji-shortcodes.md": "# A Small Opening Day\n\nThe doors are open :smile: and the first repaired lamp is ready :tada:.",
        "fixtures/md/vi-front-matter.md": "---\ntitle: Vườn đọc bên hiên\nauthor:\n  - Nhóm thư viện\nlang: vi\ndate: 2026-10-06\n---\n\n" + garden,
        "fixtures/md/vi-nfc.md": "# Ánh đèn cuối ngõ\n\n" + river.splitlines()[0] + "\n\nCộng đồng giữ lại chiếc ghế gỗ để mọi người nghỉ chân.",
        "fixtures/md/vi-nfd.md": "# Ánh đèn cuối ngõ\n\n" + river.splitlines()[0] + "\n\nCộng đồng giữ lại chiếc ghế gỗ để mọi người nghỉ chân.",
        "fixtures/md/en-lf.md": "# A Place to Repair\n\n" + repair_cafe.splitlines()[0] + "\n\nTools return to their shelves before closing.",
        "fixtures/md/en-crlf.md": "# A Place to Repair\n\n" + repair_cafe.splitlines()[0] + "\n\nTools return to their shelves before closing.",
        "fixtures/md/vi-kitchen-sink.md": "# Vườn mở cửa\n\n" + garden + "\n\n| Việc | Trạng thái |\n|---|---|\n| Tưới cây | Đã xong |\n| Mở cổng | Đang chờ |\n\n- [x] Quét lối đi\n- [ ] Đặt thêm ghế\n\nXem [lịch sinh hoạt](https://example.org/garden).",
        "fixtures/md/en-kitchen-sink.md": "# The Map Room\n\n" + map_room + "\n\n> Keep a note of what the map cannot show.\n\n```text\ncopy → compare → annotate\n```\n\nA route index lives at <https://example.org/index>.",
        "fixtures/md/mixed-kitchen-sink.md": "---\ntitle: A Shared Route\nlang: vi\n---\n\n# Một lối đi, two readers\n\n" + river.splitlines()[0] + "\n\n| Điểm dừng / Stop | Minute |\n|---|---:|\n| Bến sông | 8 |\n| Reading garden | 14 |\n\nThe combined distance is $d_1 + d_2$. **Check the bridge** before leaving.\n\n- [x] Save the route\n- [ ] Share it with a neighbor",
    }

    if len(entries) != 24:
        raise AssertionError(f"Expected 24 generated Markdown entries, found {len(entries)}")

    normalized_pair = entries["fixtures/md/vi-nfc.md"]
    if unicodedata.normalize("NFC", normalized_pair) != normalized_pair:
        raise AssertionError("The Vietnamese source used for the NFC fixture is not NFC-normalized")
    entries["fixtures/md/vi-nfd.md"] = unicodedata.normalize("NFD", normalized_pair)
    if not any(unicodedata.combining(character) for character in entries["fixtures/md/vi-nfd.md"]):
        raise AssertionError("The NFD fixture contains no combining marks")

    crlf_twin = entries["fixtures/md/en-crlf.md"]
    if entries["fixtures/md/en-lf.md"] != crlf_twin:
        raise AssertionError("The LF and CRLF Markdown fixtures must have identical text")

    outputs = {path: content.replace("\r\n", "\n").encode("utf-8") for path, content in entries.items()}
    outputs["fixtures/md/en-crlf.md"] = crlf_twin.replace("\n", "\r\n").encode("utf-8")
    outputs["fixtures/md/assets/vietnamese-route.png"] = _route_image()
    return outputs
