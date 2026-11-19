#!/usr/bin/env bash
# Sourced by macos-adhoc-sign.sh and macos-sign-notarize.sh; not run on its own.
#
# The one place that knows how AriadShift.app is signed: the walk (which code to sign, deepest first) and the codesign
# argument list of each mode. Both signing scripts sign from it, and macos-sign-notarize.sh verifies from the same list,
# so signing and verifying cannot disagree about what is code or in which order it is sealed.
#
# The walk, inside-out (a container seals what it holds, so everything inside is signed before the container):
#   1. Sparkle XPC services, the updater tool and Updater.app of each Versions/* directory (the Current symlink is skipped)
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

# shellcheck source=scripts/macos-helpers-lib.sh
source "$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/macos-helpers-lib.sh"

# list_sign_targets APP_PATH: every code item to sign, in signing order, NUL separated. The last one is APP_PATH itself.
list_sign_targets() {
    local app_path="$1"
    local frameworks_dir="${app_path}/Contents/Frameworks"
    local sparkle="${frameworks_dir}/Sparkle.framework"
    local version_dir xpc updater_tool

    # 1-2. Sparkle: the parts of each version, then the framework that holds them.
    if [[ -d "${sparkle}/Versions" ]]; then
        while IFS= read -r -d '' version_dir; do
            for xpc in "${version_dir}"/XPCServices/*.xpc; do
                if [[ -d "${xpc}" ]]; then
                    printf '%s\0' "${xpc}"
                fi
            done
            updater_tool="${version_dir}/Autoupdate"
            if [[ -f "${updater_tool}" ]]; then
                printf '%s\0' "${updater_tool}"
            fi
            if [[ -d "${version_dir}/Updater.app" ]]; then
                printf '%s\0' "${version_dir}/Updater.app"
            fi
        done < <(find "${sparkle}/Versions" -mindepth 1 -maxdepth 1 -type d ! -name 'Current' -print0)
        printf '%s\0' "${sparkle}"
    fi

    # 3-4. Any other embedded dylib, then framework: a dylib inside a framework is signed before the framework seals it.
    if [[ -d "${frameworks_dir}" ]]; then
        find "${frameworks_dir}" -mindepth 1 -maxdepth 2 -type f -name '*.dylib' -print0
        find "${frameworks_dir}" -mindepth 1 -maxdepth 1 -type d -name '*.framework' ! -name 'Sparkle.framework' -print0
    fi

    # 5. Every Mach-O in Helpers. A helper needs no line here to be signed.
    list_helper_machos "${app_path}"

    # 6. The app.
    printf '%s\0' "${app_path}"
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
# Signs every target of list_sign_targets in order and records each in SIGNED_TARGETS (set by the caller's script and
# verified there). The app gets ENTITLEMENTS_FILE, the explicit entitlements instead of whatever the build injected;
# the file is required before the app is signed, after everything inside it. Any codesign failure stops the script.
sign_app_targets() {
    local mode="$1" app_path="$2" entitlements_file="$3"
    local label="Signing bundle" target
    if [[ "${mode}" == "adhoc" ]]; then
        label="Ad-hoc signing"
    fi
    SIGNED_TARGETS=()
    while IFS= read -r -d '' target; do
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
    done < <(list_sign_targets "${app_path}")
}
