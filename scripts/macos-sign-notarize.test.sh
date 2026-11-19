#!/usr/bin/env bash
set -euo pipefail

# Test the Contents/Helpers handling of macos-sign-notarize.sh (the Developer ID path, unused today but kept working)
# on any OS with a stub `codesign` that records its arguments. `sign-app` needs no notary credential. This proves what
# gets signed and how: every Mach-O file in Helpers (the ashift helper and Pandoc) is signed with the hardened runtime
# and a secure timestamp, before the app, never with --deep, and a file in Helpers that is not code is left alone;
# `verify-app` then checks the same set. Real codesign, Apple identities and notarization are only reachable on a macOS
# runner with the six Apple secrets.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SIGN="${SCRIPT_DIR}/macos-sign-notarize.sh"
failures=0
checks=0

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

STUB_DIR="${TMP_DIR}/stub"
mkdir -p "${STUB_DIR}"
cat > "${STUB_DIR}/codesign" <<'STUB'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "${STUB_LOG:?}"
case "${1:-}" in
    --verify) ;;
    -dvv) echo "CodeDirectory v=20500 flags=0x10000(runtime)" >&2 ;;
    -d) echo '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict></dict></plist>' ;;
    *) ;;
esac
exit 0
STUB
chmod +x "${STUB_DIR}/codesign"

APP="${TMP_DIR}/Fake/AriadShift.app"
mkdir -p "${APP}/Contents/Helpers" "${APP}/Contents/MacOS"
printf '\312\376\272\276' > "${APP}/Contents/Helpers/ashift"
printf '\312\376\272\276' > "${APP}/Contents/Helpers/pandoc"
echo "not code" > "${APP}/Contents/Helpers/NOTES.txt"
: > "${APP}/Contents/MacOS/AriadShift"
HELPER="${APP}/Contents/Helpers/ashift"
PANDOC="${APP}/Contents/Helpers/pandoc"
NOTES="${APP}/Contents/Helpers/NOTES.txt"

LOG="${TMP_DIR}/codesign.log"
OUT="${TMP_DIR}/out.txt"

run_sign() {
    : > "${LOG}"
    local rc=0
    env -i PATH="${STUB_DIR}:${PATH}" STUB_LOG="${LOG}" APPLE_SIGNING_IDENTITY="Developer ID Application: Example" \
        bash "${SIGN}" "$@" > "${OUT}" 2>&1 || rc=$?
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

sign_targets() { grep -- '--force ' "${LOG}" | awk '{print $NF}' || true; }
sign_index() { sign_targets | grep -n -x -F -- "$1" | head -n 1 | cut -d: -f1 || true; }
before() {
    local a b
    a="$(sign_index "$1")"
    b="$(sign_index "$2")"
    [[ -n "${a}" && -n "${b}" && "${a}" -lt "${b}" ]]
}
logged_line() { grep -- '--force ' "${LOG}" | awk -v t="$1" '$NF == t' || true; }

echo "== sign-app =="
rc=0
run_sign sign-app "${APP}" || rc=$?
check "exits 0" "${rc}"
expect "signs the ashift helper" test -n "$(sign_index "${HELPER}")"
expect "signs Pandoc" test -n "$(sign_index "${PANDOC}")"
expect "does not sign a file in Helpers that is not code" test -z "$(sign_index "${NOTES}")"
expect "signs the ashift helper before the app" before "${HELPER}" "${APP}"
expect "signs Pandoc before the app" before "${PANDOC}" "${APP}"
expect "the app is the last target signed" test "$(sign_targets | tail -n 1)" = "${APP}"
for helper in "${HELPER}" "${PANDOC}"; do
    line="$(logged_line "${helper}")"
    ok=0
    [[ "${line}" == *"--options runtime"* && "${line}" == *"--timestamp "* && "${line}" != *"--timestamp=none"* \
        && "${line}" == *"--sign Developer ID Application: Example"* && "${line}" != *"--deep"* \
        && "${line}" != *"--entitlements "* ]] || ok=1
    check "${helper##*/} is signed with the hardened runtime, a secure timestamp, the identity, no --deep, no entitlements file" "${ok}"
done
for helper in "${HELPER}" "${PANDOC}"; do
    expect "${helper##*/} is verified" grep -q -F -- "--verify --deep --strict --verbose=2 ${helper}" "${LOG}"
done

echo "== verify-app =="
rc=0
run_sign verify-app "${APP}" || rc=$?
check "exits 0" "${rc}"
for helper in "${HELPER}" "${PANDOC}"; do
    expect "${helper##*/} is verified" grep -q -F -- "--verify --deep --strict --verbose=2 ${helper}" "${LOG}"
done
expect "nothing is signed" test -z "$(sign_targets)"

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} of ${checks} check(s) failed" >&2
    exit 1
fi
echo "all ${checks} checks passed"
