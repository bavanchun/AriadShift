#!/usr/bin/env python3
"""Tests for check-design-literals.py. Run: python3 apps/macos/scripts/test-check-design-literals.py"""

from __future__ import annotations

import importlib.util
import io
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("lint", Path(__file__).with_name("check-design-literals.py"))
lint = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(lint)


class ViolationsInLine(unittest.TestCase):
    def rules(self, line: str) -> list[str]:
        return lint.violations_in_line(lint.strip_code(line)[0])

    def test_numeric_padding_spacing_and_frames(self):
        self.assertEqual(self.rules(".padding(28)"), ["padding"])
        self.assertEqual(self.rules(".padding(.horizontal, 12)"), ["padding"])
        self.assertEqual(self.rules(".padding(-9)"), ["padding"])
        self.assertEqual(self.rules("VStack(alignment: .leading, spacing: 14) {"), ["spacing"])
        self.assertEqual(self.rules(".frame(minWidth: 40, idealWidth: 52, maxWidth: 80)"), ["frame"])
        self.assertEqual(self.rules(".frame(width: thumb * 1.5)"), ["frame"])

    def test_tokens_zero_and_infinity_pass(self):
        for line in [
            ".padding(DSSpacing.l)",
            ".padding(.horizontal, DSSpacing.s)",
            ".padding()",
            "VStack(spacing: 0) {",
            "HStack(spacing: DSSpacing.xs) {",
            ".frame(maxWidth: .infinity, maxHeight: .infinity)",
            ".frame(width: DSSize.inspectorWidth)",
            ".frame(height: 0)",
        ]:
            self.assertEqual(self.rules(line), [], line)

    def test_colors(self):
        for line in [
            "Color(red: 0.2, green: 0.3, blue: 0.4)",
            'Color("Brand")',
            "NSColor(calibratedRed: 1, green: 0, blue: 0, alpha: 1)",
            ".foregroundStyle(.red)",
            ".background(Color.black.opacity(0.1))",
            "let c = NSColor.systemOrange",
        ]:
            self.assertIn("color", self.rules(line), line)
        for line in [".foregroundStyle(.secondary)", ".foregroundStyle(DSColor.accent)", ".tint(DSColor.accent)"]:
            self.assertEqual(self.rules(line), [], line)

    def test_fonts(self):
        for line in [
            ".font(.system(size: 13))",
            '.font(.custom("Inter", size: 14))',
            "NSFont.systemFont(ofSize: 11)",
            "NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .regular)",
        ]:
            self.assertIn("font", self.rules(line), line)
        for line in [".font(DSFont.callout)", ".font(.headline)", ".font(.system(.callout, design: .monospaced))"]:
            self.assertEqual(self.rules(line), [], line)

    def test_strings_and_comments_are_ignored(self):
        self.assertEqual(self.rules('Text("Use .padding(12) here")'), [])
        self.assertEqual(self.rules("// .padding(12) is gone"), [])
        self.assertEqual(self.rules('.help("spacing: 4") // .frame(width: 10)'), [])
        self.assertEqual(self.rules("/* .padding(12) */ .padding(DSSpacing.s)"), [])
        self.assertEqual(self.rules('Text("a/*b").padding(12)'), ["padding"])

    def test_block_comment_spans_lines(self):
        code, open_comment = lint.strip_code("x /* start .padding(12)")
        self.assertTrue(open_comment)
        code, open_comment = lint.strip_code(".frame(width: 10) */ .padding(8)", open_comment)
        self.assertFalse(open_comment)
        self.assertEqual(lint.violations_in_line(code), ["padding"])


class BaselineFlow(unittest.TestCase):
    def run_check(self, root: Path, baseline: Path, *extra: str) -> tuple[int, str]:
        out = io.StringIO()
        with redirect_stdout(out):
            code = lint.main(["--root", str(root), "--baseline", str(baseline), *extra])
        return code, out.getvalue()

    def test_new_violation_fails_baselined_passes_fixed_requires_update(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "src"
            root.mkdir()
            baseline = Path(tmp) / "baseline.txt"
            source = root / "View.swift"
            source.write_text("Text(\"a\")\n    .padding(28)\n")

            code, out = self.run_check(root, baseline)
            self.assertEqual(code, 1)
            self.assertIn("View.swift:2: literal padding", out)

            self.assertEqual(self.run_check(root, baseline, "--update-baseline")[0], 0)
            self.assertEqual(self.run_check(root, baseline)[0], 0)

            # Moving a baselined line does not fail; a second identical literal does.
            source.write_text("\n\nText(\"a\")\n    .padding(28)\n")
            self.assertEqual(self.run_check(root, baseline)[0], 0)
            source.write_text(".padding(28)\n.padding(28)\n")
            self.assertEqual(self.run_check(root, baseline)[0], 1)

            # A fixed violation leaves a stale entry, which fails until the baseline is rewritten.
            source.write_text(".padding(DSSpacing.xl)\n")
            code, out = self.run_check(root, baseline)
            self.assertEqual(code, 1)
            self.assertIn("--update-baseline", out)


if __name__ == "__main__":
    unittest.main()
