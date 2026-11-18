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
cp "$repo/engines/qpdf/pack/targets.txt" "$tmp/targets.txt"
cp "$repo/engines/qpdf/pack/licence-review.toml" "$tmp/licence-review.toml"
cp "$repo/engines/qpdf/pack/build-inputs.txt" "$tmp/build-inputs.txt"
cp "$repo/engines/qpdf/pack/compile-qpdf.sh" "$tmp/compile-qpdf.sh"
mkdir -p "$tmp/sources" "$tmp/receipts" "$tmp/packdir" "$tmp/notices"
python3 - "$tmp" <<'PYTEST'
import gzip, hashlib, io, pathlib, subprocess, sys, tarfile
root = pathlib.Path(sys.argv[1])

def make_tar_bytes(member_name: str, payload: bytes) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode='w:') as tf:
        info = tarfile.TarInfo(member_name)
        info.size = len(payload)
        info.mtime = 0
        info.uid = 0
        info.gid = 0
        tf.addfile(info, io.BytesIO(payload))
    return buf.getvalue()

entries = []
pointer_blocks = ['GNU runtime source pointers\n']
aports_specs = {
    1: ('aports-gcc.tar.gz', '1' * 40, '2' * 40, 'gcc/APKBUILD', b'aports gcc fixture\n' * 128),
    3: ('aports-musl.tar.gz', '3' * 40, '4' * 40, 'musl/APKBUILD', b'aports musl fixture\n' * 128),
}
for i in range(5):
    if i in aports_specs:
        name, commit, tree, member_name, raw_payload = aports_specs[i]
        tar_bytes = make_tar_bytes(member_name, raw_payload)
        if i == 1:
            (root / 'aports-gcc.tar').write_bytes(tar_bytes)
        gz_bytes = gzip.compress(tar_bytes, compresslevel=9, mtime=0)
        (root / 'sources' / name).write_bytes(gz_bytes)
        tar_digest = hashlib.sha256(tar_bytes).hexdigest()
        gz_digest = hashlib.sha256(gz_bytes).hexdigest()
        url = f'https://github.com/alpinelinux/aports.git#{commit}:{tree}'
        entries.append(f'{tar_digest} {name} {url}\n')
        pointer_blocks.append(
            'https://github.com/alpinelinux/aports.git\n'
            f'Commit: {commit}\n'
            f'Tree: {tree}\n'
            f'Uncompressed tar SHA-256: {tar_digest}\n'
            f'Informational .tar.gz SHA-256 (gzip -n -9): {gz_digest}\n'
        )
    else:
        name = f'source-{i}'
        data = name.encode()
        (root / 'sources' / name).write_bytes(data)
        digest = hashlib.sha256(data).hexdigest()
        url = f'https://example.invalid/{name}'
        entries.append(f'{digest} {name} {url}\n')
        pointer_blocks.append(f'{url}\nSHA-256: {digest}\n')
manifest = ''.join(entries)
(root / 'runtime-sources.txt').write_text(manifest)
(root / 'notices' / 'gnu-runtime-source-pointers.txt').write_text('\n'.join(pointer_blocks))
build_inputs_manifest = (root / 'build-inputs.txt').read_text()
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
targets = {line.split()[0]: line.split()[1] for line in (root / 'targets.txt').read_text().splitlines() if line and not line.startswith('#')}
manifest_lines = []
(root / 'bin').mkdir()
(root / 'bin/qpdf').write_bytes(b'qpdf fixture')
for slug, (image, gcc, stdcpp, libgcc) in platforms.items():
    archive = root / 'packdir' / f'{slug}.tar.zst'
    subprocess.run(['tar', '--zstd', '-cf', str(archive), '-C', str(root), 'bin/qpdf'], check=True)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    manifest_lines.append(f'[[targets]]\ntriple = "{targets[slug]}"\n[[targets.components]]\nname = "engine"\nurl = "{archive.name}"\nsha256 = "{digest}"\n')
    d = root / 'receipts' / f'qpdf-input-receipts-{slug}'
    d.mkdir()
    for name, value in {'image.txt': f'image@sha256:{image}', 'image-id.txt': 'sha256:abc', 'gcc-version.txt': gcc, 'packages.txt': 'package', 'package-sha256.txt': ''.join(f'{h} package\n' for h in apk_hashes.get(slug, ['hash'])), 'runtime-sha256.txt': f'{libgcc}  libgcc.a\n{stdcpp}  libstdc++.a\n', 'runtime-sources.txt': manifest, 'build-inputs.txt': build_inputs_manifest, 'qpdf-sha256.txt': f'{hashlib.sha256(b"qpdf fixture").hexdigest()}  bin/qpdf\n', 'runtime-extra-sha256.txt': ''.join(f'{"0" * 64}  /runtime/{name}\n' for name in ('libgcc_eh.a', 'crtbegin.o', 'crtend.o'))}.items():
        (d / name).write_text(value)
