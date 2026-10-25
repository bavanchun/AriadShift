"""Regression coverage for archive and Homebrew platform verification."""

import hashlib
from pathlib import Path
import re
import runpy
import tempfile
import unittest


MODULE = runpy.run_path(str(Path(__file__).with_name("verify-homebrew.py")))


class HomebrewVerificationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.targets = list(MODULE["HOMEBREW_TARGETS"]) + ["x86_64-pc-windows-msvc"]
        self.config = self.root / "dist-workspace.toml"
        self.config.write_text(
            '[dist]\nformula = "ashift"\ntargets = ['
            + ", ".join(f'"{target}"' for target in self.targets) + "]\n"
        )
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.0.0"\n')
        self.distrib = self.root / "distrib"
        self.distrib.mkdir()
        self.formula = self.distrib / "ashift.rb"
        blocks = {"mac": [], "linux": []}
        for target in self.targets:
            archive = self.archive(target)
            archive.write_bytes(f"Test archive for {target}".encode())
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_suffix(archive.suffix + ".sha256").write_text(f"{digest}  {archive.name}\n")
            if target in MODULE["HOMEBREW_TARGETS"]:
                os_name, cpu = MODULE["HOMEBREW_TARGETS"][target]
                blocks[os_name].append(
                    f"    if Hardware::CPU.{cpu}?\n"
                    f'      url "https://github.com/bavanchun/AriadShift/releases/download/v1.0.0/{archive.name}"\n'
                    f'      sha256 "{digest}"\n    end\n'
                )
        self.formula.write_text(
            'class Ashift < Formula\n  version "1.0.0"\n'
            + "".join(f"  if OS.{os_name}?\n{''.join(parts)}  end\n" for os_name, parts in blocks.items())
            + '  depends_on "pandoc"\n  def install\n    bin.install "ashift"\n  end\nend\n'
        )

    def archive(self, target):
        extension = "zip" if "windows" in target else "tar.xz"
        return self.distrib / f"ariad-cli-{target}.{extension}"

    def verify(self):
        MODULE["verify"](self.distrib, self.root)

    def test_all_published_targets(self):
        self.verify()

    def test_changed_archive_bytes_including_windows(self):
        for target in self.targets:
            with self.subTest(target=target):
                archive = self.archive(target)
                original = archive.read_bytes()
                archive.write_bytes(original + b"corrupted")
                with self.assertRaisesRegex(ValueError, "Archive checksum"):
                    self.verify()
                archive.write_bytes(original)

    def test_formula_hash_cannot_borrow_another_targets_digest(self):
        original = self.formula.read_text()
        hashes = re.findall(r'sha256 "([0-9a-f]+)"', original)
        self.formula.write_text(original.replace(hashes[0], hashes[1]))
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            self.verify()

    def test_wrong_version_or_platform_url(self):
        original = self.formula.read_text()
        for old, new in [('version "1.0.0"', 'version "0.9.0"'), ("/v1.0.0/", "/v0.9.0/"),
                         ("OS.mac?", "OS.linux?"), ("CPU.arm?", "CPU.intel?")]:
            with self.subTest(replacement=new):
                self.formula.write_text(original.replace(old, new, 1))
                with self.assertRaises(ValueError):
                    self.verify()

    def test_missing_archive_or_checksum(self):
        archive = self.archive(self.targets[-1])
        checksum = archive.with_suffix(archive.suffix + ".sha256")
        checksum.unlink()
        with self.assertRaises(FileNotFoundError):
            self.verify()
        archive.unlink()
        with self.assertRaises(FileNotFoundError):
            self.verify()

    def test_extra_url_cannot_hide_a_stale_formula(self):
        self.formula.write_text(self.formula.read_text() + '  url "https://example.com/stale.tar.xz"\n')
        with self.assertRaisesRegex(ValueError, "exactly the published"):
            self.verify()

    def test_new_unix_target_requires_platform_verification(self):
        self.config.write_text(self.config.read_text().replace(
            '"aarch64-apple-darwin"', '"riscv64-unknown-linux-musl"', 1,
        ))
        archive = self.archive("riscv64-unknown-linux-musl")
        archive.write_bytes(b"Test archive for an unsupported target")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_suffix(archive.suffix + ".sha256").write_text(digest)
        with self.assertRaisesRegex(ValueError, "Add Homebrew platform verification"):
            self.verify()

    def test_required_runtime_dependency_and_binary(self):
        original = self.formula.read_text()
        for line in ['  depends_on "pandoc"\n', '    bin.install "ashift"\n']:
            with self.subTest(line=line):
                self.formula.write_text(original.replace(line, ""))
                with self.assertRaises(ValueError):
                    self.verify()


if __name__ == "__main__":
    unittest.main()
