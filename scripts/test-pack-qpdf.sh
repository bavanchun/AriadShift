#!/usr/bin/env bash
set -euo pipefail
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
script=$repo/engines/qpdf/pack/make.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf 'invalid archive\n' >"$tmp/bad"
if bash "$script" verify-archive mac-arm64 "$tmp/bad" >/dev/null 2>&1; then
    echo 'hash mismatch was accepted' >&2; exit 1
fi
if bash "$script" verify-archive invalid "$tmp/bad" >/dev/null 2>&1; then
    echo 'unknown target was accepted' >&2; exit 1
fi
# The PR completing the GNU runtime review must flip or remove this assertion.
if bash "$repo/engines/qpdf/pack/check-release.sh" >/dev/null 2>&1; then
    echo 'committed incomplete runtime review was accepted' >&2; exit 1
fi
cp "$repo/engines/qpdf/pack/check-release.sh" "$tmp/check-release.sh"
cp "$repo/engines/qpdf/pack/licence-review.toml" "$tmp/licence-review.toml"
mkdir -p "$tmp/sources" "$tmp/receipts"
python3 - "$tmp" <<'PYTEST'
import hashlib, pathlib, sys
root = pathlib.Path(sys.argv[1])
entries = []
for i in range(5):
    name = f'source-{i}'
    data = name.encode()
    (root / 'sources' / name).write_bytes(data)
    entries.append(f'{hashlib.sha256(data).hexdigest()} {name} https://example.invalid/{name}\n')
manifest = ''.join(entries)
(root / 'runtime-sources.txt').write_text(manifest)
platforms = {
    'linux-x64': ('f6adcfd450f0927a05c66ded9b4c3bb538fbab625b1ca3f35d71ac093b24b57f', '14.2.1', '308fa896e2bdae71da3b8dc67fc577d3951c74e8bc7a6e6c96e9da7ee1911784', '99358806eb985f6202a57b56ea2f105d1e4c987729421cc136b4b7c0678a3b9a'),
    'linux-arm64': ('54b271d9562a3686298880df05367909e949ed317e7b1b79b59c696d75f78676', '14.2.1', 'd8dcea39c8507d21c2274e9353b1ee720f172b303dcb5e1bf4d4cfbe38f72f2c', '69c8b51af4a99672127fffc2fe892d0d57329e2b4058bcd925edc44fada8c213'),
    'linux-musl-x64': ('be2656061b536c1883c4034790e21d0a09c5aaaf51968b89368a3c0371ea2c9a', '15.2.0', '3c5a681959e644f7274b39a9efb4dbb56533a57306a456ce3580bd94f31520a5', 'aeae756b465a790521bc47a999d0c935c3efad7df06e1b89d334f52d371918e2'),
    'linux-musl-arm64': ('be2656061b536c1883c4034790e21d0a09c5aaaf51968b89368a3c0371ea2c9a', '15.2.0', 'b2b03b70be60cd0ae15cdca348fb002313786d229391a6b064c747528de68936', '13418fd4e14bf4cd601f3d41addbec08617533454cc6fef2a0ce92ecbf83118f'),
}
apk_hashes = {
    'linux-musl-x64': ['d1d482472ed91240731c60dbae8176bc1e32e1dfb694856da6759f6d32716325', '563244a562f5c48b921f7f0b4a571e373cfcba65175fe2d4f752c5f975738bcd', '62ce308cdbf127dcac6eee1520a4ffec8dc98b30974653908842fe4cc7d90b43', '310b51cff12e1d5ce5f135c558a82e2550427da5465bbd16c9ff9b05e63ee3d9'],
    'linux-musl-arm64': ['ff3fecfc32c9a1bed4a47a32ccfee6f6f575708258f912c218dce8ed0d892221', '2ec3dc4be953d842cc51193248367d86829ed699a1ed094fb5149b1b3fc62919', '8385c811d93b64ee1877005a5473cde8ebabe82d346301f2b153ff8cb6a71510', '45afe0a9f78de3402b6b5d11f5153ed955bf59d59b45344d9c3fed9f4bbf2f66'],
}
for slug, (image, gcc, stdcpp, libgcc) in platforms.items():
    d = root / 'receipts' / f'qpdf-input-receipts-{slug}'
    d.mkdir()
    for name, value in {'image.txt': f'image@sha256:{image}', 'image-id.txt': 'sha256:abc', 'gcc-version.txt': gcc, 'packages.txt': 'package', 'package-sha256.txt': ''.join(f'{h} package\n' for h in apk_hashes.get(slug, ['hash'])), 'runtime-sha256.txt': f'{libgcc}  libgcc.a\n{stdcpp}  libstdc++.a\n', 'runtime-sources.txt': manifest}.items():
        (d / name).write_text(value)
PYTEST
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('pending_owner_approval = true', 'pending_owner_approval = false'))
PY
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" >/dev/null 2>&1; then
    echo 'incomplete runtime review was accepted' >&2; exit 1