(root / 'packdir/pack.toml').write_text(''.join(manifest_lines))
PYTEST
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('pending_owner_approval = true', 'pending_owner_approval = false'))
PY
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" "$tmp/packdir" >/dev/null 2>&1; then
    echo 'incomplete runtime review was accepted' >&2; exit 1
fi
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('runtime_review_complete = false', 'runtime_review_complete = true'))
PY
bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" "$tmp/packdir"
printf 'wrong  bin/qpdf\n' > "$tmp/receipts/qpdf-input-receipts-linux-x64/qpdf-sha256.txt"
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" "$tmp/packdir" >/dev/null 2>&1; then
    echo 'qpdf hash mismatch was accepted' >&2; exit 1
fi
printf '%s  bin/qpdf\n' "$(printf 'qpdf fixture' | sha256sum | cut -d ' ' -f 1)" > "$tmp/receipts/qpdf-input-receipts-linux-x64/qpdf-sha256.txt"
mv "$tmp/receipts/qpdf-input-receipts-linux-x64" "$tmp/missing-receipt"
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" "$tmp/packdir" >/dev/null 2>&1; then
    echo 'missing receipt was accepted' >&2; exit 1
fi
mv "$tmp/missing-receipt" "$tmp/receipts/qpdf-input-receipts-linux-x64"
mv "$tmp/sources/source-0" "$tmp/missing-source"
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" "$tmp/packdir" >/dev/null 2>&1; then
    echo 'missing source was accepted' >&2; exit 1
fi
mv "$tmp/missing-source" "$tmp/sources/source-0"
printf 'changed' > "$tmp/sources/source-0"
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" "$tmp/packdir" >/dev/null 2>&1; then
    echo 'source hash mismatch was accepted' >&2; exit 1
fi
printf 'source-0' > "$tmp/sources/source-0"
python3 - "$tmp/licence-review.toml" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('version = "3.2.1"', 'version = "3.2.0"'))
PY
if bash "$tmp/check-release.sh" "$tmp/receipts" "$tmp/sources" "$tmp/packdir" >/dev/null 2>&1; then
    echo 'mismatched component review was accepted' >&2; exit 1
fi
python3 - "$repo/engines/qpdf/pack/licence-review.toml" "$tmp" <<'PYTEST'
import gzip, hashlib, pathlib, re, subprocess, sys, tomllib

source, directory = map(pathlib.Path, sys.argv[1:])
review_path = directory / 'licence-review.toml'
original = source.read_text()
review = tomllib.loads(original)
repo = source.parents[3]
gate = (source.parent / 'check-release.sh').read_text()
compiler = (source.parent / 'compile-qpdf.sh').read_text()
build_inputs_text = (source.parent / 'build-inputs.txt').read_text()
runtime_sources_raw = (source.parent / 'runtime-sources.txt').read_bytes()
pointers_text = (source.parent / 'notices/gnu-runtime-source-pointers.txt').read_text()
targets_text = (source.parent / 'targets.txt').read_text()
workflow = (repo / '.github/workflows/pack-qpdf.yml').read_text()

def one(pattern, text, label):
    matches = re.findall(pattern, text, re.MULTILINE)
    if len(matches) != 1:
        sys.exit(f'{label}: expected exactly one match')
    return matches[0]

