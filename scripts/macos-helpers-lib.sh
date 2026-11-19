#!/usr/bin/env bash
# Sourced (through macos-signing-lib.sh) by macos-adhoc-sign.sh and macos-sign-notarize.sh; not run on its own.
#
# Contents/Helpers holds the programs the app starts as child processes: the ashift helper and Pandoc. Each one is
# nested code that needs its own signature before the app is sealed, so both signing scripts take the list from here
# instead of naming files. A file is code when it starts with a Mach-O or fat-binary magic number; anything else in the
# folder (a notice, a script, an empty file) is not signed. A file that cannot be read fails the walk.

# is_macho_file FILE: classify one regular file by its first four bytes.
#   0  a thin or fat Mach-O, in either byte order
#   1  not code: another kind of file, an empty file or one shorter than four bytes
#   2  FILE cannot be read (the message names it): it is unknown whether it is code, so the caller must fail, never skip
# Reading and classifying are separate on purpose. A file that cannot be read must not look like "not Mach-O", or an
# unreadable helper would go unsigned while the run reports success.
is_macho_file() {
    local file="$1" magic=""
    if [[ ! -r "${file}" ]] || ! magic="$(od -An -tx1 -N4 -- "${file}" 2> /dev/null)"; then
        echo "error: cannot read ${file}; it cannot be classified as code to sign." >&2
        return 2
    fi
    magic="$(printf '%s' "${magic}" | tr -d ' \n')"
    # A file with content that yielded no bytes was not read, whatever od's exit status said.
    if [[ -z "${magic}" && -s "${file}" ]]; then
        echo "error: cannot read ${file}; it cannot be classified as code to sign." >&2
        return 2
    fi
    case "${magic}" in
        feedface | feedfacf | cefaedfe | cffaedfe | cafebabe | cafebabf | bebafeca | bfbafeca) return 0 ;;
        *) return 1 ;;
    esac
}

# list_helper_machos APP_PATH: the regular files under Contents/Helpers that are Mach-O, NUL separated.
# The loop only prints, so it ends a pipe and the status is find's or the loop's (the sourcing script runs with
# pipefail): a find that fails, or a file that cannot be read, makes the function fail instead of silently dropping
# helpers.
list_helper_machos() {
    local helpers_dir="$1/Contents/Helpers" item rc
    [[ -d "${helpers_dir}" ]] || return 0
    find "${helpers_dir}" -type f -print0 |
        while IFS= read -r -d '' item; do
            rc=0
            is_macho_file "${item}" || rc=$?
            case "${rc}" in
                0) printf '%s\0' "${item}" || exit 1 ;;
                1) ;;
                *) exit 1 ;;
            esac
        done
}
