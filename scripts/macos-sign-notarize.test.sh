#!/usr/bin/env bash
set -euo pipefail

# Test the signing walk and flags of macos-sign-notarize.sh (the Developer ID path, unused today but kept working) on
# any OS with a stub `codesign` that records its arguments. `sign-app` and `verify-app` need no notary credential.
# This proves what gets signed, in which order and with which flags: inside-out (Sparkle parts, Sparkle, dylibs before
# frameworks, every Mach-O file in Helpers, the app), the exact Developer ID options on every call, never --deep, a
# file in Helpers that is not code left alone, and a verify gate that fails closed. `verify-app` checks the same list.
# Real codesign, Apple identities and notarization are only reachable on a macOS runner with the six Apple secrets.
#
# The bundle shape and the order checks mirror scripts/macos-adhoc-sign.test.sh; that test also runs this script on
# the same bundle and requires the same order, and checks that the walk fails closed with a fake walk. Keep the two in
# step. The script runs under the interpreter this test runs under ("${BASH}"), so a run under /bin/bash 3.2 exercises 3.2.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SIGN="${SCRIPT_DIR}/macos-sign-notarize.sh"
ENTITLEMENTS="${SCRIPT_DIR}/macos-app.entitlements"
IDENTITY="Developer ID Application: Example"
failures=0
checks=0

TMP_DIR="$(mktemp -d)"
cleanup() {
    # A directory a check made unreadable would stop rm; give the owner access back first.
    chmod -R u+rwx "${TMP_DIR}" 2> /dev/null || true
    rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

# The stub codesign logs every call as one line, then answers by the form of the call. A STUB_* pattern is a substring
# of the target path, or the exact path when it starts with "=".
#   --verify ...             exit 1 when the target matches STUB_FAIL_VERIFY, else 0
#   -dvv TARGET              prints a CodeDirectory line
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
    -dvv) echo "CodeDirectory v=20500 flags=0x10000(runtime)" >&2 ;;
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

# Helpers are recognised as code by their first four bytes: a fat binary and a 64-bit little-endian Mach-O.
write_macho() { printf '\317\372\355\376' > "$1"; }
write_fat_macho() { printf '\312\376\272\276' > "$1"; }

# make_app DIR [with-extras]: a fake bundle shaped like the real one. Always: Sparkle with two XPC services, an updater
# tool, Updater.app and the Current symlink, plus Helpers with the ashift helper and Pandoc (both Mach-O) beside a
# note, an empty file and a two-byte file, none of them code, which must stay unsigned. With "with-extras": one more
# framework, one loose dylib, and a
# framework that holds a dylib (the case where signing order decides whether the framework's seal survives).
make_app() {
    local app="$1"
    local sparkle="${app}/Contents/Frameworks/Sparkle.framework"
    mkdir -p "${app}/Contents/Helpers" "${app}/Contents/MacOS" \
        "${sparkle}/Versions/B/XPCServices/Downloader.xpc" \
        "${sparkle}/Versions/B/XPCServices/Installer.xpc" \
        "${sparkle}/Versions/B/Updater.app"
    write_fat_macho "${app}/Contents/Helpers/ashift"
    write_macho "${app}/Contents/Helpers/pandoc"
    echo "not code" > "${app}/Contents/Helpers/NOTES.txt"
    : > "${app}/Contents/Helpers/EMPTY"
    printf '\312\376' > "${app}/Contents/Helpers/SHORT"
    : > "${app}/Contents/MacOS/AriadShift"
    : > "${sparkle}/Versions/B/Autoupdate"
    ln -s B "${sparkle}/Versions/Current"
    if [[ "${2:-}" == "with-extras" ]]; then
        mkdir -p "${app}/Contents/Frameworks/Other.framework" "${app}/Contents/Frameworks/Inner.framework"
        : > "${app}/Contents/Frameworks/libextra.dylib"
        : > "${app}/Contents/Frameworks/Inner.framework/libinner.dylib"
    fi
}

APP="${TMP_DIR}/Fake/AriadShift.app"
make_app "${APP}" with-extras
FW="${APP}/Contents/Frameworks"
SPK="${FW}/Sparkle.framework"
XPC_DOWNLOADER="${SPK}/Versions/B/XPCServices/Downloader.xpc"
XPC_INSTALLER="${SPK}/Versions/B/XPCServices/Installer.xpc"
UPDATE_TOOL="${SPK}/Versions/B/Autoupdate"
UPDATER="${SPK}/Versions/B/Updater.app"
DYLIB="${FW}/libextra.dylib"
OTHER="${FW}/Other.framework"
INNER="${FW}/Inner.framework"
INNER_DYLIB="${INNER}/libinner.dylib"
HELPER="${APP}/Contents/Helpers/ashift"
PANDOC="${APP}/Contents/Helpers/pandoc"
NOTES="${APP}/Contents/Helpers/NOTES.txt"
EMPTY_FILE="${APP}/Contents/Helpers/EMPTY"
SHORT_FILE="${APP}/Contents/Helpers/SHORT"

