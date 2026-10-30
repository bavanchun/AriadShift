# Desktop file opening, startup and per-user installers

## Candidate and delivery

- Branch: feat/v1-hardening--desktop.
- Accepted base: 8f6743ac6a0a8837aa2ea140c747bd116506938b.
- Exact product/documentation head: 003ebdac3ea7c32442fc26a725f74e880c0b56f2.
- Worktree: /home/vchun/Codes/01-AriadShift-wt-ws-3b-desktop.
- Implementer session: /root. Independent reviewer session:
  /root/desktop_independent_review (gpt-6.1-sol, high).
- No moving dev merge, rebase, push, PR, branch deletion or release occurred.
  Delivery adds a report-only commit after the source head above.

The completed green gate ran the candidate's working-tree bytes immediately
before local commits. Its final section records SHA-256 for all 13 changed
product/documentation files. Every committed file was checked against those
digests after committing; all matched. This is source-equivalent verification,
not a claim that the gate originally printed the later commit SHA.
The final delivery HEAD, full commit list and post-commit message/source checks
are recorded in /home/vchun/Codes/ariadshift-logs/ws-3b-desktop-delivery.log.

| Commit | Change |
| --- | --- |
| 469a37c114c1cc0d517e76b21160f8b97c27b97f | Ignore Finder file URLs in preference links; retain file-open delivery and link validation. |
| 031c162fbdef8c6255988c98a5074f9e417bfb67 | Serialize Windows single-instance startup with a bounded standard-library file lock. |
| ec3b35ba33f5d91c75514d99898e4d5acfb79090 | Enforce per-user MSI authoring, generated resources, packaging checks and actual tool drift detection. |
| ea628c9fdaddd9c7ecdc45ad13d89d176c39d7c1 | Advance the notice-expiry test clock asynchronously. |
| 003ebdac3ea7c32442fc26a725f74e880c0b56f2 | Document installer, startup, packaging and platform-check contracts. |

## Implemented behavior

Finder file URLs return Ok(None) from the preference-link parser. New integration
tests exercise percent-encoded and localhost URLs through both that parser and
the real file-open function. Invalid URLs, HTTPS/unknown schemes, invalid routes
and forbidden parameters remain errors; real routing and reserved authentication
links retain their behavior. The vendored plugin is unchanged.

Windows startup acquires a per-user temporary-file lock before plugin setup and
holds it until the application's setup callback, after plugin setup. Its guard
releases on drop/process exit. The file stays present so waiters use the same
file identity. Standard-library tests cover exclusion, release, a waiting launch,
a bounded deadline and an unusable lock path. Engine and sandbox entry points
return before run() and never acquire the gate. The historical Windows race was
not reproduced or measured here.

The MSI uses a complete custom template adapted from official Tauri CLI 2.12.4:
perUser scope, limited privileges, LocalAppData installation, HKCU protocol keys
and user-context WebView2 update behavior. NSIS remains currentUser, with its
classic verbs and existing hooks. Upgrade/uninstall sequencing, extension
references, feature/merge loops and binary delivery are retained. The template
attribution records the pinned upstream source and license. The matching Windows
native CLI package was inspected because the Linux CLI embeds no WiX template.

Profile file components require HKCU registry keypaths for WiX validation. Main
and additional binaries use registry-derived component identities for their
changed keypaths. Generated resource components derive from the actual explicit
source-to-filename mapping, including all four current license/notice resources;
the resource GUID derives from that mapping. The unchanged Explorer verb
component retains its GUID. Only the owned WiX output changed on regeneration;
NSIS, macOS and Linux outputs have no unrelated drift.

