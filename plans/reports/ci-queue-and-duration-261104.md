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

## Conclusion

- Queue time is heavily concentrated on `macos-26` runners (due to GitHub Actions platform queue limits for macOS).
- Execution time is overwhelmingly dominated by the `Fuzz` job running full 60s rounds across 19 targets on every PR (28–36 minutes).
- Reducing PR `fuzz` time to a fast smoke budget (≤ 5 minutes) directly targets the primary execution critical path, allowing PR feedback to converge towards the 18–25 minute mark (governed by native Verify and Server integration).
