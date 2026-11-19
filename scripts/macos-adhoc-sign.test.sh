#!/usr/bin/env bash
set -euo pipefail

# Test macos-adhoc-sign.sh on any OS with a stub `codesign` that records its arguments. This cannot prove that real
# codesign accepts the result (only a macOS runner can). It proves the ORDER and the FLAGS the script passes, and that
# every gate fails closed: inside-out order, `--force --sign -` with the hardened runtime and no secure timestamp,
# never `--deep` when signing, the explicit entitlements only on the app, and a verify gate that rejects a failed
# verification, a leftover non-ad-hoc identity, a lost hardened runtime and a get-task-allow entitlement.
#
# The two signing scripts share one bundle walk (scripts/macos-signing-lib.sh). This test also runs the Developer ID
# script on the same bundle and requires the same target order, and checks that the walk fails closed: an unreadable
# directory, an empty list and a list that does not end at the app all stop the run before anything is signed.
# macos-sign-notarize.test.sh checks the same order and the Developer ID flags; keep the two in step.
# The scripts run under the interpreter this test runs under ("${BASH}"), so a run under /bin/bash 3.2 exercises 3.2.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SIGN="${SCRIPT_DIR}/macos-adhoc-sign.sh"
SIGN_DEVELOPER_ID="${SCRIPT_DIR}/macos-sign-notarize.sh"
ENTITLEMENTS="${SCRIPT_DIR}/macos-app.entitlements"
failures=0
checks=0

TMP_DIR="$(mktemp -d)"
cleanup() {
    if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
        # A directory a check made unreadable would stop rm; give the owner access back first.
        chmod -R u+rwx "${TMP_DIR}" 2> /dev/null || true
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

# Helpers are recognised as code by their first four bytes: a 64-bit little-endian Mach-O and a fat binary.
write_macho() { printf '\317\372\355\376' > "$1"; }
write_fat_macho() { printf '\312\376\272\276' > "$1"; }

# make_app DIR [with-sparkle]: a fake bundle shaped like the real one: Sparkle with two XPC services, Autoupdate,
# Updater.app and the Current symlink, one more framework, one loose dylib, a framework that holds a dylib (signing
# order decides whether its seal survives), and Helpers with the ashift helper and Pandoc (both Mach-O) beside a
# licence note that is not code and must stay unsigned.
make_app() {
    local app="$1"
    mkdir -p "${app}/Contents/Helpers" "${app}/Contents/MacOS"
    write_fat_macho "${app}/Contents/Helpers/ashift"
    write_macho "${app}/Contents/Helpers/pandoc"
    echo "not code" > "${app}/Contents/Helpers/NOTES.txt"
    : > "${app}/Contents/MacOS/AriadShift"
    if [[ "${2:-}" == "with-sparkle" ]]; then
        local sparkle="${app}/Contents/Frameworks/Sparkle.framework"
        mkdir -p "${sparkle}/Versions/B/XPCServices/Downloader.xpc" \
            "${sparkle}/Versions/B/XPCServices/Installer.xpc" \
            "${sparkle}/Versions/B/Updater.app" \
            "${app}/Contents/Frameworks/Other.framework" \
            "${app}/Contents/Frameworks/Inner.framework"
        : > "${sparkle}/Versions/B/Autoupdate"
        ln -s B "${sparkle}/Versions/Current"
        : > "${app}/Contents/Frameworks/libextra.dylib"
        : > "${app}/Contents/Frameworks/Inner.framework/libinner.dylib"
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
INNER="${FW}/Inner.framework"
INNER_DYLIB="${INNER}/libinner.dylib"
HELPER="${APP}/Contents/Helpers/ashift"
PANDOC="${APP}/Contents/Helpers/pandoc"
NOTES="${APP}/Contents/Helpers/NOTES.txt"

LOG="${TMP_DIR}/codesign.log"
OUT="${TMP_DIR}/out.txt"

# run_sign ARGS... [-- ENV=VALUE...]: runs the script with the stub first on PATH and no inherited environment. It
# runs SIGN_SCRIPT, the ad-hoc script unless a caller sets it. The codesign log lands in LOG and the script output in OUT.
SIGN_SCRIPT="${SIGN}"
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
        "${BASH}" "${SIGN_SCRIPT}" ${args[@]+"${args[@]}"} > "${OUT}" 2>&1 || rc=$?
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
check "signs exactly 12 targets (4 Sparkle parts, Sparkle, 2 dylibs, 2 frameworks, 2 helpers, app), got ${signed}" "$((signed == 12 ? 0 : 1))"
for t in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}" "${SPK}" "${DYLIB}" "${INNER_DYLIB}" "${OTHER}" "${INNER}" "${HELPER}" "${PANDOC}" "${APP}"; do
    expect "signs ${t#"${TMP_DIR}/Fake/"}" test -n "$(sign_index "${t}")"
done
expect "never signs the Versions/Current symlink" test -z "$(sign_index "${SPK}/Versions/Current")"
expect "never signs a Helpers file that is not code" test -z "$(sign_index "${NOTES}")"

echo "== Order: deepest first, the app last =="
for nested in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}"; do
    expect "${nested##*/} is signed before Sparkle.framework" before "${nested}" "${SPK}"
