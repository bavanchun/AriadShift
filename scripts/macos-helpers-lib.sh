#!/usr/bin/env bash
# Sourced (through macos-signing-lib.sh) by macos-adhoc-sign.sh and macos-sign-notarize.sh; not run on its own.
#
# Contents/Helpers holds the programs the app starts as child processes: the ashift helper and Pandoc. Each one is
# nested code that needs its own signature before the app is sealed, so both signing scripts take the list from here
# instead of naming files. A file is code when it starts with a Mach-O or fat-binary magic number; anything else in the
# folder (a notice, a script) is not signed.

# is_macho_file FILE: true for a thin or fat Mach-O, in either byte order.
is_macho_file() {
    local magic
    magic="$(od -An -tx1 -N4 -- "$1" 2>/dev/null | tr -d ' \n')"
    case "${magic}" in
        feedface | feedfacf | cefaedfe | cffaedfe | cafebabe | cafebabf | bebafeca | bfbafeca) return 0 ;;
        *) return 1 ;;
    esac
}

# list_helper_machos APP_PATH: the regular files under Contents/Helpers that are Mach-O, NUL separated.
# The loop only prints, so it ends a pipe and the status is find's (the sourcing script runs with pipefail): a find that
# fails makes the function fail instead of silently dropping the helpers after the failure.
list_helper_machos() {
    local helpers_dir="$1/Contents/Helpers" item
    [[ -d "${helpers_dir}" ]] || return 0
    find "${helpers_dir}" -type f -print0 |
        while IFS= read -r -d '' item; do
            if is_macho_file "${item}"; then
                printf '%s\0' "${item}"
            fi
        done
}
