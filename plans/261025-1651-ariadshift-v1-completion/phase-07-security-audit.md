---
phase: 7
title: "Security audit and fixes"
status: pending
depends_on: [5, 6]
branch: fix/v1-security-audit
---

# Phase 7: Security audit and fixes

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | GPT-6.1-Sol High (first) | `docs/security/**`, `plans/261025-1651-ariadshift-v1-completion/reports/security-audit.md` | Refresh the threat model, then audit auth and OAuth, uploads and presigned URLs, MCP path confinement (local and remote), pack and toolchain-wheel verification and extraction, sandboxes, every parser, and web headers/CSP. Rank findings by severity and name the owning files. No code edits. |
| T2 | Opus 5.5 High | `deny.toml`, JS and Python manifests and lockfiles, `engines/docling/pack/licenses.py` exceptions | `cargo-deny`, `pnpm audit` and `pip-audit`: upgrade or document each advisory. |
| T3 | Opus 5.5 High | `apps/web/next.config.*`, `apps/web/src/middleware.*`, `apps/web/src/app/layout.tsx` | Security headers and CSP (COOP/COEP on `/app/*` only), with Lighthouse kept at 90 or more. |
| T4 | GPT-6.1-Sol High (after T1) | Files named by T1's critical and high findings that T2/T3 do not own | Fix every critical and high finding, each with a regression test. |

## Build and test pass (coordinator, sequential)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just ci && just test-server && just desktop-ci && just web-e2e && just web-lighthouse
just deny && pnpm audit --prod && (cd engines/docling && uv run pip-audit)
```

## Acceptance

- [ ] The audit report shows zero open critical or high findings.
- [ ] Dependency audits are clean, or each advisory has a documented, scoped exception.
- [ ] Lighthouse is at least 90 on `/` and `/app`.
