#!/usr/bin/env bash
set -euo pipefail

# Test apps/macos/scripts/embed-pandoc.sh on any OS with stub `lipo` and `codesign` that record their arguments. It
# cannot prove that Xcode runs the phase or that macOS runs the binary; it proves what the phase does with its inputs:
# the binary lands at Contents/Helpers/pandoc, executable, with a slice for every arch of the build; a binary that lacks
# an arch, is no Mach-O or is missing fails the build (and leaves no helper behind); the three licence files land in
# Contents/Resources/ThirdParty/pandoc and are the unchanged upstream texts (checked by SHA-256); and the hardened-runtime
# signature is applied only when Xcode allows signing.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH='' cd -- "${SCRIPT_DIR}/.." && pwd)"
failures=0
checks=0

# The upstream 3.12 files, as fetched from https://github.com/jgm/pandoc/tree/3.12 (COPYING.md is shipped as COPYING.txt).
COPYING_SHA256="3ebfbf9235b049d384cdba5a66a1a3434bb544391e720b76faa8e1b3ee61a4e5"
COPYRIGHT_SHA256="842e33ef01625e93f85bebb8bac83aa570186b7aa77a09971257cc29f8f60740"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

# The script finds the repository two levels above SRCROOT, so run a copy inside a scratch tree.
TREE="${TMP_DIR}/tree"
SRCROOT="${TREE}/apps/macos"
mkdir -p "${SRCROOT}/scripts" "${TMP_DIR}/stub"
cp "${REPO_ROOT}/apps/macos/scripts/embed-pandoc.sh" "${SRCROOT}/scripts/embed-pandoc.sh"
cp -R "${REPO_ROOT}/apps/macos/ThirdParty" "${SRCROOT}/ThirdParty"

