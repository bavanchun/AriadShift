#!/usr/bin/env python3
"""Run a command and kill it when it prints nothing for too long.

    idle-timeout.py --idle SECONDS -- COMMAND [ARGUMENT]...

The command's standard output and error are merged into one stream and passed through unchanged.
When the command prints nothing for SECONDS, its whole process group is stopped (SIGTERM, then
SIGKILL for every member that is still there after 20 seconds, whether or not the group leader
exited), a line saying so goes to standard error, `idle_killed=true` is appended to the file named
by GITHUB_OUTPUT when that variable is set, and the exit code is 124 (the one `timeout` uses). In
every other case the exit code is the command's own, or 128 plus the signal number when the
command was killed by a signal. SIGINT and SIGTERM sent to this script are forwarded to the
command's process group, so a local Ctrl-C does not leave the command running.

The Docling pack step compiles for about two hours and prints a line every few seconds, so a long
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


def group_alive(process):
    """True while any process of the child's group exists (the leader is reaped by poll)."""
    process.poll()
    try:
        os.killpg(process.pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def stop(process):
    """Stop the child and everything it started: SIGTERM, a grace period, then SIGKILL for the group."""
    if os.name != "posix":
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(process.pid)], check=False)
        process.wait()
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + KILL_GRACE_SECONDS
    while time.monotonic() < deadline and group_alive(process):
        time.sleep(0.2)
    # The leader often exits on SIGTERM while members that ignore it (make, a compiler, a docker
    # client) are still running: kill the group whether or not the leader is gone.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and group_alive(process):
        time.sleep(0.1)
    process.wait()


def exit_code(process):
    """The command's exit code; 128 plus the signal number when a signal ended it."""
    code = process.returncode
    return 128 - code if code < 0 else code


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

    if os.name == "posix":

        def forward(signum, _frame):
            try:
                os.killpg(process.pid, signum)
            except ProcessLookupError:
                pass

        signal.signal(signal.SIGINT, forward)
        signal.signal(signal.SIGTERM, forward)

    state = {"last": time.monotonic()}
    reader = threading.Thread(target=pump, args=(process.stdout, state), daemon=True)
    reader.start()

    while process.poll() is None:
        time.sleep(min(1.0, args.idle / 4))
        # The command may have exited between the poll above and now: only a command that is still
        # running and silent counts as hung.
        if time.monotonic() - state["last"] > args.idle and process.poll() is None:
            print(f"idle-timeout: no output for {args.idle:g} s, stopping the command", file=sys.stderr, flush=True)
            marker = os.environ.get("GITHUB_OUTPUT")
            if marker:
                with open(marker, "a", encoding="utf-8") as handle:
                    handle.write("idle_killed=true\n")
            stop(process)
            reader.join(5)
            return 124
    reader.join(5)
    return exit_code(process)


if __name__ == "__main__":
    sys.exit(main())
