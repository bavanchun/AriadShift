#!/usr/bin/env bash
# Assemble a Tesseract runtime and a separately hashed, shared model component.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH='' cd -- "$here/../../.." && pwd)
requires_ashift=${PACK_REQUIRES_ASHIFT:-">=0.4.0, <2.0.0"}
die() { printf 'tesseract-pack: %s\n' "$*" >&2; exit 1; }
slug() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) echo linux-x64 ;;
        Linux-aarch64|Linux-arm64) echo linux-arm64 ;;
        Darwin-arm64) echo mac-arm64 ;;
        Darwin-x86_64) echo mac-x64 ;;
        *) die 'unsupported host platform' ;;
    esac
    if [ "$(uname -s)" = Linux ] && ldd --version 2>&1 | grep -qi musl; then
        die 'musl is not supported'
    fi
}
triple() {
    case $1 in
        linux-x64) echo x86_64-unknown-linux-gnu ;;
        linux-arm64) echo aarch64-unknown-linux-gnu ;;
        mac-x64) echo x86_64-apple-darwin ;;
        mac-arm64) echo aarch64-apple-darwin ;;
        *) die "unsupported target: $1" ;;
    esac
}
data_archive() (
    local data=$1 dest=$2 work tar_bin
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    mkdir -p "$work/tessdata"
    python3 - "$repo/engines/docling/pack/pins.toml" "$data" "$work/tessdata" <<'PY'
import hashlib, pathlib, sys, tomllib
pins = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())['tessdata']
root = pathlib.Path(sys.argv[2])
staged = pathlib.Path(sys.argv[3])
required = {'vie.traineddata', 'eng.traineddata', 'osd.traineddata', 'LICENSE'}
if {entry['path'] for entry in pins['files']} & required != required:
    sys.exit('tesseract-pack: incomplete model pin set')
for entry in pins['files']:
    if entry['path'] not in required:
        continue
    path = root / entry['path']
    if path.stat().st_size != entry['size']:
        sys.exit(f'tesseract-pack: invalid model or licence: {path}')
    with path.open('rb') as source:
        content = source.read(entry['size'] + 1)
    if len(content) != entry['size'] or hashlib.sha256(content).hexdigest() != entry['sha256']:
        sys.exit(f'tesseract-pack: invalid model or licence: {path}')
    if entry['path'].endswith('.traineddata'):
        (staged / entry['path']).write_bytes(content)
