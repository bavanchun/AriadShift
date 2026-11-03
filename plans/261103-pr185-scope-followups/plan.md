# PDFium preflight follow-ups

Status: complete; implementation and verification are recorded in the worker report.

Outcome: selected-page execution budgets, correct soft-mask resources, bounded Type3 accounting, end-to-end safety regressions, and documented limits.

Constraints: preserve document-wide canonical classification and existing safety limits; no follow-up issues; use the shared build lock and 2 GiB test scopes. No tests may be weakened.

1. Pass selected page ranges into preflight while canonicalizing every stream; test a 1,500-page PDF.
2. Correct soft-mask page-resource resolution and test it.
3. Share scopes and memoize glyph costs without bypassing cycle or depth checks; measure many-font performance.
4. Test unclassified appearances, nesting boundaries, and inline filter agreement through PDFium.
5. Update tools documentation and the worker report; run required checks and external corpora; commit, lint, push, and update the requested result artifact.

Acceptance: all requested cases pass, the existing real-document corpus gains no refusals, and the pushed head contains focused commits.
