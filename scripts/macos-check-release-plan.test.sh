#!/usr/bin/env bash
set -euo pipefail

# Unit tests for scripts/macos-check-release-plan.sh using tiny C/Mach-O fixtures
# built with xcrun clang and xcrun lipo inside a temporary directory.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CHECK="${SCRIPT_DIR}/macos-check-release-plan.sh"

TMP_DIR="$(mktemp -d)"
cleanup() {
    if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
        rm -rf "${TMP_DIR}"
    fi
}
trap cleanup EXIT

SRC_DIR="${TMP_DIR}/src"
BIN_DIR="${TMP_DIR}/bin"
mkdir -p "${SRC_DIR}" "${BIN_DIR}"

cat <<'EOF' > "${SRC_DIR}/clean.c"
int main(void) { return 0; }
EOF

cat <<'EOF' > "${SRC_DIR}/obj_clean.c"
int plan_helper_stub(void) { return 0; }
EOF

cat <<'EOF' > "${SRC_DIR}/ashift_plan.c"
const char *read_plan_key(void) { return "ASHIFT_PLAN"; }
int main(void) { return read_plan_key()[0] == 0; }
EOF

cat <<'EOF' > "${SRC_DIR}/obj_ashift_plan.c"
const char *debug_plan_key(void) { return "ASHIFT_PLAN"; }
EOF

cat <<'EOF' > "${SRC_DIR}/env_provider.c"
int EnvironmentPlanProvider(void) { return 1; }
int main(void) { return EnvironmentPlanProvider() - 1; }
EOF

# Build reusable Mach-O slices and objects in parallel so all cases finish in under 1 second.
pids=()
xcrun clang -arch arm64 -c "${SRC_DIR}/obj_clean.c" -o "${BIN_DIR}/clean_core.o" &
pids+=("$!")
xcrun clang -arch arm64 -c "${SRC_DIR}/obj_ashift_plan.c" -o "${BIN_DIR}/dirty_core.o" &
pids+=("$!")
xcrun clang -arch arm64 "${SRC_DIR}/clean.c" -o "${BIN_DIR}/clean_arm64" &
pids+=("$!")
xcrun clang -arch x86_64 "${SRC_DIR}/clean.c" -o "${BIN_DIR}/clean_x86_64" &
pids+=("$!")
xcrun clang -arch x86_64 "${SRC_DIR}/ashift_plan.c" -o "${BIN_DIR}/dirty_x86_64" &
pids+=("$!")
xcrun clang -arch arm64 "${SRC_DIR}/env_provider.c" -o "${BIN_DIR}/env_provider_bin" &
pids+=("$!")
for pid in "${pids[@]}"; do
    wait "${pid}"
done

xcrun lipo -create "${BIN_DIR}/clean_arm64" "${BIN_DIR}/clean_x86_64" -output "${BIN_DIR}/clean_univ"
xcrun lipo -create "${BIN_DIR}/clean_arm64" "${BIN_DIR}/dirty_x86_64" -output "${BIN_DIR}/dirty_x86_univ"

failures=0
case_idx=0

run_case() {
    local label="$1" target="$2" want_rc="$3" want_err_substr="$4"
    case_idx=$((case_idx + 1))
    local out err summary rc=0 ok=1
    out="${TMP_DIR}/case_${case_idx}.out"
    err="${TMP_DIR}/case_${case_idx}.err"
    summary="${TMP_DIR}/case_${case_idx}.summary"

    GITHUB_STEP_SUMMARY="${summary}" bash "${CHECK}" "${target}" >"${out}" 2>"${err}" || rc=$?

    [[ "${rc}" -eq "${want_rc}" ]] || ok=0
    if [[ "${want_rc}" -ne 0 ]]; then
        grep -qF "::error::" "${err}" || ok=0
        if [[ -n "${want_err_substr}" ]]; then
            grep -qF "${want_err_substr}" "${err}" || ok=0
            grep -qF "${want_err_substr}" "${summary}" || ok=0
        fi
        [[ -s "${summary}" ]] || ok=0
    else
        [[ ! -s "${err}" ]] || ok=0
        [[ ! -s "${summary}" ]] || ok=0
    fi

    if [[ "${ok}" -eq 1 ]]; then
        echo "PASS ${label}: rc=${rc}"
    else
        echo "FAIL ${label}: want_rc=${want_rc} got_rc=${rc} stderr:" >&2
        cat "${err}" >&2
        echo "summary:" >&2
        cat "${summary}" >&2 || true
        failures=$((failures + 1))
    fi
}

