# Worker p4-t5 (Opus 5.5 High): GHCR container images

The images workflow and the web image are written and pass the workflow linters. Nothing was built, per the brief. Two small changes are needed in files I do not own: one `actionlint.yaml` label, and one image-name decision for `.env.example`. Both are listed under "Needs coordinator action".

## Files changed

- `.github/workflows/images.yml` (new)
- `infra/docker/web.Dockerfile` (new)
- `infra/docker/web.Dockerfile.dockerignore` (new)

## Commits

There are no commits yet. The worktree index is locked. `.git/worktrees/01-AriadShift-wt-cloud-foundation/index.lock` is a 0-byte file dated 17:52 and was still there at 18:31. No git process is working on this worktree: the hung `git worktree remove` processes from 17:06 and 17:11 target other worktrees. The T3 worker is also waiting on this lock. The brief forbids deleting lock files, so I stopped. The three files are untracked and complete. Once the coordinator clears the stale lock, run:

```sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-cloud-foundation
git add infra/docker/web.Dockerfile infra/docker/web.Dockerfile.dockerignore
git commit -m "feat(infra): add a static web image served by unprivileged nginx" -- infra/docker/web.Dockerfile infra/docker/web.Dockerfile.dockerignore
git add .github/workflows/images.yml
git commit -m "ci(images): publish signed multi-arch images to GHCR on release tags" -- .github/workflows/images.yml
```

## What was done

### `images.yml`

- **Triggers.** A `v*` tag push builds, pushes, merges and signs every image. `workflow_dispatch` builds the same images for both platforms and pushes nothing: there is no login, `push=false`, and the publish job is skipped.
- **Images.** `ariadshift-api` (`server.Dockerfile`, target `server`, CMD `api`) and `ariadshift-worker-core` (the api image plus `CMD ["worker", "--packs", "core"]`, using a one-line inline Dockerfile). Also `ariadshift-worker-office`, `ariadshift-worker-docling`, `ariadshift-auth` (`services/auth/Dockerfile`, repository-root context, as in compose) and `ariadshift-web` (`infra/docker/web.Dockerfile`, target `web`).
- **Multi-arch.** A matrix runs linux/amd64 on `ubuntu-26.04` and linux/arm64 on `ubuntu-26.04-arm`. The arm64 hosted runner is listed in actions/runner-images. Each job builds natively, so the Rust compile never runs under QEMU. Every image, docling included, is built for both platforms. `docs/docling.md` lists `aarch64-unknown-linux-gnu` as a pack target, and the LibreOffice and rustup steps already carry arm64 checksums.
- **Chaining without a registry.** The step writes a Bake definition to `$RUNNER_TEMP/images.hcl`. The worker images take the api image or the office image through `target:` contexts, the same names compose uses (`ariadshift-server`, `ariadshift-worker-office`). The whole chain therefore builds inside one builder, which is what lets the dispatch run work without pushing. The definition lives inline because I may edit only `images.yml`. It could move to `infra/docker/images.docker-bake.hcl` later if a local `docker buildx bake` is wanted.
- **Docling isolation.** The docling image is built in its own step with `continue-on-error`. It reuses the api and office layers from the same builder's cache, with no second Rust build. If the signed pack is not published, the other five images still publish. The build job's last step then fails, and the docling publish leg fails with a clear message, so the run is red and nothing is hidden.
- **Attestations.** Every target has `type=provenance,mode=max` and `type=sbom` through buildx. Per-platform images are pushed by digest (`push-by-digest=true,name-canonical=true`). Their digests travel as one-day artifacts.
- **Publish job** (a matrix over the six images, `needs: build`, `if: !cancelled() && push`). It checks that the tag matches `vX.Y.Z[-pre]`. It then runs `docker buildx imagetools create` from both platform digests with the tag `vX.Y.Z`, matching `.env.example`, and adds OCI index annotations for source, revision and version. This carries over the attestation manifests. Next it reads the index digest and runs `cosign sign --yes ref@digest` (keyless, through GitHub OIDC). Finally it runs `cosign verify` against the exact workflow identity, `$GITHUB_SERVER_URL/$GITHUB_WORKFLOW_REF`, with the issuer `https://token.actions.githubusercontent.com`, and prints that command for users.
- **Permissions.** The top level is `permissions: {}`. The build job has `contents: read` and `packages: write`. The publish job has `packages: write` and `id-token: write`. `id-token` appears only on the signing job. Each permission has an inline reason for zizmor's pedantic audit.
- **Conventions.** `actions/checkout`, `upload-artifact` and `download-artifact` use the repository's existing SHA pins. New pins were checked against the GitHub API and are each at least 7 days old, following the repository's Dependabot cooldown: `docker/setup-buildx-action` v4.4.1 `f87e599…`, `docker/login-action` v4.6.0 `dbcb813…` and `sigstore/cosign-installer` v4.1.2 `6f9f177…`. The tools are pinned too: buildx `v0.38.0` and cosign `v3.1.3`. The newer buildx v0.38.1 and cosign v3.1.4 are 5 days old, and login-action v4.7.0 is 2 days old. All steps use `persist-credentials: false`, put no expressions inside `run:` (values pass through `env:`), use the bash shell with pipefail, and set `cache-binary: false`, so the publishing workflow touches no GitHub cache. A disk-cleanup step removes the runner's Android, .NET and GHC SDKs before the large worker builds.

### `web.Dockerfile`

