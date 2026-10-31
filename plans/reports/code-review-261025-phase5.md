# Cloud product review

The web and account diffs require fixes before integration. There are six high findings and no critical findings. The compose E2E and API documentation diffs have no verified critical or high finding within this review's scope.

## Reviewed revisions

| Diff | Head | Comparison base |
|---|---|---|
| `origin/feat/cloud-product--web` | `0bff13ca40146bd232bdf6caf69f3a6ec141c33f` | `origin/feat/cloud-product-base` (`67a06fb59673e90ef7f72271073a47cc70c70afa`) |
| `origin/feat/cloud-product--pages` | `dfcc63ff4cdfc240d242513402d5ec1fa47fdede` | `origin/feat/cloud-product-base` (`67a06fb59673e90ef7f72271073a47cc70c70afa`) |
| `origin/feat/cloud-product--e2e` | `5014c640ae6fd64ac84bf32b03d412d0443a0780` | `origin/feat/cloud-foundation` (`afb5ea17179b3e557bbe4f3254ca37e2942d85d4`) |
| `origin/feat/cloud-product--docs` | `c1ed7ca4db8e0adfa955f5de9f054caac4a88e7f` | `origin/feat/cloud-foundation` (`afb5ea17179b3e557bbe4f3254ca37e2942d85d4`) |

Locations below refer to the relevant head, unless marked as shared base code. The requested completion plan was absent from this worktree; its plan and cloud-product phase were read from `origin/dev` instead. The review also read architecture sections 8–12, the Git workflow, worker reports, SDK transport/SSE implementation, auth service, compose configuration, server handlers and OpenAPI contract.

## Web findings

### High: the dependency change cannot pass a frozen installation

**Location:** `apps/web/package.json:19`, `:22–23`.

The diff adds the SDK and both Uppy packages without updating `pnpm-lock.yaml`. The unchanged web importer at `pnpm-lock.yaml:366` has none of them. A clean `pnpm install --frozen-lockfile`, including the install in `infra/docker/web.Dockerfile:62`, therefore fails before the site can build or export. This blocks both the existing local site and cloud deployment, regardless of whether cloud environment variables are set.

**Required fix:** commit the matching lockfile update with the manifest. The worker report already identifies this integration requirement; it remains unresolved in the reviewed diff.

**Coordinator verification:** run `pnpm install --frozen-lockfile` from a clean integrated checkout, then the normal web gates.

### High: clean static exports import a client that has never been generated

**Location:** `apps/web/src/lib/cloud/api.ts:6–13`.

These new runtime imports reach `packages/sdk/src/index.ts:5` and `packages/sdk/src/client.ts:21–22`, which require the ignored `src/generated/` tree. Neither the SDK package nor the web package has an install hook that generates it. The existing `just web-build` and `just web-export` recipes do not generate it, and `infra/docker/web.Dockerfile:62–68` installs packages and immediately exports the site. After the lockfile issue is fixed, a fresh Docker build still cannot resolve the generated SDK modules. A preceding `just web-typecheck` can hide this on a developer machine because the SDK's typecheck script generates them as a side effect.

**Required fix:** make generation an explicit prerequisite of standalone web build/export/test entry points and of the web image build. Do not rely on an earlier typecheck or an untracked local directory.

**Coordinator verification:** build/export in a fresh checkout with no `packages/sdk/src/generated/`, including the compose web image. Verify both cloud-enabled and unconfigured exports.

### High: same-origin auth responses enter the offline app cache

**Location:** `apps/web/src/components/cloud/cloud-panel.tsx:234–238`; `apps/web/src/lib/cloud/auth.ts:111–118`.

The new panel requests the session from `/api/auth/get-session`. With web and auth behind one HTTPS origin, which the configuration accepts, that request appears in the document's Resource Timing entries. After returning to device mode and doing a local conversion, `converter-app.tsx:267` calls the unchanged `enableOffline()`. Shared base code `apps/web/src/converter/offline.ts:43–46` collects every resource URL, and its filter at `:18–31` accepts this auth URL. The service worker then refetches it with same-origin session cookies and persists the response, including the exposed `set-auth-jwt` header, through `service-worker.js:45–48`. The JWT is consequently stored on disk in `ariadshift-app-v1`, although `CloudAuth.signOut()` clears only its in-memory token. It can remain accessible after sign-out on a shared browser profile. A request to `/api/auth/token` can be included by the same route.

