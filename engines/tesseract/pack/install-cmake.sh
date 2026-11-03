#!/usr/bin/env bash
# Install the reviewed CMake release in a caller-owned CI directory.
set -euo pipefail
[ "$#" -eq 1 ] || { echo 'usage: install-cmake.sh DEST' >&2; exit 1; }
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) asset=linux-x86_64; hash=ed85135c5a5135b45e50423cb406dea04ca3338da0b8ef3c9ede963559680be0 ;;
    Linux-aarch64|Linux-arm64) asset=linux-aarch64; hash=301d5063ab259577c1b57ce5490b1aab9ea5c56dc4c822f73499553dab94925f ;;
    Darwin-*) asset=macos-universal; hash=679e77a96b9ef9484cc6c1445255fc4f84206b48a65420efbedf16bdab55fbff ;;
    *) echo 'unsupported CMake host' >&2; exit 1 ;;
esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
archive=$work/cmake.tar.gz
curl -fsSL --retry 3 --max-time 300 --proto '=https' --proto-redir '=https' --tlsv1.2 \
    "https://github.com/Kitware/CMake/releases/download/v4.4.5/cmake-4.4.5-$asset.tar.gz" -o "$archive"
if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$archive"); else actual=$(shasum -a 256 "$archive"); fi
[ "${actual%% *}" = "$hash" ] || { echo 'CMake archive hash mismatch' >&2; exit 1; }
mkdir -p "$1"
tar -xzf "$archive" -C "$1" --strip-components=1
