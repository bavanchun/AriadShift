# Fix specs: hardening fuzz oracle, macOS sandbox tests and the dev merge (D9, D1, D0), plus Mac-only Seatbelt fixes (D3, D5, D2)

- Date: 2026-10-27 (Asia/Saigon). Read-only review: nothing was built, run, fetched, merged or checked out.
- Refs read with `git show`, `git grep` and `git diff`: `origin/feat/v1-hardening` at `dcb34bc`, `origin/dev` at `0b118f6` (one docs commit on top of the `e6bd29b` the master plan used), `origin/feat/cloud-product` at `796c83e`.
- Source of truth: `console-review-261027-2045-master-fix-plan-final.md` (MFP-final), rows D0, D1, D2, D3, D5 and D9. Supporting reports: `1935-hardening-fuzz` (FUZZ), `2000-verify-fuzz-sdk` (V-FS), `1935-hardening-desktop-host` (HDH), `2000-verify-desktop-host` (V-HDH), `1935-hardening-sdk-web` (SDKW), `2015-verify-security-branch` (V-SEC). Nothing here duplicates `2045-dev-fix-specs.md` or `2045-dev-core-repro-tests.md`; both cover dev-direct items only.
- Item IDs exist only in this report. Proposed code, test names and commit messages describe behaviour instead (AGENTS.md and the review rules).
- I could not run `git merge-tree` or `gh run list`: both needed approval. The D0 conflict surface below comes from path deltas against the real common ancestor, not from a trial merge.

## 1. Verdict

All six items still hold on the current refs. D9 and D1 are ready to implement now. D0 is ready to specify but gated on PR #84 landing on dev and the OAuth registration port (task 2) landing on hardening. D2, D3 and D5 are Mac-only. D2 also needs OD-8. D3 and D5 are ready to run on a Mac.

## 2. Summary table

| ID | Sev | Still valid | Branch | Files | Size | Worker grouping |
|---|---|---|---|---|---|---|
| D9 | High | yes | `feat/v1-hardening` | `fuzz/fuzz_targets/ir_json_versioned.rs`; 3 new seeds in `fuzz/seeds/ir_json_versioned/`; `crates/ariad-host/tests/ir_io.rs` (append) | S | **W-A**, stage A |
| D1 | High | yes | `feat/v1-hardening` | `crates/ariad-cli/tests/commands_cli.rs` (`:734-770`); `crates/ariad-cli/tests/sandbox_cli.rs` (`:1-4`, `:67-94`); `crates/ariad-server/src/sandbox.rs` (`:198-209`) | S | **W-A**, stage A (macOS leg verified on the Mac) |
| D0 | High | yes, and now more precisely bounded (§3.3) | `feat/v1-hardening` | merge of `origin/dev`; regenerated `schemas/openapi.v1.json`, `crates/ariad-server/tests/snapshots/openapi_v1.json`, `packages/sdk/generator.lock.json`; conflict files listed in §3.3; one optional test in `crates/ariad-server/tests/openapi_v1_snapshot.rs` | L (gates), M (merge itself) | **W-A**, stage B (gated) |
| D3 | High (runtime unverified) | yes | `feat/v1-hardening` | `crates/ariad-host/src/sandbox/macos_profile.rs:12-22`; `crates/ariad-host/tests/sandbox_macos.rs` | S | **Mac**, M-1 |
| D5 | Medium (security) | yes, and wider than the row says: `BASE_EXEC` re-grants `/usr` | `feat/v1-hardening` | `macos_profile.rs:30-39`; `crates/ariad-host/src/sandbox.rs:243-253`; `crates/ariad-host/src/engines/sandbox.rs:75-80`; `sandbox_macos.rs`; `docs/cli.md:499-505` | M | **Mac**, M-1 |
| D2 | High (runtime unverified) | yes | `feat/v1-hardening` | `macos_profile.rs`, `sandbox/macos.rs` (grant option) or `runner.rs:439-463` (exemption option); `engines/sandbox.rs:75-80`; `docs/cli.md:525-527`; `crates/ariad-cli/tests/builtin_engine_contracts.rs:87-90` | M | **Mac**, M-2, after OD-8 |

W-A and the Mac groups share no files. W-A owns the four test files, the fuzz target and its seeds, and the merge. The Mac groups own the host sandbox sources, `sandbox_macos.rs` and `docs/cli.md`. Neither dev nor #84 changes `docs/cli.md` (checked), so the D0 merge does not touch it either.

## 3. Worker task W-A: make the hardening CI gates pass (one sub-branch)

- **Branch:** `feat/v1-hardening--ci-green`, created from `origin/feat/v1-hardening`. Pushing it and merging it into hardening need the owner's go-ahead (AGENTS.md "Git").
- **Order:** stage A (D9, then D1) can start now. Stage B (D0) starts only when both gates hold: task 2 (the OAuth registration cap port, C1–C3) has merged into `feat/v1-hardening`, and PR #84 has merged into `dev` (MFP-final step 2). If stage A merges into hardening first, start stage B from the updated hardening tip.
- **Why A before B is safe:** the three D1 test files and `crates/ariad-host/tests/ir_io.rs` are identical on dev and hardening, and the fuzz target and its seeds exist only on hardening. Neither dev nor #84 changes any of them (`git diff` empty). Distribution edits `commands_cli.rs:813-819`, 70 lines away from D1's hunk, so H-8 stays a clean merge there.
- **Constraints:** no new dependencies. No `ARCHITECTURE.md` edits apart from conflict resolution in D0 (D27 owns the Seatbelt rewrite). Stage explicit paths only. Commits follow `committed.toml`: types `build|chore|ci|docs|feat|fix|perf|refactor|test` (not `style`), subjects of at most 72 characters, imperative and lower case, and no AI references (`scripts/check-commits.sh`). Wrap heavy commands in `flock ~/Codes/01-AriadShift/.run/build.lock` on the ASUS box.
- **The macOS leg:** the ASUS box cannot run macOS tests. D1's failing-before proof runs on the Mac (or on the `verify (macos-26)` CI job once a push is approved). The Mac does this at review.
- **Report:** to `.run/mac-link/to-mac/`, ending with the status block from the orchestration protocol (`Status: DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT`).

