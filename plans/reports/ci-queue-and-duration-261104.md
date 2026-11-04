# CI Queue and Job Duration Report (2026-11-04)

Empirical measurement of queue times and execution durations across the last 30 runs of `.github/workflows/ci.yml` (covering pull requests and pushes to `dev` leading up to commit `e4a02cbd`).

## Summary of Sampled Runs

- **Total runs sampled:** 30 runs (29 `pull_request`, 1 `push` to `dev`).
- **Outcomes:** 4 succeeded, 2 failed on test/lint errors, 24 cancelled prematurely by newer pushes to the same PR branches.
- **Typical end-to-end wall clock time (completed successful runs):** 33m 30s to 53m 35s.

The high cancellation rate (80%) is directly driven by long end-to-end feedback cycles: developers pushing follow-up commits within 15–35 minutes cancel earlier in-flight runs.

---

## Aggregate Job Metrics (Completed Runs)

The table below summarizes queue time (from job creation to job execution start) and run duration (from start to completion) across jobs that reached completion in the sampled dataset:

| Job Name | Samples | Avg Queue | Med Queue | Max Queue | Avg Duration | Med Duration | Max Duration |
|---|---|---|---|---|---|---|---|
| **Fuzz** | 6 | 1m 46s | 10s | 6m 19s | 28m 10s | 34m 16s | 35m 44s |
| **Server integration (ubuntu-26.04)** | 7 | 3m 49s | 5m 12s | 9m 18s | 19m 08s | 20m 34s | 22m 26s |
| **Verify (windows-2025)** | 10 | 1m 42s | 27s | 9m 18s | 14m 58s | 14m 21s | 27m 51s |
| **Verify (ubuntu-26.04)** | 9 | 2m 55s | 4s | 9m 43s | 12m 29s | 11m 52s | 23m 43s |
| **Web** | 10 | 2m 40s | 12s | 10m 15s | 11m 39s | 11m 34s | 13m 24s |
| **Verify (macos-26)** | 10 | 12m 35s | 11m 59s | 34m 44s | 9m 32s | 8m 40s | 19m 45s |
| **Lint (windows-2025)** | 13 | 1m 43s | 25s | 6m 11s | 5m 18s | 5m 52s | 7m 45s |
| **Static checks** | 15 | 2m 51s | 4s | 14m 39s | 4m 05s | 4m 19s | 5m 48s |
| **Scope** | 22 | 53s | 4s | 10m 30s | 19s | 19s | 39s |
| **CI passed** | 26 | 23s | 5s | 3m 40s | 3s | 3s | 13s |

*Note: In the full successful runs where all matrix legs and suites executed completely, run durations for `Verify (windows-2025)`, `Verify (ubuntu-26.04)`, and `Verify (macos-26)` ranged from 17m to 28m.*

---

## Detailed Breakdown of Successful Runs

| Run ID | Event & Ref | Critical Path Bottleneck | Total Wall Time | Fuzz Duration | macOS Queue + Run | Win Verify Run | Ubuntu Verify Run |
|---|---|---|---|---|---|---|---|
| `41739496367` | `pull_request` (`feat/redact`) | `Verify (macos-26)` (queue) & `Fuzz` | 34m 55s | 28m 23s | 14m 02s + 19m 45s (33m 47s) | 27m 51s | 23m 43s |
| `41720599859` | `push` (`dev`) | `Verify (macos-26)` (34m queue) & `Fuzz` | 53m 35s | 35m 44s | 34m 44s + 18m 49s (53m 33s) | 19m 12s | 21m 02s |
| `41718691179` | `pull_request` (`feat/redact`) | `Fuzz` (33m 16s execution) | 34m 54s | 33m 16s | 6m 25s + 12m 28s (18m 53s) | 17m 06s | 23m 04s |
| `41704457593` | `pull_request` (`feat/annotations-forms`) | `Fuzz` (35m 17s execution) | 39m 15s | 35m 17s | 13m 07s + 18m 10s (31m 17s) | 19m 00s | 21m 43s |

---

## Ranked List of Where Wall Time Goes

1. **`Fuzz` Job Execution Duration (28m – 36m):**
   - **Where time goes:** `fuzz` runs 19 distinct targets sequentially with `-max_total_time=60` seconds each plus compilation overhead. Even with target caches, building the fuzz harness and running 19 targets takes 28m to 35m44s.
   - **Impact:** In almost every run where macOS queue is under 15 minutes, `fuzz` is the sole gating bottleneck determining when `CI passed` can execute.

