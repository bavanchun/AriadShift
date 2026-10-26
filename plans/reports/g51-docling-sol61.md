# Docling toolchain worker result

The task is blocked by a conflict in the brief: Static checks reject three
already-pushed commit messages, while rebasing and force-pushing are expressly
forbidden. New commits and a fast-forward push cannot repair existing messages.
Exempting those commits from the checker would hide the failure and violate the
development rules. Per the common brief, this worker stopped on the blocker
without asking the user. No implementation change or gate pass is claimed.

## Completed work and commits

- Read both ASUS briefs, repository instructions, the Git workflow, the Docling
  plan and completion phase. The completion plan was initially absent locally;
  it was read from `origin/dev` before merging.
- Merged `origin/dev` at `68ac304` without rebasing. Git resolved everything
  automatically; no conflict required intervention.
- Merge commit: `155640dc15df307271c9566d7b5c1af1a07b7022`,
  `chore: merge dev into docling toolchain fixes`.
- Diagnosed the historical Windows CMake failure and all current failed jobs.
  No implementation files were edited after identifying the policy blocker.
- The merge and this result are committed and pushed only to the authorized
  ASUS task branch, `fix/docling-drop-intel-ipp--asus`. The report commit is
  identifiable with `git log -1 -- plans/reports/g51-docling-sol61.md`.
- No PR-branch push, PR merge, release, tag or history rewrite was performed.

## CI diagnoses and outstanding repairs

### Historical Windows CMake error