LOG="${TMP_DIR}/codesign.log"
OUT="${TMP_DIR}/out.txt"

# run_sign ARGS... [-- ENV=VALUE...]: runs the script with the stub first on PATH and no inherited environment but the
# identity. The codesign log lands in LOG and the script output in OUT.
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
    env -i PATH="${STUB_DIR}:${PATH}" STUB_LOG="${LOG}" APPLE_SIGNING_IDENTITY="${IDENTITY}" ${envs[@]+"${envs[@]}"} \
        "${BASH}" "${SIGN}" ${args[@]+"${args[@]}"} > "${OUT}" 2>&1 || rc=$?
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

expect() {
    local label="$1"
    shift
    if "$@"; then check "${label}" 0; else check "${label}" 1; fi
}

# Signing calls start with --force; the target is the last argument.
sign_targets() { grep -- '^--force ' "${LOG}" | awk '{print $NF}' || true; }
sign_index() { sign_targets | grep -n -x -F -- "$1" | head -n 1 | cut -d: -f1 || true; }
sign_lines() { grep -- '^--force ' "${LOG}" || true; }
logged_line() { grep -- '^--force ' "${LOG}" | awk -v t="$1" '$NF == t' || true; }
first_verify_line() { grep -n -- '^--verify ' "${LOG}" | head -n 1 | cut -d: -f1 || true; }
last_sign_line() { grep -n -- '^--force ' "${LOG}" | tail -n 1 | cut -d: -f1 || true; }
before() {
    local a b
    a="$(sign_index "$1")"
    b="$(sign_index "$2")"
    [[ -n "${a}" && -n "${b}" && "${a}" -lt "${b}" ]]
}
lacks_file() { ! grep -q -F -- "$2" "$1"; }

# The exact argument list the Developer ID path passes, as the stub logs it (arguments joined by one space).
# nested_line TARGET [KEYCHAIN]: nested code keeps the entitlements, requirements, flags and runtime it was built with.
nested_line() {
    local keychain=""
    [[ -z "${2:-}" ]] || keychain=" --keychain $2"
    printf '%s' "--force --options runtime --timestamp --preserve-metadata=entitlements,requirements,flags,runtime --sign ${IDENTITY}${keychain} $1"
}
# app_line TARGET [KEYCHAIN]: the app gets the explicit entitlements file and preserves nothing.
app_line() {
    local keychain=""
    [[ -z "${2:-}" ]] || keychain=" --keychain $2"
    printf '%s' "--force --options runtime --timestamp --entitlements ${ENTITLEMENTS} --sign ${IDENTITY}${keychain} $1"
}

echo "== sign-app: targets =="
rc=0
run_sign sign-app "${APP}" || rc=$?
check "exits 0" "${rc}"
signed="$(sign_targets | wc -l | tr -d ' ')"
check "signs exactly 12 targets (4 Sparkle parts, Sparkle, 2 dylibs, 2 frameworks, 2 helpers, app), got ${signed}" "$((signed == 12 ? 0 : 1))"
for t in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${UPDATE_TOOL}" "${UPDATER}" "${SPK}" "${DYLIB}" "${INNER_DYLIB}" "${OTHER}" "${INNER}" "${HELPER}" "${PANDOC}" "${APP}"; do
    expect "signs ${t#"${TMP_DIR}/Fake/"}" test -n "$(sign_index "${t}")"
done
expect "never signs the Versions/Current symlink" test -z "$(sign_index "${SPK}/Versions/Current")"
expect "does not sign a file in Helpers that is not code" test -z "$(sign_index "${NOTES}")"
expect "does not sign an empty file in Helpers" test -z "$(sign_index "${EMPTY_FILE}")"
expect "does not sign a two-byte file in Helpers" test -z "$(sign_index "${SHORT_FILE}")"

echo "== sign-app: order, deepest first, the app last =="
for nested in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${UPDATE_TOOL}" "${UPDATER}"; do
    expect "${nested##*/} is signed before Sparkle.framework" before "${nested}" "${SPK}"
