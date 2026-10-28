# Cloud E2E cache-poisoning repair

The reported checkout path is repaired in commit `7f9ffbb6ea7adfd7b436008455bfbc2af8b365ba` (`fix(ci): isolate conversion checkout and dependency caches`). The conversion job disables shared uv caching, uses direct event-authorized checkout refs and verifies the recorded source SHA before executing repository code. Pack SHA and event verification remain intact. No alert was dismissed and nothing was pushed. The fold worker can apply this commit to the PR branch.

- Branch: `feat/cloud-product--e2e-docling`.
- Verified implementation HEAD: `7f9ffbb6ea7adfd7b436008455bfbc2af8b365ba`.
- Gate: [d2120-t3-codeql-static.log](/home/vchun/Codes/ariadshift-logs/d2120-t3-codeql-static.log).
- Gate last line: `=== 7f9ffbb6ea7adfd7b436008455bfbc2af8b365ba ALL GREEN`.
- Exact drivers: [codeql-gate.zsh](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/codeql-gate.zsh), [codeql-gate.py](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/codeql-gate.py).

## Diagnosis and cache inventory

[Alert 2](https://github.com/bavanchun/AriadShift/security/code-scanning/2) is high severity, rule `actions/cache-poisoning/poisonable-step`, at `cloud-e2e.yml:335` for schedule and manual triggers. The [saved alert](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/codeql-alert.json) records the PR merge analysis commit `b1caeef38a3372470be578312ea2375a77434f23`. Before repair, this branch and fold commit `5444c25` had identical cloud and pack workflow files; no merge was needed to reproduce the relevant configuration.

The conversion job uses only pinned `actions/checkout` and pinned `astral-sh/setup-uv`; it has no nested local/composite action. The complete [action and input inventory](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/codeql-cache-audit.json) includes every cloud job and every pack job.

| Cache mechanism | Before repair | After repair |
|---|---|---|
| setup-uv dependency cache | Pinned action `1c37ad0` defaults to `enable-cache: auto`, `restore-cache: true`, `save-cache: auto`. On these GitHub-hosted PR, push and manual runs it enables restore/save of the uv cache, normally under `$RUNNER_TEMP/setup-uv-cache`, keyed by platform, Python and dependency hashes. | `enable-cache: false` disables both remote restore and the post-action save. |
| Scheduled conversion | The entire setup-uv step is skipped, so it already has no explicit dependency-cache action. | Still skips setup-uv. The checkout trust boundary is repaired for this event too. |
| checkout, Rust, Node/pnpm and Docker | Checkout has no Actions dependency cache. No Rust, setup-node/pnpm, actions/cache or buildx action exists in this job. Dockerfile Cargo registry/target cache mounts are local builder state; no GHA `cache-from` or `cache-to` is configured. | Unchanged; local builder mounts remain available on the disposable runner. |

The defaults are verified from the exact pinned [action metadata](https://github.com/astral-sh/setup-uv/blob/1c37ad07a6a961277cf70c0d37d6f313000f5884/action.yml). Its [setup implementation](https://github.com/astral-sh/setup-uv/blob/1c37ad07a6a961277cf70c0d37d6f313000f5884/src/setup-uv.ts) gates restoration on `inputs.enableCache`; its [post action](https://github.com/astral-sh/setup-uv/blob/1c37ad07a6a961277cf70c0d37d6f313000f5884/src/save-cache.ts) gates saving on the same value. The uv executable's runner-local tool cache is separate from this shared dependency cache.

The [query source](https://github.com/github/codeql/blob/fd2bfc8a3da9c73a0c8be05d110efaf14a856445/actions/ql/src/Security/CWE-349/CachePoisoningViaPoisonableStep.ql) does not require an explicit restore/save step. It combines an untrusted checkout, a subsequent executable step and default-branch cache-write context. The [event model](https://github.com/github/codeql/blob/fd2bfc8a3da9c73a0c8be05d110efaf14a856445/actions/ql/lib/codeql/actions/security/CachePoisoningQuery.qll) includes schedule and manual dispatch. This explains the scheduled alert even though setup-uv is skipped. Disabling uv caching alone would leave the reported source path in place; `actions: read` does not restrict the separate cache token.

## Repair and adjacent audit

Scope and conversion share one event-ref expression through a YAML scalar anchor. PRs check out their immutable head SHA, push/manual runs use `github.sha`, and schedules retain the trusted repository `dev` branch. No cloud job checks out a job-output-derived ref. Immediately after checkout, every event compares `git rev-parse HEAD` with Scope's recorded SHA. A moved `dev` branch, wrong SHA or missing output fails before setup-uv or repository scripts execute. The output is a comparison value rather than checkout authority. The existing pack download guard still checks the checkout SHA, pack SHA and push/dispatch event.

The other cloud jobs have no cache actions. Only Scope also checks out code, using the same direct event-ref expression. The pack request, both wait jobs and final status do not check out repository code.

All five `pack-docling.yml` jobs use default checkout of their immutable workflow-event source; none consumes `needs.scope.outputs.source-sha` or another dynamic checkout ref. The dispatch branch SHA check and subsequent artifact SHA/event checks are unchanged. Existing pack caches were audited without modification:

| Pack job | Cache configuration |
|---|---|
| Scope | No cache action. |
| Engine | setup-uv automatic dependency caching. |
| Conformance | setup-uv cache; explicit model/OCR blob cache. Rust toolchain cache is disabled. |
| Pack | setup-uv cache; explicit model/OCR blobs, native source builds and compiler-work restore/save caches. Rust toolchain cache is disabled. |
| Manifest | setup-uv cache; artifact download/upload, which are not dependency caches. Rust toolchain cache is disabled. |

These jobs do not have the reported output-derived checkout pattern. Their PR runs retain PR cache scope; manual dispatch uses its own event source. This is a scoped audit of checkout/cache routing, not a new certification of every pack dependency. No query exclusion, alert dismissal or suppression was added. `docs/ci.md` documents the changed checkout and cache rules.

## Verification

The new regression check fails against the original workflow with `output-derived checkout`; see [the expected failure](/home/vchun/Codes/ariadshift-logs/d2120-t3-codeql-red.log). After repair, [codeql-proof.py](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/codeql-proof.py) passes 51 cases: the previous 44 cases, four direct event-ref selections and three executions of the actual SHA guard. The [JSON result](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/codeql-cases.json) records the tested commit. It also checks disabled uv caching, absence of output-derived cloud checkouts, guard ordering and unchanged default pack checkouts. These are configuration and shell cases, not a local CodeQL run.

The detached gate used PID 2108995 through `gate-slot.sh 2`, which waits on `build.lock.2`, checks load below 12 and available memory above 8 GB, and runs at priority 19. It exported the prescribed tool pins, `CARGO_BUILD_JOBS=4` and this worktree's own Cargo target directory. The driver records these successful commands:

```sh
.tools/bin/actionlint .github/workflows/cloud-e2e.yml .github/workflows/pack-docling.yml
bash scripts/test-ci-scope.sh
python3 /home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/codeql-proof.py
typos docs/ci.md .github/workflows/cloud-e2e.yml
just lint-commits
just lint-commits 02f741e704d7f2e6fc23ef26884d06ddb427613a..HEAD
git diff --check 02f741e704d7f2e6fc23ef26884d06ddb427613a..HEAD
git diff --exit-code 02f741e704d7f2e6fc23ef26884d06ddb427613a..HEAD -- infra/test/e2e/run.sh .github/workflows/pack-docling.yml
```

Actionlint, all 76 scope cases, all 51 workflow cases, typos and commit checks passed. `codeql` is absent from PATH, as recorded in the gate. No CLI installation, local Actions query execution, full `just ci` or Compose rerun is claimed. The rule's documented untrusted-checkout source path has been removed; fresh GitHub analysis is still needed to confirm the alert closes after folding. The existing real OCR proof remains in [t3-proof-011908.log](/home/vchun/Codes/ariadshift-logs/t3-proof-011908.log); conversion code and `run.sh` are unchanged.

A separate report-only commit records this result. The canonical operations copy records the final branch HEAD and report checks. The gate exited; no service, container or listener was started, and no process was suspended.

## Remaining concerns

- The fold worker must apply `7f9ffbb6ea7adfd7b436008455bfbc2af8b365ba` and rerun CodeQL on PR #84. The latest PR instance is still open on analysis commit `b1caeef`; no remote success is claimed.
- Live source-matched pack handoff, label concurrency and nightly signed installation retain their previously documented verification limits. This repair does not change their policy.

Status: DONE_WITH_CONCERNS
