#!/usr/bin/env bash
set -euo pipefail

# Test macos-adhoc-sign.sh on any OS with a stub `codesign` that records its arguments. This cannot prove that real
# codesign accepts the result (only a macOS runner can). It proves the ORDER and the FLAGS the script passes, and that
# every gate fails closed: inside-out order, `--force --sign -` with the hardened runtime and no secure timestamp,
# never `--deep` when signing, the explicit entitlements only on the app, and a verify gate that rejects a failed
# verification, a leftover non-ad-hoc identity, a lost hardened runtime and a get-task-allow entitlement.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SIGN="${SCRIPT_DIR}/macos-adhoc-sign.sh"
ENTITLEMENTS="${SCRIPT_DIR}/macos-app.entitlements"
failures=0
checks=0

TMP_DIR="$(mktemp -d)"
cleanup() {
    if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
        rm -rf "${TMP_DIR}"
    fi
}
trap cleanup EXIT

# The stub codesign logs every call as one line, then answers by the form of the call. A STUB_* pattern is a substring of
# the target path, or the exact path when it starts with "=".
#   --verify ...             exit 1 when the target matches STUB_FAIL_VERIFY, else 0
#   -dvv TARGET              prints a CodeDirectory line; STUB_FLAGS (flag names) replaces "adhoc,runtime" for STUB_FLAGS_FOR
#   -d --entitlements - ...  prints a plist, with get-task-allow when the target matches STUB_GTA
#   anything else (signing)  exit 1 when the target matches STUB_FAIL_SIGN, else 0
STUB_DIR="${TMP_DIR}/stub"
mkdir -p "${STUB_DIR}"
cat > "${STUB_DIR}/codesign" <<'STUB'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "${STUB_LOG:?}"
target=""
for argument in "$@"; do
    target="${argument}"
done
matches() {
    local pattern="${1:-}"
    [[ -n "${pattern}" ]] || return 1
    if [[ "${pattern}" == =* ]]; then
        [[ "${target}" == "${pattern#=}" ]]
    else
        [[ "${target}" == *"${pattern}"* ]]
    fi
}
case "${1:-}" in
    --verify)
        if matches "${STUB_FAIL_VERIFY:-}"; then
            echo "stub: invalid signature for ${target}" >&2
            exit 1
        fi
        ;;
    -dvv)
        flags="adhoc,runtime"
        if matches "${STUB_FLAGS_FOR:-}"; then
            flags="${STUB_FLAGS:-}"
        fi
        echo "Executable=${target}" >&2
        echo "CodeDirectory v=20500 size=600 flags=0x10002(${flags}) hashes=10+2 location=embedded" >&2
        ;;
    -d)
        echo '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>'
        if matches "${STUB_GTA:-}"; then
            echo '<key>com.apple.security.get-task-allow</key><true/>'
        fi
        echo '</dict></plist>'
        ;;
    *)
        if matches "${STUB_FAIL_SIGN:-}"; then
            echo "stub: cannot sign ${target}" >&2
            exit 1
        fi
        ;;
esac
exit 0
STUB
chmod +x "${STUB_DIR}/codesign"

# make_app DIR [with-sparkle]: a fake bundle shaped like the real one: Sparkle with two XPC services, Autoupdate,
# Updater.app and the Current symlink, one more framework, one loose dylib and the ashift helper.
make_app() {
    local app="$1"
    mkdir -p "${app}/Contents/Helpers" "${app}/Contents/MacOS"
    : > "${app}/Contents/Helpers/ashift"
    : > "${app}/Contents/MacOS/AriadShift"
    if [[ "${2:-}" == "with-sparkle" ]]; then
        local sparkle="${app}/Contents/Frameworks/Sparkle.framework"
        mkdir -p "${sparkle}/Versions/B/XPCServices/Downloader.xpc" \
            "${sparkle}/Versions/B/XPCServices/Installer.xpc" \
            "${sparkle}/Versions/B/Updater.app" \
            "${app}/Contents/Frameworks/Other.framework"
        : > "${sparkle}/Versions/B/Autoupdate"
        ln -s B "${sparkle}/Versions/Current"
        : > "${app}/Contents/Frameworks/libextra.dylib"
    fi
}