### 3.1 D9: `ir_json_versioned` fails its own seeds

**Current behaviour (hardening):**

- `read_versioned` accepts `ariad-ir/0` (`crates/ariad-core/src/ir/mod.rs:13-15`, probe at `crates/ariad-host/src/ir_io.rs:192`) and then overwrites the version: `document.version = IR_VERSION` at `ir_io.rs:207`. Plain `read` (`ir_io.rs:161-169`) keeps the declared version. `Document` derives `PartialEq` over `version` (`ir/mod.rs:18-22`).
- The target asserts `assert_eq!(plain, document)` at `fuzz/fuzz_targets/ir_json_versioned.rs:29`. The check at `:19-22` (`document.version == IR_VERSION`) can never fail, because the reader just set that value.
- All four seeds declare `"version": "ariad-ir/0"` (`vi-garden-notice.json:44`, `en-fenced-code.json:40`, `en-footnotes.json:77`, `vi-aligned-table.json:186`). They are byte-identical to the `ir_json` seeds (same blob ids). The first seed panics during the initial corpus pass.
- The target is in the CI Fuzz loop (`.github/workflows/ci.yml:426`, `shell: bash`, so `-e`) and has a `[[bin]]` entry (`fuzz/Cargo.toml:155-156`). `CI passed` requires the Fuzz job on heavy runs (V-FS). Seeds alone cannot fix this: a one-byte mutation `1`→`0` brings the panic back (V-FS). The oracle has to change.

**Change: replace `fuzz/fuzz_targets/ir_json_versioned.rs` with:**

```rust
#![no_main]

//! The version-checked IR JSON reader (`ir_io::read_versioned`) that `convert` uses for IR
//! input: byte cap, depth prescan, version probe, bounded parse and block count.

use ariad_core::{
    ir::{Document, IR_VERSION, is_supported_version},
    json_depth::{json_depth_budget_for_nesting, prescan_json_depth},
    limits::Limits,
};
use ariad_host::ir_io::{ReadError, read, read_versioned};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

const MAX_INPUT_BYTES: usize = 256 * 1024;

/// The top-level `version` member exactly as the input declares it.
#[derive(Deserialize)]
struct DeclaredVersion {
    version: Option<String>,
}

/// `None` when the input is not an object whose `version` is a string, null or absent.
fn declared_version(data: &[u8]) -> Option<Option<String>> {
    serde_json::from_slice::<DeclaredVersion>(data)
        .ok()
        .map(|declared| declared.version)
}

fn check(data: &[u8], limits: &Limits) {
    match read_versioned(data, limits) {
        Ok(document) => {
            let declared =
                declared_version(data).expect("an accepted document declares its version");
            assert!(
                is_supported_version(declared.as_deref()),
                "a document declaring {declared:?} was accepted"
            );
            assert_eq!(
                document.version, IR_VERSION,
                "an accepted document is upgraded to the current IR version"
            );
            assert!(data.len() as u64 <= limits.max_ir_json_bytes);
            let budget = json_depth_budget_for_nesting(limits.max_nesting_depth);
            assert!(prescan_json_depth(data, budget).is_ok());
            assert!(document.block_count() <= limits.max_blocks as usize);
            // Apart from upgrading the version, the version check only narrows what the plain
            // bounded reader accepts.
            let plain = read(data, limits).expect("the plain reader accepts a versioned document");
            assert_eq!(declared.as_deref(), Some(plain.version.as_str()));
            assert_eq!(
                Document {
                    version: IR_VERSION.to_owned(),
                    ..plain
                },
                document
            );
        }
        Err(ReadError::UnsupportedVersion) => {
            if let Some(declared) = declared_version(data) {
                assert!(
                    !is_supported_version(declared.as_deref()),
                    "a document declaring the supported {declared:?} was refused"
                );
            }
        }
        Err(ReadError::ByteLimitExceeded { limit }) => {
            assert_eq!(limit, limits.max_ir_json_bytes);
            assert!(data.len() as u64 > limit);
        }
        Err(ReadError::BlockLimitExceeded { count, limit }) => {
            assert_eq!(limit, limits.max_blocks);
            assert!(count > limit as usize);
        }
        Err(ReadError::DepthExceeded(_) | ReadError::Json(_) | ReadError::Io(_)) => {}
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    check(data, &Limits::local());
    check(
        data,
        &Limits {
            max_ir_json_bytes: 4096,
            max_nesting_depth: 8,
            max_blocks: 16,
            ..Limits::local()
        },
    );
});
```

Why the oracle has no false positives (checked against `ir_io.rs:171-209`):

- The harness probe has the same shape as the reader's `VersionProbe` (`ir_io.rs:171-174`). In the `Ok` arm the reader's probe succeeded on the same bytes, so the `expect` cannot fire. serde_json skips nested unknown members iteratively, so depth cannot make the two probes disagree.
- `UnsupportedVersion` comes either from a probe that parsed with an unsupported version (the harness sees the same value) or from a data error (the harness probe fails too and the arm skips).
- Duplicate `version` keys make both probes fail as data errors. Plain `read` also fails, because derived `Deserialize` rejects duplicate fields.
- `fuzz/Cargo.toml:16-17` already has `serde` with `derive` and `serde_json`, so there is no new dependency.

**Seeds (keep the four legacy seeds; they cover the upgrade path). Add:**

`fuzz/seeds/ir_json_versioned/current-version.json`

