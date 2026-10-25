# Dependency audit policy

How AriadShift checks third-party dependencies for published vulnerabilities, and how an advisory is either fixed or recorded as a scoped exception. Licence rules live in [AGENTS.md](../../AGENTS.md#licensing); version rules live in [AGENTS.md](../../AGENTS.md#versions).

## Audits

| Ecosystem | Lockfile | Command | Scope |
|---|---|---|---|
| Rust | `Cargo.lock` | `just deny` (`cargo deny check`: advisories, bans, licences, sources) | Every crate on the targets listed in [`deny.toml`](../../deny.toml) |
| JavaScript | `pnpm-lock.yaml` | `pnpm audit --prod`, then `pnpm audit` | Every workspace importer, including `services/auth` and `packages/sdk` |
| Python | `uv.lock` | `uv audit --frozen` | Every member of the uv workspace: `engines/docling`, `bench` and `fixtures/gen` |

`pnpm audit --prod` gates what ships. Run the full `pnpm audit` as well, because development tools such as the SDK generator still parse files they did not write.

`uv audit` queries OSV for every locked distribution, including the CPU-only PyTorch wheels from the PyTorch index. To cross-check one project with `pip-audit`, export its pins and audit them without installing:

```sh
uv export --frozen --package ariad-docling --all-groups --no-emit-workspace --no-hashes \
  --format requirements-txt -o /tmp/ariad-docling.txt
uvx --from pip-audit==2.10.1 pip-audit -r /tmp/ariad-docling.txt --no-deps --disable-pip
```

`pip-audit` resolves names against PyPI, so it skips local-version wheels such as `torch==2.14.1+cpu`; `uv audit` covers them.

The daily [`security.yml`](../../.github/workflows/security.yml) workflow runs the Rust advisory check on `dev` and `main` ([ci.md](../ci.md)).

## Handling an advisory

1. **Upgrade** to the lowest stable release that fixes it, verified on its registry. Change the direct pin when the package is a direct dependency. For a transitive package, refresh the lockfile when the parent's range already allows the fix (`cargo update -p <crate> --precise <version>`, `uv lock --upgrade-package <name>`).
2. **Override** a transitive JavaScript package whose parent pins the vulnerable release, when the fix is in the same major line. Add the override to `overrides` in [`pnpm-workspace.yaml`](../../pnpm-workspace.yaml) with a version range that matches only vulnerable releases, an exact fixed version, and a comment naming the advisory IDs and the parent. Drop it once the parent resolves the fix on its own.
3. **Except** only when no fixed release is reachable and the vulnerable code path is unreachable or harmless in AriadShift. Each exception names one advisory ID, states why it does not apply, and says what removes it.

| Ecosystem | Where exceptions live |
|---|---|
| Rust | `[advisories] ignore` in [`deny.toml`](../../deny.toml): one `{ id, reason }` entry per advisory, with a comment that names the crate path, the reason and the removal condition |
| JavaScript | `audit.ignore` in `pnpm-workspace.yaml`, with a comment per GHSA ID |
| Python | `uv audit --ignore-until-fixed <ID>` in the recipe or workflow that runs the audit, with a comment per ID |

Every exception list carries a "last reviewed" date and a next review date at most three months later. At each review, check whether the parent packages have released a fix, and remove each exception that no longer matches an advisory.

## Current exceptions

Last reviewed 2026-10-25. Next review by 2027-01-25.

| Advisory | Package | Path | Why it does not apply | Removed when |
|---|---|---|---|---|
| RUSTSEC-2023-0071 | `rsa` 0.9.10 | `jsonwebtoken` in `ariad-server` | Timing side channel in private-key operations; the server only verifies signatures with public keys | `rsa` publishes a fixed release, or the server stops using RSA |
| RUSTSEC-2026-0387 | `crypto-bigint` 0.5.5 | `jsonwebtoken` (p256/p384) in `ariad-server` | Constant-time issues in secret-integer arithmetic; verification with public JWKS keys handles no secrets. `scripts/check-crypto-bigint.mjs` keeps the crate confined to that path | A stable `jsonwebtoken` adopts `crypto-bigint` 0.7.6 or later |
| RUSTSEC-2026-0194 | `quick-xml` 0.38.4 | `typst-library`, `hayagriva`, `citationberg` | Quadratic duplicate-attribute check in CSL parsing; generated Typst markup never loads a bibliography or CSL file | `citationberg` moves to `quick-xml` 0.41 or later |
| RUSTSEC-2026-0195 | `quick-xml` 0.38.4 | Same as above | Namespace allocation in `NsReader` during CSL parsing, unreachable for the same reason | Same as above |

JavaScript and Python have no audit exceptions. The JavaScript workspace overrides `js-yaml` to 4.3.2 (GHSA-52cp-r559-cp3m, GHSA-5p4m-2wfm-xmqj, GHSA-2883-xcg3-v3hh, through `@hey-api/json-schema-ref-parser`) and `body-parser` to 2.4.0 (GHSA-27r3-jxmh-p828, through the shadcn CLI's `@modelcontextprotocol/sdk`).
