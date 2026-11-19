#!/usr/bin/env bash
# Sourced by macos-adhoc-sign.sh and macos-sign-notarize.sh; not run on its own.
#
# The one place that knows how AriadShift.app is signed: the walk (which code to sign, deepest first) and the codesign
# argument list of each mode. Both signing scripts sign from it, and macos-sign-notarize.sh verifies from the same list,
# so signing and verifying cannot disagree about what is code or in which order it is sealed.
#
# The walk, inside-out (a container seals what it holds, so everything inside is signed before the container):
#   1. Sparkle XPC services, Autoupdate and Updater.app of each Versions/* directory (the Current symlink is skipped)
#   2. Sparkle.framework
#   3. any other embedded dylib, down to one level inside a framework (signed before the framework seals it)
#   4. any other embedded framework
#   5. every Mach-O file in Contents/Helpers (the ashift helper and Pandoc), from macos-helpers-lib.sh
#   6. the app
#
# Modes:
#   signed  Developer ID: the identity in APPLE_SIGNING_IDENTITY, an optional keychain in KEYCHAIN_PATH, the hardened
#           runtime and a secure timestamp. Nested code keeps its entitlements, requirements, flags and runtime.
#   adhoc   Free release path: identity "-", the hardened runtime and no timestamp (--timestamp=none keeps codesign off
#           the network). Nested code keeps its identifier and entitlements. Reads nothing from the environment.
#
# Everything here must stay compatible with the bash 3.2 that macOS ships: no mapfile, no associative arrays.

# The walk reports a failing find only through a pipeline's status, so every caller must run with pipefail.
if [[ ! -o pipefail ]]; then
    echo "error: macos-signing-lib.sh needs 'set -o pipefail' in the sourcing script." >&2
    exit 1
fi

# shellcheck source=scripts/macos-helpers-lib.sh
source "$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/macos-helpers-lib.sh"

# require_listable DIR: fail, with a message naming DIR, when DIR cannot be entered and listed. A test of a name inside
# a directory (-d, -f, a glob) answers "absent" when the directory itself cannot be read, which would drop that code
# from the list without a word.
require_listable() {
    if [[ ! -r "$1" || ! -x "$1" ]]; then
        echo "error: cannot read the directory $1; the code inside it cannot be listed." >&2
        return 1
    fi
}

