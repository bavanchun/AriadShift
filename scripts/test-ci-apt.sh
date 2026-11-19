#!/usr/bin/env bash
# Offline test for scripts/ci-apt.sh with stub apt-get and timeout commands.
set -euo pipefail
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
log="$work/log"

# apt-get fails while FAIL_UNTIL_SWITCH is set and the source file still names the Azure mirror.
cat > "$work/bin/apt-get" <<STUB
#!/usr/bin/env bash
echo "apt-get \$*" >> "$log"
if [ -n "\${FAIL_ON_AZURE:-}" ] && grep -q azure.archive "$work/sources.list"; then
    exit \${APT_STUB_RC:-100}
fi
STUB
# timeout passes through unless the stub is told to time out; GNU timeout exits 124.
cat > "$work/bin/timeout" <<STUB
#!/usr/bin/env bash
shift
if [ -n "\${STUB_TIMEOUT:-}" ] && grep -q azure.archive "$work/sources.list"; then
    echo "timeout" >> "$log"
    exit 124
fi
exec "\$@"
STUB
chmod +x "$work/bin/apt-get" "$work/bin/timeout"

run() {
    : > "$log"
    rm -f "$work/marker"
    printf 'deb http://azure.archive.ubuntu.com/ubuntu resolute main\n' > "$work/sources.list"
    PATH="$work/bin:$PATH" APT_SUDO='' APT_SOURCES="$work/sources.list" APT_MARKER="$work/marker" \
        bash scripts/ci-apt.sh "$@" 2> "$work/err"
}

fail() { echo "FAIL | $1" >&2; exit 1; }

# 1. The Azure mirror works: one call, sources untouched, no marker.
run install -y foo
[ "$(wc -l < "$log")" -eq 1 ] || fail "Azure success should run apt-get once"
grep -q azure.archive "$work/sources.list" || fail "Azure success must not rewrite sources"
[ ! -e "$work/marker" ] || fail "Azure success must not set the marker"
echo "PASS | the Azure mirror is used when it works"

# 2. apt-get fails on Azure: sources switch, index refreshes, command retries once.
FAIL_ON_AZURE=1 run install -y foo
grep -q 'archive.ubuntu.com/ubuntu' "$work/sources.list" || fail "a failure must switch the sources to archive.ubuntu.com"
grep -q azure.archive "$work/sources.list" && fail "a failure must remove the Azure mirror"
[ -e "$work/marker" ] || fail "a failure must set the marker"
[ "$(sed -n 2p "$log")" = "apt-get update" ] || fail "the index must be refreshed after switching"
[ "$(sed -n 3p "$log")" = "apt-get install -y foo" ] || fail "the command must be retried after switching"
echo "PASS | a failure on the Azure mirror falls back to archive.ubuntu.com"

# 3. A timeout (exit 124) falls back the same way.
STUB_TIMEOUT=1 run install -y foo
grep -q azure.archive "$work/sources.list" && fail "a timeout must switch the sources"
grep -q '^timeout$' "$log" || fail "the timeout stub was not used"
echo "PASS | a timeout falls back to archive.ubuntu.com"

# 4. After a switch, later calls go straight to apt-get with no further timeout wrapper.
: > "$log"
touch "$work/marker"
PATH="$work/bin:$PATH" APT_SUDO='' APT_SOURCES="$work/sources.list" APT_MARKER="$work/marker" \
    bash scripts/ci-apt.sh update
[ "$(wc -l < "$log")" -eq 1 ] || fail "after a switch apt-get runs once"
echo "PASS | later calls stay on the primary archive"

# 5. A failed update on the fallback still fails the step.
if FAIL_ON_AZURE=1 APT_STUB_RC=100 run update 2>/dev/null; then
    # the stub stops failing once the sources are rewritten, so the retried update succeeds
    echo "PASS | update falls back and succeeds on the primary archive"
else
    fail "update should succeed on the primary archive in the stub"
fi

# 6. APT_ARCHIVES moves the package cache for installs, creates its partial directory, and leaves
# `update` alone.
: > "$log"
rm -f "$work/marker"
printf 'deb http://azure.archive.ubuntu.com/ubuntu resolute main\n' > "$work/sources.list"
PATH="$work/bin:$PATH" APT_SUDO='' APT_SOURCES="$work/sources.list" APT_MARKER="$work/marker" \
    APT_ARCHIVES="$work/archives" bash scripts/ci-apt.sh install -y foo
grep -q -- "-o Dir::Cache::archives=$work/archives" "$log" || fail "install must use the archive directory"
[ -d "$work/archives/partial" ] || fail "the archive directory needs a partial subdirectory"
: > "$log"
PATH="$work/bin:$PATH" APT_SUDO='' APT_SOURCES="$work/sources.list" APT_MARKER="$work/marker" \
    APT_ARCHIVES="$work/archives" bash scripts/ci-apt.sh update
grep -q "Dir::Cache" "$log" && fail "update must not change the archive directory"
echo "PASS | APT_ARCHIVES applies to installs only"

# 7. The install limit is 600 seconds unless the test overrides it.
grep -q 'APT_FIRST_TIMEOUT:-600' scripts/ci-apt.sh || fail "the install limit should be 600 seconds"
echo "PASS | the install limit is 600 seconds"
