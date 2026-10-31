# CI database, OAuth hygiene and fuzz discovery

Base: `51b8f52a71829f54ea41ff6b045610c0e34608da`.

Deliver the existing PostgreSQL OAuth suite in CI with mandatory execution,
private OAuth E2E credentials and owned database reset, an independent
core OAuth job following the current immutable-source policy, all default fuzz
targets, and schedule-only nightly cache publication. Preserve Compose pack
checks, required status semantics, engine boundaries, and dependency versions.
No dev merge, sandbox changes, push, publication, or unrelated runtime changes.

1. Add hygiene and executable CI wiring regressions; record base failures.
2. Repair OAuth script, catalog, recipes, workflows and owning CI documentation.
3. Run focused checks, real disposable PostgreSQL, real core Compose OAuth and
   all default fuzz targets through detached gate slot 2. Accepted auth/fuzz
   joint proof and ordinary Docling Compose are later lead-scheduled steps;
   finish this exact-base candidate without merging sibling or moving dev heads.
4. Obtain independent review in a separate worktree, repair findings, commit
   explicit paths, lint the actual commit range and copy the result report.

Acceptance requires real DB execution, no missing-URL silent success, matching
pg_dump 18 support and owned teardown, all declared fuzz bins at 60 seconds,
no credential argv leaks, both E2E signup opt-ins and no weakened CI policies.
If an external dependency prevents proof, record it as a blocker honestly.

Completed own-base candidate with concerns. Independent review accepts source
and catalog lock. Exact-base regressions fail behaviorally; focused checks
pass. The first DB hook failure and Docker removal preflight timeout remain
recorded. The genuine retry began at load 9.72 after host SQL readiness:
12/12 DB tests and 118/118 auth tests passed, with zero skips. The base's
missing-URL silent skip and the candidate's refusal are both demonstrated.

All 18 fuzz bins were attempted; 16 completed at least 60 seconds, while
unchanged-base docx_meta and ir_json_versioned assertions panic. Fuzz-owned
source/lock was not changed. Full-duration green fuzz remains part of the
lead's later accepted-head joint proof. Real core OAuth passes through
conversion and registration cap; its containers, volumes and networks are
removed. The DB container and temporary base worktree are removed; assigned
ports are clear and no owned gate/dev server remains.

The memory-pressure rule kept the DB gate's existing queue position and used
CARGO_BUILD_JOBS=2/NEXTEST_TEST_THREADS=2 with two Vitest workers. Earlier gates
used the then-authorized four-job limit. Implementation commits and actual-base
lint pass; the documentation-only delivery records final review, evidence,
remaining integration commands and retained fuzz concerns in the result report.
