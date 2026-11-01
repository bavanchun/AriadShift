# Worker Fix Report: `node26`

## Issue #79: Migrate Node.js runtime and `@types/node` together to Node 26 LTS

### Cause and Evidence
- AriadShift's versioning policy (`ARCHITECTURE.md` §2.1) requires pinning the latest active LTS release for toolchains with an LTS channel and keeping the Node.js runtime and `@types/node` major versions aligned.
- Node.js 26 entered Active LTS ("Lithium") on `2026-10-28` (`26.13.0` per `https://nodejs.org/dist/index.json` and `https://endoflife.date/api/nodejs.json`, maintained until `2029-04-30`), while the repository remained pinned to Node 24 LTS (`24.21.0`) and `@types/node` `24.19.2`:
  - `.node-version:1` pinned `24.21.0`.
  - `pnpm-workspace.yaml:23` pinned `"@types/node": 24.19.2` (resolved in `pnpm-lock.yaml`).
  - `services/auth/Dockerfile:15` and `infra/docker/web.Dockerfile:16` pinned `node:24.21.0-trixie-slim@sha256:173f125896c3b47ddf056734c7ea789d04595a6a08769a8f78e0df642781fb66`.
  - `ARCHITECTURE.md:47`, `ARCHITECTURE.md:272`, `README.md:139`, and `services/auth/README.md:99` referenced Node 24 / `24.21.0`.

### Registry Verification
- **Node.js 26 LTS (`https://nodejs.org/dist/index.json`, `https://endoflife.date/api/nodejs.json`):** Latest 26.x LTS release is `26.13.0` (`lts: "Lithium"`, released `2026-10-28`, EOL `2029-04-30`).
- **Docker Hub (`https://registry-1.docker.io/v2/library/node/manifests/26.13.0-trixie-slim`):** Multi-arch manifest list digest is `sha256:e4db4c914bf8ad241bd24804fc25d272d89bad97e68cdc8a1414862790a02c39`.
- **npm `@types/node` (`https://registry.npmjs.org/@types/node`):** Latest stable 26.x release is `26.6.6` (`latest` dist-tag).

### What Broke and Assumptions Recorded
- **Breakage under `@types/node` 26.6.6 (`just web-typecheck`):**
  - After upgrading `@types/node` from `24.19.2` to `26.6.6`, `just web-typecheck` (`pnpm -r --if-present typecheck`) failed in `services/auth` at `services/auth/test/oauth-flow.test.ts(7,32)` with:
    `error TS2305: Module '"node:crypto"' has no exported member 'JsonWebKey'.`
  - In `@types/node` 26.x, the top-level `JsonWebKey` re-export was removed from `"node:crypto"` in favor of `webcrypto.JsonWebKey` and `JsonWebKeyInput` (`JsonWebKeyInput["key"]`).
  - **Decision/Assumption:** Because `just web-typecheck` is a required check and part of Issue #79's acceptance criteria (`just web-ci`), and no other active worker owns `services/auth/test/oauth-flow.test.ts`, updated lines 7 and 187 of `services/auth/test/oauth-flow.test.ts` to import `type JsonWebKeyInput` from `"node:crypto"` and cast `jwk as JsonWebKeyInput["key"]`.
- **`docs/release-notes.md`:** Does not contain an `Unreleased` section (starts at `## 0.2.0 (2026-10-31)`), so it was left unchanged per task instructions (`unreleased section, if present`).
- **CI Workflow Node Pins:** Audited all files under `.github/workflows/`. None hardcode Node 24; all six `actions/setup-node` steps read `.node-version` via `node-version-file: .node-version`:
  - `.github/workflows/ci.yml`: lines 89–91, 369–371, 536–538
  - `.github/workflows/desktop.yml`: lines 44–46, 148–150, 237–239
- **`package.json` `engines.node`:** Left `"node": ">=24"` unchanged (not in the owned files list, and `>=24` remains satisfied by Node 26).

