#!/usr/bin/env bash
# Build qpdf with static zlib/libjpeg and only qpdf's native crypto provider.
# Arguments are verified qpdf, libjpeg-turbo, zlib tarballs and output directory.
set -euo pipefail
[ "$#" -eq 4 ] || { echo 'usage: compile-qpdf.sh QPDF_TAR JPEG_TAR ZLIB_TAR OUT' >&2; exit 1; }
qpdf=$1 jpeg=$2 zlib=$3 out=$4
verify() {
    local expected=$1 path=$2 actual
    if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$path"); else actual=$(shasum -a 256 "$path"); fi
    [ "${actual%% *}" = "$expected" ] || { echo "hash mismatch: $path" >&2; exit 1; }
}
verify 8a58af5b6141319287c1883bec8bd1bd545b7567b7fc5e6ce5d25a1c85f36397 "$qpdf"
verify c27ad72a68da151177929aa969ae773a2da5ac2af9ce946079314fdac8cece7f "$jpeg"
verify bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16 "$zlib"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for archive in "$qpdf" "$jpeg" "$zlib"; do tar -xzf "$archive" -C "$work"; done
prefix=$work/prefix
cmake -S "$work/zlib-1.3.2" -B "$work/zlib-out" -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_BUILD_TYPE=Release
cmake --build "$work/zlib-out" -j "${QPDF_BUILD_JOBS:-2}"
cmake --install "$work/zlib-out"
rm -f "$prefix"/lib/libz.so* "$prefix"/lib/libz*.dylib
cmake -S "$work/libjpeg-turbo-3.2.1" -B "$work/jpeg-out" -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_BUILD_TYPE=Release -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DWITH_TURBOJPEG=OFF -DWITH_SIMD=OFF
cmake --build "$work/jpeg-out" -j "${QPDF_BUILD_JOBS:-2}"
cmake --install "$work/jpeg-out"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"
linker_flags="-L$prefix/lib"
if [ "$(uname -s)" = Linux ]; then linker_flags="$linker_flags -static-libstdc++ -static-libgcc"; fi
cmake -S "$work/qpdf-12.4.2" -B "$work/qpdf-out" -DCMAKE_BUILD_TYPE=Release -DUSE_IMPLICIT_CRYPTO=OFF -DREQUIRE_CRYPTO_NATIVE=ON -DBUILD_STATIC_LIBS=ON -DBUILD_SHARED_LIBS=OFF -DSTATIC_JPEG=ON -DCMAKE_EXE_LINKER_FLAGS="$linker_flags"
cmake --build "$work/qpdf-out" --target qpdf -j "${QPDF_BUILD_JOBS:-2}"
bin=$work/qpdf-out/qpdf/qpdf
[ "$("$bin" --show-crypto)" = native ] || { echo 'unexpected crypto provider' >&2; exit 1; }
if [ "$(uname -s)" = Darwin ]; then
    deps=$(otool -L "$bin" | tail -n +2)
    if ! printf '%s\n' "$deps" | awk 'NF && $1 !~ /^\/usr\/lib\/(libSystem\.B\.dylib|libc\+\+\.1\.dylib)$/ { exit 1 }'; then
        echo "unexpected dynamic dependencies: $deps" >&2; exit 1
    fi
else
    deps=$(readelf -d "$bin")
    if ! printf '%s\n' "$deps" | awk '
     /\(NEEDED\)/ && $0 !~ /\[(libc\.so(\.6)?|libc\.musl-[^]]*|libm\.so(\.6)?|libpthread\.so(\.0)?|ld-linux[^]]*|ld-musl[^]]*)\]/ { exit 1 }
    '; then echo "unexpected dynamic dependencies: $deps" >&2; exit 1; fi
fi
mkdir -p "$out/bin"
cp "$bin" "$out/bin/qpdf"
