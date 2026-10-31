#!/usr/bin/env python3
"""Run a command with a validated GitHub token, without exposing its value."""

import os
import subprocess
import sys


def clean(token):
    return bool(token) and all(33 <= ord(char) <= 126 for char in token)


def token_environment():
    env = os.environ.copy()
    for name in ("GH_TOKEN", "GITHUB_TOKEN"):
        token = env.get(name, "")
        if clean(token):
            env.pop("GITHUB_TOKEN", None)
            env["GH_TOKEN"] = token
            return env
        if token:
            print(
                f"Ignoring malformed {name}; trying another credential source.",
                file=sys.stderr,
            )
    env.pop("GH_TOKEN", None)
    env.pop("GITHUB_TOKEN", None)
    try:
        result = subprocess.run(
            ["gh", "auth", "token"], env=env.copy(), capture_output=True, check=False
        )
    except OSError:
        raise RuntimeError("Cannot run gh; set a clean GH_TOKEN or GITHUB_TOKEN.") from None
    if result.returncode:
        raise RuntimeError("No GitHub token available; set GH_TOKEN or run gh auth login.")
    # gh terminates its single output line with a newline; preserve all other bytes.
    output = result.stdout
    if output.endswith(b"\r\n"):
        output = output[:-2]
    elif output.endswith(b"\n"):
        output = output[:-1]
    token = output.decode("ascii", errors="replace")
    if not clean(token):
        raise RuntimeError(
            "gh auth token returned malformed output; "
            "whitespace/control characters are forbidden."
        )
    env["GH_TOKEN"] = token
    return env


def main():
    if len(sys.argv) < 2:
        print("Usage: with-github-token.py <command> [args...]", file=sys.stderr)
        return 1
    try:
        env = token_environment()
        os.execvpe(sys.argv[1], sys.argv[1:], env)
    except (RuntimeError, OSError) as error:
        print(f"GitHub token launcher: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
