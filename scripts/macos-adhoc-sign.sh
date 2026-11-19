#!/usr/bin/env bash
set -euo pipefail

# Ad-hoc sign AriadShift.app for the free release path (no Apple Developer account), then gate on the result.
#
# Usage: macos-adhoc-sign.sh app APP_PATH
#
# Sparkle.framework, its XPC services and its helpers ship already signed by the Sparkle project. Xcode leaves them
# alone, so the built app mixes identities and the hardened runtime's library validation refuses to load the framework.
# This script re-signs every piece with the same ad-hoc identity ("-"), deepest first:
#   1. Sparkle XPC services, Autoupdate and Updater.app (every Versions/* directory except the Current symlink)
#   2. Sparkle.framework
#   3. any other embedded dylib, then any other embedded framework
#   4. every Mach-O file in Contents/Helpers (the ashift helper and Pandoc)
#   5. the app, with the explicit empty entitlements file scripts/macos-app.entitlements
# Rules:
#   - Never `--deep` to sign. It applies one set of options to everything in an order Apple does not recommend for
#     signing; it is used only to verify.
#   - `--options runtime` keeps the hardened runtime that apps/macos/project.yml enables.
#   - `--timestamp=none`: an ad-hoc signature cannot get a secure timestamp, and naming it keeps codesign off the network.
#   - Nested code keeps the identifier and entitlements it was built with (Sparkle documents the entitlements for its XPC
#     services). Requirements are not preserved: they name the old identity, which no longer signs the code.
#   - No secret, keychain or identity is involved; nothing here reads the environment.
#
# Gate, after signing: every signed target must pass `codesign --verify --deep --strict`, be ad-hoc signed with the
# hardened runtime flag, and carry no com.apple.security.get-task-allow entitlement (a debug entitlement that lets any
# process attach to the app). Any failure exits non-zero before a DMG is built.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
APP_ENTITLEMENTS="${SCRIPT_DIR}/macos-app.entitlements"
# shellcheck source=scripts/macos-helpers-lib.sh
source "${SCRIPT_DIR}/macos-helpers-lib.sh"

# Every signed target, in signing order. Verified after signing.
SIGNED_TARGETS=()

# sign_target TARGET [EXTRA_CODESIGN_ARGS...]
sign_target() {
    local target="$1"
    shift
    echo "==> Ad-hoc signing: ${target}"
    codesign --force --sign - --options runtime --timestamp=none "$@" "${target}"
    SIGNED_TARGETS+=("${target}")
}

# sign_nested TARGET: nested code keeps the identifier and entitlements it was built with.
sign_nested() {
    sign_target "$1" --preserve-metadata=identifier,entitlements
}

sign_app_inside_out() {
    local app_path="$1"
    local frameworks_dir="${app_path}/Contents/Frameworks"
    local sparkle="${frameworks_dir}/Sparkle.framework"
    local version_dir xpc item

    SIGNED_TARGETS=()

    # 1-2. Sparkle: XPC services, Autoupdate and Updater.app of each version, then the framework itself.
    if [[ -d "${sparkle}/Versions" ]]; then
        while IFS= read -r -d '' version_dir; do
            for xpc in "${version_dir}"/XPCServices/*.xpc; do
                if [[ -d "${xpc}" ]]; then
                    sign_nested "${xpc}"
                fi
            done
            if [[ -f "${version_dir}/Autoupdate" ]]; then
                sign_nested "${version_dir}/Autoupdate"
            fi
            if [[ -d "${version_dir}/Updater.app" ]]; then
                sign_nested "${version_dir}/Updater.app"
            fi
        done < <(find "${sparkle}/Versions" -mindepth 1 -maxdepth 1 -type d ! -name 'Current' -print0)
        sign_nested "${sparkle}"
    fi

    # 3. Any other embedded dylib, then framework (a dylib inside a framework is signed before the framework seals it).
    if [[ -d "${frameworks_dir}" ]]; then
        while IFS= read -r -d '' item; do
            sign_nested "${item}"
        done < <(find "${frameworks_dir}" -mindepth 1 -maxdepth 2 -type f -name '*.dylib' -print0)
        while IFS= read -r -d '' item; do
            sign_nested "${item}"
        done < <(find "${frameworks_dir}" -mindepth 1 -maxdepth 1 -type d -name '*.framework' ! -name 'Sparkle.framework' -print0)
    fi

    # 4. Every Mach-O in Helpers: the ashift helper and Pandoc. A helper no longer needs a line here to be signed.
    while IFS= read -r -d '' item; do
        sign_nested "${item}"
    done < <(list_helper_machos "${app_path}")

    # 5. The app, with the explicit empty entitlements instead of whatever the build injected.
    if [[ ! -f "${APP_ENTITLEMENTS}" ]]; then
        echo "error: entitlements file ${APP_ENTITLEMENTS} not found." >&2
        exit 1
    fi
    sign_target "${app_path}" --entitlements "${APP_ENTITLEMENTS}"
}

# fail MESSAGE: print a GitHub error annotation and stop.
fail() {
    echo "::error::$1" >&2
    exit 1
}

verify_target() {
    local target="$1"
    local info flags entitlements
    echo "==> Verifying signature: ${target}"
    codesign --verify --deep --strict --verbose=2 "${target}"
    if ! info="$(codesign -dvv "${target}" 2>&1)"; then
        fail "cannot read the code signature of ${target}"
    fi
    printf '%s\n' "${info}"
    # The CodeDirectory line reads like: flags=0x10002(adhoc,runtime). Take the flag names of the first such line.
    flags="$(sed -n 's/.*flags=0x[0-9a-fA-F]*(\([^)]*\)).*/\1/p' <<< "${info}")"
    flags="${flags%%$'\n'*}"
    case ",${flags}," in
        *,adhoc,*) ;;
        *) fail "${target} is not ad-hoc signed (flags: ${flags:-none}); a leftover identity would break library validation." ;;
    esac
    case ",${flags}," in
        *,runtime,*) ;;
        *) fail "${target} lost the hardened runtime flag (flags: ${flags:-none})." ;;
    esac
    if ! entitlements="$(codesign -d --entitlements - --xml "${target}" 2>/dev/null)"; then
        fail "cannot read the entitlements of ${target}"
    fi
    if grep -q 'com.apple.security.get-task-allow' <<< "${entitlements}"; then
        fail "${target} carries com.apple.security.get-task-allow, a debug entitlement that must not ship."
    fi
}

verify_signed_targets() {
    echo "==> Verifying all signatures, flags and entitlements..."
    local target
    for target in "${SIGNED_TARGETS[@]}"; do
        verify_target "${target}"
    done
}

subcommand_app() {
    local app_path="${1:-}"
    if [[ -z "${app_path}" || ! -d "${app_path}" || "${app_path}" != *.app ]]; then
        echo "error: '${app_path}' is not a valid .app bundle directory." >&2
        exit 1
    fi
    sign_app_inside_out "${app_path}"
    verify_signed_targets
    echo "==> Ad-hoc signed and verified: ${app_path}"
}

COMMAND="${1:-}"
shift || true

case "${COMMAND}" in
    app)
        subcommand_app "$@"
        ;;
    *)
        echo "Usage: $0 app APP_PATH" >&2
        exit 1
        ;;
esac