if 'zlib.net' in build_inputs_text or 'zlib.net' in workflow:
    sys.exit('mutable zlib.net URL must not appear in build inputs or workflow')

build_rows = [line.split() for line in build_inputs_text.splitlines()]
if len(build_rows) != 3 or any(len(row) != 3 for row in build_rows):
    sys.exit('build-inputs.txt must have exactly three <sha256> <filename> <url> entries')
build_inputs = {name: (digest, url) for digest, name, url in build_rows}
if len(build_inputs) != 3:
    sys.exit('duplicate filename in build-inputs.txt')

source_url = one(r"'source': '([^']+)'", gate, 'release gate source')
source_hash = one(r"'sha256': '([^']+)'", gate, 'release gate hash')
qpdf_build_hash = one(r'^verify ([0-9a-f]{64}) "\$qpdf"$', compiler, 'qpdf build hash')
jpeg_build_hash = one(r'^verify ([0-9a-f]{64}) "\$jpeg"$', compiler, 'libjpeg-turbo build hash')
zlib_build_hash = one(r'^verify ([0-9a-f]{64}) "\$zlib"$', compiler, 'zlib build hash')

target_assets = {(line.split()[2], line.split()[3]) for line in targets_text.splitlines() if line and not line.startswith('#')}
if target_assets != {('qpdf-12.4.2.tar.gz', qpdf_build_hash)}:
    sys.exit('qpdf target asset or hash differs between targets.txt and compile-qpdf.sh')

expected_inputs = {
    'qpdf-12.4.2.tar.gz': (
        qpdf_build_hash,
        'https://github.com/qpdf/qpdf/releases/download/v12.4.2/qpdf-12.4.2.tar.gz',
    ),
    'libjpeg-turbo-3.2.1.tar.gz': (
        jpeg_build_hash,
        review['source'],
    ),
    'zlib-1.3.2.tar.gz': (
        'bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16',
        'https://github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.gz',
    ),
}
if review['source'] != source_url or review['sha256'] != source_hash or source_hash != jpeg_build_hash:
    sys.exit('libjpeg-turbo source URL or SHA-256 differs between review, gate and build')
if zlib_build_hash != expected_inputs['zlib-1.3.2.tar.gz'][0]:
    sys.exit('zlib SHA-256 in compile-qpdf.sh does not match pinned release asset')
if build_inputs != expected_inputs:
    sys.exit('build-inputs.txt differs from pinned build hashes or immutable upstream URLs')

runtime_lines = runtime_sources_raw.decode().splitlines()
runtime_rows = [line.split() for line in runtime_lines]
if (not runtime_sources_raw.endswith(b'\n') or len(runtime_rows) != 5 or
        any(len(row) != 3 or line != ' '.join(row) for line, row in zip(runtime_lines, runtime_rows))):
    sys.exit('runtime-sources.txt must have 5 canonical <sha256> <filename> <url> lines')

if 'Re-archive SHA-256:' in pointers_text:
    sys.exit('gnu-runtime-source-pointers.txt still contains legacy Re-archive SHA-256')

direct_pointer_rows = re.findall(
    r'^(https://[^\s]+)\nSHA-256: ([0-9a-f]{64})$',
    pointers_text,
    re.MULTILINE,
)
aports_pointer_rows = re.findall(
    r'^(https://github\.com/alpinelinux/aports\.git)\n'
    r'Commit: ([0-9a-f]{40})\n'
    r'Tree: ([0-9a-f]{40})\n'
    r'Uncompressed tar SHA-256: ([0-9a-f]{64})\n'
    r'Informational \.tar\.gz SHA-256 \(gzip -n -9\): ([0-9a-f]{64})$',
    pointers_text,
    re.MULTILINE,
)
if len(direct_pointer_rows) != 3 or len(aports_pointer_rows) != 2:
    sys.exit('gnu-runtime-source-pointers.txt must declare 3 direct and 2 aports entries')

pointer_authoritative = {url: digest for url, digest in direct_pointer_rows}
informational_aports = {}
for repo_url, commit, tree, tar_digest, gz_digest in aports_pointer_rows:
    aports_url = f'{repo_url}#{commit}:{tree}'
    pointer_authoritative[aports_url] = tar_digest
    informational_aports[aports_url] = gz_digest

