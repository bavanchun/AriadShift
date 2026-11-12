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

echo "==> Building ariad-ffi release slice: aarch64-apple-darwin"
cargo build -p ariad-ffi --release --target aarch64-apple-darwin

echo "==> Building ariad-ffi release slice: x86_64-apple-darwin"
cargo build -p ariad-ffi --release --target x86_64-apple-darwin

echo "==> Creating universal binary with lipo"
UNIVERSAL_DIR="${TARGET_DIR}/universal/release"
mkdir -p "${UNIVERSAL_DIR}"
lipo -create \
    "${TARGET_DIR}/aarch64-apple-darwin/release/libariad_ffi.a" \
    "${TARGET_DIR}/x86_64-apple-darwin/release/libariad_ffi.a" \
    -output "${UNIVERSAL_DIR}/libariad_ffi.a"

echo "==> Generating Swift bindings and C headers"
TEMP_BINDINGS="$(mktemp -d)"
trap 'rm -rf "${TEMP_BINDINGS}"' EXIT

cargo run -p ariad-ffi --bin uniffi-bindgen -- generate \
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
