# Worker 7-T2: dependency audits

Branch `fix/v1-security-audit--deps`, based on `origin/feat/cloud-product-base` (`67a06fb`). All three ecosystems now audit clean. I fixed the JavaScript and Python advisories by upgrading. Rust needed no changes: its four advisories already had exceptions, and I re-checked that each one is still needed.

## Before and after

| Audit | Before | After |
|---|---|---|
| `cargo deny check` (advisories, bans, licences, sources) | Passes; 4 advisories, each with an existing ignore entry | Unchanged: passes, same 4 ignored advisories, now with a review date |
| `pnpm audit --prod` (all 10 importers, including `services/auth` and `packages/sdk`) | 0 | 0 |
| `pnpm audit` (all dependency groups) | 4 high (js-yaml ×3, body-parser ×1), dev tooling only | 0 |
| `uv audit --frozen` (98 locked distributions: docling, bench, fixtures/gen) | 41 (all Pillow 12.3.0, fixed in 13.0.0) | 0 |
| `pip-audit` cross-check per project | Not run before the fix | 0 for each of ariad-docling, ariad-bench and ariad-fixture-gen |

`cargo deny` reports only `duplicate` warnings, and `multiple-versions = "warn"` allows those. The advisory database was fetched fresh during the run (RustSec head of 2026-10-24).

## What changed

- **Pillow 12.3.0 to 13.0.0** (`fixtures/gen/pyproject.toml`, `uv.lock`). Pillow is bundled in the Docling engine pack, where it decodes untrusted images. The fixes cover JPEG and TIFF decoder heap overflows, out-of-bounds reads and more. Every dependent accepts this version: docling-core, docling-ibm-models, docling-parse and docling-slim all require `pillow<14`, and python-pptx, rapidocr and torchvision set no upper bound. cp314 wheels exist for linux x86_64/aarch64, macOS arm64/x86_64 and Windows. The licence is still `MIT-CMU`, which `engines/docling/pack/licenses.py` already allows, so that file did not change.
- **pnpm overrides** (`pnpm-workspace.yaml`, `pnpm-lock.yaml`):
  - `"js-yaml@>=4.0.0 <4.3.2": 4.3.2` fixes GHSA-52cp-r559-cp3m, GHSA-5p4m-2wfm-xmqj and GHSA-2883-xcg3-v3hh. `@hey-api/json-schema-ref-parser@1.4.4` pins `js-yaml 4.2.0` exactly, and 1.4.4 is its newest release, as is `@hey-api/openapi-ts` 0.99.0.
  - `"body-parser@>=2.0.0 <2.4.0": 2.4.0` fixes GHSA-27r3-jxmh-p828. It comes in through `shadcn` → `@modelcontextprotocol/sdk` → `express@5.3.0`.
  - The lockfile diff only changes those two packages and adds the `overrides` block. `pnpm install --lockfile-only` still passes the supply-chain policy check.
- **`deny.toml`**: I added a "last reviewed 2026-10-25, review again by 2027-01-25" note to the existing ignore list. jsonwebtoken 11.1.0, rsa 0.9.10, citationberg 0.7.0, hayagriva 0.10.1 and typst-library 0.15.1 are all the newest stable releases on crates.io, and none of them pulls a fixed version yet. `scripts/check-crypto-bigint.mjs` confirms that `crypto-bigint` is still only used under jsonwebtoken.
- **`docs/security/dependency-policy.md`** (new; the repo had no dependency policy doc): the audit commands for each lockfile, the order to try fixes (upgrade, then override, then exception), where each ecosystem records exceptions, and a table of current exceptions with the condition for removing each one.

## Exceptions added

None. The Rust exceptions already in place are RUSTSEC-2023-0071, RUSTSEC-2026-0387, RUSTSEC-2026-0194 and RUSTSEC-2026-0195. I re-reviewed them and gave them a review date. I did not add any pnpm `audit.ignore` or uv `--ignore` entries.

## Commits

- `f368ee4` build(deps): override vulnerable js-yaml and body-parser
- `36db66f` build(deps): bump pillow to 13.0.0
- `538e64c` chore(deps): record the review date of the advisory exceptions
- `7dee98d` docs: describe the dependency audit and exception policy
- plus the commit that adds this report

## Coordinator verification

```sh
just deny
pnpm install --frozen-lockfile && pnpm audit --prod && pnpm audit
just py                       # uv lock --check
uv audit --frozen
just sdk-check                # SDK generator now runs on js-yaml 4.3.2
just fixtures && git status --short fixtures/   # Pillow 13 must keep fixtures byte-identical
just bench-test
just docling-dev && just docling-test
just pack-docling && just pack-docling-verify   # pack licence gate and OCR on Pillow 13
```

## Open concerns

- The phase command `(cd engines/docling && uv run pip-audit)` will not work, because pip-audit is not a dependency of any project. Use `uv audit --frozen` instead (it is built in, uses OSV, and covers the `+cpu` torch wheels). If pip-audit itself is required, use the `uvx --from pip-audit==2.10.1` form given in the new policy doc.
- Pillow 13 is a new major version. The fixture generator's scan degradation (`fixtures/gen/src/ariad_fixture_gen/scan.py`: JPEG re-encode, rotate, noise) may produce different bytes from the committed fixtures. If `just fixtures` changes any files, decide whether to regenerate the fixtures or revisit the pin. The docling tests and the pack verify step exercise Pillow at runtime.
- `body-parser` 2.4.0 was published on 2026-10-22. It is a stable release and passed pnpm's supply-chain policy, but it is only three days old.
- No CI job currently runs `pnpm audit` or `uv audit`; `security.yml` covers only Rust advisories. Adding them belongs to whoever owns `.github/workflows/` or the `justfile`. A `deny`-style `audit-js` / `audit-py` recipe would fit `just static`.
- `pnpm peers check` reports an unmet `@opentelemetry/api` peer in `apps/web`. It predates this change and is unrelated to it.
- The new `docs/security/dependency-policy.md` sits under T1's `docs/security/**` tree. The brief assigns this one file to me. T1 may want to link it from their threat model.

Status: DONE_WITH_CONCERNS
