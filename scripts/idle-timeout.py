#!/usr/bin/env python3
"""Run a command and kill it when it prints nothing for too long.

    idle-timeout.py --idle SECONDS -- COMMAND [ARGUMENT]...

The command's standard output and error are passed through unchanged. When the command prints
nothing for SECONDS, its whole process group is stopped (SIGTERM, then SIGKILL after 20 seconds),
a line saying so goes to standard error, `idle_killed=true` is appended to the file named by
GITHUB_OUTPUT when that variable is set, and the exit code is 124 (the one `timeout` uses). In every
other case the exit code is the command's own.

The Docling pack build compiles for about two hours and prints a line every few seconds, so a long
silence means a hang. Without this guard a hung step runs until its 310 minute limit.
"""

import argparse
import os
import signal
import subprocess
import sys
import threading
import time

KILL_GRACE_SECONDS = 20


def pump(stream, state):
    """Copy the child's output to ours and remember when it last produced any."""
    out = sys.stdout.buffer
    while True:
        chunk = os.read(stream.fileno(), 65536)
        if not chunk:
            return
        state["last"] = time.monotonic()
        out.write(chunk)
        out.flush()


def stop(process):
    """Stop the child and everything it started."""
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        try:
            process.wait(KILL_GRACE_SECONDS)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    else:
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(process.pid)], check=False)
    process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--idle", type=float, required=True, help="seconds without output that count as a hang")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("a command is required after --")

    options = {"start_new_session": True} if os.name == "posix" else {}
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, **options)
    state = {"last": time.monotonic()}
    reader = threading.Thread(target=pump, args=(process.stdout, state), daemon=True)
    reader.start()

    while process.poll() is None:
        time.sleep(min(1.0, args.idle / 4))
        if time.monotonic() - state["last"] > args.idle:
            print(f"idle-timeout: no output for {args.idle:g} s, stopping the command", file=sys.stderr, flush=True)
            marker = os.environ.get("GITHUB_OUTPUT")
            if marker:
                with open(marker, "a", encoding="utf-8") as handle:
                    handle.write("idle_killed=true\n")
            stop(process)
            reader.join(5)
            return 124
    reader.join(5)
    return process.returncode


if __name__ == "__main__":
    sys.exit(main())