runtime_map = {url: (digest, name) for digest, name, url in runtime_rows}
if {url: digest for url, (digest, _) in runtime_map.items()} != pointer_authoritative:
    sys.exit('runtime-sources.txt and gnu-runtime-source-pointers.txt authoritative digests/URLs differ')

expected_aports_info = {
    'https://github.com/alpinelinux/aports.git#423a8ad043d07f2c7546c8ec3e2b0384cda360ae:fdc22fc9917d617912bd2c28bfee03adf6d6337f': (
        'aports-gcc.tar.gz',
        'c38e19e3131cce45c172ed1ac4c37bb6db3052ad2c82335a1254f9c292d81fc0',
        'f0da39bfec4e09fa4b094a8b56de789ef51173fb0fde40d5956e5d08b8fdf44f',
    ),
    'https://github.com/alpinelinux/aports.git#8f79d39391b46d6dbf1800fb68b8212a21d5ee2e:a2c974cbf55e94e4486872ef61f890e41eaed465': (
        'aports-musl.tar.gz',
        '0d9b3c0faf12c68caa14bc4d096ff2e59b513ac30552bf0e59d1599abf19a78a',
        'f60b8b0741ea79a46c50b1c0cf193da2f03b47569f105a6e7190426ed03f3cf6',
    ),
}
for url, (expected_name, expected_tar_sha, expected_gz_sha) in expected_aports_info.items():
    if runtime_map.get(url) != (expected_tar_sha, expected_name):
        sys.exit(f'runtime-sources.txt mismatch for {expected_name}')
    if informational_aports.get(url) != expected_gz_sha:
        sys.exit(f'gnu-runtime-source-pointers.txt missing informational .tar.gz SHA-256 for {expected_name}')

for needle, count, label in (
    ('done < engines/qpdf/pack/build-inputs.txt', 1, 'workflow build-inputs loop'),
    ('done < engines/qpdf/pack/runtime-sources.txt', 1, 'workflow runtime-sources loop'),
    ("--proto '=https' --proto-redir '=https' --tlsv1.2", 2, 'workflow HTTPS-only curl flags'),
    ('sha256sum --check', 2, 'workflow Linux-only sha256sum --check verification'),
    ('shasum -a 256 --check', 0, 'workflow shasum --check prohibition'),
    ('actual=$(sha256sum "$RUNNER_TEMP/qpdf-input/$name")', 1, 'workflow flag-free sha256sum verification'),
    ('actual=$(shasum -a 256 "$RUNNER_TEMP/qpdf-input/$name")', 1, 'workflow flag-free shasum verification'),
    ('[ "${actual%% *}" = "$digest" ]', 1, 'workflow digest comparison'),
    ('cp engines/qpdf/pack/build-inputs.txt "$RUNNER_TEMP/qpdf-output/receipt/build-inputs.txt"', 1, 'workflow receipt copy'),
    ('compile-qpdf.sh /input/qpdf-12.4.2.tar.gz /input/libjpeg-turbo-3.2.1.tar.gz /input/zlib-1.3.2.tar.gz /output', 2, 'workflow Linux compile-qpdf args'),
    ('compile-qpdf.sh "$RUNNER_TEMP/qpdf-input/qpdf-12.4.2.tar.gz"\n          "$RUNNER_TEMP/qpdf-input/libjpeg-turbo-3.2.1.tar.gz" "$RUNNER_TEMP/qpdf-input/zlib-1.3.2.tar.gz"\n          "$RUNNER_TEMP/qpdf-output"', 1, 'workflow macOS compile-qpdf args'),
):
    if workflow.count(needle) != count:
        sys.exit(f'{label}: expected {count} occurrence(s) in pack-qpdf.yml')

download_step = re.search(
    r'^\s*- name: Download pinned inputs\n(.*?)(?=^\s*- name: )',
    workflow,
    re.MULTILINE | re.DOTALL,
)
if download_step is None:
    sys.exit('missing Download pinned inputs step in pack-qpdf.yml')
