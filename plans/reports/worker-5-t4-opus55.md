# Worker 5-t4: compose end-to-end test

Branch `feat/cloud-product--e2e`, based on `origin/feat/cloud-foundation` at `fe0915c`.

## Result

`infra/test/e2e/run.sh` drives the self-hosted compose stack the way a client would, from sign-up to a verified DOCX and a quota refusal. `.github/workflows/cloud-e2e.yml` runs it in CI. Nothing was built or run: per the ASUS rules there was no `compose build`/`up`. The only checks were `bash -n`, a dry run of the env generation, and `docker compose config` on the generated environment. That last check resolved all 12 services, the four published ports, `ARIAD_PACKS=core,office,docling` and the public URLs.

## Files changed

- `infra/test/e2e/run.sh` (new, bash, `set -eu` plus `pipefail`, executable)
- `.github/workflows/cloud-e2e.yml` (new)
- `plans/reports/worker-5-t4-opus55.md` (this report)

## Commits

- `72fce72 test(infra): add an end-to-end test of the compose stack`
- `1ff99c5 ci(cloud-e2e): run the compose end-to-end test`
- the commit that adds this report

## What `run.sh` does

1. **Tools.** It checks for docker with the compose plugin, curl, jq and unzip. It also uses coreutils (`od`, `sha256sum` or `shasum`, `mktemp`).
2. **Environment.** It writes a throwaway env file in a `mktemp` directory from `.env.example`:
   - every `replace-with-…` value becomes 32 random bytes of hex;
   - profiles are core, office and docling, and `ARIAD_PACKS=core,office,docling`;
   - ports are 18080 (API), 13001 (auth), 18333 (S3) and 13000 (web);
   - the public URLs match those ports, because the JWT `iss`/`aud`, the CORS/trusted origin and the presigned S3 host must all agree.
3. **Isolation.** It runs as compose project `ariadshift-e2e` with `--env-file`. A developer's `infra/compose/.env`, their volumes and the default ports are never touched. Ports and timeouts can be overridden through `E2E_*` variables, which the script header documents.
4. **Start-up.** It runs `compose build`, then `compose up -d --wait --wait-timeout 900`. It then confirms `/readyz` and `/api/auth/ok` through the published ports.
5. **Account.** It signs up with `POST /api/auth/sign-up/email` (cookie jar, trusted `Origin`), then creates an API key with `POST /api/auth/api-key/create` and checks the `ashift_` prefix. All `/v1` calls use `x-api-key`.
6. **Upload.** The fixture is `fixtures/scan/en-alice-p3.pdf` (manifest id `pd-en-alice-page-3`, CC-PDM-1.0, tagged `scan`). The script checks its SHA-256 against the manifest value. It then calls `POST /v1/uploads`, sends the presigned `PUT` with exactly the returned headers (`--upload-file`, so curl adds no Content-Type of its own), and calls `POST /v1/uploads/{id}/complete`. It asserts the upload is `completed` and its format is `pdf`.
7. **Job.** It calls `POST /v1/jobs` with `to: docx` and asserts `pack == docling` and `route[0].engine == docling`. With docling deployed, `routing::plan` keeps only docling edges for PDF.
8. **Following the job.** It reads SSE from `/v1/jobs/{id}/events` with `Last-Event-ID` resume. Between streams it polls `GET /v1/jobs/{id}`, so a broken stream cannot hide a finished or failed job. The overall timeout is 900 s. At the end, the job status must be `succeeded`.
9. **Output.** It calls `GET /v1/jobs/{id}/outputs`, downloads the DOCX output and checks its size and SHA-256 against the listing.
   - The archive must pass `unzip -t` and contain both `[Content_Types].xml` and `word/document.xml`.
   - After the markup is stripped, `word/document.xml` must contain "Alice" and, case-insensitively, "rabbit". The page has an OCR text layer, and `crates/ariad-core/tests/pdf_reader.rs` asserts "DOWN THE RABBIT HOLE" and "Alice" on it.
10. **Deletion.** `DELETE /v1/jobs/{id}` must return 204. Afterwards the job reads as `expired`, `/outputs` returns 410, and the old presigned DOCX URL no longer returns 200.
11. **Quota refusal.** `admit_upload` in `entitlements.rs` refuses a new upload when the subject already has `concurrent_jobs` live pending uploads. The script reads the free plan's `concurrent_jobs` from `plan_entitlements` (via `compose exec postgres psql`). It creates that many pending uploads, which must all return 201. The next one must return 429 with `error.code == quota_exceeded`. Your workspace defaults to plan `free` (`db::DEFAULT_PLAN`). A quota is deterministic; the per-minute rate limit would depend on fixed-window boundaries.
12. **Teardown.** A `trap` on EXIT, INT and TERM always runs `compose down --volumes --remove-orphans`. On failure it first prints `compose ps -a` and `compose logs --timestamps --tail 400`. Built images are kept as a cache.