done
expect "Sparkle.framework is signed before the loose dylib" before "${SPK}" "${DYLIB}"
expect "Sparkle.framework is signed before the dylib inside Inner.framework" before "${SPK}" "${INNER_DYLIB}"
expect "the loose dylib is signed before Other.framework" before "${DYLIB}" "${OTHER}"
expect "the loose dylib is signed before Inner.framework" before "${DYLIB}" "${INNER}"
expect "the dylib inside Inner.framework is signed before Inner.framework seals it" before "${INNER_DYLIB}" "${INNER}"
expect "the dylib inside Inner.framework is signed before Other.framework" before "${INNER_DYLIB}" "${OTHER}"
expect "Other.framework is signed before the ashift helper" before "${OTHER}" "${HELPER}"
expect "Inner.framework is signed before Pandoc" before "${INNER}" "${PANDOC}"
expect "the ashift helper is signed before the app" before "${HELPER}" "${APP}"
expect "Pandoc is signed before the app" before "${PANDOC}" "${APP}"
expect "the app is the last target signed" test "$(sign_targets | tail -n 1)" = "${APP}"

echo "== sign-app: flags =="
all_ok=0
while IFS= read -r line; do
    target="${line##* }"
    if [[ "${target}" == "${APP}" ]]; then
        expected="$(app_line "${target}")"
    else
        expected="$(nested_line "${target}")"
    fi
    if [[ "${line}" != "${expected}" ]]; then
        echo "  unexpected signing call: ${line}" >&2
        echo "          expected:        ${expected}" >&2
        all_ok=1
    fi
done < <(sign_lines)
check "every call carries exactly the Developer ID options: the hardened runtime, a secure timestamp, the identity, no --deep" "${all_ok}"
expect "no call passes --timestamp=none or an ad-hoc identity" lacks_file "${LOG}" "--timestamp=none"
app_signing="$(logged_line "${APP}")"
expect "only the app is signed with the entitlements file" test "$(grep -c -F -- '--entitlements ' <<< "$(sign_lines)")" = 1
expect "the app does not preserve nested metadata" test "${app_signing}" = "$(app_line "${APP}")"
for t in "${XPC_DOWNLOADER}" "${UPDATE_TOOL}" "${SPK}" "${INNER_DYLIB}" "${INNER}" "${HELPER}" "${PANDOC}"; do
    expect "${t##*/} is verified" grep -q -F -- "--verify --deep --strict --verbose=2 ${t}" "${LOG}"
done
expect "the app is verified" grep -q -F -- "--verify --deep --strict --verbose=2 ${APP}" "${LOG}"
check "verification starts only after the last signing call" "$(($(first_verify_line) > $(last_sign_line) ? 0 : 1))"

echo "== sign-app: the keychain option =="
rc=0
run_sign sign-app "${APP}" -- KEYCHAIN_PATH=/tmp/example.keychain-db || rc=$?
check "exits 0 with a keychain" "${rc}"
keychain_ok=0
while IFS= read -r line; do
    target="${line##* }"
    if [[ "${target}" == "${APP}" ]]; then
        expected="$(app_line "${target}" /tmp/example.keychain-db)"
    else
        expected="$(nested_line "${target}" /tmp/example.keychain-db)"
    fi
    [[ "${line}" == "${expected}" ]] || { echo "  unexpected signing call: ${line}" >&2; keychain_ok=1; }
done < <(sign_lines)
check "every call adds --keychain after the identity and nothing else changes" "${keychain_ok}"

echo "== sign-app: the layout of the current release (Sparkle only, one helper) =="
# The sequence of signing calls for this layout is what notarized releases were built with; it must not change.
APP_SPARKLE="${TMP_DIR}/SparkleOnly/AriadShift.app"
make_app "${APP_SPARKLE}"
rm "${APP_SPARKLE}/Contents/Helpers/pandoc"
SPK2="${APP_SPARKLE}/Contents/Frameworks/Sparkle.framework"
rc=0
run_sign sign-app "${APP_SPARKLE}" || rc=$?
check "exits 0" "${rc}"
expected_sequence="$(
    nested_line "${SPK2}/Versions/B/XPCServices/Downloader.xpc"
    echo
    nested_line "${SPK2}/Versions/B/XPCServices/Installer.xpc"
    echo
    nested_line "${SPK2}/Versions/B/Autoupdate"
    echo
    nested_line "${SPK2}/Versions/B/Updater.app"
    echo
    nested_line "${SPK2}"
    echo
    nested_line "${APP_SPARKLE}/Contents/Helpers/ashift"
    echo
    app_line "${APP_SPARKLE}"
)"
check "the signing calls are, in order: 2 XPC services, the updater tool, Updater.app, Sparkle.framework, the helper, the app" \
    "$([[ "$(sign_lines)" == "${expected_sequence}" ]] && echo 0 || echo 1)"

