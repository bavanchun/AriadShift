#!/usr/bin/env bash
set -euo pipefail

# Sign and notarize AriadShift.app or AriadShift universal DMG using Apple Developer ID credentials.
#
# Subcommands:
#   app [APP_PATH]      - Sign app inside-out, verify nested bundles, zip, notarize, staple app, spctl assess
#   dmg [DMG_PATH]      - Sign DMG, notarize, staple DMG, spctl assess
#   sign-app [APP_PATH] - Sign app inside-out and verify nested bundles without notarizing
#   sign-dmg [DMG_PATH] - Sign DMG without notarizing
#   verify-app [APP_PATH] - Verify signatures and the get-task-allow gate of an already signed app (no credentials needed)
#
# The app is signed with scripts/macos-app.entitlements; every signed bundle must be free of
# com.apple.security.get-task-allow or the script fails before anything reaches the notary service.
#
# What is signed, in which order and with which codesign options comes from scripts/macos-signing-lib.sh (mode
# `signed`), shared with macos-adhoc-sign.sh. Signing and verify-app walk the same list.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH='' cd -- "${SCRIPT_DIR}/.." && pwd)"

DERIVED_DATA="${MACOS_DERIVED_DATA:-${DERIVED_DATA_DIR:-${REPO_ROOT}/target/derived-data}}"
NOTARY_TIMEOUT="${NOTARY_TIMEOUT:-20m}"

KEY_DIR=""
cleanup_notary_key() {
    if [[ -n "${KEY_DIR}" && -d "${KEY_DIR}" ]]; then
        rm -rf "${KEY_DIR}"
    fi
}
trap cleanup_notary_key EXIT

resolve_app_path() {
    local target="${1:-}"
    if [[ -z "${target}" ]]; then
        if [[ -d "${DERIVED_DATA}/Build/Products/Release/AriadShift.app" ]]; then
            target="${DERIVED_DATA}/Build/Products/Release/AriadShift.app"
        elif [[ -d "${REPO_ROOT}/target/release/macos/AriadShift.app" ]]; then
            target="${REPO_ROOT}/target/release/macos/AriadShift.app"
        elif [[ -d "${REPO_ROOT}/target/release/AriadShift.app" ]]; then
            target="${REPO_ROOT}/target/release/AriadShift.app"
        else
            echo "error: AriadShift.app not found. Run 'just macos-build' first or pass app path." >&2
            exit 1
        fi
    fi
    if [[ ! -d "${target}" || "${target}" != *.app ]]; then
        echo "error: '${target}' is not a valid .app bundle directory." >&2
        exit 1
    fi
    echo "${target}"
}

resolve_dmg_path() {
    local target="${1:-}"
    if [[ -z "${target}" ]]; then
        local found
        found="$(find "${REPO_ROOT}/target/release" -maxdepth 1 -name "AriadShift-*-universal.dmg" ! -name "*-unsigned.dmg" | head -n 1)"
        if [[ -n "${found}" && -f "${found}" ]]; then
            target="${found}"
        else
            echo "error: DMG file not specified and none found in target/release." >&2
            exit 1
        fi
    fi
    if [[ ! -f "${target}" || "${target}" != *.dmg ]]; then
        echo "error: '${target}' is not a valid .dmg file." >&2
        exit 1
    fi
    echo "${target}"
}

check_signing_credentials() {
    if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
        echo "======================================================================" >&2
        echo "ERROR / UNSIGNED NOTICE:" >&2
        echo "APPLE_SIGNING_IDENTITY is not set." >&2
        echo "Code signing cannot proceed without a valid signing identity." >&2
        echo "======================================================================" >&2
        exit 1
    fi
}

check_notary_credentials() {
    check_signing_credentials
    if [[ -z "${APPLE_API_KEY_ID:-}" || -z "${APPLE_API_ISSUER:-}" || -z "${APPLE_API_KEY:-}" ]]; then
        echo "======================================================================" >&2
        echo "ERROR / UNSIGNED NOTICE:" >&2
        echo "Apple notarization credentials (APPLE_API_KEY_ID, APPLE_API_ISSUER, APPLE_API_KEY) are not set." >&2
        echo "Cannot submit to Apple notarytool without credentials." >&2
        echo "======================================================================" >&2
        exit 1
    fi
}

