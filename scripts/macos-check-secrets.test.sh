#!/usr/bin/env bash
set -euo pipefail

# Test macos-check-secrets.sh with 0, 1, 3 and 6 of the six secrets present (plus every single-secret case).

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CHECK="${SCRIPT_DIR}/macos-check-secrets.sh"
ALL=(APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY)
failures=0

# run_case LABEL EXPECTED_STDOUT EXPECTED_RC EXPECTED_MISSING_LIST_OR_EMPTY PRESENT_NAMES...
run_case() {
    local label="$1" want_out="$2" want_rc="$3" want_missing="$4"
    shift 4
    local -a env_args=()
    local name
    for name in "$@"; do
        env_args+=("${name}=placeholder")
    done
    local summary out err rc=0
    summary="$(mktemp)"
    err="$(mktemp)"
    out="$(env -i PATH="${PATH}" GITHUB_STEP_SUMMARY="${summary}" ${env_args[@]+"${env_args[@]}"} bash "${CHECK}" 2>"${err}")" || rc=$?
    local ok=1
    [[ "${out}" == "${want_out}" ]] || ok=0
    [[ "${rc}" -eq "${want_rc}" ]] || ok=0
    if [[ "${want_rc}" -ne 0 ]]; then
        grep -qF "Missing: ${want_missing}" "${err}" || ok=0
        grep -qF "Missing: ${want_missing}." "${summary}" || ok=0
    fi
    if [[ "${ok}" -eq 1 ]]; then
        echo "PASS ${label}: stdout=${out:-<none>} rc=${rc} missing=${want_missing:-<none>}"
    else
        echo "FAIL ${label}: stdout='${out}' rc=${rc} stderr:" >&2
        cat "${err}" >&2
        failures=$((failures + 1))
    fi
    rm -f "${summary}" "${err}"
}

run_case "0 present" "unsigned" 0 ""
run_case "1 present (certificate only)" "" 1 "APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY" APPLE_CERTIFICATE
run_case "3 present" "" 1 "APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY
run_case "5 present (key missing)" "" 1 "APPLE_API_KEY" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER
run_case "6 present" "signed" 0 "" "${ALL[@]}"
# The secret listed first must never be reported missing when it is set.
run_case "1 present (api key only)" "" 1 "APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER" APPLE_API_KEY

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} case(s) failed" >&2
    exit 1
fi
echo "all cases passed"
