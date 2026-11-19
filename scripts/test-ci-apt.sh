#!/usr/bin/env bash
# Offline test for scripts/ci-apt.sh with stub apt-get, apt-cache and timeout commands. Written
# for bash 3.2 and BSD tools so that `just static` also passes on macOS.
set -euo pipefail
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
log="$work/log"

# apt-get logs its arguments. With FAIL_DOWNLOAD_ON_AZURE it fails a download or an update while
# the sources still name the Azure mirror; FAIL_ALWAYS fails every call; FAIL_INSTALL fails the
# real install (the one without --download-only) to model a dpkg error.
cat > "$work/bin/apt-get" <<STUB
#!/usr/bin/env bash
echo "apt-get \$*" >> "$log"
case " \$* " in *" --download-only "* | *" update "*) network=1 ;; *) network= ;; esac
if [ -n "\${FAIL_ALWAYS:-}" ]; then exit 100; fi
if [ -n "\${FAIL_DOWNLOAD_ON_AZURE:-}" ] && [ -n "\$network" ] && grep -q azure.archive "$work/sources.list"; then exit 100; fi
if [ -n "\${FAIL_INSTALL:-}" ] && [ -z "\$network" ]; then exit 100; fi
exit 0
STUB
# The candidate version depends on the mirror, as it can when a mirror lags behind the other.
cat > "$work/bin/apt-cache" <<STUB
#!/usr/bin/env bash
if grep -q azure.archive "$work/sources.list"; then v=1.0; else v=2.0; fi
printf '%s:\n  Installed: (none)\n  Candidate: %s\n' "\$2" "\$v"
STUB
# GNU timeout is called as: timeout -k 30 <limit> <command>. The stub logs the limit and, when
# STUB_TIMEOUT is set, times out a network command on the Azure mirror (exit 124).
cat > "$work/bin/timeout" <<STUB
#!/usr/bin/env bash
shift 2
limit="\$1"
shift
echo "timeout \$limit \$*" >> "$log"
case " \$* " in *" --download-only "* | *" update "*) network=1 ;; *) network= ;; esac
if [ -n "\${STUB_TIMEOUT:-}" ] && [ -n "\$network" ] && grep -q azure.archive "$work/sources.list"; then exit 124; fi
exec "\$@"
STUB
chmod +x "$work/bin/apt-get" "$work/bin/apt-cache" "$work/bin/timeout"

reset() {
    : > "$log"
    rm -f "$work/marker"
    printf 'deb http://azure.archive.ubuntu.com/ubuntu resolute main\n' > "$work/sources.list"
}

# Run ci-apt.sh with the stubs; the exit code is returned.
run() {
    PATH="$work/bin:$PATH" APT_SUDO='' APT_SOURCES="$work/sources.list" APT_MARKER="$work/marker" \
        bash scripts/ci-apt.sh "$@" 2> "$work/err"
}

fail() { echo "FAIL | $1" >&2; cat "$log" >&2; exit 1; }
count() { grep -c -- "$1" "$log" || true; }

# 1. The Azure mirror works: a bounded download, then an unbounded install, nothing rewritten.
reset
run install -y foo
[ "$(count '^timeout 420 apt-get install --download-only -y foo=1.0$')" -eq 1 ] || fail "the download must run under a 420 s limit"
[ "$(count '^apt-get install -y foo=1.0$')" -eq 1 ] || fail "the install must run once, without a limit"
[ "$(count '^timeout')" -eq 1 ] || fail "only the download is bounded"
grep -q azure.archive "$work/sources.list" || fail "success must not rewrite the sources"
[ ! -e "$work/marker" ] || fail "success must not set the marker"
echo "PASS | the Azure mirror is used when it works, and only the download is time limited"

# 2. A failed download switches the sources, refreshes the index and downloads again, with the
# version pinned from the new index.
reset
FAIL_DOWNLOAD_ON_AZURE=1 run install -y foo
grep -q azure.archive "$work/sources.list" && fail "a failure must remove the Azure mirror"
grep -q 'archive.ubuntu.com/ubuntu' "$work/sources.list" || fail "a failure must switch to archive.ubuntu.com"
[ -e "$work/marker" ] || fail "a failure must set the marker"
[ "$(grep -n '^apt-get update$' "$log" | cut -d: -f1)" -eq 3 ] || fail "the index must be refreshed right after the failed download"
[ "$(count '^apt-get install --download-only -y foo=2.0$')" -eq 1 ] || fail "the download must be repeated with the pin of the new index"
[ "$(count '^apt-get install -y foo=2.0$')" -eq 1 ] || fail "the install must use the pin of the new index"
echo "PASS | a failed download falls back to archive.ubuntu.com and re-resolves the pin"

# 3. A timeout (exit 124) falls back the same way.
reset
STUB_TIMEOUT=1 run install -y foo
grep -q azure.archive "$work/sources.list" && fail "a timeout must switch the sources"
[ "$(count '^apt-get install -y foo=2.0$')" -eq 1 ] || fail "a timeout must end in an install of the new pin"
echo "PASS | a timeout falls back to archive.ubuntu.com"

# 4. After a switch, later calls run without the timeout wrapper.
reset
touch "$work/marker"
run install -y foo
run update
[ "$(count '^timeout')" -eq 0 ] || fail "after a switch nothing is time limited"
echo "PASS | later calls stay on the primary archive without a limit"

# 5. update: bounded at 240 s, falls back, and succeeds on the primary archive.
reset
FAIL_DOWNLOAD_ON_AZURE=1 run update
[ "$(count '^timeout 240 apt-get update$')" -eq 1 ] || fail "update must run under a 240 s limit"
grep -q azure.archive "$work/sources.list" && fail "a failed update must switch the sources"
echo "PASS | update is limited to 240 s and falls back"

# 6. If the fallback fails as well, the failure propagates.
reset
if FAIL_ALWAYS=1 run install -y foo; then fail "a failing fallback must fail the step"; fi
echo "PASS | a failure on both mirrors fails the step"

# 7. A failing real install (dpkg) fails the step and is not retried on another mirror.
reset
if FAIL_INSTALL=1 run install -y foo; then fail "a failing install must fail the step"; fi
[ ! -e "$work/marker" ] || fail "an install failure after the download must not switch mirrors"
echo "PASS | an install failure is not retried on another mirror"

# 8. APT_ARCHIVES applies to installs only and the directory is created.
reset
PATH="$work/bin:$PATH" APT_SUDO='' APT_SOURCES="$work/sources.list" APT_MARKER="$work/marker" \
    APT_ARCHIVES="$work/archives" bash scripts/ci-apt.sh install -y foo
[ "$(count "Dir::Cache::archives=$work/archives")" -ge 2 ] || fail "both install calls must use the archive directory"
[ -d "$work/archives" ] || fail "the archive directory must be created"
reset
PATH="$work/bin:$PATH" APT_SUDO='' APT_SOURCES="$work/sources.list" APT_MARKER="$work/marker" \
    APT_ARCHIVES="$work/archives" bash scripts/ci-apt.sh update
[ "$(count 'Dir::Cache')" -eq 0 ] || fail "update must not change the archive directory"
echo "PASS | APT_ARCHIVES applies to installs only"

# 9. Unknown subcommands and missing arguments are usage errors.
if run 2>/dev/null; then fail "no arguments must fail"; fi
if run frobnicate 2>/dev/null; then fail "an unknown command must fail"; fi
echo "PASS | usage errors"
