"""Derive the Office worker seccomp profile from Docker's default profile.

The Office engine builds a private /tmp in an unprivileged user and mount
namespace. Docker's default profile allows `unshare` and `mount` only with
CAP_SYS_ADMIN, which the workers never get. This script adds exactly two
rules on top of the pinned upstream profile:

- `unshare` with the flag set CLONE_NEWUSER | CLONE_NEWNS and nothing else;
- `mount`, which the kernel restricts to the new namespace's own mounts.

Usage, from the repository root:

    python3 -I infra/compose/seccomp/build-office-profile.py

Upstream: https://github.com/moby/profiles (Apache-2.0), tag seccomp/v0.2.4.
"""

import hashlib
import json
import pathlib
import urllib.request

UPSTREAM = (
    "https://raw.githubusercontent.com/moby/profiles/"
    "245180c51918481c0525424b3ee025d2b435d46c/seccomp/default.json"
)
UPSTREAM_SHA256 = "785b2429264afba4d594320337cb17f144f3c7d51585f9805eef72e28f4f9334"
CLONE_NEWNS = 0x00020000
CLONE_NEWUSER = 0x10000000
OUT = pathlib.Path(__file__).with_name("worker-office.json")


def main() -> None:
    with urllib.request.urlopen(UPSTREAM, timeout=30) as response:
        raw = response.read()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != UPSTREAM_SHA256:
        raise SystemExit(f"upstream profile digest changed: {digest}")
    profile = json.loads(raw)
    profile["syscalls"].append(
        {
            "names": ["unshare"],
            "action": "SCMP_ACT_ALLOW",
            "args": [
                {
                    "index": 0,
                    "value": CLONE_NEWUSER | CLONE_NEWNS,
                    "op": "SCMP_CMP_EQ",
                }
            ],
        }
    )
    profile["syscalls"].append({"names": ["mount"], "action": "SCMP_ACT_ALLOW"})
    OUT.write_text(json.dumps(profile, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
