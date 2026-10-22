#!/bin/sh
# Installs the pinned wasm-bindgen CLI into .tools/bin.
# Its version must equal the wasm-bindgen crate in Cargo.lock. Bump both together and
# replace every checksum with the values from the release's .sha256sum assets.
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
version='0.2.129'

case "$(uname -s)/$(uname -m)" in
    Darwin/arm64|Darwin/aarch64)
        target='aarch64-apple-darwin'
        checksum='81d4a23d56b3c3eb8187658329116d50e0b228a93b343825fb71f70179051cd1'
        exe=''
        ;;
    Darwin/x86_64)
        target='x86_64-apple-darwin'
        checksum='7e028879a68ec53dae14048b55cc5215f6df75b17a9c1426fac8fa086d3202fd'
        exe=''
        ;;
    Linux/x86_64|Linux/amd64)
        target='x86_64-unknown-linux-musl'
        checksum='82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e'
        exe=''
        ;;
    Linux/aarch64|Linux/arm64)
        target='aarch64-unknown-linux-musl'
        checksum='2ed4351c35dd9440308bbb02767d47ea278efe851a52465300f3c94f5b6c2a87'
        exe=''
        ;;
    MINGW*/x86_64|MSYS*/x86_64|CYGWIN*/x86_64)
        target='x86_64-pc-windows-msvc'
        checksum='79e348c169d0c10c0b287647f1e753d0985353ca96800bc29be8f4261baf13e6'
        exe='.exe'
        ;;
    *)
        printf 'Unsupported platform: %s %s\n' "$(uname -s)" "$(uname -m)" >&2
        exit 1
        ;;
esac

locked=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$repo_root/Cargo.lock")
if [ "$locked" != "$version" ]; then
    printf 'Cargo.lock pins wasm-bindgen %s but this script installs %s.\n' "$locked" "$version" >&2
    exit 1
fi

tools_dir="$repo_root/.tools"
bin_dir="$tools_dir/bin"
destination="$bin_dir/wasm-bindgen$exe"
stamp="$bin_dir/.wasm-bindgen.version"
if [ -x "$destination" ] && [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$version" ]; then
    exit 0
fi
mkdir -p "$bin_dir"

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

asset="wasm-bindgen-$version-$target.tar.gz"
archive="$tools_dir/$asset"
if [ ! -f "$archive" ] || [ "$(sha256_file "$archive")" != "$checksum" ]; then
    printf 'Downloading wasm-bindgen %s.\n' "$version"
    curl -fsSL --retry 3 \
        "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$version/$asset" \
        -o "$archive.download"
    mv "$archive.download" "$archive"
fi
if [ "$(sha256_file "$archive")" != "$checksum" ]; then
    rm -f "$archive"
    printf 'wasm-bindgen SHA-256 mismatch for %s.\n' "$asset" >&2
    exit 1
fi

temp_dir=$(mktemp -d "$tools_dir/.wasm-bindgen.XXXXXX")
tar -xzf "$archive" -C "$temp_dir"
mv -f "$temp_dir/wasm-bindgen-$version-$target/wasm-bindgen$exe" "$destination"
chmod +x "$destination"
rm -rf "$temp_dir"
printf '%s\n' "$version" > "$stamp"
printf 'Installed wasm-bindgen %s at %s\n' "$version" "$destination"