echo "== verify-app =="
rc=0
run_sign verify-app "${APP}" || rc=$?
check "exits 0" "${rc}"
expect "nothing is signed" test -z "$(sign_targets)"
for helper in "${HELPER}" "${PANDOC}" "${INNER_DYLIB}"; do
    expect "${helper##*/} is verified" grep -q -F -- "--verify --deep --strict --verbose=2 ${helper}" "${LOG}"
done
verified_order="$(grep -- '^--verify ' "${LOG}" | awk '{print $NF}')"
rc=0
run_sign sign-app "${APP}" || rc=$?
signed_order="$(sign_targets)"
check "verify-app checks the same targets, in the same order, that sign-app signs" \
    "$([[ -n "${signed_order}" && "${verified_order}" == "${signed_order}" ]] && echo 0 || echo 1)"

echo "== An unreadable directory in the bundle fails the run =="
# find cannot list a directory it cannot read. Before the walk was checked, sign-app and verify-app went on without the
# rest of the list (later frameworks, the helpers, the app) and still printed success.
WALK_TMP="${TMP_DIR}/walk-tmp"
mkdir -p "${WALK_TMP}"
# can_read PATH: true when this user can read the file or list the directory (false for a chmod 000 path, true for root).
can_read() {
    if [[ -d "$1" ]]; then
        ls "$1" > /dev/null 2>&1
    else
        head -c 1 "$1" > /dev/null 2>&1
    fi
}

# Frameworks/Inner.framework and Helpers/lib: find cannot list them. Helpers/h: a Mach-O that is listed but cannot be
# read, so it cannot be told from "not code". Versions/B and its XPCServices: the tests of the names inside them
# (-d, -f, a glob) would answer "absent" and drop the Sparkle parts without a word.
unreadable_case=0
for locked in Contents/Frameworks/Inner.framework Contents/Helpers/lib Contents/Helpers/h \
    Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices Contents/Frameworks/Sparkle.framework/Versions/B; do
    unreadable_case=$((unreadable_case + 1))
    APP_LOCKED="${TMP_DIR}/Locked${unreadable_case}/AriadShift.app"
    make_app "${APP_LOCKED}" with-extras
    mkdir -p "${APP_LOCKED}/Contents/Helpers/lib"
    write_macho "${APP_LOCKED}/Contents/Helpers/lib/extra"
    write_macho "${APP_LOCKED}/Contents/Helpers/h"
    chmod 000 "${APP_LOCKED}/${locked}"
    if can_read "${APP_LOCKED}/${locked}"; then
        echo "SKIP unreadable ${locked}: chmod 000 does not block reads here (running as root?)"
    else
        rc=0
        run_sign sign-app "${APP_LOCKED}" -- "TMPDIR=${WALK_TMP}" || rc=$?
        check "sign-app fails when ${locked} is unreadable" "$((rc != 0 ? 0 : 1))"
        expect "and nothing is signed" test ! -s "${LOG}"
        expect "and it does not report success" lacks_file "${OUT}" "Successfully signed"
        expect "and it says the walk failed" grep -q "cannot list the code to sign" "${OUT}"
        expect "and it names ${locked##*/}" grep -q -F "${locked}" "${OUT}"
        expect "and it leaves no temporary file" test -z "$(ls -A "${WALK_TMP}")"
        rc=0
        run_sign verify-app "${APP_LOCKED}" -- "TMPDIR=${WALK_TMP}" || rc=$?
        check "verify-app fails when ${locked} is unreadable" "$((rc != 0 ? 0 : 1))"
        expect "and nothing is verified" test ! -s "${LOG}"
        expect "and it does not report success" lacks_file "${OUT}" "Verified application"
        expect "and it says the walk failed" grep -q "cannot list the code to sign" "${OUT}"
        expect "and it names ${locked##*/}" grep -q -F "${locked}" "${OUT}"
        expect "and it leaves no temporary file" test -z "$(ls -A "${WALK_TMP}")"
    fi
    chmod 755 "${APP_LOCKED}/${locked}"
done
rc=0
run_sign sign-app "${APP}" -- "TMPDIR=${WALK_TMP}" || rc=$?
check "the same bundle with readable directories signs and exits 0" "${rc}"
expect "and leaves no temporary file" test -z "$(ls -A "${WALK_TMP}")"