PY
    chmod 755 "$work/tessdata"
    chmod 644 "$work/tessdata/"*.traineddata
    if command -v gtar >/dev/null 2>&1; then tar_bin=gtar; else tar_bin=tar; fi
    "$tar_bin" --sort=name --mtime='@1735689600' --owner=0 --group=0 --numeric-owner \
        -cf - -C "$work" tessdata | zstd -q -19 -T1 -c >"$dest"
)
case ${1:-} in
    slug) slug ;;
    data)
        [ "$#" -eq 3 ] || die 'usage: make.sh data TESSDATA_DIR ARCHIVE'
        data_archive "$2" "$3"
        ;;
    pack)
        [ "$#" -eq 6 ] || die 'usage: make.sh pack SLUG RUNTIME_DIR TESSDATA_DIR ASHIFT OUT'
        host=$2 runtime=$3 data=$4 ashift=$5 dest=$6
        [ "$host" = "$(slug)" ] || die 'pack target must match the host'
        [ -x "$runtime/bin/tesseract" ] || die 'Tesseract executable is missing'
        mkdir -p "$dest-dist"
        data_file=$dest-dist/tesseract-0.1.0-tessdata.tar.zst
        data_archive "$data" "$data_file"
        bash "$repo/scripts/pack/build-pack.sh" build --name tesseract --version 0.1.0 \
            --triple "$(triple "$host")" --ashift "$ashift" --out "$dest-dist" \
            --description 'Tesseract OCR engine' --license 'Apache-2.0 AND BSD-2-Clause' \
            --requires-ashift "$requires_ashift" --engine-protocol ariad-engine/2 \
            --component-name runtime --shared-component "tessdata=$data_file" \
            --target-env 'TESSDATA_PREFIX={root}/tessdata' \
            --entrypoint bin/tesseract --engine "$runtime/bin/tesseract" \
            --notices "$here/notices" --unpack-dir "$dest"
        zstd -q -d -c "$data_file" | tar -xf - -C "$dest"
        ;;
    merge)
        [ "$#" -eq 4 ] || die 'usage: make.sh merge DIST TESSDATA_DIR ASHIFT'
        dist=$2 data=$3 ashift=$4
        data_file=$dist/tesseract-0.1.0-tessdata.tar.zst
        data_archive "$data" "$data_file"
        archives=()
        for host in linux-x64 linux-arm64 mac-x64 mac-arm64; do
            target=$(triple "$host")
            archive=$dist/tesseract-0.1.0-$target-runtime.tar.zst
            [ -f "$archive" ] || die "missing runtime for $host"
            archives+=(--archive "$target=$archive")
        done
        bash "$repo/scripts/pack/build-pack.sh" merge --name tesseract --version 0.1.0 \
            --ashift "$ashift" --out "$dist" --description 'Tesseract OCR engine' \
            --license 'Apache-2.0 AND BSD-2-Clause' --requires-ashift "$requires_ashift" \
            --engine-protocol ariad-engine/2 --component-name runtime \
            --shared-component "tessdata=$data_file" --target-env 'TESSDATA_PREFIX={root}/tessdata' \
            --entrypoint bin/tesseract "${archives[@]}"
        ;;
    verify)
        [ "$#" -eq 2 ] || die 'usage: make.sh verify PACK_DIR'
        root=$2
        [ -x "$root/bin/tesseract" ] || die 'missing executable'
        for notice in tesseract-LICENSE.txt leptonica-LICENSE.txt tessdata-LICENSE.txt; do
            [ -s "$root/notices/$notice" ] || die "missing notice: $notice"
            cmp -s "$here/notices/$notice" "$root/notices/$notice" || die "changed notice: $notice"
        done
        python3 - "$root/pack.toml" "$root/tessdata" "$repo/engines/docling/pack/pins.toml" <<'PY'
import hashlib, pathlib, sys, tomllib
manifest = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
if manifest.get('name') != 'tesseract' or manifest.get('engine_protocol') != 'ariad-engine/2':
    sys.exit('invalid Tesseract manifest')
if set(manifest.get('notices', [])) != {'notices/tesseract-LICENSE.txt', 'notices/leptonica-LICENSE.txt', 'notices/tessdata-LICENSE.txt'}:
    sys.exit('invalid notice list')
if not manifest.get('targets'):
    sys.exit('no pack targets')
for target in manifest['targets']:
    if target['entrypoint'] != ['bin/tesseract'] or target.get('env', {}).get('TESSDATA_PREFIX') != '{root}/tessdata':
        sys.exit('invalid target configuration')
    if {c['name'] for c in target['components']} != {'runtime', 'tessdata'}:
        sys.exit('invalid component set')
pins = tomllib.loads(pathlib.Path(sys.argv[3]).read_text())['tessdata']['files']
for entry in pins:
    if not entry['path'].endswith('.traineddata'):
        continue
    path = pathlib.Path(sys.argv[2]) / entry['path']
    if path.stat().st_size != entry['size']:
        sys.exit('model hash mismatch')
    with path.open('rb') as source:
        content = source.read(entry['size'] + 1)
    if len(content) != entry['size'] or hashlib.sha256(content).hexdigest() != entry['sha256']:
        sys.exit('model hash mismatch')
PY
        ;;
    *) die 'usage: make.sh slug | data | pack | merge | verify' ;;
esac
