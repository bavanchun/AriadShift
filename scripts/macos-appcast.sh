#!/usr/bin/env bash
set -euo pipefail

# Generate a Sparkle 2 appcast feed for macOS releases using pinned Sparkle tools.
# The EdDSA private key is read from SPARKLE_ED_PRIVATE_KEY (env) or stdin, NEVER argv.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH='' cd -- "${SCRIPT_DIR}/.." && pwd)"

# Check key presence; fail loudly if absent
KEY_INPUT="${SPARKLE_ED_PRIVATE_KEY:-}"
if [[ -z "${KEY_INPUT}" && ! -t 0 ]]; then
    # Read stdin with timeout to prevent hanging in non-interactive pipeline contexts
    if read -r -t 2 first_line; then
        rest="$(cat || true)"
        if [[ -n "${rest}" ]]; then
            KEY_INPUT="${first_line}"$'\n'"${rest}"
        else
            KEY_INPUT="${first_line}"
        fi
    fi
fi

if [[ -z "${KEY_INPUT}" ]]; then
    echo "======================================================================" >&2
    echo "ERROR / UNSIGNED NOTICE:" >&2
    echo "SPARKLE_ED_PRIVATE_KEY is not set." >&2
    echo "Sparkle appcast generation requires the EdDSA private key." >&2
    echo "======================================================================" >&2
    exit 1
fi

INPUT_TARGET="${1:-${REPO_ROOT}/target/release}"
OUTPUT_XML=""

STAGING_DIR=""
cleanup_staging() {
    if [[ -n "${STAGING_DIR}" && -d "${STAGING_DIR}" ]]; then
        rm -rf "${STAGING_DIR}"
    fi
}
trap cleanup_staging EXIT

if [[ -f "${INPUT_TARGET}" && "${INPUT_TARGET}" == *.dmg ]]; then
    # Staging dir containing ONLY the DMG
    STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/sparkle-staging.XXXXXX")"
    ditto "${INPUT_TARGET}" "${STAGING_DIR}/$(basename "${INPUT_TARGET}")"
    OUTPUT_XML="${2:-$(dirname "${INPUT_TARGET}")/appcast.xml}"
elif [[ -d "${INPUT_TARGET}" ]]; then
    STAGING_DIR="${INPUT_TARGET}"
    OUTPUT_XML="${2:-${STAGING_DIR}/appcast.xml}"
else
    echo "error: Target '${INPUT_TARGET}' is neither a .dmg file nor a directory." >&2
    exit 1
fi

SPARKLE_VERSION="2.10.1"
SPARKLE_SHA256="1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27"
SPARKLE_DIR="${SPARKLE_CACHE_DIR:-${REPO_ROOT}/target/sparkle-${SPARKLE_VERSION}}"
TARBALL="${SPARKLE_DIR}/Sparkle-${SPARKLE_VERSION}.tar.xz"
GENERATE_APPCAST="${SPARKLE_DIR}/bin/generate_appcast"

# Always verify tarball SHA-256 before extraction / execution
NEED_DOWNLOAD=1
if [[ -f "${TARBALL}" ]]; then
    CACHED_SHA256="$(shasum -a 256 "${TARBALL}" | awk '{print $1}')"
    if [[ "${CACHED_SHA256}" == "${SPARKLE_SHA256}" ]]; then
        NEED_DOWNLOAD=0
    else
        echo "warn: Cached Sparkle tarball SHA-256 mismatch. Re-downloading..." >&2
        rm -f "${TARBALL}"
    fi
fi

if [[ "${NEED_DOWNLOAD}" -eq 1 ]]; then
    echo "==> Fetching Sparkle ${SPARKLE_VERSION} release tools..."
    mkdir -p "${SPARKLE_DIR}"
    TMP_DL_DIR="$(mktemp -d "${TMPDIR:-/tmp}/sparkle-dl.XXXXXX")"
    DL_TARBALL="${TMP_DL_DIR}/Sparkle-${SPARKLE_VERSION}.tar.xz"
    SPARKLE_URL="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"

    curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "${DL_TARBALL}" "${SPARKLE_URL}"

    ACTUAL_SHA256="$(shasum -a 256 "${DL_TARBALL}" | awk '{print $1}')"
    if [[ "${ACTUAL_SHA256}" != "${SPARKLE_SHA256}" ]]; then
        echo "error: SHA-256 mismatch for Sparkle ${SPARKLE_VERSION} tarball:" >&2
        echo "  Expected: ${SPARKLE_SHA256}" >&2
        echo "  Actual:   ${ACTUAL_SHA256}" >&2
        rm -rf "${TMP_DL_DIR}"
        exit 1
    fi

    mv "${DL_TARBALL}" "${TARBALL}"
    rm -rf "${TMP_DL_DIR}"
    tar -xf "${TARBALL}" -C "${SPARKLE_DIR}"
    chmod +x "${SPARKLE_DIR}/bin/"*
elif [[ ! -x "${GENERATE_APPCAST}" ]]; then
    tar -xf "${TARBALL}" -C "${SPARKLE_DIR}"
    chmod +x "${SPARKLE_DIR}/bin/"*
fi

DOWNLOAD_PREFIX="${DOWNLOAD_URL_PREFIX:-https://github.com/${GITHUB_REPOSITORY:-bavanchun/AriadShift}/releases/download/${RELEASE_TAG:-v0.0.0}/}"

echo "==> Generating Sparkle appcast for: ${STAGING_DIR}"
echo "==> Enclosure URL prefix: ${DOWNLOAD_PREFIX}"
mkdir -p "$(dirname "${OUTPUT_XML}")"

# Pipe private key to generate_appcast through stdin to prevent leakage in argv or logs
printf '%s' "${KEY_INPUT}" | "${GENERATE_APPCAST}" \
    --ed-key-file - \
    --download-url-prefix "${DOWNLOAD_PREFIX}" \
    -o "${OUTPUT_XML}" \
    "${STAGING_DIR}"

echo "==> Successfully generated appcast: ${OUTPUT_XML}"