APP="${TMP_DIR}/Fake/AriadShift.app"
make_app "${APP}" with-sparkle
FW="${APP}/Contents/Frameworks"
SPK="${FW}/Sparkle.framework"
XPC_DOWNLOADER="${SPK}/Versions/B/XPCServices/Downloader.xpc"
XPC_INSTALLER="${SPK}/Versions/B/XPCServices/Installer.xpc"
AUTOUPDATE="${SPK}/Versions/B/Autoupdate"
UPDATER="${SPK}/Versions/B/Updater.app"
DYLIB="${FW}/libextra.dylib"
OTHER="${FW}/Other.framework"
HELPER="${APP}/Contents/Helpers/ashift"

LOG="${TMP_DIR}/codesign.log"
OUT="${TMP_DIR}/out.txt"

# run_sign ARGS... [-- ENV=VALUE...]: runs the script with the stub first on PATH and no inherited environment.
# The codesign log lands in LOG and the script output in OUT.
run_sign() {
    local -a args=() envs=()
    local seen_sep=0 item
    for item in "$@"; do
        if [[ "${item}" == "--" && "${seen_sep}" -eq 0 ]]; then
            seen_sep=1
        elif [[ "${seen_sep}" -eq 1 ]]; then
            envs+=("${item}")
        else
            args+=("${item}")
        fi
    done
    : > "${LOG}"
    local rc=0
    env -i PATH="${STUB_DIR}:${PATH}" STUB_LOG="${LOG}" ${envs[@]+"${envs[@]}"} \
        bash "${SIGN}" ${args[@]+"${args[@]}"} > "${OUT}" 2>&1 || rc=$?
    return "${rc}"
}

check() {
    local label="$1" ok="$2"
    checks=$((checks + 1))
    if [[ "${ok}" -eq 0 ]]; then
        echo "PASS ${label}"
    else
        echo "FAIL ${label}" >&2
        failures=$((failures + 1))
    fi
}

# Succeeds when the previous command did; used as: expect "label" command args...
expect() {
    local label="$1"
    shift
    if "$@"; then check "${label}" 0; else check "${label}" 1; fi
}

# Logged signing calls (they start with --force), as "line-number:args". The target is the last argument.
sign_calls() { grep -n -- '^--force ' "${LOG}" || true; }
# Signing targets in order, one per line.
sign_targets() { grep -- '^--force ' "${LOG}" | awk '{print $NF}' || true; }
# Position (1-based) of a target in the signing order; empty when not signed.
sign_index() { sign_targets | grep -n -x -F -- "$1" | head -n 1 | cut -d: -f1 || true; }
# The logged signing line for one target.
sign_line() { grep -- '^--force ' "${LOG}" | awk -v t="$1" '$NF == t' || true; }
# The first line (1-based) of the log that is a verification, and the last that is a signing call.
first_verify_line() { grep -n -- '^--verify ' "${LOG}" | head -n 1 | cut -d: -f1 || true; }
last_sign_line() { grep -n -- '^--force ' "${LOG}" | tail -n 1 | cut -d: -f1 || true; }
# True when the target is signed before the other: before A B
before() {
    local a b
    a="$(sign_index "$1")"
    b="$(sign_index "$2")"
    [[ -n "${a}" && -n "${b}" && "${a}" -lt "${b}" ]]
}
count_lines() { wc -l | tr -d ' '; }
# True when the file does not contain the fixed string.
lacks_file() { ! grep -q -F -- "$2" "$1"; }
# True when the text does not contain the fixed string.
lacks_text() { [[ "$1" != *"$2"* ]]; }

echo "== Success path on a bundle shaped like the real one =="
rc=0
run_sign app "${APP}" || rc=$?
check "exits 0" "${rc}"
signed="$(sign_targets | count_lines)"
check "signs exactly 9 targets (4 Sparkle parts, Sparkle, dylib, Other.framework, helper, app), got ${signed}" "$((signed == 9 ? 0 : 1))"
for t in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}" "${SPK}" "${DYLIB}" "${OTHER}" "${HELPER}" "${APP}"; do
    expect "signs ${t#"${TMP_DIR}/Fake/"}" test -n "$(sign_index "${t}")"