```json
{
  "assets": {},
  "body": [
    {
      "content": [
        {
          "text": "Lịch chăm vườn đọc",
          "type": "text"
        }
      ],
      "level": 1,
      "type": "heading"
    }
  ],
  "layout": null,
  "meta": {
    "authors": [],
    "date": null,
    "keywords": [],
    "language": "vi",
    "source_format": "html",
    "subject": null,
    "title": "Lịch chăm vườn đọc"
  },
  "provenance": null,
  "version": "ariad-ir/1"
}
```

`fuzz/seeds/ir_json_versioned/future-version.json`: the same content with `"version": "ariad-ir/2"`.

`fuzz/seeds/ir_json_versioned/missing-version.json`: the same content without the `version` line. Remove the comma after `"provenance": null`.

These are synthetic, derived from an existing seed, with no third-party content. Existing practice records no fuzz seed in `fixtures/manifest.toml` (FUZZ §3). OD-15 decides whether that changes.

**Regression proof (fails before, passes after):** replay the seeds on Linux from the repository root.

```bash
cargo fuzz run -s none ir_json_versioned fuzz/seeds/ir_json_versioned/*.json
```

Before the fix, the first legacy seed aborts with `assertion left == right failed` and a non-zero exit. After the fix, every file reports `Executed … in … ms` and the command exits 0. Then run `just fuzz ir_json_versioned 60` (`justfile:273-290` accepts a target argument).

**Contract test (passes before and after).** The library is correct, so this does not fail before. It pins the read/read_versioned relationship that nothing else checks (V-FS §5). Append to `crates/ariad-host/tests/ir_io.rs`, which is identical on dev, hardening and cloud-product and already imports `Block`, `Document`, `Limits` and `ir_io`:

```rust
#[test]
fn versioned_read_upgrades_legacy_ir_and_otherwise_matches_the_plain_reader() {
    let current = Document {
        body: vec![Block::Paragraph {
            content: Vec::new(),
        }],
        ..Document::default()
    };
    let mut legacy = serde_json::to_value(&current).unwrap();
    legacy["version"] = serde_json::Value::from("ariad-ir/0");
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let limits = Limits::local();

    let plain = ir_io::read(bytes.as_slice(), &limits).unwrap();
    let versioned = ir_io::read_versioned(&bytes, &limits).unwrap();

    assert_eq!(plain.version, "ariad-ir/0", "the plain reader keeps the declared version");
    assert_eq!(versioned.version, ariad_core::ir::IR_VERSION);
    assert_eq!(
        Document {
            version: ariad_core::ir::IR_VERSION.to_owned(),
            ..plain
        },
        versioned
    );
}

#[test]
fn versioned_read_refuses_versions_the_plain_reader_accepts() {
    let limits = Limits::local();
    for json in [
        br#"{"version":"ariad-ir/2","body":[]}"#.as_slice(),
        br#"{"body":[]}"#.as_slice(),
    ] {
        assert!(ir_io::read(json, &limits).is_ok());
        assert!(matches!(
            ir_io::read_versioned(json, &limits),
            Err(ir_io::ReadError::UnsupportedVersion)
        ));
    }
}
```

**Gate:**

```bash
cargo nextest run -p ariad-host --test ir_io
cargo fmt --manifest-path fuzz/Cargo.toml --check   # the root `just fmt-check` does not cover the fuzz workspace
cargo fuzz run -s none ir_json_versioned fuzz/seeds/ir_json_versioned/*.json
just fuzz ir_json_versioned 60
```

**Commit:**

```
test(fuzz): check versioned IR reads against the declared version

read_versioned upgrades an accepted ariad-ir/0 document to the current
version, so comparing it with the plain read failed on every legacy
seed. Compare after the upgrade, require the declared version to be
supported, and refuse no supported version. Add current, future and
missing-version seeds, and pin how read and read_versioned relate.
```

**Risks:**

- The CI Fuzz job runs all 18 targets. MFP-final D12 is a known false positive in `asset_resolve.rs:117-127` (a `data:` image URI trips the oracle). Its seeds hold no `data:` URI, so a 60 s run probably will not find it, but comparison tracing could. If the first CI run after D9 is red on `asset_resolve`, D12 (S, a different file) is the cause. Consider adding D12 to this task.
- `just fuzz` without an argument still skips the nine new targets (D13, `justfile:279`). That is out of scope here, and D0 touches `justfile`.

### 3.2 D1: three tests still assume macOS has no sandbox

**Current behaviour (hardening; the three files are identical on dev):** hardening makes `SandboxSupport::platform` true on macOS (`crates/ariad-host/src/sandbox/macos.rs:23-28`), and `gaps()` there looks only at the Seatbelt fields (`sandbox.rs:140-144`, `macos.rs:31-41`). The `seatbelt_*` fields exist only under `cfg(target_os = "macos")` (`sandbox.rs:115-122`). `verify` runs `just test` on `macos-26` (`ci.yml:154`, `:257`).

1. `commands_cli.rs:743` asserts `sandbox["platform"] == cfg!(target_os = "linux")`. `DoctorSandbox.support` is flattened (`crates/ariad-host/src/commands/doctor.rs:36-37`), so macOS reports `true`. The test fails. The human-output half still passes, because the CLI rewrite keeps the prefix (`crates/ariad-cli/src/render.rs:379-381`).
2. `sandbox_cli.rs:80-93`. With `ARIAD_TEST_SANDBOX_SUPPORT=none`, the simulated support has `platform: true` on macOS (`sandbox.rs:174-179`). `prepare_launch` returns `Degraded` with the Seatbelt gap (`runner.rs:439-462`). The runner forwards the variable to the launcher (`runner.rs:553-558`), so the launcher runs Pandoc without `sandbox-exec` (`macos.rs:65-89`). The conversion succeeds with a `sandbox_unavailable` warning (`convert.rs:1764-1775`), and `assert!(codes.is_empty())` fails.
3. `crates/ariad-server/src/sandbox.rs:198-208`. The "supported" value sets only `landlock_filesystem` and `seccomp`. On macOS `gaps()` is non-empty, `preflight_with_support(Required, …)` returns `Err`, and `.unwrap()` panics.

