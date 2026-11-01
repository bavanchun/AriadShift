#!/usr/bin/env bash
# Offline tests for the shared pack builder (scripts/pack/build-pack.sh), the pdfium wrapper
# (engines/pdfium/pack/make.sh) and the signing guard (scripts/pack/sign-pack.sh).
#
# The upstream archive, the engine binary and `ashift` are synthetic: the tests exercise the
# builder's own behaviour (hash pin, layout, notices, codesign calls, manifest flags) without a
# network or a Rust build. Set ASHIFT_BIN to an ashift that supports
# `__pack manifest --engine-protocol` to add one run against the real manifest writer.
set -euo pipefail

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH='' cd -- "$here/../.." && pwd)
builder=$here/build-pack.sh

passed=0
failed=0
pass() {
    passed=$((passed + 1))
    printf 'PASS | %s\n' "$1"
}
fail() {
    failed=$((failed + 1))
    printf 'FAIL | %s\n' "$1" >&2
}
check() {
    local desc=$1
    shift
    if "$@" >/dev/null 2>&1; then pass "$desc"; else fail "$desc"; fi
}

tmp=$(mktemp -d "${TMPDIR:-/tmp}/pack-tooling.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d ' ' -f 1; else shasum -a 256 "$1" | cut -d ' ' -f 1; fi
}

# --- fixtures ---------------------------------------------------------------------------------

mkdir -p "$tmp/up/lib" "$tmp/up/licenses" "$tmp/bin"
printf 'fake libpdfium\n' >"$tmp/up/lib/libpdfium.so"
printf 'fake dylib\n' >"$tmp/up/lib/libpdfium.dylib"
printf 'MIT wrapper licence\n' >"$tmp/up/LICENSE"
printf 'MAJOR=158\nBUILD=8125\n' >"$tmp/up/VERSION"
for notice in abseil.txt agg23.txt freetype.txt harfbuzz.txt icu.txt lcms.txt libjpeg_turbo.md \
    libopenjpeg.txt libpng.txt pdfium.txt zlib.txt fast_float.txt; do
    printf 'licence text for %s\n' "$notice" >"$tmp/up/licenses/$notice"
done
tar -czf "$tmp/upstream.tgz" -C "$tmp/up" .
good_sha=$(sha256_of "$tmp/upstream.tgz")
wrong_sha=$(printf 'a%.0s' $(seq 1 64))

printf '#!/bin/sh\necho engine\n' >"$tmp/bin/ariad-pdfium-engine"
chmod 755 "$tmp/bin/ariad-pdfium-engine"
mkdir -p "$tmp/notices"
printf 'engine notice\n' >"$tmp/notices/PACK.txt"

# A stand-in for `ashift __pack manifest` that follows its documented behaviour: it keeps the
# base's pack fields and other targets, writes engine_protocol when asked, and records its args.
cat >"$tmp/fake-ashift" <<'FAKE'
#!/bin/sh
log=${FAKE_ASHIFT_LOG:-/dev/null}
if [ "$1 $2" != "__pack manifest" ]; then exit 2; fi
shift 2
if [ "${1:-}" = --help ]; then
    echo "Usage: ashift __pack manifest --name --version --target --base --entrypoint --url-prefix"
    [ -n "${FAKE_ASHIFT_NO_PROTOCOL:-}" ] || echo "      --engine-protocol <VALUE>"
    exit 0
fi
printf '%s\n' "$*" >>"$log"
base=''; out=''; triple=''; proto=''; component=''; entry=''
while [ "$#" -gt 0 ]; do
    case $1 in
    --name|--version|--url-prefix|--executable) shift 2 ;;
    --target) triple=$2; shift 2 ;;
    --base) base=$2; shift 2 ;;
    -o) out=$2; shift 2 ;;
    --entrypoint) entry="$entry \"$2\","; shift 2 ;;
    --engine-protocol) proto=$2; shift 2 ;;
    *) component=$1; shift ;;
    esac
