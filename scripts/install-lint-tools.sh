#!/bin/sh
# Installs the pinned workflow and commit linters into .tools/bin.
# Bump a tool by changing its version and every checksum from the release assets.
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
system=$(uname -s)
machine=$(uname -m)

actionlint_version='1.7.12'
committed_version='1.1.11'

case "$system" in
    Linux)
        case "$machine" in
            x86_64|amd64)
                actionlint_asset="actionlint_${actionlint_version}_linux_amd64.tar.gz"
                actionlint_checksum='8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8'
                committed_asset="committed-v${committed_version}-x86_64-unknown-linux-musl.tar.gz"
                committed_checksum='1e5c20049eeaa4633e6798283913f92dc90a97f2b5c6dfc28e8c4b7e77a67157'
                ;;
            aarch64|arm64)
                actionlint_asset="actionlint_${actionlint_version}_linux_arm64.tar.gz"
                actionlint_checksum='325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6'
                committed_asset="committed-v${committed_version}-aarch64-unknown-linux-musl.tar.gz"
                committed_checksum='96d6334ab2dccc0e90b67ebdcaa2af856f29eadef48161926e21a0e35f1ae80d'
                ;;
            *)
                printf 'Unsupported Linux architecture: %s\n' "$machine" >&2
                exit 1
                ;;
        esac
        exe=''
        ;;
    Darwin)
        case "$machine" in
            arm64|aarch64)
                actionlint_asset="actionlint_${actionlint_version}_darwin_arm64.tar.gz"
                actionlint_checksum='aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f'
                committed_asset="committed-v${committed_version}-aarch64-apple-darwin.tar.gz"
                committed_checksum='98eae08e1b0715ef6446c4bd08352f21f3a3ce21535a35687a1e132bcb4fcac1'
                ;;
            x86_64|amd64)
                actionlint_asset="actionlint_${actionlint_version}_darwin_amd64.tar.gz"
                actionlint_checksum='5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644'
                committed_asset="committed-v${committed_version}-x86_64-apple-darwin.tar.gz"
                committed_checksum='b1718d951c8a90539ffb7a81145fdadf81c6c9a1f17560be433a74c5b3f7bfca'
                ;;
            *)
                printf 'Unsupported macOS architecture: %s\n' "$machine" >&2
                exit 1
                ;;
        esac
        exe=''
        ;;
    MINGW*|MSYS*|CYGWIN*)
        case "$machine" in
            x86_64|amd64)
                actionlint_asset="actionlint_${actionlint_version}_windows_amd64.zip"
                actionlint_checksum='6e7241b51e6817ea6a047693d8e6fed13b31819c9a0dd6c5a726e1592d22f6e9'
                committed_asset="committed-v${committed_version}-x86_64-pc-windows-msvc.zip"
                committed_checksum='af4f5a65320471751ed1dfbb5502f15f4857cea797b077c6049acb2b995d293b'
                ;;
            *)
                printf 'Unsupported Windows architecture: %s\n' "$machine" >&2
                exit 1
                ;;
        esac
        exe='.exe'
        ;;
    *)
        printf 'Unsupported operating system: %s\n' "$system" >&2
        exit 1
        ;;
esac

tools_dir="$repo_root/.tools"
bin_dir="$tools_dir/bin"
mkdir -p "$bin_dir"

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        printf '%s\n' 'sha256sum or shasum is required to verify downloads.' >&2
        return 1
    fi
}

# install_tool NAME VERSION ASSET URL CHECKSUM
install_tool() {
    name=$1
    version=$2
    asset=$3
    url=$4
    checksum=$5
    destination="$bin_dir/$name$exe"
    stamp="$bin_dir/.$name.version"

    if [ -x "$destination" ] && [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$version" ]; then
        return 0
    fi

    archive="$tools_dir/$asset"
    if [ -f "$archive" ] && [ "$(sha256_file "$archive")" != "$checksum" ]; then
        rm -f "$archive"
    fi
    if [ ! -f "$archive" ]; then
        printf 'Downloading %s %s.\n' "$name" "$version"
        curl -fsSL --retry 3 "$url" -o "$archive.download"
        mv "$archive.download" "$archive"
    fi
    if [ "$(sha256_file "$archive")" != "$checksum" ]; then
        rm -f "$archive"
        printf '%s SHA-256 mismatch for %s.\n' "$name" "$asset" >&2
        exit 1
    fi

    temp_dir=$(mktemp -d "$tools_dir/.$name.XXXXXX")
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
                rm -rf "$temp_dir"
                printf '%s\n' 'unzip or 7z is required to extract this archive.' >&2
                exit 1
            fi
            ;;
    esac

    binary=$(find "$temp_dir" -type f -name "$name$exe" -print | sed -n '1p')
    if [ -z "$binary" ]; then
        rm -rf "$temp_dir"
        printf 'Could not find %s%s in %s.\n' "$name" "$exe" "$asset" >&2
        exit 1
    fi
    mv -f "$binary" "$destination"
    chmod +x "$destination"
    rm -rf "$temp_dir"
    printf '%s\n' "$version" > "$stamp"
    printf 'Installed %s %s at %s\n' "$name" "$version" "$destination"
}

install_tool actionlint "$actionlint_version" "$actionlint_asset" \
    "https://github.com/rhysd/actionlint/releases/download/v$actionlint_version/$actionlint_asset" \
    "$actionlint_checksum"
install_tool committed "$committed_version" "$committed_asset" \
    "https://github.com/crate-ci/committed/releases/download/v$committed_version/$committed_asset" \
    "$committed_checksum"
