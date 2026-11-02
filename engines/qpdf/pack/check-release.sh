#!/usr/bin/env bash
# Fail closed unless the exact component review explicitly records approval.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
python3 - "$here/licence-review.toml" "$here/runtime-sources.txt" "${1:-}" "${2:-}" "${3:-}" "$here/targets.txt" <<'PY'
import pathlib, subprocess, sys, tomllib
p = pathlib.Path(sys.argv[1])
try:
    review = tomllib.loads(p.read_text())
    expected = {
        'pack': 'qpdf', 'component': 'libjpeg-turbo', 'version': '3.2.1',
        'modified': False, 'license': 'IJG AND BSD-3-Clause AND Zlib',
        'source': 'https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/3.2.1/libjpeg-turbo-3.2.1.tar.gz',
        'sha256': 'c27ad72a68da151177929aa969ae773a2da5ac2af9ce946079314fdac8cece7f',
        'pending_owner_approval': False,
        'runtime_review_complete': True,
    }
    if any(isinstance(value, bool) and not isinstance(review.get(key), bool)
           for key, value in expected.items()):
        raise ValueError('review flags must be booleans')
    if review != expected:
        raise ValueError('review is pending or does not match the approved component')
    import hashlib
    source_manifest = pathlib.Path(sys.argv[2]).read_bytes()
    receipts, sources, packdir = map(pathlib.Path, sys.argv[3:6])
    if not all(sys.argv[3:6]):
        raise ValueError('receipt, source and pack directories are required')
    entries = [line.split() for line in source_manifest.decode().splitlines()]
    if len(entries) != 5 or any(len(entry) != 3 for entry in entries):
        raise ValueError('invalid source manifest')
    for digest, name, _ in entries:
        if pathlib.Path(name).name != name or hashlib.sha256((sources / name).read_bytes()).hexdigest() != digest:
            raise ValueError(f'source hash mismatch: {name}')
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
    triples = {line.split()[0]: line.split()[1] for line in pathlib.Path(sys.argv[6]).read_text().splitlines() if line and not line.startswith('#')}
    linux_targets = {slug for slug in triples if slug.startswith('linux-')}
    if set(platforms) != linux_targets:
        raise ValueError('Linux target and reviewed platform sets differ')
    manifest = tomllib.loads((packdir / 'pack.toml').read_text())
    for platform, (image, gcc, stdcpp, libgcc) in platforms.items():
        receipt = receipts / f'qpdf-input-receipts-{platform}'
        targets = [t for t in manifest['targets'] if t['triple'] == triples[platform]]
        if len(targets) != 1:
            raise ValueError(f'pack target mismatch: {platform}')
        components = [c for c in targets[0]['components'] if c['name'] == 'engine']
        if len(components) != 1:
            raise ValueError(f'pack component mismatch: {platform}')
        component = components[0]
        archive = packdir / pathlib.Path(component['url']).name
        if hashlib.sha256(archive.read_bytes()).hexdigest() != component['sha256']:
            raise ValueError(f'pack archive hash mismatch: {platform}')
        binary = subprocess.run(['tar', '--zstd', '-xOf', str(archive), 'bin/qpdf'], check=True, capture_output=True).stdout
        if (receipt / 'qpdf-sha256.txt').read_text().split() != [hashlib.sha256(binary).hexdigest(), 'bin/qpdf']:
            raise ValueError(f'qpdf hash mismatch: {platform}')
        if (receipt / 'runtime-sources.txt').read_bytes() != source_manifest:
            raise ValueError(f'source receipt mismatch: {platform}')
        if not (receipt / 'image.txt').read_text().strip().endswith('@sha256:' + image):
            raise ValueError(f'image mismatch: {platform}')
        if not (receipt / 'image-id.txt').read_text().strip().startswith('sha256:'):
            raise ValueError(f'image ID missing: {platform}')
        if gcc not in (receipt / 'gcc-version.txt').read_text():
            raise ValueError(f'compiler mismatch: {platform}')
        if not (receipt / 'packages.txt').read_text().strip() or not (receipt / 'package-sha256.txt').read_text().strip():
            raise ValueError(f'package receipt missing: {platform}')
        package_hashes = {line.split()[0] for line in (receipt / 'package-sha256.txt').read_text().splitlines() if line.split()}
        if not set(apk_hashes.get(platform, [])).issubset(package_hashes):
            raise ValueError(f'package hash mismatch: {platform}')
        runtime = (receipt / 'runtime-sha256.txt').read_text().splitlines()
        if len(runtime) != 2 or runtime[0].split()[0] != libgcc or runtime[1].split()[0] != stdcpp:
            raise ValueError(f'runtime hash mismatch: {platform}')
        extra = (receipt / 'runtime-extra-sha256.txt').read_text().splitlines()
        records = [line.split() for line in extra]
        if any(len(parts) != 2 or len(parts[0]) != 64 or any(c not in '0123456789abcdef' for c in parts[0]) for parts in records):
            raise ValueError(f'invalid extra runtime receipt: {platform}')
        names = [pathlib.Path(parts[1]).name for parts in records]
        if len(names) != len(set(names)) or not {'libgcc_eh.a', 'crtbegin.o', 'crtend.o'}.issubset(names):
            raise ValueError(f'extra runtime receipt missing: {platform}')
except (OSError, ValueError, KeyError, subprocess.CalledProcessError, tomllib.TOMLDecodeError) as exc:
    sys.exit(f'qpdf release refused: {exc}')
PY
