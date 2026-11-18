#!/usr/bin/env bash
set -euo pipefail

# Test macos-check-secrets.sh over the six Apple secrets and the Sparkle key: named cases for every documented row of the
# mode table, every single-secret case, empty-string values (how GitHub passes an unset secret), and an exhaustive sweep of
# all 2^7 presence combinations against an independent oracle. Values must never reach stdout, stderr or the step summary.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CHECK="${SCRIPT_DIR}/macos-check-secrets.sh"
ALL=(APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY)
KEY=SPARKLE_ED_PRIVATE_KEY
CANARY="canary-value-that-must-never-be-printed"
failures=0
cases=0

# run_case LABEL EXPECTED_STDOUT EXPECTED_RC EXPECTED_MISSING_LIST_OR_EMPTY NAME=VALUE...
# Each NAME=VALUE is exported to the script; NAME=<empty> exports an empty value, like an unset GitHub secret.
run_case() {
    local label="$1" want_out="$2" want_rc="$3" want_missing="$4"
    shift 4
    local summary out err rc=0
    summary="$(mktemp)"
    err="$(mktemp)"
    out="$(env -i PATH="${PATH}" GITHUB_STEP_SUMMARY="${summary}" "$@" bash "${CHECK}" 2>"${err}")" || rc=$?
    local ok=1
    [[ "${out}" == "${want_out}" ]] || ok=0
    [[ "${rc}" -eq "${want_rc}" ]] || ok=0
    if [[ "${want_rc}" -ne 0 ]]; then
        grep -qF "Missing: ${want_missing}" "${err}" || ok=0
        grep -qF "Missing: ${want_missing}." "${summary}" || ok=0
    else
        # A successful run prints exactly one line, the mode, and nothing on stderr or in the summary.
        [[ "$(wc -l < "${err}")" -eq 0 ]] || ok=0
        [[ ! -s "${summary}" ]] || ok=0
    fi
    # No value may leak, on any path.
    if [[ "${out}" == *"${CANARY}"* ]] || grep -qF "${CANARY}" "${err}" "${summary}"; then
        ok=0
        echo "LEAK ${label}: a secret value reached an output stream" >&2
    fi
    cases=$((cases + 1))
    if [[ "${ok}" -eq 1 ]]; then
        [[ "${QUIET:-0}" -eq 1 ]] || echo "PASS ${label}: stdout=${out:-<none>} rc=${rc} missing=${want_missing:-<none>}"
    else
        echo "FAIL ${label}: stdout='${out}' rc=${rc} stderr:" >&2
        cat "${err}" >&2
        failures=$((failures + 1))
    fi
    rm -f "${summary}" "${err}"
}

# env_args NAME... -> fills the global array ENV_ARGS with NAME=<canary> for each present name
env_args() {
    ENV_ARGS=()
    local name
    for name in "$@"; do
        ENV_ARGS+=("${name}=${CANARY}")
    done
}

# named LABEL STDOUT RC MISSING NAME...
named() {
    local label="$1" want_out="$2" want_rc="$3" want_missing="$4"
    shift 4
    env_args "$@"
    run_case "${label}" "${want_out}" "${want_rc}" "${want_missing}" ${ENV_ARGS[@]+"${ENV_ARGS[@]}"}
}

ALL_MISSING_BUT_CERT="APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY"

echo "== Mode table: all six Apple secrets =="
named "six, no Sparkle key" signed 0 "" "${ALL[@]}"
named "six + Sparkle key" signed 0 "" "${ALL[@]}" "${KEY}"

echo "== Mode table: no Apple secret =="
named "none, no Sparkle key" unsigned 0 ""
named "none + Sparkle key" adhoc 0 "" "${KEY}"

