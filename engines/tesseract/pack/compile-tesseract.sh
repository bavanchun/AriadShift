#!/usr/bin/env bash
# Build the pinned source archives without optional codecs or network libraries.
set -euo pipefail
[ "$#" -eq 3 ] || { echo 'usage: compile-tesseract.sh TESSERACT_TAR LEPTONICA_TAR OUT' >&2; exit 1; }
tesseract=$1 leptonica=$2 out=$3
verify() {
    local expected=$1 path=$2 actual
    if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$path"); else actual=$(shasum -a 256 "$path"); fi
    [ "${actual%% *}" = "$expected" ] || { echo "hash mismatch: $path" >&2; exit 1; }
}
verify 445603ab1d0a63df552e193a600d9d5c4021aff36e144f3091af149e5efb381a "$tesseract"
verify c73363397f96eb1295602bf44d708a994ad42046c791bf03ea0505d829bdb6a7 "$leptonica"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar -xzf "$tesseract" -C "$work"
tar -xzf "$leptonica" -C "$work"
prefix=$work/prefix
export SOURCE_DATE_EPOCH=1735689600
export ZERO_AR_DATE=1
flags="-ffile-prefix-map=$work=/usr/src/ariad-tesseract -fdebug-prefix-map=$work=/usr/src/ariad-tesseract"
cmake -S "$work/leptonica-1.87.0" -B "$work/lept-out" \
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_C_FLAGS="$flags" -DBUILD_SHARED_LIBS=OFF -DBUILD_PROG=OFF \
    -DENABLE_ZLIB=OFF -DENABLE_PNG=OFF -DENABLE_GIF=OFF -DENABLE_JPEG=OFF \
    -DENABLE_TIFF=OFF -DENABLE_WEBP=OFF -DENABLE_OPENJPEG=OFF
cmake --build "$work/lept-out" -j "${TESSERACT_BUILD_JOBS:-2}"
cmake --install "$work/lept-out"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"
cmake -S "$work/tesseract-7e548e6eff172f38f97664afe7f7ce40db36c8d6" -B "$work/tess-out" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="$prefix" \
    -DCMAKE_C_FLAGS="$flags" -DCMAKE_CXX_FLAGS="$flags" \
    -DBUILD_SHARED_LIBS=OFF -DBUILD_TRAINING_TOOLS=OFF -DBUILD_TESTS=OFF \
    -DOPENMP_BUILD=OFF -DGRAPHICS_DISABLED=ON -DDISABLE_TIFF=ON \
    -DDISABLE_ARCHIVE=ON -DDISABLE_CURL=ON -DENABLE_NATIVE=OFF \
    -DENABLE_PRECOMPILED_HEADERS=OFF -DENABLE_CCACHE=OFF
cmake --build "$work/tess-out" --target tesseract -j "${TESSERACT_BUILD_JOBS:-2}"
binary=$work/tess-out/bin/tesseract
if [ "$(uname -s)" = Darwin ]; then
    dependencies=$(otool -L "$binary" | tail -n +2)
    printf '%s\n' "$dependencies" | awk 'NF && $1 !~ /^\/usr\/lib\/(libSystem\.B\.dylib|libc\+\+\.1\.dylib)$/ { exit 1 }' || {
        echo "unexpected dynamic dependencies: $dependencies" >&2; exit 1;
    }
else
    dependencies=$(readelf -d "$binary")
    printf '%s\n' "$dependencies" | bash "$(dirname "$0")/check-linux-dependencies.sh" "$(uname -m)" || {
        echo "unexpected dynamic dependencies: $dependencies" >&2; exit 1;
    }
fi
mkdir -p "$out/bin"
cp "$binary" "$out/bin/tesseract"
strip "$out/bin/tesseract"
"$out/bin/tesseract" --version
