# Cross-model review: engine packs (PR #35, `feat/engine-packs`)

Reviewer: Claude Sonnet 5.5 (xhigh), read-only. Reviewed against Linux x86_64, timezone Asia/Ho_Chi_Minh.

## Scope and evidence

- **Reviewed:** the committed state, local HEAD `c3714ce`, against the full phase scope. The remote PR head is `18977ba`. Its code tree matches local HEAD except for `plan.md`. The local branch was rebased onto a newer `dev`, so pushing it needs a force push, which `docs/git-workflow.md` restricts and needs the maintainer's explicit go-ahead.
- **CI on the remote head:** all green. That covers Verify on ubuntu, macos and windows, Windows lint, Fuzz and CodeQL. It also covers the `build-local-artifacts` jobs for both musl targets, Windows MSVC and both macOS targets, and `plan`. This is the only evidence that the musl, Windows and macOS builds link.
- **Not run:** I did not run `cargo` or any test. Everything below comes from reading the source, the vendored crate sources (`ureq 3.5.0`, `ruzstd 0.9.1`, `minisign-verify 0.3.0`, `toml 1.1.8`), and CI results. The previous reviewer's probe file is a lead only, and none of it was executed.
- **Working tree moved during the review.** Another process is editing the tree. It changed `extract.rs`, `install.rs`, `mod.rs`, `tests/packs.rs`, `docs/packs.md` and the phase file, and added an untracked `src/packs/zstd.rs`. Line numbers below refer to committed HEAD. The in-flight changes are covered at the end.

## Verdict

The design is sound and the phase scope is delivered. That includes the manifest and schema, minisign verification, the index, `--from`, the HTTPS client, extraction rules, receipts, locks, remove and verify, `resolve`, the override, doctor and MCP, plus docs and the Decision Log. Every acceptance-criteria test exists, with one partial case (finding M4).

**Merge blocker:** only H1, and its fix already exists in the working tree but is uncommitted. The medium items should land before the first signed pack ships. Everything else is hardening.

## High

**H1. Multi-frame `tar.zst` is read as a single frame, so archives can be truncated silently**
- **Where:** `crates/ariad-host/src/packs/extract.rs:233-234`. The same path serves `scan` (`extract.rs:179-194`), which `__pack manifest` uses at `author.rs:132`.
- **Cause:** `ruzstd::decoding::StreamingDecoder` reads exactly one frame. Its own docs say so (`streaming_decoder.rs` caveat), and `read` returns EOF once the first frame finishes.
- **Trigger:** a `.tar.zst` made of several frames, such as `pzstd` output, `cat a.zst b.zst`, or zstd's seekable format with its skippable seek-table frame.
- **Impact:** if the cut falls on a tar header boundary, `tar` sees a clean end of archive. The component installs with fewer files, which still pass the upper-bound checks. The receipt records the truncated tree, so `engines verify` passes. Because `scan` shares the code, the manifest records the truncated counts too, and every integrity check agrees with the truncated result. If the cut falls mid-entry, the install fails with a misleading "corrupt archive".
- **Repair:**
  - Commit the multi-frame reader and its test. Both exist only in the working tree: `tests/packs.rs` `zstd_archives_can_span_multiple_frames` is absent from every commit.
  - Also assert after extraction that `Stats.entries` and `Stats.bytes` equal the declared `entries` and `unpacked_size`. `__pack manifest` writes exact values, so equality is safe and catches truncation for every codec. Today `Bounds` only caps the upper side (`extract.rs:39-46`).

## Medium

**M1. Reinstall is called idempotent and intact, but the check compares sizes only**
- **Where:** `install.rs:360-363` calls `quick_check`, which compares file sizes at `install.rs:778-796`.
- **Trigger:** change bytes in `models/weights.bin` without changing its length, then run `ashift engines install docling`.
- **Impact:** it reports `already_installed` and leaves the damaged file in place. `docs/packs.md` and `docs/cli.md` promise "identical, intact".
- **Repair:** the working-tree change (full `verify` before returning `AlreadyInstalled`) is the right direction. See the caution at the end.

