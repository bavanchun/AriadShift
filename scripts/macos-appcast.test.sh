#!/usr/bin/env bash
set -euo pipefail

# Test that macos-appcast.sh keeps the Sparkle EdDSA private key away from every child process it starts.
#
# The script copies SPARKLE_ED_PRIVATE_KEY into a shell variable and unsets the exported variable at once, so curl, tar,
# ditto and shasum (and generate_appcast itself) never inherit it; the key reaches generate_appcast only on standard input.
# Here every child is a stub that records whether the key is visible in its environment, and generate_appcast also records
# its argument list and standard input. Nothing is downloaded and no Sparkle tool is needed, so this runs on Linux too.
# A blank key (empty or whitespace only) must fail before any child runs.

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
APPCAST="${SCRIPT_DIR}/macos-appcast.sh"
CANARY="canary-private-key-that-must-reach-only-stdin"
PINNED_SHA="$(sed -n 's/^SPARKLE_SHA256="\([0-9a-f]\{64\}\)"$/\1/p' "${APPCAST}")"
if [[ -z "${PINNED_SHA}" ]]; then
    echo "FAIL: could not read SPARKLE_SHA256 from ${APPCAST}" >&2
    exit 1
fi
SPARKLE_VERSION="$(sed -n 's/^SPARKLE_VERSION="\([0-9.]*\)"$/\1/p' "${APPCAST}")"
if [[ -z "${SPARKLE_VERSION}" ]]; then
    echo "FAIL: could not read SPARKLE_VERSION from ${APPCAST}" >&2
    exit 1
fi

ROOT="$(mktemp -d)"
trap 'rm -rf "${ROOT}"' EXIT
BIN="${ROOT}/bin"
mkdir -p "${BIN}"
failures=0
cases=0

# Every stub starts with the same probe: is the key variable, or the key value, visible in this process's environment?
# The canary is written into the stub text, not passed in the environment, so the probe cannot trip over itself.
probe() {
    cat <<EOF
#!/bin/sh
name="$1"
if env | grep -q '^SPARKLE_ED_PRIVATE_KEY=' || env | grep -qF '${CANARY}'; then
    echo "\${name} LEAK" >> "\${STUB_LOG}"
else
    echo "\${name} clean" >> "\${STUB_LOG}"
fi
EOF
}

write_stub() {
    local name="$1" body="$2"
    {
        probe "${name}"
        printf '%s\n' "${body}"
    } > "${BIN}/${name}"
    chmod +x "${BIN}/${name}"
}

# generate_appcast: records the argument list and standard input, then writes the feed given by -o.
GENERATE_TEMPLATE="${ROOT}/generate_appcast.template"
{
    probe generate_appcast
    cat <<'EOF'
printf '%s\n' "$@" > "${STUB_LOG}.argv"
cat > "${STUB_LOG}.stdin"
out=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        *) shift ;;
    esac
done
[ -n "${out}" ] && printf '<rss/>\n' > "${out}"
exit 0
EOF
} > "${GENERATE_TEMPLATE}"
chmod +x "${GENERATE_TEMPLATE}"

write_stub shasum "printf '%s  %s\\n' '${PINNED_SHA}' \"\$3\""
# shellcheck disable=SC2016 # the stub bodies are literal scripts for /bin/sh
write_stub ditto 'cp "$1" "$2"'
# curl: write a placeholder tarball to the -o target.
# shellcheck disable=SC2016 # the stub bodies are literal scripts for /bin/sh
write_stub curl 'out=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        *) shift ;;
    esac
done
printf "placeholder tarball\n" > "${out}"'
# tar -xf TARBALL -C DIR: "extract" the stub generate_appcast into DIR/bin.
write_stub tar "dest=''
while [ \"\$#\" -gt 0 ]; do
    case \"\$1\" in
        -C) dest=\"\$2\"; shift 2 ;;
        *) shift ;;
    esac
done
mkdir -p \"\${dest}/bin\"
cp '${GENERATE_TEMPLATE}' \"\${dest}/bin/generate_appcast\""

# prepare_run -> sets WORK, CACHE, STUB_LOG, DMG, STAGING for one fresh run
prepare_run() {
    WORK="$(mktemp -d "${ROOT}/run.XXXXXX")"
    CACHE="${WORK}/cache"
    STUB_LOG="${WORK}/stub.log"
    DMG="${WORK}/AriadShift-v0.0.0-universal.dmg"
    STAGING="${WORK}/staging"
    mkdir -p "${WORK}/tmp" "${STAGING}"
    : > "${STUB_LOG}"
    printf 'dmg\n' > "${DMG}"
    cp "${DMG}" "${STAGING}/"
}

# seed_cache: a cached tarball (whose stub hash matches the pin) and an executable generate_appcast, so no download runs.
seed_cache() {
    mkdir -p "${CACHE}/bin"
    printf 'placeholder tarball\n' > "${CACHE}/Sparkle-${SPARKLE_VERSION}.tar.xz"
    cp "${GENERATE_TEMPLATE}" "${CACHE}/bin/generate_appcast"
}

