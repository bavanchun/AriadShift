"""Assert licenses of installed dependencies and locked packages.

Distributed artifacts and test harnesses must not contain GPL/AGPL/LGPL
packages or banned GPL edit-distance libraries (Levenshtein, python-Levenshtein, distance).
All dependencies must use exact OSI-approved permissive licenses.
"""

from __future__ import annotations

import importlib.metadata
from pathlib import Path
import re
import tomllib
import unittest

BANNED_PACKAGE_NAMES = {
    "levenshtein",
    "python-levenshtein",
    "distance",
    "editdistance",
}

BANNED_LICENSE_SUBSTRINGS = (
    "gpl",
    "general public license",
    "gnu",
    "agpl",
    "affero",
    "lgpl",
)

ALLOWED_EXACT_SPDX = {
    "mit",
    "mit-cmu",
    "apache-2.0",
    "bsd-2-clause",
    "bsd-3-clause",
    "psf-2.0",
    "isc",
    "unlicense",
    "cc0-1.0",
}

ALLOWED_EXACT_CLASSIFIERS = {
    "license :: osi approved :: mit license",
    "license :: osi approved :: apache software license",
    "license :: osi approved :: bsd license",
    "license :: osi approved :: python software foundation license",
    "license :: osi approved :: isc license (iscl)",
}


def _is_permissive_exact(license_val: str, license_expr: str, classifiers: list[str]) -> bool:
    """Verify license information matches exact allowed permissive SPDX identifiers or classifiers."""
    # 1. Check License-Expression if present
    if license_expr:
        tokens = re.split(r"[\s()]+|\bOR\b|\bAND\b", license_expr.strip())
        tokens = [t.strip().lower() for t in tokens if t.strip()]
        if tokens and all(t in ALLOWED_EXACT_SPDX for t in tokens):
            return True

    # 2. Check License field if present
    if license_val:
        norm = license_val.strip().lower()
        if norm in ALLOWED_EXACT_SPDX:
            return True
        tokens = re.split(r"[\s()]+|\bor\b|\band\b", norm)
        tokens = [t.strip() for t in tokens if t.strip()]
        if tokens and all(t in ALLOWED_EXACT_SPDX for t in tokens):
            return True

    # 3. Check Classifier fields
    license_classifiers = [c.strip().lower() for c in classifiers if c.strip().lower().startswith("license ::")]
    if license_classifiers and all(c in ALLOWED_EXACT_CLASSIFIERS for c in license_classifiers):
        return True

    return False


class TestLicenses(unittest.TestCase):
    def test_no_banned_packages_in_uv_lock(self) -> None:
        """Verify uv.lock contains no banned or GPL packages."""
        root = Path(__file__).resolve().parents[2]
        lock_path = root / "uv.lock"
        self.assertTrue(lock_path.is_file(), f"uv.lock not found at {lock_path}")

        data = tomllib.loads(lock_path.read_text(encoding="utf-8"))
        packages = data.get("package", [])
        self.assertGreater(len(packages), 0, "uv.lock should contain resolved packages")

        for pkg in packages:
            name = pkg.get("name", "").lower()
            self.assertNotIn(
                name,
                BANNED_PACKAGE_NAMES,
                f"Banned package '{name}' found in uv.lock",
            )

    def test_installed_distributions_have_permissive_licenses(self) -> None:
        """Verify all installed distributions have permissive OSI-approved licenses via exact match."""
        dists = list(importlib.metadata.distributions())
        self.assertGreater(len(dists), 0, "Expected installed distributions in environment")

        installed_names: set[str] = set()
        workspace_members = {"ariad-bench", "ariad-fixture-gen", "ariadshift"}

        for dist in dists:
            pkg_name = dist.metadata.get("Name", "").lower()
            installed_names.add(pkg_name)

            self.assertNotIn(
                pkg_name,
                BANNED_PACKAGE_NAMES,
                f"Banned package '{pkg_name}' is installed in the environment",
            )

            if pkg_name in workspace_members:
                continue

            license_val = dist.metadata.get("License", "") or ""
            license_expr = dist.metadata.get("License-Expression", "") or ""
            classifiers = dist.metadata.get_all("Classifier") or []

            combined_raw = " ".join([license_val, license_expr, *classifiers]).lower()
            for banned in BANNED_LICENSE_SUBSTRINGS:
                self.assertNotIn(
                    banned,
                    combined_raw,
                    f"Package '{pkg_name}' contains banned license substring '{banned}'",
                )

            is_valid = _is_permissive_exact(license_val, license_expr, classifiers)
            self.assertTrue(
                is_valid,
                f"Package '{pkg_name}' does not have an exact allowed permissive license. "
                f"License={license_val!r}, Expression={license_expr!r}, Classifiers={classifiers!r}",
            )

        # Assert all required bench dependencies and fixture-gen dependencies are verified
        expected_bench = {"jiwer", "rapidfuzz", "apted", "psutil", "jsonschema"}
        expected_fixture_gen = {"lxml", "pillow", "python-docx", "typst"}
        self.assertTrue(
            expected_bench.issubset(installed_names),
            f"Missing bench packages: {expected_bench - installed_names}",
        )
        self.assertTrue(
            expected_fixture_gen.issubset(installed_names),
            f"Missing fixture-gen packages: {expected_fixture_gen - installed_names}",
        )


if __name__ == "__main__":
    unittest.main()
