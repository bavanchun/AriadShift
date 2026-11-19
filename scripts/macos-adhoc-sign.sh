#!/usr/bin/env bash
set -euo pipefail

# Ad-hoc sign AriadShift.app for the free release path (no Apple Developer account), then gate on the result.
#
# Usage: macos-adhoc-sign.sh app APP_PATH
#
# Sparkle.framework, its XPC services and its helpers ship already signed by the Sparkle project. Xcode leaves them
# alone, so the built app mixes identities and the hardened runtime's library validation refuses to load the framework.
# This script re-signs every piece with the same ad-hoc identity ("-"), deepest first:
#   1. Sparkle XPC services, the updater tool and Updater.app (every Versions/* directory except the Current symlink)
#   2. Sparkle.framework
#   3. any other embedded dylib, then any other embedded framework
#   4. every Mach-O file in Contents/Helpers (the ashift helper and Pandoc)
#   5. the app, with the explicit empty entitlements file scripts/macos-app.entitlements
# The walk and the codesign options (mode `adhoc`) come from scripts/macos-signing-lib.sh, shared with the Developer ID
# script macos-sign-notarize.sh, so the two cannot drift apart.
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
# shellcheck source=scripts/macos-signing-lib.sh
source "${SCRIPT_DIR}/macos-signing-lib.sh"

# Every signed target, in signing order. Verified after signing.
SIGNED_TARGETS=()

sign_app_inside_out() {
    sign_app_targets adhoc "$1" "${APP_ENTITLEMENTS}"
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
