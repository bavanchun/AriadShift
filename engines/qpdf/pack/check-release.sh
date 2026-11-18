#!/usr/bin/env bash
# Fail closed unless the exact component review explicitly records approval.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
python3 - "$here/licence-review.toml" "$here/runtime-sources.txt" "${1:-}" "${2:-}" "${3:-}" "$here/targets.txt" "$here/build-inputs.txt" "$here/compile-qpdf.sh" "$here/notices/gnu-runtime-source-pointers.txt" <<'PY'
import gzip, hashlib, io, pathlib, re, subprocess, sys, tarfile, tomllib, zlib

MAX_APORTS_TAR_BYTES = 256 * 1024 * 1024
APORTS_PREFIX = 'https://github.com/alpinelinux/aports.git'


def read_bounded_gzip(path: pathlib.Path, max_bytes: int, name: str) -> bytes:
    """Decompress a gzip file fail-closed without exceeding max_bytes."""
    with gzip.open(path, 'rb') as gz:
        chunks = []
        total = 0
        while True:
            chunk = gz.read(min(1024 * 1024, max_bytes + 1 - total))
            if not chunk:
                return b''.join(chunks)
            total += len(chunk)
            if total > max_bytes:
                raise ValueError(f'aports archive exceeds max uncompressed size: {name}')
            chunks.append(chunk)


def verify_aports_archive(path: pathlib.Path, name: str, digest: str) -> None:
    """Verify a gzipped Alpine aports tar archive against its uncompressed tar SHA-256."""
    payload = read_bounded_gzip(path, MAX_APORTS_TAR_BYTES, name)
    with tarfile.open(fileobj=io.BytesIO(payload), mode='r:') as tf:
        member_count = sum(1 for _ in tf)
    if member_count == 0:
        raise ValueError(f'empty aports tar archive: {name}')
    if hashlib.sha256(payload).hexdigest() != digest:
        raise ValueError(f'source hash mismatch: {name}')


def load_source_manifest(source_manifest: bytes) -> list[tuple[str, str, str]]:
    """Parse and strictly validate the 5-line runtime-sources.txt manifest."""
    if b'\r' in source_manifest:
        raise ValueError('invalid source manifest')
    source_lines = source_manifest.decode().splitlines()
    entries = [line.split() for line in source_lines]
    if (not source_manifest.endswith(b'\n') or len(entries) != 5 or
            any(len(entry) != 3 or line != ' '.join(entry) for line, entry in zip(source_lines, entries))):
        raise ValueError('invalid source manifest')
    seen_names: set[str] = set()
    seen_urls: set[str] = set()
    validated: list[tuple[str, str, str]] = []
    for digest, name, url in entries:
        if not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError(f'invalid source hash: {name}')
        if pathlib.PurePosixPath(name).name != name or name in ('.', '..') or '\\' in name:
            raise ValueError(f'invalid source filename: {name}')
        if name in seen_names:
            raise ValueError(f'duplicate source filename: {name}')
        if not url.startswith('https://'):
            raise ValueError(f'invalid source URL: {url}')
        if url.startswith(APORTS_PREFIX):
            if not name.endswith('.tar.gz'):
                raise ValueError(f'aports source archive must end in .tar.gz: {name}')
            if not re.fullmatch(rf'{re.escape(APORTS_PREFIX)}#[0-9a-f]{{40}}:[0-9a-f]{{40}}', url):
                raise ValueError(f'invalid aports source URL: {url}')
        if url in seen_urls:
            raise ValueError(f'duplicate source URL: {url}')
        seen_names.add(name)
        seen_urls.add(url)
        validated.append((digest, name, url))
    return validated


