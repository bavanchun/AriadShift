#!/usr/bin/env bash
# Assemble an unsigned development pack from a prepared Linux qpdf directory.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH='' cd -- "$here/../../.." && pwd)
die() { printf 'qpdf-pack: %s\n' "$*" >&2; exit 1; }
slug() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) arch=x64 ;;
        Linux-aarch64|Linux-arm64) arch=arm64 ;;
        *) die 'only Linux development assembly is currently reviewed' ;;
    esac
    libc=''
    if ldd --version 2>&1 | grep -qi musl; then libc=musl-; fi
    printf 'linux-%s%s\n' "$libc" "$arch"
}
case ${1:-} in
    verify-archive)
        [ "$#" -eq 3 ] || die 'usage: make.sh verify-archive SLUG FILE'
        match=$(awk -v target="$2" '$1 == target { print $4; count++ } END { if (count != 1) exit 1 }' "$here/targets.txt") || die "unknown target '$2'"
        [ -f "$3" ] || die "missing archive '$3'"
        if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$3"); else actual=$(shasum -a 256 "$3"); fi
        [ "${actual%% *}" = "$match" ] || die 'archive SHA-256 mismatch'
        ;;
    slug) slug ;;
    dev)
        [ "$#" -eq 4 ] || die 'usage: make.sh dev QPDF_DIR ASHIFT OUT'
        dir=$2 ashift=$3 dest=$4
        [ -x "$dir/bin/qpdf" ] || die 'qpdf executable is missing'
        if [ -d "$dir/lib" ] && [ -n "$(ls -A "$dir/lib")" ]; then
            die 'development input contains unreviewed shared libraries'
        fi
        for notice in qpdf-LICENSE.txt qpdf-NOTICE.txt libjpeg-turbo-LICENSE.txt libjpeg-turbo-README.ijg.txt zlib-README.txt; do
            [ -s "$here/notices/$notice" ] || die "notice $notice is missing"
        done
        host=$(slug)
        read -r _ triple _ _ < <(awk -v target="$host" '$1 == target { print; exit }' "$here/targets.txt")
        case "$host" in
            *x64) pattern='x86-64|x86_64' ;;
            *arm64) pattern='aarch64|ARM aarch64' ;;
        esac
        file -b "$dir/bin/qpdf" | grep -Eqi "$pattern" || die "qpdf architecture does not match $host"
        bash "$repo/scripts/pack/build-pack.sh" build --name qpdf --version 0.1.0 \
            --triple "$triple" --ashift "$ashift" --out "$dest-dist" \
            --description 'qpdf document engine' \
            --license 'Apache-2.0 AND IJG AND BSD-3-Clause AND Zlib' \
            --requires-ashift '>=0.3.0, <2.0.0' --engine-protocol ariad-engine/2 \
            --entrypoint bin/qpdf --engine "$dir/bin/qpdf" --engine-dest bin/qpdf \
            --notices "$here/notices" --unpack-dir "$dest"
        ;;
    *) die 'usage: make.sh slug | verify-archive SLUG FILE | dev QPDF_DIR ASHIFT OUT' ;;
esac
