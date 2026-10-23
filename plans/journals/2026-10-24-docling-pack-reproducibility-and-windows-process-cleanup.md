---
title: Docling pack reproducibility and Windows process cleanup
date: 2026-10-24
summary: "Reproducible Docling packs, sandboxed sessions, signed installation tests, and a Windows Job Object cleanup fix."
---

# Docling pack reproducibility and Windows process cleanup

Two clean Linux pack builds produced byte-identical runtime, models and tessdata archives and manifests. Stable bytecode requires a fixed hash seed, one ordered compilation process and relative filenames; installer location/time records must also be removed from distribution RECORD files.

The branch was integrated with installed engine packs, Linux sandbox policy, host process-tree memory limits, and warm sessions. A real signed test pack installs through the CLI, converts a PDF, reuses two Office jobs in a sandboxed session and rejects a 64 MiB limit. The source host is still 0.1.0 before release preparation, so only the temporary signed test manifest widens compatibility; distributed packs require the 0.2 host.

The Windows conversion stall was host cleanup rather than model inference. Instrumentation showed Docling emitting its artifact and successful result in 12.297 seconds, then the host waiting again after try_wait had consumed the Windows Job Object's final completion notification. Cleanup and stale-session checks now retain the observed exit status. A Windows regression drained the completion notifications and completed cleanup in 0.315 seconds; the real Windows pack then passed conversion and signed installation/session checks. Temporary tracing was removed and conversion timeouts were not increased.

Verification included 51 Python tests, 665 Linux Rust tests, 628 Windows Rust tests, 64 benchmark tests, 49 web tests, packaging with a fresh target directory, offline Vietnamese OCR conversion, and 229,695 mapper fuzz runs in 31 seconds. The stale browser test that treated PDF as unsupported was updated to assert the CLI hand-off. Heavy commands ran sequentially under the shared ASUS build lock after the coordinator's resource-management interruption.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
