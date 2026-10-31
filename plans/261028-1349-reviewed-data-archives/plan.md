---
title: Review data archives in locked pack wheels
status: completed
priority: P1
effort: medium
branch: fix/pack-prebuild-gates
created: 2026-10-28
---

# Review data archives in locked pack wheels

Outcome: the pack accepts reviewed inert model/data archives and continues to
refuse unknown or runnable archive payloads. The exact distribution, version,
relative path and SHA-256 bind every new exception. No suffix-wide torch
exception, dependency update, source build or push is in scope.

Acceptance: enumerate ZIP-local-header payloads across the locked wheels for
Linux x86_64/aarch64, macOS arm64 and Windows; report their owners, sizes and
hashes; reproduce the aarch64 failure before editing; prove reviewed/hash/
owner refusal cases; inspect other pack CI logs; pass the full scoped pack gate
at an exact code head and commit the final report alone.

- [x] Inspect CI logs and enumerate all compatible locked wheel ZIP payloads.
- [x] Review archive contents and demonstrate the real pre-fix failure.
- [x] Add regression tests, exact reviewed entries and the minimal gate repair.
- [x] Review the diff and pass focused checks, then commit the code.
- [x] Run the full exact-head pack gate and record the report-only successor.

## Completion evidence

Code head ea43f406da7ae5aa2602b47a49ebe9372cc11232 passes the full scoped gate.
Last line: === PACK GATE ALL GREEN HEAD=ea43f406da7ae5aa2602b47a49ebe9372cc11232
Log: /home/vchun/Codes/ariadshift-logs/d2120-t7-rework5-final-gate.log

182 focused tests, 273 engine tests (4 skips), 75 bench tests, 135 reproducible
fixtures, 216 locked wheel hashes, 148 ZIP payloads, complete real ARM and
pywin32 wheels, Linux native inventory, encoding/lock/audit/Ruff/typos and commit
lint all pass. Source, TOML and owning docs were reviewed with no open finding
in the ZIP repair. Final CI snapshot at 07:31:36 UTC keeps x86_64 running and the
two diagnosed ARM/macOS pack failures on the old ce0d878 head. The independent
Mac libIex review remains outside this repair and is listed in the report.
All tracked source is committed; the remaining successor contains documentation
only. The journal is local; AgentWiki publish skipped. No push.

Status: DONE_WITH_CONCERNS
