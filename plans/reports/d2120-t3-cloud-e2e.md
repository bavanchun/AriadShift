# Cloud E2E rework verification

All four requested workflow corrections are implemented. The static gate and exercised workflow cases passed on implementation commit `815af932761690c3b742027310d6b5bad8e63158`. This report supersedes the earlier delivery's label-scope and pack-provenance claims. Nothing was pushed, no workflow was dispatched, and no PR was opened. The local branch awaits ACCEPT.

- Branch: `feat/cloud-product--e2e-docling`.
- Verified implementation HEAD: `815af932761690c3b742027310d6b5bad8e63158`.
- Raw rework gate: [t3-rework-032049.log](/home/vchun/Codes/ariadshift-logs/t3-rework-032049.log).
- Raw gate last line: `=== 03:20:57 ALL GREEN`.
- Exact commands: [rework-gate.zsh](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework-gate.zsh).
- Executed case driver: [rework-proof.py](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework-proof.py).
- Machine-readable case results: [rework-cases.json](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework-cases.json).

## Corrections and verification

| Review requirement | Final behavior | Exercised evidence |
|---|---|---|
| Unrelated labels cannot create a skipped green cloud check | Every PR label event recomputes changed-path scope; an existing or newly added `cloud-e2e` label forces conversion. Concurrency remains on the actual conversion job. | Real git diffs cover cloud and documentation-only changes. An unrelated label on a cloud change produces `run=true`; skipped or failed conversion then makes the required check fail. |
| Failed or empty scope must fail the required check | The always-run final job retains the exact required name `Compose stack (core, office, docling)`. It validates scope success and a boolean decision before accepting conversion success or an intentional nonmatching decision. | Failed, cancelled and skipped scope; failed scope with a false output; missing and invalid output; intentional omission; successful, failed, skipped and cancelled conversion. |
| Deadline covers the dependency chain and queues | A single absolute 540-minute deadline covers request overhead, pack scope, engine tests, the 350-minute Linux pack build and queue time. A 300-minute polling window hands off to a 240-minute window without resetting the deadline. Job limits are 310 and 250 minutes. | The driver executes the real deadline assignment, derives the dependency budgets from `pack-docling.yml`, and checks that 385 minutes plus a 10-minute request and a 120-minute queue scenario fits in 540 minutes. Handoff succeeds; an expired deadline fails clearly in both windows. Failed, cancelled, timed-out and skipped Linux pack jobs fail. |
| Pack and E2E use the same source | Scope records the PR head SHA; E2E checks out that exact SHA. Only push or dispatch pack runs for that SHA are accepted. PR artifacts are rejected because their API head SHA does not identify the synthetic merge tree. The download step validates its own checkout, artifact run SHA and event before downloading. | Exact-source push and dispatch metadata pass. Same-head PR metadata, a stale PR merge after base advancement, another pack commit and another E2E checkout are rejected with explanations. The actual resolver selector excludes PR runs before selecting dispatch. |

The 540-minute deadline is finite and includes queue delay after the request begins. The 120-minute queue value is an exercised budget scenario, not a promise about GitHub runner availability. Both polling jobs remain below GitHub's six-hour per-job limit and leave ten minutes beyond their polling windows for reporting. They share the same YAML-anchored polling script. Only the request job has `actions: write`; polling and conversion use `actions: read`.

## Scope and required-check case tables

These cases execute the actual workflow shell against existing git history. Cloud changes use `d197cae..9c682bc`; documentation-only changes use `9c682bc..183c390`.

| Event | Changed paths / label state | Scope result |
|---|---|---|
| Unrelated label | Cloud paths | `true` |
| Unrelated label | Nonmatching paths | `false` |
| Unrelated label | Nonmatching paths, existing `cloud-e2e` label | `true` |
| `cloud-e2e` label | Nonmatching paths | `true` |
| PR synchronize | Cloud paths | `true` |
| PR synchronize | Nonmatching paths | `false` |
| Push | Cloud paths | `true` |
| Schedule | No path restriction | `true` |
| Manual dispatch | No path restriction | `true` |

| Scope state / decision | Conversion state | Required Compose result |
|---|---|---|
| Failed, cancelled or skipped | Skipped | FAIL |
| Failed with `run=false` | Skipped | FAIL |
| Successful with missing or invalid output | Skipped | FAIL |
| Successful with explicit `run=false` | Skipped intentionally | PASS |
| Successful with `run=true` | Success | PASS |
| Successful with `run=true` | Failed, skipped or cancelled | FAIL |
| Unrelated label on a cloud change (`run=true`) | Failed or skipped | FAIL |

All 36 exercised shell cases passed, including expected nonzero exits. Additional assertions check the resolver's actual jq selection, pinned-checkout wiring, shared deadline, job timeout margins and job-only concurrency. Explicit metadata inputs are unit cases; they are not live GitHub runs and do not substitute for a remote artifact handoff.

## Gate commands and outcome

