#!/usr/bin/env bash
# Shared engine-pack builder. Every pack that ships an out-of-process engine binary
# (pdfium now; qpdf, tesseract, sign, verapdf, ai and inpaint later) builds through this
# script, driven by a small per-pack wrapper such as engines/pdfium/pack/build.sh.
#
#   build-pack.sh build ...   assemble one target: verify the pinned upstream archive, lay out
#                             the pack root, code-sign macOS binaries, write one tar.zst
#                             component and an unsigned pack.toml for that target.
#   build-pack.sh merge ...   combine already built components of several targets into one
#                             pack.toml (the release job runs this before signing).
#
# The manifest itself is written by `ashift __pack manifest`; this script never hand-writes
# component hashes. Packs are signed with minisign only in CI (scripts/pack/sign-pack.sh).
#
# Common options (build and merge):
#   --name NAME              pack name (engine id)
#   --version VERSION        pack version (SemVer)
#   --ashift PATH            ashift binary that writes the manifest
#   --out DIR                output directory (component archive and pack.toml)
#   --description TEXT       manifest description
#   --license SPDX           manifest SPDX licence summary of everything in the pack
#   --requires-ashift REQ    SemVer requirement on ashift
#   --entrypoint ARG         engine argv element (repeat; first is the path under the pack root)
#   --engine-protocol VALUE  manifest engine_protocol, passed as `ashift __pack manifest
#                            --engine-protocol VALUE`; the script checks the written pack.toml
#                            has the top-level key `engine_protocol = "VALUE"`
#   --component-name NAME    component name in the manifest (default: engine)
#   --url-prefix URL         prefix for component URLs (default: bare file names)
#
# build options:
#   --triple TRIPLE          Rust target triple of the pack's binaries
#   --engine PATH            engine binary
#   --engine-dest REL        where it goes in the pack root (default: bin/<file name>)
#   --archive-url URL        https URL of the upstream archive (needs --archive-sha256)
#   --archive FILE           pre-downloaded upstream archive (replaces the download)
#   --archive-sha256 HEX     pinned SHA-256 of the upstream archive; empty or malformed refuses
#   --from-archive SRC=DEST  copy SRC (file or directory inside the upstream archive) to DEST
#                            inside the pack root (repeat)
#   --extra SRC=DEST         copy a local file or directory to DEST inside the pack root (repeat)
#   --executable REL         give REL (inside the pack root) the executable bit (repeat; the
#                            engine is always executable)
#   --notices DIR            local directory copied to notices/ (optional)
#   --no-codesign            on a macOS triple, do not codesign (default: codesign, ad-hoc
#                            unless APPLE_SIGNING_IDENTITY names a Developer ID identity)
#   --unpack-dir DIR         also unpack the pack root and pack.toml into DIR, a development
#                            pack for ASHIFT_PACK_<NAME>
#
# merge options:
#   --archive TRIPLE=FILE    built component of one target (repeat)
#
# Environment: SOURCE_DATE_EPOCH fixes archive timestamps; APPLE_SIGNING_IDENTITY enables
# Developer-ID signing on macOS builds.
set -euo pipefail

prog=build-pack
bs=$(printf '\134')

die() {
    printf '%s: %s\n' "$prog" "$*" >&2
    exit 1
}

note() {
    printf '%s: %s\n' "$prog" "$*" >&2
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "required tool '$1' was not found"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        die "neither sha256sum nor shasum was found"
    fi
}

