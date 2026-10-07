"""Unit tests for benchmark reference resolution and truth companion independence."""

from __future__ import annotations

import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from ariad_bench.canonical import CanonicalDoc
from ariad_bench.references import get_reference_for_fixture
from ariad_bench.run import find_binary


class TestReferences(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = Path(__file__).resolve().parents[2]
        cls.pandoc_bin = find_binary(
            "pandoc",
            os.environ.get("ASHIFT_PANDOC"),
            [cls.root / ".tools" / "pandoc" / "bin" / "pandoc"],
        )

    def test_resolve_authored_truth_companion(self) -> None:
        fixture = {
            "id": "vi-garden-notice",
            "format": "html",
            "origin": "generated",
            "path": "fixtures/html/vi-garden-notice.html",
            "companions": [
                {
                    "path": "fixtures/html/vi-garden-notice.truth.json",
                    "sha256": "dummy",
                }
            ],
        }
        ref = get_reference_for_fixture(fixture, self.root)
        self.assertIsNotNone(ref)
        assert ref is not None
        self.assertIn("Lịch chăm vườn đọc", ref.text)
        self.assertEqual(len(ref.headings), 1)
        self.assertEqual(ref.headings[0].text, "Lịch chăm vườn đọc")

    def test_resolve_markdown_source_via_pandoc(self) -> None:
        fixture = {
            "id": "vi-kitchen-sink",
            "format": "md",
            "origin": "generated",
            "path": "fixtures/md/vi-kitchen-sink.md",
            "companions": [],
        }
        ref = get_reference_for_fixture(fixture, self.root, pandoc_bin=self.pandoc_bin)
        self.assertIsNotNone(ref)
        assert ref is not None
        self.assertTrue(len(ref.text) > 0)
        self.assertTrue(len(ref.headings) > 0)

    def test_unscored_public_fixture_returns_none(self) -> None:
        fixture = {
            "id": "vn-tt-28-2012-conformity",
            "format": "docx",
            "origin": "public",
            "path": "fixtures/docx/vn-tt-28-2012-conformity.docx",
            "companions": [],
        }
        ref = get_reference_for_fixture(fixture, self.root)
        self.assertIsNone(ref)

    def test_unscored_companion_returns_none(self) -> None:
        """Synthetic merged-table edge case has an unscored truth companion and must return None."""
        fixture = {
            "id": "synthetic-merged-table",
            "format": "docx",
            "origin": "generated",
            "path": "fixtures/docx/edge-case-merged-table.docx",
            "companions": [
                {
                    "path": "fixtures/docx/synthetic-merged-table.truth.json",
                    "sha256": "dummy",
                }
            ],
        }
        ref = get_reference_for_fixture(fixture, self.root)
        self.assertIsNone(ref)

    def test_all_truth_files_conform_to_canonical_schema(self) -> None:
        """Verify that every .truth.json file in fixtures/ loads cleanly into CanonicalDoc."""
        truth_files = list(self.root.glob("fixtures/**/*.truth.json"))
        self.assertGreaterEqual(len(truth_files), 14, "Expected at least 14 truth companions")

        scored_count = 0
        unscored_count = 0

        for tf in truth_files:
            data = json.loads(tf.read_text(encoding="utf-8"))
            self.assertEqual(data.get("version"), "ariad-truth/0", f"{tf} invalid version")
            self.assertTrue("id" in data, f"{tf} missing id")

            if data.get("unscored") is True:
                unscored_count += 1
                self.assertIn("unscored_reason", data, f"{tf} unscored companion must explain reason")
            else:
                scored_count += 1
                self.assertIn("canonical", data, f"{tf} missing canonical field")
                doc = CanonicalDoc.from_dict(data["canonical"])
                self.assertIsInstance(doc.text, str)
                self.assertTrue(len(doc.text) > 0, f"{tf} canonical text should not be empty")

        self.assertEqual(scored_count, 13)
        self.assertEqual(unscored_count, 1)

    def test_truth_generation_independent_of_fixtures_golden(self) -> None:
        """Verify truth.generate works completely without fixtures/golden existing."""
        from ariad_fixture_gen import truth

        with tempfile.TemporaryDirectory(prefix="ashift-no-golden-") as tmp_dir:
            tmp_root = Path(tmp_dir)
            # Copy fixtures/gen source files to tmp_root
            tmp_gen_text = tmp_root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text"
            tmp_gen_text.mkdir(parents=True, exist_ok=True)
            real_gen_text = self.root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text"
            for f in real_gen_text.glob("*.md"):
                (tmp_gen_text / f.name).write_text(f.read_text(encoding="utf-8"), encoding="utf-8")

            # Note: tmp_root/fixtures/golden does NOT exist
            self.assertFalse((tmp_root / "fixtures" / "golden").exists())

            # Generate truth from tmp_root
            results = truth.generate(tmp_root)
            self.assertEqual(len(results), 14)
            for path_str, content_bytes in results.items():
                self.assertTrue(len(content_bytes) > 0)
                parsed = json.loads(content_bytes.decode("utf-8"))
                self.assertEqual(parsed.get("version"), "ariad-truth/0")

    def test_mutating_generator_data_changes_truth(self) -> None:
        """Mutating generator input data changes the truth companion output."""
        from ariad_fixture_gen import html

        # Baseline
        base_truth = html.generate_truth(self.root)
        notice_base = base_truth["fixtures/html/vi-garden-notice.truth.json"]

        # Mutate the source text selectively
        orig_source = html._source

        def fake_source(root: Path, filename: str) -> list[str]:
            if filename == "vietnamese-garden.md":
                return ["Đoạn văn thử nghiệm đột biến 1", "Đoạn văn thử nghiệm đột biến 2"]
            return orig_source(root, filename)

        with patch.object(html, "_source", side_effect=fake_source):
            mutated_truth = html.generate_truth(self.root)
            notice_mutated = mutated_truth["fixtures/html/vi-garden-notice.truth.json"]

            self.assertNotEqual(
                notice_base["canonical"]["text"],
                notice_mutated["canonical"]["text"],
            )
            self.assertIn("thử nghiệm đột biến", notice_mutated["canonical"]["text"])

    def test_markdown_reference_uses_gfm_reader(self) -> None:
        """Verify markdown reference uses Pandoc's GFM reader (-f gfm)."""
        fixture = {
            "id": "vi-kitchen-sink",
            "format": "md",
            "origin": "generated",
            "path": "fixtures/md/vi-kitchen-sink.md",
            "companions": [],
        }
        with patch("subprocess.run") as mock_run:
            mock_run.return_value.returncode = 0
            mock_run.return_value.stdout = json.dumps({
                "pandoc-api-version": [1, 23, 1, 2],
                "meta": {},
                "blocks": [],
            })
            get_reference_for_fixture(fixture, self.root, pandoc_bin="dummy_pandoc")
            self.assertTrue(mock_run.called)
            cmd_args = mock_run.call_args[0][0]
            self.assertEqual(cmd_args[1], "-f")
            self.assertEqual(cmd_args[2], "gfm")
            self.assertEqual(cmd_args[3], "-t")
            self.assertEqual(cmd_args[4], "json")


if __name__ == "__main__":
    unittest.main()