## Workflow

- **Triggers:**
  - nightly cron at `0 4 * * *` (scheduled runs check out `dev`, as `fuzz-nightly.yml` does);
  - `workflow_dispatch`;
  - `push` to `dev`/`main` with paths `infra/**`, `crates/ariad-server/**`, `services/**`, `packages/sdk/**`, plus `.env.example`, the fixture and the workflow itself (run.sh reads the first two);
  - `pull_request` into `dev`/`main`, for the types opened, synchronize, reopened and labeled.
- **Scope job.** It runs for pull requests because a `paths` filter would also suppress the `cloud-e2e` label trigger. It decides as follows:
  - a `labeled` event runs only when the label is `cloud-e2e`;
  - otherwise the run happens when the PR carries the label, or when `git diff base...head` touches the same paths.
- **Permissions.** `permissions: {}` at the top level and `contents: read` per job. Checkouts use `persist-credentials: false`. Event data reaches the scripts only through `env:`.
- **Pins.** `actions/checkout` uses the same pinned SHA as the existing workflows (v7.0.1). No other actions are used.
- **The e2e job.** It runs on `ubuntu-26.04` with `timeout-minutes: 180`, because every image builds from source. Its steps:
  - free disk space, as in `images.yml`;
  - set `kernel.apparmor_restrict_unprivileged_userns=0` with an `unshare` probe, as in `ci.yml`, for the LibreOffice namespace in the office and docling workers;
  - print tool versions;
  - run `bash infra/test/e2e/run.sh`.

## Commands for the coordinator

```sh
cd ~/Codes/01-AriadShift-wt-p5-e2e
bash -n infra/test/e2e/run.sh
shellcheck infra/test/e2e/run.sh          # not installed on this box
actionlint .github/workflows/cloud-e2e.yml
zizmor .github/workflows/cloud-e2e.yml
just static                                # typos, actionlint, zizmor
# Full run: needs about 34 GiB at the default limits, internet for the docling
# pack, and on Ubuntu the AppArmor userns sysctl above.
sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
infra/test/e2e/run.sh
```

## Changes needed elsewhere

- **`scripts/ci-scope.sh`** (owned by another task). As it stands, a change to `infra/test/e2e/**` or `.github/workflows/cloud-e2e.yml` counts as "heavy" and starts the three-OS Rust build, although neither file affects it. Add a path class excluded from `heavy` the way `js_paths` is, for example `e2e_paths='^infra/test/e2e/|^\.github/workflows/cloud-e2e\.yml$'`, and use `grep -v -E "$docs_paths|$js_paths|$e2e_paths"` in the heavy check. The static job still lints the workflow with actionlint and zizmor. Add matching rows to the `--classify` table test.
- **`docs/ci.md`** needs a row for `cloud-e2e.yml` in the workflow table: "Pushes and pull requests touching `infra/`, `crates/ariad-server/`, `services/`, `packages/sdk/`, or PRs labelled `cloud-e2e`; nightly; manual | Builds the compose stack with all three profiles and runs `infra/test/e2e/run.sh`".
- **`docs/self-hosting.md`** could add a short "Verify your stack" pointer to `infra/test/e2e/run.sh`.
- **The `cloud-e2e` label** must be created in the GitHub repository (`gh label create cloud-e2e`). That is a GitHub settings change, so it needs the user's go-ahead.

## Open concerns

- **Unverified.** The test has not been run. Things to watch on the first run:
  - whether Compose's `up --wait` accepts the one-shot services' exit 0 (docs/self-hosting.md relies on it too);
  - the Better Auth response shapes (`user.id` from sign-up, `key` from api-key/create; both match the current Better Auth docs);
  - the exact OCR text docling produces for the Alice page.
- **Docling image in CI.** The image installs the published signed Docling pack during its build. `images.yml` notes that this fails until the pack exists, and the e2e job fails the same way until then.
- **Runner capacity.**
  - Memory: `ubuntu-26.04` has 16 GiB. The compose `mem_limit` values (up to 20g for docling) are limits, not reservations, and a one-page job should fit, but this is untested.
  - Disk: the docling image is large; the free-disk step mirrors `images.yml`.
  - Build time: no build cache is used, so a run may come close to the 180-minute timeout.
- **Missing plan file.** The phase file `plans/261025-1651-ariadshift-v1-completion/phase-05-cloud-product.md` named in the common brief does not exist in this worktree, and I found no copy elsewhere under `~/Codes`. I worked from the task brief and decision A-1655 §5 as it is quoted there.
- **`ARCHITECTURE.md` not read.** I did not read it, because the change adds a test and a workflow only: no crate, service or dependency.

Status: DONE_WITH_CONCERNS
