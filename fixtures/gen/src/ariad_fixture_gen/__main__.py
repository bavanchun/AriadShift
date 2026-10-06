"""Command-line entry point for regenerating the fixture suite."""

from __future__ import annotations

import argparse
from pathlib import Path

from ariad_fixture_gen import docx, epub, html, image, markdown, pdf, scan
from ariad_fixture_gen.manifest import update_generated_hashes


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    generators = {
        "md": markdown.generate,
        "html": html.generate,
        "docx": docx.generate,
        "epub": epub.generate,
        "pdf": pdf.generate,
        "scan": scan.generate,
        "image": image.generate,
    }
    parser.add_argument("--only", choices=(*generators, "all"), default="all")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[4]
    selected = generators if args.only == "all" else {args.only: generators[args.only]}
    outputs = {path: content for generator in selected.values() for path, content in generator(root).items()}
    for relative_path, content in sorted(outputs.items()):
        output_path = root / relative_path
        output_path.parent.mkdir(parents=True, exist_ok=True)
        if not output_path.exists() or output_path.read_bytes() != content:
            output_path.write_bytes(content)

    update_generated_hashes(root, set(outputs))
    families = ", ".join(selected)
    print(f"Generated {len(outputs)} files for {families} fixtures and updated their manifest hashes.")


if __name__ == "__main__":
    main()
