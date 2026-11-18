#!/usr/bin/env bash
set -euo pipefail

# Prove that the release workflow itself fails on a partial Apple secret set.
# macos-check-secrets.test.sh tests the script; this test runs the real `run:` block of the
# `mode` step of .github/workflows/macos.yml the way the runner does (bash -eo pipefail), because
# the exit status of a command substitution inside echo arguments is silently discarded.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SCRIPT_DIR}")"
WORKFLOW="${REPO_ROOT}/.github/workflows/macos.yml"
ALL=(APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER APPLE_API_KEY)
failures=0

# Print the `run: |` block of the step whose id is "mode", with the YAML indentation removed.
extract_mode_block() {
    awk '
        !found && $0 ~ /^[[:space:]]+id: mode[[:space:]]*$/ { found = 1; next }
        found && !in_run && $0 ~ /^[[:space:]]+run: \|[[:space:]]*$/ {
            in_run = 1
            match($0, /^[[:space:]]+/)
            run_indent = RLENGTH
            next
        }
        found && !in_run && $0 ~ /^[[:space:]]+run: / { sub(/^[[:space:]]+run: /, ""); print; exit }
        in_run {
            match($0, /^[[:space:]]*/)
            if ($0 !~ /^[[:space:]]*$/ && RLENGTH <= run_indent) { exit }
            print substr($0, run_indent + 1)
        }
    ' "${WORKFLOW}"
}

block="$(extract_mode_block)"
if [[ -z "${block}" ]]; then
    echo "FAIL: could not extract the run block of the step with id 'mode' from ${WORKFLOW}" >&2
    exit 1
fi

# run_block LABEL BLOCK EXPECT(ok|fail) EXPECTED_OUTPUT_LINE_OR_EMPTY PRESENT_NAMES...
run_block() {
    local label="$1" script="$2" expect="$3" want_line="$4"
    shift 4
    local -a env_args=()
    local name
    for name in "$@"; do
        env_args+=("${name}=placeholder")
    done
    local out_file summary err rc=0 ok=1
    out_file="$(mktemp)"
    summary="$(mktemp)"
    err="$(mktemp)"
    (
        cd "${REPO_ROOT}"
        env -i PATH="${PATH}" GITHUB_OUTPUT="${out_file}" GITHUB_STEP_SUMMARY="${summary}" \
            ${env_args[@]+"${env_args[@]}"} \
            bash --noprofile --norc -eo pipefail -c "${script}" >/dev/null 2>"${err}"
    ) || rc=$?
    if [[ "${expect}" == "ok" ]]; then
        [[ "${rc}" -eq 0 ]] || ok=0
        grep -qxF "${want_line}" "${out_file}" || ok=0
    else
        [[ "${rc}" -ne 0 ]] || ok=0
        # A failing step must not publish any mode, not even an empty one.
        ! grep -q '^mode=' "${out_file}" || ok=0
    fi
    if [[ "${ok}" -eq 1 ]]; then
        echo "PASS ${label}: step rc=${rc} GITHUB_OUTPUT=$(tr '\n' ' ' < "${out_file}")"
    else
        echo "FAIL ${label}: expected ${expect}, got rc=${rc} GITHUB_OUTPUT='$(tr '\n' ' ' < "${out_file}")' stderr:" >&2
        cat "${err}" >&2
        failures=$((failures + 1))
    fi
    rm -f "${out_file}" "${summary}" "${err}"
}

# Control: the previous form `echo "mode=$(...)"` swallows the failure, which is why the real block is tested.
# shellcheck disable=SC2016 # the command text is deliberately literal; the inner bash expands it
old_form='echo "mode=$(bash scripts/macos-check-secrets.sh)" >> "$GITHUB_OUTPUT"'
control_rc=0
control_out="$(mktemp)"
(
    cd "${REPO_ROOT}"
    env -i PATH="${PATH}" GITHUB_OUTPUT="${control_out}" APPLE_CERTIFICATE=placeholder \
        bash --noprofile --norc -eo pipefail -c "${old_form}" >/dev/null 2>&1
) || control_rc=$?
if [[ "${control_rc}" -eq 0 && "$(cat "${control_out}")" == "mode=" ]]; then
    echo "PASS control: the old echo form exits 0 and writes an empty mode on a partial set (the defect this test guards)"
else
    echo "FAIL control: expected the old echo form to swallow the failure, got rc=${control_rc}" >&2
    failures=$((failures + 1))
fi
rm -f "${control_out}"

run_block "workflow step, 0 present" "${block}" ok "mode=unsigned"
run_block "workflow step, 6 present" "${block}" ok "mode=signed" "${ALL[@]}"
run_block "workflow step, 1 present" "${block}" fail "" APPLE_CERTIFICATE
run_block "workflow step, 3 present" "${block}" fail "" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY
run_block "workflow step, 5 present (key missing)" "${block}" fail "" APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY_ID APPLE_API_ISSUER
run_block "workflow step, 1 present (api key only)" "${block}" fail "" APPLE_API_KEY

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} case(s) failed" >&2
    exit 1
fi
echo "all cases passed"
