#!/usr/bin/env bash
set -euo pipefail

# Xcode Run Script phase: embed the `ashift` CLI helper at
# <App>.app/Contents/Helpers/ashift.
#
# - ASHIFT_HELPER_PATH set: copy that binary (fast local builds, CI artifacts).
#   It must contain a slice for every arch in $ARCHS.
# - Otherwise: build `ariad-cli` with cargo for every arch in $ARCHS and lipo them.

REPO_ROOT="$(CDPATH= cd -- "${SRCROOT}/../.." && pwd)"
HELPERS_DIR="${TARGET_BUILD_DIR}/${CONTENTS_FOLDER_PATH}/Helpers"
DEST="${HELPERS_DIR}/ashift"
mkdir -p "${HELPERS_DIR}"

if [[ -n "${ASHIFT_HELPER_PATH:-}" ]]; then
    if [[ ! -x "${ASHIFT_HELPER_PATH}" || -d "${ASHIFT_HELPER_PATH}" ]]; then
        echo "error: ASHIFT_HELPER_PATH is not an executable file: ${ASHIFT_HELPER_PATH}" >&2
        exit 1
    fi
    if ! helper_archs="$(lipo -archs "${ASHIFT_HELPER_PATH}" 2>/dev/null)"; then
        echo "error: ASHIFT_HELPER_PATH is not a Mach-O binary: ${ASHIFT_HELPER_PATH}" >&2
        exit 1
    fi
    for arch in ${ARCHS}; do
        if [[ " ${helper_archs} " != *" ${arch} "* ]]; then
            echo "error: ASHIFT_HELPER_PATH has architectures '${helper_archs}' but the build needs '${ARCHS}': ${ASHIFT_HELPER_PATH}" >&2
            exit 1
        fi
    done
    cp -f "${ASHIFT_HELPER_PATH}" "${DEST}"
else
    export PATH="${HOME}/.cargo/bin:${PATH}"
    export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-15.0}"
    # A relative CARGO_TARGET_DIR is relative to the repository root, where cargo runs.
    TARGET_DIR="${CARGO_TARGET_DIR:-target}"
    [[ "${TARGET_DIR}" = /* ]] || TARGET_DIR="${REPO_ROOT}/${TARGET_DIR}"
    triple_for() {
        case "$1" in
            arm64) echo "aarch64-apple-darwin" ;;
            x86_64) echo "x86_64-apple-darwin" ;;
            *) echo "error: unsupported architecture $1" >&2; return 1 ;;
        esac
    }
    # Fail before any slow build when a needed rustup target is missing.
    for arch in ${ARCHS}; do
        triple="$(triple_for "${arch}")"
        if [[ ! -d "$(rustc --print target-libdir --target "${triple}")" ]]; then
            echo "error: Rust target ${triple} is not installed (needed for ARCHS='${ARCHS}'). Run: rustup target add ${triple}" >&2
            exit 1
        fi
    done
    SLICES=()
    for arch in ${ARCHS}; do
        triple="$(triple_for "${arch}")"
        (cd "${REPO_ROOT}" && CARGO_TARGET_DIR="${TARGET_DIR}" cargo build --locked --release -p ariad-cli --target "${triple}")
        SLICES+=("${TARGET_DIR}/${triple}/release/ashift")
    done
    if [[ ${#SLICES[@]} -eq 1 ]]; then
        cp -f "${SLICES[0]}" "${DEST}"
    else
        lipo -create "${SLICES[@]}" -output "${DEST}"
    fi
fi

chmod 755 "${DEST}"

# Nested code needs its own hardened-runtime signature. Notarization also requires a
# secure timestamp, which only a real identity can obtain; ad-hoc signing ("-") cannot.
if [[ "${CODE_SIGNING_ALLOWED:-NO}" == "YES" && -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]]; then
    if [[ "${EXPANDED_CODE_SIGN_IDENTITY}" == "-" ]]; then
        timestamp_flag="--timestamp=none"
    else
        timestamp_flag="--timestamp"
    fi
    codesign --force --options runtime "${timestamp_flag}" \
        --sign "${EXPANDED_CODE_SIGN_IDENTITY}" "${DEST}"
fi