download_body = download_step.group(1)
if 'sha256sum --check' in download_body or '--check' in download_body:
    sys.exit('Download pinned inputs step must not use GNU-only --check flag')
for needle in (
    'actual=$(sha256sum "$RUNNER_TEMP/qpdf-input/$name")',
    'actual=$(shasum -a 256 "$RUNNER_TEMP/qpdf-input/$name")',
    '[ "${actual%% *}" = "$digest" ]',
):
    if needle not in download_body:
        sys.exit(f'Download pinned inputs step missing portable check: {needle}')

sources_step = re.search(
    r'^\s*- name: Download corresponding sources\n(.*?)(?=^\s*- name: )',
    workflow,
    re.MULTILINE | re.DOTALL,
)
if sources_step is None:
    sys.exit('missing Download corresponding sources step in pack-qpdf.yml')
sources_body = sources_step.group(1)
for needle in (
    'rm -rf "$RUNNER_TEMP/aports" "$RUNNER_TEMP/aports.tar"',
    'test "$(git -C "$RUNNER_TEMP/aports" rev-parse "FETCH_HEAD^{tree}")" = "$tree"',
    'git -C "$RUNNER_TEMP/aports" archive --format=tar --mtime=1970-01-01T00:00:00Z "$tree" > "$RUNNER_TEMP/aports.tar"',
    'verify_path="$RUNNER_TEMP/aports.tar"',
    'printf \'%s  %s\\n\' "$digest" "$verify_path" | sha256sum --check',
    'gzip -n -9 -c "$RUNNER_TEMP/aports.tar" > "sources/$name"',
    'Informational .tar.gz SHA-256 (gzip -n -9):',
    "--proto '=https' --proto-redir '=https' --tlsv1.2",
    '[ "$count" -eq 5 ]',
):
    if needle not in sources_body:
        sys.exit(f'Download corresponding sources step missing: {needle}')
if sources_body.index('sha256sum --check') >= sources_body.index('gzip -n -9 -c "$RUNNER_TEMP/aports.tar" > "sources/$name"'):
    sys.exit('Download corresponding sources must verify uncompressed tar before gzip compression')

def check(text, accepted, label, expected_err=None):
    review_path.write_text(text)
    result = subprocess.run(['bash', str(directory / 'check-release.sh'), str(directory / 'receipts'), str(directory / 'sources'), str(directory / 'packdir')], capture_output=True)
    stderr = result.stderr.decode(errors='replace')
    if (result.returncode == 0) != accepted:
        sys.exit(f'{label}: unexpected release gate result (rc={result.returncode}, stderr={stderr})')
    if not accepted:
        if 'qpdf release refused:' not in stderr or 'Traceback' in stderr:
            sys.exit(f'{label}: expected clean refusal without traceback, got: {stderr}')
        if expected_err is not None and expected_err not in stderr:
            sys.exit(f'{label}: expected {expected_err!r} in stderr, got: {stderr}')

approved = original.replace('pending_owner_approval = true', 'pending_owner_approval = false')
approved = approved.replace('runtime_review_complete = false', 'runtime_review_complete = true')
check(approved.replace('runtime_review_complete = true', 'runtime_review_complete = false'), False, 'runtime pending')
check(approved.replace('pending_owner_approval = false', 'pending_owner_approval = true'), False, 'owner approval pending')
check(approved, True, 'both reviews complete')
if subprocess.run(['bash', str(directory / 'check-release.sh'), '--review-only']).returncode != 0:
    sys.exit('approved review-only check was rejected')
review_path.write_text(original)
if subprocess.run(['bash', str(directory / 'check-release.sh'), '--review-only'], capture_output=True).returncode == 0:
    sys.exit('pending review-only check was accepted')

receipt_build_inputs = directory / 'receipts/qpdf-input-receipts-linux-x64/build-inputs.txt'
saved_receipt_inputs = receipt_build_inputs.read_text()
receipt_build_inputs.unlink()
check(approved, False, 'missing receipt build-inputs.txt')
receipt_build_inputs.write_text(saved_receipt_inputs)