This is a new interaction between cloud auth and the existing broad asset collector. It does not affect the compose default with auth on another port. HTTP `no-store` alone would not fix explicit Cache API storage: the [Cache API documentation](https://developer.mozilla.org/en-US/docs/Web/API/Cache) specifies that HTTP caching headers are not honored.

**Required fix:** restrict offline caching and fallback to the app shell and known static/engine assets; exclude `/api/auth/**`, `/v1/**`, and presigned/document URLs at both the collector and service-worker boundary. Remove any previously cached sensitive entries on activation.

**Coordinator verification:** on a same-origin reverse-proxy deployment, sign in from cloud mode, return to device mode, convert locally and sign out. Inspect Cache Storage and confirm no auth response, JWT, job response or document body is present. Repeat the existing offline local conversion checks.

### High: start-over silently abandons failed server deletion

**Location:** `apps/web/src/lib/cloud/cloud-job.ts:483–501`; `apps/web/src/components/converter/converter-app.tsx:308–318`.

After a cloud job succeeds, disconnect the network or make `DELETE /v1/jobs/{id}` return 500, then choose **Start over and delete files**. The converter clears the staged file and unmounts the cloud panel before deletion is confirmed. `discard()` sets `#discarded` before attempting the request, catches the failure, and returns `false`; reset then shows only that device files were removed. There is no warning that the server still holds the input/output, and no retained controller or delete action for retrying. The unmount cleanup calls `discard()` again, but `#discarded` makes that call skip the deletion. Files remain until retention instead of fulfilling the explicit deletion action.

The same loss of deletion state occurs when an upload exists without a job and the current contract lacks `DELETE /v1/uploads/{id}`: `deleteUpload()` returns `false`, which is indistinguishable from having nothing to delete at the reset boundary.

**Required fix:** retain the remote identifiers and a retryable deletion state until deletion is confirmed. Show a clear remaining-server-copy/retention notice when immediate deletion is unavailable or fails, and do not permanently mark a failed discard as complete.

**Coordinator verification:** exercise reset with a successful job while offline and with a 500 deletion response, then restore connectivity and retry deletion. Also cover a completed upload whose job creation was refused against the current contract.

## Account/pages findings

### High: the new account routes import undeclared dependencies

**Location:** `apps/web/src/components/account/auth-api.ts:5–8`; `apps/web/src/components/account/jobs-api.ts:5`.

The pages diff imports `better-auth`, `@better-auth/passkey`, `@better-auth/api-key` and the SDK but changes neither `apps/web/package.json` nor its lockfile importer. Having Better Auth under `services/auth` does not make those packages dependencies of `apps/web` in pnpm's workspace layout. Every account route reaches `auth-api.ts` through `AccountShell`, so a clean build/export fails resolving these modules even when no cloud URLs are configured. Combining with the web diff adds only the SDK; the three Better Auth dependencies are still absent.

**Required fix:** declare the account client's runtime dependencies in the web package using the existing exact catalog pins and commit the updated lockfile. This is already acknowledged as a required change in the pages worker report, but it is not in either reviewed UI diff.

**Coordinator verification:** run frozen installation, web typecheck and static export from a clean integrated checkout, with and without cloud URLs.

### High: no real conversion can populate the jobs management page

**Location:** `apps/web/src/components/account/jobs-panel.tsx:36`, `:67–75`.

The page's only job source is local history. Its only production call to `rememberJob()` is inside **Retry**, which first requires an existing history row. The web diff's successful creation path at `apps/web/src/lib/cloud/cloud-job.ts:232–242` records the job only in controller memory and never writes that history or emits its change event. There is no server list endpoint in the reviewed contract. Starting from an empty browser, creating a real conversion therefore leaves `/jobs` permanently at **No cloud jobs yet**: the promised status, download, retry and delete controls cannot be reached for any real job. The tests hide this by seeding localStorage directly (`apps/web/e2e/account.spec.ts:303–317` and the jobs unit tests).

**Required fix:** connect successful job creation to the shared history contract, including the actual authenticated owner or anonymous owner, and verify the complete converter-to-jobs journey without manually seeding storage. Coordinate the panel's unmount deletion policy with that journey so navigation does not erase the result the jobs page is intended to manage.

**Coordinator verification:** in a fresh browser, create a real cloud job from `/app`, open `/jobs`, and verify that the same job and its appropriate actions are present for both authenticated and anonymous use.

## E2E and documentation assessment

The script's auth routes and cookie jar match `services/auth/src/auth.ts` and `src/app.ts`. Its upload/job/output fields and expected 201/202/204/410 responses match `schemas/openapi.v1.json` and the Rust handlers. PDF admission selects the Docling route when that pack is deployed. The pending-upload quota check matches `entitlements::admit_upload`, including the seeded free plan. The workflow uses the real compose script and disposes its throwaway volumes. No verified critical/high endpoint or response-shape mismatch was found.

The documentation's authentication examples, upload headers, SDK method options, event resumption, deletion side effects and plan limits agree with the inspected implementations. No verified critical/high example mismatch was found.

These assessments do not establish that the compose stack runs successfully. In particular, its Docling image requires a published signed pack; that is an existing deployment prerequisite, not a new defect in the E2E diff. Once integrated with the UI branches, the web build blockers above also block `compose build`.

## Verification boundary

This was a source review only. No build, test, typecheck, package installation, compose build/up, browser process or server was started, following the ASUS worker rule. Installed dependency-source inspection was blocked by the environment's node_modules access hook; no bypass was attempted. Current [Uppy signing documentation](https://uppy.io/docs/aws-s3/#signrequestrequest) confirms the single-PUT `signRequest`/signed-headers contract used here. [Better Auth security documentation](https://better-auth.com/docs/reference/security#csrf-protection), together with the service's trusted-origin and JSON-request configuration, supports rejecting a generic missing-CSRF-token finding. No verified raw-HTML XSS, JWT/API-key URL/log leakage, upload before the explicit start action, or unbounded UI-owned SSE reconnect loop was found beyond the cache exposure described above.

The coordinator should run the focused auth/cloud/account suites, the existing offline/browser suites, both export configurations, and the compose script after the findings are repaired. Build-time issues above are established by manifests and call paths; runtime scenarios are source-derived and still require execution.

## Verdicts

`origin/feat/cloud-product--web`

Verdict: FIX_REQUIRED

`origin/feat/cloud-product--pages`

Verdict: FIX_REQUIRED

`origin/feat/cloud-product--e2e`

Verdict: MERGEABLE

`origin/feat/cloud-product--docs`

Verdict: MERGEABLE