done
expect "never signs the Versions/Current symlink" test -z "$(sign_index "${SPK}/Versions/Current")"

echo "== Order: deepest first, the app last =="
for nested in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}"; do
    expect "${nested##*/} is signed before Sparkle.framework" before "${nested}" "${SPK}"
done
expect "Sparkle.framework is signed before the app" before "${SPK}" "${APP}"
expect "the loose dylib is signed before Other.framework" before "${DYLIB}" "${OTHER}"
expect "Other.framework is signed before the app" before "${OTHER}" "${APP}"
expect "the helper is signed before the app" before "${HELPER}" "${APP}"
expect "the app is the last target signed" test "$(sign_targets | tail -n 1)" = "${APP}"

echo "== Flags on every signing call =="
all_ok=0
while IFS= read -r line; do
    read -ra tok <<< "${line}"
    signs=0 sign_ok=0 deep=0 bare_ts=0 ts_none=0 force=0 runtime=0 keychain=0
    for ((i = 0; i < ${#tok[@]}; i++)); do
        case "${tok[i]}" in
            --sign) signs=$((signs + 1)); [[ "${tok[i + 1]:-}" == "-" ]] && sign_ok=1 ;;
            --deep) deep=1 ;;
            --timestamp) bare_ts=1 ;;
            --timestamp=none) ts_none=1 ;;
            --force) force=1 ;;
            --options) [[ "${tok[i + 1]:-}" == "runtime" ]] && runtime=1 ;;
            --keychain) keychain=1 ;;
        esac
    done
    if [[ "${signs}" -ne 1 || "${sign_ok}" -ne 1 || "${deep}" -ne 0 || "${bare_ts}" -ne 0 || "${ts_none}" -ne 1 \
        || "${force}" -ne 1 || "${runtime}" -ne 1 || "${keychain}" -ne 0 ]]; then
        echo "  bad signing call: ${line}" >&2
        all_ok=1
    fi
done < <(grep -- '^--force ' "${LOG}")
check "every call is --force --sign - --options runtime --timestamp=none, with no --deep, no secure timestamp, no keychain" "${all_ok}"

echo "== Entitlements and preserved metadata =="
app_line="$(sign_line "${APP}")"
expect "the app is signed with the explicit entitlements file" grep -q -F -- " --entitlements ${ENTITLEMENTS} " <<< " ${app_line}"
expect "the app does not preserve nested metadata" lacks_text "${app_line}" "--preserve-metadata"
nested_ok=0
for nested in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}" "${SPK}" "${DYLIB}" "${OTHER}" "${HELPER}"; do
    line="$(sign_line "${nested}")"
    if [[ "${line}" != *"--preserve-metadata=identifier,entitlements"* || "${line}" == *"--entitlements "* \
        || "${line}" == *requirements* ]]; then
        echo "  bad nested call: ${line}" >&2
        nested_ok=1
    fi
done
check "nested code keeps identifier and entitlements, gets no entitlements file, never preserves requirements" "${nested_ok}"

echo "== Verification gate =="
first_verify="$(first_verify_line)"
last_sign="$(last_sign_line)"
check "verification starts only after the last signing call" "$(( first_verify > last_sign ? 0 : 1 ))"
verify_ok=0
for t in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}" "${SPK}" "${DYLIB}" "${OTHER}" "${HELPER}" "${APP}"; do
    grep -q -F -- "--verify --deep --strict --verbose=2 ${t}" "${LOG}" || { echo "  not verified: ${t}" >&2; verify_ok=1; }
done
check "every signed target, the app included, is verified with --verify --deep --strict" "${verify_ok}"
expect "the script says it verified the app" grep -q "Ad-hoc signed and verified" "${OUT}"

echo "== The environment cannot change the identity =="
rc=0
run_sign app "${APP}" -- APPLE_SIGNING_IDENTITY="Developer ID Application: Example" KEYCHAIN_PATH=/nonexistent/k.keychain \
    APPLE_CERTIFICATE=canary-cert-value SPARKLE_ED_PRIVATE_KEY=canary-sparkle-value || rc=$?