No other test assumes this. A grep of the CLI, host and server tests for `NotApplicable`, `["platform"]` and `cfg!(target_os = "linux")` turns up only these three. `crates/ariad-host/tests/sandbox.rs` is `#![cfg(target_os = "linux")]`.

**Change, `crates/ariad-cli/tests/commands_cli.rs` (in `doctor_reports_the_engine_sandbox`):**

```diff
     let sandbox = &report["sandbox"];
     assert_eq!(sandbox["mode"], "best-effort");
-    assert_eq!(sandbox["platform"], cfg!(target_os = "linux"));
+    assert_eq!(
+        sandbox["platform"],
+        cfg!(any(target_os = "linux", target_os = "macos")),
+        "Linux and macOS start engines through the sandbox launcher"
+    );
     for key in [
         "landlock_abi",
         "landlock_filesystem",
         "landlock_network",
         "landlock_scopes",
         "landlock_unix_sockets",
         "seccomp",
     ] {
         assert!(sandbox.get(key).is_some(), "sandbox.{key} is reported");
     }
+    for key in ["seatbelt_available", "seatbelt_usable", "seatbelt_enforced"] {
+        if cfg!(target_os = "macos") {
+            assert!(sandbox[key].is_boolean(), "sandbox.{key} is reported on macOS");
+        } else {
+            assert!(sandbox.get(key).is_none(), "sandbox.{key} is reported only on macOS");
+        }
+    }
+    if cfg!(target_os = "macos") {
+        assert_eq!(
+            sandbox["seatbelt_enforced"], sandbox["seatbelt_usable"],
+            "best effort enforces Seatbelt exactly when its probe succeeds"
+        );
+    }
```

`seatbelt_enforced` is added by `render_doctor_json` on macOS only (`render.rs:305-312`). It is `mode ∈ {best-effort, required} && seatbelt_usable` (`render.rs:351-358`), so in best-effort mode it equals `seatbelt_usable`. That holds even on a runner where `sandbox-exec` is refused. The doctor snapshots strip the sandbox object (`commands_cli.rs:401-441`), so they are unaffected.

**Change, `crates/ariad-cli/tests/sandbox_cli.rs`:**

```diff
-//! Debug builds treat `ARIAD_TEST_SANDBOX_SUPPORT=none` as a kernel without Landlock or seccomp,
-//! so the degraded and fail-closed paths run on every CI kernel.
+//! Debug builds treat `ARIAD_TEST_SANDBOX_SUPPORT=none` as a system without Landlock and seccomp
+//! (Linux) or Seatbelt (macOS), so the degraded and fail-closed paths run on every CI runner.
@@ fn best_effort_without_kernel_support_converts_and_reports_the_gap() {
     let codes = warning_codes(&output.stdout);
-    if cfg!(target_os = "linux") {
+    if cfg!(any(target_os = "linux", target_os = "macos")) {
         assert_eq!(codes, vec!["sandbox_unavailable"]);
         let stderr = String::from_utf8_lossy(&output.stderr);
         assert!(stderr.contains("warning[sandbox_unavailable]"), "{stderr}");
+        let missing = if cfg!(target_os = "macos") {
+            "Seatbelt"
+        } else {
+            "Landlock"
+        };
         assert!(
-            stderr.contains("Landlock"),
+            stderr.contains(missing),
             "the warning names what is missing: {stderr}"
         );
     } else {
-        assert!(
-            codes.is_empty(),
-            "macOS and Windows do not use the launcher"
-        );
+        assert!(codes.is_empty(), "Windows does not use the launcher");
     }
```

The macOS warning reads "The engine ran with reduced isolation: Seatbelt is unavailable (/usr/bin/sandbox-exec is missing), …" (`macos.rs:33-35` via `convert.rs:1770`). `required_mode_fails_closed_without_kernel_support` and `off_mode_never_warns` already hold on macOS.

**Change, `crates/ariad-server/src/sandbox.rs` (test `required_uses_the_host_confinement_contract`):**

```diff
         let supported = SandboxSupport {
             platform: true,
+            #[cfg(target_os = "macos")]
+            seatbelt_available: true,
+            #[cfg(target_os = "macos")]
+            seatbelt_usable: true,
             landlock_filesystem: true,
             seccomp: true,
             ..SandboxSupport::default()
         };
+        assert!(
+            supported.gaps().is_empty(),
+            "the supported value covers this platform's confinement contract"
+        );
         assert!(
             preflight_with_support(SandboxMode::Required, &supported)
```

`#[cfg]` on struct-expression fields is stable Rust. If the worker prefers, the alternative is a `let mut supported` plus a `#[cfg(target_os = "macos")] { … }` block that sets the two fields. On Windows `gaps()` keeps the Landlock/seccomp branch, so the value still has no gaps there.

**Regression proof:** on macOS, the three tests fail on the current tip and pass after the change. On Linux and Windows the outcome does not change. Mac command, run at review:

```bash
cargo nextest run -p ariad-cli --features ariad-host/test-probe --test commands_cli --test sandbox_cli \
  -E 'test(doctor_reports_the_engine_sandbox) | test(best_effort_without_kernel_support_converts_and_reports_the_gap)'
cargo nextest run -p ariad-server --lib -E 'test(sandbox::tests)'
```

**Gate (ASUS, Linux):** the same two commands, then `just fmt-check` and `cargo clippy -p ariad-cli -p ariad-server --all-targets --features ariad-host/test-probe`. **Mac:** the two commands above, then `just test`. That last run is the real `verify (macos-26)` equivalent, and it shows whether any other macOS test fails.

**Commit:**

```
test(sandbox): expect Seatbelt confinement on macOS

macOS now starts engines through the launcher and reports Seatbelt in
doctor, so the doctor, best-effort conversion and server preflight
tests must treat it like Linux: report the platform, the seatbelt_*
fields, and a sandbox_unavailable warning when Seatbelt is missing.
```