2. **`Verify (macos-26)` Runner Queuing Delay (12m – 35m queue):**
   - **Where time goes:** GitHub-hosted macOS runners (`macos-26`) experience significant concurrency limits and queue delays across GitHub Actions pools. Average queue time is 12m 35s, with peak queue time reaching 34m 44s on `dev` push `41720599859`.
   - **Impact:** Once queued, the job runs for 18m–20m. When runner wait exceeds ~15 minutes, total elapsed time for macOS Verify reaches 33m–53m, defining the overall workflow completion wall time.

3. **Multi-OS Native Verify Jobs Execution (17m – 28m):**
   - `Verify (windows-2025)`: Runs between 17m 06s and 27m 51s (compilation, Pandoc, full tests, bench-test, package-check on Windows).
   - `Verify (ubuntu-26.04)`: Runs between 21m 02s and 23m 43s (Linux native test suites, LibreOffice conformance, WASM check, package-check).
   - Both run in parallel with `Fuzz` and `Verify (macos-26)`.

4. **Server Integration Job Execution (16m – 22m):**
   - Runs PostgreSQL 18 container, SeaweedFS container, server Dockerfile checks, image build, and integration suites. Consistently finishes around 16m–21m, safely under the critical path set by `Fuzz`.

5. **Medium Jobs (4m – 13m):**
   - `Web`: 10m 30s – 13m 24s (WASM build, JS typecheck, Vitest, Playwright browser E2E across Chromium, Firefox, WebKit).
   - `Lint (windows-2025)`: 5m 18s – 7m 45s (early Windows Clippy check).
   - `Static checks`: 3m 45s – 5m 22s (formatting, typos, actionlint, zizmor, cargo-deny, security checks).

6. **Fast Gating Jobs (< 1m):**
   - `Scope`: 8s – 39s (path diff inspection, classification).
   - `CI passed`: 3s – 4s (aggregate collector).

---

## Rust Compilation vs Test Execution Analysis (sccache Evaluation Input)

To evaluate whether compiler caching (`sccache`) can accelerate CI wall times, raw job logs across completed successful and representative runs (`41739496367`, `41720599859`, `41718691179`, `41704457593`) were extracted via `gh api` and analyzed for:
1. `rust-cache` restore outcome (`full match: true`, fallback prefix match, or complete miss).
2. Cold compilation duration (Clippy checks, WASM compilation, test binary compilation preceding nextest execution, CLI binary builds, and `server.Dockerfile` container compilation during `test-server-container`).
3. Pure test execution duration (nextest test run time, bench-test run time, container smoke test execution).

### Per-Job Compilation vs Test Breakdown

| Run ID | Job Name | Cache Restore Status | Compilation Duration | Test Execution Duration | Comp % of Run |
|---|---|---|---|---|---|
| `41739496367` (PR) | **Server integration (ubuntu-26.04)** | Miss (~248 MB partial, full: false) | 12m 26s | 2m 50s | 81.4% |
| `41739496367` (PR) | **Verify (ubuntu-26.04)** | Miss (~0 MB metadata only) | 11m 36s | 3m 52s | 75.0% |
| `41739496367` (PR) | **Verify (windows-2025)** | Miss (no match) | 11m 13s | 6m 32s | 63.2% |
| `41739496367` (PR) | **Verify (macos-26)** | Prefix hit (~74 MB, full: false) | 9m 07s | 5m 10s | 63.8% |
| `41739496367` (PR) | **Fuzz** | Hit (`fuzz-target-Linux-...` ~20 MB) | 0m 24s (oracle build) | 0m 02s (oracle test) | 90.0% |
| `41720599859` (dev) | **Server integration (ubuntu-26.04)** | Miss (no match) | 16m 00s | 3m 42s | 81.2% |
| `41720599859` (dev) | **Verify (ubuntu-26.04)** | Miss (~0 MB metadata only) | 9m 08s | 3m 05s | 74.7% |
| `41720599859` (dev) | **Verify (windows-2025)** | Miss (~1.9 GB stale, full: false) | 6m 09s | 3m 49s | 61.7% |
| `41720599859` (dev) | **Verify (macos-26)** | Miss (no match) | 7m 43s | 4m 04s | 65.4% |
| `41720599859` (dev) | **Fuzz** | Miss (~14 MB stale) | 15m 20s (targets cold build) | 19m 08s (campaigns) | 44.5% |
| `41718691179` (PR) | **Server integration (ubuntu-26.04)** | Miss (no match) | 15m 33s | 3m 34s | 81.3% |
| `41718691179` (PR) | **Verify (ubuntu-26.04)** | Miss (~62 MB stale, full: false) | 11m 08s | 3m 44s | 74.9% |
| `41718691179` (PR) | **Verify (windows-2025)** | Miss (~1.9 GB stale, full: false) | 6m 54s | 4m 22s | 61.2% |
| `41718691179` (PR) | **Verify (macos-26)** | Prefix hit (~74 MB, full: false) | 5m 37s | 3m 02s | 64.9% |
| `41704457593` (PR) | **Server integration (ubuntu-26.04)** | Miss (~248 MB stale, full: false) | 11m 27s | 2m 42s | 80.9% |
| `41704457593` (PR) | **Verify (ubuntu-26.04)** | Miss (~62 MB stale, full: false) | 10m 29s | 3m 35s | 74.5% |
| `41704457593` (PR) | **Verify (windows-2025)** | Prefix hit (~74 MB, full: false) | 7m 14s | 4m 29s | 61.8% |
| `41704457593` (PR) | **Verify (macos-26)** | Prefix hit (~74 MB, full: false) | 7m 52s | 4m 09s | 65.5% |

