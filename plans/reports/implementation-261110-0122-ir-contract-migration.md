# IR contract migration execution

Scope: execute only the approved IR design and T1 in the existing Phase 11 plan. Preserve all frozen v0/v1 schema and golden bytes. Reader enrichment, styled output and fidelity measurement remain outside this change.

Preflight: branch `p11/t1`, clean tracked and untracked tree, HEAD and immutable base both `4b51bdf9b786be2d06553eee49f4d72891dbbfab`. The PM directive in `.run/coord/from-mac/pm-023-codex-fill.md` explicitly permits work before Phase 10 finishes; no release completion is inferred. Proposals 1–5 are approved by the maintainer and dispatch brief. Commit and push are authorized; merge is not.

Execution order: freeze legacy wire types; implement styles, IDs and version boundaries; migrate all callers and visitors; add adversarial and compatibility tests, schema support and docs; only then acquire the shared build lock for focused and workspace gates. Use `CARGO_TARGET_DIR=/home/vchun/.cache/ariadshift-target-p11-t1` and a 600-second timeout including lock wait for every heavy command.

Acceptance: the named tests and full gate list in the dispatch brief pass; legacy bytes stay unchanged; v2 schema and snapshots are separate; default JSON is v2 and explicit v1 warns about loss; layout/source maps resolve stable identities; nested styled content retains text, assets and bounds. Independent approval belongs to the coordinator before Wave 2 starts.

Evidence and ownership handoff will be recorded in the absolute primary-workspace result mailbox requested by the maintainer. No background process has been started during inspection.

## Initial verification outcome

Status: BLOCKED. The final regression check waited 600.01 seconds for the shared lock and exited 124 before Cargo started. The dispatch stop rule ended dependent verification. No commit, push or ownership handoff occurred.

Historical checks passed the v2 schema generation test, the then-current eleven IR tests and all three reader snapshot suites. Broader validation exposed a stack overflow before semantic bounds ran. The JSON-level bounds repair and later native-byte-cap/font/geometry edits require fresh verification. Final source has twelve IR tests.

Full command evidence, frozen-byte proof, exact changed paths and the resume gate list are in `/home/vchun/Codes/01-AriadShift/.run/coord/to-pm/p11-t1-result.md`. All 158 legacy snapshots and the v0/v1 schema bytes match the immutable base; forty v2 snapshots are separate. No owned background process remains.

The requested PM notification was sent with `hc send t80 "codex-p11-t1 finished: see .run/coord/to-pm/p11-t1-result.md"`; exit 0 returned `{"started":true,"status":"working","turn":21}`. AgentWiki publish skipped.

## Resumed verification

The coordinator explicitly resumed verification, repairs, commit and push after the first lock timeout. The deep-IR regression now passes, as do all twelve style/identity tests. The combined core gate passed snapshots, schema drift and IR validation, then exposed old writer assertions comparing unassigned construction IDs with imported IDs and blanket newline assertions inconsistent with opaque HTML payload preservation. Expected imported identities are now finalized, and Markdown emission normalizes separators before container prefixing to preserve its existing injection safeguards. Verification continues without blessing either schema or snapshots.

## Verified outcome

The implementation and requested code/runtime gates now pass. The final focused core gate covers twelve style/identity tests, all three snapshot suites, thirteen IR validation tests, the frozen/current schema drift checks and thirty-six writer round-trips. Additional verification passed 404 core unit tests, eighteen Docling tests, sixty-three HTML tests, eleven HTML bounds tests, nineteen Pandoc mapper tests, eighteen PDF reader tests, fourteen Typst writer tests, twenty-one writer bounds tests and nine HTML stripping tests. Existing ignored schema regeneration and release-only PDF timing helpers are reported explicitly and are not counted as passes.

The final host suites passed thirty-five tests, with eight internal IR reader tests, thirteen existing media tests and the separate Styled media test also passing. CLI passed thirty-nine tests against real Pandoc 3.12, canonical Python passed all fourteen tests without prerequisite skips, and rebuilt WASM passed eleven JavaScript tests. Workspace/all-targets, minimal core, WASM target, locked fuzz, actual WASM build, TypeScript, Rust formatting and strict clippy checks passed. Final spelling and Git delivery receipts belong to the absolute result mailbox.

Repairs retained the original bounds and frozen expected bytes. Import identity generation now avoids unnecessary positional maps and streams canonical table/plain-paragraph payloads; reference-payload tests and unchanged v2 snapshots prove that IDs did not change. The existing 6 MB HTML table test passes its original 30-second limit. A 500-level Typst regression exposed recursive subtree cloning; the shared normalizer now traverses block containers with a heap work stack while preserving IDs, styles and direct properties. Legacy semantic reader assertions use the frozen representation; separate identity tests retain the v2 invariants.

The excluded fuzz lockfile was stale against the existing PDF manifest at the immutable base. Cargo synchronized its existing similar dependency using the exact similar 3.2.1 and bstr 1.13.1 versions/checksums already in the root lockfile. No manifest, dependency choice, root lockfile, engine protocol, registry, fixture manifest, v0/v1 schema or original golden bytes changed.

The full command history, failures, successful repairs, schema/corpus hashes, explicit changed-path list and final commit/remote evidence are recorded in the requested absolute result mailbox. Worker verification does not replace the coordinator's independent approval before Wave 2 ownership is released. No merge or release is authorized by this record.
