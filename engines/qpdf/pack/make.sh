#!/usr/bin/env bash
# Assemble unsigned packs from prepared qpdf binaries.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH='' cd -- "$here/../../.." && pwd)
die() { printf 'qpdf-pack: %s\n' "$*" >&2; exit 1; }
slug() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) arch=x64 ;;
        Linux-aarch64|Linux-arm64) arch=arm64 ;;
        Darwin-arm64) echo mac-arm64; return ;;
        Darwin-x86_64) echo mac-x64; return ;;
        *) die 'unsupported host platform' ;;
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
    dev|pack)
        cmd=$1; shift
        if [ "$cmd" = pack ]; then
            [ "$#" -eq 4 ] || die 'usage: make.sh pack SLUG QPDF_DIR ASHIFT OUT'
            host=$1; shift
            runner=$(slug)
            case "$host:$runner" in
                "$runner:$runner"|linux-musl-x64:linux-x64|linux-musl-arm64:linux-arm64) ;;
                *) die 'pack target does not match the build host' ;;
            esac
        else
            [ "$#" -eq 3 ] || die 'usage: make.sh dev QPDF_DIR ASHIFT OUT'
            host=$(slug)
        fi
        dir=$1 ashift=$2 dest=$3
        [ -x "$dir/bin/qpdf" ] || die 'qpdf executable is missing'
        extras=() notice_dir=$here/notices license='Apache-2.0 AND IJG AND BSD-3-Clause AND Zlib AND GPL-3.0-or-later WITH GCC-exception-3.1'
        if [ -d "$dir/lib" ] && [ -n "$(ls -A "$dir/lib")" ]; then
            [ "$cmd" = dev ] || die 'release component contains unreviewed shared libraries'
            notice_dir=$(mktemp -d)
            trap 'rm -rf "$notice_dir"' EXIT
            cp "$here"/notices/* "$notice_dir/"
            printf '%s\n' 'Development-only binary dependencies are supplied by the user and are not reviewed for distribution.' >"$notice_dir/development-only.txt"
            license="$license AND LicenseRef-Development-Dependencies"
            extras+=(--extra "$dir/lib=lib")
        fi
        for notice in qpdf-LICENSE.txt qpdf-NOTICE.txt libjpeg-turbo-LICENSE.txt libjpeg-turbo-README.ijg.txt zlib-README.txt gcc-GPL-3.txt gcc-runtime-exception.txt; do
            [ -s "$here/notices/$notice" ] || die "notice $notice is missing"
        done
        read -r _ triple _ _ < <(awk -v target="$host" '$1 == target { print; exit }' "$here/targets.txt")
        case "$host" in
            *x64) pattern='x86-64|x86_64' ;;
            *arm64) pattern='aarch64|arm64' ;;
        esac
        file -b "$dir/bin/qpdf" | grep -Eqi "$pattern" || die "qpdf architecture does not match $host"
        case "$host" in
            linux-musl-*) file -b "$dir/bin/qpdf" | grep -qi musl || die 'binary is not linked for musl' ;;
        esac
        bash "$repo/scripts/pack/build-pack.sh" build --name qpdf --version 0.1.0 \
            --triple "$triple" --ashift "$ashift" --out "$dest-dist" \
            --description 'qpdf document engine' \
            --license "$license" \
            --requires-ashift '>=0.3.0, <2.0.0' --engine-protocol ariad-engine/2 \
            --entrypoint bin/qpdf --engine "$dir/bin/qpdf" --engine-dest bin/qpdf \
            ${extras[@]+"${extras[@]}"} --notices "$notice_dir" --unpack-dir "$dest"
        ;;
    merge)
        [ "$#" -eq 3 ] || die 'usage: make.sh merge DIST ASHIFT'
        dist=$2 ashift=$3
        archives=()
        while read -r target triple _; do
            case $target in ''|'#'*) continue ;; esac
            archive=$dist/qpdf-0.1.0-$triple-engine.tar.zst
            [ -f "$archive" ] || die "missing component for $target"
            archives+=(--archive "$triple=$archive")
        done <"$here/targets.txt"
        bash "$repo/scripts/pack/build-pack.sh" merge --name qpdf --version 0.1.0 \
            --ashift "$ashift" --out "$dist" --description 'qpdf document engine' \
            --license 'Apache-2.0 AND IJG AND BSD-3-Clause AND Zlib AND GPL-3.0-or-later WITH GCC-exception-3.1' \
            --requires-ashift '>=0.3.0, <2.0.0' --engine-protocol ariad-engine/2 \
            --entrypoint bin/qpdf "${archives[@]}"
        ;;
    verify)
        [ "$#" -eq 2 ] || die 'usage: make.sh verify PACK_DIR'
        root=$2
        [ -x "$root/bin/qpdf" ] || die 'qpdf executable is missing'
        grep -q '^name = "qpdf"$' "$root/pack.toml" || die 'incorrect pack name'
        grep -q '^engine_protocol = "ariad-engine/2"$' "$root/pack.toml" || die 'incorrect engine protocol'
        python3 - "$root/pack.toml" <<'PY'
import pathlib, sys, tomllib
data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
if not data.get('targets') or any(t.get('entrypoint') != ['bin/qpdf'] for t in data['targets']):
    sys.exit('qpdf-pack: invalid entrypoint')
PY
        for notice in qpdf-LICENSE.txt qpdf-NOTICE.txt libjpeg-turbo-LICENSE.txt libjpeg-turbo-README.ijg.txt zlib-README.txt gcc-GPL-3.txt gcc-runtime-exception.txt; do
            [ -s "$root/notices/$notice" ] || die "notice $notice is missing"
            grep -q "notices/$notice" "$root/pack.toml" || die "notice $notice is absent from the manifest"
        done
        ;;
    *) die 'usage: make.sh slug | verify-archive | dev | pack | merge | verify' ;;
esac