# run_script KEY_MODE(env|stdin|none) KEY_VALUE TARGET -> sets RC, OUT, ERR
run_script() {
    local mode="$1" value="$2" target="$3"
    RC=0
    OUT="${WORK}/stdout"
    ERR="${WORK}/stderr"
    local -a base=(env -i "PATH=${BIN}:${PATH}" "STUB_LOG=${STUB_LOG}" "SPARKLE_CACHE_DIR=${CACHE}" "TMPDIR=${WORK}/tmp"
        "DOWNLOAD_URL_PREFIX=https://example.invalid/")
    case "${mode}" in
        env)
            "${base[@]}" "SPARKLE_ED_PRIVATE_KEY=${value}" bash "${APPCAST}" "${target}" "${WORK}/appcast.xml" \
                < /dev/null > "${OUT}" 2> "${ERR}" || RC=$?
            ;;
        stdin)
            printf '%s\n' "${value}" | "${base[@]}" bash "${APPCAST}" "${target}" "${WORK}/appcast.xml" \
                > "${OUT}" 2> "${ERR}" || RC=$?
            ;;
        none)
            "${base[@]}" bash "${APPCAST}" "${target}" "${WORK}/appcast.xml" < /dev/null > "${OUT}" 2> "${ERR}" || RC=$?
            ;;
    esac
}

report() {
    local label="$1" ok="$2"
    cases=$((cases + 1))
    if [[ "${ok}" -eq 1 ]]; then
        echo "PASS ${label}"
    else
        echo "FAIL ${label}: rc=${RC} stub log: $(tr '\n' ';' < "${STUB_LOG}")" >&2
        sed 's/^/  stderr: /' "${ERR}" >&2
        failures=$((failures + 1))
    fi
}

# check_success LABEL EXPECTED_CHILDREN...: the run succeeded, every expected child ran and saw no key, generate_appcast got
# the key on stdin only, and the key reached neither the argument list nor the script's output.
check_success() {
    local label="$1"
    shift
    local ok=1 child
    [[ "${RC}" -eq 0 ]] || ok=0
    ! grep -q ' LEAK$' "${STUB_LOG}" || ok=0
    for child in "$@"; do
        grep -qxF "${child} clean" "${STUB_LOG}" || ok=0
    done
    [[ -f "${STUB_LOG}.stdin" && "$(cat "${STUB_LOG}.stdin")" == "${CANARY}" ]] || ok=0
    ! grep -qF "${CANARY}" "${STUB_LOG}.argv" || ok=0
    ! grep -qF "${CANARY}" "${OUT}" "${ERR}" || ok=0
    [[ -s "${WORK}/appcast.xml" ]] || ok=0
    report "${label}" "${ok}"
}

echo "== Control: the probe really detects a key in a child environment =="
prepare_run
env -i "PATH=${BIN}:${PATH}" "STUB_LOG=${STUB_LOG}" "SPARKLE_ED_PRIVATE_KEY=${CANARY}" "${BIN}/shasum" -a 256 x > /dev/null
RC=0 OUT=/dev/null ERR=/dev/null
ok=0
grep -qxF 'shasum LEAK' "${STUB_LOG}" && ok=1
report "control: an exported key is reported as LEAK" "${ok}"
prepare_run
env -i "PATH=${BIN}:${PATH}" "STUB_LOG=${STUB_LOG}" "${BIN}/shasum" -a 256 x > /dev/null
RC=0 OUT=/dev/null ERR=/dev/null
ok=0
grep -qxF 'shasum clean' "${STUB_LOG}" && ok=1
report "control: a clean environment is reported as clean" "${ok}"

echo "== Cached tools, DMG input: ditto, shasum and generate_appcast never see the key =="
prepare_run
seed_cache
run_script env "${CANARY}" "${DMG}"
check_success "cached tools, key in the environment, DMG input" ditto shasum generate_appcast

echo "== Download path: curl, shasum, tar and generate_appcast never see the key =="
prepare_run
run_script env "${CANARY}" "${DMG}"
check_success "download and extract, key in the environment, DMG input" curl shasum tar ditto generate_appcast

echo "== Directory input =="
prepare_run
seed_cache
run_script env "${CANARY}" "${STAGING}"
check_success "cached tools, key in the environment, directory input" shasum generate_appcast

echo "== Key on standard input =="
prepare_run
seed_cache
run_script stdin "${CANARY}" "${DMG}"
check_success "cached tools, key on stdin, DMG input" ditto shasum generate_appcast
prepare_run
run_script stdin "${CANARY}" "${DMG}"
check_success "download and extract, key on stdin, DMG input" curl shasum tar ditto generate_appcast

echo "== A blank key fails before any child process runs =="
BLANKS=(
    "empty|"
    "spaces|   "
    "tab|"$'\t'
    "newline|"$'\n'
    "mixed whitespace|"$' \t\n \t\n '
)
for entry in "${BLANKS[@]}"; do
    label="${entry%%|*}"
    blank="${entry#*|}"
    prepare_run
    seed_cache
    run_script env "${blank}" "${DMG}"
    ok=1
    [[ "${RC}" -ne 0 ]] || ok=0
    [[ ! -s "${STUB_LOG}" ]] || ok=0
    [[ ! -e "${WORK}/appcast.xml" ]] || ok=0
    grep -qF "SPARKLE_ED_PRIVATE_KEY is not set" "${ERR}" || ok=0
    report "blank key (${label}) in the environment" "${ok}"
done
prepare_run
seed_cache
run_script none "" "${DMG}"
ok=1
[[ "${RC}" -ne 0 && ! -s "${STUB_LOG}" ]] || ok=0
report "no key anywhere" "${ok}"

if [[ "${failures}" -ne 0 ]]; then
    echo "${failures} of ${cases} case(s) failed" >&2
    exit 1
fi
echo "all ${cases} cases passed"
