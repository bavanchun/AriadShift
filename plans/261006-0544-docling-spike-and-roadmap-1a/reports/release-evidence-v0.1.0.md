# Pre-Release Evidence Pack — AriadShift `v0.1.0`

- **Date:** 2026-10-20
- **Repository:** `bavanchun/AriadShift`
- **Plan:** `plans/261006-0544-docling-spike-and-roadmap-1a/phase-13-release-gate-and-acceptance.md`
- **Scope:** Read-only evidence gathering for Phase 13 "Stop for the user" item 1 and pre-flight risk checks.
- **Update (2026-10-22):** §1–§8 are the pre-release snapshot. §9 records the release outcome and the live channel state after the tag.

---

## 1. Final `dev` SHA, Three-OS CI Run, and CodeQL Result

### Commands used

```bash
git rev-parse origin/dev
git log -n 3 --oneline origin/dev
gh run list --repo bavanchun/AriadShift --commit e0d770fedaa816d6fafc7df9dd7287ba674afa0f \
  --json databaseId,name,workflowName,status,conclusion,url,headSha,event,createdAt
gh run view 37661747165 --repo bavanchun/AriadShift \
  --json databaseId,url,headSha,conclusion,jobs
gh run view 37661747157 --repo bavanchun/AriadShift \
  --json databaseId,url,headSha,conclusion,jobs
gh run view 37657489944 --repo bavanchun/AriadShift \
  --json databaseId,url,headSha,conclusion,jobs
gh run view 37657489912 --repo bavanchun/AriadShift \
  --json databaseId,url,headSha,conclusion,jobs
```

### Final `dev` commit