# Each mutation must independently make the otherwise valid evidence fail.
mutations = [
    (directory / 'receipts/qpdf-input-receipts-linux-x64/image.txt', lambda text: text.replace('f6adc', '00000')),
    (directory / 'receipts/qpdf-input-receipts-linux-x64/runtime-sha256.txt', lambda text: '0' + text[1:]),
    (directory / 'receipts/qpdf-input-receipts-linux-x64/gcc-version.txt', lambda text: 'wrong compiler'),
    (directory / 'receipts/qpdf-input-receipts-linux-musl-x64/package-sha256.txt', lambda text: text.replace('d1d482', '000000')),
    (directory / 'receipts/qpdf-input-receipts-linux-x64/runtime-sources.txt', lambda text: text + 'changed'),
    (directory / 'receipts/qpdf-input-receipts-linux-x64/build-inputs.txt', lambda text: text.replace('github.com/madler/zlib/releases/download/v1.3.2', 'zlib.net')),
    (directory / 'receipts/qpdf-input-receipts-linux-x64/build-inputs.txt', lambda text: text + 'changed'),
    (directory / 'build-inputs.txt', lambda text: text.replace('bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16', '0' * 64)),
    (directory / 'build-inputs.txt', lambda text: text.replace('https://github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.gz', 'https://zlib.net/zlib-1.3.2.tar.gz')),
    (directory / 'build-inputs.txt', lambda text: text.replace('https://', 'http://')),
    (directory / 'build-inputs.txt', lambda text: text + text.splitlines(keepends=True)[0]),
    (directory / 'compile-qpdf.sh', lambda text: text.replace('bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16', '0' * 64)),
    (directory / 'targets.txt', lambda text: text + 'linux-new new-triple source hash\n'),
    (directory / 'receipts/qpdf-input-receipts-linux-x64/runtime-extra-sha256.txt', lambda text: text.replace('libgcc_eh.a', 'missing.a')),
]
for path, mutate in mutations:
    saved = path.read_text()
    path.write_text(mutate(saved))
    check(approved, False, f'mutated {path.name}')
    path.write_text(saved)

source_mutations = [
    (directory / 'runtime-sources.txt', lambda text: text + text.splitlines(keepends=True)[0], 'invalid source manifest'),
    (directory / 'runtime-sources.txt', lambda text: text[:-1], 'invalid source manifest'),
    (directory / 'runtime-sources.txt', lambda text: text.replace('\n', '\r\n'), 'invalid source manifest'),
    (directory / 'runtime-sources.txt', lambda text: text.replace(' ', '  ', 1), 'invalid source manifest'),
    (directory / 'runtime-sources.txt', lambda text: text.replace('aports-gcc.tar.gz', 'aports-gcc.tar'), 'aports source archive must end in .tar.gz: aports-gcc.tar'),
    (directory / 'runtime-sources.txt', lambda text: text.replace('source-2', 'source-0'), 'duplicate source filename: source-0'),
    (directory / 'runtime-sources.txt', lambda text: text.replace(f'{"1" * 40}:{"2" * 40}', 'bad-revision'), 'invalid aports source URL:'),
    (directory / 'runtime-sources.txt', lambda text: text.replace(f'#{"1" * 40}:{"2" * 40}', ''), 'invalid aports source URL:'),
    (directory / 'notices/gnu-runtime-source-pointers.txt', lambda text: text.replace('1' * 40, 'a' * 40), 'runtime-sources.txt and gnu-runtime-source-pointers.txt mismatch'),
    (directory / 'notices/gnu-runtime-source-pointers.txt', lambda text: text.replace('2' * 40, 'b' * 40), 'runtime-sources.txt and gnu-runtime-source-pointers.txt mismatch'),
    (directory / 'notices/gnu-runtime-source-pointers.txt', lambda text: re.sub(r'Uncompressed tar SHA-256: ([0-9a-f]{64})\nInformational \.tar\.gz SHA-256 \(gzip -n -9\): [0-9a-f]{64}', r'Uncompressed tar SHA-256: \1\nInformational .tar.gz SHA-256 (gzip -n -9): \1', text, count=1), 'informational .tar.gz digest matches uncompressed tar digest'),
    (directory / 'notices/gnu-runtime-source-pointers.txt', lambda text: text.replace('Uncompressed tar SHA-256:', 'Re-archive SHA-256:'), 'legacy Re-archive SHA-256 entry'),
    (directory / 'notices/gnu-runtime-source-pointers.txt', lambda text: text.replace('Commit:', 'BadKey:'), 'malformed pointer block in gnu-runtime-source-pointers.txt'),
]
for path, mutate, expected_err in source_mutations:
    saved = path.read_bytes()
    path.write_bytes(mutate(saved.decode()).encode())
    check(approved, False, f'{path.name}: {expected_err}', expected_err=expected_err)
    path.write_bytes(saved)

