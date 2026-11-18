#!/usr/bin/env bash
set -euo pipefail

# Build universal XCFramework and Swift bindings for ariad-ffi.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH='' cd -- "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
OUT_XCFRAMEWORK="${1:-${TARGET_DIR}/xcframework/AriadShiftFFI.xcframework}"
PARENT_DIR="$(dirname "${OUT_XCFRAMEWORK}")"
GENERATED_DIR="${PARENT_DIR}/Generated"

echo "==> Verifying rust target: x86_64-apple-darwin"
if ! rustup target list --installed | grep -q "^x86_64-apple-darwin$"; then
    echo "Notice: x86_64-apple-darwin target is missing. Running 'rustup target add x86_64-apple-darwin' to enable universal dual-slice macOS builds..."
    rustup target add x86_64-apple-darwin
fi

export MACOSX_DEPLOYMENT_TARGET=15.0

# One cargo invocation builds both slices, so each crate compiles once per target and the host-side build scripts
# are shared. When ASHIFT_HELPER_OUT is set, the same invocation also builds the ashift helper (ariad-cli), so the
# dependency graph is not compiled a second time later; the helper slices are merged into that path with lipo.
CARGO_ARGS=(build --release -p ariad-ffi --target aarch64-apple-darwin --target x86_64-apple-darwin)
if [[ -n "${ASHIFT_HELPER_OUT:-}" ]]; then
    CARGO_ARGS+=(-p ariad-cli --locked)
fi
echo "==> Building release slices: aarch64-apple-darwin x86_64-apple-darwin"
cargo "${CARGO_ARGS[@]}"

echo "==> Creating universal binary with lipo"
UNIVERSAL_DIR="${TARGET_DIR}/universal/release"
mkdir -p "${UNIVERSAL_DIR}"
lipo -create \
    "${TARGET_DIR}/aarch64-apple-darwin/release/libariad_ffi.a" \
    "${TARGET_DIR}/x86_64-apple-darwin/release/libariad_ffi.a" \
    -output "${UNIVERSAL_DIR}/libariad_ffi.a"

if [[ -n "${ASHIFT_HELPER_OUT:-}" ]]; then
    echo "==> Creating universal ashift helper with lipo: ${ASHIFT_HELPER_OUT}"
    mkdir -p "$(dirname "${ASHIFT_HELPER_OUT}")"
    lipo -create \
        "${TARGET_DIR}/aarch64-apple-darwin/release/ashift" \
        "${TARGET_DIR}/x86_64-apple-darwin/release/ashift" \
        -output "${ASHIFT_HELPER_OUT}"
fi

echo "==> Generating Swift bindings and C headers"
TEMP_BINDINGS="$(mktemp -d)"
trap 'rm -rf "${TEMP_BINDINGS}"' EXIT

# The release build above already produced uniffi-bindgen for both targets; run the one that matches this host
# instead of compiling the dependency graph again in the dev profile.
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
"${TARGET_DIR}/${HOST_TRIPLE}/release/uniffi-bindgen" generate \
    "${UNIVERSAL_DIR}/libariad_ffi.a" \
    --language swift \
    --out-dir "${TEMP_BINDINGS}"

HEADERS_DIR="${TEMP_BINDINGS}/headers"
mkdir -p "${HEADERS_DIR}"
cp "${TEMP_BINDINGS}/ariad_ffiFFI.h" "${HEADERS_DIR}/ariad_ffiFFI.h"
cp "${TEMP_BINDINGS}/ariad_ffiFFI.modulemap" "${HEADERS_DIR}/module.modulemap"

echo "==> Packaging XCFramework into ${OUT_XCFRAMEWORK}"
if [[ "${OUT_XCFRAMEWORK}" != *.xcframework ]]; then
    echo "Error: Output path must end in .xcframework: ${OUT_XCFRAMEWORK}" >&2
    exit 1
fi
rm -rf "${OUT_XCFRAMEWORK}"
mkdir -p "${PARENT_DIR}"

xcodebuild -create-xcframework \
    -library "${UNIVERSAL_DIR}/libariad_ffi.a" \
    -headers "${HEADERS_DIR}" \
    -output "${OUT_XCFRAMEWORK}"

echo "==> Saving generated Swift file to ${GENERATED_DIR}/ariad_ffi.swift"
mkdir -p "${GENERATED_DIR}"
cp "${TEMP_BINDINGS}/ariad_ffi.swift" "${GENERATED_DIR}/ariad_ffi.swift"

echo "==> XCFramework build complete:"
echo "    XCFramework: ${OUT_XCFRAMEWORK}"
echo "    Swift file:  ${GENERATED_DIR}/ariad_ffi.swift"