# A clean relative path: no absolute path, no empty, "." or ".." component, no backslash.
safe_rel() {
    local value=$1 label=$2
    case $value in
    '' | /* | *"$bs"* | . | .. | ./* | ../* | */. | */.. | */./* | */../* | *//*)
        die "$label must be a clean relative path, got '$value'"
        ;;
    esac
}

# Text that is written into TOML strings without escaping.
plain_text() {
    local value=$1 label=$2
    case $value in
    '' | *'"'* | *"$bs"* | *[[:cntrl:]]*)
        die "$label must be non-empty text without quotes, backslashes or control characters"
        ;;
    esac
}

split_pair() {
    local pair=$1 label=$2
    case $pair in
    ?*=?*) ;;
    *) die "$label expects SRC=DEST, got '$pair'" ;;
    esac
    pair_left=${pair%%=*}
    pair_right=${pair#*=}
}

epoch_stamp() {
    local epoch=$1
    date -u -d "@$epoch" +%Y%m%d%H%M.%S 2>/dev/null || date -u -r "$epoch" +%Y%m%d%H%M.%S
}

tar_is_gnu() {
    "$1" --version 2>/dev/null | grep -q 'GNU tar'
}

pick_tar() {
    if command -v gtar >/dev/null 2>&1; then
        echo gtar
    elif command -v tar >/dev/null 2>&1; then
        echo tar
    else
        die "required tool 'tar' was not found"
    fi
}

# --- shared option state ------------------------------------------------------------------

name=''
version=''
ashift=''
out=''
description=''
license=''
requires_ashift=''
engine_protocol=''
component_name='engine'
url_prefix=''
entrypoint=()

triple=''
engine=''
engine_dest=''
archive_url=''
archive_file=''
archive_sha256=''
from_archive=()
extras=()
executables=()
notices_dir=''
codesign=1
unpack_dir=''
merge_archives=()

workdir=''
cleanup() {
    if [ -n "$workdir" ] && [ -d "$workdir" ]; then
        chmod -R u+rwX "$workdir" 2>/dev/null || true
        rm -rf "$workdir"
    fi
}
trap cleanup EXIT

parse_args() {
    while [ "$#" -gt 0 ]; do
        local flag=$1
        case $flag in
        --no-codesign)
            codesign=0
            shift
            continue
            ;;
        --name | --version | --ashift | --out | --description | --license | --requires-ashift | \
            --entrypoint | --engine-protocol | --component-name | --url-prefix | --triple | \
            --engine | --engine-dest | --archive-url | --archive | --archive-sha256 | \
            --from-archive | --extra | --executable | --notices | --unpack-dir) ;;
        *) die "unknown option '$flag'" ;;
        esac
        [ "$#" -ge 2 ] || die "option $flag needs a value"
        local value=$2
        shift 2
        case $flag in
        --name) name=$value ;;
        --version) version=$value ;;
        --ashift) ashift=$value ;;
        --out) out=$value ;;
        --description) description=$value ;;
        --license) license=$value ;;
        --requires-ashift) requires_ashift=$value ;;
        --entrypoint) entrypoint+=("$value") ;;
        --engine-protocol) engine_protocol=$value ;;
        --component-name) component_name=$value ;;
        --url-prefix) url_prefix=$value ;;
        --triple) triple=$value ;;
        --engine) engine=$value ;;
        --engine-dest) engine_dest=$value ;;
        --archive-url) archive_url=$value ;;
        --archive)
            # build: the local upstream archive; merge: TRIPLE=FILE of a built component.
            archive_file=$value
            merge_archives+=("$value")
            ;;
        --archive-sha256) archive_sha256=$value ;;
        --from-archive) from_archive+=("$value") ;;
        --extra) extras+=("$value") ;;
        --executable) executables+=("$value") ;;
        --notices) notices_dir=$value ;;
        --unpack-dir) unpack_dir=$value ;;
        esac
    done
}

validate_common() {
    [ -n "$name" ] || die "--name is required"
    [ -n "$version" ] || die "--version is required"
    [ -n "$ashift" ] || die "--ashift is required"
    [ -n "$out" ] || die "--out is required"
    [ "${#entrypoint[@]}" -gt 0 ] || die "--entrypoint is required"
    case $name in
    [a-z]*) ;;
    *) die "pack name '$name' must start with a lower-case letter" ;;
    esac
    case $name in
    *[!a-z0-9-]*) die "pack name '$name' may contain only lower-case letters, digits and '-'" ;;
    esac
    [ "${#name}" -le 64 ] || die "pack name '$name' is longer than 64 bytes"
    printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$' ||
        die "version '$version' is not SemVer"
    plain_text "$description" "--description"
    plain_text "$license" "--license"
    plain_text "$requires_ashift" "--requires-ashift"
    case $component_name in
    '' | *[!A-Za-z0-9._-]*) die "component name '$component_name' has unsupported characters" ;;
    esac
    if [ -n "$engine_protocol" ]; then
        printf '%s\n' "$engine_protocol" | grep -Eq '^[a-z][a-z0-9-]*/[0-9]+$' ||
            die "engine protocol '$engine_protocol' must look like ariad-engine/2"
    fi
    [ -x "$ashift" ] || die "--ashift '$ashift' is not an executable file"
}