**Risks:** low. Test-only. D7 (moving the macOS wording host-side) keeps the same JSON fields, so these assertions survive it.

### 3.3 D0: merge dev into hardening, regenerate the contract and the SDK lock

**Current behaviour (hardening):**

- `schemas/openapi.v1.json` lacks the 408 and 429 (with `retry-after`) responses on `GET` and `PUT /v1/blobs/{token}`. `crates/ariad-server/src/api/blobs.rs` declares them, and `blobs.rs` is identical on dev and hardening. Dev's spec has them at `:132`, `:142`, `:218` and `:238`. `openapi_drift.rs:8-16` compares byte for byte, so `just test` fails.
- `packages/sdk/test/events.test.ts` is the pre-Biome version (SDKW 3), so `just web-lint` fails.
- The SDK, `services/auth/*`, `crates/ariad-server/tests/blob-transfers.rs`, `infra/compose/docker-compose.yml`, `docs/self-hosting.md` and `.env.example` did not exist at the merge base `68ac304`, so git sees them as add/add (V-FS SDK1).

**New finding that makes the merge mechanical.** Hardening's cloud-foundation merge `8526c5f` has `ffcfce8` as its second parent. Dev's #53 squash equals cloud-foundation `9cdf06d` (a descendant of `ffcfce8`) plus a few PR fixups. So `ffcfce8` is the real common content. Git picks `68ac304` only because the squash broke ancestry. Measured with `git diff --stat ffcfce8 origin/dev`, dev's true delta on the shared paths is 15 files and 157/−55 lines:

| Dev-only change since `ffcfce8` | Files | Resolution |
|---|---|---|
| `504a212`: Biome formatting | `services/auth/{README.md,package.json,src/app.ts,test/config.test.ts}`, `packages/sdk/test/events.test.ts` | Formatting only (verified). Keep hardening's content, then rerun Biome. `events.test.ts` has no hardening delta, so take dev's. |
| `9a9f1ef`, `9cdf06d`: spec and lock | `schemas/openapi.v1.json`, `packages/sdk/generator.lock.json` | Regenerate. Never hand-merge. |
| #53 PR fixups: random test signing keys, `stop_api` | `crates/ariad-server/tests/{blob-transfers.rs,common/mod.rs,common/signing_key.rs,jobs.rs}`; `docs/ci.md`; `justfile` | Hardening has no delta on these test files since `ffcfce8`, so take dev's. Hardening's new server tests do not use `common::SECRET` (grep), and `Harness::stop()` keeps its signature. |
| #51 | `ARCHITECTURE.md` (+2), `docs/ci.md`, `docs/server.md`, `justfile` | Plain three-way. Hardening's delta there is 1, 2 and 4 lines. |

Hardening's own delta on those same files since `ffcfce8` touches only `ARCHITECTURE.md`, `docs/ci.md`, `justfile` and the four `services/auth` files above. Everything else conflicts only because of the wrong base.

After step 2, dev also carries #84. Its files that overlap hardening are `ARCHITECTURE.md`, `docs/{api,ci,self-hosting}.md`, `infra/compose/docker-compose.yml`, `justfile`, `packages/sdk/{package.json,src/client.ts,src/index.ts,test/client.test.ts}` (plus the new `src/hash.ts` and `test/hash.test.ts`), `pnpm-lock.yaml`, `services/auth/src/auth.ts` and `services/auth/test/auth.test.ts`. #84 does not touch `schemas/`, `crates/ariad-server/` or any stage A file. `ffcfce8` stays a valid base, because post-#84 dev still descends from the #53 content.

**Procedure (stage B):**

```bash
git fetch origin
git switch feat/v1-hardening--ci-green          # worker's own worktree; already contains stage A
git merge --no-ff --no-commit origin/dev
# Re-merge each conflicted file against ffcfce8, the content both sides really share.
tmp=$(mktemp -d)
git diff --name-only --diff-filter=U | while IFS= read -r f; do
  if git cat-file -e "ffcfce8:$f" 2>/dev/null && git cat-file -e ":2:$f" 2>/dev/null \
     && git cat-file -e ":3:$f" 2>/dev/null; then
    git show ":2:$f" > "$f"
    git show "ffcfce8:$f" > "$tmp/base"
    git show ":3:$f" > "$tmp/theirs"
    git merge-file -L HEAD -L ffcfce8 -L origin/dev "$f" "$tmp/base" "$tmp/theirs" \
      || echo "needs a hand resolution: $f"
  fi
done
```

Then resolve what remains by hand, with these rules (H-2 and H-3 from MFP-final):

1. `schemas/openapi.v1.json`: take either side, then run `just openapi`.
2. `packages/sdk/generator.lock.json`: take either side, then run `pnpm --filter @ariadshift/sdk lock` after step 1.
3. `packages/sdk/src/client.ts`: keep hardening's `listJobs`, `limits` and `deleteUpload` wrappers and #84's hash import and additions. Typecheck the combined file.
4. `services/auth/src/auth.ts`: keep #84's `user.deleteUser` block with `beforeDelete` (`cloud-product:auth.ts:67-90`) and hardening's plugin list with `twoFactor(...)` before `oauthProvider(...)` (`hardening:auth.ts:151-152`). Keep task 2's registration cap. Do the same for `test/auth.test.ts`.
5. `pnpm-lock.yaml`: resolve the `package.json` files first, then run `pnpm install`, then `pnpm install --frozen-lockfile` to prove the lock is consistent.
6. Formatting: `pnpm exec biome format --write services/auth packages/sdk`. Biome covers `services/**` markdown too (`biome.json` `files.includes`).
7. `ARCHITECTURE.md`, `docs/*`, `justfile`, compose: plain hunk resolution, keeping both sides' lines. Do not rewrite §11.3 here (D27).

