#!/usr/bin/env bash
set -euo pipefail

# Copy the ariad-ffi XCFramework and generated UniFFI Swift file into the
# AriadShiftCore package. Build them first with `just macos-ffi`.

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/../../.." && pwd)"
# A relative CARGO_TARGET_DIR is relative to the repository root, like build-xcframework.sh.
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
[[ "${TARGET_DIR}" = /* ]] || TARGET_DIR="${REPO_ROOT}/${TARGET_DIR}"
SRC_DIR="${1:-${TARGET_DIR}/xcframework}"
PACKAGE_DIR="${SCRIPT_DIR}/../AriadShiftCore"

XCFRAMEWORK="${SRC_DIR}/AriadShiftFFI.xcframework"
GENERATED="${SRC_DIR}/Generated/ariad_ffi.swift"

for path in "${XCFRAMEWORK}" "${GENERATED}"; do
    if [[ ! -e "${path}" ]]; then
        echo "Error: ${path} is missing. Run 'just macos-ffi' first." >&2
        exit 1
    fi
done

mkdir -p "${PACKAGE_DIR}/Frameworks" "${PACKAGE_DIR}/Sources/AriadShiftCore/Generated"
rm -rf "${PACKAGE_DIR}/Frameworks/AriadShiftFFI.xcframework"
cp -R "${XCFRAMEWORK}" "${PACKAGE_DIR}/Frameworks/AriadShiftFFI.xcframework"
cp "${GENERATED}" "${PACKAGE_DIR}/Sources/AriadShiftCore/Generated/ariad_ffi.swift"

echo "Synced ${XCFRAMEWORK} and ${GENERATED} into ${PACKAGE_DIR}"
