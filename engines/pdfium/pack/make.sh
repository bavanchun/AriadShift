#!/usr/bin/env bash
# Assembles the pdfium engine pack with the shared pack builder (scripts/pack/build-pack.sh).
#
#   make.sh slug                                   print the pack target slug of this host
#   make.sh pack --slug SLUG --engine PATH --ashift PATH --out DIR [--unpack-dir DIR]
#                [--no-codesign]                   build one target (component + pack.toml)
#   make.sh dev --engine PATH --ashift PATH --out DIR --unpack-dir DIR
#                                                  build the host target's unsigned development
#                                                  pack; DIR is a valid ASHIFT_PACK_PDFIUM
#   make.sh merge --dist DIR --ashift PATH         merge the components of every target found
#                                                  in DIR into DIR/pack.toml
#   make.sh verify --pack DIR                      check a development pack's layout, manifest
#                                                  and notices
#
# Environment: PDFIUM_ARCHIVE names a pre-downloaded upstream archive (it must still match
# the pinned SHA-256); PACK_REQUIRES_ASHIFT overrides the ashift requirement; SOURCE_DATE_EPOCH
# and APPLE_SIGNING_IDENTITY are passed on to the shared builder.
set -euo pipefail

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH='' cd -- "$here/../../.." && pwd)
builder=$repo/scripts/pack/build-pack.sh

die() {
    printf 'pdfium-pack: %s\n' "$*" >&2
    exit 1
}

requires_override=${PACK_REQUIRES_ASHIFT-}
# shellcheck source=engines/pdfium/pack/pack.conf
. "$here/pack.conf"
requires_ashift=${requires_override:-$PACK_REQUIRES_ASHIFT}

# Prints "triple asset sha256" for a slug; the SHA-256 may be empty.
lookup_slug() {
    local wanted=$1 slug triple asset sha
    while read -r slug triple asset sha; do
        case $slug in
        '' | '#'*) continue ;;
        esac
        if [ "$slug" = "$wanted" ]; then
            # The V8 builds embed a JavaScript engine; the pack ships only the inert build.
            case $asset in
            pdfium-v8-* | *-v8-*) die "asset '$asset' is a V8 build; the pack uses non-V8 assets only" ;;
            esac
            printf '%s %s %s\n' "$triple" "$asset" "$sha"
            return 0
        fi
    done <"$here/targets.txt"
    die "unknown pack target '$wanted'"
}

host_slug() {
    local system machine
    system=$(uname -s)
    machine=$(uname -m)
    case $system-$machine in
    Linux-x86_64 | Linux-aarch64 | Linux-arm64)
        local arch=x64 libc=''
        case $machine in aarch64 | arm64) arch=arm64 ;; esac
        if ldd --version 2>&1 | grep -qi musl; then libc=musl-; fi
        printf 'linux-%s%s\n' "$libc" "$arch"
        ;;
    Darwin-arm64) echo mac-arm64 ;;
    Darwin-x86_64) echo mac-x64 ;;
    *) die "no pdfium pack target for $system $machine" ;;
    esac
}

pack_one() {
    local slug='' engine='' ashift='' out='' unpack_dir='' codesign_flag=()
    while [ "$#" -gt 0 ]; do
        case $1 in
        --no-codesign)
            codesign_flag=(--no-codesign)
            shift
            continue
            ;;
        --slug | --engine | --ashift | --out | --unpack-dir) ;;
        *) die "unknown option '$1'" ;;
        esac
        [ "$#" -ge 2 ] || die "option $1 needs a value"
        case $1 in
        --slug) slug=$2 ;;
        --engine) engine=$2 ;;
        --ashift) ashift=$2 ;;
        --out) out=$2 ;;
        --unpack-dir) unpack_dir=$2 ;;
        esac
        shift 2
    done
    [ -n "$slug" ] && [ -n "$engine" ] && [ -n "$ashift" ] && [ -n "$out" ] ||
        die "--slug, --engine, --ashift and --out are required"

    local triple asset sha
    read -r triple asset sha < <(lookup_slug "$slug")

    local lib=libpdfium.so
    case $triple in *-apple-darwin) lib=libpdfium.dylib ;; esac

    local source_args=(--archive-sha256 "$sha")
    if [ -n "${PDFIUM_ARCHIVE:-}" ]; then
        source_args+=(--archive "$PDFIUM_ARCHIVE")
    else
        source_args+=(--archive-url "$PDFIUM_RELEASE_BASE/$asset")
    fi

    local extra_args=() file
    for file in LICENSE NOTICE THIRD_PARTY_LICENSES; do
        if [ -f "$repo/$file" ]; then
            extra_args+=(--extra "$repo/$file=notices/ariadshift-$file")
        fi
    done

    local unpack_args=()
    if [ -n "$unpack_dir" ]; then
        unpack_args=(--unpack-dir "$unpack_dir")
    fi

    bash "$builder" build \
        --name "$PACK_NAME" --version "$PACK_VERSION" --triple "$triple" \
        --ashift "$ashift" --out "$out" \
        --description "$PACK_DESCRIPTION" --license "$PACK_LICENSE" \
        --requires-ashift "$requires_ashift" \
        --engine-protocol "$PACK_ENGINE_PROTOCOL" \
        --entrypoint "bin/$PACK_ENGINE_BIN" \
        --engine "$engine" --engine-dest "bin/$PACK_ENGINE_BIN" \
        "${source_args[@]}" \
        --from-archive "lib/$lib=bin/$lib" \
        --from-archive "LICENSE=notices/pdfium-binaries-LICENSE" \
        --from-archive "licenses=notices/third-party" \
        --from-archive "VERSION=notices/pdfium-VERSION" \
        ${extra_args[@]+"${extra_args[@]}"} \
        --notices "$here/notices" \
        ${codesign_flag[@]+"${codesign_flag[@]}"} \
        ${unpack_args[@]+"${unpack_args[@]}"}
}

