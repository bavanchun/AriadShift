"""Build and packaging script for the sign engine pack."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request
from dataclasses import asdict, dataclass
from pathlib import Path

if sys.version_info < (3, 14):
    sys.exit("build needs Python 3.14 (tarfile writes zstd archives there).")

PACK_NAME = "sign"
DESCRIPTION = "Digital signatures: PAdES signing, verification, timestamping, and visual appearance"
REQUIRES_ASHIFT = ">=0.2.0, <2.0.0"
ENGINE_PROTOCOL = "ariad-engine/2"
VERSION = "0.1.0"
NOTICES = [
    "notices/SIGN-PACK.txt",
    "notices/AriadShift-LICENSE",
    "notices/AriadShift-NOTICE",
    "notices/Python-LICENSE.txt",
    "notices/OpenSSL-LICENSE.txt",
    "notices/libxml2-libxslt-LICENSES.txt",
    "notices/libyaml-LICENSE.txt",
]

PACK_DIR = Path(__file__).resolve().parent
SIGN_ENGINE_ROOT = PACK_DIR.parent
REPOSITORY = SIGN_ENGINE_ROOT.parents[1]

PLATFORMS = {
    "x86_64-unknown-linux-gnu": "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu": "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin": "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc": "x86_64-pc-windows-msvc",
}


@dataclass
class TargetBuild:
    triple: str
    version: str
    license: str
    archive: str
    sha256: str
    size: int


def is_windows(target: str) -> bool:
    return "windows" in target


def is_macos(target: str) -> bool:
    return "darwin" in target or "apple" in target


def can_run_target(target: str) -> bool:
    sys_name = platform.system().lower()
    mach = platform.machine().lower()
    if is_windows(target):
        return sys_name == "windows"
    if is_macos(target):
        return sys_name == "darwin"
    if sys_name != "linux":
        return False
    if "aarch64" in target or "arm64" in target:
        return mach in ("aarch64", "arm64")
    if "x86_64" in target:
        return mach in ("x86_64", "amd64")
    return False


def python_executable(runtime: Path, target: str) -> Path:
    if is_windows(target):
        for candidate in (runtime / "python.exe", runtime / "Scripts" / "python.exe"):
            if candidate.is_file():
                return candidate
        return runtime / "python.exe"
    return runtime / "bin" / "python3"


def site_packages(runtime: Path, target: str) -> Path:
    if is_windows(target):
        return runtime / "Lib" / "site-packages"
    for candidate in runtime.glob("lib/python*/site-packages"):
        if candidate.is_dir():
            return candidate
    return runtime / "lib" / "python3.14" / "site-packages"


def load_pins() -> dict:
    pins_file = PACK_DIR / "pins.toml"
    return tomllib.loads(pins_file.read_text(encoding="utf-8"))


def download_file(url: str, expected_sha256: str, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_file():
        h = hashlib.sha256()
        with dest.open("rb") as f:
            while chunk := f.read(65536):
                h.update(chunk)
        if h.hexdigest() == expected_sha256:
            return

    print(f"Downloading {url} ...", flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "AriadShift-Pack-Builder/0.1.0"})
    with urllib.request.urlopen(req, timeout=120) as resp, dest.open("wb") as out:
        h = hashlib.sha256()
        while chunk := resp.read(65536):
            h.update(chunk)
            out.write(chunk)
    actual_hash = h.hexdigest()
    if actual_hash != expected_sha256:
        dest.unlink(missing_ok=True)
        raise RuntimeError(f"Hash mismatch for {url}: expected {expected_sha256}, got {actual_hash}")


def archive_directory(root: Path, members: list[str], destination: Path) -> tuple[str, int]:
    """Writes a reproducible tar.zst of members and returns its sha256 and byte size."""
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))

    def normalize(info: tarfile.TarInfo) -> tarfile.TarInfo:
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        info.mtime = epoch
        if info.isfile():
            info.mode = 0o755 if (info.mode & 0o111 or info.name.endswith((".exe", ".dll", ".so", ".dylib"))) else 0o644
        elif info.isdir():
            info.mode = 0o755
        return info

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(destination, "w:zst", level=10) as tar:
        for member in members:
            base = root / member
            if not base.exists():
                continue
            paths = [base, *sorted(base.rglob("*"))]
            for path in paths:
                tar.add(
                    path,
                    arcname=path.relative_to(root).as_posix(),
                    recursive=False,
                    filter=normalize,
                )

    h = hashlib.sha256()
    with destination.open("rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest(), destination.stat().st_size


def archive_executables(archive_path: Path) -> list[str]:
    executables: set[str] = set()
    with tarfile.open(archive_path, "r:zst") as tar:
        for info in tar.getmembers():
            if not info.isfile():
                continue
            name = info.name.removeprefix("./")
            if name.startswith("notices/"):
                continue
            if (info.mode & 0o111) or name.lower().endswith((".exe", ".bat", ".cmd")):
                executables.add(name)
    return sorted(executables)


def entrypoint_for_target(target: str) -> list[str]:
    if is_windows(target):
        return ["python-env/python.exe", "-I", "-m", "ariad_sign"]
    return ["python-env/bin/python", "-I", "-m", "ariad_sign"]


def build_pack_target(target: str, out_dir: Path, ashift_bin: Path | None = None) -> TargetBuild:
    pins = load_pins()
    target_info = pins["python"]["targets"].get(target)
    if not target_info:
        raise ValueError(f"Target '{target}' not defined in pins.toml")

    cache_dir = Path(os.environ.get("ASHIFT_PACK_CACHE", Path.home() / ".cache" / "ariadshift-pack-cache"))
    archive_name = Path(target_info["url"]).name
    archive_path = cache_dir / archive_name
    download_file(target_info["url"], target_info["sha256"], archive_path)

    with tempfile.TemporaryDirectory(prefix=f"sign-pack-{target}-") as tmp:
        stage = Path(tmp) / "staging"
        stage.mkdir(parents=True, exist_ok=True)
        py_env = stage / "python-env"

        # 1. Extract standalone python
        print(f"Extracting {archive_path.name} ...")
        shutil.unpack_archive(archive_path, py_env)
        # python-build-standalone unpacks into a top-level 'python' dir
        inner_python = py_env / "python"
        if inner_python.is_dir():
            for child in list(inner_python.iterdir()):
                shutil.move(str(child), str(py_env))
            inner_python.rmdir()

        site = site_packages(py_env, target)
        site.mkdir(parents=True, exist_ok=True)

        # 2. Export locked requirements
        reqs_file = stage / "requirements.txt"
        cmd_export = [
            "uv", "export",
            "--project", str(SIGN_ENGINE_ROOT),
            "--frozen",
            "--no-dev",
            "--no-emit-project",
            "--no-header",
            "--format", "requirements.txt",
            "--hashes",
            "--output-file", str(reqs_file),
        ]
        subprocess.run(cmd_export, cwd=REPOSITORY, check=True)

        # 3. Install wheels with hashes into standalone site-packages
        platform_args = ["--python-platform", PLATFORMS[target], "--python-version", "3.14"]
        cmd_install = [
            "uv", "pip", "install",
            "--target", str(site),
            *platform_args,
            "--require-hashes",
            "--no-deps",
            "--only-binary", ":all:",
            "--link-mode", "copy",
            "--requirements", str(reqs_file),
        ]
        print(f"Installing pinned wheels for {target} ...")
        subprocess.run(cmd_install, cwd=REPOSITORY, check=True)

        # 4. Build ariad-sign wheel and install
        dist_dir = stage / "dist"
        cmd_build = [
            "uv", "build",
            "--project", str(SIGN_ENGINE_ROOT),
            "--wheel",
            "--out-dir", str(dist_dir),
        ]
        subprocess.run(cmd_build, cwd=REPOSITORY, check=True)
        whl = next(dist_dir.glob("ariad_sign-*.whl"))
        subprocess.run(
            ["uv", "pip", "install", "--target", str(site), *platform_args, "--no-deps", "--link-mode", "copy", str(whl)],
            cwd=REPOSITORY,
            check=True,
        )

        # 5. Run license gate on site-packages
        print(f"Auditing license gate on {site} ...")
        sys.path.insert(0, str(PACK_DIR))
        import licenses
        import importlib.metadata as md
        findings = licenses.check_environment(md.distributions(path=[str(site)]), site_root=site)
        if findings:
            for f in findings:
                print(f"License finding: {f.distribution} ({f.license}): {f.reason}", file=sys.stderr)
            raise RuntimeError("License gate failed on installed sign pack wheels")

        # 6. Notices
        notices_dir = stage / "notices"
        notices_dir.mkdir(parents=True, exist_ok=True)
        pack_notices = PACK_DIR / "notices"
        if pack_notices.is_dir():
            for notice_file in sorted(pack_notices.iterdir()):
                if notice_file.is_file():
                    shutil.copy2(notice_file, notices_dir / notice_file.name)

        repo_lic = REPOSITORY / "LICENSE"
        if repo_lic.is_file():
            shutil.copy2(repo_lic, notices_dir / "AriadShift-LICENSE")
        repo_notice = REPOSITORY / "NOTICE"
        if repo_notice.is_file():
            shutil.copy2(repo_notice, notices_dir / "AriadShift-NOTICE")
        for candidate in (py_env / "lib/python3.14/LICENSE.txt", py_env / "LICENSE.txt"):
            if candidate.is_file():
                shutil.copy2(candidate, notices_dir / "Python-LICENSE.txt")
                break

        packages_notices_dir = notices_dir / "packages"
        packages_notices_dir.mkdir(parents=True, exist_ok=True)
        for dist in md.distributions(path=[str(site)]):
            dname = dist.metadata.get("Name", "")
            if not dname or dname == "ariad-sign":
                continue
            dist_notices = packages_notices_dir / dname
            for item in dist.files or []:
                s_item = str(item)
                if ".dist-info" in s_item:
                    p = Path(s_item)
                    if any(part.upper().startswith(("LICEN", "COPYING", "NOTICE")) for part in p.parts):
                        src_file = site / s_item
                        if src_file.is_file() and src_file.stat().st_size > 0:
                            dist_notices.mkdir(parents=True, exist_ok=True)
                            shutil.copy2(src_file, dist_notices / p.name)

        # 7. Create tar.zst archive
        out_dir.mkdir(parents=True, exist_ok=True)
        archive_name = f"sign-{VERSION}-{target}-engine.tar.zst"
        dest_archive = out_dir / archive_name
        print(f"Creating component archive {archive_name} ...")
        sha256, size = archive_directory(stage, ["python-env", "notices"], dest_archive)

        record = TargetBuild(
            triple=target,
            version=VERSION,
            license="MIT AND Apache-2.0 AND BSD-3-Clause AND BSD-2-Clause AND PSF-2.0 AND MPL-2.0 AND MIT-0",
            archive=archive_name,
            sha256=sha256,
            size=size,
        )
        (out_dir / f"build-{target}.json").write_text(
            json.dumps(asdict(record), indent=2) + "\n",
            encoding="utf-8",
        )

        write_single_target_manifest(ashift_bin, out_dir, record)
        print(f"Target {target} built successfully: {dest_archive.name} ({size} bytes, sha256: {sha256})")
        return record


def write_single_target_manifest(ashift_bin: Path | None, out_dir: Path, record: TargetBuild) -> None:
    manifest_path = out_dir / "pack.toml"
    archive_file = out_dir / record.archive

    # If ashift binary is available, use ashift __pack manifest
    if ashift_bin and ashift_bin.is_file():
        entrypoint_args = []
        for arg in entrypoint_for_target(record.triple):
            entrypoint_args.extend(["--entrypoint", arg])

        cmd = [
            str(ashift_bin),
            "__pack", "manifest",
            "--name", PACK_NAME,
            "--version", record.version,
            "--target", record.triple,
            "--engine-protocol", ENGINE_PROTOCOL,
            *entrypoint_args,
            "--output", str(manifest_path),
            f"engine={archive_file}",
        ]
        res = subprocess.run(cmd, capture_output=True, text=True)
        if res.returncode == 0 and manifest_path.is_file():
            # Add general header fields
            manifest_text = manifest_path.read_text(encoding="utf-8")
            if "description =" not in manifest_text:
                notices_list = ", ".join(f'"{n}"' for n in NOTICES)
                header = (
                    f'schema = "ariad-pack/1"\n'
                    f'name = "{PACK_NAME}"\n'
                    f'version = "{record.version}"\n'
                    f'description = "{DESCRIPTION}"\n'
                    f'license = "{record.license}"\n'
                    f'requires_ashift = "{REQUIRES_ASHIFT}"\n'
                    f'engine_protocol = "{ENGINE_PROTOCOL}"\n'
                    f'notices = [{notices_list}]\n\n'
                )
                manifest_path.write_text(header + manifest_text, encoding="utf-8")
            return

    # Fallback to direct Python manifest generation with real computed fields
    execs = archive_executables(archive_file)
    execs_str = ", ".join(f'"{e}"' for e in execs)
    entry_str = ", ".join(f'"{e}"' for e in entrypoint_for_target(record.triple))
    
    # Calculate unpacked size and entry count
    entries_count = 0
    unpacked_size = 0
    with tarfile.open(archive_file, "r:zst") as tar:
        for m in tar.getmembers():
            entries_count += 1
            unpacked_size += m.size

    notices_list = ", ".join(f'"{n}"' for n in NOTICES)
    manifest_content = f'''schema = "ariad-pack/1"
name = "{PACK_NAME}"
version = "{record.version}"
description = "{DESCRIPTION}"
license = "{record.license}"
requires_ashift = "{REQUIRES_ASHIFT}"
engine_protocol = "{ENGINE_PROTOCOL}"
notices = [{notices_list}]

[[targets]]
triple = "{record.triple}"
entrypoint = [{entry_str}]
executables = [{execs_str}]

[[targets.components]]
name = "engine"
url = "{record.archive}"
sha256 = "{record.sha256}"
size = {record.size}
unpack = "tar.zst"
dest = "."
unpacked_size = {unpacked_size}
entries = {entries_count}
'''
    manifest_path.write_text(manifest_content, encoding="utf-8")


def merge_manifests(out_dir: Path) -> Path:
    manifest_path = out_dir / "pack.toml"
    archives = sorted(out_dir.glob("sign-*-engine.tar.zst"))
    if not archives:
        raise ValueError(f"No sign engine archives found in {out_dir}")

    notices_list = ", ".join(f'"{n}"' for n in NOTICES)
    license_str = "MIT AND Apache-2.0 AND BSD-3-Clause AND BSD-2-Clause AND PSF-2.0 AND MPL-2.0 AND MIT-0"
    lines = [
        'schema = "ariad-pack/1"',
        f'name = "{PACK_NAME}"',
        f'version = "{VERSION}"',
        f'description = "{DESCRIPTION}"',
        f'license = "{license_str}"',
        f'requires_ashift = "{REQUIRES_ASHIFT}"',
        f'engine_protocol = "{ENGINE_PROTOCOL}"',
        f'notices = [{notices_list}]',
        "",
    ]
    for archive in archives:
        m = re.search(r"sign-[^-]+-(.+)-engine\.tar\.zst", archive.name)
        if not m:
            continue
        triple = m.group(1)
        execs = archive_executables(archive)
        execs_str = ", ".join(f'"{e}"' for e in execs)
        entry_str = ", ".join(f'"{e}"' for e in entrypoint_for_target(triple))

        entries_count = 0
        unpacked_size = 0
        with tarfile.open(archive, "r:zst") as tar:
            for member in tar.getmembers():
                entries_count += 1
                unpacked_size += member.size
        size = archive.stat().st_size
        sha256 = hashlib.sha256(archive.read_bytes()).hexdigest()

        lines.extend([
            "[[targets]]",
            f'triple = "{triple}"',
            f"entrypoint = [{entry_str}]",
            f"executables = [{execs_str}]",
            "",
            "[[targets.components]]",
            'name = "engine"',
            f'url = "{archive.name}"',
            f'sha256 = "{sha256}"',
            f"size = {size}",
            'unpack = "tar.zst"',
            'dest = "."',
            f"unpacked_size = {unpacked_size}",
            f"entries = {entries_count}",
            "",
        ])
    manifest_content = "\n".join(lines)
    manifest_path.write_text(manifest_content, encoding="utf-8")
    print(f"Merged manifest written to {manifest_path} for {len(archives)} target(s)")
    return manifest_path


def run_conformance(pack_dir: Path, target: str | None = None) -> bool:
    manifest_file = pack_dir / "pack.toml"
    if not manifest_file.is_file():
        print(f"Error: manifest missing at {manifest_file}", file=sys.stderr)
        return False

    manifest_data = tomllib.loads(manifest_file.read_text(encoding="utf-8"))
    targets = manifest_data.get("targets", [])
    if not targets:
        print("Error: no targets defined in manifest", file=sys.stderr)
        return False

    current_target = target or "x86_64-unknown-linux-gnu"
    target_entry = next((t for t in targets if t.get("triple") == current_target), None)
    if not target_entry:
        target_entry = targets[0]

    target_triple = target_entry["triple"]
    if not can_run_target(target_triple):
        print(f"Skipping binary execution check for {target_triple} on host {platform.system()} {platform.machine()}")
        print("Sign pack conformance structure check succeeded!")
        return True

    comp = target_entry["components"][0]
    archive_path = pack_dir / comp["url"]
    if not archive_path.is_file():
        print(f"Error: component archive {archive_path} not found", file=sys.stderr)
        return False

    with tempfile.TemporaryDirectory(prefix="sign-conformance-") as tmp:
        work = Path(tmp)
        with tarfile.open(archive_path, "r:zst") as tar:
            tar.extractall(work, filter="data")

        py = python_executable(work / "python-env", target_entry["triple"])
        if not py.is_file():
            print(f"Error: interpreter not found at {py}", file=sys.stderr)
            return False

        # Run describe probe
        req = json.dumps({"protocol": "ariad-engine/2", "job": "conf-desc", "op": "describe"}).encode() + b"\n"
        proc = subprocess.run([str(py), "-I", "-m", "ariad_sign"], input=req, capture_output=True, check=False)
        if proc.returncode != 0:
            print(f"Describe failed with exit code {proc.returncode}:\n{proc.stderr.decode()}", file=sys.stderr)
            return False

        events = [json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
        if not any(e.get("type") == "capabilities" for e in events):
            print("Describe did not emit capabilities", file=sys.stderr)
            return False
        if not any(e.get("type") == "result" and e.get("ok") is True for e in events):
            print("Describe did not emit ok result", file=sys.stderr)
            return False

        print("Sign pack conformance check succeeded!")
        return True


def main() -> int:
    parser = argparse.ArgumentParser(description="Sign engine pack builder")
    subparsers = parser.add_subparsers(dest="command", required=True)

    build_p = subparsers.add_parser("build", help="Build sign pack for target")
    build_p.add_argument("--target", type=str, default="x86_64-unknown-linux-gnu", help="Target triple")
    build_p.add_argument("--out", type=Path, required=True, help="Output pack directory")
    build_p.add_argument("--ashift", type=Path, default=None, help="Path to ashift binary")

    conf_p = subparsers.add_parser("conformance", help="Verify pack conformance")
    conf_p.add_argument("--out", type=Path, required=True, help="Pack directory to verify")
    conf_p.add_argument("--target", type=str, default=None, help="Target triple to verify")

    merge_p = subparsers.add_parser("merge", help="Merge multi-target pack manifest")
    merge_p.add_argument("--out", type=Path, required=True, help="Pack directory with target archives")

    args = parser.parse_args()
    if args.command == "build":
        build_pack_target(args.target, args.out, args.ashift)
        return 0
    elif args.command == "conformance":
        return 0 if run_conformance(args.out, args.target) else 1
    elif args.command == "merge":
        merge_manifests(args.out)
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
