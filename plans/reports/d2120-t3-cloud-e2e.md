# Cloud E2E verification

The requested changes are implemented and the real Compose journey passed. The image-only Vietnamese PDF completed through Docling, its DOCX contained “Khu vườn đọc sách”, and both negative checks rejected invalid inputs. Nothing was pushed and no PR was opened. The branch awaits ACCEPT.

- Branch: `feat/cloud-product--e2e-docling`.
- Verified implementation HEAD: `fa294e3dbcf9b6cb88c2007c3616534bab0be7f4`.
- Combined gate: [d2120-t3-final-green.log](/home/vchun/Codes/ariadshift-logs/d2120-t3-final-green.log).
- Gate last line: `=== fa294e3dbcf9b6cb88c2007c3616534bab0be7f4 ALL GREEN`.
- Raw Compose gate: [t3-proof-011908.log](/home/vchun/Codes/ariadshift-logs/t3-proof-011908.log), ending `=== 01:47:40 ALL GREEN`.
- Final static checks: [d2120-t3-final-static.log](/home/vchun/Codes/ariadshift-logs/d2120-t3-final-static.log), ending with the verified HEAD and `ALL GREEN`.

## Changes and evidence

| Item | Result |
|---|---|
| B1 | `run.sh` uses `fixtures/pdf/vi-garden-image-only.pdf`, SHA-256 `048930c263e1b5aefe30990d74a207fbec3ce275abfda6fd402ead05ee7170fb`, matching its exact manifest entry. `pdftotext` returned only a page separator. The final GET reported a succeeded Docling route without `ocr_required`; the downloaded 10,595-byte DOCX passed ZIP, size, hash and case-sensitive text checks. |
| B2 | PR, push and manual runs resolve the Linux pack from a matching Docling workflow head. An isolated job can dispatch the registered pack workflow when necessary; the Compose job has read-only Actions permission. It waits for the successful Linux pack job, downloads runtime and shared artifacts, generates the unsigned manifest with the built server's CLI, safely unpacks it, and sets `E2E_DOCLING_PACK_DIR`. The override skips signed installation and its host-range check. Nightly retains the signed installer. Python bytecode writes are disabled during preparation to keep the image context stable. |
| B9 | `.env.example` now includes the public Turnstile site key and operator name, with pairing and rebuild guidance. |
| B10 | Concurrency belongs to the Compose job. Unrelated labels skip expensive jobs without cancelling an existing Compose run. Seven executions of the actual scope shell covered unrelated/cloud labels, labeled and path-matched PR updates, push, schedule and dispatch. |
| B11 | Push and PR decisions call the single cloud path classifier in `ci-scope.sh`. The requested web, Rust, manifest, lockfile, toolchain, WASM installer and fixture paths are covered. All 76 scope tests passed, including excluded paths and mixed changes. |