merge_all() {
    local dist='' ashift=''
    while [ "$#" -gt 0 ]; do
        case $1 in
        --dist | --ashift) ;;
        *) die "unknown option '$1'" ;;
        esac
        [ "$#" -ge 2 ] || die "option $1 needs a value"
        case $1 in
        --dist) dist=$2 ;;
        --ashift) ashift=$2 ;;
        esac
        shift 2
    done
    [ -n "$dist" ] && [ -n "$ashift" ] || die "--dist and --ashift are required"
    local archives=() slug triple asset sha file
    while read -r slug triple asset sha; do
        case $slug in
        '' | '#'*) continue ;;
        esac
        file=$dist/$PACK_NAME-$PACK_VERSION-$triple-engine.tar.zst
        if [ -f "$file" ]; then
            archives+=(--archive "$triple=$file")
        else
            printf 'pdfium-pack: no component for %s, leaving it out\n' "$slug" >&2
        fi
    done <"$here/targets.txt"
    [ "${#archives[@]}" -gt 0 ] || die "no pdfium components found in $dist"
    bash "$builder" merge \
        --name "$PACK_NAME" --version "$PACK_VERSION" --ashift "$ashift" --out "$dist" \
        --description "$PACK_DESCRIPTION" --license "$PACK_LICENSE" \
        --requires-ashift "$requires_ashift" \
        --engine-protocol "$PACK_ENGINE_PROTOCOL" \
        --entrypoint "bin/$PACK_ENGINE_BIN" \
        "${archives[@]}"
}

verify_pack() {
    local pack=''
    [ "${1:-}" = --pack ] && [ -n "${2:-}" ] || die "usage: make.sh verify --pack DIR"
    pack=$2
    local manifest=$pack/pack.toml
    [ -f "$manifest" ] || die "$manifest is missing"
    grep -q '^schema = "ariad-pack/1"$' "$manifest" || die "manifest schema is not ariad-pack/1"
    grep -q "^name = \"$PACK_NAME\"\$" "$manifest" || die "manifest name is not $PACK_NAME"
    grep -q "^engine_protocol = \"$PACK_ENGINE_PROTOCOL\"\$" "$manifest" ||
        die "manifest has no engine_protocol = \"$PACK_ENGINE_PROTOCOL\""
    [ -x "$pack/bin/$PACK_ENGINE_BIN" ] || die "bin/$PACK_ENGINE_BIN is missing or not executable"
    [ -f "$pack/bin/libpdfium.so" ] || [ -f "$pack/bin/libpdfium.dylib" ] ||
        die "bin/libpdfium is missing"
    local notice listed=0
    while IFS= read -r notice; do
        [ -s "$pack/$notice" ] || die "notice $notice is missing or empty"
        listed=$((listed + 1))
    done < <(sed -n 's/^ *"\(notices\/[^"]*\)",\{0,1\}$/\1/p' "$manifest")
    [ "$listed" -gt 0 ] || die "the manifest lists no notices"
    local library
    for library in abseil agg freetype harfbuzz icu lcms libjpeg libopenjpeg libpng pdfium zlib; do
        ls "$pack"/notices/third-party/"$library"* >/dev/null 2>&1 ||
            die "no third-party notice for $library"
        grep -q "notices/third-party/$library" "$manifest" ||
            die "the manifest does not list the $library notice"
    done
    [ -s "$pack/notices/pdfium-binaries-LICENSE" ] || die "pdfium-binaries licence missing"
    printf 'pdfium-pack: %s looks complete (%s notices)\n' "$pack" "$listed" >&2
}

main() {
    [ "$#" -ge 1 ] || die "usage: make.sh slug|pack|dev|merge|verify (see the header)"
    local command=$1
    shift
    case $command in
    slug) host_slug ;;
    pack) pack_one "$@" ;;
    dev) pack_one --slug "$(host_slug)" "$@" ;;
    merge) merge_all "$@" ;;
    verify) verify_pack "$@" ;;
    *) die "unknown command '$command'" ;;
    esac
}

main "$@"
