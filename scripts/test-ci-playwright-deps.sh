#!/usr/bin/env bash
# Offline test for scripts/ci-playwright-deps.sh with a fake Playwright report and a fake installer.
set -euo pipefail
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL | $1" >&2; exit 1; }

cat >"$work/installer" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >"$FAKE_INSTALL_LOG"
printf 'archives=%s\n' "$APT_ARCHIVES" >>"$FAKE_INSTALL_LOG"
SH
chmod +x "$work/installer"
export CI_APT="$work/installer" FAKE_INSTALL_LOG="$work/log" APT_ARCHIVES="$work/archives"

report() { # report <body-file> -> command that prints it and exits 1 like Playwright
    printf '#!/usr/bin/env bash\ncat %q\nexit 1\n' "$1" >"$work/report"
    chmod +x "$work/report"
    export PLAYWRIGHT_DEPS_COMMAND="$work/report"
}

# 1. The missing packages are installed with --no-install-recommends through the installer.
cat >"$work/missing" <<'EOT'
BEWARE: your OS is not officially supported by Playwright; installing dependencies for ubuntu24.04-x64 as a fallback.
Missing system dependencies (3):
  fonts-freefont-ttf
  libgbm1
  libxkbcommon0
EOT
report "$work/missing"
bash scripts/ci-playwright-deps.sh chromium firefox >/dev/null 2>&1 || fail "the missing packages must be installed"
grep -qx 'install --no-install-recommends -y fonts-freefont-ttf libgbm1 libxkbcommon0' "$work/log" \
    || fail "the installer must get exactly the reported packages, got: $(head -n1 "$work/log")"
grep -qx "archives=$work/archives" "$work/log" || fail "APT_ARCHIVES must reach the installer"
echo "PASS | the reported packages are installed through the installer"

# 1b. Error lines that pnpm prints after the report (indented too) are not packages.
cat >"$work/pnpm-tail" <<'EOT'
Missing system dependencies (2):
  libgbm1
  libxkbcommon0
 ELIFECYCLE  Command failed with exit code 1.
Error: ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL

  × "pnpm recursive exec" failed in /home/runner/work/x/x/
  │ apps/web
EOT
report "$work/pnpm-tail"
bash scripts/ci-playwright-deps.sh chromium >/dev/null 2>&1 || fail "pnpm error lines after the report must not break the parse"
grep -qx 'install --no-install-recommends -y libgbm1 libxkbcommon0' "$work/log" \
    || fail "only the reported packages may be installed, got: $(head -n1 "$work/log")"
echo "PASS | pnpm error lines after the report are ignored"

# 2. Nothing missing: nothing installed, success.
printf 'All system dependencies are installed.\n' >"$work/none"
report "$work/none"
rm -f "$work/log"
bash scripts/ci-playwright-deps.sh chromium >/dev/null 2>&1 || fail "nothing missing must succeed"
[ ! -e "$work/log" ] || fail "nothing may be installed when nothing is missing"
echo "PASS | nothing is installed when nothing is missing"

# 3. A report that cannot be read fails instead of installing nothing.
printf 'something unexpected\n' >"$work/garbage"
report "$work/garbage"
if bash scripts/ci-playwright-deps.sh chromium >/dev/null 2>&1; then fail "an unreadable report must fail"; fi
printf 'Missing system dependencies (2):\n  libgbm1\n' >"$work/short"
report "$work/short"
if bash scripts/ci-playwright-deps.sh chromium >/dev/null 2>&1; then fail "a report with a wrong package count must fail"; fi
# Lines that look like list entries but are not package names end the list, so the count check fails.
rm -f "$work/log"
for odd in '  -o foo' '  libfoo:i386' '  pkg=1.0'; do
    printf 'Missing system dependencies (2):\n  libgbm1\n%s\n' "$odd" >"$work/odd"
    report "$work/odd"
    if bash scripts/ci-playwright-deps.sh chromium >/dev/null 2>&1; then fail "a non-name list line ($odd) must fail the count check"; fi
done
# More package-like lines than the reported count also fails.
printf 'Missing system dependencies (1):\n  libgbm1\n  libxkbcommon0\n' >"$work/many"
report "$work/many"
if bash scripts/ci-playwright-deps.sh chromium >/dev/null 2>&1; then fail "more packages than the reported count must fail"; fi
[ ! -e "$work/log" ] || fail "the installer must never run for an unreadable report"
echo "PASS | an unreadable report fails and never installs"

# 4. A failing installer fails the script.
printf '#!/usr/bin/env bash\nexit 7\n' >"$work/installer"
report "$work/missing"
if bash scripts/ci-playwright-deps.sh chromium >/dev/null 2>&1; then fail "a failing installer must fail the script"; fi
echo "PASS | a failing installer fails the script"