def load_source_pointers(pointers_text: str) -> dict[str, str]:
    """Parse gnu-runtime-source-pointers.txt and return authoritative URL -> SHA-256 map."""
    if re.search(r'^Re-archive SHA-256:', pointers_text, re.MULTILINE):
        raise ValueError('legacy Re-archive SHA-256 entry in gnu-runtime-source-pointers.txt')
    direct_matches = re.findall(
        r'^(https://[^\s]+)\nSHA-256: ([0-9a-f]{64})$',
        pointers_text,
        re.MULTILINE,
    )
    aports_matches = re.findall(
        r'^(https://github\.com/alpinelinux/aports\.git)\n'
        r'Commit: ([0-9a-f]{40})\n'
        r'Tree: ([0-9a-f]{40})\n'
        r'Uncompressed tar SHA-256: ([0-9a-f]{64})\n'
        r'Informational \.tar\.gz SHA-256 \(gzip -n -9\): ([0-9a-f]{64})$',
        pointers_text,
        re.MULTILINE,
    )
    if (len(re.findall(r'^SHA-256:', pointers_text, re.MULTILINE)) != len(direct_matches) or
            len(re.findall(r'^Commit:', pointers_text, re.MULTILINE)) != len(aports_matches) or
            len(re.findall(r'^Tree:', pointers_text, re.MULTILINE)) != len(aports_matches) or
            len(re.findall(r'^Uncompressed tar SHA-256:', pointers_text, re.MULTILINE)) != len(aports_matches) or
            len(re.findall(r'^Informational \.tar\.gz SHA-256', pointers_text, re.MULTILINE)) != len(aports_matches)):
        raise ValueError('malformed pointer block in gnu-runtime-source-pointers.txt')
    pointer_map: dict[str, str] = {}
    for url, digest in direct_matches:
        if url.startswith(APORTS_PREFIX) or url in pointer_map:
            raise ValueError(f'invalid or duplicate source pointer URL: {url}')
        pointer_map[url] = digest
    for repo_url, commit, tree, tar_digest, gz_digest in aports_matches:
        if gz_digest == tar_digest:
            raise ValueError('informational .tar.gz digest matches uncompressed tar digest')
        aports_url = f'{repo_url}#{commit}:{tree}'
        if aports_url in pointer_map:
            raise ValueError(f'duplicate aports pointer: {aports_url}')
        pointer_map[aports_url] = tar_digest
    return pointer_map


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
    if sys.argv[3] == '--review-only':
        sys.exit(0)
    source_manifest = pathlib.Path(sys.argv[2]).read_bytes()
    build_inputs_manifest = pathlib.Path(sys.argv[7]).read_bytes()
    compile_text = pathlib.Path(sys.argv[8]).read_text()
    receipts, sources, packdir = map(pathlib.Path, sys.argv[3:6])
    if not all(sys.argv[3:6]):
        raise ValueError('receipt, source and pack directories are required')
    source_entries = load_source_manifest(source_manifest)
    pointer_map = load_source_pointers(pathlib.Path(sys.argv[9]).read_text())
    if {url: digest for digest, _, url in source_entries} != pointer_map:
        raise ValueError('runtime-sources.txt and gnu-runtime-source-pointers.txt mismatch')
    for digest, name, url in source_entries:
        source_path = sources / name
        if source_path.is_symlink() or not source_path.is_file():
            raise ValueError(f'missing or invalid source file: {name}')
        if url.startswith(APORTS_PREFIX):
            verify_aports_archive(source_path, name, digest)
        elif hashlib.sha256(source_path.read_bytes()).hexdigest() != digest:
            raise ValueError(f'source hash mismatch: {name}')
    compile_hashes = {}
    for var in ('qpdf', 'jpeg', 'zlib'):
        matches = re.findall(rf'^verify ([0-9a-f]{{64}}) "\${var}"$', compile_text, re.MULTILINE)
        if len(matches) != 1:
            raise ValueError(f'missing verify hash in compile-qpdf.sh: {var}')
        compile_hashes[var] = matches[0]
    build_lines = build_inputs_manifest.decode().splitlines()
    build_entries = [line.split() for line in build_lines]
    if (not build_inputs_manifest.endswith(b'\n') or len(build_entries) != 3 or
            any(len(entry) != 3 or line != ' '.join(entry) for line, entry in zip(build_lines, build_entries))):
        raise ValueError('invalid build inputs manifest')
    build_map = {}
    for digest, name, url in build_entries:
        if len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):
            raise ValueError(f'invalid build input hash: {name}')
        if pathlib.Path(name).name != name or name in ('.', '..'):
            raise ValueError(f'invalid build input filename: {name}')
        if not url.startswith('https://') or not url.endswith(f'/{name}'):
            raise ValueError(f'invalid build input URL: {url}')
        if name in build_map:
            raise ValueError(f'duplicate build input filename: {name}')
        build_map[name] = (digest, url)
    expected_inputs = {
        'qpdf-12.4.2.tar.gz': (
            compile_hashes['qpdf'],
            'https://github.com/qpdf/qpdf/releases/download/v12.4.2/qpdf-12.4.2.tar.gz',
        ),
        'libjpeg-turbo-3.2.1.tar.gz': (
            compile_hashes['jpeg'],
            expected['source'],
        ),
        'zlib-1.3.2.tar.gz': (
            compile_hashes['zlib'],
            'https://github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.gz',
        ),
    }
    if expected_inputs['libjpeg-turbo-3.2.1.tar.gz'][0] != expected['sha256']:
        raise ValueError('libjpeg-turbo hash mismatch between review and compile script')
    if list(build_map) != list(expected_inputs) or build_map != expected_inputs:
        raise ValueError('build inputs manifest mismatch')
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
    target_rows = [line.split() for line in pathlib.Path(sys.argv[6]).read_text().splitlines() if line and not line.startswith('#')]
    if any(len(row) != 4 for row in target_rows):
        raise ValueError('invalid targets manifest')
    triples = {row[0]: row[1] for row in target_rows}
    if {(row[2], row[3]) for row in target_rows} != {('qpdf-12.4.2.tar.gz', compile_hashes['qpdf'])}:
        raise ValueError('targets.txt qpdf asset or hash mismatch')
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
        if (receipt / 'build-inputs.txt').read_bytes() != build_inputs_manifest:
            raise ValueError(f'build input receipt mismatch: {platform}')
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
except (OSError, ValueError, KeyError, EOFError, zlib.error, tarfile.TarError, subprocess.CalledProcessError, tomllib.TOMLDecodeError) as exc:
    sys.exit(f'qpdf release refused: {exc}')
PY