echo "== A walk with no targets, or one that does not end at the app, fails closed =="
# The signing library with its walk replaced by a fake one, run in the same interpreter. verify-app takes its list from
# load_sign_targets and sign-app signs what it loads, so both entries are exercised. "real" is the control.
HARNESS="${TMP_DIR}/walk-harness.sh"
cat > "${HARNESS}" <<'HARNESS_SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
# walk-harness.sh LIB APP ENTITLEMENTS ENTRY WALK
source "$1"
app="$2"
entitlements="$3"
export APPLE_SIGNING_IDENTITY="Developer ID Application: Example"
case "$5" in
    real) ;;
    empty) list_sign_targets() { return 0; } ;;
    not-the-app) list_sign_targets() { printf '%s\0' "${app}/Contents/Helpers/ashift"; } ;;
    app-not-last) list_sign_targets() { printf '%s\0' "${app}" "${app}/Contents/Helpers/ashift"; } ;;
    cut-short) list_sign_targets() { printf '%s\0' "${app}/Contents/Helpers/ashift"; return 1; } ;;
esac
SIGNED_TARGETS=()
case "$4" in
    sign) sign_app_targets signed "${app}" "${entitlements}" ;;
    load) load_sign_targets "${app}" ;;
esac
echo "HARNESS-SUCCESS"
HARNESS_SCRIPT
run_harness() {
    : > "${LOG}"
    local rc=0
    env -i PATH="${STUB_DIR}:${PATH}" STUB_LOG="${LOG}" \
        "${BASH}" "${HARNESS}" "${SCRIPT_DIR}/macos-signing-lib.sh" "${APP}" "${ENTITLEMENTS}" "$1" "$2" > "${OUT}" 2>&1 || rc=$?
    return "${rc}"
}
for entry in sign load; do
    rc=0
    run_harness "${entry}" real || rc=$?
    check "${entry}: the real walk succeeds through the harness" "${rc}"
    expect "${entry}: and the harness reports success" grep -q "HARNESS-SUCCESS" "${OUT}"
    for walk in empty not-the-app app-not-last cut-short; do
        rc=0
        run_harness "${entry}" "${walk}" || rc=$?
        check "${entry}: a walk that is ${walk} fails" "$((rc != 0 ? 0 : 1))"
        expect "${entry}: ${walk} does not reach success" lacks_file "${OUT}" "HARNESS-SUCCESS"
        expect "${entry}: ${walk} signs nothing" test ! -s "${LOG}"
    done
    run_harness "${entry}" empty || true
    expect "${entry}: an empty walk says no code was found" grep -q "no code to sign was found" "${OUT}"
    run_harness "${entry}" not-the-app || true
    expect "${entry}: a list that stops short of the app says so" grep -q "does not end at the app" "${OUT}"
    run_harness "${entry}" app-not-last || true
    expect "${entry}: a list with the app in the middle says so" grep -q "does not end at the app" "${OUT}"
    run_harness "${entry}" cut-short || true
    expect "${entry}: a walk that fails says so" grep -q "cannot list the code to sign" "${OUT}"
done

echo "== Gates fail closed =="
rc=0
run_sign sign-app "${APP}" -- "STUB_FAIL_SIGN==${INNER}" || rc=$?
check "a failure signing a framework fails the run" "$((rc != 0 ? 0 : 1))"
expect "and the app is never signed" test -z "$(sign_index "${APP}")"
expect "and nothing is verified" test -z "$(first_verify_line)"
rc=0
run_sign sign-app "${APP}" -- "STUB_FAIL_VERIFY==${INNER_DYLIB}" || rc=$?
check "a failed verification of a nested dylib fails the run" "$((rc != 0 ? 0 : 1))"
rc=0
run_sign sign-app "${APP}" -- "STUB_GTA==${PANDOC}" || rc=$?
check "get-task-allow on Pandoc fails the run" "$((rc != 0 ? 0 : 1))"
rc=0
run_sign sign-app "${APP}" -- "STUB_GTA==${APP}" || rc=$?
check "get-task-allow on the app fails the run" "$((rc != 0 ? 0 : 1))"
expect "it reports get-task-allow" grep -q "get-task-allow" "${OUT}"
rc=0
run_sign verify-app "${APP}" -- "STUB_GTA==${XPC_INSTALLER}" || rc=$?
check "verify-app fails on get-task-allow in a nested bundle" "$((rc != 0 ? 0 : 1))"
: > "${LOG}"
rc=0
env -i PATH="${STUB_DIR}:${PATH}" STUB_LOG="${LOG}" "${BASH}" "${SIGN}" sign-app "${APP}" > "${OUT}" 2>&1 || rc=$?
check "sign-app without an identity fails" "$((rc != 0 ? 0 : 1))"
expect "and runs no codesign" test ! -s "${LOG}"

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} of ${checks} check(s) failed" >&2
    exit 1
fi
echo "all ${checks} checks passed"
