"""Validation subcommand for AriadShift capabilities registry."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Any

import jsonschema


def validate_capabilities(
    capabilities_data: dict[str, Any],
    schema_data: dict[str, Any],
    committed_data: dict[str, Any] | None = None,
) -> list[str]:
    """Validate capabilities dictionary against the JSON schema and benchmark invariants."""
    errors: list[str] = []

    validator = jsonschema.Draft202012Validator(schema_data)
    for err in validator.iter_errors(capabilities_data):
        loc = " -> ".join(str(p) for p in err.path) or "root"
        errors.append(f"Schema violation at {loc}: {err.message}")

    committed_measured_edges: set[tuple[str, str, str]] = set()
    if committed_data:
        for c_edge in committed_data.get("edges", []):
            if c_edge.get("metrics") is not None:
                committed_measured_edges.add((
                    c_edge.get("from", ""),
                    c_edge.get("to", ""),
                    c_edge.get("engine", ""),
                ))

    # Invariants:
    # 1. Every measured edge must have at least 2 scored fixtures
    # 2. Every measured edge must have numeric peak_mem_mb
    # 3. An edge measured in committed baseline must not become unmeasured (null metrics)
    edges = capabilities_data.get("edges", [])
    for idx, edge in enumerate(edges):
        metrics = edge.get("metrics")
        from_fmt = edge.get("from", "")
        to_fmt = edge.get("to", "")
        engine = edge.get("engine", "")
        edge_label = f"{from_fmt} -> {to_fmt} ({engine})"
        edge_key = (from_fmt, to_fmt, engine)

        if metrics is not None:
            samples = metrics.get("samples", 0)
            if samples < 2:
                errors.append(
                    f"Edge {edge_label} has only {samples} samples; minimum 2 required."
                )
            if "peak_mem_mb" in metrics and metrics["peak_mem_mb"] is None:
                errors.append(
                    f"Edge {edge_label} has null peak_mem_mb; numeric value required."
                )
        else:
            if edge_key in committed_measured_edges:
                errors.append(
                    f"Edge {edge_label} has null metrics, but was measured in committed baseline."
                )

    return errors


def build_parser(parser: argparse.ArgumentParser | None = None) -> argparse.ArgumentParser:
    """Configure CLI parser for the check subcommand."""
    if parser is None:
        parser = argparse.ArgumentParser(description="Validate capabilities.json against schema")
    parser.add_argument(
        "--file",
        type=Path,
        default=Path("crates/ariad-core/data/capabilities.json"),
        help="Path to capabilities.json file",
    )
    parser.add_argument(
        "--schema",
        type=Path,
        default=Path("schemas/capabilities.v0.json"),
        help="Path to capabilities JSON schema",
    )
    parser.add_argument(
        "--committed",
        type=str,
        default="HEAD",
        help="Path to committed baseline capabilities.json or 'HEAD' to read from git",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if not args.file.is_file():
        print(f"Error: capabilities file not found at {args.file}", file=sys.stderr)
        return 1

    if not args.schema.is_file():
        print(f"Error: schema file not found at {args.schema}", file=sys.stderr)
        return 1

    capabilities_data = json.loads(args.file.read_text(encoding="utf-8"))
    schema_data = json.loads(args.schema.read_text(encoding="utf-8"))

    committed_data: dict[str, Any] | None = None
    if args.committed:
        if args.committed == "HEAD":
            try:
                import subprocess
                cmd = ["git", "show", "HEAD:crates/ariad-core/data/capabilities.json"]
                proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", check=False)
                if proc.returncode == 0 and proc.stdout.strip():
                    committed_data = json.loads(proc.stdout)
            except Exception:
                pass
        else:
            committed_path = Path(args.committed)
            if not committed_path.is_file():
                print(f"Error: committed baseline file not found: {args.committed}", file=sys.stderr)
                return 1
            committed_data = json.loads(committed_path.read_text(encoding="utf-8"))

    errors = validate_capabilities(capabilities_data, schema_data, committed_data=committed_data)
    if errors:
        print(f"Validation failed with {len(errors)} error(s):", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    print(f"Validation passed: {args.file} conforms to schema and all measured edges have >= 2 samples.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
