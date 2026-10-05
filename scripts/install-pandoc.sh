#!/bin/sh
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
system=$(uname -s)
machine=$(uname -m)

case "$system" in
    Linux)
        case "$machine" in
            x86_64|amd64)
                asset='pandoc-3.12-linux-amd64.tar.gz'
                checksum='67d7d011fed8c8543306022b985b9b2499ab9b74818df91d8727c7e9ebc5ba06'
                binary_name='pandoc'
                ;;
            aarch64|arm64)
                asset='pandoc-3.12-linux-arm64.tar.gz'
                checksum='6cefcf7100e23a99447c26f89d1ff5b253f3407fcef99a9e27ae06f3ed16cb82'
                binary_name='pandoc'
                ;;
            *)
                printf 'Unsupported Linux architecture: %s\n' "$machine" >&2
                exit 1
                ;;
        esac
        ;;
    Darwin)
        case "$machine" in
            arm64|aarch64)
                asset='pandoc-3.12-arm64-macOS.zip'
                checksum='f148ca09c9f36594db527a9fc988ad736290ce428f79594c50208cd1ec58b3c0'
                binary_name='pandoc'
                ;;
            x86_64|amd64)
                asset='pandoc-3.12-x86_64-macOS.zip'
                checksum='18577f9460c3dc5d2651ad3bab37d513bc2034a5a777fbe18fa0a5acf2e936ea'
                binary_name='pandoc'
                ;;
            *)
                printf 'Unsupported macOS architecture: %s\n' "$machine" >&2
                exit 1
                ;;
        esac
        ;;
    MINGW*|MSYS*|CYGWIN*)
        case "$machine" in
            x86_64|amd64)
                asset='pandoc-3.12-windows-x86_64.zip'
                checksum='2a77ebc2517d13e95056e76b1cd5b574cfe958ac61aa6058117d80c22ca19b79'
                binary_name='pandoc.exe'
                ;;
            *)
                printf 'Unsupported Windows architecture: %s\n' "$machine" >&2
                exit 1
                ;;
        esac
        ;;
    *)
        printf 'Unsupported operating system: %s\n' "$system" >&2
        exit 1
        ;;
esac

tools_dir="$repo_root/.tools"
archive="$tools_dir/$asset"
url="https://github.com/jgm/pandoc/releases/download/3.12/$asset"
mkdir -p "$tools_dir"

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        printf '%s\n' 'sha256sum or shasum is required to verify Pandoc.' >&2
        return 1
    fi
}

verify_archive() {
    actual=$(sha256_file "$1") || return 1
    [ "$actual" = "$checksum" ]
}

if [ -f "$archive" ] && ! verify_archive "$archive"; then
    printf 'Cached Pandoc archive failed SHA-256; downloading a fresh copy.\n' >&2
    rm -f "$archive"
fi

if [ ! -f "$archive" ]; then
    download="$archive.download"
    rm -f "$download"
    printf 'Downloading Pandoc 3.12 for %s/%s.\n' "$system" "$machine"
    curl -fsSL --retry 3 "$url" -o "$download"
    mv "$download" "$archive"
fi

if ! verify_archive "$archive"; then
    rm -f "$archive"
    printf 'Pandoc SHA-256 mismatch for %s.\n' "$asset" >&2
    exit 1
fi

temp_dir=$(mktemp -d "$tools_dir/.pandoc-3.12.XXXXXX")
cleanup() {
    rm -rf "$temp_dir"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM

case "$asset" in
    *.tar.gz)
        tar -xzf "$archive" -C "$temp_dir"
        ;;
    *.zip)
        if command -v unzip >/dev/null 2>&1; then
            unzip -q "$archive" -d "$temp_dir"
        elif command -v 7z >/dev/null 2>&1; then
            7z x -y "-o$temp_dir" "$archive" >/dev/null
        else
            printf '%s\n' 'unzip or 7z is required to extract this Pandoc archive.' >&2
            exit 1
        fi
        ;;
    *)
        printf 'Unsupported Pandoc archive: %s\n' "$asset" >&2
        exit 1
        ;;
esac

binary=$(find "$temp_dir" -type f -name "$binary_name" -print | sed -n '1p')
if [ -z "$binary" ]; then
    printf 'Could not find %s in the Pandoc archive.\n' "$binary_name" >&2
    exit 1
fi

bin_dir="$tools_dir/pandoc/bin"
destination="$bin_dir/$binary_name"
mkdir -p "$bin_dir"
mv -f "$binary" "$destination"
if [ "$binary_name" = 'pandoc' ]; then
    chmod +x "$destination"
fi

printf 'Installed Pandoc 3.12 at %s\n' "$destination"