[Run 40159069750](https://github.com/bavanchun/AriadShift/actions/runs/40159069750)
failed configuring the source-built LLVM OpenMP prerequisite, before torch:

```text
CMake Error at CMakeLists.txt:30 (project):
  Generator
    Visual Studio 17 2022
  could not find any instance of Visual Studio.
```

The current branch already fixes this by selecting Ninja and explicit `cl`
compilers in `engines/docling/pack/native_build.py`, with MSVC provisioned by the
workflow. The newer log confirms successful configuration and compilation; its
link command names Visual Studio `18/Enterprise`. No additional CMake-generator
fix is needed based on the latest evidence.

### Current Windows torch failure

[Job 121155285215](https://github.com/bavanchun/AriadShift/actions/runs/40188843084/job/121155285215)
built the replacement `libiomp5md.dll`, then failed linking the real MSVC probe:

```text
vcomp-probe.cpp.obj : error LNK2001: unresolved external symbol _You_must_link_with_Microsoft_OpenMP_library
openmp\runtime\src\vcomp-probe.exe : fatal error LNK1120: 1 unresolved externals
```

The link uses `/NODEFAULTLIB:vcomp` and `libiomp5md.dll.lib`. The replacement has
five `_vcomp_*` wrappers but lacks the compiler-required compatibility marker.
The next repair must supply and verify that link interface while keeping LLVM
as the sole runtime. Removing the probe or restoring proprietary Intel binaries
would violate the accepted behavior and licensing boundary.

Inspect both LLVM import libraries before fixing: LLVM generates a separate
`ompimp` library from `dllexports`, whereas this probe linked the DLL-generated
library. Torch currently consumes the other library. This additional import
surface needs verification, not an untested workaround. Relevant primary sources:
[LLVM CMake](https://github.com/llvm/llvm-project/blob/main/openmp/runtime/src/CMakeLists.txt),
[export map](https://github.com/llvm/llvm-project/blob/main/openmp/runtime/src/dllexports),
[export generator](https://github.com/llvm/llvm-project/blob/main/openmp/runtime/tools/generate-def.py).
No Windows repair was implemented or verified in this blocked run.

### Engine tests on Linux and Windows

Both Engine tests jobs in
[run 40188843138](https://github.com/bavanchun/AriadShift/actions/runs/40188843138)
fail at the same Ruff check before tests execute:

```text
E501 Line too long (101 > 100)
engines/docling/pack/licenses.py:584:101
```

Wrapping the `toolchain_release_pins` docstring is the narrow repair. The dependent
Pack jobs were skipped. No source edit was made after establishing the blocker.

### Static checks: blocking decision

[Job 121155321500](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321500)
fails in `just lint-commits`. Local inspection independently measured the same
message lengths and proved all three commits are ancestors of the pushed PR head:

| Commit | Rejected message line | Length | Limit |
|---|---|---:|---:|
| `332d4d19fed0bf126f3ec19ee24e24ddbfa02019` | Body line 3 | 242 | 72 |
| `00a1869e8454d0ae4db2610468a9d3bdf4c6b170` | Subject | 80 | 72 |
| `d48253f281c368b49ce6f637bd6f42aede2dbf17` | Body line 3 | 215 | 72 |

`scripts/check-commits.sh` checks every non-merge commit in `origin/dev..HEAD`
using `committed.toml`. Merging dev leaves the three invalid commits in that
range. A coordinator decision is required to reconcile the no-rewrite rule with
repairing those published messages. The checker was not weakened.

## Exact diagnostic commands and outcomes

```sh
git fetch origin
git log --oneline HEAD..origin/dev
git show origin/dev:plans/261025-1651-ariadshift-v1-completion/plan.md
git show origin/dev:plans/261025-1651-ariadshift-v1-completion/phase-02-docling-intel-free.md
git merge origin/dev --no-edit -m 'chore: merge dev into docling toolchain fixes'
gh auth status
gh repo view --json nameWithOwner,defaultBranchRef
gh pr view 51 --json headRefName,headRefOid,baseRefName,statusCheckRollup
gh run view 40159069750 --log-failed > /tmp/g51-torch-old-failed.log
gh run view 40188843084 --log-failed > /tmp/g51-torch-failed.log
gh run view 40188843138 --log-failed > /tmp/g51-pack-failed.log
gh run view 40188843111 --log-failed > /tmp/g51-static-failed.log
git log --format='%h %s' origin/dev..HEAD
git merge-base --is-ancestor 332d4d1 origin/fix/docling-drop-intel-ipp
git merge-base --is-ancestor 00a1869 origin/fix/docling-drop-intel-ipp
git merge-base --is-ancestor d48253f origin/fix/docling-drop-intel-ipp
gh pr checks 51 > /tmp/g51-final-checks.tsv
git diff --check
```

Fetch, merge, authentication, log downloads, ancestry checks and diff whitespace
check succeeded. `gh pr checks 51` reports failures, as shown below. A Python
inspection of `git show -s --format=%B <sha>` measured the message lines above.
The raw CI logs were copied to these durable paths:

- `/home/vchun/Codes/ariadshift-logs/g51-torch-old-failed-261027.log`
- `/home/vchun/Codes/ariadshift-logs/g51-torch-failed-261027.log`
- `/home/vchun/Codes/ariadshift-logs/g51-pack-failed-261027.log`
- `/home/vchun/Codes/ariadshift-logs/g51-static-failed-261027.log`

One diagnostic shell call was blocked by the context hook for listing a build
output path; permitted source inspections were rerun without that access. An
exploratory upstream `generate-def.pl` lookup returned HTTP 404; the actual
`generate-def.py` was then read. The first report-generation script encountered
an unexpected mise stdout line; the corrected parser accepts only check rows.

## Local gates and final CI state

No build or test gate was launched: the common brief requires stopping when
blocked. There are no local gate logs or successful terminal markers. These
required gates remain unrun:

```sh
just lint-workflows
uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pack.py
just docling-test
just docling-engine-test
just pack-docling
just pack-docling-verify
```

The brief's literal `engines/docling/pack/test_pack.py` does not exist. The actual
pack tests are `engines/docling/tests/test_pack.py`. All future heavy commands
still require the shared flock, four Cargo jobs and detached execution.

No background process, build, container, watcher or server was started. No PR
jobs were running at the final observation and no new PR run was triggered.
The table below is existing evidence for PR head `7293be4`, not verification
of the untested merge commit. It contains 5 fail, 26 pass, 10 skipping. Successful Windows torch and
four-target Pack evidence remains outstanding.

| Check | State | Duration | Evidence |
|---|---|---|---|
| CI passed | fail | 4s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121158715525) |
| CPU torch (x86_64-pc-windows-msvc) | fail | 4m50s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843084/job/121155285215) |
| Engine tests (ubuntu-26.04) | fail | 15s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843138/job/121155285300) |
| Engine tests (windows-2025) | fail | 38s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843138/job/121155285261) |
| Static checks | fail | 42s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321500) |
| Analyze (actions) | pass | 33s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843088/job/121155285076) |
| Analyze (rust) | pass | 16m19s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843088/job/121155284909) |
| CPU torch (aarch64-apple-darwin) | pass | 43s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843084/job/121155285094) |
| CPU torch (aarch64-unknown-linux-gnu) | pass | 25s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843084/job/121155285110) |
| CPU torch (x86_64-unknown-linux-gnu) | pass | 3h5m10s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843084/job/121155285192) |
| CodeQL | pass | 2s | [Job](https://github.com/bavanchun/AriadShift/runs/121155375335) |
| Desktop (macos-26) | pass | 16m22s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843133/job/121155285777) |
| Desktop (ubuntu-26.04) | pass | 18m27s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843133/job/121155285771) |
| Desktop (windows-2025) | pass | 24m26s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843133/job/121155285653) |
| Engine conformance (real engine) | pass | 5m9s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843138/job/121155285152) |
| Fuzz | pass | 16m8s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321502) |
| Lint (windows-2025) | pass | 2m16s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321511) |
| Scope | pass | 7s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155285073) |
| Scope | pass | 4s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843138/job/121155285296) |
| Server integration (ubuntu-26.04) | pass | 13m19s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321505) |
| Verify (macos-26) | pass | 8m29s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321562) |
| Verify (ubuntu-26.04) | pass | 5m9s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321490) |
| Verify (windows-2025) | pass | 6m29s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321507) |
| Web | pass | 3m14s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843111/job/121155321523) |
| build-global-artifacts | pass | 36s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121158323077) |
| build-local-artifacts (aarch64-apple-darwin) | pass | 6m26s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121155381248) |
| build-local-artifacts (aarch64-unknown-linux-musl) | pass | 5m13s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121155381319) |
| custom-publish-crates | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121158458612) |
| build-local-artifacts (x86_64-apple-darwin) | pass | 9m4s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121155381286) |
| Pack (${{ matrix.target }}) | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843138/job/121155434796) |
| Pack manifest | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843138/job/121155435648) |
| Benchmark (Ubuntu) | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843120/job/121155320269) |
| plan | pass | 25s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121155285395) |
| build-local-artifacts (x86_64-pc-windows-msvc) | pass | 13m57s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121155381413) |
| Desktop release (${{ matrix.os }}) | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843133/job/121155286447) |
| build-local-artifacts (x86_64-unknown-linux-musl) | pass | 7m39s | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121155381267) |
| Verify desktop updater release | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843133/job/121155286947) |
| announce | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121158459237) |
| host | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121158458988) |
| prepare-homebrew-formula | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121158458662) |
| publish-homebrew-formula | skipping | 0 | [Job](https://github.com/bavanchun/AriadShift/actions/runs/40188843175/job/121158459362) |

## Open concerns

1. Resolve the policy conflict over the three published commit messages.
2. Implement and verify the Windows compatibility marker and import libraries.
3. Wrap the Ruff docstring, run every local gate, fast-forward the PR branch and
   watch all required torch and Pack jobs to completion.

Status: BLOCKED
