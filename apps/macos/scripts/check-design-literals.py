#!/usr/bin/env python3
"""Fail on literal design values in the macOS app outside the AriadDesignSystem package.

Spacing, padding, frame sizes, colors and fonts in apps/macos/AriadShift must come from the design tokens
(DSSpacing, DSSize, DSColor, DSFont, ...). Zero is allowed: it is the absence of a value, not a design choice.

Existing violations live in design-literals-baseline.txt, one line per occurrence (path, rule, code). The check
fails when a violation is not in the baseline, and also when a baseline entry no longer exists, so the baseline
only ever shrinks. After fixing violations run with --update-baseline and commit the smaller file.

Usage: check-design-literals.py [--update-baseline] [--root DIR] [--baseline FILE]
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

MACOS = Path(__file__).resolve().parent.parent
DEFAULT_ROOT = MACOS / "AriadShift"
DEFAULT_BASELINE = Path(__file__).resolve().parent / "design-literals-baseline.txt"

NUMBER = re.compile(r"(?<![\w.])-?(\d+(?:\.\d+)?)(?![\w.])")
CHROMATIC = r"(?:red|orange|yellow|green|mint|teal|cyan|blue|indigo|purple|pink|brown|gray|grey|black|white)"

# Calls whose numeric arguments are design literals.
NUMERIC_CALLS = {
    "padding": re.compile(r"\.padding\s*\("),
    "frame": re.compile(r"\.frame\s*\("),
}
# Patterns that are violations wherever they appear.
PATTERNS = {
    "spacing": re.compile(r"\bspacing\s*:\s*-?(?:[1-9]\d*(?:\.\d+)?|0\.\d*[1-9]\d*)\b"),
    "color": re.compile(
        r"\bColor\s*\(\s*(?:red|white|hue|\.sRGB|\.displayP3|\"|#colorLiteral)"
        r"|\bNSColor\s*\(\s*(?:red|white|calibrated|srgb|device|displayP3|name)"
        rf"|\b(?:Color|NSColor)\s*\.\s*(?:system[A-Z]\w*|{CHROMATIC})\b"
        rf"|\.(?:foregroundStyle|foregroundColor|tint|fill|stroke|background|border|shadow)\s*\(\s*(?:color\s*:\s*)?\.{CHROMATIC}\b"
    ),
    "font": re.compile(
        r"\.system\s*\(\s*size\s*:"
        r"|(?:\bFont\s*)?\.custom\s*\("
        r"|\bNSFont\s*(?:\.\s*\w*[sS]ystemFont\s*\(\s*ofSize|\(\s*name)"
    ),
}


def strip_code(line: str, in_block_comment: bool = False) -> tuple[str, bool]:
    """Removes string literal contents and comments so only code is scanned.

    Returns the code and whether a /* block comment is still open at the end of the line.
    """
    out = []
    in_string = False
    i = 0
    while i < len(line):
        if in_block_comment:
            end = line.find("*/", i)
            if end < 0:
                break
            in_block_comment = False
            i = end + 2
            continue
        ch = line[i]
        if in_string:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_string = False
                out.append('"')
        elif ch == '"':
            in_string = True
            out.append('"')
        elif line.startswith("//", i):
            break
        elif line.startswith("/*", i):
            in_block_comment = True
            i += 2
            continue
        else:
            out.append(ch)
        i += 1
    return "".join(out), in_block_comment


def call_arguments(code: str, start: int) -> str:
    """The text between the parenthesis that opens at or after `start` and its matching close."""
    open_at = code.index("(", start)
    depth = 0
    for i in range(open_at, len(code)):
        if code[i] == "(":
            depth += 1
        elif code[i] == ")":
            depth -= 1
            if depth == 0:
                return code[open_at + 1 : i]
    return code[open_at + 1 :]


def has_nonzero_number(text: str) -> bool:
    return any(float(match.group(1)) != 0 for match in NUMBER.finditer(text))


def violations_in_line(code: str) -> list[str]:
    rules = []
    for rule, pattern in NUMERIC_CALLS.items():
        for match in pattern.finditer(code):
            if has_nonzero_number(call_arguments(code, match.start())):
                rules.append(rule)
    for rule, pattern in PATTERNS.items():
        rules.extend(rule for _ in pattern.finditer(code))
    return rules


def scan(root: Path, base: Path) -> list[tuple[str, str, str, int]]:
    """Every violation as (relative path, rule, normalized code, line number)."""
    found = []
    for path in sorted(root.rglob("*.swift")):
        relative = path.relative_to(base).as_posix()
        in_block_comment = False
        for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            code, in_block_comment = strip_code(raw, in_block_comment)
            normalized = " ".join(raw.split())
            for rule in violations_in_line(code):
                found.append((relative, rule, normalized, number))
    return found


def key(entry: tuple[str, str, str, int]) -> str:
    return "\t".join(entry[:3])


def read_baseline(path: Path) -> Counter[str]:
    if not path.exists():
        return Counter()
    lines = path.read_text(encoding="utf-8").splitlines()
    return Counter(line for line in lines if line and not line.startswith("#"))


def write_baseline(path: Path, entries: list[tuple[str, str, str, int]]) -> None:
    header = [
        "# Literal design values in apps/macos/AriadShift that predate the design system.",
        "# Generated by apps/macos/scripts/check-design-literals.py --update-baseline. Drain it to zero; never add to it.",
        "# Columns: path<TAB>rule<TAB>code",
    ]
    body = sorted(key(entry) for entry in entries)
    path.write_text("\n".join(header + body) + "\n", encoding="utf-8")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--update-baseline", action="store_true", help="rewrite the baseline from the current code")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT, help="directory of Swift sources to scan")
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE, help="baseline file")
    args = parser.parse_args(argv)

    base = MACOS.parent.parent if args.root == DEFAULT_ROOT else args.root
    entries = scan(args.root, base)
    if args.update_baseline:
        write_baseline(args.baseline, entries)
        print(f"Baseline rewritten with {len(entries)} entries: {args.baseline}")
        return 0

    baseline = read_baseline(args.baseline)
    current = Counter(key(entry) for entry in entries)
    new = current - baseline
    stale = baseline - current

    unreported = Counter(new)
    for entry in entries:
        k = key(entry)
        if unreported[k] > 0:
            unreported[k] -= 1
            print(f"{entry[0]}:{entry[3]}: literal {entry[1]} value; use the AriadDesignSystem tokens: {entry[2]}")
    if stale:
        print("These baseline entries are fixed. Run apps/macos/scripts/check-design-literals.py --update-baseline:")
        for k in sorted(stale.elements()):
            print(f"  {k}")
    if new or stale:
        return 1
    print(f"No new literal design values ({sum(current.values())} baselined).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