# list_sign_targets APP_PATH: every code item to sign, in signing order, NUL separated. The last one is APP_PATH itself.
# Fail closed: a find that fails, a directory or helper that cannot be read, or a write that fails makes the function
# return non-zero, and nothing printed after that point. Callers must not run it in a process substitution, which would
# lose the status; load_sign_targets runs it in the main shell.
list_sign_targets() {
    local app_path="$1"
    local frameworks_dir="${app_path}/Contents/Frameworks"
    local sparkle="${frameworks_dir}/Sparkle.framework"
    local version_dir xpc xpc_dir updater_tool

    # 1-2. Sparkle: the parts of each version, then the framework that holds them. The loop only prints, so it can end a
    # pipe, and pipefail turns a failing find into a failing walk.
    if [[ -d "${sparkle}" ]]; then
        require_listable "${sparkle}" || return 1
    fi
    if [[ -d "${sparkle}/Versions" ]]; then
        find "${sparkle}/Versions" -mindepth 1 -maxdepth 1 -type d ! -name 'Current' -print0 |
            while IFS= read -r -d '' version_dir; do
                require_listable "${version_dir}" || exit 1
                xpc_dir="${version_dir}/XPCServices"
                if [[ -d "${xpc_dir}" ]]; then
                    require_listable "${xpc_dir}" || exit 1
                fi
                for xpc in "${xpc_dir}"/*.xpc; do
                    if [[ -d "${xpc}" ]]; then
                        printf '%s\0' "${xpc}" || exit 1
                    fi
                done
                updater_tool="${version_dir}/Autoupdate"
                if [[ -f "${updater_tool}" ]]; then
                    printf '%s\0' "${updater_tool}" || exit 1
                fi
                if [[ -d "${version_dir}/Updater.app" ]]; then
                    printf '%s\0' "${version_dir}/Updater.app" || exit 1
                fi
            done || return 1
        printf '%s\0' "${sparkle}" || return 1
    fi

    # 3-4. Any other embedded dylib, then framework: a dylib inside a framework is signed before the framework seals it.
    if [[ -d "${frameworks_dir}" ]]; then
        find "${frameworks_dir}" -mindepth 1 -maxdepth 2 -type f -name '*.dylib' -print0 || return 1
        find "${frameworks_dir}" -mindepth 1 -maxdepth 1 -type d -name '*.framework' ! -name 'Sparkle.framework' -print0 || return 1
    fi

    # 5. Every Mach-O in Helpers. A helper needs no line here to be signed.
    list_helper_machos "${app_path}" || return 1

    # 6. The app.
    printf '%s\0' "${app_path}" || return 1
}

# load_sign_targets APP_PATH
# Runs the walk in the main shell, into a temporary file, so that a failing walk stops the script before anything is
# signed or verified, and fills SIGN_TARGETS with the list. Exits 1 when the walk fails, finds nothing, or does not end
# at APP_PATH (a list cut short must never pass for a complete one). The temporary file lives only while the walk runs and
# is removed on every path, so no EXIT trap is needed (the callers have their own and a second one would replace it).
load_sign_targets() {
    local app_path="$1"
    local tmp target last="" count=0
    SIGN_TARGETS=()
    if ! tmp="$(mktemp "${TMPDIR:-/tmp}/ariad-sign-walk.XXXXXX")"; then
        echo "error: cannot create a temporary file to list the code to sign." >&2
        exit 1
    fi
    if ! list_sign_targets "${app_path}" > "${tmp}"; then
        rm -f "${tmp}"
        echo "error: cannot list the code to sign in ${app_path} (is a directory unreadable?); nothing was signed or verified." >&2
        exit 1
    fi
    while IFS= read -r -d '' target; do
        SIGN_TARGETS+=("${target}")
        last="${target}"
        count=$((count + 1))
    done < "${tmp}"
    rm -f "${tmp}"
    if [[ "${count}" -eq 0 ]]; then
        echo "error: no code to sign was found in ${app_path}." >&2
        exit 1
    fi
    if [[ "${last}" != "${app_path}" ]]; then
        echo "error: the list of code to sign in ${app_path} does not end at the app (last: ${last}); refusing to use a partial list." >&2
        exit 1
    fi
}

# build_codesign_args MODE TARGET [ENTITLEMENTS_FILE]
# Sets CODESIGN_ARGS to the arguments of one signing call: everything after `codesign`, the target last.
# With an entitlements file the target gets exactly those entitlements (used for the app). Without one it keeps what it
# was built with, as Sparkle documents for its XPC services and tools. The order of the arguments is part of the
# contract: the tests compare whole command lines.
build_codesign_args() {
    local mode="$1" target="$2" entitlements_file="${3:-}"
    local -a metadata=()
    case "${mode}" in
        signed)
            if [[ -n "${entitlements_file}" ]]; then
                metadata=(--entitlements "${entitlements_file}")
            else
                metadata=("--preserve-metadata=entitlements,requirements,flags,runtime")
            fi
            CODESIGN_ARGS=(--force --options runtime --timestamp "${metadata[@]}" --sign "${APPLE_SIGNING_IDENTITY}")
            if [[ -n "${KEYCHAIN_PATH:-}" ]]; then
                CODESIGN_ARGS+=(--keychain "${KEYCHAIN_PATH}")
            fi
            ;;
        adhoc)
            if [[ -n "${entitlements_file}" ]]; then
                metadata=(--entitlements "${entitlements_file}")
            else
                metadata=("--preserve-metadata=identifier,entitlements")
            fi
            CODESIGN_ARGS=(--force --sign - --options runtime --timestamp=none "${metadata[@]}")
            ;;
        *)
            echo "error: unknown signing mode '${mode}' (expected signed or adhoc)." >&2
            exit 1
            ;;
    esac
    CODESIGN_ARGS+=("${target}")
}

# sign_app_targets MODE APP_PATH ENTITLEMENTS_FILE
# Signs every target of the walk in order and records each in SIGNED_TARGETS (set by the caller's script and verified
# there). The whole list is built first (load_sign_targets), so a failing walk signs nothing. The app gets
# ENTITLEMENTS_FILE, the explicit entitlements instead of whatever the build injected; the file is required before the
# app is signed, after everything inside it. Any codesign failure stops the script.
sign_app_targets() {
    local mode="$1" app_path="$2" entitlements_file="$3"
    local label="Signing bundle" target
    if [[ "${mode}" == "adhoc" ]]; then
        label="Ad-hoc signing"
    fi
    load_sign_targets "${app_path}"
    SIGNED_TARGETS=()
    for target in "${SIGN_TARGETS[@]}"; do
        if [[ "${target}" == "${app_path}" ]]; then
            if [[ ! -f "${entitlements_file}" ]]; then
                echo "error: entitlements file ${entitlements_file} not found." >&2
                exit 1
            fi
            build_codesign_args "${mode}" "${target}" "${entitlements_file}"
        else
            build_codesign_args "${mode}" "${target}"
        fi
        echo "==> ${label}: ${target}"
        codesign "${CODESIGN_ARGS[@]}"
        SIGNED_TARGETS+=("${target}")
    done
}
