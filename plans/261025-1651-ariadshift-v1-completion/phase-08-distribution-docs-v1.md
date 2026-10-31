---
phase: 8
title: "Distribution, docs, v1.0.0 prep"
status: moved
depends_on: [7]
branch: feat/v1-distribution
---

# Phase 8: Distribution, docs, v1.0.0 prep

Moved to [plans/261031-1422-next-stage-pdf-suite/phase-20-hardening-benchmarks-and-1-0-launch.md](../261031-1422-next-stage-pdf-suite/phase-20-hardening-benchmarks-and-1-0-launch.md).

Applies A-1655 §3 (unsigned desktop builds) and §4 (registry scope).

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | Opus 5.5 High | `.github/workflows/desktop.yml` | Desktop installers ship with `SHA256SUMS` and Minisign signatures. Apple/Windows signing steps stay wired and fail closed: they turn on when the secrets exist and are skipped cleanly otherwise. |
| T2 | Opus 5.5 High | `.github/workflows/release.yml`, `.github/workflows/publish-crates.yml`, Homebrew formula templates or scripts under `scripts/release/**` | The release workflow generates the Homebrew tap formula; the tap and crates.io pushes skip with a notice when their token is missing; GitHub Releases carries CLI archives, installers and packs. |
| T3 | Opus 5.5 High | `docs/desktop.md` install section, `docs/install.md` (new) | Install guide for every channel, including the Gatekeeper and SmartScreen first-run steps for unsigned builds and checksum/Minisign verification. |
| T4 | Opus 5.5 High | `README.md`, ARCHITECTURE.md status and Decision Log, AGENTS.md "Current state", `docs/*.md` not owned by T3 | Docs sweep: every surface with install and quick start; ARCHITECTURE status set to "v1". |
| T5 | Opus 5.5 High | Version fields as in phase 3 T1, the release notes file | Bump to `1.0.0` and write release notes. |

## Build and test pass (coordinator, sequential)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just ci && just desktop-ci && just check-release-workflow && just package-check && just lint-workflows
just desktop-build    # local unsigned bundle; verify checksum and minisign signature
```

## Release (user-gated)

After the merge, the coordinator sends `to-boss/Q-*-release-1-0.md` with exact commands for the `dev` → `main` promotion and the `v1.0.0` tag. Once the user acts, smoke-test every channel that publishes and record the evidence in the roadmap's phase-11 file.

## Acceptance

- [ ] A local desktop bundle verifies against its checksum and Minisign signature; the signing steps skip cleanly without secrets.
- [ ] The release dry run produces the Homebrew formula; the crates.io and tap pushes skip without tokens.
- [ ] Docs match behaviour; every roadmap product criterion is ticked or listed as user-blocked.
- [ ] The v1.0.0 Q file is sent.