### Cache Hit Rate Summary

- **Exact Cache Hit Rate (`full match: true`): 0%**
  Across all sampled jobs and events (both PRs and dev pushes), exact cache hit was 0%. Under `actions-rust-lang/setup-rust-toolchain`, cache keys incorporate hashes of `Cargo.lock` and rustc version. When PRs touch dependencies or when new commits arrive on branches, the exact key misses.
- **Prefix / Partial Hit Rate (`full match: false`): ~25%**
  A fallback key matches in ~25% of jobs (mainly macOS and Windows runners restoring ~74 MB or stale registry files). However, because workspace crates are purged on mismatch, all workspace crates and invalid downstream dependencies must be recompiled cold.
- **Complete Cache Miss Rate: ~75%**
  In Ubuntu Verify and Server integration, runners frequently restore ~0 MB (empty) or experience total cache misses, compiling every single dependency from scratch.
- **Container Build Cache Miss Rate: 100%**
  `just test-server-container` builds `infra/docker/server.Dockerfile` via Docker BuildKit inside a disposable GitHub runner VM. Because there is no persistent or remote Docker layer cache configured, BuildKit's cache mount starts completely empty on every run, forcing a 100% cold compilation of all 613 workspace and external crates (consuming 9m 27s to 12m 50s per run).

### Implications for sccache Evaluation

1. **Compilation is the true execution bottleneck for non-Fuzz jobs:**
   - In `Verify (ubuntu-26.04)`: ~11 minutes is compilation (Clippy 3.5m, WASM 1m, nextest binary compilation 5.7m); pure test execution takes only 3m 12s.
   - In `Server integration`: ~12 to 16 minutes is compilation (ashift build 1m, server nextest build 1.5m, Docker server compilation 10.5m); actual test execution takes under 3 minutes.
2. **Coarse-grained tarball caching (`rust-cache`) is largely ineffective for PR velocity:**
   - Tarball caching is all-or-nothing: changing `Cargo.lock` or a shared core type invalidates the entire workspace artifact set. Restoring and compressing 1–2 GB tarballs also adds 1–2 minutes of archive I/O per job.
3. **Potential impact of `sccache`:**
   - An object-level compiler cache (`sccache` backed by S3/R2 or shared storage) caches compilation at the individual `rustc` invocation level.
   - Even when `Cargo.lock` or workspace files change, unchanged third-party crates (hundreds of packages) would hit the compiler cache.
   - If `sccache` is integrated into both host builds and the `server.Dockerfile` container build (via `RUSTC_WRAPPER=sccache`), it could eliminate **7–12 minutes of compilation per Verify job** and **8–10 minutes from Server integration**, cutting total job runtimes by 40%–50%.

---

## Conclusion

- Queue time is heavily concentrated on `macos-26` runners (due to GitHub Actions platform queue limits for macOS).
- Execution time is overwhelmingly dominated by the `Fuzz` job running full 60s rounds across 19 targets on every PR (28–36 minutes).
- Reducing PR `fuzz` time to a fast smoke budget (≤ 5 minutes) directly targets the primary execution critical path, allowing PR feedback to converge towards the 18–25 minute mark (governed by native Verify and Server integration).
