# Image preflight work in progress

The requested work is incomplete. Conflicting inline image key pairs are now
refused, and ASCII85 and Flate inline output are bounded by the declared image
size plus 1024 bytes, subject to the existing global cap. The generated fixtures
`inline-conflicting-keys.pdf` and `inline-flate-oversized.pdf` exercise these
cases in `refuses_ambiguous_inline_keys_and_excess_flate_output`. The regression
failed against the original implementation and passed after the changes.

B1, B2, B4, Type3 CharProc preflight in B5, and N2 remain unimplemented. The
inline filter parser already rejects LZW and RunLength filters. Integration,
the full gate, and the handoff rewrite have not been performed. The 1024-byte
slack is an assumption and needs review for predictor overhead.

Formatting and PDFium-engine Clippy passed. The PDFium-engine test suite passed
under a 2 GiB systemd scope after touching the engine source to force a rebuild.
The initial full test run failed because the shared target directory had reused
another worktree's engine artifact; the first run even reported the other
worktree's detect-scanned tests. The focused regression passed both before and
after that full run's stale-artifact failure.