The package's Tauri command forwards through the existing signing wrapper. The
post-compilation, pre-bundle check binds to native versus explicit MSVC output
without relocating release artifacts. It checks effective root, Windows and CLI
configuration using Tauri's merge order, generated parity and executable presence,
and refuses unaccounted output DLLs. Unsupported resource layouts, implicit VC
runtime resources, elevated update tasks, custom output/profile options and
inherited ambiguous selectors fail with documented instructions. CARGO_TARGET_DIR
remains supported. Cargo configuration requires an explicit MSVC target. This
avoids silently omitting resources from the per-user MSI.

The actual test:tools command now includes OS-integration tests. Its scratch test
preserves repository-relative resource layout, changes disposable formats only,
invokes the real package command and requires the named generated-drift failure.
It excludes itself from nested execution, disables dependency installation only
in that disposable copy and cleans up the copy.

The notice-expiry failure was test timer/effect synchronization. The original
selected test failed. Instrumentation showed zero timers at fake time
1793220524889; synchronous advancement moved the clock 6,100 ms before React
registered the notice timer at 1793220530989. That timer remained pending instead
of firing. The same diagnostic using advanceTimersByTimeAsync passed. The
committed fix uses asynchronous advancement, preserving the original expiry
duration and assertion. Focused and full frontend runs passed. No frontend
product behavior changed.

## Red evidence

Log: /home/vchun/Codes/ariadshift-logs/ws-3b-desktop-red.log.

The runner temporarily restored the exact accepted-base parser, package script,
Windows config and original polish test while retaining new regressions and
candidate helper code. It restored all candidate files before green. This was a
targeted base overlay, not a claim that the whole workspace was base source.

| Exact command | Outcome |
| --- | --- |
| node --test apps/desktop/os-integration/msi-scope.test.mjs | Exit 1: old config lacks the per-user template; two primary scope/wiring assertions fail. Two further checks encounter missing old wiring; existing NSIS case passes. Matching official Windows CLI template inspection establishes the per-machine default. No Windows installer execution is claimed. |
| node --test apps/desktop/os-integration/tool-drift.test.mjs | Exit 1 because the old actual command returns 0, running six script tests despite scratch drift. Repeated after removing irrelevant scratch dependency-validation attempts; same behavioral result. |
| pnpm --filter @ariadshift/desktop test --maxWorkers=1 test/polish.test.tsx -t 'dismisses the latest message with Escape and expires info notices' | Original test fails; instrumented synchronous run fails; asynchronous diagnostic passes. |
| just pandoc | Exit 0; required Pandoc 3.12 installed before Rust conversion-related tests. |
| cargo nextest run -p ariad-desktop --test finder_file_links | Exit 100: file URL case fails with Invalid desktop link; existing real-link case passes. Behavioral failure, not a compile-only reproduction. |

Terminal line:

    STOPPED: desktop red baseline (expected failures retained) rc=1

## Green verification

Log: /home/vchun/Codes/ariadshift-logs/ws-3b-desktop-green.log.

Both payloads ran consecutively in the same detached gate-slot.sh 1 allocation,
with nice 19, load/memory entry guards, CARGO_BUILD_JOBS=2,
NEXTEST_TEST_THREADS=2, dev/test debug information disabled and incremental
compilation disabled. Pins: Node from .node-version, pnpm 12.9.1, uv 0.12.23.
Private cache: /home/vchun/Codes/ariadshift-builds/ws-3b-desktop.

Payloads: /tmp/ws-3b-desktop-red.sh and /tmp/ws-3b-desktop-green.sh. Launch:

    gate-slot.sh 1 zsh -lic 'bash /tmp/ws-3b-desktop-red.sh'

Queued PID 1000573 was kept in place under the lead's memory-pressure update;
both scripts were reduced to two build jobs before execution. Earlier pending
PID 899997 was stopped before acquiring a slot or executing tests.