# Case 1: Clean standalone Mach-O .o object (AriadShiftCore.o) -> rc=0
case1_dir="${TMP_DIR}/case1"
mkdir -p "${case1_dir}"
cp "${BIN_DIR}/clean_core.o" "${case1_dir}/AriadShiftCore.o"
run_case "1. clean standalone AriadShiftCore.o" "${case1_dir}/AriadShiftCore.o" 0 ""

# Case 2: Clean universal AriadShift.app whose Contents/Helpers/ashift contains ASHIFT_PLAN -> rc=0
case2_app="${TMP_DIR}/case2/AriadShift.app"
mkdir -p "${case2_app}/Contents/MacOS" "${case2_app}/Contents/Helpers"
cp "${BIN_DIR}/clean_univ" "${case2_app}/Contents/MacOS/AriadShift"
cp "${BIN_DIR}/dirty_x86_univ" "${case2_app}/Contents/Helpers/ashift"
LC_ALL=C grep -aFq "ASHIFT_PLAN" "${case2_app}/Contents/Helpers/ashift"
run_case "2. clean universal app with ASHIFT_PLAN in helper" "${case2_app}" 0 ""

# Case 3: Clean AriadShift.app accompanied by sibling AriadShiftCore.o (clean -> rc=0; dirty -> rc=1)
case3_dir="${TMP_DIR}/case3"
case3_app="${case3_dir}/AriadShift.app"
mkdir -p "${case3_app}/Contents/MacOS"
cp "${BIN_DIR}/clean_univ" "${case3_app}/Contents/MacOS/AriadShift"
cp "${BIN_DIR}/clean_core.o" "${case3_dir}/AriadShiftCore.o"
run_case "3a. clean app with sibling clean AriadShiftCore.o" "${case3_app}" 0 ""

cp "${BIN_DIR}/dirty_core.o" "${case3_dir}/AriadShiftCore.o"
run_case "3b. clean app with sibling dirty AriadShiftCore.o" "${case3_app}" 1 "AriadShiftCore.o"

# Case 4: Mach-O binary exporting symbol EnvironmentPlanProvider -> rc=1
xcrun nm -arch all "${BIN_DIR}/env_provider_bin" | grep -qF "EnvironmentPlanProvider"
run_case "4. Mach-O binary exporting EnvironmentPlanProvider" "${BIN_DIR}/env_provider_bin" 1 "EnvironmentPlanProvider"

# Case 5: Universal AriadShift.app where arm64 is clean and ONLY x86_64 contains ASHIFT_PLAN -> rc=1
case5_app="${TMP_DIR}/case5/AriadShift.app"
mkdir -p "${case5_app}/Contents/MacOS"
cp "${BIN_DIR}/dirty_x86_univ" "${case5_app}/Contents/MacOS/AriadShift"
[[ "$(xcrun lipo -archs "${case5_app}/Contents/MacOS/AriadShift")" == *"arm64"* ]]
[[ "$(xcrun lipo -archs "${case5_app}/Contents/MacOS/AriadShift")" == *"x86_64"* ]]
run_case "5. universal app with ASHIFT_PLAN in x86_64 slice only" "${case5_app}" 1 "ASHIFT_PLAN"

# Case 6: Missing path -> rc=1
run_case "6a. missing direct path" "${TMP_DIR}/does-not-exist" 1 "does-not-exist"
run_case "6b. missing app bundle path" "${TMP_DIR}/Missing.app" 1 "Missing.app"

# Case 7: Non-Mach-O plain-text file and .app missing Contents/MacOS/AriadShift -> rc=1
printf 'not a mach-o binary\n' > "${TMP_DIR}/plain.txt"
run_case "7a. non-Mach-O plain-text file" "${TMP_DIR}/plain.txt" 1 "plain.txt"

case7_app="${TMP_DIR}/case7/Incomplete.app"
mkdir -p "${case7_app}/Contents/MacOS"
run_case "7b. .app missing Contents/MacOS/AriadShift" "${case7_app}" 1 "Contents/MacOS/AriadShift"

: > "${TMP_DIR}/empty.o"
run_case "7c. empty Mach-O target file" "${TMP_DIR}/empty.o" 1 "empty.o"

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} case(s) failed" >&2
    exit 1
fi
echo "all ${case_idx} cases passed"
