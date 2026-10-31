# Docling cloud memory admission

Measured 2026-10-24 on Linux x86_64, using this worktree's `just pack-docling` build (Docling 2.142.0, CPython 3.14, the committed uv lock and model pins). The shared build lock serialized pack creation and measurement. The pack's own real-PDF smoke passed before packaging.

Input: the repository-owned one-page `fixtures/pdf/en-map-room.pdf`, with the pack's actual models and OCR data. Each budget started a fresh `runtime/bin/python3 -I -B` process, sent a real `ariad-engine/1` PDF-to-DoclingDocument request, and required a successful final protocol result. No document content or converted output is committed here.

The measurement sampled the runtime and descendants through `/proc/<pid>/statm` every 25 ms and recorded aggregate resident bytes. A wrapper around the existing engine memory guard recorded `VmData` immediately before it applied `RLIMIT_DATA`; it did not modify conversion or enforcement. The environment contained only a system PATH, with assets in request options. Measurements were outside Landlock, so the engine could read its actual `VmData` (inside confinement it can fall back to peak RSS).

| Budget (MiB) | Final result | Peak aggregate RSS (bytes) | Wall time (s) |
|---:|---|---:|---:|
| 1,024 | `limit_exceeded` | 1,058,271,232 | 8.44 |
| 2,048 | success, one page | 1,248,210,944 | 12.57 |
| 4,096 | success, one page | 1,295,343,616 | 18.36 |

All runs reached model initialization with 1,569 MiB `VmData`. The 1,024 MiB request failed the engine's existing `used + 256 MiB` headroom guard. The 2,048 MiB run establishes a conservative admission floor for this pack; it is not a sufficient budget for every document or a benchmark across platforms. Container capacity must additionally cover the worker, helper, tmpfs and concurrent jobs. RSS samples can miss brief peaks, and `VmData` includes allocations not resident in RSS.

Decision: preserve seeded plan limits, refuse Docling at planning/job admission when a finite plan budget is below 2,048 MiB, and retain the engine/host/container limits during execution. The HTTP regression checks refusal without queueing at the seeded anonymous budget, then successful planning and queue selection at the 2,048 MiB boundary. This local unpacked-pack verification does not imply that signed assets have been published or that a 0.1.0 host accepts a manifest requiring 0.2.x.
