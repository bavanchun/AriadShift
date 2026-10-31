# Cloud foundation review

Two high-severity findings require fixes before merge. No critical finding was confirmed.

Reviewed `git diff origin/dev...origin/feat/cloud-foundation`, with base
`3b8528149bc6a647bf50fbb992559cb4e2095b7c` and head
`c27c506e2dc2953da13ac6ea1a7b94bd3618ac65`. The review covers OpenAPI export,
auth, SDK, Compose, container publishing and their workspace integration.
This is a source review; no builds, tests, installs or containers were run.

## High: the new workflow fails the required workflow lint gate

**Location:** `.github/workflows/images.yml:46`.

The arm64 matrix selects `ubuntu-26.04-arm`, but
`.github/actionlint.yaml:5` allowlists only `ubuntu-26.04`.
`scripts/install-lint-tools.sh:11` pins actionlint 1.7.12, whose
[runner-label implementation](https://github.com/rhysd/actionlint/blob/v1.7.12/rule_runner_label.go)
does not recognize either 26.04 label by default and validates values resolved
from a matrix. The existing exception covers only the amd64 label.

**Failure scenario:** a PR containing this workflow reaches the always-enabled
static CI job (`.github/workflows/ci.yml:57`). `just static` calls
`lint-workflows`, which runs `.tools/bin/actionlint` (`justfile:24`). It rejects
the arm64 label and exits nonzero, blocking the mandatory gate for the whole
change. The earlier T5 report also records this exact diagnostic; the integration
report explicitly carries it forward without a fix.

**Fix:** add `ubuntu-26.04-arm` to the same actionlint configuration exception,
then run `just lint-workflows`. Keep the native arm64 build decision.

## High: the documented published-image route requests an unpublished image

**Location:** `.env.example:77`.

The example tells operators to set
`ARIADSHIFT_SERVER_IMAGE=ghcr.io/OWNER/ariadshift-server:vX.Y.Z`, and
`docs/self-hosting.md:152` directs them to use those image settings followed by
`pull` and `up --no-build`. However, the workflow exports the server target as
`ariadshift-api` (`.github/workflows/images.yml:107`) and publishes only
`api`, `worker-core`, `worker-office`, `worker-docling`, `auth` and `web`
(`.github/workflows/images.yml:232`). It never publishes `ariadshift-server`.

**Failure scenario:** after replacing OWNER and the version with a release from
this workflow, the documented `docker compose ... pull` cannot obtain the
server image. `migrate`, `api` and `worker-core` all reference that variable
(`infra/compose/docker-compose.yml:148`, `:247`, `:294`), so the published-image
stack cannot start. A source build does not validate this deployment route.

**Fix:** point the example at `ghcr.io/OWNER/ariadshift-api:vX.Y.Z`, or publish
the advertised server alias. The API image already supports all three services:
their Compose commands select `migrate`, `api` and `worker --packs core`.
Verify the published names against a real release before using `--no-build`.

## Contracts checked and limitations

- Auth explicitly signs EdDSA/Ed25519 JWTs for the normalized public origin.
  The API derives the same issuer/audience from `ARIAD_AUTH_URL`, and Compose
  points its JWKS fetch at `http://auth:3000/api/auth/jwks`. An empty custom
  payload preserves `sub` in the pinned Better Auth
  [JWT implementation](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/better-auth/src/plugins/jwt/sign.ts).
- Auth migrates before listening. `db-grants` waits for both server migrations
  and healthy auth, grants SELECT on `apikey` through `public, auth`, and grants
  schema USAGE to the API role. The API login inherits that role and uses the
  same search path. Default user-key configuration and SHA-256/base64url hashing
  agree with Rust and the pinned
  [API-key implementation](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/api-key/src/index.ts).
- Published ports bind to loopback by default. Secrets are required explicitly;
  the example instructs operators to replace placeholders. Auth CORS uses the
  configured web origin, with host-only HttpOnly cookies and Secure over HTTPS.
- The signing job alone receives OIDC permission; registry credentials are passed
  to the login action rather than Docker build arguments. SBOM/provenance are
  requested per platform, and cosign signs/verifies the final index digest.
  Attestation retention and actual registry publication remain unexecuted.
- The SDK's committed spec/config hashes match `generator.lock.json`, and the
  Rust drift test compares the generated document with the committed file.
  The script also compares existing client output with a fresh generation.
  Actual generation, typechecking and drift tests remain unexecuted.
- A missing prior install is not itself an auth Dockerfile finding: pinned
  [pnpm deploy source](https://github.com/pnpm/pnpm/blob/v12.9.1/pnpm11/releasing/commands/src/deploy/deploy.ts)
  reads the shared lockfile and installs the deployment. Docker build success
  still requires coordinator verification.
- The gVisor override explicitly documents its loss of Landlock confinement.
  No additional high finding is asserted without runtime evidence. Office,
  Docling, bucket initialization and real auth migrations still need smoke tests.

`git diff --check origin/dev...origin/feat/cloud-foundation` passed. The completion
plan is absent from this checkout; its plan and Phase 4 file were read from the
existing `01-AriadShift-wt-p3-notes` worktree, alongside the copied T1–T5 reports.

Verdict: FIX_REQUIRED
