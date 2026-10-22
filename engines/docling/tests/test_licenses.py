"""Every distribution, model and data file the pack bundles passes the licence gate."""

from __future__ import annotations

import sys
from email.message import Message
from importlib import metadata

import pytest
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines" / "docling" / "pack"))

import fetch
import licenses


class FakeDistribution(metadata.Distribution):
    def __init__(self, name: str, **fields: str | list[str]) -> None:
        self._message = Message()
        self._message["Name"] = name
        self._message["Version"] = "1.0"
        for key, value in fields.items():
            for item in value if isinstance(value, list) else [value]:
                self._message[key.replace("_", "-")] = item

    @property
    def metadata(self):  # type: ignore[override]
        return self._message

    def read_text(self, filename):
        return None

    def locate_file(self, path):
        raise NotImplementedError


def test_installed_distributions_pass_the_gate():
    findings = licenses.check(metadata.distributions())
    assert findings == [], "\n".join(f"{f.distribution}: {f.license}: {f.reason}" for f in findings)


@pytest.mark.parametrize(
    ("distribution", "allowed"),
    [
        (FakeDistribution("a", License_Expression="MIT"), True),
        (FakeDistribution("b", License_Expression="Apache-2.0 WITH LLVM-exception AND MIT"), True),
        (FakeDistribution("c", License_Expression="GPL-3.0-or-later OR MIT"), False),
        (FakeDistribution("d", License_Expression="AGPL-3.0-only"), False),
        (FakeDistribution("e", License_Expression="LicenseRef-OpenRAIL-M"), False),
        (FakeDistribution("f", License_Expression="MPL-2.0"), False),
        (FakeDistribution("certifi", License_Expression="MPL-2.0"), True),
        (FakeDistribution("g", License="Custom licence, see LICENSE"), False),
        (FakeDistribution("h", Classifier=["License :: OSI Approved :: MIT License"]), True),
        (FakeDistribution("i", License_Expression="LGPL-2.1-or-later"), False),
        (
            FakeDistribution("j", License_Expression="(MIT OR Apache-2.0) AND LicenseRef-Other"),
            False,
        ),
        (
            FakeDistribution("k", License_Expression="MIT AND (Apache-2.0 OR LicenseRef-Other)"),
            True,
        ),
        (FakeDistribution("l", License_Expression="MIT OR LicenseRef-Other"), True),
        (FakeDistribution("m", License_Expression="Apache-2.0 WITH Unreviewed-exception"), False),
        (FakeDistribution("n", License_Expression="(MIT"), False),
        (FakeDistribution("o", License_Expression="MIT AND"), False),
        (FakeDistribution("p", License_Expression="MIT Apache-2.0"), False),
    ],
)
def test_gate_accepts_permissive_and_rejects_excluded_licences(distribution, allowed):
    assert (licenses.check([distribution]) == []) is allowed


def test_pinned_models_runtime_and_data_use_permissive_licences():
    pins = fetch.load_pins()
    declared = [model["license"] for model in pins["models"]]
    declared += [pins["tessdata"]["license"], pins["python"]["license"]]
    assert set(declared) <= licenses.ALLOWED_DATA


def test_pinned_files_have_hashes_sizes_and_safe_paths():
    pins = fetch.load_pins()
    files = fetch.model_files(pins) + fetch.tessdata_files(pins)
    assert {item.dest.name for item in files} >= {"vie.traineddata", "eng.traineddata", "tsv"}
    for item in files:
        assert len(item.sha256) == 64
        assert item.size > 0
        assert item.url.startswith("https://")
    targets = set(pins["python"]["targets"])
    assert targets == {
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    }