| Exact command | Result |
| --- | --- |
| node apps/desktop/os-integration/generate.mjs | Pass; only owned WiX output changes. |
| cargo nextest run -p ariad-desktop --test finder_file_links | 2 passed, 0 skipped. |
| cargo nextest run -p ariad-desktop deep_link | 2 passed; 27 filtered out. |
| cargo nextest run -p ariad-desktop launch_gate | 4 passed; 25 filtered out. |
| pnpm --filter @ariadshift/desktop test:tools | 17 passed, 0 skipped. |
| pnpm --filter @ariadshift/desktop test --maxWorkers=1 test/polish.test.tsx -t 'dismisses the latest message with Escape and expires info notices' | 1 passed; 14 filtered out. |
| pnpm --filter @ariadshift/desktop test --maxWorkers=2 | 25 passed across 2 files. |
| cargo clippy -p ariad-desktop --all-targets | Pass. |
| cargo fmt --all --check | Pass. |
| pnpm exec biome check apps/desktop/os-integration/generate.mjs apps/desktop/os-integration/msi-scope.test.mjs apps/desktop/os-integration/tool-drift.test.mjs apps/desktop/package.json apps/desktop/src-tauri/tauri.windows.conf.json apps/desktop/test/polish.test.tsx | Pass; no fixes applied. |
| just desktop-ci | Pass: generated bindings/permissions diff check, frontend typecheck/build, 25 frontend tests, 17 tool tests, 29 native tests with 0 skipped. |
| just spell | Pass. |
| git diff --check | Pass. |
| just lint-commits 8f6743ac6a0a8837aa2ea140c747bd116506938b..HEAD | Pass for all local commits, including report delivery. |

Terminal line:

    === desktop green ALL GREEN

Filtered cases were subsequently covered by the full frontend/native suites;
neither full suite skipped tests. No database code was touched, and this stream
requires no database tests. No unrelated timing-sensitive failure occurred in
green, so no unrelated-failure rerun was needed.

## Independent review

Final verdict: ACCEPT exact source head
003ebdac3ea7c32442fc26a725f74e880c0b56f2, with no blocking code finding.
Reviewer worktree: /home/vchun/Codes/01-AriadShift-wt-r-3b-desktop.
Report: plans/reports/r-3b-desktop-sol.md in that worktree.
The fresh reviewer read both full logs and independently matched all 13 source
digests to the commit. Prior keypath, release-path, argument-filter and scratch
layout findings are resolved. The timer repair addresses the demonstrated cause.
No further simplification is required. The reviewer has not edited product files
or started builds, tests or servers. Acceptance covers scoped implementation and
Linux evidence; it does not grant publication authority or replace platform checks.

## Architecture handoff and remaining platform evidence

The architecture owner should record strict per-user MSI/NSIS scope, HKCU classic
verbs and MSI protocol registration, LocalAppData/HKCU file-component keypaths,
the pinned custom WiX template and explicit resource authoring, and startup
serialization through single-instance plugin setup. No architecture, vendored
plugin, shared conversion, dependency/lockfile, workflow or root task files were
changed in this stream.

The startup lock is best effort: an unusable lock file or a busy lock after
10 seconds permits ordinary startup. A stalled launch can still produce multiple
windows after that fallback. Linux lock tests do not measure the historical
Windows race or establish the Windows message-forwarding result.

The lead must arrange actual platform evidence before release:

- Windows MSI build/link validation and standard-user MSI/NSIS install, upgrade
  and uninstall; confirm the installing user's verbs/protocol, resource delivery
  and no writes to another user's hive or machine registrations.
- For each Windows installer, cold-start 20 supported files; require exactly one
  window containing all 20, repeated five times.
- On macOS, open a Finder selection and verify file delivery without an
  invalid-link notice.

These checks were not run: this worker has Linux only. No Windows artifact,
installer or macOS runtime result is implied by the source tests. Review the
installer template and packaging guard again when the pinned Tauri CLI changes
or new resource/runtime layouts are introduced.

The detached gate exited. This stream started no dev server, browser or container,
and owns no remaining background process or listener.

Status: DONE_WITH_CONCERNS