The requested real pack came from [Docling pack run 40462719557](https://github.com/bavanchun/AriadShift/actions/runs/40462719557), head `796c83ea6faaa820996d4107847862ae00f704d2`. Both artifacts were unexpired. This local proof uses the existing run explicitly named in the brief; it does not claim that a pack was built for the final local branch head. The workflow enforces matching heads for future CI runs.

The actual workflow resolver selected that run through the GitHub API. Running the actual preparation shell with a different expected head returned 1 with `Docling pack head mismatch`, before any download or compilation. A separate directory referencing the real pack's manifest, runtime and models but lacking tessdata made `run.sh` return 1 with `local Docling pack lacks tessdata`. No substitute engine or conversion output was used.

The positive journey also verified deletion of the downloaded output, refusal of the fifth pending upload with `429 quota_exceeded`, rejection of both API keys after their account was deleted, and continued access for the other account's key. Database snapshots preceded bootstrap, the journey and account deletion. All project containers, volumes and networks were removed; ports 18086, 13006, 18336 and 13016 were released. The detached runner exited.

## Commits

| SHA | Subject |
|---|---|
| `d197caeccfd8f2da79d91d29e6f3a28f12a51997` | `chore: merge dev into cloud e2e` |
| `9c682bca0a0925945159840188a96f912a743933` | `test(cloud): prove docling OCR with matching CI packs` |
| `183c3907078aa7713735edf6b7d575058ef90f5e` | `docs(ci): explain pack dispatch before workflow promotion` |
| `fa294e3dbcf9b6cb88c2007c3616534bab0be7f4` | `ci(cloud): avoid changing the image context during pack setup` |

The dev merge was clean. The raw proof started at `183c390`; the only subsequent implementation change disables Python bytecode writes during CI preparation. Conversion sources, the fixture and `run.sh` were unchanged, as verified with `git diff --exit-code 183c390..HEAD -- crates fixtures infra/test/e2e/run.sh`. Final workflow and commit checks were repeated on `fa294e3`.

## Commands and outcomes

The detached command was `setsid nohup zsh -lic 'exec zsh /tmp/d2120-t3-gate.zsh'`, with output redirected to `d2120-t3-launch.log`. Its PID was 3503510. The [saved gate driver](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/gate.zsh) contains the exact invocation of `flock /home/vchun/Codes/01-AriadShift/.run/build.lock nice -n 19 ionice -c 3 zsh /home/vchun/Codes/ariadshift-ops/asus-build.sh`. The runner bootstrapped a separate verification worktree, then explicitly changed to this authorized local worktree, so no push was needed to test the local commits. `CARGO_BUILD_JOBS=4` and the prescribed mise pins were exported.

The gate ran these commands successfully:

```sh
just pandoc
.tools/bin/actionlint .github/workflows/cloud-e2e.yml
bash scripts/test-ci-scope.sh
bash -n infra/test/e2e/run.sh
sh -n scripts/ci-scope.sh scripts/test-ci-scope.sh
typos docs/ci.md .env.example infra/test/e2e/run.sh scripts/ci-scope.sh scripts/test-ci-scope.sh .github/workflows/cloud-e2e.yml
python3 /tmp/d2120-t3-local-proof.py
git diff --check
git rev-parse HEAD
```

The [saved proof driver](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/local-proof.py) records the exact scope cases, both successful `just lint-commits` ranges, the expected failures, environment and preparation invocation. The [saved preparation shell](/home/vchun/Codes/ariadshift-ops/d2120-t3-evidence/ci-pack-prepare.sh) is extracted from the workflow and records the exact artifact downloads, server build, manifest generation and unpacking commands. The positive invocation was:

```sh
E2E_DOCLING_PACK_DIR=$PWD/.tools/t3-ci-pack/docling-pack E2E_PROJECT=ariadshift-e2e-t3 E2E_API_PORT=18086 E2E_AUTH_PORT=13006 E2E_S3_PORT=18336 E2E_WEB_PORT=13016 bash infra/test/e2e/run.sh
```

Final static review repeated actionlint, all scope tests, shell syntax, typos, `just lint-commits`, the own commit range `d197cae..HEAD`, and `git diff --check`. All passed. No full `just ci` run is claimed for this task.

## Remaining concerns and CI validation

- The ASUS resource ceiling was exceeded transiently: the observed one-minute load reached 51.42 during image toolchain installation and 24.42 during web export. Serialization, runner priority 19 and four Cargo jobs were in place, but Docker executor processes retained priority 0. Attempts to adjust only the owned executor failed because sudo requires a password. No priority or affinity changes were applied and no Capstone or herdr processes were touched. The [resource record](/home/vchun/Codes/ariadshift-logs/d2120-t3-resource.log) preserves the observations. Controlling executor resources requires an owner-level builder change outside the owned files.
- GitHub CI must still prove automatic dispatch with its Actions token, artifact handoff for a newly built head, fork approval behavior and actual cancellation between workflow runs. The API resolver, mismatched-head rejection, preparation commands and scope decisions were exercised locally; no remote workflow was dispatched by this worker.
- Nightly signed installation still depends on a published signed pack and compatible host version. The development override proves conversion on the current host and deliberately does not prove signed installation.

Status: DONE_WITH_CONCERNS