**M2. Permanent TLS trust failures are retried, and the CA-bundle hint is unreachable**
- **Where:** `fetch.rs:329-349` and the retry loop at `fetch.rs:299-325`.
- **Cause:** in ureq 3.5.0, `conn.complete_io(&mut sock)?` (`tls/rustls.rs`) turns a handshake failure into an `io::Error`. `From<io::Error>` (`error.rs:218-228`) only unwraps ureq-originated errors, so this becomes `ureq::Error::Io`, which `classify` marks `Retry`.
- **Trigger:** a TLS-intercepting proxy, an empty CA store (minimal container running the static musl binary), or an expired certificate.
- **Impact:** three attempts with 1 s and 2 s of backoff, then `io: invalid peer certificate … (after 3 attempts)`. The `set SSL_CERT_FILE` hint at `fetch.rs:336-341` never fires for this case.
- **Repair:** inspect the wrapped error and treat certificate or handshake errors as `Fail`. Attach the hint whenever roots are empty or the error is a certificate error.
- **Evidence strength:** source reading only.

**M3. The production HTTPS path has no test, and TLS provider selection relies on an internal feature flag**
- **Where:** workspace `Cargo.toml` (`ureq` features `rustls-no-provider` + `_ring`) and `fetch.rs:166-189`.
- **Cause:** every network test uses loopback HTTP. `_ring` is an underscore-prefixed internal feature in ureq (`Cargo.toml:63`), and it is what makes `tls/rustls.rs:148-151` pick ring.
- **Trigger:** a ureq update that renames or drops `_ring`.
- **Impact:** the first HTTPS request would fail at runtime with "No CryptoProvider for Rustls" (`tls/rustls.rs:153-158`). No test or CI job would catch it.
- **Repair:** depend on `rustls` with `ring` directly and set `TlsConfig::rustls_crypto_provider` explicitly. Add one test that attempts HTTPS against an untrusted local server and asserts a typed, non-panicking failure. That test also covers M2.

**M4. A component shorter than the declared size never produces `size_mismatch`**
- **Where:** `fetch.rs:386-390` returns `Attempt::Retry`. For local files, `fetch.rs:247-256` maps it to `PackError::Io`. The only size test, `component_size_mismatch_is_rejected`, covers the too-long body.
- **Trigger:** manifest `size` larger than the real asset (publishing error or truncated upload).
- **Impact:** HTTP retries three times with about 3 s of backoff and ends as `download`. `--from` ends as `io`. The acceptance criteria name "size mismatch" without a direction.
- **Repair:** retry only on transport errors. A clean EOF with fewer bytes than declared should return `SizeMismatch` immediately. Add a test.

## Low

- **L1. `docling` becomes `missing` when the data directory cannot be determined.** `engines.rs:242-252` treats any `try_resolve` error as an unusable pack, and `mod.rs:407` propagates `DataDir`. With no `HOME` and no `ASHIFT_DATA_DIR`, the row changes from `not installed` to `missing` with a misleading note. This is an existing output regression, but the trigger is rare. Treat `DataDir` as "not installed".
- **L2. Every release build prints a failing `pack_keys` check, and an install attempt hits the network first.**
  - `doctor.rs:327-338` and the snapshot show `✗ no engine pack signing keys are built into this ashift`. The hint "use an official release build" is wrong while no official build has keys. Exit code is unaffected.
  - `install.rs:314-327` fetches the manifest before reaching `NoTrustedKeys`, so users see HTTP 404 instead. Fail fast on an empty key set, and soften the doctor message until the first key ships.
- **L3. Doctor's quick check does not verify the signature.** `install.rs:763-776` omits it, although the phase requires receipt, signature, presence and sizes. The working tree already adds it.
- **L4. The entrypoint's executable bit is never checked.** `check_installed_paths` (`install.rs:572-593`) tests `is_file` only. An archive whose entrypoint lacks mode `0o111` installs fine and fails at spawn with `engine describe failed`. Check the Unix mode there or at authoring time.
- **L5. Component URL scheme is not validated by the manifest model.** `manifest.rs:346-348` and `author.rs:113` only check non-empty. A signed `http://` URL passes authoring and fails at install through `Location::parse` as `Usage`, exit 2, which is the wrong category for pack content. Validate the scheme in `Component::validate`.
- **L6. URL helper edge cases.** `fetch.rs:78` computes the base directory by the last `/`, which breaks on query strings containing `/` and on a URL without a path. `fetch.rs:94` appends `.minisig` after any query string. Parse with a URL type and edit the path only.
- **L7. Windows robustness (plausible, not reproduced).**
  - `install.rs:465` renames the staging directory once. AV or indexer locks on freshly written files can cause transient failures, so a short bounded retry is customary.
  - The reserved-name check applies only to archive entries (`extract.rs:441-461`). `dest` and `Unpack::File` component names bypass it (`extract.rs:224-231`). `validate_component_name` (`manifest.rs:405-412`) allows trailing dots. `CONIN$`, `CONOUT$` and superscript `COM¹` are missing from the list. The `cfg(windows)` function has no test.
