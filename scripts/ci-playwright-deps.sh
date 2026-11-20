#!/usr/bin/env bash
# Install the system libraries Playwright needs, through scripts/ci-apt.sh.
#
# `playwright install --with-deps` runs a plain apt-get, which has no time limit and no package
# cache: on a day when the archive served 124 kB/s the 140 MB took 19 minutes and the Web job hit
# its own limit. `playwright install-deps --dry-run` simulates the install (`apt-get install -s`)
# and lists every package that is missing; this script installs exactly that list with
# scripts/ci-apt.sh (Azure mirror first, a time limit on the download, a cacheable .deb directory
# through APT_ARCHIVES). The package index must be fresh (`ci-apt.sh update`) before the call.
#
# Usage: scripts/ci-playwright-deps.sh <browser>...
# Environment (for tests): PLAYWRIGHT_DEPS_COMMAND (the command that prints the dry-run report;
# default: pnpm --filter @ariadshift/web exec playwright install-deps --dry-run), CI_APT (the
# installer; default: scripts/ci-apt.sh).
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "usage: ci-playwright-deps.sh <browser>..." >&2
    exit 2
fi
here="$(cd "$(dirname "$0")" && pwd)"
installer="${CI_APT:-$here/ci-apt.sh}"
read -r -a report_command <<<"${PLAYWRIGHT_DEPS_COMMAND:-pnpm --filter @ariadshift/web exec playwright install-deps --dry-run}"

# The report ends with exit code 1 when packages are missing, so the status is not the signal.
report="$("${report_command[@]}" "$@" 2>&1)" || true
printf '%s\n' "$report" >&2

if printf '%s\n' "$report" | grep -q '^All system dependencies are installed\.'; then
    echo "Playwright system dependencies are already installed."
    exit 0
fi

# Lines under "Missing system dependencies (N):" are the package names, indented by two spaces.
packages=()
while IFS= read -r name; do
    packages+=("$name")
done < <(printf '%s\n' "$report" | awk '/^Missing system dependencies \([0-9]+\):/ {on = 1; next} on && /^  [^ ]/ {print $1}')

expected="$(printf '%s\n' "$report" | sed -n 's/^Missing system dependencies (\([0-9]*\)):.*/\1/p' | head -n1)"
if [ -z "$expected" ] || [ "${#packages[@]}" -eq 0 ] || [ "${#packages[@]}" -ne "$expected" ]; then
    echo "could not read the missing package list from the Playwright report" >&2
    exit 1
fi

export APT_ARCHIVES="${APT_ARCHIVES:-$HOME/.cache/ci-apt-archives}"
exec "$installer" install --no-install-recommends -y "${packages[@]}"
