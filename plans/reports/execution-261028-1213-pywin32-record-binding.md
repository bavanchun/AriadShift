# Complete pywin32 payload binding

Outcome: the exact pywin32 exception accepts only unchanged reviewed wheel
payloads. Bind each of the three locked CPython 3.14 Windows wheel RECORD files,
verify every installed file's SHA-256/size and reject missing/extra payloads.
Keep the licence policy, approved source pointers and other packages unchanged.
Inspect intentional installer changes before choosing the smallest binding.
No push; one code commit, full scoped gate at that head, then report-only commit.

- [x] Read the accepted review delta, brief, current source and tests.
- [x] Inspect wheel RECORD files and actual uv installation transformations.
- [x] Prove modified non-adodbapi payloads bypass the old gate: 10 failed, 11 passed.
- [x] Implement whole-payload binding and update its owning policy claims.
- [x] Verify three complete locked wheels and uv installations; 172 focused tests pass.
- [x] Commit code, run the full pack/bench gate through a detached gate slot.
- [x] Update/copy the report, commit reports only, reconcile owned processes.

Constraints: use shared SHA-256/size verification when possible; no generic
licence relaxation, new feature or dependency. Preserve all prior commits.
Unmodified synthetic payloads pass; changed outside source, edited RECORD,
missing or extra files fail. Windows execution remains a CI responsibility.

Original wheel RECORD files each list 611 files. uv moves two scripts and creates two launchers plus INSTALLER/REQUESTED/direct_url.json/uv_cache.json in the inspected direct-wheel installation, producing 617 installed records. Store original RECORD bytes as SHA-256-bound receipts; compare every original row after only the known script relocations, and independently verify every installed payload entry hash/size. Census owned directories for unlisted files, retaining only derived CPython caches for listed sources. Root site-packages and shared script directories also hold other distributions and are not owned wholesale by pywin32.

Code commit: f34a7574bc33d9eb89e5b1ff8c8d33cedb19b7ea. Focused suite (172 tests) and all three complete wheel/uv-install checks pass. Final driver PID 3742447 holds slot 1 and waits for the normal load guard before running the exact-head gate. No process is paused.

Final code-head gate: 172 focused tests, 263 whole pack tests plus four skips, 75 bench tests, three complete wheel and uv installation reviews, 256 native files and 135 generated artifacts all pass. Lock check, frozen audit, UTF-8 audit, Ruff, typos and commit lint pass. Log ends === PACK GATE ALL GREEN HEAD=f34a7574bc33d9eb89e5b1ff8c8d33cedb19b7ea. Followed by a report-only commit, no push.

Status: DONE_WITH_CONCERNS
