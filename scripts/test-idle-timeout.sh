#!/usr/bin/env bash
# Offline test for scripts/idle-timeout.py: exit codes, pass-through of output, the kill after a
# silence (including group members that ignore SIGTERM), a steady stream that keeps a command
# alive, the step output marker and the forwarding of signals.
set -euo pipefail
cd "$(dirname "$0")/.."

script=scripts/idle-timeout.py
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL | $1" >&2; exit 1; }
# A sleep length no other process uses, so pgrep finds only the children of one test.
unique() { echo "$((60000 + RANDOM))"; }
gone() { ! pgrep -f "sleep $1" >/dev/null 2>&1; }

# 1. The command's output and exit code pass through.
set +e
out=$(python3 "$script" --idle 30 -- sh -c 'echo hello; echo oops >&2; exit 3' 2>&1)
rc=$?
set -e
[ "$rc" -eq 3 ] || fail "the exit code of the command must be returned, got $rc"
printf '%s\n' "$out" | grep -q '^hello$' || fail "standard output must pass through"
printf '%s\n' "$out" | grep -q '^oops$' || fail "standard error must pass through"
echo "PASS | output and exit code pass through"

# 2. A silent command is killed, with exit code 124, the marker and a message.
: > "$work/output"
start=$(date +%s)
set +e
out=$(GITHUB_OUTPUT="$work/output" python3 "$script" --idle 2 -- sh -c 'echo started; sleep 60' 2>&1)
rc=$?
set -e
elapsed=$(( $(date +%s) - start ))
[ "$rc" -eq 124 ] || fail "a hung command must exit 124, got $rc"
[ "$elapsed" -lt 30 ] || fail "the kill took $elapsed s, expected a few seconds"
printf '%s\n' "$out" | grep -q 'no output for 2 s' || fail "the message must say why the command was stopped"
grep -qx 'idle_killed=true' "$work/output" || fail "GITHUB_OUTPUT must get idle_killed=true"
echo "PASS | a silent command is killed after the idle time (124, marker, message)"

# 3. The marker is absent when nothing was killed.
: > "$work/output"
GITHUB_OUTPUT="$work/output" python3 "$script" --idle 30 -- sh -c 'echo fine' >/dev/null
[ ! -s "$work/output" ] || fail "no marker may be written for a command that finished"
echo "PASS | no marker for a command that finished"

# 4. Steady output keeps a longer command alive.
# shellcheck disable=SC2016 # the loop runs in the child shell, so the quotes are intended
python3 "$script" --idle 3 -- sh -c 'i=0; while [ "$i" -lt 6 ]; do echo tick; i=$((i + 1)); sleep 1; done' >/dev/null \
    || fail "a command that keeps printing must not be killed"
echo "PASS | steady output keeps the command alive"

# 5. The whole process group goes: a background child does not survive.
sleeper=$(unique)
set +e
python3 "$script" --idle 2 -- sh -c "sleep $sleeper & wait" >/dev/null 2>&1
set -e
sleep 1
if ! gone "$sleeper"; then
    pkill -f "sleep $sleeper" || true
    fail "the child process group must be killed with the command"
fi
echo "PASS | the process group is killed"

# 6. A group member that ignores SIGTERM dies too, although the leader exits on SIGTERM at once.
sleeper=$(unique)
start=$(date +%s)
set +e
python3 "$script" --idle 2 -- sh -c "(trap '' TERM; exec sleep $sleeper) & sleep 60" >/dev/null 2>&1
rc=$?
set -e
elapsed=$(( $(date +%s) - start ))
[ "$rc" -eq 124 ] || fail "the exit code must be 124, got $rc"
[ "$elapsed" -lt 60 ] || fail "the kill took $elapsed s"
sleep 1
if ! gone "$sleeper"; then
    pkill -9 -f "sleep $sleeper" || true
    fail "a group member that ignores SIGTERM must be killed with SIGKILL"
fi
echo "PASS | a member that ignores SIGTERM is killed"

# 7. A command ended by a signal gives 128 plus the signal number.
set +e
# shellcheck disable=SC2016 # $$ is the child shell's own pid
python3 "$script" --idle 30 -- sh -c 'kill -9 $$' >/dev/null 2>&1
rc=$?
set -e
[ "$rc" -eq 137 ] || fail "a command killed by SIGKILL must give 137, got $rc"
echo "PASS | exit code 128 plus the signal number"

# 8. SIGTERM sent to the wrapper reaches the command's group.
sleeper=$(unique)
python3 "$script" --idle 120 -- sh -c "sleep $sleeper & wait" >/dev/null 2>&1 &
wrapper=$!
sleep 2
kill -TERM "$wrapper"
set +e
wait "$wrapper"
rc=$?
set -e
sleep 1
if ! gone "$sleeper"; then
    pkill -f "sleep $sleeper" || true
    fail "a signal sent to the wrapper must stop the command"
fi
[ "$rc" -eq 143 ] || fail "the wrapper must exit 143 after SIGTERM, got $rc"
echo "PASS | SIGTERM sent to the wrapper stops the command"
