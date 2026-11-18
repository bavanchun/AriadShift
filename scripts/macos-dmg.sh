#!/usr/bin/env bash
set -euo pipefail

# Create a macOS DMG installer containing AriadShift.app and an Applications symlink.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH='' cd -- "${SCRIPT_DIR}/.." && pwd)"

DERIVED_DATA="${MACOS_DERIVED_DATA:-${DERIVED_DATA_DIR:-${REPO_ROOT}/target/derived-data}}"

# Resolve application bundle path
APP_PATH="${1:-}"
if [[ -z "${APP_PATH}" ]]; then
    if [[ -d "${DERIVED_DATA}/Build/Products/Release/AriadShift.app" ]]; then
        APP_PATH="${DERIVED_DATA}/Build/Products/Release/AriadShift.app"
    elif [[ -d "${REPO_ROOT}/target/release/macos/AriadShift.app" ]]; then
        APP_PATH="${REPO_ROOT}/target/release/macos/AriadShift.app"
    elif [[ -d "${REPO_ROOT}/target/release/AriadShift.app" ]]; then
        APP_PATH="${REPO_ROOT}/target/release/AriadShift.app"
    else
        echo "error: AriadShift.app not found. Run 'just macos-build' first or pass app path as \$1." >&2
        exit 1
    fi
fi

if [[ ! -d "${APP_PATH}" || "${APP_PATH}" != *.app ]]; then
    echo "error: '${APP_PATH}' is not a valid .app bundle directory." >&2
    exit 1
fi

# Determine version for default DMG name
VERSION="0.0.0"
if [[ -f "${APP_PATH}/Contents/Info.plist" ]]; then
    PLIST_VER="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "${APP_PATH}/Contents/Info.plist" 2>/dev/null || true)"
    if [[ -n "${PLIST_VER}" && "${PLIST_VER}" != *"MARKETING_VERSION"* ]]; then
        VERSION="${PLIST_VER}"
    fi
fi

OUTPUT_DMG="${2:-${REPO_ROOT}/target/release/AriadShift-${VERSION}-universal.dmg}"
mkdir -p "$(dirname "${OUTPUT_DMG}")"
rm -f "${OUTPUT_DMG}"

echo "==> Packaging DMG from: ${APP_PATH}"
echo "==> Output destination: ${OUTPUT_DMG}"

STAGE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ariadshift-dmg.XXXXXX")"
trap 'rm -rf "${STAGE_DIR}"' EXIT

ditto "${APP_PATH}" "${STAGE_DIR}/AriadShift.app"
ln -s /Applications "${STAGE_DIR}/Applications"

echo "==> Creating DMG image: ${OUTPUT_DMG}"
CREATED=0
for attempt in 1 2 3; do
    if hdiutil create \
        -volname "AriadShift" \
        -srcfolder "${STAGE_DIR}" \
        -ov \
        -format UDZO \
        "${OUTPUT_DMG}"; then
        CREATED=1
        break
    fi
    echo "warn: hdiutil create attempt $attempt failed; retrying in 2 seconds..." >&2
    sleep 2
done

if [[ "$CREATED" -ne 1 ]]; then
    echo "error: Failed to create DMG after 3 attempts: ${OUTPUT_DMG}" >&2
    exit 1
fi

echo "==> Verifying DMG..."
hdiutil verify "${OUTPUT_DMG}"

echo "==> Successfully created and verified DMG: ${OUTPUT_DMG}"