- **Build stage.** It runs on `$BUILDPLATFORM`, because the export does not depend on the architecture. The base is `node:24.21.0-trixie-slim@sha256:173f1258…`, which matches `.node-version` and was verified on Docker Hub. On top of it go rustup-init (same version and SHA-256 values as `server.Dockerfile`, with the toolchain and wasm32 target from `rust-toolchain.toml`) and pnpm 12.9.1 from its GitHub release tarball. The pnpm SHA-256 values come from the release asset digests, and the x64 one was checked against a download. The stage then runs the same steps as `just wasm-build` (with `--locked`), `pnpm install --frozen-lockfile --filter "@ariadshift/web..."` and `pnpm --filter @ariadshift/web export`.
- **Build arguments.** It accepts `NEXT_PUBLIC_ARIAD_API_URL` and `NEXT_PUBLIC_ARIAD_AUTH_URL`, which compose already passes. The defaults match compose (`http://localhost:8080` and `http://localhost:3001`).
- **Runtime stage.** The base is `nginxinc/nginx-unprivileged:1.30.6-alpine-slim@sha256:5d8bb943…`, the nginx stable channel (1.30), verified on Docker Hub for amd64 and arm64. The server runs as UID 101 and listens on port 3000, which is the port compose maps. It writes only `/tmp`, so it works with compose's `read_only`, `cap_drop: [ALL]` and `/tmp` tmpfs. The entrypoint runs `nginx -g 'daemon off;'` directly, which skips the base image's entrypoint scripts that rewrite `/etc/nginx`. The image has a `wget` `HEALTHCHECK` (compose says the image defines one), `server_tokens off`, `absolute_redirect off` and gzip, including `application/wasm`. It copies LICENSE, NOTICE and THIRD_PARTY_LICENSES as `server.Dockerfile` does.
- **Headers.** They follow `route-headers.mjs` and `docs/web.md`. `/app`, `/app.html` and `/app/*` get `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless`. `/_next/static/chunks/*` gets only the COEP header. Every other path gets neither. Routing uses `try_files $uri $uri.html $uri/`, which suits the export's `*.html` files without a trailing-slash redirect, and 404s are served from `/404.html`.

## Verification already run (read-only)

- `actionlint` 1.7.12: the only error is the unknown label `ubuntu-26.04-arm`. With that label added to a scratch copy of `.github/actionlint.yaml`, actionlint and its shellcheck pass with exit code 0.
- `uvx zizmor@1.30.1 .github/workflows/images.yml`: no findings, both online (including impostor-commit and ref-version-mismatch) and with `--persona=pedantic`.

## Needs coordinator action (files I do not own)

0. **Stale index lock.** Clear it, then make the two commits above.
1. **`.github/actionlint.yaml`.** Add `- ubuntu-26.04-arm` under `self-hosted-runner.labels`, next to `ubuntu-26.04`. actionlint 1.7.12 predates that label too. Without this line, `just lint-workflows` fails.
2. **Image names against compose.** The brief names `ariadshift-api` and `ariadshift-worker-core`. T4's `.env.example` and `docs/self-hosting.md` use one variable, `ARIADSHIFT_SERVER_IMAGE=ghcr.io/OWNER/ariadshift-server:vX.Y.Z`, for api, migrate and worker-core. Compose overrides `command` on each service, so the published `ariadshift-api` image works for all three. The recommendation is that T4 points `ARIADSHIFT_SERVER_IMAGE` at `ghcr.io/OWNER/ariadshift-api:vX.Y.Z`, or splits worker-core onto `ariadshift-worker-core`. The other alternative is that I add an `ariadshift-server` alias to the workflow.
3. **Docs.** `docs/self-hosting.md` ("Published images", owned by T4) could add the `cosign verify` command that the publish job prints, plus the note that the published web image has the localhost API and auth URLs baked in, so custom URLs need a local `web` build.

## Commands for the coordinator

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just lint-workflows                       # after the actionlint.yaml label above
docker build -f infra/docker/web.Dockerfile -t ariadshift-web:local .
docker run --rm -d --name ashift-web-check --read-only --cap-drop ALL --tmpfs /tmp -p 127.0.0.1:3000:3000 ariadshift-web:local
curl -sI http://127.0.0.1:3000/ | grep -i cross-origin       # expect no lines
curl -sI http://127.0.0.1:3000/app | grep -i cross-origin    # expect COOP same-origin + COEP credentialless
curl -sI "http://127.0.0.1:3000/_next/static/chunks/$(curl -s http://127.0.0.1:3000/app | grep -o '_next/static/chunks/[^"]*\.js' | head -1 | sed 's#_next/static/chunks/##')" | grep -i cross-origin   # expect COEP only
docker inspect --format '{{.State.Health.Status}}' ashift-web-check   # healthy after ~30 s
docker rm -f ashift-web-check
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait   # web is built from this Dockerfile
```

GitHub check, after push: run `Images` by `workflow_dispatch` on the branch. Both build legs should go green with nothing pushed, and the publish job should be skipped. The docling step stays red until the signed Docling pack is published.

## Open concerns

- The workflow and image were never built here, per the brief. The first dispatch run is the real test of the Bake definition: string-form `output` and `attest`, the variables used inside the `output()` function, and the `target:` chaining.
- The second Bake step builds only `worker-docling`. Its `target:` dependencies (api and office) come from the builder's cache. If Bake also re-exports those dependencies, it pushes the same layers again, which is harmless. Only the `worker-docling` digest is read from that step.
- The `worker-core` digest differs from the `api` digest. The image layers are shared, but the config differs (CMD).
- Runner disk: the docling image is the largest. The cleanup step removes the largest preinstalled SDKs and prints `df -h /`, but I have not measured how much it frees. If the arm64 leg still runs out of disk, the docling build is the step to move to a larger runner.

Status: DONE_WITH_CONCERNS
