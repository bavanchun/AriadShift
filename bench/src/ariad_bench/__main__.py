"""Main CLI entrypoint for ariad_bench."""

from __future__ import annotations

import argparse
import sys

from ariad_bench import check, diff, run


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m ariad_bench",
        description="AriadShift benchmark and empirical capability measurement harness",
    )
    subparsers = parser.add_subparsers(dest="command")

    # Subcommands define their own arguments as the single source of truth
    run.build_parser(subparsers.add_parser("run", help="Run benchmark measurements"))
    check.build_parser(subparsers.add_parser("check", help="Check capabilities.json against schema"))
    diff.build_parser(subparsers.add_parser("diff", help="Diff capabilities.json against baseline"))

    if argv is None:
        argv = sys.argv[1:]

    args, remaining = parser.parse_known_args(argv)
    if not args.command:
        parser.print_help()
        return 1

    if args.command == "run":
        return run.main(argv[1:])
    elif args.command == "check":
        return check.main(argv[1:])
    elif args.command == "diff":
        return diff.main(argv[1:])

    return 0


if __name__ == "__main__":
    sys.exit(main())