prepare_notary_key() {
    KEY_DIR="$(mktemp -d "${TMPDIR:-/tmp}/apple-notary.XXXXXX")"
    chmod 700 "${KEY_DIR}"
    KEY_FILE="${KEY_DIR}/AuthKey_${APPLE_API_KEY_ID}.p8"
    if echo "${APPLE_API_KEY}" | grep -q "BEGIN PRIVATE KEY"; then
        printf '%s\n' "${APPLE_API_KEY}" > "${KEY_FILE}"
    else
        echo "${APPLE_API_KEY}" | base64 --decode > "${KEY_FILE}"
    fi
    chmod 600 "${KEY_FILE}"
}

# Every signed target, in signing order (inside-out). Verified and gated after signing.
SIGNED_TARGETS=()
# Deliberately empty entitlements for the app: it is not sandboxed and needs no hardened-runtime exceptions
# (the runtime itself comes from --options runtime). Keep XML comments out of that file; the parser rejects them.
APP_ENTITLEMENTS="${SCRIPT_DIR}/macos-app.entitlements"
# shellcheck source=scripts/macos-signing-lib.sh
source "${SCRIPT_DIR}/macos-signing-lib.sh"

# Apple notarization rejects com.apple.security.get-task-allow (a debug entitlement that Xcode injects into
# ad-hoc builds). Fail here, in seconds, instead of after a notary round trip.
assert_no_get_task_allow() {
    local target="$1"
    local entitlements
    if ! entitlements="$(codesign -d --entitlements - --xml "${target}" 2>/dev/null)"; then
        echo "error: cannot read the code signature of ${target}" >&2
        return 1
    fi
    if grep -q 'com.apple.security.get-task-allow' <<< "${entitlements}"; then
        echo "error: ${target} carries com.apple.security.get-task-allow; notarization would reject it." >&2
        return 1
    fi
}

verify_bundle() {
    local target="$1"
    echo "==> Verifying signature: ${target}"
    codesign --verify --deep --strict --verbose=2 "${target}"
    codesign -dvv "${target}" 2>&1
    echo "==> Entitlements for: ${target}"
    codesign -d --entitlements - --xml "${target}" 2>&1
    assert_no_get_task_allow "${target}"
}

sign_app_inside_out() {
    local app_path="$1"
    echo "==> Signing application components inside-out for: ${app_path}"

    # Every nested bundle, then the app with the explicit minimal entitlements instead of whatever the build injected
    sign_app_targets signed "${app_path}" "${APP_ENTITLEMENTS}"

    # Verify and gate every signed target, before anything is submitted to notarization
    verify_signed_targets
}

verify_signed_targets() {
    # An empty list would verify nothing and still look like success.
    if [[ -z "${SIGNED_TARGETS[0]:-}" ]]; then
        echo "error: there is no signed target to verify; refusing to report success." >&2
        exit 1
    fi
    echo "==> Verifying all signatures and entitlements..."
    local target
    for target in "${SIGNED_TARGETS[@]}"; do
        verify_bundle "${target}"
    done
}

# verify-app: check an already signed app without signing anything (post-hoc check of a built or downloaded app).
# It verifies the list that signing walks, so the two cannot disagree; a walk that fails stops here, before any check.
verify_existing_app() {
    local app_path="$1"
    load_sign_targets "${app_path}"
    SIGNED_TARGETS=("${SIGN_TARGETS[@]}")
    verify_signed_targets
}

submit_to_notarytool() {
    local target_archive="$1"
    echo "==> Submitting archive to Apple notarytool: ${target_archive}"
    local output_json
    if ! output_json="$(xcrun notarytool submit "${target_archive}" \
        --key "${KEY_FILE}" \
        --key-id "${APPLE_API_KEY_ID}" \
        --issuer "${APPLE_API_ISSUER}" \
        --wait \
        --timeout "${NOTARY_TIMEOUT}" \
        --output-format json)"; then
        echo "error: notarytool execution failed." >&2
        echo "${output_json}" >&2
        exit 1
    fi

    echo "==> notarytool JSON response:"
    echo "${output_json}"

    local status
    status="$(python3 -c "import json, sys; data = json.load(sys.stdin); print(data.get('status', ''))" <<< "${output_json}")"

    if [[ "${status}" != "Accepted" ]]; then
        echo "error: Apple notarytool rejected archive with status: '${status}'" >&2
        echo "${output_json}" >&2
        exit 1
    fi
    echo "==> Notarization successfully Accepted by Apple."
}