# `ashift __pack manifest` must understand --engine-protocol before a manifest with the field
# can be written; refuse early with a pointer instead of producing a manifest without it.
check_ashift_flag() {
    [ -n "$engine_protocol" ] || return 0
    if ! "$ashift" __pack manifest --help 2>&1 | grep -q -- '--engine-protocol'; then
        die "'$ashift' does not support 'ashift __pack manifest --engine-protocol'; build an ashift that does"
    fi
}

# --- base manifest and manifest generation --------------------------------------------------

# Writes the pack-level fields (everything but the per-target components) to $1. $2 is the
# pack root whose notices/ directory lists the notice files, or empty to reuse the list in $3.
write_base_manifest() {
    local base=$1 root=$2 notice_list=$3
    {
        printf 'schema = "ariad-pack/1"\n'
        printf 'name = "%s"\n' "$name"
        printf 'version = "%s"\n' "$version"
        printf 'description = "%s"\n' "$description"
        printf 'license = "%s"\n' "$license"
        printf 'requires_ashift = "%s"\n' "$requires_ashift"
        printf 'notices = [\n'
        if [ -n "$root" ]; then
            (cd "$root" && find notices -type f 2>/dev/null | LC_ALL=C sort) >"$notice_list"
        fi
        local line
        while IFS= read -r line; do
            [ -n "$line" ] || continue
            case $line in
            *'"'* | *"$bs"*) die "notice path '$line' has characters that need TOML escaping" ;;
            esac
            printf '    "%s",\n' "$line"
        done <"$notice_list"
        printf ']\n'
        printf 'targets = []\n'
    } >"$base"
}

# Runs `ashift __pack manifest` for one target: $1 base manifest, $2 output, $3 triple,
# $4 NAME=FILE of the component.
run_manifest() {
    local base=$1 manifest_out=$2 target_triple=$3 component=$4
    local args=(__pack manifest --name "$name" --version "$version" --target "$target_triple"
        --base "$base" -o "$manifest_out")
    local arg
    for arg in "${entrypoint[@]}"; do
        args+=(--entrypoint "$arg")
    done
    if [ -n "$engine_protocol" ]; then
        args+=(--engine-protocol "$engine_protocol")
    fi
    if [ -n "$url_prefix" ]; then
        args+=(--url-prefix "$url_prefix")
    fi
    args+=("$component")
    "$ashift" "${args[@]}"
}

check_manifest_output() {
    local manifest=$1
    [ -s "$manifest" ] || die "ashift wrote no manifest to $manifest"
    if [ -n "$engine_protocol" ]; then
        grep -q "^engine_protocol = \"$engine_protocol\"\$" "$manifest" ||
            die "$manifest has no top-level engine_protocol = \"$engine_protocol\""
    fi
    grep -q '^executables = \[' "$manifest" ||
        die "$manifest has no target executables list"
}

# --- upstream archive -------------------------------------------------------------------------

# Prints the verified upstream archive path. Pins are mandatory: an empty or malformed
# SHA-256 refuses before anything is downloaded or unpacked.
fetch_upstream() {
    local dest=$1 expected
    expected=$(printf '%s' "$archive_sha256" | tr 'A-F' 'a-f')
    printf '%s\n' "$expected" | grep -Eq '^[0-9a-f]{64}$' ||
        die "no valid pinned SHA-256 for the upstream archive (got '${archive_sha256}'); refusing to build"
    local file
    if [ -n "$archive_file" ]; then
        [ -f "$archive_file" ] || die "--archive '$archive_file' is not a file"
        file=$archive_file
    else
        [ -n "$archive_url" ] || die "an upstream archive needs --archive-url or --archive"
        case $archive_url in
        https://*) ;;
        *) die "upstream archive URL must be https://, got '$archive_url'" ;;
        esac
        need curl
        file=$dest
        curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
            --retry 3 --connect-timeout 30 --max-time 900 --output "$file" "$archive_url" ||
            die "download of $archive_url failed"
    fi
    local actual
    actual=$(sha256_of "$file")
    [ "$actual" = "$expected" ] ||
        die "SHA-256 mismatch for the upstream archive: expected $expected, got $actual"
    printf '%s\n' "$file"
}

