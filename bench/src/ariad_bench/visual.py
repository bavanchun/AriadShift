"""Page-aligned SSIM over PDFium grayscale renders, with missing pages scoring zero."""

from contextlib import closing
from pathlib import Path

import numpy as np
import pypdfium2 as pdfium


def raster_pages(path: Path) -> list[np.ndarray]:
    with pdfium.PdfDocument(path) as document:
        pages = []
        for index in range(len(document)):
            with closing(document[index]) as page:
                bitmap = page.render(scale=96 / 72)
                try:
                    pages.append(np.asarray(bitmap.to_pil().convert("L")).copy())
                finally:
                    bitmap.close()
        return pages


def page_ssim(reference: np.ndarray, hypothesis: np.ndarray) -> float:
    """Mean local SSIM on non-overlapping 8×8 windows; never stretch page geometry."""
    height = (max(reference.shape[0], hypothesis.shape[0]) + 7) // 8 * 8
    width = (max(reference.shape[1], hypothesis.shape[1]) + 7) // 8 * 8

    def windows(image: np.ndarray) -> np.ndarray:
        canvas = np.full((height, width), 255.0)
        canvas[: image.shape[0], : image.shape[1]] = image
        return (
            canvas.reshape(height // 8, 8, width // 8, 8)
            .transpose(0, 2, 1, 3)
            .reshape(-1, 64)
        )

    left, right = windows(reference), windows(hypothesis)
    mean_left, mean_right = left.mean(axis=1), right.mean(axis=1)
    centered_left = left - mean_left[:, None]
    centered_right = right - mean_right[:, None]
    var_left = (centered_left**2).mean(axis=1)
    var_right = (centered_right**2).mean(axis=1)
    covariance = (centered_left * centered_right).mean(axis=1)
    score = (
        (2 * mean_left * mean_right + 6.5025)
        * (2 * covariance + 58.5225)
        / ((mean_left**2 + mean_right**2 + 6.5025) * (var_left + var_right + 58.5225))
    )
    return float(np.clip(score.mean(), 0, 1))


def pdf_ssim(reference: Path, hypothesis: Path) -> float:
    left, right = raster_pages(reference), raster_pages(hypothesis)
    pages = max(len(left), len(right))
    if not pages:
        raise ValueError("SSIM requires at least one page")
    return round(sum(page_ssim(a, b) for a, b in zip(left, right)) / pages, 3)