The detached runner used PID 139932 and held the shared build lock only when admitted to the queue. The saved driver invokes `flock /home/vchun/Codes/01-AriadShift/.run/build.lock nice -n 19 zsh /home/vchun/Codes/ariadshift-ops/asus-build.sh`. It uses `CARGO_BUILD_JOBS=4`, UV `0.12.23`, pnpm `12.9.1` and Node from `.node-version`. The standard runner prepares a separate verification worktree, then the first supplied command explicitly changes to this authorized local worktree to verify the unpushed implementation commit.

The complete command list is preserved in the driver. The checks were:

```sh
just pandoc
.tools/bin/actionlint .github/workflows/cloud-e2e.yml
bash scripts/test-ci-scope.sh
python3 /home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/rework-proof.py
typos docs/ci.md .github/workflows/cloud-e2e.yml plans/reports/d2120-t3-cloud-e2e.md
just lint-commits
just lint-commits a25f05abc4e4ab1e0e05793c621bcc67269f5cac..HEAD
git diff --check
git diff --exit-code a25f05abc4e4ab1e0e05793c621bcc67269f5cac..HEAD -- infra/test/e2e/run.sh
git rev-parse HEAD
```

Actionlint, all 76 classifier tests, all 36 workflow cases, typos, both commit-lint ranges and whitespace checks passed. `run.sh` was unchanged, so the rework brief's Compose rerun exemption applies. No full `just ci` run is claimed. Review against the four requirements found no remaining high issue in the inspected local implementation; GitHub orchestration remains an explicit verification limit below.

## Retained real conversion evidence

The earlier real Compose proof remains valid for the unchanged runner. Its raw log is [t3-proof-011908.log](/home/vchun/Codes/ariadshift-logs/t3-proof-011908.log), ending `=== 01:47:40 ALL GREEN`. It used image-only Vietnamese fixture `fixtures/pdf/vi-garden-image-only.pdf`, SHA-256 `048930c263e1b5aefe30990d74a207fbec3ce275abfda6fd402ead05ee7170fb`; `pdftotext` emitted only `0c`. The final GET reported a succeeded Docling route without `ocr_required`; the 10,595-byte DOCX passed size, hash and ZIP checks and contained “Khu vườn đọc sách”. Missing tessdata and a mismatched expected pack head each failed their real preflight.

That local conversion deliberately used the existing pack specified by the original brief: [run 40462719557](https://github.com/bavanchun/AriadShift/actions/runs/40462719557), head `796c83ea6faaa820996d4107847862ae00f704d2`. It proves conversion, not a pack built from the reworked workflow's pinned source. The new workflow rejects those PR artifacts when resolving a future source-matched CI build.

The original journey also verified output deletion, `429 quota_exceeded` for a fifth pending upload, invalidation of both keys after account deletion, and continued access for the other account. Its containers, volumes and networks were removed and ports 18086, 13006, 18336 and 13016 were released. The original runner exited. The rework started no service, container or listener and used no sudo or process suspension.

The original B9 public Turnstile environment guidance and B11 shared cloud path classifier remain unchanged. The original implementation gate is [d2120-t3-final-green.log](/home/vchun/Codes/ariadshift-logs/d2120-t3-final-green.log), ending `=== fa294e3dbcf9b6cb88c2007c3616534bab0be7f4 ALL GREEN`.

## Commits

| SHA | Subject |
|---|---|
| `d197caeccfd8f2da79d91d29e6f3a28f12a51997` | `chore: merge dev into cloud e2e` |
| `9c682bca0a0925945159840188a96f912a743933` | `test(cloud): prove docling OCR with matching CI packs` |
| `183c3907078aa7713735edf6b7d575058ef90f5e` | `docs(ci): explain pack dispatch before workflow promotion` |
| `fa294e3dbcf9b6cb88c2007c3616534bab0be7f4` | `ci(cloud): avoid changing the image context during pack setup` |
| `a25f05abc4e4ab1e0e05793c621bcc67269f5cac` | `docs: record cloud OCR verification results` |
| `815af932761690c3b742027310d6b5bad8e63158` | `fix(ci): require verified cloud conversion checks` |

The rework implementation changes only `.github/workflows/cloud-e2e.yml` and `docs/ci.md`. A separate documentation commit records this report; the canonical operations copy records the resulting branch head without a self-referencing commit hash in the repository report.

## Remaining concerns

- GitHub must still exercise Actions-token dispatch, the new always-run required status, actual cancellation, fork prerequisites and source-matched artifact handoff on a revised remote head. This worker made no push or remote dispatch under the rework brief. Local shell decisions and static wiring passed; no green remote Compose check is claimed.
- Nightly still uses signed-release installation and requires a published compatible pack. The development override does not prove signed installation.
- The original Compose proof exceeded the shared ASUS load ceiling transiently (51.42 during toolchain installation and 24.42 during web export). Docker executor processes retained priority 0 despite a serialized priority-19 runner. The [historical resource record](/home/vchun/Codes/ariadshift-logs/d2120-t3-resource.log) preserves that limitation. No new Docker build, sudo attempt, priority change to another process, or suspension was performed during rework.

Status: DONE_WITH_CONCERNS
