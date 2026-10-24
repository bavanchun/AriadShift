---
title: Docling native licensing blocker
date: 2026-10-24
summary: OpenCV built without IPP; static MKL in PyTorch requires a coordinator decision.
---

# Docling native licensing blocker

## What happened

Verified OpenCV and LLVM source pins and implemented a provisional native replacement pipeline and vendor-content gate. OpenCV compiled in a pinned manylinux container; cv2 reported IPP disabled and resized an image successfully. Python and workflow checks passed.

## Blocking evidence

The locked Linux CPU PyTorch wheel reports USE_MKL=ON and defines MKL functions inside libtorch_cpu.so without an MKL shared-library dependency. Intel identifies MKL's proprietary ISSL licence. Replacing OpenCV and OpenMP cannot remove those static bytes. The vendor gate rejects the actual MKL binary marker.

## Next steps

The coordinator must choose a PyTorch build without MKL or another verified replacement, retaining the no-exception policy. The workspace is uncommitted, with Windows builds, final pack proofs and remote CI still unverified. The worker handoff is the requested external docling-no-intel-ipp-report.md. No processes or containers remain from this task.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