Fold the conflict resolution and the Biome pass into the merge commit. Commit the regenerated artifacts separately, as the cloud-foundation branch did (`9a9f1ef`, `9cdf06d`).

**Sanity check before committing:** every path in `git diff --name-only origin/dev HEAD` must also appear in `git diff --name-only ffcfce8 origin/feat/v1-hardening`, the stage A files, or the regenerated artifacts (`schemas/openapi.v1.json`, the snapshot, `packages/sdk/generator.lock.json`, `pnpm-lock.yaml`). Any other path means a dev change was dropped. For each shared path, `git diff origin/dev HEAD -- <path>` must show no revert of a hunk from `git diff ffcfce8 origin/dev -- <path>`.

**Snapshot.** `crates/ariad-server/tests/snapshots/openapi_v1.json` is hardening-only and lacks the blob 408/429. The checker treats them as additive (`openapi_v1_snapshot.rs:1-14`), so a refresh is optional (MFP-final D0). I recommend refreshing, because D16 will make `baseline == current` mandatory. The refresh freezes 408/429 into `/v1`, though, and OD-13 asks whether `/v1/blobs/{token}` belongs to the frozen contract. Refresh only if OD-13 says inside or is unanswered:

```bash
ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot
```

**Regression tests.** Three existing checks fail on the current tip and pass after the merge and regeneration:

- `cargo nextest run -p ariad-server --test openapi_drift` (`openapi_document_is_committed_and_current`)
- `just sdk-check`
- `pnpm exec biome check packages/sdk/test/events.test.ts`

If the snapshot is refreshed, add this to `crates/ariad-server/tests/openapi_v1_snapshot.rs`, which already imports `fs` and `Value` and defines `baseline_path`. It fails on the current snapshot and passes after the refresh:

```rust
#[test]
fn the_frozen_snapshot_keeps_the_blob_timeout_and_busy_responses() {
    let text = fs::read_to_string(baseline_path()).expect("read the snapshot");
    let baseline: Value = serde_json::from_str(&text).expect("parse the snapshot");
    for method in ["get", "put"] {
        let responses = &baseline["paths"]["/v1/blobs/{token}"][method]["responses"];
        assert!(responses["408"].is_object(), "{method} documents 408");
        assert!(
            responses["429"]["headers"]["retry-after"].is_object(),
            "{method} documents 429 with Retry-After"
        );
    }
}
```

**Gate (ASUS):**

```bash
just openapi && git diff --exit-code schemas/openapi.v1.json            # stable after commit
pnpm --filter @ariadshift/sdk generate && pnpm --filter @ariadshift/sdk check \
  && pnpm --filter @ariadshift/sdk typecheck && pnpm --filter @ariadshift/sdk test
pnpm --filter @ariadshift/auth typecheck && pnpm --filter @ariadshift/auth test
just test-server   # starts PostgreSQL on 127.0.0.1:55442; then, with it running:
ARIAD_TEST_DATABASE_URL=postgres://postgres:ariad@127.0.0.1:55442/postgres pnpm --filter @ariadshift/auth test
just ci
just lint-commits origin/feat/v1-hardening..HEAD
```

`just mcp-oauth-e2e` brings up the compose stack. Run it when the ASUS box has headroom, since it also hosts the Capstone stack. Otherwise leave it for the step 3b gate.

**Commits:**

```
chore: merge dev into feat/v1-hardening
chore(schemas): regenerate the OpenAPI document after the dev merge
chore(sdk): lock the generator to the regenerated OpenAPI document
test(server): freeze the blob timeout and busy responses in /v1      # only if refreshed
```

**Risks:**

- Hardening's tests are compiled against dev's `tests/common` changes (`SECRET` became a `LazyLock<String>`, `stop()` now `expect`s the API task). By grep, hardening's new tests do not use `SECRET`, but a panicking API task now fails tests that used to pass silently.
- `just ci` includes `desktop-ci` and `wasm`. A red result there may come from the environment, not from the merge. Report it with the output.
- Task 2 must already have landed, or `services/auth` conflicts twice.

## 4. Mac-only spec: D3, D5, D2 (Seatbelt)

All three edit the macOS sandbox and need real `sandbox-exec`. Run them on the Mac on a sub-branch of `feat/v1-hardening` (for example `feat/v1-hardening--seatbelt`), after W-A stage A or in parallel with it (no shared files). D4 (OD-8, "present but refused") and D26 (`file-map-executable` on read-only library directories) edit the same two files, so consider bundling them, but they are not specified here. Gate for every item: `cargo nextest run -p ariad-host --features test-probe --test sandbox_macos` and `cargo nextest run -p ariad-host --lib`, then `just test` on the Mac.

### 4.1 D3 (M-1): allow `kern.hostname`

**Current:** the `sysctl-read` list (`macos_profile.rs:12-22`) omits `kern.hostname`. `uname(3)` reads it, and CPython's `platform.uname()` does not catch the resulting `OSError`, so the torch import in every Docling job may fail (V-HDH N1, not verified at runtime). No test runs `uname` under the profile.

**Change:**

```diff
     (sysctl-name "kern.argmax")
+    (sysctl-name "kern.hostname")
     (sysctl-name "kern.osproductversion")
```