- **Full SHA:** `e0d770fedaa816d6fafc7df9dd7287ba674afa0f` (short: `e0d770f`)
- **Commit subject:** `docs(plans): mark the release pipeline phase done` (PR [#16](https://github.com/bavanchun/AriadShift/pull/16), merged `2026-10-07T17:47:05Z`)
- **Immediate parent (code tip of Phase 12):** `e37250decc3dced5c9d8c62df55f1a893f229a12` (short: `e37250d`, `docs: describe the release pipeline and install channels`, PR [#14](https://github.com/bavanchun/AriadShift/pull/14), merged `2026-10-07T17:14:08Z`)

### CI run on `e0d770f` (`docs(plans)` commit)

- **Workflow run:** [CI #37661747165](https://github.com/bavanchun/AriadShift/actions/runs/37661747165) — `conclusion: success`
- Note: Because `e0d770f` only modified `plans/261006-0544-docling-spike-and-roadmap-1a/{plan.md,phase-12-release-pipeline.md}`, `scripts/ci-scope.sh` ran `Scope` and `Static checks` and skipped the OS build/test/fuzz jobs on that push. The full three-OS `Verify` and `Fuzz` run on the identical code tree is on its parent `e37250d` ([CI #37657489944](https://github.com/bavanchun/AriadShift/actions/runs/37657489944)) and on PR [#17](https://github.com/bavanchun/AriadShift/pull/17) ([CI #37662984764](https://github.com/bavanchun/AriadShift/actions/runs/37662984764), documented in §4).

| Commit | Run | Job | Job ID | Conclusion | URL |
|---|---|---|---|---|---|
| `e0d770f` | `#37661747165` | `Scope` | `112930735159` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37661747165/job/112930735159> |
| `e0d770f` | `#37661747165` | `Static checks` | `112930796224` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37661747165/job/112930796224> |
| `e0d770f` | `#37661747165` | `Lint (windows-2025)` | `112930798486` | `skipped` (docs-only scope) | <https://github.com/bavanchun/AriadShift/actions/runs/37661747165/job/112930798486> |
| `e0d770f` | `#37661747165` | `Verify (${{ matrix.os }})` | `112930798691` | `skipped` (docs-only scope) | <https://github.com/bavanchun/AriadShift/actions/runs/37661747165/job/112930798691> |
| `e0d770f` | `#37661747165` | `Fuzz` | `112930798863` | `skipped` (docs-only scope) | <https://github.com/bavanchun/AriadShift/actions/runs/37661747165/job/112930798863> |
| `e0d770f` | `#37661747165` | `CI passed` | `112931409227` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37661747165/job/112931409227> |

### Full Three-OS CI run on `dev` code tree (`e37250d`, parent of `e0d770f`)

- **Workflow run:** [CI #37657489944](https://github.com/bavanchun/AriadShift/actions/runs/37657489944) — `conclusion: success`

| Commit | Run | Job | Runner | Job ID | Conclusion | URL |
|---|---|---|---|---|---|---|
| `e37250d` | `#37657489944` | `Scope` | `ubuntu-26.04` | `112916197827` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112916197827> |
| `e37250d` | `#37657489944` | `Static checks` | `ubuntu-26.04` | `112916269219` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112916269219> |
| `e37250d` | `#37657489944` | `Lint (windows-2025)` | `windows-2025` | `112916269113` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112916269113> |
| `e37250d` | `#37657489944` | `Verify (ubuntu-26.04)` | `ubuntu-26.04` | `112916269185` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112916269185> |
| `e37250d` | `#37657489944` | `Verify (macos-26)` | `macos-26` | `112916269211` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112916269211> |
| `e37250d` | `#37657489944` | `Verify (windows-2025)` | `windows-2025` | `112916269234` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112916269234> |
| `e37250d` | `#37657489944` | `Fuzz` | `ubuntu-26.04` | `112916269250` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112916269250> |
| `e37250d` | `#37657489944` | `CI passed` | `ubuntu-26.04` | `112919758825` | `success` | <https://github.com/bavanchun/AriadShift/actions/runs/37657489944/job/112919758825> |

### CodeQL results

- **On `e0d770f`:** [CodeQL #37661747157](https://github.com/bavanchun/AriadShift/actions/runs/37661747157) — `conclusion: success`
  - `Analyze (actions)` (`112930736171`): `success` (<https://github.com/bavanchun/AriadShift/actions/runs/37661747157/job/112930736171>)
  - `Analyze (rust)` (`112930736468`): `success` (<https://github.com/bavanchun/AriadShift/actions/runs/37661747157/job/112930736468>)
- **On `e37250d`:** [CodeQL #37657489912](https://github.com/bavanchun/AriadShift/actions/runs/37657489912) — `conclusion: success`
  - `Analyze (actions)` (`112916202411`): `success` (<https://github.com/bavanchun/AriadShift/actions/runs/37657489912/job/112916202411>)
  - `Analyze (rust)` (`112916202680`): `success` (<https://github.com/bavanchun/AriadShift/actions/runs/37657489912/job/112916202680>)

---

## 2. Benchmark Diff vs. Committed `capabilities.json`

### Commands used

```bash
git log --oneline -p crates/ariad-core/data/capabilities.json
gh run list --repo bavanchun/AriadShift --workflow bench.yml --limit 30 \
  --json databaseId,name,workflowName,status,conclusion,url,headSha,headBranch,event,createdAt
gh run view 37662985171 --repo bavanchun/AriadShift --log
```

### Where `capabilities.json` and the bench diff live

1. **Committed capability data:** `crates/ariad-core/data/capabilities.json`
   - Created as an unmeasured bootstrap (`"metrics": null` on all 8 edges) in Phase 7 commit `e55a0cc` (`feat(core): add the capabilities graph and planner`).
   - Populated with measured metrics in Phase 8 commit `d766505` (`feat(bench): commit measured capabilities and run the harness in CI`, landed on `dev` via PR [#12](https://github.com/bavanchun/AriadShift/pull/12) merge commit `20cea914c4124afcabf0db9308c2b0c4d5de654c`), with `"generated_at": "2026-10-07T03:29:05Z"`, `"fixture_count": 41`, `"pandoc_version": "3.12"`, `"ashift_version": "0.0.0"`.
   - **No commit since `d766505` (through `e0d770f` on `dev` and `e3a1c78` on PR #17) has modified `crates/ariad-core/data/capabilities.json`**, and **no standalone bench diff artifact file is committed in the repository**.
2. **CI `bench diff` execution (`.github/workflows/bench.yml`):**
   - `bench/src/ariad_bench/diff.py` (`uv run --package ariad-bench python -m ariad_bench diff`) compares freshly measured capability numbers against `HEAD:crates/ariad-core/data/capabilities.json` and appends a Markdown table to `$GITHUB_STEP_SUMMARY` in `.github/workflows/bench.yml`.
   - Across all `Benchmark` workflow runs, **exactly one run executed `Benchmark (Ubuntu)` to completion**: [Benchmark #37662985171](https://github.com/bavanchun/AriadShift/actions/runs/37662985171) (job [`112934950616`](https://github.com/bavanchun/AriadShift/actions/runs/37662985171/job/112934950616), `conclusion: success`) on PR [#17](https://github.com/bavanchun/AriadShift/pull/17) (`chore/prepare-0.1.0`, SHA `e3a1c78fb8716da80a910aef16ad5d09fdcf0688`). All other `bench.yml` runs on unlabelled PRs were `skipped`.

### Comparison: CI Run `#37662985171` vs. Committed `crates/ariad-core/data/capabilities.json` (`d766505`)

- **Deterministic metrics (`fidelity`, `editability`, `samples`):** **Zero drift (`0.000` delta) across all 8 planner edges.**
- **Environment-sensitive metrics (`p50_ms`, `peak_mem_mb`):** Minor expected differences between the local workstation that recorded `d766505` and the `ubuntu-26.04` GitHub Actions runner:

| Edge | Engine | Fidelity (CI vs Committed) | Editability (CI vs Committed) | p50 Latency (CI vs Committed) | Peak Memory (CI vs Committed) | Samples |
|---|---|---|---|---|---|---|
| `ariad-ir+json → docx` | `pandoc` | `0.991` (no change) | `0.971` (no change) | `120 ms` (`+20 ms` vs `100 ms`) | `231 MB` (`+15 MB` vs `216 MB`) | `30` |
| `ariad-ir+json → epub` | `pandoc` | `0.851` (no change) | `0.786` (no change) | `90 ms` (`+10 ms` vs `80 ms`) | `256 MB` (`+14 MB` vs `242 MB`) | `28` |
| `ariad-ir+json → html` | `ariad-core` | `1.000` (no change) | `1.000` (no change) | `0 ms` (no change) | `9 MB` (`+1 MB` vs `8 MB`) | `30` |
| `ariad-ir+json → markdown` | `ariad-core` | `0.989` (no change) | `0.981` (no change) | `0 ms` (no change) | `9 MB` (`+1 MB` vs `8 MB`) | `13` |
| `docx → ariad-ir+json` | `pandoc` | `1.000` (no change) | `1.000` (no change) | `190 ms` (`-10 ms` vs `200 ms`) | `150 MB` (`+5 MB` vs `145 MB`) | `5` |
| `epub → ariad-ir+json` | `pandoc` | `1.000` (no change) | `1.000` (no change) | `80 ms` (no change) | `52 MB` (`+8 MB` vs `44 MB`) | `4` |
| `html → ariad-ir+json` | `ariad-core` | `1.000` (no change) | `1.000` (no change) | `0 ms` (no change) | `9 MB` (`+1 MB` vs `8 MB`) | `4` |
| `markdown → ariad-ir+json` | `ariad-core` | `1.000` (no change) | `1.000` (no change) | `0 ms` (no change) | `9 MB` (no change) | `28` |

---

## 3. Phase 12 Dry-Run Evidence and Local Verification Checks

### Commands used

```bash
gh run list --repo bavanchun/AriadShift --workflow release.yml --limit 20 \
  --json databaseId,name,workflowName,status,conclusion,url,headSha,headBranch,event,createdAt
for run_id in 37655876867 37658877474 37662985298; do
  gh run view "$run_id" --repo bavanchun/AriadShift \
    --json databaseId,url,headBranch,headSha,conclusion,jobs
  gh api "repos/bavanchun/AriadShift/actions/runs/$run_id/artifacts" \
    --jq '.artifacts[]|{id,name,size_in_bytes,expired,created_at}'
done
just check-release-workflow
UV_NO_CONFIG=1 just check-release-workflow
just test-winget-manifest
```

### Phase 12 `release.yml` dry-run workflow runs (`pr-run-mode = "upload"`)

Phase 12 (`plans/261006-0544-docling-spike-and-roadmap-1a/phase-12-release-pipeline.md`) verified `.github/workflows/release.yml` across three PR dry runs (plus an initial rehearsal push `#37657677356` on `c86c60c9066dc6463f0744431732240fb54c7282`):

1. **Wave 7 release-pipeline PR [#14](https://github.com/bavanchun/AriadShift/pull/14)** (`feat/release-pipeline`, SHA `cda5a116b1fd5f10c4c22774961d41c48e713128`):
   - **Run:** [Release #37655876867](https://github.com/bavanchun/AriadShift/actions/runs/37655876867) — `conclusion: success`
2. **Throwaway version-bump rehearsal PR [#15](https://github.com/bavanchun/AriadShift/pull/15)** (`release/dry-run`, SHA `f222d72c304698db2fc5f56ca147c7d2871ef98e`, closed without merging per Phase 12 step 7):
   - **Run:** [Release #37658877474](https://github.com/bavanchun/AriadShift/actions/runs/37658877474) — `conclusion: success`
3. **Real version-bump PR [#17](https://github.com/bavanchun/AriadShift/pull/17)** (`chore/prepare-0.1.0`, SHA `e3a1c78fb8716da80a910aef16ad5d09fdcf0688`):
   - **Run:** [Release #37662985298](https://github.com/bavanchun/AriadShift/actions/runs/37662985298) — `conclusion: success`

#### Per-job results across the three dry runs

| Job | Runner | PR #14 (`#37655876867`) | PR #15 Rehearsal (`#37658877474`) | PR #17 (`#37662985298`) |
|---|---|---|---|---|
| `plan` | `ubuntu-26.04` | `success` (`112910231883`) | `success` (`112920964229`) | `success` (`112934978022`) |
| `build-local-artifacts (aarch64-apple-darwin)` | `macos-26` | `success` (`112910596135`) | `success` (`112921189233`) | `success` (`112935220245`) |
| `build-local-artifacts (x86_64-apple-darwin)` | `macos-26` | `success` (`112910596893`) | `success` (`112921189200`) | `success` (`112935220364`) |
| `build-local-artifacts (x86_64-unknown-linux-musl)` | `ubuntu-26.04` | `success` (`112910596531`) | `success` (`112921189392`) | `success` (`112935220516`) |
| `build-local-artifacts (aarch64-unknown-linux-musl)` | `ubuntu-26.04-arm` | `success` (`112910596373`) | `success` (`112921189333`) | `success` (`112935220136`) |
| `build-local-artifacts (x86_64-pc-windows-msvc)` | `windows-2025` | `success` (`112910596271`) | `success` (`112921189246`) | `success` (`112935220104`) |
| `build-global-artifacts` | `ubuntu-26.04` | `success` (`112913047163`) | `success` (`112923153674`) | `success` (`112937215426`) |
| `host` | `ubuntu-26.04` | `skipped` (`112913309935`) | `skipped` (`112923405006`) | `skipped` (`112937474417`) |
| `prepare-homebrew-formula` | `ubuntu-26.04` | `skipped` (`112913323704`) | `skipped` (`112923405171`) | `skipped` (`112937475364`) |
| `publish-homebrew-formula` | `ubuntu-26.04` | `skipped` (`112913335149`) | `skipped` (`112923406643`) | `skipped` (`112937476964`) |
| `custom-publish-crates` | reusable | `skipped` (`112913323619`) | `skipped` (`112923405184`) | `skipped` (`112937475864`) |
| `announce` | `ubuntu-26.04` | `skipped` (`112913346379`) | `skipped` (`112923407408`) | `skipped` (`112937478187`) |

#### Uploaded artifacts (`expired: false`)

| Artifact name | PR #14 (`#37655876867`) ID / Size | PR #15 (`#37658877474`) ID / Size | PR #17 (`#37662985298`) ID / Size |
|---|---|---|---|
| `cargo-dist-cache` | `11498496998` (10,166,971 B) | `11500350918` (10,166,971 B) | `11500878773` (10,166,971 B) |
| `artifacts-plan-dist-manifest` | `11498571748` (2,329 B) | `11499537788` (2,329 B) | `11502275465` (2,329 B) |
| `artifacts-build-local-aarch64-apple-darwin` | `11498881031` (2,943,860 B) | `11499568503` (2,944,708 B) | `11501613030` (2,943,424 B) |
| `artifacts-build-local-x86_64-apple-darwin` | `11498592281` (3,196,858 B) | `11500262583` (3,205,110 B) | `11500958640` (3,190,642 B) |
| `artifacts-build-local-x86_64-unknown-linux-musl` | `11499360120` (3,621,683 B) | `11500500728` (3,628,228 B) | `11501103387` (3,626,076 B) |
| `artifacts-build-local-aarch64-unknown-linux-musl` | `11498631910` (3,301,140 B) | `11499778069` (3,299,412 B) | `11502315582` (3,294,727 B) |
| `artifacts-build-local-x86_64-pc-windows-msvc` | `11498344460` (4,566,756 B) | `11499168819` (4,567,920 B) | `11501138730` (4,567,866 B) |
| `artifacts-build-global` | `11499650168` (13,672,837 B) | `11500441243` (13,673,649 B) | `11500879633` (13,673,182 B) |

### Local execution of `just check-release-workflow` and `just test-winget-manifest`

1. **`just check-release-workflow`:**
   - Running plain `just check-release-workflow` on this workstation exits with code `2` because Homebrew `uv` on this machine is `0.12.21` while root `pyproject.toml` pins `required-version = "==0.12.23"`, and `scripts/check-release-workflow.sh` invokes `uv run --no-project --with pyyaml==6.0.3` without `--no-config`:
     ```text
     sh scripts/check-release-workflow.sh
     error: Required uv version `==0.12.23` does not match the running version
            `0.12.21`
     error: recipe `check-release-workflow` failed on line 70 with exit code 2
     ```
   - Running `UV_NO_CONFIG=1 just check-release-workflow` **passes (`exit 0`)**. Output tail:
     ```text
     === Malformed YAML and Structural Shape Mutations ===
     PASS | step-run-non-string-integer                | Step with non-string integer run value
     PASS | checkout-with-non-mapping-string           | actions/checkout step with non-mapping string with value
     PASS | install-action-tool-non-string-integer     | install-action step with non-string integer tool value
     PASS | install-action-tool-non-string-sequence    | install-action step with non-string sequence tool value
     PASS | step-name-non-string-sequence              | Step with non-string sequence name value
     PASS | publish-crates-token-in-job-env            | publish-crates.yml has CARGO_REGISTRY_TOKEN in job-level env

     === Control Test (Semantic Equivalence) ===
     PASS | jobs-reordered-valid                       | Reordered jobs mapping passes check

     === Test Summary ===
     Total tests run: 80
     Tests passed (PASS): 78
     Tests skipped (SKIP): 2
     Tests failed (FAIL): 0
     SUCCESS: All mutations were rejected and all controls passed.
     ```
     *(The 2 skipped tests are the optional `pwsh` PowerShell AST parser checks when `pwsh` is not installed on the local macOS host; CI runs them with `pwsh` installed.)*

2. **`just test-winget-manifest`:**
   - **Passes (`exit 0`)**. Full output:
     ```text
     sh scripts/test-winget-manifest.sh
     === Test 1: No arguments should fail ===
     PASS
     === Test 2: Invalid version formats should fail ===
     PASS
     === Test 2b: Version with newline should fail and leave no 0-byte files ===
     PASS
     === Test 3: Nonexistent archive path should fail ===
     PASS
     === Test 4: Missing archive and no online asset should fail ===
     PASS
     === Test 5: Valid version and local archive should succeed ===
     PASS
     All winget-manifest tests passed.
     ```

---

## 4. PR #17 (`chore/prepare-0.1.0`) Status, CI Jobs, and Exact Diff vs. `dev`

### Commands used

```bash
gh pr view 17 --repo bavanchun/AriadShift \
  --json number,title,state,mergeable,mergeStateStatus,headRefName,baseRefName,headRefOid,statusCheckRollup,url
git log --format="%H %s" origin/dev..origin/chore/prepare-0.1.0
git merge-base origin/dev origin/chore/prepare-0.1.0
gh pr diff 17 --repo bavanchun/AriadShift
```

### PR #17 metadata

- **URL:** <https://github.com/bavanchun/AriadShift/pull/17>
- **Title:** `chore(release): prepare 0.1.0`
- **State:** `OPEN`
- **Mergeability:** `mergeable: MERGEABLE`, `mergeStateStatus: CLEAN`
- **Base branch:** `dev` (`e0d770fedaa816d6fafc7df9dd7287ba674afa0f`)
- **Head branch:** `chore/prepare-0.1.0` (`e3a1c78fb8716da80a910aef16ad5d09fdcf0688`)
- **Commits (2):**
  1. `84ca1eaadeb73a6ed0da9bff0cfb5710674ef3ed` — `test(cli): read the expected version from the package`
  2. `e3a1c78fb8716da80a910aef16ad5d09fdcf0688` — `chore(release): prepare 0.1.0`

### PR #17 CI status per job

| Workflow | Run ID | Job | Conclusion | URL |
|---|---|---|---|---|
| `CI` | `37662984764` | `Scope` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112934954519> |
| `CI` | `37662984764` | `Static checks` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112935028713> |
| `CI` | `37662984764` | `Lint (windows-2025)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112935028717> |
| `CI` | `37662984764` | `Verify (ubuntu-26.04)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112935028727> |
| `CI` | `37662984764` | `Verify (macos-26)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112935028705> |
| `CI` | `37662984764` | `Verify (windows-2025)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112935028918> |
| `CI` | `37662984764` | `Fuzz` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112935028870> |
| `CI` | `37662984764` | `CI passed` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984764/job/112938537244> |
| `CodeQL` | `37662984830` | `Analyze (actions)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984830/job/112934939078> |
| `CodeQL` | `37662984830` | `Analyze (rust)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662984830/job/112934938598> |
| `Benchmark` | `37662985171` | `Benchmark (Ubuntu)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985171/job/112934950616> |
| `Release` | `37662985298` | `plan` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298/job/112934978022> |
| `Release` | `37662985298` | `build-local-artifacts (aarch64-apple-darwin)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298/job/112935220245> |
| `Release` | `37662985298` | `build-local-artifacts (x86_64-apple-darwin)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298/job/112935220364> |
| `Release` | `37662985298` | `build-local-artifacts (x86_64-unknown-linux-musl)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298/job/112935220516> |
| `Release` | `37662985298` | `build-local-artifacts (aarch64-unknown-linux-musl)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298/job/112935220136> |
| `Release` | `37662985298` | `build-local-artifacts (x86_64-pc-windows-msvc)` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298/job/112935220104> |
| `Release` | `37662985298` | `build-global-artifacts` | `SUCCESS` | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298/job/112937215426> |
| `Release` | `37662985298` | `host` / `prepare-homebrew-formula` / `publish-homebrew-formula` / `custom-publish-crates` / `announce` | `SKIPPED` (PR mode) | <https://github.com/bavanchun/AriadShift/actions/runs/37662985298> |

*(Note: an earlier `Benchmark (Ubuntu)` run `37662984885` on the `opened` event was automatically `CANCELLED` by concurrency when the `bench` label event triggered run `37662985171` four seconds later.)*

### Exact diff of PR #17 vs. `origin/dev`

```diff
diff --git a/Cargo.lock b/Cargo.lock
index a65cca0..53c0b0b 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -119,7 +119,7 @@ dependencies = [
 
 [[package]]
 name = "ariad-cli"
-version = "0.0.0"
+version = "0.1.0"
 dependencies = [
  "ariad-core",
  "ariad-host",
@@ -145,7 +145,7 @@ dependencies = [
 
 [[package]]
 name = "ariad-core"
-version = "0.0.0"
+version = "0.1.0"
 dependencies = [
  "ammonia",
  "base64",
@@ -167,7 +167,7 @@ dependencies = [
 
 [[package]]
 name = "ariad-host"
-version = "0.0.0"
+version = "0.1.0"
 dependencies = [
  "ariad-core",
  "cap-std",
diff --git a/Cargo.toml b/Cargo.toml
index 71b67dd..4679bac 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -4,7 +4,7 @@ exclude = ["fuzz"]
 resolver = "3"
 
 [workspace.package]
-version = "0.0.0"
+version = "0.1.0"
 edition = "2024"
 rust-version = "1.99"
 license = "Apache-2.0"
@@ -13,8 +13,8 @@ publish = false
 
 [workspace.dependencies]
 ammonia = "4.2.1"
-ariad-core = { path = "crates/ariad-core", version = "0.0.0" }
-ariad-host = { path = "crates/ariad-host", version = "0.0.0" }
+ariad-core = { path = "crates/ariad-core", version = "0.1.0" }
+ariad-host = { path = "crates/ariad-host", version = "0.1.0" }
 assert_cmd = "2.2.2"
 axum = "0.8"
 base64 = "0.23.1"
diff --git a/THIRD_PARTY_LICENSES b/THIRD_PARTY_LICENSES
index 3753666..5bc1207 100644
--- a/THIRD_PARTY_LICENSES
+++ b/THIRD_PARTY_LICENSES
@@ -4024,9 +4024,9 @@ License: Apache License 2.0 (Apache-2.0)
 --------------------------------------------------------------------------------
 
 Used by:
-  - ariad-cli 0.0.0 (https://github.com/bavanchun/AriadShift)
-  - ariad-core 0.0.0 (https://github.com/bavanchun/AriadShift)
-  - ariad-host 0.0.0 (https://github.com/bavanchun/AriadShift)
+  - ariad-cli 0.1.0 (https://github.com/bavanchun/AriadShift)
+  - ariad-core 0.1.0 (https://github.com/bavanchun/AriadShift)
+  - ariad-host 0.1.0 (https://github.com/bavanchun/AriadShift)
   - arraydeque 0.5.1 (https://github.com/andylokandy/arraydeque)
   - dtoa 1.0.11 (https://github.com/dtolnay/dtoa)
   - dyn-clone 1.0.20 (https://github.com/dtolnay/dyn-clone)
diff --git a/crates/ariad-cli/tests/cli_smoke.rs b/crates/ariad-cli/tests/cli_smoke.rs
index 0d96978..8cee352 100644
--- a/crates/ariad-cli/tests/cli_smoke.rs
+++ b/crates/ariad-cli/tests/cli_smoke.rs
@@ -10,6 +10,6 @@ fn version_reports_the_binary_name_and_package_version() {
     assert!(output.status.success());
     assert_eq!(
         String::from_utf8_lossy(&output.stdout).trim(),
-        "ashift 0.0.0"
+        concat!("ashift ", env!("CARGO_PKG_VERSION"))
     );
 }
```

---

## 5. Promotion Pre-Check (`dev` → `main`)

### Commands used

```bash
git log --oneline origin/dev..origin/main
git merge-base origin/main origin/dev
git merge-tree --write-tree origin/main origin/dev
echo "merge-tree exit: $?"
git merge-tree --write-tree origin/main origin/chore/prepare-0.1.0
echo "merge-tree PR17->main exit: $?"
```

### Results

- **Commits on `origin/main` not in `origin/dev` (2 merge commits from prior promotions):**
  1. `132cbe52e6446db642d23257f82b0b708ea7cc4a` — `Merge pull request #3 from bavanchun/dev` (`chore: promote dev to main`, parents `fad8fc2` and `2a7247a`)
  2. `385926e2ba2f08dac847192a3a69582c10beb7b6` — `Merge pull request #6 from bavanchun/dev` (`chore: promote dev to main`, parents `132cbe5` and `eb311d6`)
- **Merge base (`origin/main`, `origin/dev`):** `eb311d6075829c0be3fffc4e81ac11d125bf8056`
- **Read-only merge check (`git merge-tree --write-tree origin/main origin/dev`):**
  - **Exit code:** `0`
  - **Resulting tree OID:** `6d63d0f42078d6d0281cfbb7c50b741babb1797b`
  - **Conflicts:** **None (merges cleanly).**
- **Read-only merge check including PR #17 (`git merge-tree --write-tree origin/main origin/chore/prepare-0.1.0`):**
  - **Exit code:** `0`
  - **Resulting tree OID:** `e5a0670d8375ffdbc2d6a74782c050c821cf58d7`
  - **Conflicts:** **None (merges cleanly).**

---

## 6. Release Prerequisites, Secrets, Environments, and Triggers

### Commands used

```bash
gh api repos/bavanchun/homebrew-tap --jq '{full_name,private,html_url}'
gh api repos/bavanchun/homebrew-tap/contents/
gh api repos/bavanchun/AriadShift/environments --jq '.environments[]|{name,protection_rules}'
gh api repos/bavanchun/AriadShift/environments/release/deployment-branch-policies
gh api repos/bavanchun/AriadShift/environments/release/secrets
gh api repos/bavanchun/AriadShift/actions/secrets
gh api repos/bavanchun/winget-pkgs --jq '{full_name,fork,parent:.parent.full_name,html_url}'
```

### Workflow triggers, environments, and secret names (names only)

| Workflow file | Trigger | Job(s) using environment / secrets | Environment name | Secret name(s) required | Scope / exposure |
|---|---|---|---|---|---|
| `.github/workflows/release.yml` | `push.tags: ['**[0-9]+.[0-9]+.[0-9]+*']` (e.g. `v0.1.0`); `pull_request` on release paths (build-only) | `plan`, `build-local-artifacts`, `build-global-artifacts`, `host`, `prepare-homebrew-formula`, `publish-homebrew-formula`, `announce` | *(none, except `publish-homebrew-formula`)* | `GITHUB_TOKEN` (built-in) | `contents: read` at workflow level; `contents: write` on `host` only |
| `.github/workflows/release.yml` | Same tag push | `publish-homebrew-formula` | `release` | `HOMEBREW_TAP_TOKEN` | Exposed **only** in step `Push formula` (`step.env`); fine-grained PAT with `contents: write` on `bavanchun/homebrew-tap` only |
| `.github/workflows/release.yml` → `.github/workflows/publish-crates.yml` | `workflow_call` from `custom-publish-crates` job in `release.yml` | `publish` (`Publish to crates.io`) | `release` | `CARGO_REGISTRY_TOKEN` | Exposed **only** in steps `Validate registry token` and `Publish crates in dependency order`; crates.io scoped token (`publish-new` for first release, `publish-update` afterwards) for `ariad-core`, `ariad-host`, `ariad-cli` |
| `.github/workflows/release-smoke.yml` | `workflow_dispatch` with input `version` (e.g. `0.1.0`) | `smoke-homebrew`, `smoke-shell`, `smoke-powershell`, `smoke-binstall` | *(none)* | *(none; read-only)* | `contents: read` |

### Live status of external prerequisites before tagging `v0.1.0`

| Prerequisite | Required state | Current live state (verified via `gh api`) | Action needed before tagging |
|---|---|---|---|
| Public tap repository `bavanchun/homebrew-tap` | Public repo with `main` branch | **Already exists** (<https://github.com/bavanchun/homebrew-tap>, `private: false`, `main` branch contains `README.md` `9b3d2e0c5dea7b2bc32daaee2afbe71f7d38c945`) | **None** (already created) |
| GitHub Actions environment `release` | Environment on `bavanchun/AriadShift` with `v*` tag deployment protection rule | **Already exists** (`protection_rules: branch_policy`, tag rule `id: 62283135`, `name: "v*"`, `type: "tag"`) | **None** (already created and restricted to `v*` tags) |
| Environment secret `HOMEBREW_TAP_TOKEN` | Set in `release` environment (`gh secret set HOMEBREW_TAP_TOKEN --env release`) | **NOT SET** (`environments/release/secrets` has `total_count: 0`) | **User must create token and set `HOMEBREW_TAP_TOKEN` in `release` environment** |
| Environment secret `CARGO_REGISTRY_TOKEN` | Set in `release` environment (`gh secret set CARGO_REGISTRY_TOKEN --env release`) | **NOT SET** (`environments/release/secrets` has `total_count: 0`; repo `actions/secrets` also has `total_count: 0`) | **User must create crates.io token and set `CARGO_REGISTRY_TOKEN`** (see pre-flight note below) |
| Fork `bavanchun/winget-pkgs` | Fork of `microsoft/winget-pkgs` under user's account | **Does not exist yet** (`HTTP 404` on `repos/bavanchun/winget-pkgs`) | Needed after `v0.1.0` tag for step 7 (WinGet submission); `wingetcreate submit` or `gh repo fork microsoft/winget-pkgs` creates it |

> **Pre-flight note on `CARGO_REGISTRY_TOKEN` and `workflow_call`:**
> In `.github/workflows/release.yml` (lines 469–479), `custom-publish-crates` calls `uses: ./.github/workflows/publish-crates.yml` and passes `secrets: CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}` by name to the reusable workflow, whose `publish` job sets `environment: release`. This is the documented GitHub pattern (environment secret passed by name to a reusable workflow whose job sets `environment: release`; the environment value wins — see `docs.github.com`, "Creating a reusable workflow" > "Using inputs and secrets in a reusable workflow"). Because the publish jobs are skipped on PR dry runs, this path is first exercised on the `v0.1.0` tag, failing closed at `Validate registry token` before any `cargo publish` if the secret is missing or empty and leaving the GitHub Release and Homebrew tap intact.

---

## 7. WinGet Path (`scripts/winget-manifest.sh`, `wingetcreate`, and Fork Submission Steps)

### Commands used

```bash
gh release view --repo microsoft/winget-create --json tagName,assets \
  --jq '{tagName,assets:[.assets[].name]}'
```

### What `scripts/winget-manifest.sh` needs

- **Invocation:** `scripts/winget-manifest.sh <version> [archive_path] [output_dir]`
  - `<version>`: Strict `X.Y.Z` SemVer without leading `v` (i.e. `0.1.0`).
  - `[archive_path]` (optional): Local path to `ariad-cli-x86_64-pc-windows-msvc.zip`. When omitted, the script fetches `https://github.com/bavanchun/AriadShift/releases/download/v0.1.0/ariad-cli-x86_64-pc-windows-msvc.zip.sha256` via `curl` after the GitHub Release is published and validates the 64-character hex SHA-256 digest.
  - `[output_dir]` (optional): Defaults to `target/winget/<version>/` (`target/winget/0.1.0/`).
- **Templates rendered (`packaging/winget/*.yaml.tmpl`, schema `1.9.0`, package identifier `VChun.AriadShift`, moniker `ashift`):**
  1. `target/winget/0.1.0/VChun.AriadShift.yaml` (`ManifestType: version`)
  2. `target/winget/0.1.0/VChun.AriadShift.installer.yaml` (`ManifestType: installer`, `InstallerType: zip`, `NestedInstallerType: portable`, `RelativeFilePath: ashift.exe`, `PortableCommandAlias: ashift`, `Architecture: x64`)
  3. `target/winget/0.1.0/VChun.AriadShift.locale.en-US.yaml` (`ManifestType: defaultLocale`, `Publisher: VChun`, `PackageName: AriadShift`, `License: Apache-2.0`)

### Can `wingetcreate` run on macOS or does it need a Windows runner?

- **`wingetcreate` is Windows-only and cannot run natively on macOS.**
- Latest release `microsoft/winget-create` `v1.12.13.0` (MIT) publishes only Windows assets:
  - `wingetcreate.exe`
  - `Microsoft.WindowsPackageManagerManifestCreator_1.12.13.0_8wekyb3d8bbwe.msixbundle`
- Because `scripts/winget-manifest.sh 0.1.0` already generates the complete, schema-1.9.0-compliant 3-file multi-manifest directory locally on macOS (without needing `wingetcreate new`), the coordinator/user has two execution paths for submission:
  - **Path A (from macOS host via Git + `gh`, or on a Windows machine/runner with `wingetcreate.exe submit`):** Commit the 3 files generated by `scripts/winget-manifest.sh 0.1.0` into `manifests/v/VChun/AriadShift/0.1.0/` on a branch of `bavanchun/winget-pkgs` and open the PR to `microsoft/winget-pkgs` (or run `wingetcreate.exe submit --token <PAT> target/winget/0.1.0` on Windows / a `windows-2025` runner).

### Exact submission steps for the user's fork (`bavanchun/winget-pkgs`)

1. **Generate the manifests after `v0.1.0` is published on GitHub Releases:**
   ```bash
   scripts/winget-manifest.sh 0.1.0
   ```
   Verify that `target/winget/0.1.0/` contains `VChun.AriadShift.yaml`, `VChun.AriadShift.installer.yaml`, and `VChun.AriadShift.locale.en-US.yaml` with the real 64-hex SHA-256 of `ariad-cli-x86_64-pc-windows-msvc.zip`.
2. **Option 1 — Using `wingetcreate.exe` on Windows (or a `windows-2025` runner):**
   ```powershell
   Invoke-WebRequest -Uri "https://github.com/microsoft/winget-create/releases/download/v1.12.13.0/wingetcreate.exe" -OutFile wingetcreate.exe
   .\wingetcreate.exe submit --token $env:WINGET_GITHUB_TOKEN .\target\winget\0.1.0
   ```
   (`wingetcreate submit` automatically forks `microsoft/winget-pkgs` into `bavanchun/winget-pkgs` if absent, pushes `manifests/v/VChun/AriadShift/0.1.0/`, and opens the PR.)
3. **Option 2 — Direct submission from macOS using `gh` and `git` (since `scripts/winget-manifest.sh 0.1.0` already renders the 3 YAML manifests):**
   ```bash
   gh repo fork microsoft/winget-pkgs --clone=false
   tmpdir=$(mktemp -d)
   git clone --depth=1 --filter=blob:none https://github.com/bavanchun/winget-pkgs.git "$tmpdir/winget-pkgs"
   cd "$tmpdir/winget-pkgs"
   git switch -c vchun-ariadshift-0.1.0
   mkdir -p manifests/v/VChun/AriadShift/0.1.0
   cp /path/to/AriadShift/target/winget/0.1.0/*.yaml manifests/v/VChun/AriadShift/0.1.0/
   git add manifests/v/VChun/AriadShift/0.1.0/VChun.AriadShift.yaml \
           manifests/v/VChun/AriadShift/0.1.0/VChun.AriadShift.installer.yaml \
           manifests/v/VChun/AriadShift/0.1.0/VChun.AriadShift.locale.en-US.yaml
   git commit -m "New package: VChun.AriadShift version 0.1.0"
   git push -u origin vchun-ariadshift-0.1.0
   gh pr create --repo microsoft/winget-pkgs \
     --head bavanchun:vchun-ariadshift-0.1.0 \
     --base master \
     --title "New package: VChun.AriadShift version 0.1.0" \
     --body "Submitting VChun.AriadShift v0.1.0 portable zip manifest."
   ```
4. **Sign the Microsoft CLA** on the opened `microsoft/winget-pkgs` pull request when prompted by `@microsoft-github-policy-service`, and record the PR URL.

---

## 8. Coordinator Go / No-Go Checklist

| # | Gate item | Status | Evidence / Action required |
|---|---|---|---|
| 1 | `dev` @ `e0d770f` green on 3 OSes + Fuzz + CodeQL | **GO** | `e0d770f` CI [#37661747165](https://github.com/bavanchun/AriadShift/actions/runs/37661747165) + CodeQL [#37661747157](https://github.com/bavanchun/AriadShift/actions/runs/37661747157) green; parent code commit `e37250d` 3-OS `Verify` + `Fuzz` [#37657489944](https://github.com/bavanchun/AriadShift/actions/runs/37657489944) green |
| 2 | Capability benchmark determinism (`capabilities.json`) | **GO** | Zero fidelity/editability/sample drift in CI Benchmark [#37662985171](https://github.com/bavanchun/AriadShift/actions/runs/37662985171) vs committed `d766505` |
| 3 | Phase 12 `release.yml` dry runs (5 targets + SBOMs + installers) | **GO** | Runs [#37655876867](https://github.com/bavanchun/AriadShift/actions/runs/37655876867), [#37658877474](https://github.com/bavanchun/AriadShift/actions/runs/37658877474), and [#37662985298](https://github.com/bavanchun/AriadShift/actions/runs/37662985298) all green with all 8 artifact bundles uploaded |
| 4 | Local workflow guard & winget manifest tests | **GO** | `UV_NO_CONFIG=1 just check-release-workflow` (78 PASS, 2 SKIP, 0 FAIL) and `just test-winget-manifest` (all PASS) |
| 5 | PR [#17](https://github.com/bavanchun/AriadShift/pull/17) (`chore/prepare-0.1.0`) ready to merge into `dev` | **GO** | `OPEN`, `MERGEABLE`, `CLEAN`, all 3 OS `Verify` jobs, `Fuzz`, `CodeQL`, `Benchmark`, and `Release` dry-run green |
| 6 | Promotion `dev` → `main` conflict pre-check | **GO** | `git merge-tree --write-tree origin/main origin/dev` (and with PR #17) exits `0` with zero conflicts |
| 7 | `bavanchun/homebrew-tap` repo and `release` environment (`v*` tag policy) | **GO** | Both already exist on GitHub (`bavanchun/homebrew-tap` public; `release` environment tag policy `v*` active) |
| 8 | `HOMEBREW_TAP_TOKEN` and `CARGO_REGISTRY_TOKEN` set in `release` environment | **BLOCKER (User action before tagging)** | Currently `total_count: 0` in `environments/release/secrets`. User must set both secrets before pushing tag `v0.1.0` |

---

## 9. Release Outcome and Live State (verified 2026-10-22)

### Commands used

```bash
gh release view v0.1.0 --json tagName,publishedAt,targetCommitish,assets
gh run list --workflow=release.yml --branch v0.1.0 --json databaseId,conclusion,createdAt
gh run view 39601110509 --json jobs
gh run view 39601110509 --log-failed
gh api repos/bavanchun/AriadShift/environments/release/secrets
gh api repos/bavanchun/homebrew-tap/contents/
gh api repos/bavanchun/winget-pkgs
curl -s -o /dev/null -w '%{http_code}' https://crates.io/api/v1/crates/<crate>
gh workflow run release-smoke.yml --ref main -f version=0.1.0
```

### GitHub Release

- **Release:** <https://github.com/bavanchun/AriadShift/releases/tag/v0.1.0>, published `2026-10-21T12:44:49Z`, not a draft or prerelease.
- **Tag target:** `3e5f967e654045941e68b2aa8aba8de7c5b260e2`, the promotion merge of PR [#26](https://github.com/bavanchun/AriadShift/pull/26) (`dev` at `10a5867`). That tree passed [CI #39594052685](https://github.com/bavanchun/AriadShift/actions/runs/39594052685) on `dev` (three-OS `Verify`, `Lint (windows-2025)` and `Fuzz`); the `main` push run [CI #39601059026](https://github.com/bavanchun/AriadShift/actions/runs/39601059026) skipped the rebuild because the tree is identical.
- **Assets (20):** five archives (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-musl`, `x86_64-unknown-linux-musl`, `x86_64-pc-windows-msvc`) with `.sha256` files, `ariad-cli-installer.sh`, `ariad-cli-installer.ps1`, `ashift.rb`, `ariad-cli.cdx.xml`, `ariad-core.cdx.xml`, `ariad-host.cdx.xml`, `dist-manifest.json`, `sha256.sum`, `source.tar.gz` and `source.tar.gz.sha256`.

### Release run [#39601110509](https://github.com/bavanchun/AriadShift/actions/runs/39601110509) (tag `v0.1.0`)

| Job | Job ID | Conclusion |
|---|---|---|
| `plan` | `119225538833` | `success` |
| `build-local-artifacts` (five targets) | `119225726186`, `119225726250`, `119225726345`, `119225726454`, `119225726498` | `success` |
| `build-global-artifacts` | `119228325528` | `success` |
| `host` | `119228580233` | `success` |
| `prepare-homebrew-formula` | `119228810029` | `success` |
| `custom-publish-crates / Publish to crates.io` | `119228810150` | `failure` |
| `publish-homebrew-formula` | `119229252240` | `failure` |
| `announce` | `119229356154` | `skipped` |

Failure causes, from the job logs:

- **crates.io:** `Validate registry token` passed (the secret is not empty), then `cargo publish` of `ariad-core`, the first crate, was refused: `the remote server responded with an error (status 401 Unauthorized): Invalid Authorization header: Found unexpected authentication scheme`. No crate was published, so the job can be re-run as is.
- **Homebrew:** the `Push formula` step failed with `fatal: could not read Username for 'https://github.com': No such device or address`. GitHub did not accept the basic-auth header built from `HOMEBREW_TAP_TOKEN`, and git fell back to an interactive prompt.

### Release secrets

`environments/release/secrets` now lists `HOMEBREW_TAP_TOKEN` (created `2026-10-20T04:02:59Z`, updated `04:03:40Z`) and `CARGO_REGISTRY_TOKEN` (created `2026-10-20T04:04:14Z`). The "not set" blocker in §6 and §8 no longer applies. Both secrets exist, but neither holds a token the registry or GitHub accepts. Secret values are never read; the evidence is the two errors above.

### Channel state

| Channel | State on 2026-10-22 |
|---|---|
| GitHub Release archives and installers | Published (above) |
| Homebrew `bavanchun/tap/ashift` | Not published: `bavanchun/homebrew-tap` holds only `README.md` (latest commit `7b19858`, "Initial commit") |
| crates.io `ariad-core`, `ariad-host`, `ariad-cli` | Not published: the API returns HTTP `404` for each |
| WinGet `VChun.AriadShift` | Not submitted: `repos/bavanchun/winget-pkgs` returns `404` and no pull request exists |

### Clean-runner smoke: [Release Smoke #39802059667](https://github.com/bavanchun/AriadShift/actions/runs/39802059667)

| Job | Runner | Conclusion | Reason |
|---|---|---|---|
| `smoke-shell` | `ubuntu-26.04` | `success` | Shell installer, `--version`, `doctor`, Markdown to DOCX |
| `smoke-powershell` | `windows-2025` | `success` | PowerShell installer, `--version`, `doctor`, Markdown to DOCX |
| `smoke-homebrew` | `macos-26` | `failure` | The tap has no `ashift` formula |
| `smoke-binstall` | `ubuntu-26.04` | `failure` | crates.io has no `ariad-cli` |

### Finishing the release

1. The maintainer replaces `HOMEBREW_TAP_TOKEN` and `CARGO_REGISTRY_TOKEN` in the `release` environment with working tokens (scopes in [docs/releasing.md](../../../docs/releasing.md)).
2. `gh run rerun 39601110509 --failed` re-runs `custom-publish-crates`, `publish-homebrew-formula` and `announce`. GitHub allows re-runs for 30 days, until 2026-11-20; after that, the next version tag publishes instead.
3. Run `release-smoke.yml` for `0.1.0`; all four jobs must pass.
4. Generate the WinGet manifests with `scripts/winget-manifest.sh 0.1.0` and submit them (§7).