# Lists the archive and refuses unsafe names and anything that is not a regular file or
# directory, then unpacks it into $2.
unpack_upstream() {
    local file=$1 dest=$2 tar_bin
    tar_bin=$(pick_tar)
    local entry
    while IFS= read -r entry; do
        case $entry in
        /* | *"$bs"* | .. | ../* | */.. | */../*)
            die "unsafe entry '$entry' in the upstream archive"
            ;;
        esac
    done < <("$tar_bin" -tf "$file")
    local kinds
    kinds=$("$tar_bin" -tvf "$file" | cut -c1 | LC_ALL=C sort -u | tr -d '\n')
    case $kinds in
    '' | *[!-d]*) die "upstream archive holds entries other than regular files and directories" ;;
    esac
    mkdir -p "$dest"
    "$tar_bin" -xf "$file" -C "$dest"
    if [ -n "$(find "$dest" -type l -print -quit)" ]; then
        die "upstream archive unpacked symbolic links"
    fi
}

# --- pack root --------------------------------------------------------------------------------

place() {
    local src=$1 rel=$2 root=$3
    safe_rel "$rel" "destination"
    local dest=$root/$rel
    [ ! -e "$dest" ] || die "destination '$rel' is already occupied"
    [ -e "$src" ] || die "source '$src' does not exist"
    mkdir -p "$(dirname "$dest")"
    cp -R "$src" "$dest"
}

# A directory copied into the pack must be complete: compare it with its source.
check_complete() {
    local src=$1 dest=$2
    if [ -d "$src" ]; then
        diff -r "$src" "$dest" >/dev/null || die "copy of '$src' is incomplete"
    fi
}

codesign_tree() {
    local root=$1
    case $triple in
    *-apple-darwin) ;;
    *) return 0 ;;
    esac
    if [ "$codesign" -eq 0 ]; then
        note "skipping codesign for $triple (--no-codesign)"
        return 0
    fi
    command -v codesign >/dev/null 2>&1 ||
        die "target $triple needs 'codesign' (run on macOS) or pass --no-codesign"
    local files=() rel file
    for rel in "$engine_dest" ${executables[@]+"${executables[@]}"}; do
        files+=("$root/$rel")
    done
    while IFS= read -r file; do
        files+=("$file")
    done < <(find "$root" -type f -name '*.dylib' | LC_ALL=C sort)
    for file in "${files[@]}"; do
        if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
            codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$file"
        else
            codesign --force --sign - "$file"
        fi
    done
}

normalize_tree() {
    local root=$1 stamp rel
    find "$root" -type d -exec chmod 755 {} +
    find "$root" -type f -exec chmod 644 {} +
    for rel in "$engine_dest" ${executables[@]+"${executables[@]}"}; do
        chmod 755 "$root/$rel"
    done
    stamp=$(epoch_stamp "${SOURCE_DATE_EPOCH:-1767225600}")
    find "$root" -exec touch -t "$stamp" {} +
}

make_archive() {
    local root=$1 archive=$2 tar_bin list
    need zstd
    tar_bin=$(pick_tar)
    list=$workdir/members.list
    (cd "$root" && find . -mindepth 1 | LC_ALL=C sort | sed 's|^\./||') >"$list"
    export COPYFILE_DISABLE=1
    if tar_is_gnu "$tar_bin"; then
        "$tar_bin" --no-recursion --numeric-owner --owner=0 --group=0 \
            -cf - -C "$root" -T "$list"
    else
        "$tar_bin" -n --numeric-owner --uid 0 --gid 0 -cf - -C "$root" -T "$list"
    fi | zstd -q -19 -T1 -c >"$archive"
}

# --- subcommands ------------------------------------------------------------------------------

cmd_build() {
    parse_args "$@"
    validate_common
    [ -n "$triple" ] || die "--triple is required"
    printf '%s\n' "$triple" | grep -Eq '^[a-z0-9_]+(-[A-Za-z0-9_.]+){2,3}$' ||
        die "target triple '$triple' is not well formed"
    [ -n "$engine" ] || die "--engine is required"
    [ -f "$engine" ] || die "engine binary '$engine' is not a file"
    [ -n "$engine_dest" ] || engine_dest=bin/$(basename "$engine")
    safe_rel "$engine_dest" "--engine-dest"
    check_ashift_flag

    workdir=$(mktemp -d "${TMPDIR:-/tmp}/build-pack.XXXXXX")
    local root=$workdir/root
    mkdir -p "$root"

    # The upstream archive is verified before anything is taken from it.
    if [ "${#from_archive[@]}" -gt 0 ] || [ -n "$archive_url" ] || [ -n "$archive_file" ]; then
        local upstream upstream_dir=$workdir/upstream pair
        upstream=$(fetch_upstream "$workdir/upstream.archive")
        unpack_upstream "$upstream" "$upstream_dir"
        for pair in ${from_archive[@]+"${from_archive[@]}"}; do
            split_pair "$pair" "--from-archive"
            safe_rel "$pair_left" "--from-archive source"
            place "$upstream_dir/$pair_left" "$pair_right" "$root"
            check_complete "$upstream_dir/$pair_left" "$root/$pair_right"
        done
    fi

    place "$engine" "$engine_dest" "$root"
    local pair
    for pair in ${extras[@]+"${extras[@]}"}; do
        split_pair "$pair" "--extra"
        place "$pair_left" "$pair_right" "$root"
        check_complete "$pair_left" "$root/$pair_right"
    done
    if [ -n "$notices_dir" ]; then
        [ -d "$notices_dir" ] || die "--notices '$notices_dir' is not a directory"
        local entry
        for entry in "$notices_dir"/*; do
            [ -e "$entry" ] || die "--notices '$notices_dir' is empty"
            place "$entry" "notices/$(basename "$entry")" "$root"
        done
    fi
    local rel
    for rel in ${executables[@]+"${executables[@]}"}; do
        safe_rel "$rel" "--executable"
        [ -f "$root/$rel" ] || die "--executable '$rel' is not a file in the pack"
    done

    [ -d "$root/notices" ] || die "the pack has no notices/ directory; every pack ships its licences"
    local empty
    empty=$(find "$root/notices" -type f -size 0 -print -quit)
    [ -z "$empty" ] || die "empty notice file ${empty#"$root"/}"

    codesign_tree "$root"
    normalize_tree "$root"

    mkdir -p "$out"
    local archive=$out/$name-$version-$triple-$component_name.tar.zst
    make_archive "$root" "$archive"

    local base=$workdir/base.toml
    write_base_manifest "$base" "$root" "$workdir/notices.list"
    run_manifest "$base" "$out/pack.toml" "$triple" "$component_name=$archive"
    check_manifest_output "$out/pack.toml"

    if [ -n "$unpack_dir" ]; then
        rm -rf "$unpack_dir"
        mkdir -p "$unpack_dir"
        cp -R "$root"/. "$unpack_dir"/
        cp "$out/pack.toml" "$unpack_dir/pack.toml"
    fi
    note "built $archive"
}

cmd_merge() {
    parse_args "$@"
    validate_common
    [ "${#merge_archives[@]}" -gt 0 ] || die "merge needs at least one --archive TRIPLE=FILE"
    check_ashift_flag
    workdir=$(mktemp -d "${TMPDIR:-/tmp}/build-pack.XXXXXX")

    # The notice list is the union found in the first component; every target carries the
    # same notices directory.
    local first_pair
    split_pair "${merge_archives[0]}" "--archive"
    first_pair=$pair_right
    need zstd
    local tar_bin
    tar_bin=$(pick_tar)
    local notice_list=$workdir/notices.list
    zstd -q -d -c "$first_pair" | "$tar_bin" -tf - | grep '^notices/' | grep -v '/$' |
        LC_ALL=C sort >"$notice_list" || true
    [ -s "$notice_list" ] || die "'$first_pair' lists no notices/ files"

    local base=$workdir/base.toml current=$workdir/current.toml pair
    write_base_manifest "$base" "" "$notice_list"
    cp "$base" "$current"
    local next=$workdir/next.toml
    for pair in "${merge_archives[@]}"; do
        split_pair "$pair" "--archive"
        [ -f "$pair_right" ] || die "component '$pair_right' is not a file"
        run_manifest "$current" "$next" "$pair_left" "$component_name=$pair_right"
        check_manifest_output "$next"
        mv "$next" "$current"
    done
    mkdir -p "$out"
    cp "$current" "$out/pack.toml"
    note "wrote $out/pack.toml for ${#merge_archives[@]} target(s)"
}

main() {
    [ "$#" -ge 1 ] || die "usage: build-pack.sh build|merge [options] (see the header of this script)"
    local command=$1
    shift
    case $command in
    build) cmd_build "$@" ;;
    merge) cmd_merge "$@" ;;
    -h | --help)
        sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
        ;;
    *) die "unknown command '$command'" ;;
    esac
}

main "$@"
