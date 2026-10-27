# Cloud E2E verification after label-cancellation rework

Both requested corrections are implemented on `feat/cloud-product--e2e-docling`. Label events preserve a running conversion, and the documentation accurately describes polling deadlines and runner queue limits. No push, workflow dispatch or PR creation was performed. The branch awaits ACCEPT.

- Verified implementation HEAD: `e775a158ffee5f9b731785c229775b83245540ce`.
- Static gate: [d2120-t3-rework2-static.log](/home/vchun/Codes/ariadshift-logs/d2120-t3-rework2-static.log).
- Gate last line: `=== e775a158ffee5f9b731785c229775b83245540ce ALL GREEN`.
- Exact gate drivers: [rework2-gate.zsh](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework2-gate.zsh) and [rework2-gate.py](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework2-gate.py).
- Executed cases: [rework2-proof.py](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework2-proof.py), with [JSON results](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework2-cases.json).

## Changes and case evidence

The conversion job retains its PR concurrency group. Its cancellation expression now requires a PR event whose action is neither `labeled` nor `unlabeled`. A new commit's `synchronize` event still permits cancellation. The workflow currently triggers `labeled`; the `unlabeled` guard also protects a future trigger addition.

| Configured event | Cancel a running conversion? | Local evidence |
|---|---|---|
| PR `labeled` | No | The extracted expression evaluates false. |
| PR `unlabeled` | No | The extracted expression evaluates false. |
| PR `synchronize` | Yes | The extracted expression evaluates true. |
| PR `reopened` | Yes | The extracted expression evaluates true. |
| Push, schedule or manual dispatch | No | All three expression cases evaluate false. |
| Surviving conversion fails after unrelated labels | Required status FAIL | The actual final-status shell exits 1 with `Required Compose run did not succeed: failure`. |

Under GitHub's default [one-running, one-pending policy](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency), a label arrival preserves running conversion A while it waits. A later arrival can replace pending job B; it does not cancel A through the label policy. Pending jobs are not guaranteed to survive. A cancelled pending conversion cannot pass this workflow's final required status, and A's final status fails if A fails. A label-triggered conversion must itself succeed before its required status can pass. Actual scheduling, same-name required-check selection and cross-run cancellation need a live PR; the local expression and shell cases do not reproduce GitHub's scheduler.

The new regression case was first run against the previous expression and failed because a label produced `cancel=true`; see [d2120-t3-rework2-red.log](/home/vchun/Codes/ariadshift-logs/d2120-t3-rework2-red.log). After repair, all 44 cases passed. The driver evaluates the configured conjunction using Bash's equivalent boolean operators and executes the workflow's actual final-status shell. The other 36 cases retain scope, failure, deadline, pack-state and source-provenance coverage from the previous rework.

The workflow comments and `docs/ci.md` now explain that the request shell creates the 540-minute timestamp only after obtaining a runner. A polling shell enforces that timestamp only once a waiter has a runner; a waiter queued past it fails when it starts. Time spent queued before the request or a waiter starts is bounded only by GitHub's queue limits. The timestamp is not a hard 540-minute wall-clock bound, and there is no independent watchdog. GitHub's [`timeout-minutes`](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idtimeout-minutes) limits execution rather than runner queue time. The two polling windows and their 310/250-minute job limits are unchanged. Both existing expired-timestamp cases still fail clearly after their shell starts.

## Validation and commits

The original queued PID 344171 was cancelled before acquiring a lock, as instructed by the Mac master. The detached replacement used PID 606025 and `zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 python3 /home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework2-gate.py`. The slot wrapper waits on `build.lock.2`, requires load below 12 and available memory above 8 GB, and runs at priority 19. It checked this local worktree directly, preserving the no-push constraint. The driver exported `CARGO_BUILD_JOBS=4`, UV `0.12.23`, pnpm `12.9.1` and Node from `.node-version`, used this worktree's own Cargo target directory, checked load and ran:

```sh
git show --no-patch --format=Verified-head:%H HEAD
.tools/bin/actionlint .github/workflows/cloud-e2e.yml
bash scripts/test-ci-scope.sh
python3 /home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework2-proof.py
typos docs/ci.md .github/workflows/cloud-e2e.yml
just lint-commits
just lint-commits ac0314d7627ade2e0f115f22ff3faf0247ddec32..HEAD
git diff --check ac0314d7627ade2e0f115f22ff3faf0247ddec32..HEAD
git diff --exit-code ac0314d7627ade2e0f115f22ff3faf0247ddec32..HEAD -- infra/test/e2e/run.sh
```

Actionlint, all 76 classifier cases, all 44 workflow cases, typos, both commit-lint ranges and whitespace checks passed. Review of the final diff confirmed that only the cancellation policy and its documentation changed. No conversion code, pack resolver behavior or `run.sh` was changed, so Compose was not rerun. No full `just ci` run is claimed.

| SHA | Subject |
|---|---|
| `e775a158ffee5f9b731785c229775b83245540ce` | `fix(ci): preserve conversions across label events` |

A separate report-only commit records this result. The canonical operations copy records its final branch HEAD and subsequent report checks without introducing a self-referencing commit hash into this repository report. No service, container or listener was started, and no sudo or process suspension was used. The detached gate exited after its checks.

## Retained evidence and open concerns

The prior gate [t3-rework-032049.log](/home/vchun/Codes/ariadshift-logs/t3-rework-032049.log) verified implementation `815af93` and ended `=== 03:20:57 ALL GREEN`. Its four fixes remain in place: recomputed label scope, the always-run final required status, the extended polling budget and exact-source pack selection.

The unchanged real OCR journey is recorded in [t3-proof-011908.log](/home/vchun/Codes/ariadshift-logs/t3-proof-011908.log), ending `=== 01:47:40 ALL GREEN`. It converted the image-only Vietnamese PDF through Docling to a validated DOCX, enforced quota and deletion, and used the older pack explicitly requested by the original brief. It does not prove artifact handoff for this revised head. The original B9 environment guidance and B11 classifier remain unchanged.

- A live PR must still prove label preservation, new-commit cancellation, replacement of a pending run, final required-check behavior and exact-source artifact handoff. No green remote Compose run is claimed.
- Automatic dispatch cost and fork prerequisites remain operational decisions from the review. This small rework does not change the pack workflow's platform matrix or its permissions. Nightly signed installation still needs a compatible published pack.
- The historical Docker executor load overrun remains documented in [d2120-t3-resource.log](/home/vchun/Codes/ariadshift-logs/d2120-t3-resource.log). This rework started no build or Docker executor and did not alter another worker's processes.

Status: DONE_WITH_CONCERNS