**Test (append to `crates/ariad-host/tests/sandbox_macos.rs`; it uses only that file's existing helpers and imports):**

```rust
#[test]
fn engines_can_read_the_system_identity() {
    if !seatbelt_available() {
        return;
    }
    let uname = std::path::Path::new("/usr/bin/uname");
    let policy = SandboxPolicy::for_engine("probe", uname, &[]);
    let output = Command::new(probe())
        .args(policy.launcher_args(true, uname, &["-a".into()]))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("Darwin "));
}
```

If this passes without the change, V-HDH's mechanism is wrong. Keep the test, skip the profile change, and record that D3 is refuted. In both cases, run one real Docling conversion under the default best-effort mode, with the pack installed or `ASHIFT_DOCLING_ENGINE` set:

```bash
target/debug/ashift convert fixtures/scan/vi-garden-300dpi.png --to md --json -o "$TMPDIR/garden.md"
```

The output must contain `Khu vườn đọc sách` and carry no `sandbox_unavailable` warning.

**Commit:** `fix(sandbox): let macOS engines read the host name through uname`. **Risk:** the host name becomes readable inside the sandbox. It is low-value, and the network is denied.

### 4.2 D5 (M-1): keep package-manager state out of the grants

**Current:** `SYSTEM_EXEC` grants all of `/usr` (and so `/usr/local/var` and `/usr/local/etc`, where Intel Homebrew keeps service data) and all of `/opt/homebrew` (`var/postgresql@*`, `var/mysql`, `etc`) with read and exec (`macos_profile.rs:30-39`). V-HDH also found that `libreoffice_paths` grants `read_only: /opt` on every platform (`engines/sandbox.rs:76-77`). **New:** narrowing `SYSTEM_EXEC` alone does nothing, because `SandboxPolicy::for_engine` also pushes the Linux `BASE_EXEC` list (`sandbox.rs:243-253`, which includes `/usr`) into `policy.exec`, and the profile grants `policy.exec` too (`macos_profile.rs:59-64`).

**Change:**

1. `macos_profile.rs`: replace `"/usr"` and `"/opt/homebrew"` in `SYSTEM_EXEC` with explicit runtime directories:
   - `/usr/{bin,sbin,lib,libexec,share}`
   - `/opt/homebrew/{bin,sbin,lib,libexec,opt,Cellar,share,Frameworks}`
   - `/usr/local/{bin,sbin,lib,libexec,opt,Cellar,share,Frameworks}`

   Paths that do not exist are already skipped by `grant`. Update the comment to say that package-manager state (`var`, `etc`) is never granted.
2. `sandbox.rs`: make `BASE_EXEC` platform-specific, so the Seatbelt profile owns the macOS system allow-list:
   ```rust
   /// System paths every engine may execute from (libraries, the dynamic loader, shells).
   /// On macOS the Seatbelt profile owns the system allow-list instead.
   #[cfg(not(target_os = "macos"))]
   const BASE_EXEC: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/nix/store"];
   #[cfg(target_os = "macos")]
   const BASE_EXEC: &[&str] = &[];
   ```
3. `engines/sandbox.rs:75-80`: `read_only: if cfg!(target_os = "linux") { vec![PathBuf::from("/opt")] } else { Vec::new() }`. Keep the "Standard Linux installs" comment.
4. `docs/cli.md:499-505`: state that Homebrew and `/usr/local` state directories (`var`, `etc`) are not readable by engines.

**Tests:**

Append to the `tests` module in `macos_profile.rs`. It needs no sandbox run, and it fails today on any Mac because `/usr` is granted:

```rust
#[test]
fn system_grants_leave_package_manager_state_out() {
    let policy = SandboxPolicy::for_engine("libreoffice", Path::new("/usr/bin/true"), &[]);
    let profile = Profile::new(&policy).unwrap();
    let granted: Vec<&str> = profile
        .parameters
        .iter()
        .filter_map(|parameter| parameter.to_str()?.split_once('=').map(|(_, path)| path))
        .collect();
    for state in ["/usr/local/var", "/usr/local/etc", "/opt/homebrew/var", "/opt/homebrew/etc"] {
        assert!(
            !granted.iter().any(|path| Path::new(state).starts_with(path)),
            "{state} is reachable through a broader grant: {granted:?}"
        );
    }
}
```

Append to the `tests` module in `engines/sandbox.rs`. It runs on every OS and fails today on macOS and Windows:

```rust
#[test]
fn libreoffice_reads_opt_only_on_linux() {
    let reads_opt = paths_for("libreoffice")
        .read_only
        .contains(&std::path::PathBuf::from("/opt"));
    assert_eq!(reads_opt, cfg!(target_os = "linux"));
}
```

Append to `sandbox_macos.rs`. It is a runtime check on a Mac with Homebrew state, and it skips cleanly elsewhere:

```rust
#[test]
fn seatbelt_keeps_package_manager_state_unreadable() {
    if !seatbelt_available() {
        return;
    }
    let samples: Vec<PathBuf> = ["/opt/homebrew/var", "/opt/homebrew/etc", "/usr/local/var", "/usr/local/etc"]
        .into_iter()
        .filter_map(|dir| readable_file_under(std::path::Path::new(dir), 3))
        .collect();
    if samples.is_empty() {
        eprintln!("skipping: no package-manager state on this Mac");
        return;
    }
    let workspace = Workspace::new().unwrap();
    let command = engine(SandboxMode::Required);
    for target in &samples {
        let outcome = runner::run_command(
            &command,
            &request(&workspace, BTreeMap::from([("read".to_owned(), json!(target))])),
            CancellationToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            outcome.metrics.unwrap()["read_install_root"],
            "denied",
            "{}",
            target.display()
        );
    }
}

fn readable_file_under(dir: &std::path::Path, depth: usize) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_file() && fs::File::open(&path).is_ok() {
            return Some(path);
        }
        if file_type.is_dir() && depth > 0 {
            if let Some(found) = readable_file_under(&path, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}
```

**Regression check:** `seatbelt_allows_a_real_pandoc_conversion_in_a_quoted_job_directory`, and D3's Docling run with Homebrew Tesseract, must still pass. Tesseract's dylibs resolve through `/opt/homebrew/opt` → `Cellar`, and both stay granted.

**Commit:** `fix(sandbox): keep Homebrew and /usr/local state out of Seatbelt grants`.

**Risks:** an engine that needs files from another Homebrew directory (for example fontconfig under `/opt/homebrew/etc/fonts`) breaks. The Mac run surfaces this. Add a narrow read-only grant only for what fails, and record it in the comment.

### 4.3 D2 (M-2): LibreOffice under Seatbelt (needs OD-8)

**Current:** best-effort is the macOS default (`sandbox.rs:87-94`), so `ashift __engine libreoffice` and its `soffice` child run under a profile with no UNIX-socket or `network*` grant (`macos_profile.rs:83-86`), a single read-write root (the workspace), and three mach services. The macOS launcher ignores `policy.private_tmp` (no reference in `macos.rs`). LibreOffice creates its single-instance IPC socket before `--convert-to` runs, so Office conversions are expected to fail (V-HDH, not verified at runtime). `docs/cli.md:525-527` calls the denial intended but never says Office fails. The macOS CI runner has no LibreOffice, so `builtin_engine_contracts.rs:100-103` skips Office.

**Step 1: reproduce and capture (before any code).** On a Mac with LibreOffice ≥24.2 in `/Applications`:

```bash
log stream --style compact --predicate 'sender == "Sandbox"' > "$TMPDIR/seatbelt-denials.log" &
cargo nextest run -p ariad-cli --features ariad-host/test-probe \
  -E 'test(pdf_typst_and_office_execute_through_the_sandbox_launcher)'   # expected to fail at the Office leg
target/debug/ashift plan fixtures/office/vi-garden-notes.doc --to docx --json   # confirm the route uses libreoffice
target/debug/ashift convert fixtures/office/vi-garden-notes.doc --to docx --json -o "$TMPDIR/notes.docx"
kill %1
```

The denial log names the socket path and every other refused operation. It decides between the two shapes below. If the test passes, D2 is refuted: record that and stop.

**Step 2, option (a), grant (keeps Office confined; my recommendation if the log shows only the socket and a few mach services):**

- If the socket lands under the workspace (the engine sets `TMPDIR` to the job's work directory, `engines/libreoffice.rs:394`), grant UNIX-socket bind and connect under the workspace only, when `policy.private_tmp` is set (only LibreOffice sets it, `engines/sandbox.rs:78`). Candidate SBPL, which must be validated with `sandbox-exec -p` on the Mac because the syntax is unverified:
  ```
  (allow network-bind network-inbound (local unix-socket (subpath (param "PATH_n"))))
  (allow network-outbound (remote unix-socket (subpath (param "PATH_n"))))
  ```
  Here `PATH_n` is the workspace parameter `grant` already registers.
- If it lands in `/private/tmp` or `/private/var/tmp`, grant only LibreOffice's pipe name there through a `regex` filter on `file-write-create`, `file-write-unlink` and the socket operations. Never grant `/tmp` as a whole (`docs/cli.md:526-527`).
- Add only the mach-lookup names the log shows.
- `seatbelt_confines_workspace_installation_and_network` must still report `unix_connect` as denied for the socket outside the workspace.

**Step 2, option (b), exemption:** in `prepare_launch` (`runner.rs:439-463`), on macOS, for `command.engine == "libreoffice"`:

- best-effort runs it directly with `SandboxOutcome::Degraded("LibreOffice runs without Seatbelt on macOS because its single-instance IPC needs a UNIX socket")`;
- required returns `RunError::SandboxUnavailable` with the same reason.

Doctor and `docs/cli.md` must say so.

**Tests:**

- Both options: `pdf_typst_and_office_execute_through_the_sandbox_launcher` with Office not skipped (it fails today on such a Mac), plus the real `.doc`→DOCX conversion above.
- Option (b) only: change `builtin_engine_contracts.rs:88-90` so it expects `Degraded` for `libreoffice` on macOS, and add a `sandbox_macos.rs` case asserting the required-mode refusal.
- Option (a) only: add a `sandbox_macos.rs` case asserting that a UNIX socket bound outside the workspace is still denied for the LibreOffice policy.

**Docs:** rewrite `docs/cli.md:525-527` to match the choice. The Decision Log row belongs to D27.

**Commit:** `fix(sandbox): allow LibreOffice's IPC socket under Seatbelt`, or `fix(sandbox): run LibreOffice outside Seatbelt with a reported gap`.

**Risks:** option (a) widens the profile, so keep it path-scoped. Option (b) leaves Office unconfined on macOS. That is a regression from the "Seatbelt by default" intent, but not from the shipped v0.1 behaviour, where macOS engines ran unsandboxed.

## 5. Owner decisions

| OD | Why it matters here |
|---|---|
| OD-8 | D2's shape (grant or exempt), D5's Linux-only `/opt` (specified as Linux-only), and D4 if bundled. |
| OD-13 | Whether `/v1/blobs/{token}` is inside the frozen contract. That decides whether D0 refreshes the snapshot and adds the snapshot test. |
| OD-15 | Whether fuzz seeds count as fixtures. If yes, D9's three new seeds (and all existing seeds) need `fixtures/manifest.toml` entries. |
| (go-ahead) | Pushing `feat/v1-hardening--ci-green` and merging it into hardening; merging PR #84 into dev (step 2), which gates D0. Not a new OD; AGENTS.md "Git". |

## 6. Unresolved questions

1. CI state is still unknown: has the Fuzz job run on any commit with `f57b1c1`, and has `verify (macos-26)` run on `dcb34bc`? `gh run list` needed approval.
2. The exact D0 conflict set is inferred from deltas against `ffcfce8`, not from `git merge-tree` (not permitted). Running the procedure in §3.3 settles it.
3. Will D12's `data:` false positive in `asset_resolve` trip a 60 s CI run once D9 is fixed? If it does, add D12 to W-A.
4. If step 2 slips, should D0 merge today's dev now (H-2 only) and #84 later (H-3)? Both merges would use the same `ffcfce8` procedure, and the second would be small.
5. D2: does LibreOffice on macOS put its pipe under `TMPDIR` or in `/tmp`? D3: does `uname(3)` need `kern.hostname` under this profile? Both are runtime facts for the Mac run, as is the exact SBPL form for UNIX-socket path filters.
