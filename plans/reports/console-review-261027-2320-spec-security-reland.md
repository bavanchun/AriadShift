# Spec: security re-land branch replacing `origin/fix/v1-security-audit` (section E)

Date: 2026-10-27 (Asia/Saigon). Read-only review: nothing was checked out, built, run or fetched. Source of truth: `console-review-261027-2045-master-fix-plan-final.md` §4.E (MFPF). Supporting reports: `2000-security-audit-branch-server` (SAS), `2000-security-audit-branch-web` (SAW), `2015-verify-security-branch` (V-SEC, which overrides the first two).

Refs used (local remote-tracking, `git for-each-ref`):

| Ref | Head | Note |
|---|---|---|
| `origin/dev` | `0b118f6` | Docs-only commit on top of MFPF's `e6bd29b`; no code changed (`git diff --stat e6bd29b origin/dev` lists only `plans/reports/*`). |
| `origin/fix/v1-security-audit` | `de92a19` | Merge base with dev is `3b85281`; it carries #48 and cloud-product history. |
| `origin/feat/cloud-product` (#84) | `796c83e` | |
| `origin/feat/v1-hardening` | `dcb34bc` | |

Conflict predictions below come from comparing each commit's pre-image (`git diff <sha>^ <target> -- <files>`) with dev, cloud-product and hardening, and from the merge resolutions recorded on the old branch (`git show --cc`). `git merge-file`/`merge-tree` were not available in this session, so every conflict is inferred. Re-check with the real cherry-pick.

## 1. Verdict

Eight of the nine items are ready to implement: E0 (with E2 folded into its conflict resolution), E1, E4, E6, E7, E8 and E9. E5 has a full spec but is blocked on OD-9/OD-12. E3 already moved to I4. Nothing is stale. The re-land must start from `dev` after step 2 (#84) at the earliest and after step 3b by plan. I found five corrections to MFPF: `e3c2b21` and `076fe90` do not cherry-pick cleanly, `01fc65a` conflicts with hardening's OAuth bullets, E6's suggested `status='verifying'` needs a migration, and SRV-1 in `2045-dev-fix-specs` collides with `90787ce` and must not be dispatched.

## 2. Summary table

| ID | Sev | Still valid | Lands on | Files (main) | Size | Worker |
|---|---|---|---|---|---|---|
| E0 | High | yes (list corrected, §3.0) | new `fix/v1-security-reland` from integrated dev | the 12 picked commits' files, plus the regenerated `schemas/openapi.v1.json`, snapshot, `packages/sdk/generator.lock.json`, `pnpm-lock.yaml` | M (L if D25 rewrote `put_stream_until`) | **R1** (runs first, alone) |
| E1 | High | yes | reland | `infra/compose/docker-compose.yml`, `apps/web/route-headers.mjs`, `apps/web/scripts/write-static-headers.mjs`, `apps/web/test/{headers,build-wiring}.test.ts`, `docs/{web,self-hosting}.md`, `.env.example` | S | **R2** |
| E2 | Medium | partly: on today's dev, yes; on the reland base it is only E0's Dockerfile conflict resolution | reland (inside E0) | `infra/docker/web.Dockerfile` | S | R1 |
| E3 | — | moved to I4 | dev (step 0a) | — | — | — |
| E4 | Medium | yes | reland | `crates/ariad-server/src/storage.rs`, `tests/blob-transfers.rs` | S | **R3** |
| E5 | Medium | yes (blocked: OD-9, OD-12) | reland | `storage.rs`, `config.rs`, `api/{blobs,uploads,jobs}.rs`, `tests/{blob-transfers,s3_storage}.rs`, `docs/server.md`, spec, snapshot, SDK lock | M | R3 |
| E6 | Medium | yes (design changed, §3.6) | reland | `storage.rs`, `api/uploads.rs`, spec, snapshot, SDK lock | M | R3 |
| E7 | Medium (test) | yes | reland | `apps/web/e2e/cloud.spec.ts` | S | R2 |
| E8 | Low | yes (9 sub-items) | reland | web: `route-headers.mjs`, `headers.test.ts`, `build-wiring.test.ts`, `docs/web.md`, `docs/security/threat-model.md`; server: `api/blobs.rs`; SDK: `packages/sdk/src/client.ts`, `packages/sdk/test/client.test.ts` | M | R2 (web), R3 (server and SDK) |
| E9 | Medium | yes | reland | new `apps/web/e2e-static/static-csp.spec.ts`, `apps/web/playwright.static.config.ts`, `apps/web/scripts/serve-static-export.mjs`; `justfile`; `docs/ci.md` | M | **R4** |

Worker file sets are disjoint. R1 runs alone. R2, R3 and R4 then run in parallel. One E8 commit, the worker `default-src`, is made by R2 last, after R4's project exists, because only R4's project can prove it.

## 3. Items

### 3.0 E0: the cherry-pick plan (High)

**Base.** Create `fix/v1-security-reland` from `origin/dev` once steps 1 (#85), 2 (#84), 3a and 3b (hardening) have merged, as MFPF step 4 says. These are hard prerequisites:

- Step 2 (#84). `be8da76`'s test reads `apps/web/src/app/(account)`, which only cloud-product has (`account`, `api-keys`, `jobs`, `sign-in`). The Dockerfile resolution needs cloud-product's Typst build and `unzip` (`4eaba39`, `c19ea77`, both ancestors of `origin/feat/cloud-product`). E7 edits `apps/web/e2e/cloud.spec.ts`, which only #84 has.
- I2 and I4 on dev. They carry `7836169` and Pillow 13. If I2 has not landed, add `7836169` to this branch; MFPF checked that it applies cleanly to dev. Never pick `36db66f` alone: without I4's `bench/pyproject.toml` bump, `uv lock` fails (V-SEC F3).

Step 3b is a soft prerequisite. Hardening's `storage.rs`, `config.rs` and `api/blobs.rs` are byte-identical to dev today (`git diff origin/dev origin/feat/v1-hardening -- <those files>` is empty), so the server picks do not depend on it. Landing after 3b avoids regenerating the spec, snapshot and SDK lock twice.

**Verified skip list** (each checked against current refs):

- `f9deea5` and `0136cd9`: already on dev via #53. `git diff f9deea5 origin/dev -- crates/ariad-server/src/{api/blobs.rs,api/mod.rs,config.rs,storage.rs}` is empty. `git diff 0136cd9 origin/dev -- crates/ariad-server/src/auth/jwt.rs crates/ariad-server/tests/jwks-cache/mod.rs` is empty. `tests/blob-transfers.rs` differs only by dev's later random signing key. `git cherry` still shows them as `+` only because #53 was squashed.
- `7836169`: lands through I2. `36db66f`: lands through I4.
- `3b85281`: already an ancestor of dev.

**Pick order** (use `git cherry-pick -x`; stage explicit paths only):

| # | Commit | Files | Expected on the integrated base | Resolution |
|---|---|---|---|---|
| 1 | `90787ce` fix(server): stop slow blob transfers from holding every slot | `crates/ariad-server/src/{app,config,storage}.rs`, `tests/blob-transfers.rs`, `docs/server.md` | Clean today. The pre-image equals dev, cloud-product and hardening for `app.rs`, `config.rs` and `storage.rs`. In the test file, dev's random-key lines (`SIGNING_KEY`, `Some(SIGNING_KEY.as_bytes())`) sit two or more lines from the pick's hunks. In `docs/server.md`, dev differs only at line 37. Hardening's `app.rs` hunk is at lines 71–82, while the pick touches lines 35–38. **Hotspot:** if D25 (step 3b) rewrites `put_stream_until` (`storage.rs:486ff`), the `admit(Some(key))` line conflicts. | Keep D25's structure. Apply `admit(key, true)`, `let floor = RateFloor::new(&self.blob_limits);` and `floor.check(total)?;` inside its loop. |
| 2 | `f8b0347` fix(schemas): document the blob timeout and busy responses | `api/blobs.rs`, `schemas/openapi.v1.json`, `packages/sdk/generator.lock.json` | `blobs.rs` is clean: its pre-image equals all three refs. The schema and lock conflict, or apply as stale text. | Whether or not git reports a conflict, overwrite both by regeneration: `just openapi`; then, if D16's equality rule landed, `ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot`; then `pnpm --filter @ariadshift/sdk lock`. Stage `schemas/openapi.v1.json crates/ariad-server/tests/snapshots/openapi_v1.json packages/sdk/generator.lock.json`. Never hand-merge the lock. |
| 3 | `709f982` feat(server): warn at startup about local blob transfer deadlines | `storage.rs` | Clean on top of #1. | — |
| 4 | `01fc65a` docs: describe the local storage transfer deadline | `docs/api.md`, `docs/self-hosting.md` | `api.md`: clean after #84. Both hunks anchor on cloud-product text (`"Optional" means …` and the `| 404 |` error row), and no ref documents 408 yet. `self-hosting.md`: **conflict**. The pick inserts "Storage" between "Public URLs" and "Ports"; hardening inserts "OAuth resource" at the same point. | Keep every bullet, in this order: Secrets, Public URLs, Storage, OAuth resource, Ports, OAuth load limits, and the rest. |
| 5 | `ad6969d` fix(web): send a strict CSP and security headers on every route | `apps/web/{next.config.ts,route-headers.mjs,scripts/write-static-headers.mjs,test/headers.test.ts}` | Clean. The pre-image equals dev and hardening. Cloud-product changes only `next.config.ts:12` (`transpilePackages`), three lines from this commit's comment hunk at :16. | — |
| 6 | `e3c2b21` fix(web): serve the generated security headers from the web image | `infra/docker/web.Dockerfile` | **Conflict (this is E2).** The pre-image is dev's broken Dockerfile, without Turnstile, Typst or `unzip`. Cloud-product rewrote the same ARG/ENV/RUN block (`:66-76`). The old branch resolved this by hand in merge `d81fbe8` (`git show --cc d81fbe8 -- infra/docker/web.Dockerfile`), and a cherry-pick loses that resolution. V-SEC's "hunks do not overlap" holds for a branch merge, not for this pick. | Keep cloud-product's lines 1–65 (`unzip`, `install-wasm-bindgen.sh`, `-p ariad-wasm -p ariad-typst`, both bindgen calls). Replace everything from the `# Public base URLs` comment through the inline nginx heredoc with the block below. |
| 7 | `076fe90` docs(web): explain each security header and CSP directive | `docs/web.md` | **Conflict likely.** `d81fbe8` also resolved this file by hand. | Keep cloud-product's offline-policy paragraphs and the whole `## Cloud mode` section. Take this commit's `### Security headers` section and its "Node server"/"Static hosting" edits. Then fix the anchor that the heading rename breaks: `([Headers](#headers))` becomes `([Security headers](#security-headers))`. The old branch still has the broken link at `docs/web.md:160`. |
| 8 | `be8da76` fix(web): let the account pages reach the cloud origins under CSP | `route-headers.mjs`, `headers.test.ts`, `docs/web.md` | Clean once #5–#7 are in, because the pre-image is the branch state. | — |
| 9 | `f368ee4` build(deps): override vulnerable js-yaml and body-parser | `pnpm-workspace.yaml`, `pnpm-lock.yaml` | Workspace file is clean: its pre-image equals all three refs. Lockfile conflicts. | `git checkout --ours pnpm-lock.yaml`, then `pnpm install --lockfile-only`, then `git add pnpm-lock.yaml`. Run `pnpm why js-yaml body-parser`. Drop an override whose parent now resolves the fixed version on its own, as `docs/security/dependency-policy.md` requires. |
| 10 | `538e64c` chore(deps): record the review date of the advisory exceptions | `deny.toml` | Clean: the pre-image equals dev, hardening and distribution. | Re-run `just deny`. |
| 11 | `7dee98d` docs: describe the dependency audit and exception policy | new `docs/security/dependency-policy.md` | Clean (new file). | — |
| 12 | `85dcee0` docs(security): record threat model and source review | new `docs/security/threat-model.md`, `plans/reports/security-audit-261025-v1.md` | Clean (new files). E8 fixes three stale paragraphs afterwards. | — |
| 13 (optional, see §4) | `134460f 73606d9 03ddd88 fcde8ba 1de8482 2bcb5ec de92a19` | `plans/reports/worker-7-*.md`, `code-review-261025-phase7-final.md`; `2bcb5ec` also updates the audit report that `threat-model.md:97` links | Clean: new files, plus a follow-up edit to #12's report. | They preserve the phase-7 evidence before the old branch is deleted (MFPF H). Their old hashes describe history; they are not current state. |

Dockerfile block for pick #6, identical to `origin/fix/v1-security-audit:infra/docker/web.Dockerfile:66-88`:

```dockerfile
# Public base URLs for cloud mode; the defaults match infra/compose. The
# Turnstile site key and the operator name are optional; empty leaves them off.
ARG NEXT_PUBLIC_ARIAD_API_URL=http://localhost:8080
ARG NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3001
ARG NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=
ARG NEXT_PUBLIC_ARIAD_OPERATOR=
# Object-store origins of the API's presigned URLs, for the converter's CSP
# connect-src (space-separated); the default matches ARIAD_S3_PUBLIC_ENDPOINT
# in infra/compose.
ARG ARIADSHIFT_WEB_STORAGE_ORIGINS=http://localhost:8333
ENV NEXT_PUBLIC_ARIAD_API_URL=${NEXT_PUBLIC_ARIAD_API_URL} \
    NEXT_PUBLIC_ARIAD_AUTH_URL=${NEXT_PUBLIC_ARIAD_AUTH_URL} \
    NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=${NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY} \
    NEXT_PUBLIC_ARIAD_OPERATOR=${NEXT_PUBLIC_ARIAD_OPERATOR} \
    ARIADSHIFT_WEB_STORAGE_ORIGINS=${ARIADSHIFT_WEB_STORAGE_ORIGINS}
RUN ARIADSHIFT_WEB_NGINX_CONF=/src/web-nginx.conf pnpm --filter @ariadshift/web export

FROM ${NGINX} AS web
COPY LICENSE NOTICE THIRD_PARTY_LICENSES /usr/share/doc/ariadshift/
COPY --from=build /src/apps/web/out /srv/web
# The server and its headers are generated from apps/web/route-headers.mjs at
# export time: a CSP per page that pins its inline scripts, cross-origin
# isolation on /app only, and worker policies on the script chunks.
COPY --from=build /src/web-nginx.conf /etc/nginx/conf.d/default.conf
```

Only `d81fbe8` changed content while merging: `docs/web.md` and the Dockerfile. `git show --cc` shows no hunks for `4e6eb2c`, `484457b`, `7ebad35` or `bd41cea`.

**Post-pick consistency check.** Run `git diff origin/fix/v1-security-audit -- apps/web/route-headers.mjs apps/web/scripts/write-static-headers.mjs apps/web/test/headers.test.ts apps/web/next.config.ts infra/docker/web.Dockerfile docs/web.md crates/ariad-server/src/storage.rs crates/ariad-server/src/config.rs`. Every remaining hunk must be explained by the integrated dev (cloud-product or hardening additions, the anchor fix, or the random-signing-key test lines). Anything else is a bad resolution. After the server picks, run `cargo check -p ariad-server --tests`.

**Gate.** This is the full step-4 gate. Run it once, after R2, R3 and R4 have merged into the re-land branch:

```sh
cargo nextest run -p ariad-server --lib --test blob-transfers --test openapi_drift --test openapi_v1_snapshot
cargo nextest run -p ariad-host --test confine --test assets && cargo nextest run -p ariad-cli --test mcp_stdio   # regression for I2
pnpm --filter @ariadshift/web test && pnpm --filter @ariadshift/sdk test
just sdk-check && just deny && pnpm audit
just test-server && just ci
just web-e2e && just web-lighthouse && just web-e2e-static
docker build -f infra/docker/web.Dockerfile -t ariadshift-web:reland .
docker run --rm -d --name ariadshift-web-reland -p 127.0.0.1:3400:3000 --read-only --tmpfs /tmp ariadshift-web:reland
for p in / /app /sign-in /jobs; do curl -sI "http://127.0.0.1:3400$p" | grep -iE '^(content-security-policy|cross-origin|x-content-type)'; done
curl -sI http://127.0.0.1:3400/index | head -1   # 404 once the E8 fallback fix is in
docker stop ariadshift-web-reland                 # stop what you started
```

**Commits.** The picks keep their original messages; `-x` adds the trace line. Conflict resolutions belong in the pick's own commit. Regeneration-only changes go in that pick's commit too, never in a separate "fix lockfile" commit.

**Risks.** The conflict list is inferred and becomes exact only on the real base. The SDK lock and spec must be regenerated after every server change that touches utoipa text (picks #2, E5 and E6), so do it in each commit; skipping one leaves the branch failing `just sdk-check`. The Dockerfile resolution is the only one that changes runtime behaviour, so `docker build` plus `curl` are mandatory.

### 3.1 E1: CSP storage origin never reaches the web build (High)

**Current behaviour.**

- `cloudSources` (`origin/fix/v1-security-audit:apps/web/route-headers.mjs:180-198`) takes storage origins only from `ARIADSHIFT_WEB_STORAGE_ORIGINS`. Line 186 splits the raw value, and lines 187–190 reject anything not matching `ORIGIN_SOURCE` (`:148`), including a trailing slash or path.
- Compose passes the web build only `NEXT_PUBLIC_ARIAD_{API_URL,AUTH_URL,TURNSTILE_SITE_KEY,OPERATOR}` (`origin/feat/cloud-product:infra/compose/docker-compose.yml:376-379`). The API presigns with `ARIAD_S3_PUBLIC_ENDPOINT` (`:255`).
- So the Dockerfile default `http://localhost:8333` always wins. Any other endpoint, for example `infra/test/e2e/run.sh:46,164` (`S3_PORT=18333`), makes the browser refuse presigned PUTs and GETs on `/app` and `/jobs`.
- The variable is undocumented in `docs/self-hosting.md`, `.env.example` and the `docs/web.md` configuration table (`origin/fix/v1-security-audit:docs/web.md:145-150`).

**Change.**

1. `infra/compose/docker-compose.yml`, `web.build.args`, after `NEXT_PUBLIC_ARIAD_OPERATOR`:
   ```yaml
           # The converter's CSP connect-src must list the origin of the API's presigned URLs.
           ARIADSHIFT_WEB_STORAGE_ORIGINS: ${ARIAD_S3_PUBLIC_ENDPOINT:-http://localhost:8333}
   ```
2. `apps/web/route-headers.mjs`. Add above `cloudSources`:
   ```js
   /**
    * One `ARIADSHIFT_WEB_STORAGE_ORIGINS` entry reduced to the origin a CSP source needs.
    * Operators pass the endpoint they gave the API (`ARIAD_S3_PUBLIC_ENDPOINT`), which may
    * end in a slash or a path; presigned URLs only need its origin. A leading `*.` label is
    * kept for virtual-hosted buckets.
    * @param {string} entry
    */
   function storageOrigin(entry) {
     const wildcard = /^(https?:\/\/)\*\.(.+)$/.exec(entry);
     const candidate = wildcard ? `${wildcard[1]}${wildcard[2]}` : entry;
     const url = URL.canParse(candidate) ? new URL(candidate) : null;
     const origin = url && (wildcard ? `${url.protocol}//*.${url.host}` : url.origin);
     if (!origin || !ORIGIN_SOURCE.test(origin)) {
       throw new Error(`ARIADSHIFT_WEB_STORAGE_ORIGINS takes http or https origins: ${entry}`);
     }
     return origin;
   }

   /**
    * A build warning for cloud mode with no object-store origin. That is right only for
    * `file://` storage, whose presigned URLs are on the API origin.
    * @param {Record<string, string | undefined>} env
    * @returns {string | null}
    */
   export function storageOriginsWarning(env) {
     if (!env.NEXT_PUBLIC_ARIAD_API_URL?.trim() || env.ARIADSHIFT_WEB_STORAGE_ORIGINS?.trim()) {
       return null;
     }
     return "ARIADSHIFT_WEB_STORAGE_ORIGINS is empty: browsers may upload only to the API origin (file:// storage). Set it to the S3 public endpoint when the API uses S3.";
   }
   ```
   In `cloudSources`, replace lines 186–191 with:
   ```js
   const storage = (env.ARIADSHIFT_WEB_STORAGE_ORIGINS ?? "")
     .split(/\s+/)
     .filter(Boolean)
     .map(storageOrigin);
   ```
3. `apps/web/scripts/write-static-headers.mjs`. Import `storageOriginsWarning`. After `const table = …`, add:
   ```js
   const warning = storageOriginsWarning(process.env);
   if (warning) {
     console.warn(warning);
   }
   ```
   This is the default behaviour for OD-21. If the owner picks "fail", replace the `console.warn` with `throw new Error(warning)`.
4. Docs:
   - `docs/self-hosting.md` "Public URLs": add "and the origin of `ARIAD_S3_PUBLIC_ENDPOINT`, which the web image's Content-Security-Policy allows for uploads and downloads" to the list the web image bakes in.
   - `.env.example`, above `ARIAD_S3_PUBLIC_ENDPOINT`: `# The web image also bakes this origin into its CSP; rebuild web after changing it.`
   - `docs/web.md` configuration table, new row: `| ARIADSHIFT_WEB_STORAGE_ORIGINS | Space-separated object-store origins of the API's presigned URLs, allowed in connect-src on /app and the account pages. Read by the header generator, not inlined into the app. Compose sets it from ARIAD_S3_PUBLIC_ENDPOINT. |`
   - `docs/web.md` CSP paragraph: replace "A value that is not an http or https origin fails the build" with "Each entry is reduced to its origin (a trailing slash or path is dropped); an entry that is not an http or https URL fails the build, and a cloud build with no entry warns."
5. Optional. Add one assertion to `infra/test/e2e/run.sh` that `curl -sI http://localhost:$WEB_PORT/app` lists `http://localhost:$S3_PORT` in `content-security-policy`. That is the only existing flow using a non-default S3 port.

**Regression tests.**

Add to `apps/web/test/build-wiring.test.ts` (vitest globals are on: `apps/web/vitest.config.ts` sets `globals: true`):

```ts
describe("web image storage origins", () => {
  // The converter's CSP must allow the origin the API presigns object URLs with,
  // or the browser refuses every upload and download before it is sent.
  const compose = read("infra/compose/docker-compose.yml");
  const web = compose.slice(compose.indexOf("\n  web:\n"));
  const args = web.slice(web.indexOf("      args:\n"), web.indexOf("\n    image:"));
  const presigned = /^ {6}ARIAD_S3_PUBLIC_ENDPOINT: (\$\{ARIAD_S3_PUBLIC_ENDPOINT:-([^}]*)\})$/m.exec(
    compose,
  );

  it("the compose web build allows the endpoint the API presigns with", () => {
    expect(presigned, "the api service sets ARIAD_S3_PUBLIC_ENDPOINT").not.toBeNull();
    expect(args.split("\n")).toContain(
      `        ARIADSHIFT_WEB_STORAGE_ORIGINS: ${presigned?.[1]}`,
    );
  });

  it("the web image forwards the storage origins with the compose default", () => {
    const dockerfile = read("infra/docker/web.Dockerfile");
    expect(dockerfile.split("\n")).toContain(`ARG ARIADSHIFT_WEB_STORAGE_ORIGINS=${presigned?.[2]}`);
    expect(dockerfile).toMatch(/\bARIADSHIFT_WEB_STORAGE_ORIGINS=\$\{ARIADSHIFT_WEB_STORAGE_ORIGINS\}/);
  });
});
```

In `apps/web/test/headers.test.ts`, add `storageOriginsWarning` to the import from `../route-headers.mjs`. Replace the test `refuses storage entries that are not origins` (`:203-212`): its `"https://storage.example.com/bucket"` case is now accepted and reduced. Use these three tests instead:

```ts
  it("reduces storage entries to their origins", () => {
    const sources = cloudSources({
      NEXT_PUBLIC_ARIAD_API_URL: "https://api.example.com",
      ARIADSHIFT_WEB_STORAGE_ORIGINS:
        "https://S3.Example.com/ http://localhost:18333/bucket https://*.r2.example.com/",
    });
    expect(sources.connect).toEqual([
      "https://api.example.com",
      "https://s3.example.com",
      "http://localhost:18333",
      "https://*.r2.example.com",
    ]);
  });

  it("refuses storage entries that are not http or https origins", () => {
    for (const storage of ["https:", "*", "https://*", "ftp://storage.example.com", "'unsafe-eval'"]) {
      const env = {
        NEXT_PUBLIC_ARIAD_API_URL: "https://api.example.com",
        ARIADSHIFT_WEB_STORAGE_ORIGINS: storage,
      };
      expect(() => cloudSources(env)).toThrow(/ARIADSHIFT_WEB_STORAGE_ORIGINS/);
    }
  });

  it("warns about a cloud build without object-store origins", () => {
    expect(storageOriginsWarning({ NEXT_PUBLIC_ARIAD_API_URL: "https://api.example.com" })).toMatch(
      /ARIADSHIFT_WEB_STORAGE_ORIGINS/,
    );
    expect(storageOriginsWarning(CLOUD)).toBeNull();
    expect(storageOriginsWarning(NO_CLOUD)).toBeNull();
  });
```

Before the fix, the compose test fails because the arg is missing. The reduction test also fails: `"https://S3.Example.com/"` throws.

**Gate.**

```sh
pnpm --filter @ariadshift/web test
pnpm --filter @ariadshift/web typecheck
docker compose --env-file .env.example -f infra/compose/docker-compose.yml --profile core config --quiet
docker build -f infra/docker/web.Dockerfile --build-arg ARIADSHIFT_WEB_STORAGE_ORIGINS=http://localhost:18333/ -t ariadshift-web:e1 .
```

After the build, check that the `/app` CSP lists `http://localhost:18333`.

**Commit.** `fix(web): allow the compose S3 endpoint in the converter's CSP`

**Risks.** Accepting a path is a behaviour change, but it only drops the path, so the policy is never wider than the origin. The GHCR image is still built with the localhost default (SAS Q5), so it works only for operators who rebuild.

### 3.2 E2: the web image does not build (Medium; partly valid)

**Current behaviour.**

- On today's dev, `infra/docker/web.Dockerfile` builds only `-p ariad-wasm` and installs no `unzip`. `git grep ariad-typst\|unzip origin/dev -- infra/docker/web.Dockerfile` finds nothing.
- Meanwhile `apps/web` imports `@ariadshift/ariad-typst` (`origin/dev:apps/web/src/converter/browser-engines.ts:8,26`; `package.json:25`), and `apps/web/scripts/sync-pandoc.mjs:41` runs `unzip`.
- Cloud-product fixes both in `4eaba39` and `c19ea77`.

**Change.** None of its own on the re-land base. Cloud-product's fix arrives with step 2, and E2 becomes the pick #6 resolution in §3.0. Do not port cloud-product's two commits separately.

**Test.** `apps/web/test/build-wiring.test.ts` "web image browser engines" (`origin/feat/cloud-product:…:122-134`) already guards the Typst build, but after pick #6 it no longer works: see E8 (b). The real proof is `docker build` plus `curl` (§3.0 gate).

**Commit.** Part of the `e3c2b21` pick.

### 3.3 E4: a download slot is held after EOF (Medium, unverified at runtime)

**Current behaviour** (`origin/fix/v1-security-audit:crates/ariad-server/src/storage.rs:690-694`, which is `:673` in MFPF's `90787ce` numbering).

- After the last chunk, the producer sends `None` with the idle timeout, then runs `sender.closed().await`. That wait has no idle limit and no rate floor.
- A client that reads every chunk and then stops reading leaves the `None` queued, and the receiver stays alive. The permit is then held until `timeout_at(deadline, …)` (`:696`): 5 minutes for a GET URL, or the 15-minute transfer timeout.
- The existing test `idle_backpressure_on_a_small_download_releases_admission` covers only the case where `None` cannot be queued.

**Change** (`storage.rs`, inside `get_stream_until`'s producer):

```diff
-                // EOF is explicit so the producer can retain admission until
-                // the consumer drains/drops its body, or the deadline fires.
-                if matches!(timeout(idle, sender.send(None)).await, Ok(Ok(()))) {
-                    sender.closed().await;
-                }
+                // EOF is explicit so the producer can retain admission until
+                // the consumer drains/drops its body. A consumer that stops
+                // reading after the last chunk gets the same idle limit as one
+                // that stops mid-transfer; the queued data still reaches it.
+                if matches!(timeout(idle, sender.send(None)).await, Ok(Ok(()))) {
+                    let _ = timeout(idle, sender.closed()).await;
+                }
```

**Regression test.** Append to `crates/ariad-server/tests/blob-transfers.rs`. It uses `storage`, `storage_with` and `BlobTransferLimits { .. }` from `90787ce`, and `assert_slot_released` and `assert_no_files` from dev.

```rust
#[tokio::test]
async fn download_left_unread_after_its_last_chunk_releases_admission() {
    let dir = tempfile::tempdir().unwrap();
    let hash = hex::encode(Sha256::digest(PAYLOAD));
    assert!(
        storage(dir.path(), 1, WAIT, WAIT)
            .put_stream("tail", chunks(PAYLOAD), PAYLOAD.len() as u64, &hash)
            .await
            .unwrap()
    );
    // Only the idle limit can release the slot: the transfer deadline and the
    // rate floor's grace period outlast the test.
    let strict = storage_with(
        dir.path(),
        BlobTransferLimits {
            max_concurrent: 1,
            idle_timeout: Duration::from_millis(200),
            transfer_timeout: Duration::from_secs(60),
            ..BlobTransferLimits::default()
        },
    );
    let (_, mut stream) = strict.get_stream("tail").await.unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap().as_ref(), PAYLOAD);
    // The end of the body is queued but never read, and the body stays open.
    assert_slot_released(&strict).await;
    drop(stream);
    strict.delete("tail").await.unwrap();
    assert_no_files(dir.path()).await;
}
```

Before the fix, `assert_slot_released` times out after `WAIT` (5 s) because the permit is held for 60 s, so the test panics.

**Gate.** `cargo nextest run -p ariad-server --test blob-transfers`, then `just test-server`.

**Commit.** `fix(server): release a download slot when the client stops reading at the end`

**Risks.** Low. Admission is released up to 15 s after EOF while hyper may still be flushing the buffered tail. That matches the idle-limit model in `docs/server.md:97`.

### 3.4 E5: one caller can hold a workspace's whole share (Medium; blocked on OD-9/OD-12)

**Current behaviour** (after `90787ce`; `origin/fix/v1-security-audit`).

- `admit(key, upload)` (`storage.rs:318-345`) counts per workspace taken from the object key. Every anonymous caller writes under the nil workspace (`db.rs:25`), so they all share 8 slots.
- GETs have no per-key or per-caller limit (`:651`), and blob routes skip the `Caller` rate limiter.
- V-SEC's corrected scenario: one anonymous IP opens 8 slow GETs of one output and starves all anonymous transfers.
- V-SEC's new finding N1: 8 free accounts × 8 GETs hold all 64 slots.
- The caller identity exists at presign time: `ip:{client}` for anonymous callers (`origin/dev:crates/ariad-server/src/auth/mod.rs:161`) and `ws:{workspace}` for signed-in ones (`:179`). It is not in the token today (`BlobClaims`, `storage.rs:105-123`).

**Relation to `2045-dev-fix-specs` SRV-1.** SRV-1 was written against dev without `90787ce`. Its configuration part (`ARIAD_BLOB_MAX_CONCURRENT`, `…_PER_SUBJECT` default 1, `ApiConfig.blob_limits`) collides with `90787ce`'s `ARIAD_BLOB_MAX_TRANSFERS*` and `ApiConfig.blob_transfers`. **Do not dispatch SRV-1 (that report's W2) on dev.** This item reuses SRV-1's claim design (§3 items 1–3 there: a keyed tag in claim `s`, legacy tokens without it) on top of `90787ce`. Only the deltas are specified here.

**Change.**

- **`config.rs`.**
  - `BlobTransferLimits` gains a doc comment ("Transfers in flight for one caller: an anonymous client address or a signed-in workspace") and the field `pub max_per_subject: usize`, default `4`.
  - `from_env` reads `ARIAD_BLOB_MAX_TRANSFERS_PER_SUBJECT` through `positive_or`.
- **`storage.rs`.**
  - `BlobClaims` gains `#[serde(rename = "s", default, skip_serializing_if = "Option::is_none")] pub subject: Option<String>` with the doc comment "Keyed tag of the caller the URL was issued to".
  - Add the helper below, in the style of `blob_url` (`origin/dev:storage.rs:655-670`):
    ```rust
    /// A keyed tag for the caller a local URL was issued to, so admission can
    /// count one caller's transfers without putting its address in the URL.
    fn subject_tag(secret: &[u8], subject: &str) -> Result<String, StorageError> {
        let mut mac =
            HmacSha256::new_from_slice(secret).map_err(|e| StorageError::Config(e.to_string()))?;
        mac.update(b"blob-subject:");
        mac.update(subject.as_bytes());
        Ok(hex::encode(&mac.finalize().into_bytes()[..16]))
    }
    ```
  - `presign_put(key, size, sha256_hex, subject: Option<&str>)` and `presign_get(key, subject: Option<&str>)`. In their `Signing::Local` arms, set `subject: subject.map(|subject| subject_tag(secret, subject)).transpose()?`. The S3 arms ignore the argument.
  - `ActiveTransfers` gains `subjects: HashMap<String, usize>`. `TransferPermit` gains `subject: Option<String>`. Factor the existing workspace decrement in `Drop` into `fn release(counts: &mut HashMap<String, usize>, holder: Option<&String>)` and call it for both maps.
  - `admit(&self, key: &str, upload: bool, subject: Option<&str>)` refuses with `Busy` when `subject` already holds `max_per_subject` slots. It increments the subject count next to the workspace count.
  - `put_stream_until(…, expires, subject: Option<&str>)` and `get_stream_until(key, expires, subject: Option<&str>)` pass the subject to `admit`. `put_stream` and `get_stream` pass `None`.
  - `with_blob_transfer_limits` also rejects `max_per_subject == 0`.
- **Callers.**
  - `api/blobs.rs`: `put_blob` (`origin/dev:…:89`) and `get_blob` (`:135`) pass `claims.subject.as_deref()`.
  - `api/uploads.rs:132`: `presign_put(&key, request.size, &sha256, Some(&caller.principal.subject))`.
  - `api/jobs.rs:394` (`:409` on hardening): `presign_get(&artifact.storage_key, Some(&caller.principal.subject))`.
  - Tests pass `None`: `storage.rs:710`, `tests/blob-transfers.rs:117,225,247,305,418`, `tests/s3_storage.rs:294,339`.
  - Hardening's MCP `put_stream` call (`mcp/tools.rs:205`) is unchanged.
- **`blobs.rs` utoipa.** Both 429 descriptions become "The transfer budget, a workspace's or caller's share, or the upload key is busy" (GET: "The transfer budget or a workspace's or caller's share is busy"). Then run `just openapi`, refresh the snapshot if needed, and run `pnpm --filter @ariadshift/sdk lock`.
- **`docs/server.md`.**
  - In `:97`, after the per-workspace clause, add: "and one caller (an anonymous client address, IPv6 grouped by /64, or a signed-in workspace) at most `ARIAD_BLOB_MAX_TRANSFERS_PER_SUBJECT` (default 4). The caller is the one a URL was issued to. Someone with many accounts or addresses still needs ingress limits per client."
  - Add an env table row after `…_PER_WORKSPACE`.
- PUT replay inside the URL window stays as designed (`sweeper.rs:135-139`), unless OD-12 says otherwise.

**Regression tests.**

Append to `crates/ariad-server/tests/blob-transfers.rs`. `workspace_key` and `held_stream` come from `90787ce`.

```rust
#[tokio::test]
async fn one_caller_cannot_take_the_anonymous_workspace_share() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage_with(
        dir.path(),
        BlobTransferLimits {
            max_concurrent: 8,
            max_per_workspace: 4,
            max_per_subject: 2,
            idle_timeout: WAIT,
            transfer_timeout: WAIT,
            ..BlobTransferLimits::default()
        },
    );
    // Every anonymous caller writes under the nil workspace.
    let mut held = Vec::new();
    for owner in 1..=2 {
        let (body, observed) = held_stream();
        let storage = storage.clone();
        let key = workspace_key(0, owner);
        held.push(tokio::spawn(async move {
            storage
                .put_stream_until(&key, body, 2, &"0".repeat(64), None, Some("peer-a"))
                .await
        }));
        timeout(WAIT, observed).await.unwrap().unwrap();
    }
    // The same caller is refused in either direction...
    assert!(matches!(
        storage
            .get_stream_until(&workspace_key(0, 3), None, Some("peer-a"))
            .await,
        Err(StorageError::Busy)
    ));
    // ...while another caller in the same workspace is still admitted.
    match storage
        .get_stream_until(&workspace_key(0, 3), None, Some("peer-b"))
        .await
    {
        Err(err) if err.is_not_found() => {}
        _ => panic!("another caller should be admitted"),
    }
    for task in held {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
    assert_no_files(dir.path()).await;
}

#[tokio::test]
async fn one_download_url_opened_many_times_is_refused_past_the_caller_share() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage_with(
        dir.path(),
        BlobTransferLimits {
            max_concurrent: 8,
            max_per_workspace: 8,
            max_per_subject: 2,
            idle_timeout: WAIT,
            transfer_timeout: WAIT,
            ..BlobTransferLimits::default()
        },
    );
    let key = workspace_key(5, 1);
    // Several backend chunks, so an unread response keeps its slot.
    let payload = Bytes::from(vec![b'x'; 3 * 1024 * 1024]);
    let hash = hex::encode(Sha256::digest(&payload));
    let body = stream::iter([Ok::<_, Infallible>(payload.clone())]);
    assert!(
        storage
            .put_stream(&key, body, payload.len() as u64, &hash)
            .await
            .unwrap()
    );
    let router = app(storage.clone());
    let open = |path: String| {
        router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
    };
    let (url, _) = storage.presign_get(&key, Some("ws:attacker")).await.unwrap();
    let first = open(url.path().to_owned()).await.unwrap();
    let second = open(url.path().to_owned()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    let third = open(url.path().to_owned()).await.unwrap();
    assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);
    // Another caller's URL for the same file is admitted.
    let (other, _) = storage.presign_get(&key, Some("ws:someone-else")).await.unwrap();
    let theirs = open(other.path().to_owned()).await.unwrap();
    assert_eq!(theirs.status(), StatusCode::OK);
    // A URL without a caller tag, as issued before this change, is bounded by
    // the workspace share only.
    let (legacy, _) = storage.presign_get(&key, None).await.unwrap();
    let legacy_first = open(legacy.path().to_owned()).await.unwrap();
    let legacy_second = open(legacy.path().to_owned()).await.unwrap();
    let legacy_third = open(legacy.path().to_owned()).await.unwrap();
    for response in [&legacy_first, &legacy_second, &legacy_third] {
        assert_eq!(response.status(), StatusCode::OK);
    }
    drop((first, second, third, theirs, legacy_first, legacy_second, legacy_third));
    assert_slot_released(&storage).await;
    storage.delete(&key).await.unwrap();
    assert_no_files(dir.path()).await;
}
```

In `crates/ariad-server/src/config.rs` `mod tests` (`base()` and `with()` exist at `:491`, `:502`):

```rust
    #[test]
    fn blob_transfer_caller_share_reads_a_positive_override() {
        let defaults = ApiConfig::from_env(&Env::from_pairs(base())).unwrap();
        assert_eq!(defaults.blob_transfers.max_per_subject, 4);
        let pairs = with(base(), "ARIAD_BLOB_MAX_TRANSFERS_PER_SUBJECT", "2");
        let config = ApiConfig::from_env(&Env::from_pairs(pairs)).unwrap();
        assert_eq!(config.blob_transfers.max_per_subject, 2);
        for value in ["0", "-1", "many"] {
            let pairs = with(base(), "ARIAD_BLOB_MAX_TRANSFERS_PER_SUBJECT", value);
            assert!(ApiConfig::from_env(&Env::from_pairs(pairs)).is_err());
        }
    }
```

In `crates/ariad-server/src/storage.rs` `mod tests` (`local()` at `:682`):

```rust
    #[tokio::test]
    async fn local_urls_carry_a_keyed_caller_tag_not_the_caller() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = local(dir.path());
        let token = |url: &Url| url.path().rsplit('/').next().expect("token").to_owned();
        let (url, _) = storage
            .presign_get("t/a/b/out/c", Some("ip:203.0.113.7"))
            .await
            .expect("presign");
        assert!(!url.as_str().contains("203.0.113.7"));
        let tag = storage
            .verify_blob_token(&token(&url), Utc::now())
            .and_then(|claims| claims.subject)
            .expect("tag");
        assert_eq!(tag.len(), 32);
        let (again, _) = storage
            .presign_get("t/a/b/out/c", Some("ip:203.0.113.7"))
            .await
            .expect("presign");
        let same = storage.verify_blob_token(&token(&again), Utc::now()).expect("claims");
        assert_eq!(same.subject.as_deref(), Some(tag.as_str()));
    }
```

Before the fix these do not compile, because the subject API does not exist. The behaviour each test asserts is the defect: before the fix, a third open of one URL is admitted.

**Gate.**

```sh
cargo nextest run -p ariad-server --lib --test blob-transfers --test openapi_drift --test openapi_v1_snapshot
just sdk-check
just test-server
```

**Commit.** `fix(server): limit the blob transfer share of one caller`

**Risks.**

- The token format grows, but serde's default keeps it backward compatible, and tokens live at most 15 minutes.
- Signed-in workspaces drop from 8 to 4 concurrent `file://` transfers. Web uploads use Uppy with `limit: 1` (`apps/web/src/lib/cloud/transfer.ts:40`), so the web is unaffected. SDK callers get E8's 429 retry.
- Callers with many accounts or addresses remain a residual. Record it under OD-12.
- Only `file://` deployments are affected; compose uses S3.

### 3.5 E6: concurrent `complete` calls hash the same object repeatedly (Medium)

**Current behaviour.**

- `complete_upload` (`origin/dev:crates/ariad-server/src/api/uploads.rs:186-234`) checks `Pending` (`:200`), then streams the whole object through `Storage::digest` (`:213`) with no in-flight guard.
- N parallel calls on a pending 2 GiB upload each read 2 GiB, on every backend including S3 (SAS F6). Only the final `db::complete_upload` is conditional (`WHERE status = 'pending'`).

**Design correction.** MFPF suggests `UPDATE … SET status='verifying'`. The `uploads.status` CHECK allows only `('pending', 'completed', 'deleted', 'expired')` (`migrations/20261022000001_cloud_schema.sql:70`), so that needs a migration. The sweeper, the job admission and the crash recovery would also have to learn about `verifying` rows: that is a data-model change. A per-process guard bounds the cost with no schema change: one digest per object at a time, plus a cap on digests in flight. Across replicas, the bound becomes "one per object per replica". If the owner wants a cluster-wide bound, add a lease column `verifying_until` in a follow-up migration.

**Change.**

`storage.rs`:

```rust
/// Uploads verified at once per process; further callers wait for a slot.
pub const MAX_CONCURRENT_DIGESTS: usize = 4;

#[derive(Debug)]
struct DigestSlots {
    keys: Mutex<HashSet<String>>,
    permits: Arc<tokio::sync::Semaphore>,
}

impl Default for DigestSlots {
    fn default() -> Self {
        Self {
            keys: Mutex::default(),
            permits: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DIGESTS)),
        }
    }
}

/// Marks one object as being verified; dropped on return, error or cancellation.
struct DigestKey {
    slots: Arc<DigestSlots>,
    key: String,
}

impl Drop for DigestKey {
    fn drop(&mut self) {
        self.slots
            .keys
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&self.key);
    }
}

/// Held while one upload is verified.
pub struct DigestClaim {
    _key: DigestKey,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
```

- `Storage` gains the field `digests: Arc<DigestSlots>`. Set it to `Arc::default()` in both `from_config` arms (`origin/dev:storage.rs:199-205`, `:217-223`).
- `StorageError` gains `#[error("the object is already being verified")] DigestInProgress`. No match on `StorageError` outside `storage.rs` is exhaustive (checked on dev, cloud-product and hardening).
- Add the method:

```rust
    /// Claims `key` for verification. A second claim on the same object fails
    /// at once; claims beyond [`MAX_CONCURRENT_DIGESTS`] wait for a slot.
    pub async fn claim_digest(&self, key: &str) -> Result<DigestClaim, StorageError> {
        let slots = Arc::clone(&self.digests);
        if !slots
            .keys
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key.to_owned())
        {
            return Err(StorageError::DigestInProgress);
        }
        let marker = DigestKey {
            slots: Arc::clone(&slots),
            key: key.to_owned(),
        };
        let permit = Arc::clone(&slots.permits)
            .acquire_owned()
            .await
            .map_err(|_| StorageError::Busy)?;
        Ok(DigestClaim {
            _key: marker,
            _permit: permit,
        })
    }
```

`api/uploads.rs`. Factor the expiry and status checks (`:196-205`) into one helper and run them before and after the claim:

```rust
/// The answer for an upload that needs no verification: `Some` when it is
/// already completed, an error when it expired or is gone, `None` when pending.
fn settled(row: &UploadRow) -> ApiResult<Option<Upload>> {
    if row.expires_at <= chrono::Utc::now() {
        return Err(ApiError::conflict("the upload has expired"));
    }
    match row.status() {
        UploadStatus::Pending => Ok(None),
        UploadStatus::Completed => Ok(Some(upload_dto(row, None))),
        UploadStatus::Deleted | UploadStatus::Expired => {
            Err(ApiError::conflict("the upload is no longer available"))
        }
    }
}
```

In `complete_upload`, after the access-filtered `get_upload`:

```rust
    if let Some(done) = settled(&row)? {
        return Ok(Json(done));
    }
    // One verification per upload at a time: a concurrent call would stream
    // the whole object from storage again for the same answer.
    let _claim = match state.storage.claim_digest(&row.storage_key).await {
        Ok(claim) => claim,
        Err(StorageError::DigestInProgress) => {
            return Err(ApiError::conflict("the upload is already being verified"));
        }
        Err(err) => return Err(ApiError::internal(&err)),
    };
    // A call that finished while this one waited for a slot has verified it.
    let row = db::get_upload(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("upload"))?;
    if let Some(done) = settled(&row)? {
        return Ok(Json(done));
    }
```

The size, digest and `db::complete_upload` steps then follow unchanged. Change the 409 utoipa description to "The file is missing, the upload is no longer live, or another call is verifying it". Then regenerate the spec, snapshot and SDK lock.

**Regression test** (`storage.rs` `mod tests`):

```rust
    #[tokio::test]
    async fn one_digest_per_object_and_a_bounded_number_in_flight() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = local(dir.path());
        let first = storage.claim_digest("t/a/0/in/x").await.expect("claim");
        assert!(matches!(
            storage.claim_digest("t/a/0/in/x").await,
            Err(StorageError::DigestInProgress)
        ));
        let mut others = Vec::new();
        for n in 1..MAX_CONCURRENT_DIGESTS {
            others.push(storage.claim_digest(&format!("t/a/{n}/in/x")).await.expect("claim"));
        }
        // Every slot is taken: the next object waits instead of failing.
        let waiting = tokio::spawn({
            let storage = storage.clone();
            async move { storage.claim_digest("t/a/9/in/x").await.map(drop) }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiting.is_finished());
        drop(first);
        timeout(Duration::from_secs(5), waiting)
            .await
            .expect("admitted once a slot is free")
            .expect("task")
            .expect("claim");
        drop(others);
        assert!(storage.claim_digest("t/a/0/in/x").await.is_ok());
    }
```

The handler mapping (409, and the re-read after waiting) needs a database. Cover it in `just test-server` by adding a case to the existing upload tests that run under `ARIAD_TEST_DATABASE_URL`: two concurrent `complete` calls on one pending upload give one 200 and one 409 or 200, and never two digests. That file belongs to hardening's test layout, so add it on the re-land base.

**Gate.**

```sh
cargo nextest run -p ariad-server --lib --test openapi_drift --test openapi_v1_snapshot
just sdk-check
just test-server
```

**Commit.** `fix(server): verify each upload once at a time and bound concurrent digests`

**Risks.**

- Waiting callers hold an HTTP connection. `complete` is exempt from the request timeout (`docs/server.md`, `ARIAD_REQUEST_TIMEOUT_S` row).
- The SDK's `completeUpload` does not retry a 409, but a concurrent duplicate `complete` from one client is rare.
- Multi-replica deployments keep one digest per object per replica.

### 3.6 E7: the mocked cloud spec is blocked by the new CSP (Medium, test only)

**Current behaviour.**

- `apps/web/e2e/cloud.spec.ts:12` (`origin/feat/cloud-product`) hard-codes `STORAGE = "https://storage.cloud.test"`. The mocks route `${STORAGE}/**` (`:136`, `:215`, `:227`).
- Playwright does not bypass the CSP (`playwright.config.ts` has no `bypassCSP`). `next start` serves the CSP compiled at build time, which lists storage origins only from `ARIADSHIFT_WEB_STORAGE_ORIGINS`.
- A cloud build made as the spec header says (`:1-4`, API URL only) therefore fails at the upload, before `page.route` sees the request.

**Change.**

```diff
 // Cloud mode against a mocked API, auth service and object store. Cloud mode
 // is configured at build time, so this spec runs only against a build made
 // with NEXT_PUBLIC_ARIAD_API_URL set (any unresolvable origin works, for
-// example https://api.cloud.test); every request to it is answered here.
+// example https://api.cloud.test); every request to it is answered here. The
+// build must also list the mocked object store in ARIADSHIFT_WEB_STORAGE_ORIGINS
+// (for example https://storage.cloud.test), or the converter's CSP blocks the
+// presigned PUT before page.route sees it.
@@
-const STORAGE = "https://storage.cloud.test";
+const STORAGE =
+  process.env.ARIADSHIFT_WEB_STORAGE_ORIGINS?.split(/\s+/)
+    .find((origin) => origin !== "" && !origin.includes("*"))
+    ?.replace(/\/+$/, "") ?? "https://storage.cloud.test";
```

**Regression test.** Add it after `test.skip(!API, …)`:

```ts
test("builds the converter's CSP with the mocked object store", async ({ page }) => {
  const response = await page.goto("/app");
  const connect = (response?.headers()["content-security-policy"] ?? "")
    .split(";")
    .map((directive) => directive.trim().split(/\s+/))
    .find(([name]) => name === "connect-src");
  expect(connect, `rebuild with ARIADSHIFT_WEB_STORAGE_ORIGINS=${STORAGE}`).toContain(STORAGE);
});
```

On a cloud build without the variable, this test fails with the rebuild hint, and the upload tests fail at the CSP as today. With the variable set, every cloud test reaches its mocks.

**Gate.**

```sh
export NEXT_PUBLIC_ARIAD_API_URL=https://api.cloud.test NEXT_PUBLIC_ARIAD_AUTH_URL=https://auth.cloud.test ARIADSHIFT_WEB_STORAGE_ORIGINS=https://storage.cloud.test
just web-build && just web-e2e
```

Afterwards, run `just web-build` again without these variables so that later runs do not reuse the cloud build.

**Commit.** `test(web): build the mocked cloud spec with its storage origin in the CSP`

**Risks.** None at runtime. The spec still skips in CI (MFPF §8).

### 3.7 E8: low-severity bundle (Low)

Each sub-item is a separate commit.

**(a) Stale threat model** (`85dcee0:docs/security/threat-model.md`, lines 81, 83 and 97).

- Line 81 says the header source "lacks a CSP and general frame/nosniff/referrer policy". Replace it with a description of the shipped policy: three page profiles (site, account, converter), hashed inline scripts on the static export, a worker policy on chunks and `/sw.js`, `frame-ancestors`/`base-uri`/`object-src 'none'`, and nosniff, referrer and permissions headers. Link `docs/web.md#security-headers`.
- Line 83 says the cache handler has no allowlist. Replace it with: the service worker answers only same-origin requests (`service-worker.js:92` returns early for other origins), stores only what `apps/web/offline-policy.mjs` admits, and never stores `/api/auth/`, `/v1/` or presigned URLs.
- Line 97 says "one open high finding and two medium findings". Replace it with the post-re-land status: fixed, with the test runs that prove it.
- Commit: `docs(security): describe the shipped CSP and offline allowlist in the threat model`.

**(b) Vacuous Dockerfile check** (`apps/web/test/build-wiring.test.ts:122-134`, cloud-product).

- After pick #6, `indexOf("RUN pnpm --filter @ariadshift/web export")` returns -1. `slice(0, -1)` then covers almost the whole file, so the test can no longer catch bindgen moving after the export.
- Change: `const exportAt = dockerfile.indexOf("pnpm --filter @ariadshift/web export"); expect(exportAt, "the export step").toBeGreaterThan(0); const beforeExport = dockerfile.slice(0, exportAt);`.
- Proof: the new `toBeGreaterThan(0)` assertion fails if the step is renamed again.
- Commit: `test(web): keep the web image engine check tied to the export step`.

**(c) Unchecked nginx page paths** (`route-headers.mjs:342-348`).

- `nginxServer` validates header keys and values (`:316-321`) but writes page paths into `location = …` unchecked.
- Add:
  ```js
  /** @param {string} path */
  function nginxPath(path) {
    if (!/^\/[A-Za-z0-9._~/-]*$/.test(path)) {
      throw new Error(`page path cannot be written to nginx: ${path}`);
    }
    return path;
  }
  ```
  Use it for both `pretty` and `source` in `pageLocations`.
- Test (`headers.test.ts`, "static export headers"):
  ```ts
  it("refuses page paths nginx would misread", () => {
    for (const file of ["bad path.html", "semi;colon.html", "brace{.html", 'quote".html']) {
      const unsafe = staticHeaderRules({ pages: [{ file, html: "<p></p>" }], env: NO_CLOUD });
      expect(() => nginxServer(unsafe)).toThrow(/nginx/);
    }
  });
  ```
- Commit: `fix(web): refuse page paths the generated nginx server would misread`.

**(d) `/index` served without a CSP** (`route-headers.mjs:365`).

- The catch-all `try_files $uri $uri.html $uri/ =404` serves `/index` from `index.html` with the common headers only.
- Every exported page already has exact `=` locations, so change it to `...location("/", [], "$uri =404")`.
- Test:
  ```ts
  it("serves no page through the catch-all location, so every page keeps its CSP", () => {
    const conf = nginxServer(staticHeaderRules({ pages: PAGES, env: NO_CLOUD }));
    const catchAll = conf.split("    location ").find((block) => block.startsWith("/ {"));
    expect(catchAll).toContain("try_files $uri =404;");
    expect(catchAll).not.toContain("$uri.html");
  });
  ```
- Commit: `fix(web): serve exported pages only through their own header locations`.

**(e) Netlify 404 page.** Accepted gap (SAW 6). Add to `docs/web.md` "Static hosting": "On Netlify, an unknown path serves `404.html` under the catch-all rule only, without a page CSP. Netlify joins the values of every matching rule, so a catch-all CSP would also apply to every page and refuse their hashed scripts. The nginx image routes 404s to the `404.html` location, which has its own policy." Commit: `docs(web): record the CSP gap on Netlify 404 pages`.

**(f) Worker policy without `default-src`** (`route-headers.mjs:65`; `docs/web.md:86` says "leaves fetches open").

- Change to `"default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; object-src 'none'; base-uri 'none'"`. The service worker ignores cross-origin requests (`service-worker.js:92`), and engines load from this origin.
- In `headers.test.ts` "lets worker scripts compile WebAssembly and nothing else", add `expect(policy.get("default-src")).toEqual(["'self'"]);`. It fails before the change.
- Rewrite the `docs/web.md:86` sentence to "limits fetches to this origin".
- **Gate: E9's project must pass**, because it converts DOCX and PDF under the static CSP and reloads offline. Land this commit last, in R2's branch, after R4 is merged.
- Commit: `fix(web): limit worker scripts to same-origin fetches`.

**(g) Test picks an entry by position** (`headers.test.ts:259-263`).

- `dev[1]` is now `/account` (after `be8da76`), not `/app`. Select by source:
  ```ts
  it("adds eval in development", () => {
    const dev = serverRouteHeaders({ env: NO_CLOUD, dev: true });
    const devEntry = (source: string) => dev.find((candidate) => candidate.source === source);
    for (const source of ["/:path*", "/account", "/app"]) {
      expect(cspOf(devEntry(source)?.headers).get("script-src")).toContain("'unsafe-eval'");
    }
    expect(cspOf(entry("/app")?.headers).get("script-src")).not.toContain("'unsafe-eval'");
  });
  ```
- Commit: `test(web): select header entries by source in the development policy test`.

**(h) SDK does not retry busy transfers** (`origin/feat/cloud-product:packages/sdk/src/client.ts:231-242`).

- Every `UploadBody` (`Blob | ArrayBuffer | Uint8Array`, `:45`) can be sent again.
- Change:
  ```ts
  /** Presigned transfers retried when the local store is busy (429) or timed out (408). */
  const TRANSFER_RETRY_STATUSES = new Set([408, 429]);
  const TRANSFER_ATTEMPTS = 3;
  ```
  ```ts
      async transfer(presigned, body, signal) {
        // Every upload body can be sent again, so a busy or timed-out local
        // store gets a short retry instead of failing the upload.
        for (let attempt = 1; ; attempt += 1) {
          const response = await fetchImpl(presigned.url, {
            method: presigned.method,
            headers: presigned.headers,
            body,
            signal,
          });
          if (response.ok) {
            await response.body?.cancel();
            return;
          }
          if (attempt >= TRANSFER_ATTEMPTS || !TRANSFER_RETRY_STATUSES.has(response.status)) {
            throw await errorFromResponse(response, "transfer_failed");
          }
          await response.body?.cancel();
          await sleep((retryAfterSeconds(response) ?? 1) * 1_000, signal);
        }
      },
  ```
- Tests (`packages/sdk/test/client.test.ts`, "upload" describe; helpers from `./mock-fetch`):
  ```ts
  it("retries a presigned transfer the store refused as busy or timed out", async () => {
    const mock = mockFetch(
      () => apiError(429, "rate_limited", "busy", { "retry-after": "0" }),
      () => apiError(408, "transfer_timeout", "timed out", { "retry-after": "0" }),
      () => new Response(null, { status: 200 }),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    const presigned = uploadBody("pending").upload;
    if (!presigned) throw new Error("fixture has no presigned request");

    await client.transfer(presigned, hello());

    expect(mock.requests).toHaveLength(3);
    expect(mock.pending()).toBe(0);
  });

  it("gives up on a busy store after the last attempt", async () => {
    const busy = () => apiError(429, "rate_limited", "busy", { "retry-after": "0" });
    const mock = mockFetch(busy, busy, busy);
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    const presigned = uploadBody("pending").upload;
    if (!presigned) throw new Error("fixture has no presigned request");

    await expect(client.transfer(presigned, hello())).rejects.toMatchObject({
      status: 429,
      code: "rate_limited",
    });
    expect(mock.requests).toHaveLength(3);
  });
  ```
- Add one sentence to the SDK paragraph of `docs/api.md`. R3 owns that file.
- Commit: `fix(sdk): retry presigned transfers when the store is busy or timed out`.

**(i) Blob downloads lack `nosniff` and `attachment`** (Info; `origin/dev:crates/ariad-server/src/api/blobs.rs:154`).

- After the `CACHE_CONTROL` insert, add `X_CONTENT_TYPE_OPTIONS: nosniff` and `CONTENT_DISPOSITION: attachment`.
- Test: in `replay_and_global_overload_are_refused_before_polling_the_body` (`tests/blob-transfers.rs`), after the `cache-control` assertion, add `assert_eq!(get.headers()["x-content-type-options"], "nosniff"); assert_eq!(get.headers()["content-disposition"], "attachment");`.
- The web downloads with `fetch`, which ignores `Content-Disposition`.
- Commit: `fix(server): mark blob downloads as attachments and disable sniffing`.

**Gate (all of E8).**

```sh
pnpm --filter @ariadshift/web test
pnpm --filter @ariadshift/sdk test
cargo nextest run -p ariad-server --test blob-transfers
just web-e2e-static   # for (f)
```

### 3.8 E9: no browser run under the hashed static-export CSP (Medium)

**Current behaviour.**

- Playwright runs only against `next start` (`origin/feat/cloud-product:apps/web/playwright.config.ts:3,21-26`), which sends `'unsafe-inline'`.
- The hashed policy that the nginx image and Netlify serve has no browser coverage. A missed inline-script hash, a blocked origin (E1) or a worker fetch blocked by (f) would ship green.
- The export and `next start` cannot share one `.next` directory (`scripts/export.mjs` runs `next build` with `ARIADSHIFT_WEB_OUTPUT=export`), so this needs its own config and recipe rather than another project in the main config.

**Change (new files).**

`apps/web/scripts/serve-static-export.mjs`:

```js
// Serves apps/web/out/ the way a static host does, with the response headers from
// out/_headers (Netlify's format, written by scripts/write-static-headers.mjs), so a
// browser test runs under the hashed CSP the deployments use instead of `next start`'s.
// Usage: node scripts/serve-static-export.mjs [port]
import { createReadStream, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";

const out = path.join(import.meta.dirname, "../out");
const port = Number(process.argv[2] ?? 3300);
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
  ".webmanifest": "application/manifest+json",
  ".xml": "application/xml",
};

/** `_headers` blocks: a path line, then indented `Key: Value` lines. */
function parseHeaders(text) {
  const rules = [];
  for (const line of text.split("\n")) {
    if (!line.trim() || line.startsWith("#")) continue;
    if (!/^\s/.test(line)) {
      rules.push({ source: line.trim(), headers: [] });
      continue;
    }
    const colon = line.indexOf(":");
    rules.at(-1)?.headers.push([line.slice(0, colon).trim(), line.slice(colon + 1).trim()]);
  }
  return rules;
}

const rules = parseHeaders(readFileSync(path.join(out, "_headers"), "utf8"));
const matches = (source, pathname) =>
  source.endsWith("/*") ? pathname.startsWith(source.slice(0, -1)) : source === pathname;

function resolve(pathname) {
  for (const candidate of [pathname, `${pathname}.html`, path.posix.join(pathname, "index.html")]) {
    const file = path.join(out, candidate);
    if (path.relative(out, file).startsWith("..")) return null;
    if (statSync(file, { throwIfNoEntry: false })?.isFile()) return file;
  }
  return null;
}

createServer((request, response) => {
  const pathname = decodeURIComponent(new URL(request.url ?? "/", "http://localhost").pathname);
  const file = resolve(pathname);
  // Like Netlify, an unknown path gets only the rules that match it.
  for (const rule of rules) {
    if (matches(rule.source, pathname)) {
      for (const [key, value] of rule.headers) response.setHeader(key, value);
    }
  }
  const served = file ?? path.join(out, "404.html");
  response.statusCode = file ? 200 : 404;
  response.setHeader("Content-Type", TYPES[path.extname(served)] ?? "application/octet-stream");
  createReadStream(served).pipe(response);
}).listen(port, "127.0.0.1", () => console.log(`static export on http://localhost:${port}`));
```

`apps/web/playwright.static.config.ts`:

```ts
import { defineConfig, devices } from "@playwright/test";

// The static export under the headers it ships with: hashed inline scripts and no
// 'unsafe-inline'. `just web-e2e-static` exports the site first.
const PORT = 3300;

export default defineConfig({
  testDir: "e2e-static",
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  use: { baseURL: `http://localhost:${PORT}`, trace: "retain-on-failure" },
  projects: [{ name: "chromium-static-csp", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: `node scripts/serve-static-export.mjs ${PORT}`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
```

`apps/web/e2e-static/static-csp.spec.ts`. The helpers mirror `e2e/heavy-routes.spec.ts:7-34` and `e2e/offline.spec.ts`; the fixture and texts are the ones those specs use.

```ts
import { readFile } from "node:fs/promises";
import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

test.setTimeout(180_000);

const fixture = (name: string) => path.resolve(import.meta.dirname, "../../../fixtures", name);

/** Records every CSP violation the page reports, from its first script on. */
async function watchViolations(page: Page): Promise<string[]> {
  const seen: string[] = [];
  await page.exposeBinding("reportCspViolation", (_source, text: string) => {
    seen.push(text);
  });
  await page.addInitScript(() => {
    document.addEventListener("securitypolicyviolation", (event) => {
      (window as unknown as { reportCspViolation(text: string): void }).reportCspViolation(
        `${event.effectiveDirective} blocked ${event.blockedURI || "inline"} on ${location.pathname}`,
      );
    });
  });
  page.on("console", (message) => {
    if (/Content[- ]Security[- ]Policy/i.test(message.text())) seen.push(message.text());
  });
  return seen;
}

function scriptSources(policy: string | undefined): string[] {
  const directive = (policy ?? "")
    .split(";")
    .map((part) => part.trim().split(/\s+/))
    .find(([name]) => name === "script-src");
  return directive?.slice(1) ?? [];
}

async function pick(page: Page, name: string, buffer: Buffer) {
  await page.locator("input[type=file]").setInputFiles({
    name,
    buffer,
    mimeType: "application/octet-stream",
  });
  await expect(page.getByTestId("input-name")).toHaveText(name);
}

async function convert(page: Page, label: string): Promise<Buffer> {
  await page.getByTestId("target-select").click();
  await page.getByRole("option", { name: label, exact: true }).click();
  await page.getByRole("button", { name: "Convert", exact: true }).click();
  await expect(page.getByTestId("result")).toBeVisible({ timeout: 120_000 });
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: /^Download / }).click();
  return readFile(await (await download).path());
}

for (const route of ["/", "/sign-in", "/convert/markdown-to-html"]) {
  test(`${route} runs under its hashed policy`, async ({ page }) => {
    const seen = await watchViolations(page);
    const response = await page.goto(route);
    const sources = scriptSources(response?.headers()["content-security-policy"]);
    expect(sources).toContain("'self'");
    expect(sources).not.toContain("'unsafe-inline'");
    expect(sources.some((source) => source.startsWith("'sha256-"))).toBe(true);
    await page.waitForLoadState("networkidle");
    expect(seen).toEqual([]);
  });
}

test("/app converts DOCX and writes PDF under its policy, then reloads offline", async ({
  page,
  context,
}) => {
  const seen = await watchViolations(page);
  const response = await page.goto("/app");
  expect(scriptSources(response?.headers()["content-security-policy"])).toContain(
    "'wasm-unsafe-eval'",
  );
  await page.waitForFunction(() => navigator.serviceWorker.controller !== null);
  await pick(page, "route.docx", await readFile(fixture("docx/vi-route-table.docx")));
  expect((await convert(page, "Markdown")).toString()).toContain("Bảng hành trình ven sông");
  await page.getByRole("button", { name: "Start over and delete files" }).click();
  await pick(page, "static.md", Buffer.from("# Static policy\n\nTiếng Việt.\n"));
  expect((await convert(page, "PDF")).subarray(0, 5).toString()).toBe("%PDF-");
  await expect
    .poll(() =>
      page.evaluate(async () =>
        (await (await caches.open("ariadshift-app-v1")).keys()).map(
          (request) => new URL(request.url).pathname,
        ),
      ),
    )
    .toContain("/app");
  await context.setOffline(true);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Convert a document" })).toBeVisible();
  await context.setOffline(false);
  expect(seen).toEqual([]);
});
```

`justfile`, next to `web-e2e`:

```just
# The static export served with its generated headers, in Chromium; fails on any CSP violation.
web-e2e-static: web-export
    pnpm --filter @ariadshift/web exec playwright test -c playwright.static.config.ts
```

Add the recipe to `docs/ci.md`. Whether CI runs it is OD-15. The cheapest option is a step in the `web` job after `just web-e2e`, Chromium only.

**Regression proof.** The project fails today for each of these cases:

- Remove one hash from `inlineScriptHashes` output: the `/` test reports a `script-src` violation.
- Set `default-src 'none'` in the worker policy: the `/app` conversion fails.
- Build cloud mode with storage pointing elsewhere: covered by E1's unit tests, not by this project.

**Gate.** `just web-e2e-static`, then the §3.0 gate.

**Commit.** `test(web): check the static export under its generated CSP in a browser`

**Risks.**

- The Node server reproduces Netlify's matching, not nginx's. Both serialise the same `staticHeaderRules` table, and `headers.test.ts` checks both serialisations. The nginx image is also covered by the §3.0 `curl` check.
- Workers report CSP violations to their own scope, not to the document. A worker-side block shows up as a failed conversion instead of an entry in `seen`.

## 4. Owner decisions needed

| Ref | Decision | Recommendation | Blocks |
|---|---|---|---|
| OD-21 | Empty `ARIADSHIFT_WEB_STORAGE_ORIGINS` with a cloud API URL set | Warn at export (implemented in E1). "Default to the API origin" is already today's behaviour, because the API origin is always in `connect-src`. "Fail" breaks `file://` cloud builds. | E1 (one line) |
| OD-9, OD-12 | `file://` storage for untrusted multi-tenant traffic | If yes, land E5 (per-caller share, default 4) and record the sybil residual. If `file://` is single-tenant, replace E5 with a `docs/server.md` sentence. | E5 |
| OD-15 | Should CI run `just web-e2e-static`, and as a required check? | Add it to the `web` job, Chromium only. | E9 wiring |
| new | Keep the seven phase-7 record commits (pick #13)? | Yes. `threat-model.md:97` links the audit report that `2bcb5ec` updates, and MFPF H deletes the old branch. | E0 |
| new | E6 design: per-process guard, or a DB lease (migration)? | Per-process guard now. Add a lease only if one digest per object across replicas is required. | E6 |
| PM (not owner) | `2045-dev-fix-specs` SRV-1 (its W2) collides with `90787ce` | Drop it from the dev queue; E0 plus E5 replace it. | dispatch |
| optional | Start the server half early (90787ce, f8b0347, 709f982, E4–E6) on today's dev? | Not recommended. It forces another spec, snapshot and SDK-lock regeneration through D0, while hardening's server files are identical to dev anyway. | none |

## 5. Unresolved questions

1. The re-land base (dev after steps 1–3b) does not exist yet. Every conflict above is inferred from pre-image comparisons. Run `git merge-tree --write-tree` or the real picks first, and compare with the §3.0 table.
2. Does D25 on hardening rewrite `Storage::put_stream_until` (`storage.rs:486ff`) or only add a new non-local path? This decides whether pick #1 conflicts.
3. Will I13 (web notices) touch `infra/docker/web.Dockerfile`? If it does, pick #6's resolution must keep its lines too.
4. Does Uppy's XHR upload retry a 429? With E5's per-caller share it matters only for clients that upload in parallel; the web uses `limit: 1`.
5. Does any worker load `blob:` or `data:` sub-resources that `default-src 'self'` would refuse? E9 settles this before E8 (f) lands.
6. Turnstile inside the COEP `credentialless` `/app` (R8) is still unverified in a real browser.
7. On the integrated lockfile, do the js-yaml and body-parser parents already resolve the fixed versions, which would make `f368ee4`'s overrides unnecessary?
8. The published GHCR web image bakes localhost origins (SAS Q5). Should `docs/self-hosting.md` say that the image must be rebuilt for any other host?
9. Should `docs/SECURITY.md` (on `origin/docs/v1-security-policy`, built on distribution) link `docs/security/threat-model.md` and `dependency-policy.md` once both land?
