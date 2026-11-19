#!/usr/bin/env bash
# Run apt-get on a GitHub-hosted Ubuntu runner, Azure mirror first.
#
# The runner image points apt at azure.archive.ubuntu.com, which is next to the runner but
# sometimes stalls; archive.ubuntu.com is always reachable but was slow for long stretches too
# (a LibreOffice install took 9 to 18 minutes instead of 20 seconds). Only the network part is
# bounded: `update` and the download of an install run under a time limit, and when one fails or
# times out apt is pointed at archive.ubuntu.com, the index is refreshed and the step is run
# again. The unpacking that follows a finished download has no limit, because killing dpkg
# half way through leaves the package database interrupted. Once switched, later calls in the
# same job stay on the primary archive.
#
# Usage: scripts/ci-apt.sh update
#        scripts/ci-apt.sh install [options] <package>...
# Package names are pinned to the candidate version of the index in use, after any switch of
# mirror, and the pins are printed. Options start with "-" (for example -y).
#
# Set APT_ARCHIVES to a directory to keep the downloaded .deb files there instead of
# /var/cache/apt/archives, so a job can cache and restore them; apt still checks every cached file
# against the hashes in the package index before it uses it.
#
# Environment (for tests): APT_SUDO (default sudo), APT_SOURCES (files to rewrite), APT_MARKER,
# APT_FIRST_TIMEOUT (seconds allowed on the Azure mirror; default 240 for update, 420 for the
# download of an install).
set -euo pipefail

sudo_cmd="${APT_SUDO-sudo}"
marker="${APT_MARKER:-/tmp/ci-apt-primary}"
sources=${APT_SOURCES:-"/etc/apt/apt-mirrors*.txt /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources"}

if [ "$#" -lt 1 ]; then
    echo "usage: ci-apt.sh update | install [options] <package>..." >&2
    exit 2
fi
command_name="$1"
shift

apt_args=()
if [ -n "${APT_ARCHIVES:-}" ] && [ "$command_name" = install ]; then
    # Created by the runner user; apt adds its own partial/ directory. The sandbox user cannot
    # reach a home directory, so downloads are made by root instead of warning about it.
    mkdir -p "$APT_ARCHIVES"
    apt_args=(-o "Dir::Cache::archives=$APT_ARCHIVES" -o APT::Keep-Downloaded-Packages=true -o APT::Sandbox::User=root)
fi

use_primary() {
    local source
    # shellcheck disable=SC2086 # the list is a space separated set of globs
    for source in $sources; do
        if [ -f "$source" ]; then
            $sudo_cmd perl -pi -e 's#https?://azure\.archive\.ubuntu\.com/ubuntu#https://archive.ubuntu.com/ubuntu#g' "$source"
        fi
    done
    touch "$marker"
}

# Pin every package name to its candidate version in the current index.
pinned=()
resolve_pins() {
    local arg version
    pinned=()
    for arg in "$@"; do
        case "$arg" in
            -* | *=*)
                pinned+=("$arg")
                ;;
            *)
                version="$(apt-cache policy "$arg" | awk '/Candidate:/ {print $2}')"
                if [ -z "$version" ] || [ "$version" = '(none)' ]; then
                    echo "no installable candidate for $arg" >&2
                    exit 1
                fi
                printf 'Selected %s=%s\n' "$arg" "$version" >&2
                pinned+=("$arg=$version")
                ;;
        esac
    done
}

# Run apt-get with the bounded network part first. $1 is the limit in seconds; the rest is the
# apt-get command (without the apt options).
apt_bounded() {
    local limit="$1"
    shift
    if [ -f "$marker" ]; then
        $sudo_cmd apt-get ${apt_args[@]+"${apt_args[@]}"} "$@"
    else
        $sudo_cmd timeout -k 30 "$limit" apt-get ${apt_args[@]+"${apt_args[@]}"} "$@"
    fi
}

switch_to_primary() {
    echo "apt-get $command_name failed or timed out on the Azure mirror; switching to archive.ubuntu.com" >&2
    use_primary
    $sudo_cmd apt-get update
}

case "$command_name" in
    update)
        if ! apt_bounded "${APT_FIRST_TIMEOUT:-240}" update; then
            switch_to_primary
        fi
        ;;
    install)
        limit="${APT_FIRST_TIMEOUT:-420}"
        resolve_pins "$@"
        if ! apt_bounded "$limit" install --download-only "${pinned[@]}"; then
            switch_to_primary
            resolve_pins "$@"
            $sudo_cmd apt-get ${apt_args[@]+"${apt_args[@]}"} install --download-only "${pinned[@]}"
        fi
        # Everything is downloaded: unpack and configure without a time limit.
        $sudo_cmd apt-get ${apt_args[@]+"${apt_args[@]}"} install "${pinned[@]}"
        ;;
    *)
        echo "usage: ci-apt.sh update | install [options] <package>..." >&2
        exit 2
        ;;
esac
