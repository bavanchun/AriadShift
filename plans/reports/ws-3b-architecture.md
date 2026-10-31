# Architecture reconciliation against integrated hardening

Completed the architecture reconciliation and repaired both source-backed
wording findings from the later independent review. The current rework evidence
appears below; earlier review and gate records remain historical evidence.
Only `ARCHITECTURE.md` and this result file changed. Product code, tests,
lockfiles, release material and other streams' documentation were preserved.
Nothing was pushed, published or merged into the integration branch.

## Revisions and review

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-3b-architecture`.
- Branch: `feat/v1-hardening--architecture`.
- Original dispatched base: `8f6743ac6a0a8837aa2ea140c747bd116506938b`.
- Previous local head: `c1a2b3d4a22101d1adf343bf7c4761e31c808cb6`.
- Accepted integration head: `539ec2f68ae2fee24cf722be51b6611e6de93ff4`.
- Existing merge commit retained: `162d3d051ab3572e34ea928a56ef6c26a5d7b706`.
  Its second parent is the integration head above; ancestry was checked again
  during this run. No redundant merge, rebase or moving-dev merge was needed.
- `a6f4f231b2f9a1d3f35a015bac08c09b3e536a1f`:
  `docs(architecture): reconcile integrated hardening contracts`.
- `8aa0725ab6c0c2f60215cbd253c7fad706bfba3a`:
  `docs(architecture): correct storage boundary link`.
  This also preserves a source-verified clarification separating legacy host
  confinement entry points from stdio MCP's directory-handle asset path.
- Exact independently reviewed and source-gated head:
  `8aa0725ab6c0c2f60215cbd253c7fad706bfba3a`.
- A final report-only commit follows. Its exact delivery SHA is recorded in
  the operations copy of this report and the final gate's `HEAD=` line.

Implementer session: `/root`. Fresh reviewer session:
`/root/architecture_review`, `gpt-6.1-sol`, high reasoning, without inherited
implementation history. The reviewer used its own detached worktree and issued
ACCEPT at that time. The later review superseded that verdict with two
source-backed wording findings, both repaired below. Earlier review artifact:
`/home/vchun/Codes/01-AriadShift-wt-ws-3b-architecture-review-261030/plans/reports/architecture-independent-review-261030.md`.
The initial invalid storage anchor was repaired and independently rechecked.

## Source and claim checklist

The checklist below was recorded before the architecture edits in the resumed
session and reconciled against the merged implementation. This is the docs
counterpart of a failing-first check; no behavioral red test is claimed.

| Stale claim | Executable or accepted evidence | Result |
| --- | --- | --- |
| Only stdio MCP and CLI SDK features are described. | Cargo manifests; `crates/ariad-server/src/mcp/{mod,tools}.rs`. | Remote surface, stateless transport, resource audience, inline cap and shared worker boundary recorded. |
| OS integration is future work and opener is absent. | Desktop `src-tauri/src/{lib,launch_gate,deep_link}.rs`, capability file, installer configs and per-user WiX template. | Classic HKCU verbs, per-user MSI/NSIS, reveal-only permission and the startup fallback recorded. Desktop sign-in remains planned. |
| macOS has no sandbox and doctor is Linux-only. | Host `sandbox/macos{,_profile}.rs`, runner, `commands/doctor.rs` and accepted host-sandbox report. | Missing versus refused Seatbelt, resource limits, dedicated Office IPC, Linux-only blanket `/opt` and host-owned diagnostics recorded. |
| OAuth models and deployment policy are missing. | Auth `src/{auth,config,app,oauth-lock}.ts`; seven runtime OAuth schema keys in the accepted auth report; auth-policy report. | Issuer/audience split, plugin/delete hook preservation, signup default-off, explicit plaintext opt-in and global session lock recorded. |
| API freeze lacks a blob-route boundary. | `crates/ariad-server/tests/openapi_v1_snapshot.rs`, full generated spec and SDK input lock. | Frozen projection and compatibility during refresh distinguished from full-document drift checks. |
| Fuzz counts, oracles and budgets are stale. | `fuzz/Cargo.toml`, harnesses, `justfile`, CI/nightly workflows and accepted fuzz/CI reports. | All 18 current bins, nine ASan bins, host events, real pack extraction, normalized versions, timeouts, RSS and cache policy reconciled. |
| ZIP name compatibility is absent from the threat model. | `crates/ariad-core/tests/package_meta_entry_names.rs`, package stamper and accepted fuzz-fixes report. | Flag-zero valid UTF-8 names become flagged UTF-8; exact byte/flag preservation is explicitly v1.x work. |
| Residual risks imply stronger ownership or confinement. | Server storage/jobs and host assets/confinement/conversion; CLI MCP tools. | Shared transfer fairness, signed PUT replay/expiry, anonymous bearer capabilities, shared-upload deletion and Windows limits recorded. Legacy host and MCP asset paths distinguished. |

The integrated package tests already pin the ZIP policy; no duplicate test was
added in this docs-only task. No later writer fuzz bins were introduced early.
Nine corresponding decisions were added to the existing Decision Log table.

## Verification

All gates ran detached through
`/home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 zsh -lic`, with nice 19 applied
by the helper. Payloads exported `CARGO_BUILD_JOBS=4`, private
`CARGO_TARGET_DIR`, `MISE_UV_VERSION=0.12.23`, `MISE_PNPM_VERSION=12.9.1` and
`MISE_NODE_VERSION` from `.node-version`. No compilation was needed.

The source gate executed these commands on the reviewed source head:

```sh
just spell
lychee --config lychee.toml --no-progress ARCHITECTURE.md
pnpm exec biome format --config-path=/tmp/ws3b-architecture-biome --stdin-file-path=ARCHITECTURE.md < ARCHITECTURE.md > /tmp/ws3b-architecture-biome-output.txt
python /tmp/ws3b-architecture-contract-check.py
sh scripts/check-commits.sh 162d3d051ab3572e34ea928a56ef6c26a5d7b706..HEAD
git diff --check
```

All passed. Lychee reported 33 successful links and zero errors. The one-off
inspection confirmed all manifest bins, nine ASan bins, nine required decision
rows in the existing four-column table, local paths and Markdown heading
fragments, and the ZIP residual/test pointer. There is no dedicated repository
architecture ledger recipe, so source/ledger inspection supplements the
repository's spelling and link commands without adding a CI surface.

Biome 2.6.2 rendered Markdown successfully through a temporary include config.
The repository's root Biome configuration excludes Markdown; applying its
rendered preview would reformat the whole pre-existing document. Existing
compact formatting was retained and the Decision Log's table continuity was
checked. No tracked formatter configuration changed.

Source log:
`/home/vchun/Codes/ariadshift-logs/ws-3b-architecture-source-261030.log`.
Terminal line: `=== ws-3b-architecture source documentation ALL GREEN`.
The preliminary spell/link log is
`/home/vchun/Codes/ariadshift-logs/ws-3b-architecture-preliminary-261030.log`,
ending `=== ws-3b-architecture preliminary docs ALL GREEN`.

The final delivery gate repeats the commands above after the report-only
commit, checks this report's spelling, checks committed whitespace and
confirms that only the two owned files differ from the integration base.
Runner: `/tmp/ws3b-architecture-final-gate.zsh`.
Log: `/home/vchun/Codes/ariadshift-logs/ws-3b-architecture-final-261030.log`.
Its required terminal line is
`=== ws-3b-architecture final documentation ALL GREEN`.
The operations copy records the observed final SHA and terminal line after
completion, avoiding a self-referential SHA in the committed report.

Tool setup used the Links workflow's Lychee 0.24.2, also verified against the
upstream stable release. The downloaded musl archive matched GitHub's SHA-256
`73657a111819a30c47c08352896796f23d64e4eb2b3ed39b6d32149241566fc5`.
An initial install command assumed the wrong archive layout; inspecting the
archive and using its actual nested binary path repaired setup. Login shells
emitted existing monitor/gitstatus diagnostics; gate payloads still completed.
No skill script failed or was modified.

## Integration handoff and limits

Distribution overlap is limited to sections 2.2/2.3, 4, 6.1, 8.2, 9.1/9.2,
10, 11.1/11.3, 14 and the Decision Log. Compatible remote-surface, sandbox and
OS-integration text from `8eb1ef7` was reused only after source verification.
No version/status, release-signing, Homebrew or roadmap rewrite was copied.
Preserve the integrated cloud/delete-user/plugin/licensing decisions.

Evidence here is source review and documentation checks on Linux ASUS. Native
macOS Seatbelt/Finder and Windows installer/multi-file cold-start acceptance
remain with the platform owners. This report does not turn sibling logs into
fresh combined-head runtime proof. The integration head remained unchanged
when checked during this task; later integration changes need their own review.

I16 remains the separately ordered dev follow-up: it needs the whole hardening
merge, accepted core/browser bounds and writer-fuzz facts, and explicit owning
document handoff. No dev follow-up branch or phase-status edit was made here.
The lead owns the full combined gate and acceptance before publication.

Detached documentation runners exited. No database, container, dev server,
browser, listener or build daemon was started. The independent-review worktree
and review artifact remain available; no task-owned background process remains.

No unresolved product decision or source finding remains. Platform execution,
combined integration validation and the separately authorized I16 work retain
the limits described above.

## Current rework result

Repaired exactly the two accepted documentation findings in the rework brief.
The architecture delta from the dispatched head changes three lines: the upload
residual risk, the macOS isolation paragraph and its existing Decision Log row.
No implementation, dependency or date-column changes were made.

- Dispatched head: `d3fcc2f2b8a3dc8f8185330ea27c9172c9384435`.
- Architecture repair commit: `52d935b4d141651a4422663f007949b4c3e16428`,
  `docs(architecture): clarify Office IPC and upload size sweeps`.
- The report-only delivery commit follows that repair. Its exact SHA is the
  final gate log's `HEAD=` value and is recorded in the operations copy after
  that gate completes, avoiding a self-referential SHA in the committed report.

### Verified repairs and scope review

The runner creates `as-ipc-*` beneath `/private/tmp` with mode `0700` and retains
the temporary-directory owner through process completion. Both architecture
occurrences now describe this host-managed directory outside the conversion
workspace, limited socket grants, cleanup on process exit and the deliberate
short-path exception for Darwin UNIX sockets. Cancellation and launch failure
also release the host-owned temporary directory. Verified against:

- `crates/ariad-host/src/runner.rs:320-350`, including the socket-length rationale.
- `crates/ariad-host/src/sandbox.rs:280-302`, which selects the private IPC path.
- `crates/ariad-host/src/sandbox/macos.rs:140-143`, setting `OSL_SOCKET_PATH`.
- `crates/ariad-host/src/sandbox/macos_profile.rs:121-133`, restricting socket grants.
- `libreoffice_ipc_stays_in_a_private_directory_and_is_cleaned_up` in
  `crates/ariad-host/tests/sandbox_macos.rs`, which asserts mode, placement,
  workspace exclusion, cleanup and denial of unrelated socket access.

Initial upload completion reads the object and compares size and SHA-256.
The sweeper only rechecks stored size with `storage.size` and removes retired
or size-mismatched objects; it never re-hashes content. The accepted signed PUT
replay risk remains explicit, including same-size content changes. Verified
against `crates/ariad-server/src/api/uploads.rs:204-223` and
`crates/ariad-server/src/sweeper.rs:136-190`.

Reviewed the complete three-line architecture repair delta against these
sources once more; no additional unsupported claim was found. Source links
were added at the corrected statements. The earlier independent review's
coverage of unchanged reconciliation text remains historical evidence, not a
new runtime test. The lead rejected the date finding: the existing Decision
Log has Topic, Chosen, Rejected and Rationale columns, and git history dates
entries. That convention is preserved.

### Rework verification and delivery

The source gate ran detached at the exact architecture repair commit through:

```sh
/home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 zsh -lic 'exec zsh /tmp/ws3b-architecture-rework-gate.zsh'
```

The runner retains the earlier final documentation checks, nice 19, required
mise pins and `CARGO_BUILD_JOBS=4`. It executes:

```sh
just spell
typos plans/reports/ws-3b-architecture.md
lychee --config lychee.toml --no-progress ARCHITECTURE.md
pnpm exec biome format --config-path=/tmp/ws3b-architecture-biome --stdin-file-path=ARCHITECTURE.md < ARCHITECTURE.md > /tmp/ws3b-architecture-biome-output.txt
python /tmp/ws3b-architecture-contract-check.py
sh scripts/check-commits.sh 162d3d051ab3572e34ea928a56ef6c26a5d7b706..HEAD
git diff --check
git diff --check 539ec2f68ae2fee24cf722be51b6611e6de93ff4..HEAD
```

The runner also confirms that the committed delta over the integration head
contains only the two authorized files, the worktree is clean, and HEAD stays
unchanged. Source gate passed with 36 successful links and zero link errors.
All spelling, formatter parsing, source-inventory, ledger, local-link,
commit-message and whitespace checks passed. Existing Markdown formatting
was retained; no formatter configuration was committed.

Source log:
`/home/vchun/Codes/ariadshift-logs/ws-3b-architecture-rework-source-261030.log`.
Observed last line:
`=== ws-3b-architecture rework documentation ALL GREEN`.

After the report-only commit, the same detached runner repeats every check on
the final delivery HEAD. Final log:
`/home/vchun/Codes/ariadshift-logs/ws-3b-architecture-rework-final-261030.log`.
Required last line:
`=== ws-3b-architecture rework documentation ALL GREEN`.
The operations copy records the observed terminal line and delivery SHA after
completion. Nothing is pushed or merged; lead acceptance remains pending.

No native macOS test was run on Linux and no fresh combined integration gate
is claimed. These are unchanged ownership limits, not blockers for the two
documentation repairs. No database or long-running service was started. The
task-owned detached gate runner must exit before delivery.

No unresolved questions remain for this rework.

Status: DONE