done
expect "Sparkle.framework is signed before the app" before "${SPK}" "${APP}"
expect "Sparkle.framework is signed before the loose dylib" before "${SPK}" "${DYLIB}"
expect "Sparkle.framework is signed before the dylib inside Inner.framework" before "${SPK}" "${INNER_DYLIB}"
expect "the loose dylib is signed before Other.framework" before "${DYLIB}" "${OTHER}"
expect "the loose dylib is signed before Inner.framework" before "${DYLIB}" "${INNER}"
expect "the dylib inside Inner.framework is signed before Inner.framework seals it" before "${INNER_DYLIB}" "${INNER}"
expect "the dylib inside Inner.framework is signed before Other.framework" before "${INNER_DYLIB}" "${OTHER}"
expect "Other.framework is signed before the app" before "${OTHER}" "${APP}"
expect "the helper is signed before the app" before "${HELPER}" "${APP}"
expect "Pandoc is signed before the app" before "${PANDOC}" "${APP}"
expect "Pandoc is signed after the frameworks" before "${OTHER}" "${PANDOC}"
expect "the ashift helper is signed after Inner.framework" before "${INNER}" "${HELPER}"
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
for nested in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}" "${SPK}" "${DYLIB}" "${INNER_DYLIB}" "${OTHER}" "${INNER}" "${HELPER}" "${PANDOC}"; do
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
for t in "${XPC_DOWNLOADER}" "${XPC_INSTALLER}" "${AUTOUPDATE}" "${UPDATER}" "${SPK}" "${DYLIB}" "${INNER_DYLIB}" "${OTHER}" "${INNER}" "${HELPER}" "${PANDOC}" "${APP}"; do
    grep -q -F -- "--verify --deep --strict --verbose=2 ${t}" "${LOG}" || { echo "  not verified: ${t}" >&2; verify_ok=1; }
done
check "every signed target, the app included, is verified with --verify --deep --strict" "${verify_ok}"
expect "the script says it verified the app" grep -q "Ad-hoc signed and verified" "${OUT}"

echo "== One walk for both modes =="
# The Developer ID script signs the same bundle in the same order, one dylib-before-framework rule for both.
adhoc_order="$(sign_targets)"
SIGN_SCRIPT="${SIGN_DEVELOPER_ID}"
rc=0
run_sign sign-app "${APP}" -- APPLE_SIGNING_IDENTITY="Developer ID Application: Example" || rc=$?
SIGN_SCRIPT="${SIGN}"
check "the Developer ID script signs the same bundle and exits 0" "${rc}"
developer_id_order="$(sign_targets)"
check "both modes sign the same targets in the same order" \
    "$([[ -n "${adhoc_order}" && "${adhoc_order}" == "${developer_id_order}" ]] && echo 0 || echo 1)"
expect "in Developer ID mode the dylib inside Inner.framework is signed before Inner.framework" before "${INNER_DYLIB}" "${INNER}"
expect "in Developer ID mode the loose dylib is signed before Other.framework" before "${DYLIB}" "${OTHER}"
for script in macos-adhoc-sign.sh macos-sign-notarize.sh; do
    expect "${script} takes the walk from the shared library" grep -q -F "source \"\${SCRIPT_DIR}/macos-signing-lib.sh\"" "${SCRIPT_DIR}/${script}"
done

