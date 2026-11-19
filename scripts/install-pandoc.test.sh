#!/usr/bin/env bash
set -euo pipefail

# Test the `--macos-arch` mode of install-pandoc.sh without the network: a stub `curl` records the URL it is asked to
# fetch and fails, so the script stops right after choosing the asset. This proves that the mode picks the pinned macOS
# asset of the requested architecture on any host (the app embeds both slices), that it rejects other arguments, and
# that the default mode still goes by the host. The checksums themselves are the script's own and are not repeated here.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
failures=0
checks=0

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

# The script finds .tools next to its own directory, so run a copy inside a scratch tree.
mkdir -p "${TMP_DIR}/repo/scripts" "${TMP_DIR}/stub"
cp "${SCRIPT_DIR}/install-pandoc.sh" "${TMP_DIR}/repo/scripts/install-pandoc.sh"
cat > "${TMP_DIR}/stub/curl" <<'STUB'
#!/bin/sh
printf '%s\n' "$*" >> "${STUB_LOG:?}"
exit 22
STUB
chmod +x "${TMP_DIR}/stub/curl"

check() {
    checks=$((checks + 1))
    local name="$1"
    shift
    if "$@"; then
        echo "PASS | ${name}"
    else
        echo "FAIL | ${name}" >&2
        failures=$((failures + 1))
    fi
}

run_installer() {
    : > "${TMP_DIR}/curl.log"
    : > "${TMP_DIR}/out.log"
    rm -rf "${TMP_DIR}/repo/.tools"
    STUB_LOG="${TMP_DIR}/curl.log" PATH="${TMP_DIR}/stub:${PATH}" \
        sh "${TMP_DIR}/repo/scripts/install-pandoc.sh" "$@" > "${TMP_DIR}/out.log" 2>&1
}

fetches() {
    grep -q -- "$1" "${TMP_DIR}/curl.log"
}

arm64_mode_fetches_the_arm64_macos_asset() {
    ! run_installer --macos-arch arm64 \
        && fetches 'releases/download/3.12/pandoc-3.12-arm64-macOS.zip' \
        && grep -q 'Downloading Pandoc 3.12 for Darwin/arm64' "${TMP_DIR}/out.log"
}

x86_64_mode_fetches_the_x86_64_macos_asset() {
    ! run_installer --macos-arch x86_64 \
        && fetches 'releases/download/3.12/pandoc-3.12-x86_64-macOS.zip' \
        && grep -q 'Downloading Pandoc 3.12 for Darwin/x86_64' "${TMP_DIR}/out.log"
}

unknown_arch_is_refused_before_any_download() {
    ! run_installer --macos-arch ppc \
        && [[ ! -s "${TMP_DIR}/curl.log" ]] \
        && grep -q 'Usage:' "${TMP_DIR}/out.log"
}

missing_arch_is_refused_before_any_download() {
    ! run_installer --macos-arch \
        && [[ ! -s "${TMP_DIR}/curl.log" ]] \
        && grep -q 'Usage:' "${TMP_DIR}/out.log"
}

other_arguments_are_refused_before_any_download() {
    ! run_installer --bogus \
        && [[ ! -s "${TMP_DIR}/curl.log" ]] \
        && grep -q 'Usage:' "${TMP_DIR}/out.log"
}

default_mode_still_goes_by_the_host() {
    local expected
    case "$(uname -s)/$(uname -m)" in
        Linux/x86_64) expected='pandoc-3.12-linux-amd64.tar.gz' ;;
        Linux/aarch64 | Linux/arm64) expected='pandoc-3.12-linux-arm64.tar.gz' ;;
        Darwin/arm64) expected='pandoc-3.12-arm64-macOS.zip' ;;
        Darwin/x86_64) expected='pandoc-3.12-x86_64-macOS.zip' ;;
        *) return 0 ;;
    esac
    ! run_installer && fetches "releases/download/3.12/${expected}"
}

check "--macos-arch arm64 fetches the pinned arm64 macOS asset" arm64_mode_fetches_the_arm64_macos_asset
check "--macos-arch x86_64 fetches the pinned x86_64 macOS asset" x86_64_mode_fetches_the_x86_64_macos_asset
check "an unknown architecture is refused before any download" unknown_arch_is_refused_before_any_download
check "a missing architecture is refused before any download" missing_arch_is_refused_before_any_download
check "other arguments are refused before any download" other_arguments_are_refused_before_any_download
check "the default mode still goes by the host" default_mode_still_goes_by_the_host

echo "${checks} checks, ${failures} failed"
[[ "${failures}" -eq 0 ]]
