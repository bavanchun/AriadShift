# Worker report: p8-t5 v1.0.0 release notes draft

Branch `feat/v1-distribution--t5`, cut from `origin/feat/v1-distribution` (`88c52eb`).

## Result

The 1.0.0 entry is drafted in `docs/release-notes.md`, headed `1.0.0 (unreleased)`, above the 0.2.0 entry. No version field was touched. Nothing was built or tested, as the common rules require.

## Files changed

- `docs/release-notes.md`: the file does not exist on `feat/v1-distribution`. It lives on `origin/release/v0.2.0--notes`, which is not in `dev` yet. I copied that file byte for byte in the first commit, then added the 1.0.0 entry in a second one.
- `plans/reports/worker-p8-t5-v1-notes.md`: this report.

## Commits

1. `docs: add the release notes file with the 0.2.0 entry`: identical blob to `origin/release/v0.2.0--notes:docs/release-notes.md`.
2. `docs: draft the 1.0.0 release notes`: only the new `## 1.0.0 (unreleased)` section.
3. The commit that adds this report.

## What the entry covers

Sections: Highlights, Breaking changes, Upgrade notes from 0.2.0, Added (CLI, Web, Desktop, Cloud and self-hosting, MCP, SDK), Security, Distribution and install (unsigned desktop builds and how to verify them), Known limitations (R4, no code signing, DCR only, no OAuth sign-up or consent-revocation UI, plus the other open items) and Roadmap for 1.x.

## Where the facts come from

The five branches are not stacked. `feat/cloud-foundation`, `feat/cloud-product`, `feat/v1-hardening` and `fix/v1-security-audit` each hold part of phases 4 to 7, and `feat/v1-distribution` is cut from `dev`. I read their `docs/`, `services/auth/README.md`, `packages/sdk`, `.github/workflows/images.yml`, `crates/ariad-cli/tests/mcp_stdio.rs`, the worker and review reports and the commit logs with `git show` and `git archive`. Each branch's claims come from its own tree:

| Topic | Source |
|---|---|
| Compose stack, auth service, `/v1` API, SDK, GHCR images | `origin/feat/cloud-foundation`, `origin/feat/cloud-product` |
| Web cloud mode, account pages, offline policy | `origin/feat/cloud-product` (`docs/web.md`, `docs/api.md`) |
| Remote MCP, OAuth, Seatbelt, OS integration, `GET /v1/jobs`, `/v1/limits`, `/v1` freeze, fuzz targets, desktop polish | `origin/feat/v1-hardening` |
| Audit fixes, CSP profiles, blob budgets, dependency overrides, R4 notice | `origin/fix/v1-security-audit` |
| Install guide, release checksums and signatures, Homebrew check, docs sweep | `origin/feat/v1-distribution` |

## Verification for the coordinator

No build needed. Read-only checks:

```sh
git diff origin/release/v0.2.0--notes -- docs/release-notes.md | head   # only the 1.0.0 section differs
git show HEAD~2:docs/release-notes.md | cmp - <(git show origin/release/v0.2.0--notes:docs/release-notes.md)   # copied file is identical
just spell                                                              # typos is not installed on this Mac, so I could not run it
just lint-commits origin/feat/v1-distribution..HEAD
```

Check that every relative link in the new section resolves once the branches are merged: `api.md`, `mcp.md`, `self-hosting.md`, `install.md`, `web.md`, `desktop.md`, `cli.md`, `security/threat-model.md` and `security/dependency-policy.md`. The last two exist only on `fix/v1-security-audit`.

## Open concerns

- **Merge order.** `docs/release-notes.md` is added by the first commit here and by `release/v0.2.0--notes`. After that branch merges, rebase this branch onto `dev`: the first commit becomes empty if the blobs match, and the second applies cleanly.
- **Pack range.** `docs/packs.md` and the pack manifest declare `requires_ashift = ">=0.2.0, <0.3.0"`. A 1.0.0 host refuses that pack (exit 5). A `docling` pack release whose range covers 1.0.0 must be published before the tag. The Upgrade notes assume it exists, and the coordinator blockquote at the top of the entry says so.
- **Pull request numbers.** The file's intro says each entry cites its pull request. The 1.0.0 bullets have none, because the branches are unmerged. A blockquote asks the coordinator to add them and drop sentences that no longer match.
- **Untested fixes.** The audit fixes, remote MCP OAuth, Seatbelt and the OS integration are described as the code on the branches reads. Their worker reports say the code was not compiled or tested. If the coordinator's build queue changes a behavior, update the matching bullet.
- **Open review items I listed as known limitations** came from the phase 6 and 7 reviews: Seatbelt read scope (M2), the MCP `Host` rule, Windows image-confinement gaps (R6), completion hashing (R5) and Turnstile under COEP (R8). Drop any that the coordinator fixes before the tag.

Status: DONE_WITH_CONCERNS
