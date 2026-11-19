#!/usr/bin/env bash
# Run `apt-get <args>` on a GitHub-hosted Ubuntu runner, Azure mirror first.
#
# The runner image points apt at azure.archive.ubuntu.com, which is next to the runner but
# sometimes stalls; archive.ubuntu.com is always reachable but was slow for long stretches too
# (a LibreOffice install took 9 to 18 minutes instead of 20 seconds). Try the Azure mirror under
# a time limit; if the command fails or times out, point apt at archive.ubuntu.com, refresh the
# index and run the command again. Once switched, later calls in the same job stay on the
# primary archive.
#
# Usage: scripts/ci-apt.sh update
#        scripts/ci-apt.sh install --no-install-recommends -y <package>...
# Environment (for tests): APT_SUDO (default sudo), APT_SOURCES (files to rewrite),
# APT_FIRST_TIMEOUT (seconds for the Azure attempt, default 240 for update and 420 for install).
set -euo pipefail

sudo_cmd="${APT_SUDO-sudo}"
marker="${APT_MARKER:-/tmp/ci-apt-primary}"
sources=${APT_SOURCES:-"/etc/apt/apt-mirrors*.txt /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources"}

use_primary() {
    local source
    # shellcheck disable=SC2086 # the list is a space separated set of globs
    for source in $sources; do
        if [ -f "$source" ]; then
            $sudo_cmd sed -i -E 's#https?://azure\.archive\.ubuntu\.com/ubuntu#https://archive.ubuntu.com/ubuntu#g' "$source"
        fi
    done
    touch "$marker"
}

if [ "$#" -lt 1 ]; then
    echo "usage: ci-apt.sh <apt-get arguments>" >&2
    exit 2
fi

if [ "$1" = update ]; then
    limit="${APT_FIRST_TIMEOUT:-240}"
else
    limit="${APT_FIRST_TIMEOUT:-420}"
fi

if [ -f "$marker" ]; then
    $sudo_cmd apt-get "$@"
    exit
fi

if $sudo_cmd timeout "$limit" apt-get "$@"; then
    exit 0
fi

echo "apt-get $1 failed or timed out on the Azure mirror; switching to archive.ubuntu.com" >&2
use_primary
$sudo_cmd apt-get update
if [ "$1" != update ]; then
    $sudo_cmd apt-get "$@"
fi
