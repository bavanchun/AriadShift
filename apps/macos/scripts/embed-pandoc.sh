#!/usr/bin/env bash
set -euo pipefail

# Xcode Run Script phase: embed Pandoc at <App>.app/Contents/Helpers/pandoc and its licence notices at
# <App>.app/Contents/Resources/ThirdParty/pandoc/, so Export as Word works with no Pandoc installed.
#
# Pandoc is GPL-2.0-or-later and stays a separate executable: the app and `ashift` start it as a child process and
# never link it. The binary is the unmodified official release pinned in scripts/install-pandoc.sh (one place for the
# version and the SHA-256 of each archive).
#
# - ASHIFT_BUNDLED_PANDOC_PATH set: copy that binary (CI artifacts, local overrides). It must contain a slice for every
#   arch in $ARCHS. It is not checked against the pin: whoever sets it owns that.
# - Otherwise: use the pinned binary of every arch in $ARCHS from .tools/pandoc-macos-<arch>/bin/pandoc and lipo them.
#   That fetch needs the network, which a build phase must not use, so `just macos-pandoc` (or
#   `scripts/install-pandoc.sh --macos-arch <arch>`) runs first; `just macos-build` and `just macos-test` do it.

REPO_ROOT="$(CDPATH='' cd -- "${SRCROOT}/../.." && pwd)"
HELPERS_DIR="${TARGET_BUILD_DIR}/${CONTENTS_FOLDER_PATH}/Helpers"
DEST="${HELPERS_DIR}/pandoc"
NOTICES_SRC="${SRCROOT}/ThirdParty/pandoc"
NOTICES_DEST="${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}/ThirdParty/pandoc"
NOTICE_FILES=(COPYING.txt COPYRIGHT ACKNOWLEDGEMENTS.txt)
mkdir -p "${HELPERS_DIR}"

# require_archs BINARY: every arch of the build must be a slice of BINARY.
require_archs() {
    local binary="$1" binary_archs arch
    if ! binary_archs="$(lipo -archs "${binary}" 2>/dev/null)"; then
        echo "error: not a Mach-O binary: ${binary}" >&2
        exit 1
    fi
    for arch in ${ARCHS}; do
        if [[ " ${binary_archs} " != *" ${arch} "* ]]; then
            echo "error: ${binary} has architectures '${binary_archs}' but the build needs '${ARCHS}'." >&2
            exit 1
        fi
    done
}

if [[ -n "${ASHIFT_BUNDLED_PANDOC_PATH:-}" ]]; then
    if [[ ! -x "${ASHIFT_BUNDLED_PANDOC_PATH}" || -d "${ASHIFT_BUNDLED_PANDOC_PATH}" ]]; then
        echo "error: ASHIFT_BUNDLED_PANDOC_PATH is not an executable file: ${ASHIFT_BUNDLED_PANDOC_PATH}" >&2
        exit 1
    fi
    require_archs "${ASHIFT_BUNDLED_PANDOC_PATH}"
    cp -f "${ASHIFT_BUNDLED_PANDOC_PATH}" "${DEST}"
else
    SLICES=()
    for arch in ${ARCHS}; do
        slice="${REPO_ROOT}/.tools/pandoc-macos-${arch}/bin/pandoc"
        if [[ ! -x "${slice}" ]]; then
            echo "error: the pinned Pandoc for ${arch} is missing: ${slice}. Run: bash scripts/install-pandoc.sh --macos-arch ${arch}" >&2
            exit 1
        fi
        SLICES+=("${slice}")
    done
    if [[ ${#SLICES[@]} -eq 1 ]]; then
        cp -f "${SLICES[0]}" "${DEST}"
    else
        lipo -create "${SLICES[@]}" -output "${DEST}"
    fi
    require_archs "${DEST}"
fi

chmod 755 "${DEST}"

# The licence notices travel with the binary. A missing one fails the build rather than shipping Pandoc without them.
mkdir -p "${NOTICES_DEST}"
for notice in "${NOTICE_FILES[@]}"; do
    if [[ ! -s "${NOTICES_SRC}/${notice}" ]]; then
        echo "error: Pandoc notice file missing or empty: ${NOTICES_SRC}/${notice}" >&2
        exit 1
    fi
    cp -f "${NOTICES_SRC}/${notice}" "${NOTICES_DEST}/${notice}"
done

# Nested code needs its own hardened-runtime signature, and joining slices with lipo invalidates the signatures the
# slices came with (an arm64 slice with no valid signature does not run). Same rule as embed-helper.sh: no secure
# timestamp for ad-hoc signing, which cannot get one. Pandoc gets no entitlements.
if [[ "${CODE_SIGNING_ALLOWED:-NO}" == "YES" && -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]]; then
    if [[ "${EXPANDED_CODE_SIGN_IDENTITY}" == "-" ]]; then
        timestamp_flag="--timestamp=none"
    else
        timestamp_flag="--timestamp"
    fi
    codesign --force --options runtime "${timestamp_flag}" \
        --sign "${EXPANDED_CODE_SIGN_IDENTITY}" "${DEST}"
fi
