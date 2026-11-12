"""Portable raster masks, fast marching interpolation and PNG previews."""

from __future__ import annotations

import heapq
import math
import struct
import zlib
from collections import deque
from dataclasses import dataclass
from pathlib import Path

from ariad_inpaint.protocol import EngineError


@dataclass
class Image:
    width: int
    height: int
    rgb: bytearray

    def copy(self) -> Image:
        return Image(self.width, self.height, bytearray(self.rgb))


def png(path: Path, image: Image) -> None:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    rows = b"".join(
        b"\0" + image.rgb[y * image.width * 3 : (y + 1) * image.width * 3]
        for y in range(image.height)
    )
    header = struct.pack(">IIBBBBB", image.width, image.height, 8, 2, 0, 0, 0)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )


def detect(image: Image) -> tuple[bytearray, int]:
    """Locate neutral, low-contrast raster marks while preserving dark text pixels."""
    size = image.width * image.height
    candidates = bytearray(size)
    for index in range(size):
        r, g, b = image.rgb[index * 3 : index * 3 + 3]
        gray = (r + g + b) / 3
        if 110 <= gray <= 242 and max(r, g, b) - min(r, g, b) <= 22:
            candidates[index] = 1
    visited = bytearray(size)
    mask = bytearray(size)
    regions = 0
    for seed, candidate in enumerate(candidates):
        if not candidate or visited[seed]:
            continue
        todo = deque([seed])
        visited[seed] = 1
        component = []
        boundary = []
        while todo:
            index = todo.popleft()
            component.append(index)
            x, y = index % image.width, index // image.width
            for dx, dy in ((-1, 0), (1, 0), (0, -1), (0, 1)):
                nx, ny = x + dx, y + dy
                if not (0 <= nx < image.width and 0 <= ny < image.height):
                    continue
                neighbor = ny * image.width + nx
                if candidates[neighbor] and not visited[neighbor]:
                    visited[neighbor] = 1
                    todo.append(neighbor)
                elif not candidates[neighbor]:
                    boundary.append(sum(image.rgb[neighbor * 3 : neighbor * 3 + 3]) / 3)
        # Do not interpret full-page gray paper/background as a watermark.
        if (
            len(component) >= 4
            and len(component) < size * 0.5
            and boundary
            and sum(boundary) / len(boundary) > 245
        ):
            regions += 1
            for index in component:
                mask[index] = 1
    return mask, regions


def neighbors(index: int, width: int, height: int) -> list[int]:
    x, y = index % width, index // width
    return [
        ny * width + nx
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1))
        if 0 <= nx < width and 0 <= ny < height
    ]


def inpaint(
    image: Image, mask: bytearray, method: str, radius: float, *, backend: str = "auto"
) -> tuple[Image, str]:
    if not any(mask):
        return image.copy(), "none"
    if all(mask):
        raise EngineError("invalid_request", "mask has no unmasked background to interpolate")
    if backend != "python":
        try:
            import cv2
            import numpy as np
        except ImportError:
            if backend == "opencv":
                raise EngineError("tool_missing", "local OpenCV is not installed") from None
        else:
            array = np.frombuffer(image.rgb, dtype=np.uint8).reshape(image.height, image.width, 3)
            region = np.frombuffer(mask, dtype=np.uint8).reshape(image.height, image.width) * 255
            flag = cv2.INPAINT_TELEA if method == "telea" else cv2.INPAINT_NS
            result = cv2.inpaint(array, region, radius, flag)
            return Image(image.width, image.height, bytearray(result.tobytes())), "opencv"
    if sum(mask) > 2_000_000:
        raise EngineError("limit_exceeded", "pure Python inpainting mask exceeds 2 million pixels")
    result = image.copy()
    width, height = image.width, image.height
    known = bytearray(1 - value for value in mask)
    queued = bytearray(len(mask))
    heap: list[tuple[float, int]] = []
    for index, value in enumerate(mask):
        if value and any(known[n] for n in neighbors(index, width, height)):
            heapq.heappush(heap, (1.0, index))
            queued[index] = 1
    reach = math.ceil(radius)
    offsets = [
        (dx, dy, 1 / (dx * dx + dy * dy))
        for dy in range(-reach, reach + 1)
        for dx in range(-reach, reach + 1)
        if 0 < dx * dx + dy * dy <= radius * radius
    ]
    while heap:
        distance, index = heapq.heappop(heap)
        x, y = index % width, index // width
        weights = 0.0
        values = [0.0, 0.0, 0.0]
        for dx, dy, weight in offsets:
            nx, ny = x + dx, y + dy
            if 0 <= nx < width and 0 <= ny < height and known[ny * width + nx]:
                neighbor = ny * width + nx
                weights += weight
                for channel in range(3):
                    values[channel] += result.rgb[neighbor * 3 + channel] * weight
        if not weights:
            raise EngineError("engine_failure", "inpainting front lost its known boundary")
        for channel in range(3):
            result.rgb[index * 3 + channel] = round(values[channel] / weights)
        known[index] = 1
        for neighbor in neighbors(index, width, height):
            if not known[neighbor] and not queued[neighbor]:
                queued[neighbor] = 1
                heapq.heappush(heap, (distance + 1, neighbor))
    if method == "ns":
        # Relax the biharmonic equation inside the mask, preserving the exact boundary.
        # The fast-marched image supplies stable initial values and Dirichlet data.
        indices = [i for i, value in enumerate(mask) if value]
        for _ in range(30):
            previous = bytearray(result.rgb)
            largest = 0
            for index in indices:
                x, y = index % width, index // width
                if not 2 <= x < width - 2 or not 2 <= y < height - 2:
                    continue
                axial = [index - 1, index + 1, index - width, index + width]
                diagonal = [
                    index - width - 1,
                    index - width + 1,
                    index + width - 1,
                    index + width + 1,
                ]
                second = [index - 2, index + 2, index - 2 * width, index + 2 * width]
                for channel in range(3):
                    value = round(
                        (
                            8 * sum(previous[n * 3 + channel] for n in axial)
                            - 2 * sum(previous[n * 3 + channel] for n in diagonal)
                            - sum(previous[n * 3 + channel] for n in second)
                        )
                        / 20
                    )
                    value = max(0, min(255, value))
                    largest = max(largest, abs(value - previous[index * 3 + channel]))
                    result.rgb[index * 3 + channel] = value
            if largest == 0:
                break
    return result, "python_fast_marching" if method == "telea" else "python_biharmonic"
