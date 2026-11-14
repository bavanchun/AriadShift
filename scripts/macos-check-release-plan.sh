#!/usr/bin/env bash
set -euo pipefail

# Verify that the Release AriadShift bundle (and sibling AriadShiftCore.o when present)
# or a direct Mach-O binary/object omits EnvironmentPlanProvider and ASHIFT_PLAN.
#
# Usage: macos-check-release-plan.sh [APP_OR_OBJECT_PATH]
# Defaults to target/derived-data/Build/Products/Release/AriadShift.app.
# For a *.app bundle, inspects <app>/Contents/MacOS/AriadShift (never Contents/Helpers/ashift)
# and $(dirname <app>)/AriadShiftCore.o whenever that sibling object file exists.

DEFAULT_APP_PATH="target/derived-data/Build/Products/Release/AriadShift.app"
FORBIDDEN_PATTERN='EnvironmentPlanProvider|ASHIFT_PLAN'

TMP_DIR="$(mktemp -d)"
cleanup() {
    if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
        rm -rf "${TMP_DIR}"
    fi
}
trap cleanup EXIT

errors=()

record_error() {
    local message="$1"
    echo "::error::${message}" >&2
    errors+=("${message}")
}

flush_errors_and_exit() {
    if [[ "${#errors[@]}" -gt 0 ]]; then
        if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
            {
                echo "### Error: Release plan guard failed"
                local err
                for err in "${errors[@]}"; do
                    echo "- ${err}"
                done
            } >> "${GITHUB_STEP_SUMMARY}"
        fi
        exit 1
    fi
}

inspect_macho_target() {
    local target="$1"

    if [[ ! -e "${target}" ]]; then
        record_error "Missing Mach-O target file: ${target}"
        return 0
    fi
    if [[ ! -f "${target}" ]]; then
        record_error "Mach-O target is not a regular file: ${target}"
        return 0
    fi
    if [[ ! -s "${target}" ]]; then
        record_error "Mach-O target file is empty: ${target}"
        return 0
    fi

    local slice_arches
    if ! slice_arches="$(xcrun lipo -archs "${target}" 2>&1)"; then
        local lipo_err_detail
        lipo_err_detail="$(printf '%s' "${slice_arches}" | tr '\n' ' ' | sed 's/ $//')"
        record_error "Target is not a valid Mach-O binary or object (${target}): ${lipo_err_detail}"
        return 0
    fi
    if [[ -z "${slice_arches}" ]]; then
        record_error "Target has no Mach-O architectures (${target})."
        return 0
    fi

    local nm_out nm_err nm_filtered
    nm_out="${TMP_DIR}/nm.out"
    nm_err="${TMP_DIR}/nm.err"
    nm_filtered="${TMP_DIR}/nm.filtered"
    if ! xcrun nm -arch all "${target}" > "${nm_out}" 2> "${nm_err}"; then
        local nm_err_detail
        nm_err_detail="$(tr '\n' ' ' < "${nm_err}" | sed 's/ $//')"
        record_error "Failed to inspect Mach-O symbol table for ${target}: ${nm_err_detail}"
        return 0
    fi

    # Strip universal slice headers ("<path> (for architecture <arch>):") so a parent path cannot match.
    awk -v prefix="${target} (for architecture " 'index($0, prefix) != 1 { print }' "${nm_out}" > "${nm_filtered}"

    local nm_rc=0
    LC_ALL=C grep -Eq "${FORBIDDEN_PATTERN}" "${nm_filtered}" || nm_rc=$?
    if [[ "${nm_rc}" -eq 0 ]]; then
        local matched_syms
        matched_syms="$(LC_ALL=C grep -m 5 -E "${FORBIDDEN_PATTERN}" "${nm_filtered}" | tr '\n' ';' | sed 's/;$//')"
        record_error "Forbidden debug plan symbol(s) found in ${target} [${slice_arches}]: ${matched_syms}"
    elif [[ "${nm_rc}" -ne 1 ]]; then
        record_error "Symbol table scan failed (grep exit ${nm_rc}) for ${target}"
    fi

    local raw_rc=0
    LC_ALL=C grep -aEq "${FORBIDDEN_PATTERN}" "${target}" || raw_rc=$?
    if [[ "${raw_rc}" -eq 0 ]]; then
        local matched_tokens
        matched_tokens="$(LC_ALL=C grep -aoE "${FORBIDDEN_PATTERN}" "${target}" | LC_ALL=C sort -u | tr '\n' ' ' | sed 's/ $//')"
        record_error "Forbidden debug plan string(s) (${matched_tokens}) found in raw bytes of ${target} [${slice_arches}]"
    elif [[ "${raw_rc}" -ne 1 ]]; then
        record_error "Raw binary byte scan failed (grep exit ${raw_rc}) for ${target}"
    fi
}

if [[ "$#" -gt 1 ]]; then
    record_error "Expected at most one argument ([APP_OR_OBJECT_PATH]), got $#."
    flush_errors_and_exit
fi

if [[ "$#" -eq 1 ]]; then
    input_path="$1"
else
    input_path="${DEFAULT_APP_PATH}"
fi

if [[ -z "${input_path}" ]]; then
    record_error "Target path must not be empty."
    flush_errors_and_exit
fi

while [[ "${input_path}" != "/" && "${input_path}" == */ ]]; do
    input_path="${input_path%/}"
done

targets=()
if [[ "${input_path}" == *.app ]]; then
    if [[ ! -d "${input_path}" ]]; then
        record_error "Application bundle directory not found: ${input_path}"
        flush_errors_and_exit
    fi
    targets+=("${input_path}/Contents/MacOS/AriadShift")

    parent_dir="$(dirname -- "${input_path}")"
    sibling_core="${parent_dir}/AriadShiftCore.o"
    if [[ -e "${sibling_core}" || -L "${sibling_core}" ]]; then
        targets+=("${sibling_core}")
    fi
else
    targets+=("${input_path}")
fi

for target in "${targets[@]}"; do
    inspect_macho_target "${target}"
done

flush_errors_and_exit

echo "Release plan guard passed: checked ${#targets[@]} Mach-O target(s) (${targets[*]}) with no EnvironmentPlanProvider or ASHIFT_PLAN."