subcommand_app() {
    local app_path
    app_path="$(resolve_app_path "${1:-}")"
    check_notary_credentials
    prepare_notary_key

    # Step 1: Sign inside-out and verify
    sign_app_inside_out "${app_path}"

    # Step 2: Zip app for notarization submission
    local app_zip
    app_zip="${KEY_DIR}/AriadShift.zip"
    echo "==> Packaging app into zip for notary submission: ${app_zip}"
    ditto -c -k --keepParent "${app_path}" "${app_zip}"

    # Step 3: Submit zip to notarytool and require status == Accepted
    submit_to_notarytool "${app_zip}"

    # Step 4: Staple ticket to app bundle (fatal on failure)
    echo "==> Stapling notarization ticket to app bundle: ${app_path}"
    xcrun stapler staple "${app_path}"

    # Step 5: Assess app bundle with spctl
    echo "==> Assessing app bundle with spctl..."
    spctl --assess --type exec -v "${app_path}"

    echo "==> Successfully signed, notarized, stapled, and assessed application: ${app_path}"
}

subcommand_dmg() {
    local dmg_path
    dmg_path="$(resolve_dmg_path "${1:-}")"
    check_notary_credentials
    prepare_notary_key

    # Step 1: Sign DMG
    local -a keychain_args=()
    if [[ -n "${KEYCHAIN_PATH:-}" ]]; then
        keychain_args=(--keychain "${KEYCHAIN_PATH}")
    fi
    echo "==> Signing disk image: ${dmg_path}"
    codesign --force --timestamp \
        --sign "${APPLE_SIGNING_IDENTITY}" \
        ${keychain_args[@]+"${keychain_args[@]}"} \
        "${dmg_path}"

    # Step 2: Submit DMG to notarytool and require status == Accepted
    submit_to_notarytool "${dmg_path}"

    # Step 3: Staple ticket to DMG (fatal on failure)
    echo "==> Stapling notarization ticket to DMG: ${dmg_path}"
    xcrun stapler staple "${dmg_path}"

    # Step 4: Assess DMG with spctl
    echo "==> Assessing DMG with spctl..."
    spctl --assess --type open --context context:primary-signature -v "${dmg_path}"

    echo "==> Successfully signed, notarized, stapled, and assessed DMG: ${dmg_path}"
}

subcommand_sign_app() {
    local app_path
    app_path="$(resolve_app_path "${1:-}")"
    check_signing_credentials
    sign_app_inside_out "${app_path}"
    echo "==> Successfully signed and verified application: ${app_path}"
}

subcommand_verify_app() {
    local app_path
    app_path="$(resolve_app_path "${1:-}")"
    verify_existing_app "${app_path}"
    echo "==> Verified application: ${app_path}"
}

subcommand_sign_dmg() {
    local dmg_path
    dmg_path="$(resolve_dmg_path "${1:-}")"
    check_signing_credentials
    local -a keychain_args=()
    if [[ -n "${KEYCHAIN_PATH:-}" ]]; then
        keychain_args=(--keychain "${KEYCHAIN_PATH}")
    fi
    echo "==> Signing disk image: ${dmg_path}"
    codesign --force --timestamp \
        --sign "${APPLE_SIGNING_IDENTITY}" \
        ${keychain_args[@]+"${keychain_args[@]}"} \
        "${dmg_path}"
    echo "==> Verifying DMG signature..."
    codesign --verify --verbose=2 "${dmg_path}"
    codesign -dvv "${dmg_path}" 2>&1
    echo "==> Successfully signed DMG: ${dmg_path}"
}

COMMAND="${1:-}"
shift || true

case "${COMMAND}" in
    app)
        subcommand_app "$@"
        ;;
    dmg)
        subcommand_dmg "$@"
        ;;
    sign-app)
        subcommand_sign_app "$@"
        ;;
    sign-dmg)
        subcommand_sign_dmg "$@"
        ;;
    verify-app)
        subcommand_verify_app "$@"
        ;;
    *.app)
        subcommand_app "${COMMAND}" "$@"
        ;;
    *.dmg)
        subcommand_dmg "${COMMAND}" "$@"
        ;;
    *)
        echo "Usage: $0 app [APP_PATH] | $0 dmg [DMG_PATH] | $0 sign-app [APP_PATH] | $0 sign-dmg [DMG_PATH] | $0 verify-app [APP_PATH]" >&2
        exit 1
        ;;
esac