echo "== A failing walk fails closed =="
# find cannot list a directory it cannot read. Before the walk was checked, the run went on without the rest of the list
# (later frameworks, the helpers, the app) and still printed success.
WALK_TMP="${TMP_DIR}/walk-tmp"
mkdir -p "${WALK_TMP}"
unreadable_case=0
for locked in Contents/Frameworks/Inner.framework Contents/Helpers/lib; do
    unreadable_case=$((unreadable_case + 1))
    APP_LOCKED="${TMP_DIR}/Locked${unreadable_case}/AriadShift.app"
    make_app "${APP_LOCKED}" with-sparkle
    mkdir -p "${APP_LOCKED}/Contents/Helpers/lib"
    write_macho "${APP_LOCKED}/Contents/Helpers/lib/extra"
    chmod 000 "${APP_LOCKED}/${locked}"
    if ls "${APP_LOCKED}/${locked}" > /dev/null 2>&1; then
        echo "SKIP unreadable ${locked}: chmod 000 does not block reads here (running as root?)"
    else
        rc=0
        run_sign app "${APP_LOCKED}" -- "TMPDIR=${WALK_TMP}" || rc=$?
        check "an unreadable ${locked} fails the run" "$((rc != 0 ? 0 : 1))"
        expect "and nothing is signed" test ! -s "${LOG}"
        expect "and it does not report success" lacks_file "${OUT}" "Ad-hoc signed and verified"
        expect "and it says the walk failed" grep -q "cannot list the code to sign" "${OUT}"
        expect "and it leaves no temporary file" test -z "$(ls -A "${WALK_TMP}")"
    fi
    chmod 755 "${APP_LOCKED}/${locked}"
done
rc=0
run_sign app "${APP}" -- "TMPDIR=${WALK_TMP}" || rc=$?
check "the same bundle with readable directories signs and exits 0" "${rc}"
expect "and leaves no temporary file" test -z "$(ls -A "${WALK_TMP}")"

echo "== A walk with no targets, or one that does not end at the app, fails closed =="
# The library with its walk replaced by a fake one, run in the same interpreter. "real" is the control: the harness works.
HARNESS="${TMP_DIR}/walk-harness.sh"
cat > "${HARNESS}" <<'HARNESS_SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
# walk-harness.sh LIB APP ENTITLEMENTS ENTRY WALK
source "$1"
app="$2"
entitlements="$3"
case "$5" in
    real) ;;
    empty) list_sign_targets() { return 0; } ;;
    not-the-app) list_sign_targets() { printf '%s\0' "${app}/Contents/Helpers/ashift"; } ;;
    app-not-last) list_sign_targets() { printf '%s\0' "${app}" "${app}/Contents/Helpers/ashift"; } ;;
    cut-short) list_sign_targets() { printf '%s\0' "${app}/Contents/Helpers/ashift"; return 1; } ;;
esac
SIGNED_TARGETS=()
case "$4" in
    sign) sign_app_targets adhoc "${app}" "${entitlements}" ;;
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
run_sign app "${APP}" -- "STUB_FLAGS_FOR==${PANDOC}" STUB_FLAGS=adhoc || rc=$?
check "a Pandoc that lost the hardened runtime fails the run" "$((rc != 0 ? 0 : 1))"
rc=0
run_sign app "${APP}" -- "STUB_GTA==${PANDOC}" || rc=$?
check "get-task-allow on Pandoc fails the run" "$((rc != 0 ? 0 : 1))"
rc=0
run_sign app "${APP}" -- "STUB_FAIL_SIGN==${PANDOC}" || rc=$?
check "a failure signing Pandoc fails the run before the app is signed" "$((rc != 0 ? 0 : 1))"
expect "and the app is never signed" test -z "$(sign_index "${APP}")"

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
expect "it signs only the two helpers and the app" test "$(sign_targets | wc -l | tr -d ' ')" = 3
expect "both helpers are signed before the app" test "$(sign_targets | tail -n 1)" = "${APP2}"
expect "and Pandoc is one of them" test -n "$(sign_index "${APP2}/Contents/Helpers/pandoc")"
APP3="${TMP_DIR}/NoPandoc/AriadShift.app"
make_app "${APP3}"
rm "${APP3}/Contents/Helpers/pandoc"
rc=0
run_sign app "${APP3}" || rc=$?
check "a bundle without Pandoc still signs and exits 0" "${rc}"
expect "it signs only the ashift helper and the app" test "$(sign_targets | wc -l | tr -d ' ')" = 2

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