- **L8. The glibc-loader heuristic can be fooled.** `mod.rs:299-312` treats the presence of `/lib64/ld-linux-x86-64.so.2` as glibc. Alpine compat packages can provide that path on a musl system, so a gnu pack is selected and then fails at launch. Resolve the loader and reject musl targets.
- **L9. Ctrl-C can wait up to 30–60 s during network I/O.** A blocking `agent.get(url).call()` and body reads (`fetch.rs:299`) only observe cancellation between reads. `run_blocking` then awaits the task. A stalled connection delays exit with code 130.
- **L10. Housekeeping races and leftovers (hypothetical).**
  - `sweep` (`install.rs:598-629`) prunes the shared `.cache` while holding only one pack's lock.
  - Failed installs leave multi-GB cache files for up to 7 days.
  - A crash between the two renames (`install.rs:457-476`) can lose the previous version at the next sweep.
  - `.old` trash from `remove` (`install.rs:906-912`) stays until that pack is installed again.
- **L11. `resolve` does not re-check the receipt signature (`install.rs:696-725`).** This would cheaply block tampering with the receipt's manifest copy. Its value is limited, because installed binaries are not re-hashed at launch either.

## Test gaps

- There are no negative zip tests. `walk_zip` has its own mode, encryption and name handling, and only a positive zip install is covered.
- Missing tar edge cases: hard links to a missing target, to a symlink, to a directory or across components; PAX and GNU long names; absolute symlink targets.
- There are no exact-count or short-stream truncation tests, and no short-component test (M4).
- `fuzz/` has no target for `extract`, `Manifest::parse`, `Location::parse` or `check_link_target`, although the phase risk table promises fuzz-style tests and `fuzz-nightly` exists.
- Nothing covers the HTTPS handshake, the `https`→`http` redirect refusal, or manifest URLs with query strings.
- No test covers the `cfg(windows)` name checks.

## Public contract changes

All are documented, and docs, ARCHITECTURE §4, §7.1 and the Decision Log are updated.
- `EngineRow` gains an optional `pack` object, so the JSON is additive and still an array.
- Human `ashift engines` output gains a trailing pack section.
- `docling` status can now be `missing` (L1).
- `doctor` gains `data_dir`, `pack_keys` and `pack_<name>` checks, with `pack_keys` failing in current builds (L2).
- The exit-code table is extended per command.
- The `ariad_host::packs` public module is new in a crate with `publish = true`. `PackError` and the report enums are not `#[non_exhaustive]`, so adding variants later will be a breaking change.
- The persisted `ariad-pack-receipt/1` format uses `deny_unknown_fields`, so an older `ashift` cannot read a newer receipt.
- `schemas/pack.v1.json` is structural only. It has no `const`, `pattern` or `enum` constraints.

## Checked and found sound

- `test-probe` key injection (`mod.rs:230-249`) is absent from release builds. The dist config sets no features, and only `justfile` and `ci.yml` enable `ariad-host/test-probe`.
- `minisign-verify` authenticates the trusted comment through the global signature (`lib.rs:334-346`), and legacy signatures are rejected.
- ureq re-checks `https_only` on every redirect hop (`run.rs:155`), so an HTTPS→HTTP downgrade is refused.
- **Extraction write-through:** `prepare_parent` refuses any non-directory ancestor, and `check_links` canonicalizes every link afterwards. That blocks the zip and tar chained-symlink escapes the earlier probe explored.
- **Deep-TOML lead:** not supported. `toml 1.1.8` has a `RecursionGuard` with `LIMIT = 80` (`de/parser/mod.rs:37,71`).
- JSON outputs and the human `doctor` and `verify` text contain no absolute paths, except where documented.

## In-flight working-tree changes

Uncommitted and unreviewed beyond a read.
- `src/packs/zstd.rs` is a multi-frame decoder that also skips skippable frames. It looks right, and its truncation tests cover both frame kinds. Still missing: a cancellation check inside its frame loop, and the exact-count assertion from H1.
- Calling `verify(...)?` inside `install` (the M1 fix) propagates I/O errors. An unreadable file would abort the install instead of triggering repair. Map every error except `Interrupted` to "not intact". Also note that reinstall now re-hashes every file.