pointers_file = directory / 'notices/gnu-runtime-source-pointers.txt'
saved_pointers = pointers_file.read_text()
pointers_file.unlink()
check(approved, False, 'missing gnu-runtime-source-pointers.txt')
pointers_file.write_text(saved_pointers)

# Verify gzip-implementation independence and fail-closed rejection on aports archives.
aports_archive = directory / 'sources/aports-gcc.tar.gz'
saved_aports_gz = aports_archive.read_bytes()
gcc_tar = (directory / 'aports-gcc.tar').read_bytes()
gz_level1 = gzip.compress(gcc_tar, compresslevel=1, mtime=0)
gz_level9 = gzip.compress(gcc_tar, compresslevel=9, mtime=0)
if hashlib.sha256(gz_level1).hexdigest() == hashlib.sha256(gz_level9).hexdigest():
    sys.exit('expected distinct .tar.gz SHA-256 digests for compresslevel=1 vs compresslevel=9')
aports_archive.write_bytes(gz_level1)
check(approved, True, 'aports archive compressed at level 1 accepted')
aports_archive.write_bytes(gz_level9)
check(approved, True, 'aports archive compressed at level 9 accepted')

mutated_tar = bytearray(gcc_tar)
mutated_tar[512] ^= 0x01
deflate_corrupted = bytearray(gz_level9)
deflate_corrupted[14] ^= 0xff
for label, bad_bytes, expected_err in (
    ('mutated uncompressed tar byte re-gzipped', gzip.compress(bytes(mutated_tar), compresslevel=9, mtime=0), 'source hash mismatch: aports-gcc.tar.gz'),
    ('raw uncompressed tar masquerading as .tar.gz', gcc_tar, None),
    ('truncated gzip stream', gz_level9[:-8], None),
    ('deflate-corrupted gzip stream', bytes(deflate_corrupted), None),
    ('trailing non-gzip garbage', gz_level9 + b'trailing-garbage', None),
    ('valid gzip wrapping non-tar bytes', gzip.compress(b'not a tar archive', compresslevel=9, mtime=0), None),
    ('valid gzip wrapping truncated tar archive', gzip.compress(gcc_tar[:600], compresslevel=9, mtime=0), None),
    ('valid gzip wrapping empty tar archive', gzip.compress(b'\x00' * 1024, compresslevel=9, mtime=0), 'empty aports tar archive: aports-gcc.tar.gz'),
):
    aports_archive.write_bytes(bad_bytes)
    check(approved, False, label, expected_err=expected_err)
aports_archive.unlink()
aports_archive.symlink_to(directory / 'sources/aports-musl.tar.gz')
check(approved, False, 'symlinked aports archive', expected_err='missing or invalid source file: aports-gcc.tar.gz')
aports_archive.unlink()
aports_archive.write_bytes(saved_aports_gz)

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
    for notice in qpdf-LICENSE.txt gnu-LGPL-2.1.txt gnu-runtime-source-offer.txt gnu-runtime-source-pointers.txt; do
        mv "$tmp/pack/notices/$notice" "$tmp/notice"
        if bash "$script" verify "$tmp/pack" >/dev/null 2>&1; then
            echo "missing notice $notice was accepted" >&2; exit 1
        fi
        mv "$tmp/notice" "$tmp/pack/notices/$notice"
    done
fi
echo 'qpdf pack checks passed'