fi
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('runtime_review_complete = false', 'runtime_review_complete = true'))
PY
bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources"
mv "$tmp/receipts/qpdf-input-receipts-linux-x64" "$tmp/missing-receipt"
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" >/dev/null 2>&1; then
    echo 'missing receipt was accepted' >&2; exit 1
fi
mv "$tmp/missing-receipt" "$tmp/receipts/qpdf-input-receipts-linux-x64"
mv "$tmp/sources/source-0" "$tmp/missing-source"
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" >/dev/null 2>&1; then
    echo 'missing source was accepted' >&2; exit 1
fi
mv "$tmp/missing-source" "$tmp/sources/source-0"
printf 'changed' > "$tmp/sources/source-0"
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" >/dev/null 2>&1; then
    echo 'source hash mismatch was accepted' >&2; exit 1
fi
printf 'source-0' > "$tmp/sources/source-0"
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('version = "3.2.1"', 'version = "3.2.0"'))
PY
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" >/dev/null 2>&1; then
    echo 'mismatched component review was accepted' >&2; exit 1
fi
python3 - "$repo/engines/qpdf/pack/licence-review.toml" "$tmp" <<'PYTEST'
import pathlib, re, subprocess, sys, tomllib

source, directory = map(pathlib.Path, sys.argv[1:])
review_path = directory / 'licence-review.toml'
original = source.read_text()
review = tomllib.loads(original)
repo = source.parents[3]
gate = (source.parent / 'check-release.sh').read_text()
compiler = (source.parent / 'compile-qpdf.sh').read_text()
workflow = (repo / '.github/workflows/pack-qpdf.yml').read_text()

def one(pattern, text, label):
    matches = re.findall(pattern, text, re.MULTILINE)
    if len(matches) != 1:
        sys.exit(f'{label}: expected exactly one match')
    return matches[0]

source_url = one(r"'source': '([^']+)'", gate, 'release gate source')
source_hash = one(r"'sha256': '([^']+)'", gate, 'release gate hash')
build_hash = one(r'^verify ([0-9a-f]{64}) "\$jpeg"$', compiler, 'build hash')
download_url = one(r'curl [^\n]* (https://\S*libjpeg-turbo\S*) -o "\$RUNNER_TEMP/qpdf-input/jpeg.tar.gz"', workflow, 'download source')
if review['source'] != source_url or source_url != download_url:
    sys.exit('libjpeg-turbo source URL differs between review, gate and workflow')
if review['sha256'] != source_hash or source_hash != build_hash:
    sys.exit('libjpeg-turbo SHA-256 differs between review, gate and build')

def check(text, accepted, label):
    review_path.write_text(text)
    result = subprocess.run(['bash', str(directory / 'check-release.sh'), str(directory / 'receipts'), str(directory / 'sources')], capture_output=True)
    if (result.returncode == 0) != accepted:
        sys.exit(f'{label}: unexpected release gate result')

approved = original.replace('pending_owner_approval = true', 'pending_owner_approval = false')
approved = approved.replace('runtime_review_complete = false', 'runtime_review_complete = true')
check(approved.replace('runtime_review_complete = true', 'runtime_review_complete = false'), False, 'runtime pending')
check(approved.replace('pending_owner_approval = false', 'pending_owner_approval = true'), False, 'owner approval pending')
check(approved, True, 'both reviews complete')
check(approved + '\nunknown_key = true\n', False, 'unknown key')
check(approved.replace('pending_owner_approval = false', 'pending_owner_approval = "false"'), False, 'string flag')
for key, value in tomllib.loads(approved).items():
    if isinstance(value, bool):
        literal = 'true' if value else 'false'
        for invalid in ('0', '1', '"true"', '"false"'):
            check(approved.replace(f'{key} = {literal}', f'{key} = {invalid}'), False,
                  f'non-boolean {key} = {invalid}')
for key, value in tomllib.loads(approved).items():
    lines = approved.splitlines(keepends=True)
    replacement = 'true' if value is False else 'false' if value is True else '"mismatch"'
    changed = ''.join(f'{key} = {replacement}\n' if line.startswith(f'{key} = ') else line for line in lines)
    check(changed, False, f'mismatched {key}')
PYTEST
if [ -n "${QPDF_SOURCE_ARCHIVE:-}" ]; then bash "$script" verify-archive mac-arm64 "$QPDF_SOURCE_ARCHIVE"; fi
if [ -n "${QPDF_TEST_PACK:-}" ]; then
    bash "$script" verify "$QPDF_TEST_PACK"
    cp -R "$QPDF_TEST_PACK" "$tmp/pack"
    rm "$tmp/pack/notices/qpdf-LICENSE.txt"
    if bash "$script" verify "$tmp/pack" >/dev/null 2>&1; then
        echo 'missing notice was accepted' >&2; exit 1
    fi
fi
echo 'qpdf pack checks passed'