done
file=${component#*=}
cname=${component%%=*}
sum=$(sha256sum "$file" 2>/dev/null | cut -d ' ' -f 1 || true)
[ -n "$sum" ] || sum=$(shasum -a 256 "$file" | cut -d ' ' -f 1)
execs=$(zstd -q -d -c "$file" 2>/dev/null | tar -tvf - 2>/dev/null |
    awk '$1 ~ /^-.{2}x/ { p = $NF; sub(/^\.\//, "", p); if (p !~ /^notices\//) printf " \"%s\",", p }')
{
    sed -n '/^\[\[targets\]\]/q;/^targets = \[\]/d;/^engine_protocol/d;p' "$base"
    [ -z "$proto" ] || echo "engine_protocol = \"$proto\""
    sed -n '/^\[\[targets\]\]/,$p' "$base"
    echo '[[targets]]'
    echo "triple = \"$triple\""
    echo "entrypoint = [$entry ]"
    echo "executables = [$execs ]"
    echo '[[targets.components]]'
    echo "name = \"$cname\""
    echo "url = \"$(basename "$file")\""
    echo "sha256 = \"$sum\""
} >"$out"
FAKE
chmod 755 "$tmp/fake-ashift"
fake=$tmp/fake-ashift

common=(--name pdfium --version 0.1.0 --ashift "$fake" --description 'test pack'
    --license 'Apache-2.0 AND MIT' --requires-ashift '>=0.2.0, <2.0.0'
    --engine-protocol ariad-engine/2 --entrypoint bin/ariad-pdfium-engine)
layout=(--engine "$tmp/bin/ariad-pdfium-engine" --engine-dest bin/ariad-pdfium-engine
    --archive "$tmp/upstream.tgz" --from-archive lib/libpdfium.so=bin/libpdfium.so
    --from-archive LICENSE=notices/pdfium-binaries-LICENSE
    --from-archive licenses=notices/third-party --notices "$tmp/notices")

run_build() {
    local out=$1
    shift
    SOURCE_DATE_EPOCH=1767225600 FAKE_ASHIFT_LOG="$tmp/ashift.log" \
        bash "$builder" build "${common[@]}" --triple x86_64-unknown-linux-gnu --out "$out" "$@"
}

archive_list() {
    zstd -q -d -c "$1" | tar -tf -
}

echo "=== shared pack builder ==="

# 1. A good build: layout, notices, manifest, protocol flag.
: >"$tmp/ashift.log"
run_build "$tmp/out1" "${layout[@]}" --archive-sha256 "$good_sha" >/dev/null 2>&1 || fail "good build exits 0"
archive=$tmp/out1/pdfium-0.1.0-x86_64-unknown-linux-gnu-engine.tar.zst
check "good build writes the component archive" test -s "$archive"
check "good build writes pack.toml" test -s "$tmp/out1/pack.toml"
members=$(archive_list "$archive" 2>/dev/null || true)
for member in bin/ariad-pdfium-engine bin/libpdfium.so notices/pdfium-binaries-LICENSE notices/PACK.txt; do
    check "archive holds $member" grep -qx "$member" <<<"$members"
done
upstream_count=$(find "$tmp/up/licenses" -type f | wc -l | tr -d ' ')
packed_count=$(grep -c '^notices/third-party/.' <<<"$members" || true)
check "every upstream licence file is packed ($upstream_count)" test "$packed_count" -eq "$upstream_count"
check "pack.toml lists every notice file" test "$(grep -c '^    "notices/' "$tmp/out1/pack.toml")" -eq "$(grep -c '^notices/.*[^/]$' <<<"$members")"
check "pack.toml carries engine_protocol" grep -qx 'engine_protocol = "ariad-engine/2"' "$tmp/out1/pack.toml"
check "pack.toml lists target executables" grep -q '^executables = \[.*"bin/ariad-pdfium-engine"' "$tmp/out1/pack.toml"
check "ashift got --engine-protocol ariad-engine/2" grep -q -- '--engine-protocol ariad-engine/2' "$tmp/ashift.log"
check "ashift got the entrypoint" grep -q -- '--entrypoint bin/ariad-pdfium-engine' "$tmp/ashift.log"
check "ashift got the engine component" grep -q -- ' engine=.*-engine.tar.zst$' "$tmp/ashift.log"
check "engine is executable inside the archive" test "$(zstd -q -d -c "$archive" | tar -tvf - | grep ' bin/ariad-pdfium-engine$' | cut -c4)" = x

# 2. Reproducible archive bytes.
run_build "$tmp/out1b" "${layout[@]}" --archive-sha256 "$good_sha" >/dev/null 2>&1 || true
check "two builds give identical archive bytes" cmp -s "$archive" "$tmp/out1b/pdfium-0.1.0-x86_64-unknown-linux-gnu-engine.tar.zst"

# 3. Hash pins refuse.
if run_build "$tmp/out2" "${layout[@]}" --archive-sha256 "$wrong_sha" >"$tmp/o2" 2>&1; then
    fail "hash mismatch must refuse"
else
    pass "hash mismatch refuses"
fi
check "hash mismatch says so" grep -q 'SHA-256 mismatch' "$tmp/o2"
check "hash mismatch leaves no pack.toml" test ! -e "$tmp/out2/pack.toml"
check "hash mismatch leaves no archive" test -z "$(find "$tmp/out2" -name '*.tar.zst' 2>/dev/null)"
if run_build "$tmp/out3" "${layout[@]}" --archive-sha256 '' >"$tmp/o3" 2>&1; then
    fail "an empty pin must refuse"
else
    pass "an empty pin refuses"
fi
check "empty pin says so" grep -q 'no valid pinned SHA-256' "$tmp/o3"
if run_build "$tmp/out3b" "${layout[@]}" --archive-sha256 'abc123' >/dev/null 2>&1; then
    fail "a malformed pin must refuse"
else
    pass "a malformed pin refuses"
fi
if run_build "$tmp/out3c" --engine "$tmp/bin/ariad-pdfium-engine" \
    --archive-url http://example.invalid/pdfium.tgz --archive-sha256 "$good_sha" \
    --from-archive LICENSE=notices/x --notices "$tmp/notices" >"$tmp/o3c" 2>&1; then
    fail "a plain http URL must refuse"
else
    pass "a plain http URL refuses"
fi
check "http refusal says so" grep -q 'must be https' "$tmp/o3c"

# 4. Unsafe upstream archives.
python3 -I - "$tmp" <<'PY'
import io, sys, tarfile
tmp = sys.argv[1]
def make(path, build):
    with tarfile.open(path, "w:gz") as tar:
        build(tar)
def add(tar, name, data=b"x\n", kind=tarfile.REGTYPE, link=""):
    info = tarfile.TarInfo(name)
    info.type = kind
    info.linkname = link
    info.size = len(data) if kind == tarfile.REGTYPE else 0
    tar.addfile(info, io.BytesIO(data) if kind == tarfile.REGTYPE else None)
make(tmp + "/dotdot.tgz", lambda t: (add(t, "lib/libpdfium.so"), add(t, "../escape")))
make(tmp + "/symlink.tgz", lambda t: (add(t, "lib/libpdfium.so"), add(t, "LICENSE", kind=tarfile.SYMTYPE, link="/etc/passwd")))
PY
for bad in dotdot symlink; do
    if run_build "$tmp/out-$bad" "${layout[@]}" --archive "$tmp/$bad.tgz" \
        --archive-sha256 "$(sha256_of "$tmp/$bad.tgz")" >"$tmp/o-$bad" 2>&1; then
        fail "$bad upstream archive must refuse"
    else
        pass "$bad upstream archive refuses"
    fi
done

# 5. Missing notices, unknown flag support, bad destination.
if run_build "$tmp/out5" --engine "$tmp/bin/ariad-pdfium-engine" --engine-dest bin/ariad-pdfium-engine \
    >"$tmp/o5" 2>&1; then
    fail "a pack without notices must refuse"
else
    pass "a pack without notices refuses"
fi
check "missing notices says so" grep -q 'no notices/ directory' "$tmp/o5"
if FAKE_ASHIFT_NO_PROTOCOL=1 run_build "$tmp/out6" "${layout[@]}" --archive-sha256 "$good_sha" >"$tmp/o6" 2>&1; then
    fail "an ashift without --engine-protocol must refuse"
else
    pass "an ashift without --engine-protocol refuses"
fi
check "flag refusal says so" grep -q 'does not support' "$tmp/o6"
if run_build "$tmp/out7" "${layout[@]}" --archive-sha256 "$good_sha" --extra "$tmp/notices/PACK.txt=../x" >/dev/null 2>&1; then
    fail "a destination with .. must refuse"
else
    pass "a destination with .. refuses"
fi

# 6. macOS codesign: ad-hoc by default, Developer ID when the identity is set, never required
# when --no-codesign is given.
mkdir -p "$tmp/fakebin"
cat >"$tmp/fakebin/codesign" <<'SH'
#!/bin/sh
printf '%s\n' "$*" >>"$CODESIGN_LOG"
SH
chmod 755 "$tmp/fakebin/codesign"
mac_layout=(--engine "$tmp/bin/ariad-pdfium-engine" --engine-dest bin/ariad-pdfium-engine
    --archive "$tmp/upstream.tgz" --from-archive lib/libpdfium.dylib=bin/libpdfium.dylib
    --from-archive LICENSE=notices/pdfium-binaries-LICENSE --notices "$tmp/notices"
    --archive-sha256 "$good_sha")
mac_build() {
    local out=$1
    shift
    SOURCE_DATE_EPOCH=1767225600 FAKE_ASHIFT_LOG="$tmp/ashift.log" \
        bash "$builder" build "${common[@]}" --triple aarch64-apple-darwin --out "$out" "${mac_layout[@]}" "$@"
}
: >"$tmp/codesign.log"
CODESIGN_LOG="$tmp/codesign.log" PATH="$tmp/fakebin:$PATH" mac_build "$tmp/out-mac" >/dev/null 2>&1 || fail "mac build with codesign exits 0"
check "ad-hoc signs the engine" grep -q -- '--force --sign - .*bin/ariad-pdfium-engine' "$tmp/codesign.log"
check "ad-hoc signs the dylib" grep -q -- '--force --sign - .*libpdfium.dylib' "$tmp/codesign.log"
: >"$tmp/codesign.log"
APPLE_SIGNING_IDENTITY='Developer ID Application: Test (TEAM)' CODESIGN_LOG="$tmp/codesign.log" PATH="$tmp/fakebin:$PATH" \
    mac_build "$tmp/out-mac2" >/dev/null 2>&1 || fail "mac build with identity exits 0"
check "Developer ID signing uses the identity and hardened runtime" grep -q -- '--options runtime --timestamp --sign Developer ID Application: Test (TEAM)' "$tmp/codesign.log"
if command -v codesign >/dev/null 2>&1; then
    pass "codesign refusal skipped (codesign is installed here)"
else
    if mac_build "$tmp/out-mac3" >"$tmp/o-mac3" 2>&1; then fail "mac build without codesign must refuse"; else pass "mac build without codesign refuses"; fi
fi
check "--no-codesign builds without codesign" mac_build "$tmp/out-mac4" --no-codesign

# 7. Merge of two targets.
run_build "$tmp/dist" "${layout[@]}" --archive-sha256 "$good_sha" >/dev/null 2>&1 || true
SOURCE_DATE_EPOCH=1767225600 FAKE_ASHIFT_LOG="$tmp/ashift.log" bash "$builder" build "${common[@]}" \
    --triple x86_64-unknown-linux-musl --out "$tmp/dist-musl" "${layout[@]}" --archive-sha256 "$good_sha" >/dev/null 2>&1 || true
cp "$tmp/dist-musl"/*.tar.zst "$tmp/dist/"
if bash "$builder" merge "${common[@]}" --out "$tmp/merged" \
    --archive "x86_64-unknown-linux-gnu=$tmp/dist/pdfium-0.1.0-x86_64-unknown-linux-gnu-engine.tar.zst" \
    --archive "x86_64-unknown-linux-musl=$tmp/dist/pdfium-0.1.0-x86_64-unknown-linux-musl-engine.tar.zst" >/dev/null 2>&1; then
    pass "merge exits 0"
else
    fail "merge exits 0"
fi
check "merged manifest has both targets" test "$(grep -c '^\[\[targets\]\]' "$tmp/merged/pack.toml")" -eq 2
check "merged manifest keeps engine_protocol once" test "$(grep -c '^engine_protocol' "$tmp/merged/pack.toml")" -eq 1
check "merged manifest keeps the notices" grep -q '"notices/third-party/abseil.txt"' "$tmp/merged/pack.toml"

echo "=== pdfium wrapper ==="

# A copy of the pack tree with the synthetic pin, so the wrapper runs its real logic offline.
wt=$tmp/wrapper
mkdir -p "$wt/engines/pdfium/pack" "$wt/scripts/pack"
cp -R "$repo/engines/pdfium/pack/." "$wt/engines/pdfium/pack/"
cp "$builder" "$wt/scripts/pack/build-pack.sh"
printf 'licence\n' >"$wt/LICENSE"
printf 'notice\n' >"$wt/NOTICE"
sed "s|^linux-x64 .*|linux-x64 x86_64-unknown-linux-gnu pdfium-linux-x64.tgz $good_sha|" \
    "$repo/engines/pdfium/pack/targets.txt" >"$wt/engines/pdfium/pack/targets.txt"
wrap=$wt/engines/pdfium/pack/make.sh
check "wrapper prints a host slug" bash "$wrap" slug
if PDFIUM_ARCHIVE="$tmp/upstream.tgz" FAKE_ASHIFT_LOG="$tmp/ashift.log" bash "$wrap" pack --slug linux-x64 \
    --engine "$tmp/bin/ariad-pdfium-engine" --ashift "$fake" --out "$tmp/w-out" --unpack-dir "$tmp/w-dev" >"$tmp/ow" 2>&1; then
    pass "wrapper builds the pdfium pack"
else
    fail "wrapper builds the pdfium pack"
    cat "$tmp/ow" >&2
fi
check "dev pack passes verify" bash "$wrap" verify --pack "$tmp/w-dev"
check "dev pack holds the engine next to libpdfium" test -x "$tmp/w-dev/bin/ariad-pdfium-engine" -a -f "$tmp/w-dev/bin/libpdfium.so"
check "dev pack carries the AriadShift licence" test -s "$tmp/w-dev/notices/ariadshift-LICENSE"
check "dev pack manifest has engine_protocol" grep -qx 'engine_protocol = "ariad-engine/2"' "$tmp/w-dev/pack.toml"
# The committed table has no usable pin for an unrelated archive: the wrapper must refuse.
if PDFIUM_ARCHIVE="$tmp/upstream.tgz" bash "$repo/engines/pdfium/pack/make.sh" pack --slug linux-arm64 \
    --engine "$tmp/bin/ariad-pdfium-engine" --ashift "$fake" --out "$tmp/w-out2" >/dev/null 2>&1; then
    fail "the committed table must not accept an unpinned archive"
else
    pass "the committed table refuses an archive that does not match its pin"
fi
check "refusal leaves no pack.toml" test ! -e "$tmp/w-out2/pack.toml"
sed "s|pdfium-linux-x64.tgz|pdfium-v8-linux-x64.tgz|" "$wt/engines/pdfium/pack/targets.txt" >"$wt/engines/pdfium/pack/targets-v8.txt"
cp "$wt/engines/pdfium/pack/targets.txt" "$wt/engines/pdfium/pack/targets.keep"
cp "$wt/engines/pdfium/pack/targets-v8.txt" "$wt/engines/pdfium/pack/targets.txt"
if PDFIUM_ARCHIVE="$tmp/upstream.tgz" bash "$wrap" pack --slug linux-x64 --engine "$tmp/bin/ariad-pdfium-engine" \
    --ashift "$fake" --out "$tmp/w-out3" >"$tmp/ov8" 2>&1; then fail "a V8 asset must refuse"; else pass "a V8 asset refuses"; fi
check "V8 refusal says so" grep -q 'non-V8 assets only' "$tmp/ov8"
cp "$wt/engines/pdfium/pack/targets.keep" "$wt/engines/pdfium/pack/targets.txt"
rm -rf "$tmp/w-dev/notices/third-party/zlib.txt"
if bash "$wrap" verify --pack "$tmp/w-dev" >/dev/null 2>&1; then fail "verify catches a missing notice"; else pass "verify catches a missing notice"; fi

echo "=== signing guard ==="
printf 'name = "pdfium"\nversion = "0.1.0"\n' >"$tmp/m.toml"
printf 'untrusted comment: key\nRWQ\n' >"$tmp/key.pub"
sign() {
    env -u GITHUB_ACTIONS -u GITHUB_REF_TYPE -u GITHUB_REF_NAME "$@" \
        bash "$here/sign-pack.sh" --name pdfium --version 0.1.0 --manifest "$tmp/m.toml" --public-key "$tmp/key.pub"
}
if sign MINISIGN_SECRET_KEY=x >/dev/null 2>&1; then fail "signing refuses outside GitHub Actions"; else pass "signing refuses outside GitHub Actions"; fi
if sign GITHUB_ACTIONS=true GITHUB_REF_TYPE=branch GITHUB_REF_NAME=dev MINISIGN_SECRET_KEY=x >/dev/null 2>&1; then fail "signing refuses branch builds"; else pass "signing refuses branch builds"; fi
if sign GITHUB_ACTIONS=true GITHUB_REF_TYPE=tag GITHUB_REF_NAME=pack-pdfium-v9.9.9 MINISIGN_SECRET_KEY=x >/dev/null 2>&1; then fail "signing refuses a tag for another version"; else pass "signing refuses a tag for another version"; fi
if sign GITHUB_ACTIONS=true GITHUB_REF_TYPE=tag GITHUB_REF_NAME=pack-pdfium-v0.1.0 >/dev/null 2>&1; then fail "signing refuses without a key"; else pass "signing refuses without a key"; fi

if [ -n "${ASHIFT_BIN:-}" ]; then
    echo "=== real ashift ==="
    if "$ASHIFT_BIN" __pack manifest --help 2>&1 | grep -q -- '--engine-protocol'; then
        real_common=("${common[@]}")
        real_common[5]=$ASHIFT_BIN
        if bash "$builder" build "${real_common[@]}" --triple x86_64-unknown-linux-gnu --out "$tmp/real" \
            "${layout[@]}" --archive-sha256 "$good_sha" >"$tmp/oreal" 2>&1; then
            pass "real ashift writes the manifest"
            check "real manifest has engine_protocol" grep -qx 'engine_protocol = "ariad-engine/2"' "$tmp/real/pack.toml"
            check "real manifest lists target executables" grep -q '^executables = \[.*"bin/ariad-pdfium-engine"' "$tmp/real/pack.toml"
        else
            fail "real ashift writes the manifest"
            cat "$tmp/oreal" >&2
        fi
    else
        echo "SKIP | ASHIFT_BIN does not support --engine-protocol yet"
    fi
fi

echo "=== Summary ==="
echo "Passed: $passed, Failed: $failed"
[ "$failed" -eq 0 ]