check "still exits 0 with Apple and Sparkle variables set" "${rc}"
expect "no call names a Developer ID identity" lacks_file "${LOG}" "Developer ID"
expect "no call names a keychain" lacks_file "${LOG}" "--keychain"
expect "no secret value reaches a codesign call" lacks_file "${LOG}" "canary"
expect "no secret value reaches the output" lacks_file "${OUT}" "canary"

echo "== Gate failures exit non-zero =="
rc=0
run_sign app "${APP}" -- "STUB_FAIL_VERIFY==${AUTOUPDATE}" || rc=$?
check "a failed nested verification fails the run" "$((rc != 0 ? 0 : 1))"
rc=0
run_sign app "${APP}" -- "STUB_FAIL_VERIFY==${APP}" || rc=$?
check "a failed verification of the app itself fails the run" "$((rc != 0 ? 0 : 1))"
expect "it names the failing signature" grep -q "invalid signature" "${OUT}"

rc=0
run_sign app "${APP}" -- "STUB_FLAGS_FOR==${HELPER}" STUB_FLAGS=adhoc || rc=$?
check "a lost hardened runtime fails the run" "$((rc != 0 ? 0 : 1))"
expect "it reports the runtime flag" grep -q "hardened runtime" "${OUT}"

rc=0
run_sign app "${APP}" -- "STUB_FLAGS_FOR==${UPDATER}" STUB_FLAGS=runtime || rc=$?
check "a leftover non-ad-hoc identity fails the run" "$((rc != 0 ? 0 : 1))"
expect "it reports the identity" grep -q "not ad-hoc signed" "${OUT}"

rc=0
run_sign app "${APP}" -- "STUB_FLAGS_FOR==${APP}" STUB_FLAGS= || rc=$?
check "an app with no readable flags fails the run" "$((rc != 0 ? 0 : 1))"

rc=0
run_sign app "${APP}" -- "STUB_GTA==${APP}" || rc=$?
check "get-task-allow on the app fails the run" "$((rc != 0 ? 0 : 1))"
expect "it reports get-task-allow" grep -q "get-task-allow" "${OUT}"
rc=0
run_sign app "${APP}" -- "STUB_GTA==${XPC_INSTALLER}" || rc=$?
check "get-task-allow on a nested bundle fails the run" "$((rc != 0 ? 0 : 1))"

rc=0
run_sign app "${APP}" -- "STUB_FAIL_SIGN==${SPK}" || rc=$?
check "a signing failure fails the run" "$((rc != 0 ? 0 : 1))"
expect "after a signing failure the app is never signed" test -z "$(sign_index "${APP}")"
expect "after a signing failure nothing is verified" test -z "$(first_verify_line)"

echo "== Other bundle shapes and bad arguments =="
APP2="${TMP_DIR}/Plain/AriadShift.app"
make_app "${APP2}"
rc=0
run_sign app "${APP2}" || rc=$?
check "a bundle without Sparkle or Frameworks signs and exits 0" "${rc}"
expect "it signs only the helper and the app, in that order" test "$(sign_targets | tr '\n' ' ')" = "${APP2}/Contents/Helpers/ashift ${APP2} "

rc=0
run_sign app || rc=$?
check "no app path fails" "$((rc != 0 ? 0 : 1))"
expect "and runs no codesign" test ! -s "${LOG}"
rc=0
run_sign app "${TMP_DIR}/Missing/AriadShift.app" || rc=$?
check "a missing app fails" "$((rc != 0 ? 0 : 1))"
expect "and runs no codesign" test ! -s "${LOG}"
mkdir -p "${TMP_DIR}/NotAnApp"
rc=0
run_sign app "${TMP_DIR}/NotAnApp" || rc=$?
check "a directory that is not a .app fails" "$((rc != 0 ? 0 : 1))"
expect "and runs no codesign" test ! -s "${LOG}"
rc=0
run_sign sign-dmg "${APP}" || rc=$?
check "an unknown subcommand fails" "$((rc != 0 ? 0 : 1))"
rc=0
run_sign || rc=$?
check "no subcommand fails" "$((rc != 0 ? 0 : 1))"

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} of ${checks} check(s) failed" >&2
    exit 1
fi
echo "all ${checks} checks passed"
