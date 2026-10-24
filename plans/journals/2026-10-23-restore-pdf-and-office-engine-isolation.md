---
title: Restore PDF and Office engine isolation
date: 2026-10-23
summary: "Recover the rebased engines and preserve sandbox, memory, session and artifact contracts."
---

# Restore PDF and Office engine isolation

The worktree was clean before recovery. The transferred bundle restored caba2b8 on merge base 7e2f389, and the interrupted ten-file patch was reviewed before application.

PDF, Typst and LibreOffice now declare one-shot session behavior. Route artifacts are read through captured directory handles and writer outputs are snapshotted outside engine access before stamping or subsequent route steps.

Real LibreOffice conversion failed under Landlock because its IPC binds a hard-coded socket beneath /tmp and ignores TMPDIR. A syscall trace showed EACCES at bind; the same generated document converted without filesystem isolation. The launcher now gives LibreOffice a workspace-backed private /tmp through user and mount namespaces, preserves the workspace paths, and maps the caller to a non-root namespace identity. Setup failures stop launch. Existing nix features supply the safe APIs without adding a dependency version.

The focused integration suite passed 24 tests, including actual Office conversions, Vietnamese PDF round trips, batch extension handling, host memory termination, session fallback, cancellation and timeout, and private-temp isolation. After rebasing onto the web foundation at 3a2d5ea, the CI-equivalent checks passed workspace Clippy, WASM checks, 756 native tests (one release-only timing skip), documentation tests, 64 benchmark tests, static checks, all 49 JavaScript tests, TypeScript and production web builds. A fresh target directory verified all packaged crates after Cargo reused an older same-version registry build in the normal target directory. PowerShell mutation guards remain for CI because pwsh is absent locally. GitHub delivery continues in this session; the coordinator owns final independent review and merge.

The first GitHub run passed macOS, Windows, all five release targets, CodeQL, web and regular fuzzing. Ubuntu correctly stopped at namespace setup because its runner policy denied the operation. The CI configuration now enables this prerequisite only in its disposable VM and checks it explicitly. The native job also follows the web job's primary HTTPS Ubuntu mirror after the Azure download consumed almost twenty minutes. Application confinement and fail-closed behavior stay intact.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