Status: DONE_WITH_CONCERNS
Summary: The committed PR delivers the phase scope, and CI is green on all three OSes and the dist builds. One merge-blocking correctness defect remains, multi-frame zstd handling (H1), and its fix is uncommitted. Four medium items (M1–M4) are best fixed before the first signed pack ships.
Concerns/Blockers:
- H1 must be committed together with an exact-count check and tests before merge.
- Do not rely on the unpushed local rebase. The CI evidence is for `18977ba`, and pushing needs the maintainer's explicit go-ahead.
- M2 rests on source reading rather than execution.
- No tests or builds were run, and the L7 and L8 claims are plausible but not reproduced.


## Resolution after review (2026-10-23)

The implementation owner addressed every high and medium finding:

- H1: `zstd.rs` joins all data frames and skips metadata frames. Extraction also requires the exact signed entry and byte counts. Regression tests cover multiple frames, empty frames, truncated frames and count mismatches.
- M1: reinstall re-hashes files and repairs same-size changes and unreadable files. Only cancellation propagates from the intact-install probe; other probe failures trigger repair.
- M2: rustls errors wrapped in I/O errors are classified as permanent. Certificate errors carry a trusted-CA hint. The HTTPS test verifies that an untrusted certificate causes one connection, without retrying.
- M3: rustls 0.23.46 selects ring explicitly through the client configuration; the ureq `_ring` feature is removed. rcgen 0.14.10 generates throwaway certificates in memory for trusted and untrusted HTTPS tests. Both versions and licences were checked through the Cargo registry.
- M4: a clean short response or local file produces `size_mismatch` immediately. Transport-truncated bodies remain retryable. Both source types and interrupted downloads have passing regression tests.
- L3: the doctor health check re-verifies the recorded manifest signature using the current trust keys.

A separate deterministic regression reproduced two installers consuming the same cached component. Cache names now include the pack name, matching the lock scope. Both installs and subsequent verification pass.

The five targeted review regression tests passed, as did the earlier 33 pack/schema tests. A follow-up Sonnet review was attempted after these changes, but the CLI returned a session-limit error. The initial independent review above is complete; the owner reviewed the repairs directly and continues validation through local checks and PR CI.

The permission concern in the initial report does not apply: the assignment explicitly authorizes rebasing and force-with-lease pushes to this feature branch. No merge or protected-branch push is authorized for this worker.

The remaining low-severity suggestions are retained above as follow-ups. Empty production trust keys are intentional until the signing-key delivery in the release phase. Installed pack files and development overrides are user-controlled local state; re-checking a receipt at every launch would not authenticate locally modified executables. The loader heuristic and cancellation timeouts follow the accepted design, and the review supplied no contradictory empirical evidence. The Windows transient-lock and stale-cache-age concerns were not reproduced. None of these observations changes the stable `InstalledPack`, resolver or manifest-authoring interfaces.


### Cross-platform HTTPS fixture verification

The first repaired implementation passed Linux but exposed a fixture bug on Windows and macOS: accepted sockets inherited nonblocking mode from the listener, and the synchronous TLS read caused the fixture to drop the connection. The behavior is documented by [Winsock accept](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept) and [Apple accept](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/accept.2.html). Forcing that mode on Linux reproduced the failing trusted HTTPS request. Explicitly restoring blocking mode made all three fetch tests pass; the TLS assertions and production client were unchanged.

Both musl release builds linked successfully. The SHA-256-verified x86_64 musl artifact from release rehearsal 39822688241 was confirmed as static PIE and executed locally. Its normal pack-install HTTPS request reached GitHub and returned the expected HTTP 404 for the not-yet-published Docling pack, confirming the actual release binary's TLS provider and OS trust-store path.


### Verified implementation outcome

All high/medium repairs are committed in `6b311c0`; the portable HTTPS fixture correction is committed in `090d7ce`. The final local runs passed all 538 Rust tests, including 35 pack/schema tests, with all remaining quality-gate components passing. The five HTML timing failures under high machine load passed on the final serial run with unchanged thresholds.

[Implementation CI](https://github.com/bavanchun/AriadShift/actions/runs/39823796614) completed successfully on all three operating systems with fuzzing, [CodeQL](https://github.com/bavanchun/AriadShift/actions/runs/39823796626) passed, and [release rehearsal](https://github.com/bavanchun/AriadShift/actions/runs/39823796557) built every release target. No high or medium finding remains open. The initial review and unavailable follow-up are distinguished above; no second independent review is claimed.
