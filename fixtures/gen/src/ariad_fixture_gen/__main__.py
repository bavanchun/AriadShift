"""Command-line entry point for regenerating the fixture suite."""

from __future__ import annotations

import argparse
from pathlib import Path

from ariad_fixture_gen import docx, html, markdown
from ariad_fixture_gen.manifest import update_generated_hashes


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", choices=("md", "html", "docx"), default="md")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[4]
    outputs = {"md": markdown.generate, "html": html.generate, "docx": docx.generate}[args.only](root)
    for relative_path, content in sorted(outputs.items()):
        output_path = root / relative_path
        output_path.parent.mkdir(parents=True, exist_ok=True)
        if not output_path.exists() or output_path.read_bytes() != content:
            output_path.write_bytes(content)

    update_generated_hashes(root, set(outputs))
    print(f"Generated {len(outputs)} files for {args.only} fixtures and updated their manifest hashes.")


if __name__ == "__main__":
    main()