# A fake binary is one line, "ARCHS: <names>". The stub lipo reads and writes that line.
cat > "${TMP_DIR}/stub/lipo" <<'STUB'
#!/usr/bin/env bash
set -u
printf 'lipo %s\n' "$*" >> "${STUB_LOG:?}"
case "${1:-}" in
    -archs)
        line="$(head -n 1 "$2" 2>/dev/null)" || exit 1
        [[ "${line}" == "ARCHS: "* ]] || exit 1
        echo "${line#ARCHS: }"
        ;;
    -create)
        shift
        archs=""
        files=()
        while [[ $# -gt 0 && "$1" != "-output" ]]; do
            archs="${archs:+${archs} }$(head -n 1 "$1" | sed 's/^ARCHS: //')"
            files+=("$1")
            shift
        done
        echo "ARCHS: ${archs}" > "$2"
        ;;
    *) exit 1 ;;
esac
STUB
cat > "${TMP_DIR}/stub/codesign" <<'STUB'
#!/usr/bin/env bash
printf 'codesign %s\n' "$*" >> "${STUB_LOG:?}"
STUB
chmod +x "${TMP_DIR}/stub/lipo" "${TMP_DIR}/stub/codesign"

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

fake_binary() { # fake_binary PATH ARCHS...
    local path="$1"
    shift
    mkdir -p "$(dirname "${path}")"
    echo "ARCHS: $*" > "${path}"
    chmod 755 "${path}"
}

reset() {
    rm -rf "${TMP_DIR}/build" "${TREE}/.tools"
    : > "${TMP_DIR}/log"
    BUILT="${TMP_DIR}/build/AriadShift.app/Contents"
}

# embed [VAR=VALUE...]: run the phase the way Xcode does, from its own directory, with the build variables set.
embed() {
    (
        cd "${SRCROOT}"
        env -i PATH="${TMP_DIR}/stub:${PATH}" STUB_LOG="${TMP_DIR}/log" HOME="${TMP_DIR}" \
            SRCROOT="${SRCROOT}" TARGET_BUILD_DIR="${TMP_DIR}/build" \
            CONTENTS_FOLDER_PATH="AriadShift.app/Contents" \
            UNLOCALIZED_RESOURCES_FOLDER_PATH="AriadShift.app/Contents/Resources" \
            ARCHS="arm64 x86_64" "$@" \
            bash "${SRCROOT}/scripts/embed-pandoc.sh" > "${TMP_DIR}/out.log" 2>&1
    )
}

log_has() { grep -q -- "$1" "${TMP_DIR}/log"; }
out_has() { grep -q -- "$1" "${TMP_DIR}/out.log"; }
helper_absent() { [[ ! -e "${BUILT}/Helpers/pandoc" ]]; }

notices_are_embedded_unchanged() {
    local dir="${BUILT}/Resources/ThirdParty/pandoc"
    [[ -s "${dir}/ACKNOWLEDGEMENTS.txt" ]] \
        && [[ "$(sha256sum "${dir}/COPYING.txt" | cut -d' ' -f1)" == "${COPYING_SHA256}" ]] \
        && [[ "$(sha256sum "${dir}/COPYRIGHT" | cut -d' ' -f1)" == "${COPYRIGHT_SHA256}" ]]
}

override_is_copied_as_an_executable_helper() {
    reset
    fake_binary "${TMP_DIR}/given/pandoc" arm64 x86_64
    embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" \
        && [[ -x "${BUILT}/Helpers/pandoc" ]] \
        && cmp -s "${TMP_DIR}/given/pandoc" "${BUILT}/Helpers/pandoc" \
        && notices_are_embedded_unchanged
}

override_missing_an_arch_fails() {
    reset
    fake_binary "${TMP_DIR}/given/pandoc" arm64
    ! embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" \
        && out_has "needs 'arm64 x86_64'" && helper_absent
}

override_that_is_no_macho_fails() {
    reset
    mkdir -p "${TMP_DIR}/given"
    echo "#!/bin/sh" > "${TMP_DIR}/given/pandoc"
    chmod 755 "${TMP_DIR}/given/pandoc"
    ! embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" \
        && out_has "not a Mach-O binary" && helper_absent
}

override_that_is_not_executable_fails() {
    reset
    fake_binary "${TMP_DIR}/given/pandoc" arm64 x86_64
    chmod 644 "${TMP_DIR}/given/pandoc"
    ! embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" \
        && out_has "is not an executable file" && helper_absent
}

pinned_slices_are_joined_with_lipo() {
    reset
    fake_binary "${TREE}/.tools/pandoc-macos-arm64/bin/pandoc" arm64
    fake_binary "${TREE}/.tools/pandoc-macos-x86_64/bin/pandoc" x86_64
    embed \
        && log_has "lipo -create ${TREE}/.tools/pandoc-macos-arm64/bin/pandoc ${TREE}/.tools/pandoc-macos-x86_64/bin/pandoc -output ${BUILT}/Helpers/pandoc" \
        && [[ "$(head -n 1 "${BUILT}/Helpers/pandoc")" == "ARCHS: arm64 x86_64" ]] \
        && [[ -x "${BUILT}/Helpers/pandoc" ]] \
        && notices_are_embedded_unchanged
}

one_arch_build_copies_without_lipo() {
    reset
    fake_binary "${TREE}/.tools/pandoc-macos-arm64/bin/pandoc" arm64
    embed ARCHS="arm64" \
        && ! log_has "lipo -create" \
        && [[ "$(head -n 1 "${BUILT}/Helpers/pandoc")" == "ARCHS: arm64" ]]
}

missing_pinned_slice_names_the_fetch_command() {
    reset
    fake_binary "${TREE}/.tools/pandoc-macos-arm64/bin/pandoc" arm64
    ! embed \
        && out_has "bash scripts/install-pandoc.sh --macos-arch x86_64" && helper_absent
}

missing_notice_fails_the_build() {
    reset
    fake_binary "${TMP_DIR}/given/pandoc" arm64 x86_64
    mv "${SRCROOT}/ThirdParty/pandoc/COPYRIGHT" "${TMP_DIR}/COPYRIGHT.kept"
    local status=0
    embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" || status=$?
    mv "${TMP_DIR}/COPYRIGHT.kept" "${SRCROOT}/ThirdParty/pandoc/COPYRIGHT"
    [[ "${status}" -ne 0 ]] && out_has "notice file missing or empty"
}

signed_with_the_hardened_runtime_when_xcode_allows_signing() {
    reset
    fake_binary "${TMP_DIR}/given/pandoc" arm64 x86_64
    embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" CODE_SIGNING_ALLOWED=YES EXPANDED_CODE_SIGN_IDENTITY=- \
        && log_has "codesign --force --options runtime --timestamp=none --sign - ${BUILT}/Helpers/pandoc" \
        && ! log_has "--entitlements"
}

real_identity_gets_a_secure_timestamp() {
    reset
    fake_binary "${TMP_DIR}/given/pandoc" arm64 x86_64
    embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" CODE_SIGNING_ALLOWED=YES \
        EXPANDED_CODE_SIGN_IDENTITY="Developer ID Application: Example" \
        && log_has "codesign --force --options runtime --timestamp --sign Developer ID Application: Example ${BUILT}/Helpers/pandoc"
}

not_signed_when_xcode_disallows_signing() {
    reset
    fake_binary "${TMP_DIR}/given/pandoc" arm64 x86_64
    embed ASHIFT_BUNDLED_PANDOC_PATH="${TMP_DIR}/given/pandoc" CODE_SIGNING_ALLOWED=NO EXPANDED_CODE_SIGN_IDENTITY=- \
        && ! log_has "codesign"
}

shipped_notices_are_the_unchanged_upstream_texts() {
    [[ "$(sha256sum "${REPO_ROOT}/apps/macos/ThirdParty/pandoc/COPYING.txt" | cut -d' ' -f1)" == "${COPYING_SHA256}" ]] \
        && [[ "$(sha256sum "${REPO_ROOT}/apps/macos/ThirdParty/pandoc/COPYRIGHT" | cut -d' ' -f1)" == "${COPYRIGHT_SHA256}" ]]
}

acknowledgement_names_the_exact_version_and_source() {
    local file="${REPO_ROOT}/apps/macos/ThirdParty/pandoc/ACKNOWLEDGEMENTS.txt"
    grep -q 'Version:   3.12 ' "${file}" \
        && grep -q 'https://github.com/jgm/pandoc/tree/3.12' "${file}" \
        && grep -q 'GPL-2.0-or-later' "${file}"
}

check "an override binary is copied to Helpers/pandoc, executable, with the notices" override_is_copied_as_an_executable_helper
check "an override without every build arch fails" override_missing_an_arch_fails
check "an override that is not a Mach-O fails" override_that_is_no_macho_fails
check "an override that is not executable fails" override_that_is_not_executable_fails
check "the pinned slices are joined with lipo in ARCHS order" pinned_slices_are_joined_with_lipo
check "a one-arch build copies the slice without lipo" one_arch_build_copies_without_lipo
check "a missing pinned slice names the command that fetches it" missing_pinned_slice_names_the_fetch_command
check "a missing notice file fails the build" missing_notice_fails_the_build
check "the helper is signed ad hoc with the hardened runtime and no entitlements" signed_with_the_hardened_runtime_when_xcode_allows_signing
check "a real identity gets a secure timestamp" real_identity_gets_a_secure_timestamp
check "nothing is signed when Xcode disallows signing" not_signed_when_xcode_disallows_signing
check "the shipped COPYING.txt and COPYRIGHT are the unchanged 3.12 texts" shipped_notices_are_the_unchanged_upstream_texts
check "the acknowledgement names version 3.12, its source and its licence" acknowledgement_names_the_exact_version_and_source

echo "${checks} checks, ${failures} failed"
[[ "${failures}" -eq 0 ]]
