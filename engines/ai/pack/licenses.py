"""Fail-closed licence checks for the self-contained development pack inputs."""

from __future__ import annotations

import hashlib
import tomllib
from pathlib import Path

PACK_DIR = Path(__file__).resolve().parent
ALLOWED = frozenset({"MIT", "Apache-2.0", "BSD-3-Clause", "PSF-2.0"})


def check(root: Path = PACK_DIR) -> list[str]:
    pins = tomllib.loads((root / "pins.toml").read_text())
    inventory = tomllib.loads((root / "licence-review.toml").read_text())
    errors = []
    if set(inventory["allowed_licenses"]) != ALLOWED:
        errors.append("licence allow-list differs from the approved permissive licences")
    for component in inventory["component"]:
        name = component["name"]
        pin = pins.get(name, {})
        if component["spdx"] not in ALLOWED or pin.get("license") != component["spdx"]:
            errors.append(f"unapproved licence for {name}")
        actual = (
            (pin.get("version") + "+" + pin.get("release"))
            if name == "python"
            else (pin.get("version", pin.get("revision")))
        )
        if actual != component["version"]:
            errors.append(f"unreviewed version for {name}")
    for notice in inventory["notice"]:
        path = (root / notice["path"]).resolve()
        if not path.is_relative_to(root.resolve()) or not path.is_file():
            errors.append("missing or unsafe notice path")
        elif (
            not path.stat().st_size
            or hashlib.sha256(path.read_bytes()).hexdigest() != notice["sha256"]
        ):
            errors.append(f"notice digest mismatch: {notice['path']}")
    for component in ("pypdf", "model", "llama_cpp", "florence"):
        if component in pins:
            digest = pins[component].get("sha256", "")
            if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
                errors.append(f"missing SHA-256 for {component}")
    if inventory["pack"] == "inpaint":
        if set(pins.get("policy", {}).get("excluded_weights", [])) != {"big-lama", "Places365"}:
            errors.append("unreviewed inpainting weights are not explicitly excluded")
        config = root / "florence-config.json"
        if hashlib.sha256(config.read_bytes()).hexdigest() != pins["florence"]["sha256"]:
            errors.append("Florence configuration digest mismatch")
    return errors


if __name__ == "__main__":
    findings = check()
    if findings:
        raise SystemExit("\n".join(findings))
    print("All development pack inputs have reviewed permissive licences and intact notices.")
