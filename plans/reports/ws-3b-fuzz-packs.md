# Fuzz oracles and pack extraction confinement

Base: `8f6743ac6a0a8837aa2ea140c747bd116506938b`.
Exact implementation head: `16b9423d0d796c9f877077e809f5d7c10efffb63`.
Previous delivery: `e40aaf83facd570ac536139b9566d24836693f95`.
Branch: `feat/v1-hardening--fuzz-packs`. No push or dev merge occurred.

## Review rework: plain-file acceptance

The fresh lead review of `e40aaf8` found a valid medium issue. Conditional
scanner checks and writer agreement could silently accept a shared rejection
of raw bytes. Commit `16b9423` restores unconditional scan success, one entry,
exact byte count, bounded extraction success and matching writer statistics
for every admitted plain-file input. All admitted inputs are at most 16 KiB,
within the 1 MiB / 1024-entry extraction budgets. Archive formats retain
typed rejection. No production extraction code changed.

A documented negative control proves the previously missing failure path.
The detached worktree `/tmp/ws3b-pack-archive-oracle-mutation` starts at
`e40aaf83facd570ac536139b9566d24836693f95`. Only there, this mutation was
inserted at the start of the shared `walk` function's `Unpack::File` arm:

```rust
if component == "payload.bin" {
    eprintln!("NEGATIVE CONTROL: forced plain-file walk rejection");
    return Err(unsafe_entry(
        component,
        component,
        "negative control: forced plain-file rejection",
    ));
}
```

The input is the original synthetic byte string
`b"plain file bytes\x00\xff\n"` (19 bytes), SHA-256
`c7fb1f40062c234e114085c5b3bce7eb35bff2f62cc9496e116be8f8869ed623`,
written to `/tmp/ws3b-pack-archive-negative-control.bin`.

- In that worktree, `cargo fuzz run -s none pack_archive
  /tmp/ws3b-pack-archive-negative-control.bin` with the old harness falsely
  exits successfully. Two mutation markers prove both writer and scanner
  reached the forced rejection. Raw log:
  `/home/vchun/Codes/ariadshift-logs/ws3b-pack-archive-mutant-old.log`.
- Replacing only that worktree's harness with the corrected source and
  repeating the identical command panics at `pack_archive.rs:142`,
  `a plain file always scans`, with the forced-rejection error. Cargo's
  wrapper exits 1 and libFuzzer reports exit 77. The gate requires this
  actual assertion and reason; compile failure cannot count as proof.
  Raw expected-failure log:
  `/home/vchun/Codes/ariadshift-logs/ws3b-pack-archive-mutant-fixed.log`.
- The actual candidate uses the unmodified production extractor. It passes
  the same 19-byte input, all 14 archive seeds, and the timed fuzz run.
  No mutation was committed or applied to the candidate's production code.

Gate runner `/tmp/ws3b-pack-archive-rework.zsh`, detached slot 1, jobs 2,
nextest threads 2, nice 19 and private cache. The log
`/home/vchun/Codes/ariadshift-logs/ws3b-pack-archive-rework.log` ends:
`=== ws3b-pack-archive rework ALL GREEN`.
Positive commands after both controls were:

```sh
cargo fuzz run -s none pack_archive /tmp/ws3b-pack-archive-negative-control.bin
cargo fuzz run -s none pack_archive fuzz/seeds/pack_archive/*
just fuzz pack_archive 60
cargo fmt --manifest-path fuzz/Cargo.toml --check
git diff --check
```

The timed run completed **111,086 runs in 61 seconds**, without panic.
The gate's fixed-source snapshot was compared byte-for-byte with the
committed harness (SHA-256
`e2db9633e52c8c630fc1ba50dba50250d21575e26f142cecc264357eb5ee3f86`).
Commit lint passes from the actual base through `16b9423`.
The tracked gate exited; no worker-owned background process remains.
Seed inventory is unchanged at 102 seeds across 18 targets.

## Delivered changes

- The six original-brief harnesses check declared IR versions and normalized
  content, resolver-added assets while preserving reader assets, real pack
  extraction, genuine manifest signatures, deterministic package stamping,
  and independent URI properties.
