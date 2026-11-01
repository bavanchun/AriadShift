"""Two-run convergence and idempotence tests for EPUB fixture generation."""

from __future__ import annotations

import io
from pathlib import Path
import shutil
import tempfile
import unittest
import zipfile

from ariad_fixture_gen import epub, markdown, truth


def _run_generation_pass(root: Path) -> dict[str, bytes]:
    outputs = {
        path: content
        for generator in (markdown.generate, epub.generate, truth.generate)
        for path, content in generator(root).items()
    }
    for relative_path, content in sorted(outputs.items()):
        output_path = root / relative_path
        output_path.parent.mkdir(parents=True, exist_ok=True)
        if not output_path.exists() or output_path.read_bytes() != content:
            output_path.write_bytes(content)
    return outputs


class TestEpubIdempotence(unittest.TestCase):
    def test_two_runs_into_temporary_root_produce_identical_epub_bytes(self) -> None:
        repo_root = Path(__file__).resolve().parents[3]
        source_text_dir = repo_root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text"

        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_root = Path(tmp_dir)
            target_text_dir = tmp_root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text"
            shutil.copytree(source_text_dir, target_text_dir)

            # Seed a stale route image on disk to verify the first run uses freshly
            # generated route image bytes rather than reading stale disk state.
            stale_route = tmp_root / "fixtures" / "md" / "assets" / "vietnamese-route.png"
            stale_route.parent.mkdir(parents=True, exist_ok=True)
            stale_route.write_bytes(b"stale-route-image-bytes")

            first_run = _run_generation_pass(tmp_root)
            second_run = _run_generation_pass(tmp_root)

            epub_paths = [path for path in first_run if path.startswith("fixtures/epub/")]
            self.assertTrue(epub_paths, "Expected generated EPUB fixture outputs")

            for path in epub_paths:
                self.assertEqual(
                    first_run[path],
                    second_run[path],
                    f"EPUB output {path} did not converge on the first run",
                )

            with zipfile.ZipFile(io.BytesIO(first_run["fixtures/epub/vi-epub-table-image.epub"])) as zf:
                embedded_png = zf.read("EPUB/images/route.png")
            self.assertEqual(
                embedded_png,
                first_run["fixtures/md/assets/vietnamese-route.png"],
            )


if __name__ == "__main__":
    unittest.main()
