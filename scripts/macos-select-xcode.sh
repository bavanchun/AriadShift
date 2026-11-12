#!/usr/bin/env bash
set -euo pipefail

# Select the one pinned Xcode for macOS CI and fail loudly when it is missing or reports another version.
# No fallback: an unexpected Xcode must stop the job, not silently build with a different toolchain.
# Overrides (tests and local runs): XCODE_APP, XCODE_VERSION, XCODE_BUILD.

XCODE_APP="${XCODE_APP:-/Applications/Xcode_27.2.app}"
XCODE_VERSION="${XCODE_VERSION:-27.2}"
XCODE_BUILD="${XCODE_BUILD:-27B52}"

if [[ ! -d "${XCODE_APP}" ]]; then
    echo "::error::${XCODE_APP} is not installed on this runner image." >&2
    ls -d /Applications/Xcode*.app >&2 || true
    exit 1
fi

developer_dir="${XCODE_APP}/Contents/Developer"
if [[ "$(xcode-select -p)" != "${developer_dir}" ]]; then
    sudo xcode-select -s "${developer_dir}"
fi

reported="$(xcodebuild -version)"
echo "${reported}"
if ! grep -qx "Xcode ${XCODE_VERSION}" <<< "${reported}" || ! grep -qx "Build version ${XCODE_BUILD}" <<< "${reported}"; then
    echo "::error::Expected Xcode ${XCODE_VERSION} (build ${XCODE_BUILD}) from ${XCODE_APP}." >&2
    exit 1
fi
swift --version