- The hidden extraction entry point uses the actual install writer, budgets
  and final link check. It bypasses signature/component-hash verification
  only; its documented boundary requires a caller-owned isolated root.
  Regression tests cover valid formats, byte/entry limits, partial output,
  traversal, symlink prefixes/chains, dangling links and hard-link copies.
- The separate `[[fuzz_seed]]` collection has 102 seeds across 18 targets:
  99 Apache-2.0 and 3 MIT. Each has an exact SHA-256 and verified source,
  license, license basis and attribution. The checker rejects incomplete
  inventory, altered hashes, forbidden licensing, missing provenance,
  duplicate IDs/paths, unsafe paths and symlinks. Its negative regression
  proves the combined checker validates the new collection. Existing
  `[[fixture]]` safety checks, document routes and counts are preserved.
- The once-signed fixture reaches successful verification and checks both
  tampering and trusted-comment identity binding. Only manifest, signature
  and public key are retained. SHA-256:
  `3e81490bb3e1e99d0879a2dd91e3a3890e5dbbcf08a592dd7bcbc653da3d9404`.
  No private key was serialized. No dependency was added; Cargo regenerated
  the existing host `same-file` edge in `fuzz/Cargo.lock`. `fuzz/Cargo.toml`
  required no change under the approved no-new-dependency option.

Planner correction: “missing versions pass” in the brief conflicts with the
named specification, `is_supported_version(None)`, existing host tests and
ARCHITECTURE.md section 6.2. Missing, null, future and non-string versions
remain rejected; supported legacy/current input normalizes. No production
IR reader changed. Reader regressions already passed on the base and are
coverage additions, not a claimed product regression.

New archive seeds are original project data generated with Python standard
`tarfile` USTAR, deterministic gzip and `zipfile` stored archives from the
cases in `pack_extract_confinement.rs`; archive timestamps are fixed.
Existing document copies retain their owning fixture licenses; generated
IR/AST data records identify its source documents and introduction commits.
No external/private/user document was imported.

## Commits and review

1. `503773240756c046fba9f50b5122f5c4f36ff438` —
   `test(fuzz): enforce parser and extraction invariants`.
2. `4521cb6ecb72f59eba59132ce4b03fdad8a3fe24` —
   `chore(fuzz): format the pdf reader harness`.
   The lead explicitly extended ownership for this formatting-only hunk;
   behavior is unchanged.
3. `e40aaf83facd570ac536139b9566d24836693f95` — prior delivery report.
4. `16b9423d0d796c9f877077e809f5d7c10efffb63` —
   `test(fuzz): require plain-file scan and extraction success`.

The initial independent review missed the lost failure path. The fresh lead
review found it; the rework above addresses it with a paired mutation check.
An independent narrow re-review in
`/home/vchun/Codes/01-AriadShift-wt-ws-3b-fuzz-packs-review` covers the corrected
source and completed rework logs. A separate simplification review suggested
reusing the preserved AssetStore for ID membership; applied. Final review
record: `plans/reports/ws-3b-fuzz-packs-review.md`.

## Verification

All build/test work ran detached through gate-slot, with jobs 2, nextest
threads 2, nice 19, a private cache and the load <12 start guard. No owned
server, container or listener was started. No DB tests apply to this scope.

The initial source gate log (before the rework above) is
`/home/vchun/Codes/ariadshift-logs/ws3b-fuzz-packs-green.log` and ends:
`=== ws3b-fuzz-packs verification ALL GREEN`.

| Checks | Outcome |
|---|---|
| Host nextest: normalization, IR I/O, extraction confinement, packs | 49 passed, 0 skipped |
| CLI fixture/seed manifest nextest | 3 passed, 0 skipped |
| CLI test-target clippy; host all-target clippy with test-probe | Passed |
| Workspace and fuzz fmt checks | Passed with the now-committed PDF layout |
| Explicit IR, GIF, signed-manifest, archive and URI seed replays | Passed |
| Six original-brief targets, `just fuzz <target> 60` | All completed without panic |
| `SOURCE_DATE_EPOCH=garbage just fuzz docx_meta 10` | 8,562 runs completed; environment did not disable stamping |
| `just pandoc`; `just spell` | Passed |