### Fix
- `.node-version`: Updated from `24.21.0` to `26.13.0`.
- `pnpm-workspace.yaml` & `pnpm-lock.yaml`: Updated catalog `"@types/node"` from `24.19.2` to `26.6.6` and refreshed `pnpm-lock.yaml` via `pnpm install`.
- `services/auth/Dockerfile` & `infra/docker/web.Dockerfile`: Updated `ARG NODE` to `node:26.13.0-trixie-slim@sha256:e4db4c914bf8ad241bd24804fc25d272d89bad97e68cdc8a1414862790a02c39`.
- `ARCHITECTURE.md`: Updated line 47 (`Node.js` row in §2.1 Runtimes and Platforms) to `**26.x "Lithium"** (26.13.0)` and line 272 (`.node-version` comment in §5 Monorepo Structure) to `# 26` without touching the Decision Log table.
- `README.md`: Updated line 139 from `Node.js 24.21.0` to `Node.js 26.13.0`.
- `services/auth/README.md`: Updated line 99 `server.requestTimeout` documentation URL from `v24.21.0` to `v26.13.0`.
- `services/auth/test/oauth-flow.test.ts`: Replaced `type JsonWebKey` import from `"node:crypto"` with `type JsonWebKeyInput` and `jwk as JsonWebKeyInput["key"]`.

### Tests Verified
- `services/auth/test/oauth-flow.test.ts` (`OAuth authorization for remote MCP`, 7 tests) and full `@ariadshift/auth` test suite (14 passed test files, 3 DB-gated skipped, 170 passed tests).
- Full workspace Vitest suite via `just web-test` (`packages/ariad-wasm`, `services/auth`, `apps/web`, `packages/sdk`, `packages/ui`: 55 passed test files, 3 skipped, 601 passed tests).

### Verification Commands
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=26.10.0 pnpm install`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env UV_NO_CONFIG=1 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=26.10.0 just web-lint`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env UV_NO_CONFIG=1 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=26.10.0 just js-licenses`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=26.10.0 pnpm --filter @ariadshift/auth test`: **PASS** (14 test files passed, 170 tests passed)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env UV_NO_CONFIG=1 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=26.10.0 just web-typecheck`: **PASS** (all 6 JS/TS packages including `apps/web` and `services/auth` passed `tsc --noEmit`)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env UV_NO_CONFIG=1 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=26.10.0 just web-test`: **PASS** (55 test files passed, 601 tests passed)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh docker build --check -f services/auth/Dockerfile .`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh docker build -f services/auth/Dockerfile -t ariadshift-auth-node26 .`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh docker build --check -f infra/docker/web.Dockerfile .`: **PASS**

### What Only CI or Another OS Can Verify
- GitHub Actions `actions/setup-node` reading `.node-version` (`26.13.0`) on `ubuntu-26.04`, `macos-26`, and `windows-2025` runners (local host has `mise` Node `26.10.0` installed).
- Full multi-stage `infra/docker/web.Dockerfile` image build and Playwright browser E2E suite in CI (`just web-ci` / `cloud-e2e`).

### Suggested PR Text
**Title:**
`chore(node): migrate runtime and @types/node to Node.js 26 LTS`

**Body:**
```markdown
## Summary
- Pin `.node-version` to `26.13.0` (Node 26 "Lithium" Active LTS) and update `services/auth/Dockerfile` and `infra/docker/web.Dockerfile` to `node:26.13.0-trixie-slim@sha256:e4db4c914bf8ad241bd24804fc25d272d89bad97e68cdc8a1414862790a02c39`.
- Upgrade `@types/node` in `pnpm-workspace.yaml` and `pnpm-lock.yaml` from `24.19.2` to `26.6.6` and update `services/auth/test/oauth-flow.test.ts` to use `JsonWebKeyInput["key"]` from `"node:crypto"`.
- Update Node.js version references in `ARCHITECTURE.md`, `README.md`, and `services/auth/README.md`.

Closes #79
```
