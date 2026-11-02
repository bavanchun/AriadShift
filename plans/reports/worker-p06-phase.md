# Overlay and watermark implementation status

Status: in progress. The phase is not ready for release or final review.

The branch implements the 15 approved in-process overlay, colour, watermark detection and removal, and page-furniture tools. Drawing uses the bundled Libertinus fonts and shared PDF primitives. The original base was `1bdab0f5`; `origin/dev` at `ff8603f6` was merged in `a25d0732`. Slice details and focused checks are recorded in [the drawing report](worker-p06-s1.md), [the marks report](worker-p06-s2.md), [the page furniture report](worker-p06-s3.md), [the colour report](worker-p06-s4.md), and [the watermark report](worker-p06-s5.md).

The registry and tool schemas are generated. PDF plus inline JSON is implemented across host and WASM, with CLI and MCP coverage in progress. The independent Argon review, dispatched by the PM, found bounded-work, mark-removal and resource-cleanup defects; corrections and regression tests are in progress. Its findings and the required delta review remain gate blockers. The Mac PM handles the final Sonnet review.

Focused slice tests passed, including 2,000 outline entries in 38.45 seconds under a 2 GiB scope, Vietnamese text extraction, and PDFium rendering and watermark round-trip checks. A full clippy invocation passed before the security review changes. The final phase gate has not passed and must be rerun against the final head. Manual macOS Preview, Chrome, Acrobat and phone QR checks remain pending.

The approved D1-D10 decisions and existing marker contract govern this work. No new progress events are introduced.