Initial timed fuzz executions (runs / seconds): IR 2,104,342 / 61; pack archive
47,080 / 67; assets 17,842 / 79; manifest 326,894 / 73; package metadata
16,366 / 61; URI 2,837 / 80. That gate used the initial implementation
source at `4521cb6`; the rework gate above supersedes the pack-archive run.
Exact command lines are preserved under `COMMAND:` in the log.

Initial closing exact-head log:
`/home/vchun/Codes/ariadshift-logs/ws3b-fuzz-packs-final.log`.
It records exact head `4521cb6ecb72f59eba59132ce4b03fdad8a3fe24` and ends:
`=== ws3b-fuzz-packs exact head verification ALL GREEN`.
`just lint-commits 8f6743ac6a0a8837aa2ea140c747bd116506938b..HEAD`, both fmt
checks, `just pandoc`, the same 49 host and 3 manifest tests, and `just spell`
all passed; 0 tests skipped. The tracked gates have exited; no owned
background process remains.

The report-only delivery commit is recorded by full SHA in
`/home/vchun/Codes/ariadshift-logs/ws3b-fuzz-packs-rework-delivery.log`.
That final metadata check covers commit messages, report spelling, clean
diff and identical source/fixture/fuzz content to the implementation head.

The base proof produced genuine assertion panics for all four legacy IR
seeds and the GIF data-URI seed; 36 base host tests passed, 0 skipped.
Individual expected-failure logs use prefix
`/home/vchun/Codes/ariadshift-logs/ws3b-fuzz-packs-base-`.
The combined `ws3b-fuzz-packs-red.log` preserves that proof but ultimately
ends `STOPPED: ws3b-fuzz-packs red runner rc=127`: editing the running wrapper
caused it to read a changed tail after the successful child gate. That
orchestration failure is not reported as a passing combined gate.

The earlier `ws3b-fuzz-packs-ci-replay.log` stopped at commit lint before any
saved-input replay. The suggested `style` type is disallowed by this repo;
the unpublished formatting commit was amended to `chore`, with identical
source. The initial closing gate passed after that amendment; delivery metadata
checks commit lint including the rework and report commits.

## Lead-directed remaining work

At 05:11 +07 the lead transferred the separate saved-CI-input investigation
for `docx_meta` / `ir_json_versioned` to another worker. No saved-input or
all-18 follow-up executed here. Already committed
original-brief oracle repairs and their passing evidence are retained.
This reassignment is not an acceptance blocker for this deliverable.

Native macOS/Windows execution is not claimed. Linux exercised the pure
Windows path-string properties. The following integration work remains:

HOST-SANDBOX PLACEHOLDER (lead-directed integration follow-up): do not add an
active record on this candidate, because the seed exists only on the sibling
branch. After host-sandbox merges, the manifest owner must add:

```toml
[[fuzz_seed]]
id = "fuzz-engine-protocol-host-event-rules-jsonl"
target = "engine_protocol"
path = "fuzz/seeds/engine_protocol/host-event-rules.jsonl"
sha256 = "6844e50d634bcf3185b80ce0a4c33aff952f99c75553d140172e2dcb7fa50ad6"
source = "Original protocol event examples authored for AriadShift; no external or private document."
license = "Apache-2.0"
license_basis = "Original project test data distributed under the repository license."
attribution = "AriadShift contributors"
```

Origin is generated. Authoritative handoff:
`/home/vchun/Codes/ariadshift-ops/reports/ws-3b-host-sandbox-handoff.json`,
`fuzz_packs` block. The active checker continues to require a complete inventory
of the files actually present in this branch. The lead confirmed this entry
must wait until the sibling seed lands; it is not a missing candidate fixture.

I15 writer fuzz targets wait for all 3b merges and the lead-accepted dev SHA.
They were not started, and moving dev was not merged. The current candidate
has 18 targets and 102 inventoried seeds; integrating the approved protocol
seed adds one seed and requires the record above before the next manifest gate.

Status: DONE_WITH_CONCERNS