echo "== Mode table: partial Apple set fails closed, with or without the Sparkle key =="
named "1 present (certificate only)" "" 1 "${ALL_MISSING_BUT_CERT}" APPLE_CERTIFICATE
named "1 present (certificate only) + Sparkle key" "" 1 "${ALL_MISSING_BUT_CERT}" APPLE_CERTIFICATE "${KEY}"
named "3 present" "" 1 "APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY
named "3 present + Sparkle key" "" 1 "APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY "${KEY}"
named "5 present (key missing)" "" 1 "APPLE_API_KEY" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER
named "5 present (key missing) + Sparkle key" "" 1 "APPLE_API_KEY" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER "${KEY}"
# The secret listed first must never be reported missing when it is set.
named "1 present (api key only)" "" 1 "APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER" APPLE_API_KEY
named "1 present (api key only) + Sparkle key" "" 1 "APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER" APPLE_API_KEY "${KEY}"

echo "== Every single Apple secret, and every five-of-six, with and without the Sparkle key =="
for one in "${ALL[@]}"; do
    others=()
    for name in "${ALL[@]}"; do
        [[ "${name}" != "${one}" ]] && others+=("${name}")
    done
    named "only ${one}" "" 1 "${others[*]}" "${one}"
    named "only ${one} + Sparkle key" "" 1 "${others[*]}" "${one}" "${KEY}"
    named "all but ${one}" "" 1 "${one}" "${others[@]}"
    named "all but ${one} + Sparkle key" "" 1 "${one}" "${others[@]}" "${KEY}"
done

echo "== Empty values count as absent, the way GitHub passes an unset secret =="
empties=()
for name in "${ALL[@]}" "${KEY}"; do
    empties+=("${name}=")
done
run_case "all seven exported but empty" unsigned 0 "" "${empties[@]}"
only_key=()
for name in "${ALL[@]}"; do
    only_key+=("${name}=")
done
run_case "six empty + Sparkle key" adhoc 0 "" "${only_key[@]}" "${KEY}=${CANARY}"
all_set=()
for name in "${ALL[@]}"; do
    all_set+=("${name}=${CANARY}")
done
run_case "six set + empty Sparkle key" signed 0 "" "${all_set[@]}" "${KEY}="
run_case "five set, one empty + empty Sparkle key" "" 1 "APPLE_API_KEY" \
    APPLE_CERTIFICATE="${CANARY}" APPLE_CERTIFICATE_PASSWORD="${CANARY}" APPLE_SIGNING_IDENTITY="${CANARY}" \
    APPLE_API_KEY_ID="${CANARY}" APPLE_API_ISSUER="${CANARY}" APPLE_API_KEY= "${KEY}="

echo "== Exhaustive sweep: all 2^7 presence combinations against an independent oracle =="
QUIET=1
sweep_failures_before="${failures}"
for ((mask = 0; mask < 128; mask++)); do
    names=()
    missing_names=()
    for ((bit = 0; bit < 6; bit++)); do
        if (( (mask >> bit) & 1 )); then
            names+=("${ALL[bit]}")
        else
            missing_names+=("${ALL[bit]}")
        fi
    done
    count="${#names[@]}"
    key_set=$(( (mask >> 6) & 1 ))
    if (( count == 6 )); then
        want_out=signed want_rc=0 want_missing=""
    elif (( count == 0 && key_set == 1 )); then
        want_out=adhoc want_rc=0 want_missing=""
    elif (( count == 0 )); then
        want_out=unsigned want_rc=0 want_missing=""
    else
        want_out="" want_rc=1 want_missing="${missing_names[*]}"
    fi
    if (( key_set == 1 )); then
        names+=("${KEY}")
    fi
    named "mask ${mask} (apple=${count}, sparkle=${key_set})" "${want_out}" "${want_rc}" "${want_missing}" ${names[@]+"${names[@]}"}
done
QUIET=0
if [[ "${failures}" -eq "${sweep_failures_before}" ]]; then
    echo "PASS exhaustive sweep: all 128 combinations match the oracle"
fi

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} of ${cases} case(s) failed" >&2
    exit 1
fi
echo "all ${cases} cases passed"
