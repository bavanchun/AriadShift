# Fix specs for the remaining `feat/v1-hardening` items (D4, D6–D8, D10–D27)

Date: 2026-10-27 (Asia/Saigon). Reviewer: Mac console, read-only. Nothing was built, run, generated or checked out. Every file:line below is on `origin/feat/v1-hardening` at `dcb34bc` unless it says otherwise. `origin/dev` is now `0b118f6` (one docs commit after `e6bd29b`). Source of truth: `console-review-261027-2045-master-fix-plan-final.md` §4 D. Not repeated here: D0, D1, D2, D3, D5, D9 (other specs), and everything in `console-review-261027-2045-dev-fix-specs.md` and `console-review-261027-2045-dev-core-repro-tests.md` (no overlap with these items).

## 1. Verdict

All 22 items are still valid on `dcb34bc`; none is stale or already fixed. Fifteen are ready to implement once their prerequisites land (D0 for everything, C1 for the auth items). Five are ready with a recommended default but wait on an owner choice (D4, D8, D19, D26b, D27 wording). Two are partly blocked: D18 part 2 (OD-7 plus a Better Auth check) and D22 (a Better Auth check on ASUS).

## 2. Summary

### 2.1 Items

| ID | Sev | Still valid | Files (hardening) | Size | Worker |
|---|---|---|---|---|---|
| D4 | Medium (OD-8) | yes | `crates/ariad-host/src/sandbox/macos.rs`, `crates/ariad-host/src/sandbox.rs`, `docs/cli.md:509-523`, `crates/ariad-host/tests/sandbox_macos.rs` | S–M | W2 (Mac) |
| D6 | Medium | yes | `apps/desktop/src-tauri/src/deep_link.rs` | S | W3 |
| D7 | Medium | yes | `crates/ariad-host/src/commands/doctor.rs`, `crates/ariad-cli/src/render.rs`, `crates/ariad-host/src/runner.rs:405,424`, new `crates/ariad-cli/tests/doctor_sandbox_cli.rs` | S | W1 |
| D8 | Medium (timing unmeasured) | yes (by reading) | `apps/desktop/src-tauri/src/lib.rs`, new `src-tauri/src/launch_gate.rs`, `docs/desktop.md:135-138` | M | W3 |
| D10 | Medium | yes | `crates/ariad-host/src/runner.rs:824-828`, `crates/ariad-host/tests/runner.rs`, `fuzz/fuzz_targets/engine_protocol.rs`, new seed | M | W1 |
| D11 | Medium | yes | `crates/ariad-host/src/packs/{extract,mod}.rs`, `crates/ariad-host/tests/packs.rs`, `fuzz/fuzz_targets/pack_archive.rs` | M | W1 |
| D12 | Medium | yes | `fuzz/fuzz_targets/asset_resolve.rs`, new seed | S | W1 |
| D13 | Low | yes (7 sub-parts) | a–d: `fuzz/fuzz_targets/{pack_manifest,docx_meta,uri_parsers}.rs`, `fuzz/Cargo.{toml,lock}`; e: `ARCHITECTURE.md:750,756-766`; f–g: `justfile:273-291`, `.github/workflows/fuzz-nightly.yml`, `docs/ci.md:64` | M | W1 (a–d), W9 (e), W8 (f–g) |
| D14 | Medium | yes | `packages/sdk/src/client.ts:285-300`, `packages/sdk/test/client.test.ts` | S | W4 |
| D15 | Medium | yes (coverage only) | `packages/sdk/test/list-jobs.test.ts`, new `packages/sdk/test/uploads-and-limits.test.ts` | S | W4 |
| D16 | Medium | yes | `crates/ariad-server/tests/openapi_v1_snapshot.rs:38-53`, `tests/snapshots/openapi_v1.json`, `docs/api.md:138` | S | W5 |
| D17 | Medium | yes | `crates/ariad-server/src/api/mod.rs:123-147`, `tests/{auth,mcp_remote}.rs`, `docs/server.md:113,200` | S | W6 |
| D18 | Medium (OD-7) | yes | `services/auth/src/{app,oauth-lock}.ts`, `test/{app,oauth-postgres}.test.ts`, `test/oauth-helpers.ts`, new `test/oauth-lock.test.ts`, `README.md` | M (part 2: L, blocked) | W7 |
| D19 | Medium (OD-9) | yes | `services/auth/src/{config,auth,app,main}.ts`, `test/{config,auth,app}.test.ts`, `README.md`, `docs/self-hosting.md`, `.env.example` | M | W7 |
| D20 | Medium | yes | CI: `.github/workflows/ci.yml` server job, `cloud-e2e.yml` (after D0), `justfile`, `docs/ci.md`; guard: `services/auth/test/oauth-postgres.test.ts`, `README.md:115` | S–M | W8 (CI), W7 (guard) |
| D21 | Unverified; critical if it fails | yes (gap) | new `services/auth/test/oauth-two-factor.test.ts` | S | W7 |
| D22 | Medium (library unverified) | partly | `services/auth/src/auth.ts`, new `test/oauth-linking.test.ts` | S | W7 |
| D23 | Low | yes | `services/auth/src/{app,oauth-lock}.ts`, `src/pages/oauth.js`, `test/app.test.ts`, new `test/oauth-page.test.ts` | S | W7 |
| D24 | Low | yes | `infra/test/e2e/mcp-oauth.sh`, `services/auth/test/oauth-diagnostics.test.ts`, `pnpm-workspace.yaml`, `services/auth/package.json:20`, `pnpm-lock.yaml` | S | W8 |
| D25 | Low | yes (part of SS 11 is covered by E0 `90787ce`) | a–b: `src/mcp/tools.rs:179-218`, `src/storage.rs`, `tests/mcp_remote.rs`, new `tests/inline_storage.rs`; c–d: `src/api/dto.rs:345-403`, `openapi_v1_snapshot.rs`, spec, snapshot, SDK lock | M | W6 (a–b), W5 (c–d) |
| D26 | Low | yes | a: `sandbox/macos_profile.rs:73-79`; b: `apps/desktop/os-integration/generate.mjs:322-350`, regenerated `.wxs`, `generate.test.mjs`; c: `apps/desktop/package.json:11` | S | W2 (a), W3 (b, c) |
| D27 | Medium (docs) | yes | `ARCHITECTURE.md` only | M | W9 |

### 2.2 Worker tasks (disjoint file sets)

Every worker branches from hardening **after D0** (dev, including #84, merged into hardening) and **after C1** (step 3a). Line numbers shift after D0 in `auth.ts`, `docs/*` and the spec; re-locate by symbol. ASUS runs workers one at a time, so the order column is the recommended sequence.

| Worker | Items | Files owned | Machine | Gate | Order |
|---|---|---|---|---|---|
| W1 host and fuzz | D7, D10, D11, D12, D13 a–d | `crates/ariad-host/src/{runner.rs,commands/doctor.rs,packs/extract.rs,packs/mod.rs}`, `crates/ariad-cli/src/render.rs`, `crates/ariad-host/tests/{runner,packs}.rs`, new `crates/ariad-cli/tests/doctor_sandbox_cli.rs`, `fuzz/fuzz_targets/{engine_protocol,pack_archive,asset_resolve,pack_manifest,docx_meta,uri_parsers}.rs`, `fuzz/seeds/{engine_protocol,asset_resolve}/*`, `fuzz/Cargo.{toml,lock}` | Linux (ASUS); macOS legs through CI `verify` | `just test`, `just clippy`, `just fuzz <target> 60` per touched target, `just ci` | 1 |
| W2 Mac sandbox | D4, D26a (plus D2, D3, D5 from the other spec, same files) | `crates/ariad-host/src/sandbox/{macos,macos_profile}.rs`, `crates/ariad-host/src/sandbox.rs`, `docs/cli.md`, `crates/ariad-host/tests/sandbox_macos.rs` | a Mac | `cargo nextest run -p ariad-host --features test-probe --test sandbox_macos`, `just test`, `just ci` | parallel (Mac) |
| W3 desktop | D6, D26c, D26b, D8 | `apps/desktop/src-tauri/src/{deep_link,lib}.rs`, new `launch_gate.rs`, `apps/desktop/os-integration/{generate.mjs,generate.test.mjs}`, regenerated `apps/desktop/src-tauri/windows/os-integration.*`, `apps/desktop/package.json`, `docs/desktop.md` | any; D8 and D26b need a manual Windows check | `cargo nextest run -p ariad-desktop`, `pnpm --filter @ariadshift/desktop test:tools`, `just desktop-ci` | 2 |
| W4 SDK | D14, D15 | `packages/sdk/src/client.ts`, `packages/sdk/test/{client,list-jobs}.test.ts`, new `packages/sdk/test/uploads-and-limits.test.ts` | any | `pnpm --filter @ariadshift/sdk generate && … check && … typecheck && … test`, `just web-lint` | 3 |
| W5 API contract | D16, then D25 c–d | `crates/ariad-server/tests/openapi_v1_snapshot.rs`, `crates/ariad-server/tests/snapshots/openapi_v1.json`, `crates/ariad-server/src/api/dto.rs`, `schemas/openapi.v1.json`, `packages/sdk/generator.lock.json`, `docs/api.md` | any | `just openapi`, snapshot refresh, `pnpm --filter @ariadshift/sdk lock`, `just sdk-check`, `just ci` | 4 |
| W6 server auth and MCP | D17, D25 a–b | `crates/ariad-server/src/{api/mod.rs,mcp/tools.rs,storage.rs}`, `crates/ariad-server/tests/{auth,mcp_remote}.rs`, new `crates/ariad-server/tests/inline_storage.rs`, `docs/server.md` | Linux with Docker | `just test-server` (check the log has no "skipping"), `just clippy` | 5 |
| W7 auth service | D21, D23, D18 parts 1 and 3, D19, D22, D20 guard | `services/auth/src/**`, `services/auth/test/**` except `oauth-diagnostics.test.ts`, `services/auth/README.md`, `docs/self-hosting.md`, `.env.example` | Linux; PG18 for the postgres suite | `pnpm --filter @ariadshift/auth typecheck`, `… test`, `pnpm exec biome check services/auth`, `just test-auth-db` | 6 |
| W8 CI and e2e wiring | D24, D20 CI side, D13 f–g | `justfile`, `docs/ci.md`, `.github/workflows/{ci.yml,fuzz-nightly.yml,cloud-e2e.yml}`, `infra/test/e2e/mcp-oauth.sh`, `services/auth/test/oauth-diagnostics.test.ts`, `pnpm-workspace.yaml`, `services/auth/package.json`, `pnpm-lock.yaml` | Linux with Docker | `just lint-workflows`, `bash -n infra/test/e2e/mcp-oauth.sh`, `just mcp-oauth-e2e`, `just js`, `just js-licenses`, `just test-auth-db` | 7 |
| W9 architecture | D27, D13 e, plus the Decision Log rows for D4, D8 and D26b | `ARCHITECTURE.md` | any | `just spell`, `lychee --config lychee.toml --no-progress ARCHITECTURE.md` | last (after W1–W3 and OD-8/OD-10) |

Shared contracts between workers: W7 and W8 agree on the env name `ARIAD_TEST_REQUIRE_DATABASE=1`. W9 copies final wording from W1 (fuzz oracles), W2 (Seatbelt behaviour) and W3 (Windows start-up, MSI scope). If OD-8 picks fail-closed for D4 (option B), D4 also edits `runner.rs` and `doctor.rs`; then move D4 into W1 or run it after W1.

### 2.3 OpenAPI 408/429 staleness and the pre-squash SDK

Neither is one of these items; both are D0 and are specified elsewhere. Current state, re-checked:

- **408/429:** `crates/ariad-server/src/api/blobs.rs:60-61,122-123` annotates 408 and 429 and is byte-identical to dev. The hardening spec has no `"408"`; its blob operations list only 200/403/404/500 and 200/403/422/500 (`schemas/openapi.v1.json:72-211`). Dev's spec has them at `:132,142,218,238`. The frozen snapshot (hardening only) is just as stale, so `openapi_drift` fails today.
- **Pre-squash SDK:** `8526c5f` is an ancestor of hardening; `504a212` (Biome fix) and `9a9f1ef` (regenerated spec) are not. `packages/sdk/test/events.test.ts` differs from dev in the two formatting hunks, and `generator.lock.json` carries `spec_sha256` `0cafcc24…` against dev's `ffc79268…`.
- **Dependencies on D0:** W4's D15 gate is meaningful only once the regenerated spec and lock are in. W5's D16 makes the snapshot refresh mandatory, so on today's branch it would fail on the 408/429 gap; land it after D0 and refresh in the same commit. W5's D25c changes the spec again (regenerate, refresh, relock).

---

## 3. Per-item specs, by worker

### W1 host and fuzz

#### D7: the host doctor uses Linux wording on macOS (Medium)

**Current behaviour.**
- The host `sandbox_check` (`crates/ariad-host/src/commands/doctor.rs:471-507`) always says "no Landlock, no seccomp" and hints "use a kernel with Landlock enabled…".
- The desktop calls the host directly (`apps/desktop/src-tauri/src/backend.rs:376`) and shows `message` and `hint`, so macOS Diagnostics shows Linux text.
- Only the CLI rewrites the message on macOS (`crates/ariad-cli/src/render.rs:351-393`) and adds `seatbelt_enforced` to its JSON (`render.rs:305-312`). In off mode that rewrite (`render.rs:376-381`) drops "(ASHIFT_SANDBOX=off); engines run without isolation" (`doctor.rs:460`).
- Stale strings: `doctor.rs:469` "required mode needs Linux", `runner.rs:424` "the engine sandbox is only available on Linux", `runner.rs:405` "…launcher on Linux".

**Change.** The host owns the wording; delete the CLI rewrite. The host produces exactly what the CLI prints today, and sandbox lines are stripped from the insta snapshots (`crates/ariad-cli/tests/commands_cli.rs:402,427`), so there is no snapshot churn.

```diff
--- crates/ariad-host/src/commands/doctor.rs
 pub struct DoctorSandbox {
     pub mode: Option<SandboxMode>,
     #[serde(flatten)]
     pub support: SandboxSupport,
+    /// The selected mode enables Seatbelt for engine launches and its probe succeeded (macOS only).
+    #[cfg(target_os = "macos")]
+    #[serde(default)]
+    pub seatbelt_enforced: bool,
 }
@@ fn sandbox_check
-            "required mode needs Linux",
+            "required mode needs Linux or macOS",
         ),
         Ok(mode) => {
             let gaps = support.gaps();
-            let mut features = Vec::new();
-            … (Landlock/seccomp feature list, lines 473-496) …
-            let mut message = format!("engine sandbox: {mode}; {}", features.join(", "));
+            let (features, hint) = sandbox_features(&support);
+            let mut message = format!("engine sandbox: {mode}; {features}");
             if !gaps.is_empty() { … unchanged … }
-            check(gaps.is_empty(), *mode == SandboxMode::Required, message,
-                "use a kernel with Landlock enabled (Linux 5.13 or newer with landlock in the LSM list)")
+            check(gaps.is_empty(), *mode == SandboxMode::Required, message, hint)
         }
     };
+    #[cfg(target_os = "macos")]
+    let seatbelt_enforced = matches!(mode, Ok(SandboxMode::BestEffort | SandboxMode::Required))
+        && support.seatbelt_usable;
     (
         check,
-        DoctorSandbox { mode: mode.ok(), support },
+        DoctorSandbox {
+            mode: mode.ok(),
+            support,
+            #[cfg(target_os = "macos")]
+            seatbelt_enforced,
+        },
     )
 }
+
+/// The isolation features `doctor` names on this platform, and the remedy when some are missing.
+#[cfg(target_os = "macos")]
+fn sandbox_features(support: &SandboxSupport) -> (String, &'static str) {
+    let available = if support.seatbelt_available { "available" } else { "unavailable" };
+    let enforcement = if support.seatbelt_usable { "enforced for engine launches" } else { "not enforced" };
+    (
+        format!("Seatbelt {available} (/usr/bin/sandbox-exec); {enforcement}"),
+        "use a macOS system providing sandbox-exec and permitting Seatbelt profiles; required mode refuses unavailable confinement",
+    )
+}
+
+#[cfg(not(target_os = "macos"))]
+fn sandbox_features(support: &SandboxSupport) -> (String, &'static str) {
+    let mut features = Vec::new();
+    … move lines 474-496 here verbatim …
+    (
+        features.join(", "),
+        "use a kernel with Landlock enabled (Linux 5.13 or newer with landlock in the LSM list)",
+    )
+}
```

The `Ok(mode)` arm runs only for best-effort and required (`Off` matches earlier), so `enforcement` depends on `seatbelt_usable` alone.

- `crates/ariad-cli/src/render.rs`: delete `seatbelt_enforced` (`:351-358`) and `seatbelt_doctor_report` (`:360-393`); remove the two `#[cfg(target_os = "macos")]` blocks at `:305-312` and `:322-323` so `render_doctor_json` serializes the report directly.
- `crates/ariad-host/src/runner.rs:405`: "…or through its sandbox launcher on Linux or macOS."; `:424`: `"the engine sandbox is only available on Linux and macOS"`.

**Tests.**

1. Host unit test, appended to the `mod tests` in `doctor.rs` (`:685`, which already imports `doctor` and `CancellationToken`). Verify `doctor`'s signature when pasting.

```rust
    #[cfg(target_os = "macos")]
    #[test]
    fn host_doctor_names_seatbelt_on_macos() {
        if std::env::var_os(crate::sandbox::MODE_ENV).is_some() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let report =
            doctor(&temp.path().join("missing-engine"), CancellationToken::new()).expect("doctor run");
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "sandbox")
            .expect("sandbox check");
        assert!(check.message.contains("Seatbelt"), "{}", check.message);
        assert!(!check.message.contains("Landlock"), "{}", check.message);
        assert!(!check.hint.contains("Linux"), "{}", check.hint);
        let json = serde_json::to_value(&report).unwrap();
        assert!(json["sandbox"]["seatbelt_enforced"].is_boolean(), "{json}");
    }
```

2. CLI, new file `crates/ariad-cli/tests/doctor_sandbox_cli.rs` (new file so D1's `commands_cli.rs` and `sandbox_cli.rs` stay untouched):

```rust
//! The doctor sandbox check as CLI users see it on each platform.

use std::process::Command;

use serde_json::Value;

fn doctor_json(mode: &str) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .env("ASHIFT_SANDBOX", mode)
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor --json");
    serde_json::from_slice(&output.stdout).expect("doctor JSON")
}

fn sandbox_message(report: &Value) -> &str {
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["id"] == "sandbox")
        .and_then(|check| check["message"].as_str())
        .expect("sandbox check message")
}

#[test]
fn off_mode_says_engines_run_without_isolation() {
    let report = doctor_json("off");
    assert_eq!(
        sandbox_message(&report),
        "engine sandbox: off (ASHIFT_SANDBOX=off); engines run without isolation"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(report["sandbox"]["seatbelt_enforced"], false);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_best_effort_reports_the_seatbelt_fields() {
    let report = doctor_json("best-effort");
    for key in ["seatbelt_available", "seatbelt_usable", "seatbelt_enforced"] {
        assert!(report["sandbox"][key].is_boolean(), "sandbox.{key}");
    }
    assert!(
        sandbox_message(&report).starts_with("engine sandbox: best-effort; Seatbelt "),
        "{}",
        sandbox_message(&report)
    );
}
```

3. Windows wording, in `runner.rs` `mod tests`, directly after `a_standard_workspace_is_writable_as_one_root`:

```rust
    #[cfg(windows)]
    #[test]
    fn required_mode_names_the_platforms_that_have_a_sandbox() {
        use super::{RunError, prepare_launch};
        use crate::sandbox::SandboxMode;

        let mut command = EngineCommand::builtin(&std::env::current_exe().unwrap(), "pandoc");
        command.sandbox = Some(SandboxMode::Required);
        let Err(RunError::SandboxUnavailable(message)) = prepare_launch(&command, &[]) else {
            panic!("required mode must refuse on Windows");
        };
        assert!(message.contains("Linux and macOS"), "{message}");
    }
```

Before the fix, test 1 sees "engine sandbox: best-effort; no Landlock, no seccomp", test 2's off-mode case loses its suffix on macOS, and test 3 sees "only available on Linux".

**Gate:** `cargo nextest run -p ariad-host commands::doctor`; `cargo nextest run -p ariad-cli --test doctor_sandbox_cli`; `just test`; `just ci`. The CI `verify` matrix (`ci.yml:154`) covers ubuntu, macOS and Windows.

**Commit:** `fix(host): report the macOS sandbox in the host doctor check`

**Risks:** the macOS JSON `hint` becomes non-empty even when the check passes (as on Linux; renderers show it only on failure). The desktop maps only `message` and `hint` (`backend.rs:375-389`), so its TS bindings do not change. Distribution edits other parts of `doctor.rs` (H-8): resolve by hand at F0.

#### D10: the `engine_protocol` fuzz target skips the host's event rules (Medium)

**Current behaviour.** The harness only calls `serde_json::from_slice::<Event>` (`fuzz/fuzz_targets/engine_protocol.rs:47-56`). The real boundary is the private `parse_event` (`crates/ariad-host/src/runner.rs:825-862`, called at `:692`): it enforces `artifact_is_confined` (`:864-875`), refuses capabilities when an output dir is set (`:831-835`) and checks that `ok` and `error` agree (`:853-860`). The fuzzer never reaches it. Precedent for a hidden public API in this crate: `#[doc(hidden)] pub fn convert_custom` (`convert.rs:418`).

**Change.** Make `parse_event` itself hidden-public; no wrapper. Path: `ariad_host::runner::parse_event` (`pub mod runner` at `lib.rs:15`).

```diff
--- crates/ariad-host/src/runner.rs
-/// Parses one stdout line and applies the per-event protocol rules.
-fn parse_event(
+/// Parses one stdout line and applies the per-event protocol rules: an artifact must name a path
+/// strictly inside `output_dir`, capabilities answer only a request without one, and a result's
+/// `ok` and `error` must agree. Accepted artifacts are appended to `artifacts`.
+///
+/// Public only so the fuzz crate can drive this compromised-engine boundary; not a stable API.
+#[doc(hidden)]
+pub fn parse_event(
     line: &[u8],
     output_dir: Option<&str>,
     artifacts: &mut Vec<Artifact>,
 ) -> Result<Event, RunError> {
```

New harness body for `fuzz/fuzz_targets/engine_protocol.rs`. Keep the header comment (update it to "…through `runner::parse_event`, the host's per-event rules") and `assert_round_trip`; replace imports and the host-side loop:

```rust
use std::{io::Cursor, path::Path};

use ariad_core::protocol::{Event, Message, Request};
use ariad_host::{engines::common::read_request, runner::parse_event};
use libfuzzer_sys::fuzz_target;
use serde::{Serialize, de::DeserializeOwned};

const MAX_INPUT_BYTES: usize = 64 * 1024;
/// The `output.dir` of the convert request in the seeds. Fuzzing runs on Linux only.
const OUTPUT_DIR: &str = "/work/out";

// assert_round_trip unchanged

/// An artifact the host accepted lies strictly below `output.dir` once `.` and empty segments
/// are dropped, and never contains `..`. This is checked on the string, independently of
/// `Path::components`.
fn assert_confined(path: &str) {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    assert!(path.starts_with('/'), "relative artifact {path:?} accepted");
    assert!(
        segments.len() > 2 && segments[..2] == ["work", "out"],
        "artifact {path:?} outside {OUTPUT_DIR} accepted"
    );
    assert!(!segments.contains(&".."), "artifact {path:?} with `..` accepted");
}

/// An accepted result never reports success together with an error, or failure without one.
fn assert_consistent(event: &Event) {
    if let Event::Result { ok, error, .. } = event {
        assert_eq!(*ok, error.is_none(), "result with disagreeing ok and error accepted");
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    // Engine side: the first line is the request.
    if let Ok(request) = read_request(&mut Cursor::new(data)) {
        assert_round_trip(&request);
        let first_line = data.split(|byte| *byte == b'\n').next().unwrap_or_default();
        let direct: Request =
            serde_json::from_slice(first_line).expect("read_request agrees with a direct decode");
        assert_eq!(direct, request);
    }

    // Host side: every stdout line goes through the runner's own rules, once for a convert job
    // (with `output.dir`) and once for a describe job (without one).
    let mut convert_artifacts = Vec::new();
    let mut describe_artifacts = Vec::new();
    for line in data.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let decoded = serde_json::from_slice::<Event>(line).ok();
        if let Some(event) = &decoded {
            assert_round_trip(event);
        }
        if let Ok(message) = serde_json::from_slice::<Message>(line) {
            assert_round_trip(&message);
        }

        let before = convert_artifacts.len();
        match parse_event(line, Some(OUTPUT_DIR), &mut convert_artifacts) {
            Ok(event) => {
                assert_eq!(decoded.as_ref(), Some(&event), "the runner accepted a line serde refuses");
                assert_consistent(&event);
                assert!(
                    !matches!(event, Event::Capabilities { .. }),
                    "capabilities accepted for a convert request"
                );
                if let Event::Artifact { path, .. } = &event {
                    assert_confined(path);
                    assert_eq!(convert_artifacts.len(), before + 1);
                    assert_eq!(convert_artifacts[before].path, Path::new(path));
                } else {
                    assert_eq!(convert_artifacts.len(), before, "a non-artifact event was recorded");
                }
            }
            Err(_) => assert_eq!(convert_artifacts.len(), before, "a refused line was recorded"),
        }

        if let Ok(event) = parse_event(line, None, &mut describe_artifacts) {
            assert_eq!(decoded.as_ref(), Some(&event));
            assert_consistent(&event);
            assert!(!matches!(event, Event::Artifact { .. }), "artifact accepted without output.dir");
        }
        assert!(describe_artifacts.is_empty());
    }
});
```

New seed `fuzz/seeds/engine_protocol/artifact-escapes.jsonl`:

```
{"type":"artifact","path":"/work/out/../etc/passwd","format":"docx"}
{"type":"artifact","path":"/work/outside/a.docx","format":"docx"}
{"type":"artifact","path":"/work/out","format":"docx"}
{"type":"artifact","path":"/work/out/./a.docx","format":"docx"}
{"type":"result","ok":true,"error":{"code":"limit_exceeded","message":"x"}}
```

**Regression test**, appended to `crates/ariad-host/tests/runner.rs` (it already imports `Event`, `Path`, `PathBuf` and `runner::{self, Artifact, RunError, …}`; add any missing name). It does not compile before the change and passes after. Note `output_dir.as_str()`: `parse_event` takes `Option<&str>`, and `Some(&String)` does not coerce.

```rust
#[test]
fn engine_event_lines_follow_the_protocol_rules() {
    let output = std::env::temp_dir().join("ariad-event-rules").join("out");
    let output_dir = output.display().to_string();
    let artifact_line = |path: &Path| {
        serde_json::json!({"type": "artifact", "path": path, "format": "docx"}).to_string()
    };
    let mut artifacts = Vec::new();

    let inside = output.join("document.docx");
    let event = runner::parse_event(
        artifact_line(&inside).as_bytes(),
        Some(output_dir.as_str()),
        &mut artifacts,
    )
    .unwrap();
    assert!(matches!(event, Event::Artifact { .. }));
    assert_eq!(
        artifacts,
        vec![Artifact {
            path: inside,
            format: "docx".to_owned()
        }]
    );

    for escaping in [
        output.join("..").join("escaped.docx"),
        output.with_file_name("out-sibling").join("x.docx"),
        output.clone(),
        PathBuf::from("relative.docx"),
    ] {
        let refused = runner::parse_event(
            artifact_line(&escaping).as_bytes(),
            Some(output_dir.as_str()),
            &mut artifacts,
        );
        assert!(
            matches!(refused, Err(RunError::ProtocolViolation(_))),
            "{escaping:?} was accepted"
        );
    }
    assert_eq!(artifacts.len(), 1, "a refused artifact was recorded");

    let without_output =
        runner::parse_event(artifact_line(&output.join("a.docx")).as_bytes(), None, &mut artifacts);
    assert!(matches!(without_output, Err(RunError::ProtocolViolation(_))));

    let capabilities = br#"{"type":"capabilities","engine":"probe","version":"0.1.0","tool":{"name":"probe","status":"found"},"license":"MIT","routes":[],"enforces_memory_limit":false}"#;
    assert!(runner::parse_event(capabilities, None, &mut artifacts).is_ok());
    assert!(matches!(
        runner::parse_event(capabilities, Some(output_dir.as_str()), &mut artifacts),
        Err(RunError::ProtocolViolation(_))
    ));

    for disagreeing in [
        br#"{"type":"result","ok":true,"error":{"code":"limit_exceeded","message":"x"}}"#.as_slice(),
        br#"{"type":"result","ok":false}"#.as_slice(),
    ] {
        assert!(matches!(
            runner::parse_event(disagreeing, Some(output_dir.as_str()), &mut artifacts),
            Err(RunError::ProtocolViolation(_))
        ));
    }
}
```

Verified: `Artifact` derives `Clone, Debug, Eq, PartialEq` (`runner.rs:47`); `ToolAvailability` is snake_case (`found`); the capabilities fields match `protocol.rs:132-144`.

**Gate:** `cargo nextest run -p ariad-host --features test-probe --test runner`; `cargo clippy -p ariad-host --all-targets --features test-probe`; `cargo fmt --manifest-path fuzz/Cargo.toml --check`; `just fuzz engine_protocol 60` (Linux).

**Commit:** `test(fuzz): drive engine event lines through the host protocol rules`

**Risks:** `parse_event` becomes reachable public API, mitigated by `#[doc(hidden)]` as with `convert_custom`. The oracle tolerates `.` and empty segments, which `Path::components` also normalises, so no false positive is expected. D7 edits other hunks of `runner.rs` in the same worker; I14 rebases later.

#### D11: the `pack_archive` fuzz target never runs the install writer (Medium)

**Current behaviour.** `fuzz/fuzz_targets/pack_archive.rs:37-55` calls only `packs::scan` and asserts counts. The writer (`crates/ariad-host/src/packs/extract.rs:654-840`) and `check_links` (`:544-571`) are reachable only through `pub(crate) fn extract` (`:146`), whose argument types are crate-private. `fuzz/Cargo.toml` has no `tempfile`; existing targets use `std::env::temp_dir()` plus the pid.

**Change** in `extract.rs`, after `scan`:

```rust
/// Extracts an archive into the existing directory `root` with the install-time writer, entry
/// rules and link check, within `max_bytes` and `max_entries`. Public only so the fuzz crate can
/// drive the writing path; installs go through `install`, which also verifies hashes.
#[doc(hidden)]
pub fn extract_for_fuzzing(
    source: &Path,
    unpack: Unpack,
    component: &str,
    root: &Path,
    max_bytes: u64,
    max_entries: u64,
) -> Result<Stats, PackError> {
    let cancel = CancellationToken::new();
    let mut sink = Writer {
        root,
        component,
        files: Vec::new(),
        file_index: HashMap::new(),
        links: Vec::new(),
        cancel: &cancel,
    };
    let stats = walk(
        source,
        unpack,
        component,
        ".",
        false,
        Bounds { max_bytes, max_entries },
        &mut sink,
        &cancel,
    )?;
    check_links(root, &sink.links)?;
    Ok(stats)
}
```

Check the `Writer` field list and the `walk` parameter order (`:212-222`) against the file when pasting. In `packs/mod.rs:27` keep `pub use extract::{Stats, scan};` and add:

```rust
#[doc(hidden)]
pub use extract::extract_for_fuzzing;
```

New `fuzz/fuzz_targets/pack_archive.rs` (`Unpack` is `Copy`, verified at `packs/manifest.rs:133`):

```rust
#![no_main]

//! Engine pack component archives. `packs::scan` walks a component with the extraction entry
//! rules and writes nothing; `packs::extract_for_fuzzing` runs the install-time writer and link
//! check into a scratch root. Nothing may be written beside the root, every link left after a
//! successful extraction resolves inside it, and `scan` declares exactly what extraction
//! writes, because generated manifests rely on that. Every input is tried as a `tar.zst`, a
//! `tar.gz`, a `zip` and a plain file.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use ariad_host::packs::{Unpack, extract_for_fuzzing, scan};
use libfuzzer_sys::fuzz_target;

/// Small enough that a zstd RLE bomb stays within a few hundred MiB of decoded stream.
const MAX_INPUT_BYTES: usize = 16 * 1024;
/// The fixed size of one ZIP central directory record without its variable fields.
const ZIP_CENTRAL_RECORD_BYTES: usize = 46;
/// Extraction writes to disk, so decoded bombs stop early.
const MAX_EXTRACT_BYTES: u64 = 1 << 20;
const MAX_EXTRACT_ENTRIES: u64 = 1024;

/// Per-process scratch: the input file, the install root and an `outside` sentinel directory.
struct Scratch {
    parent: PathBuf,
    input: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}

fn scratch() -> &'static Scratch {
    static SCRATCH: OnceLock<Scratch> = OnceLock::new();
    SCRATCH.get_or_init(|| {
        let parent =
            std::env::temp_dir().join(format!("ariad-fuzz-pack-archive-{}", std::process::id()));
        let outside = parent.join("outside");
        fs::create_dir_all(&outside).expect("create fuzz scratch directory");
        Scratch {
            input: parent.join("component.bin"),
            root: parent.join("root"),
            outside,
            parent,
        }
    })
}

fn assert_links_inside(dir: &Path, canonical_root: &Path) {
    for entry in fs::read_dir(dir).expect("list extracted directory") {
        let path = entry.expect("extracted entry").path();
        let metadata = fs::symlink_metadata(&path).expect("inspect extracted entry");
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).expect("read extracted link");
            assert!(target.is_relative(), "absolute link {path:?} -> {target:?} kept");
            let real = fs::canonicalize(&path)
                .unwrap_or_else(|_| panic!("dangling link {path:?} -> {target:?} kept"));
            assert!(real.starts_with(canonical_root), "link {path:?} resolves to {real:?}");
        } else if metadata.is_dir() {
            assert_links_inside(&path, canonical_root);
        }
    }
}

fn check_extract(scratch: &Scratch, unpack: Unpack) {
    let _ = fs::remove_dir_all(&scratch.root);
    fs::create_dir(&scratch.root).expect("create install root");
    let result = extract_for_fuzzing(
        &scratch.input,
        unpack,
        "payload",
        &scratch.root,
        MAX_EXTRACT_BYTES,
        MAX_EXTRACT_ENTRIES,
    );

    assert!(
        fs::read_dir(&scratch.outside).expect("list sentinel").next().is_none(),
        "extraction wrote into the directory beside the root"
    );
    let mut siblings: Vec<String> = fs::read_dir(&scratch.parent)
        .expect("list scratch directory")
        .map(|entry| entry.expect("scratch entry").file_name().to_string_lossy().into_owned())
        .collect();
    siblings.sort();
    assert_eq!(siblings, ["component.bin", "outside", "root"], "extraction created a file beside the root");

    if let Ok(written) = result {
        let canonical_root = fs::canonicalize(&scratch.root).expect("resolve install root");
        assert_links_inside(&scratch.root, &canonical_root);
        assert_eq!(
            scan(&scratch.input, unpack, "payload").ok(),
            Some(written),
            "scan declares different counts than extraction writes"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let scratch = scratch();
    fs::write(&scratch.input, data).expect("write fuzz input");

    // A single file is one entry holding exactly the input bytes.
    let stats = scan(&scratch.input, Unpack::File, "payload.bin").expect("a plain file always scans");
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.bytes, data.len() as u64);

    // Every ZIP entry has its own central directory record inside the input.
    if let Ok(stats) = scan(&scratch.input, Unpack::Zip, "payload") {
        assert!(
            stats.entries as usize <= data.len() / ZIP_CENTRAL_RECORD_BYTES,
            "zip scan counted {} entries in {} bytes",
            stats.entries,
            data.len()
        );
    }
    let _ = scan(&scratch.input, Unpack::TarGz, "payload");
    let _ = scan(&scratch.input, Unpack::TarZst, "payload");

    for unpack in [Unpack::Zip, Unpack::TarGz, Unpack::TarZst] {
        check_extract(scratch, unpack);
    }
});
```

**Regression test**, appended to `crates/ariad-host/tests/packs.rs`. It uses the existing `support::{Entry, gzip, tar_bytes, temp}` helpers (`tests/support/mod.rs:212-262,495`); `tar_bytes` writes raw names, so `..` entries are allowed. Add `Entry`, `gzip`, `tar_bytes` to the existing `use support::{…}` if missing (they are already imported) and use `packs::Stats`, `packs::Unpack`. It does not compile before the change.

```rust
#[cfg(unix)]
#[test]
fn the_install_writer_keeps_every_entry_and_link_inside_the_root() {
    let dir = support::temp();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    let archive = dir.path().join("component.tar.gz");
    let extract = |entries: &[Entry]| {
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(&archive, gzip(&tar_bytes(entries))).unwrap();
        let result =
            packs::extract_for_fuzzing(&archive, packs::Unpack::TarGz, "runtime", &root, 1 << 20, 64);
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0, "an entry left the root");
        result
    };

    let stats = extract(&[
        Entry::Dir("bin/"),
        Entry::File("bin/engine", vec![1; 10], 0o755),
        Entry::Symlink("bin/alias", "engine"),
    ])
    .unwrap();
    assert_eq!(stats, packs::Stats { bytes: 10, entries: 3 });
    assert_eq!(fs::read(root.join("bin/alias")).unwrap(), vec![1; 10]);

    for entries in [
        vec![Entry::File("../outside/escaped", b"x".to_vec(), 0o644)],
        vec![
            Entry::Symlink("up", "../outside"),
            Entry::File("up/escaped", b"x".to_vec(), 0o644),
        ],
        vec![
            Entry::Dir("deep/"),
            Entry::Symlink("deep/t", ".."),
            Entry::Symlink("deep/l", "t/.."),
            Entry::File("deep/l/outside/escaped", b"x".to_vec(), 0o644),
        ],
        vec![
            Entry::Dir("deep/"),
            Entry::Symlink("deep/t", ".."),
            Entry::Symlink("deep/l", "t/.."),
        ],
    ] {
        let error = extract(&entries).unwrap_err();
        assert!(matches!(error, PackError::UnsafeArchive { .. }), "{error}");
    }
}
```

The install-level `unsafe_archive` tests (`packs.rs:413-475`) already cover these entries through `install`; this test pins the new hidden entry point to the same rules. Confirm the variant name `PackError::UnsafeArchive` against those tests.

**Gate:** `cargo nextest run -p ariad-host --features test-probe --test packs`; `cargo clippy -p ariad-host --all-targets --features test-probe`; `just fuzz pack_archive 60`.

**Commit:** `test(fuzz): fuzz pack extraction through the install writer`

**Risks:** three extractions per exec lower throughput (the 1 MiB cap bounds it). The `scan == extract` oracle may fire on a real scanner/writer mismatch, which would be a genuine manifest bug. The hidden function skips hash verification; its name and doc say so.

#### D12: the `asset_resolve` oracle fires on reader-embedded `data:` images (Medium)

**Current behaviour.** `check` asserts PNG magic on every asset (`fuzz/fuzz_targets/asset_resolve.rs:115-127`, message "asset {id} is not the sniffed image" at `:125`). The Markdown reader embeds data-URI images before `resolve` runs (`reader/markdown.rs:372-382` → `common::decode_data_uri_image`, `reader/common.rs:47-101`, which accepts GIF, JPEG and WEBP). `![](data:;base64,R0lGODlh)` decodes to `GIF89a`, so the oracle panics.

**Change:**

```diff
--- fuzz/fuzz_targets/asset_resolve.rs
 use std::{
+    collections::BTreeSet,
     fs,
@@
-fn check(document: &Document, base: &Path) {
+/// Holds the assets `resolve` embedded to the base-directory rules. Assets the Markdown reader
+/// already embedded from `data:` URIs are document bytes, not files, so they are exempt.
+fn check(document: &Document, base: &Path, from_reader: &BTreeSet<String>) {
     let secret = secret();
-    for (id, asset) in &document.assets {
+    for (id, asset) in document.assets.iter().filter(|(id, _)| !from_reader.contains(*id)) {
@@
     let mut document = output.document;
+    // A data URI that spells the secret itself would hide an escape behind the same asset id.
+    if document.assets.values().any(|asset| asset.bytes == secret()) {
+        return;
+    }
+    let from_reader: BTreeSet<String> = document.assets.keys().cloned().collect();
     resolve(&mut document, base, &limits).expect("the base directory exists");
-    check(&document, base);
+    assert!(
+        from_reader.iter().all(|id| document.assets.contains_key(id)),
+        "resolve dropped an asset the reader embedded"
+    );
+    check(&document, base, &from_reader);
 });
```

**Regression seed** `fuzz/seeds/asset_resolve/data-uri-images.md`:

```
![](data:;base64,R0lGODlh)
![local](img.png)
```

Before: `cargo fuzz run -s none asset_resolve fuzz/seeds/asset_resolve/data-uri-images.md` panics with "asset … is not the sniffed image". After: it passes. CI replays `fuzz/seeds/<target>` on every run, so the seed is the permanent regression check. No ordinary `#[test]` can prove this, because the defect is in the harness oracle.

**Gate:** the command above, then `just fuzz asset_resolve 60`.

**Commit:** `test(fuzz): exempt reader-embedded data URIs from the asset resolve oracle`

**Risks:** minimal.

#### D13 a–d: fuzz harness coverage gaps (Low)

**(a) `pack_manifest` never reaches a verified signature.** Both signed seeds carry zeroed signatures, so `check_signature`'s `Ok` arm (`fuzz/fuzz_targets/pack_manifest.rs:76-77`) and `check_trusted_comment` (`packs/signature.rs:138-160`) never run. The module comment (`:3-4`) says "verify … then parse", but the harness parses first and falls back to `fuzz`/`0.0.0` (`:102,113-115`). Signing needs the `minisign` crate: it is only a dev-dependency of ariad-host, and `fuzz/Cargo.lock` lacks it (the original review was wrong to call it a native dependency).

- **Option A (needs owner OK: new fuzz-only, never-distributed dependency).** Add `minisign = "0.10.0"` to `fuzz/Cargo.toml` `[dependencies]` (same pin as root `Cargo.toml:43`), regenerate `fuzz/Cargo.lock` with `cargo fuzz build pack_manifest`, correct the module comment ("parses strictly, then verifies with the parsed name and version, falling back to `fuzz 0.0.0`"), and add:

```rust
use std::{io::Cursor, sync::OnceLock};

use minisign::KeyPair;

/// A per-process throwaway key. Signing inside the harness lets the verified arm and the
/// trusted-comment check run on fuzzed manifests and comments; no mutation can forge a
/// signature for the compiled keys.
fn throwaway() -> &'static (KeyPair, TrustedKeys) {
    static KEY: OnceLock<(KeyPair, TrustedKeys)> = OnceLock::new();
    KEY.get_or_init(|| {
        let pair = KeyPair::generate_unencrypted_keypair().expect("generate throwaway key");
        let public = pair.pk.to_box().expect("public key box").into_string();
        let keys = TrustedKeys::new(vec![
            TrustedKey::parse("throwaway.pub", &public).expect("parse throwaway key"),
        ]);
        (pair, keys)
    })
}

fn sign(pair: &KeyPair, manifest: &[u8], comment: &str) -> Option<String> {
    minisign::sign(Some(&pair.pk), &pair.sk, Cursor::new(manifest), Some(comment), None)
        .ok()
        .map(|signature| signature.into_string())
}

/// The canonical comment verifies, a changed manifest does not, and a fuzz-chosen comment
/// verifies only if it carries both `pack:<name>` and `version:<version>`.
fn check_self_signed(manifest: &[u8], comment: &str, name: &str, version: &str) {
    let (pair, keys) = throwaway();
    let id = keys.ids().remove(0);
    if !name.chars().any(char::is_whitespace) && !version.chars().any(char::is_whitespace) {
        let canonical = sign(pair, manifest, &format!("pack:{name} version:{version}"))
            .expect("sign the manifest");
        assert_eq!(
            keys.verify(manifest, &canonical, name, version).ok(),
            Some(id),
            "a correctly signed manifest was refused"
        );
        let mut tampered = manifest.to_vec();
        tampered.push(b'\n');
        assert!(
            matches!(
                keys.verify(&tampered, &canonical, name, version),
                Err(SignatureError::Invalid { .. })
            ),
            "a modified manifest verified"
        );
    }
    if comment.contains(['\r', '\n']) {
        return;
    }
    let Some(signature) = sign(pair, manifest, comment) else {
        return;
    };
    if keys.verify(manifest, &signature, name, version).is_ok() {
        let tokens: Vec<&str> = comment.split_whitespace().collect();
        assert!(
            tokens.contains(&format!("pack:{name}").as_str())
                && tokens.contains(&format!("version:{version}").as_str()),
            "comment {comment:?} verified for {name} {version}"
        );
    }
}
```

Call it after `(name, version)` is computed (after `:115`): `check_self_signed(manifest_bytes, signature_text.lines().next().unwrap_or_default(), name, version);`. If `OnceLock<(KeyPair, TrustedKeys)>` fails the `Sync` bound, use `thread_local!`.
- **Option B (no new dependency).** Commit a seed `<manifest>\0<minisig>\0<throwaway pub>` signed once with a throwaway key; the third input section already feeds a custom key (`:132-140`). It reaches the `Ok` arm only for that seed.

**(b) `docx_meta` entry-name comparison.** `read_zip_entry_names` decodes with `String::from_utf8_lossy` and ignores the UTF-8 flag (`crates/ariad-host/src/archive.rs:370`), while `stamp_into` rewrites through the `zip` writer (`crates/ariad-core/src/package_meta.rs:40-63`). Whether zip 9.0.5 re-encodes CP437 names is unverified, but the fix holds either way. In `stamp_case` (`fuzz/fuzz_targets/docx_meta.rs:125-128`):

```diff
-        if let (Some(before), Some(after)) = (names_before, entry_names(&path)) {
+        // The name reader decodes raw bytes as UTF-8 regardless of the ZIP UTF-8 flag, while the
+        // `zip` writer may re-encode flagless non-ASCII names, so only ASCII names are compared.
+        if let (Some(before), Some(after)) = (names_before, entry_names(&path))
+            && before.iter().all(|name| name.is_ascii())
+        {
             assert_eq!(before, after, "stamping changed the package entry names");
```

**(c) Clock-dependent timestamp.** `docx_meta.rs:152-155` calls `conversion_time()` (`SOURCE_DATE_EPOCH` or now; `crates/ariad-host/src/package_meta.rs:38-58`), so an unusable env value disables the target and crashes do not reproduce.

```diff
-    package_meta::{conversion_time, stamp_docx, stamp_epub},
+    package_meta::{stamp_docx, stamp_epub},
@@
+/// A fixed stamping time, so crashes reproduce and the environment cannot disable the target.
+const STAMP_TIME: &str = "2025-10-09T08:53:20Z";
@@
-    // SOURCE_DATE_EPOCH may be set by the caller; an unusable value has nothing to stamp.
-    let Ok(timestamp) = conversion_time() else {
-        return;
-    };
+    let timestamp = STAMP_TIME.parse().expect("the fixed stamping time parses");
```

If inference of `jiff::Timestamp` fails, add `jiff = "0.2.38"` (already at that version in `fuzz/Cargo.lock:1785`).

**(d) `uri_parsers` lacks independent properties.** `uri_parsers.rs:20-34` restates checks that `parse_file_uri` already makes (`confine.rs:728-806,844`). Add a round trip through the public `path_to_file_uri` (`confine.rs:928`) and an independent device-name list:

```diff
-use ariad_host::confine::{parse_file_uri, validate_windows_path_syntax};
+use ariad_host::confine::{parse_file_uri, path_to_file_uri, validate_windows_path_syntax};
@@
 const MAX_INPUT_BYTES: usize = 8 * 1024;
+/// Reserved Windows device names, listed independently of the validator.
+const DEVICES: [&str; 22] = [
+    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
+    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
+];
@@
         assert!(text.starts_with("file://"));
+        let uri = path_to_file_uri(&path);
+        assert_eq!(
+            parse_file_uri(&uri).ok().as_ref(),
+            Some(&path),
+            "{path:?} did not survive a file URI round trip through {uri:?}"
+        );
     }
 
     if validate_windows_path_syntax(&text).is_ok() {
         assert!(!text.contains('\0'), "NUL byte accepted in a Windows path");
+        let cleaned = text
+            .strip_prefix(r"\\?\UNC\")
+            .or_else(|| text.strip_prefix(r"\\?\"))
+            .unwrap_or(&text);
+        for (index, part) in cleaned.split(['/', '\\']).enumerate() {
+            let drive = index == 0
+                && part.len() == 2
+                && part.as_bytes()[0].is_ascii_alphabetic()
+                && part.ends_with(':');
+            assert!(drive || !part.contains(':'), "alternate data stream {part:?} accepted");
+            let stem = part.split('.').next().unwrap_or(part).trim();
+            assert!(
+                !DEVICES.iter().any(|device| device.eq_ignore_ascii_case(stem)),
+                "reserved device name {part:?} accepted"
+            );
+        }
     }
```

Check the type of `path` (`PathBuf`) and of `parse_file_uri`'s `Ok` value when pasting.

**Proving checks (coverage fixes; no `#[test]` fails before):** `just fuzz pack_manifest 60` (option A: the self-signed arm must not panic on existing seeds); `just fuzz docx_meta 60` and `SOURCE_DATE_EPOCH=garbage just fuzz docx_meta 10` (before: fuzzes nothing; after: runs); `just fuzz uri_parsers 60`; `cargo fmt --manifest-path fuzz/Cargo.toml --check`.

**Commits:** `test(fuzz): sign pack manifests with a throwaway key`; `test(fuzz): pin the docx_meta stamping time and compare only ASCII entry names`; `test(fuzz): add independent file URI and Windows path properties`.

**Risks:** (a) needs owner approval for the dependency. (d)'s round trip may expose a real `path_to_file_uri` bug and turn the Fuzz job red until fixed; that is the point.

---

### W2 Mac sandbox (on a Mac, together with D2, D3, D5)

#### D4: Seatbelt present but refused (Medium, OD-8)

**Current behaviour.**
- `macos::probe()` (`crates/ariad-host/src/sandbox/macos.rs:14-29`) sets `seatbelt_usable` from `check_profile` on a minimal profile; `gaps()` (`:36-37`) reports "Seatbelt is present but the system refused its enforcement probe".
- `prepare_launch` (`runner.rs:439-461`) turns that gap into `SandboxOutcome::Degraded` in best-effort mode, which tells the user the engine ran without Seatbelt.
- But `apply_and_exec` (`macos.rs:69-80`) checks only that `sandbox-exec` exists, runs `check_profile` again, fails and returns the error. The job ends as `SandboxLaunchFailed`, reported as `sandbox_unavailable`.
- `docs/cli.md:511-512` says a profile setup error fails the job; `:520-522` covers only an absent `sandbox-exec`. Net effect: `doctor` says best-effort with a gap, while every engine conversion fails.

**Change, option A (recommended default: degrade, mirroring Linux).** `linux.rs:88-96` skips seccomp when the kernel lacks it but fails when an offered feature cannot be applied, and `SandboxMode::BestEffort` is documented as "apply what the kernel supports and report what was skipped" (`sandbox.rs:45`). On the Mac: when the system refuses every profile, best-effort runs without Seatbelt and warns, and required refuses (already at `runner.rs:440-442`). When Seatbelt works but this engine's profile fails, the job still fails closed. Option A does not touch `runner.rs`.

```diff
--- crates/ariad-host/src/sandbox/macos.rs
 pub(super) fn probe() -> SandboxSupport {
     let available = Path::new(SANDBOX_EXEC).is_file();
-    let usable = available
-        && profile::Profile::new(&SandboxPolicy {
-            deny_network: true,
-            ..SandboxPolicy::default()
-        })
-        .and_then(|profile| check_profile(&profile))
-        .is_ok();
+    let usable = available && enforcement_probe_succeeds();
     SandboxSupport {
@@
 }
 
+/// Whether the system applies a minimal deny-by-default profile at all. A refusal means Seatbelt
+/// is unusable on this host (for example inside another sandbox), not that one engine's policy
+/// is wrong.
+fn enforcement_probe_succeeds() -> bool {
+    profile::Profile::new(&SandboxPolicy {
+        deny_network: true,
+        ..SandboxPolicy::default()
+    })
+    .and_then(|profile| check_profile(&profile))
+    .is_ok()
+}
+
 pub(super) fn gaps(support: &SandboxSupport) -> Vec<&'static str> {
@@
     } else if !support.seatbelt_usable {
-        vec!["Seatbelt is present but the system refused its enforcement probe"]
+        vec![
+            "Seatbelt is present but the system refused its enforcement probe, so filesystem and network access are not confined",
+        ]
@@
 fn check_profile(profile: &profile::Profile) -> Result<(), String> {
+    #[cfg(debug_assertions)]
+    if super::simulated_seatbelt_refusal() {
+        return Err("Seatbelt could not apply the profile: simulated refusal".to_owned());
+    }
     let output = profile
@@
-    let target = if available {
+    let confined = if available {
         let profile = match profile::Profile::new(&spec.policy) {
             Ok(profile) => profile,
             Err(error) => return error,
         };
-        if let Err(error) = check_profile(&profile) {
-            return error;
-        }
-        let mut command = profile.command();
-        command.arg(&spec.program).args(&spec.args);
-        command
-    } else {
+        match check_profile(&profile) {
+            Ok(()) => {
+                let mut command = profile.command();
+                command.arg(&spec.program).args(&spec.args);
+                Some(command)
+            }
+            // Like an absent seccomp on Linux, best-effort skips a Seatbelt the system refuses
+            // outright; the host probe saw the same refusal and reports the gap. A failure
+            // specific to this engine's profile still fails closed.
+            Err(_) if !spec.required && !enforcement_probe_succeeds() => None,
+            Err(error) => return error,
+        }
+    } else {
+        None
+    };
+    let target = confined.unwrap_or_else(|| {
         // The parent reports reduced isolation in best-effort mode, as on Linux.
         let mut command = Command::new(&spec.program);
         command.args(&spec.args);
         command
-    };
+    });
```

```diff
--- crates/ariad-host/src/sandbox.rs  (after line 200)
-/// Debug builds treat `ARIAD_TEST_SANDBOX_SUPPORT=none` as a kernel without Landlock or
-/// seccomp, so tests can exercise the degraded and fail-closed paths on any kernel.
+/// Debug builds treat `ARIAD_TEST_SANDBOX_SUPPORT=none` as a kernel without Landlock or
+/// seccomp (or a Mac without sandbox-exec), so tests can exercise the degraded and fail-closed
+/// paths on any kernel. On macOS, `seatbelt-refused` keeps sandbox-exec but has the system
+/// refuse every profile.
 #[cfg(debug_assertions)]
 fn simulated_without_support() -> bool {
     std::env::var_os("ARIAD_TEST_SANDBOX_SUPPORT").is_some_and(|value| value == "none")
 }
+
+#[cfg(all(debug_assertions, target_os = "macos"))]
+fn simulated_seatbelt_refusal() -> bool {
+    std::env::var_os("ARIAD_TEST_SANDBOX_SUPPORT").is_some_and(|value| value == "seatbelt-refused")
+}
```

`runner.rs:553-558` already forwards `ARIAD_TEST_SANDBOX_SUPPORT` to the launcher in debug builds. Check the field name `spec.required` against the launch spec in `macos.rs`.

`docs/cli.md`: replace `:511-512` with "When Seatbelt works but an engine's exact profile cannot be applied, the job fails with `sandbox_unavailable`; it never silently retries outside Seatbelt." Change `:520-522` to "When `sandbox-exec` is absent, or the system refuses every Seatbelt profile (for example inside another sandbox), `best-effort` runs with a `sandbox_unavailable` warning, while `required` refuses to start the engine (exit 5)."

**Option B (fail closed), if OD-8 picks it.** In `prepare_launch`, after `let gaps = support.gaps();`, add under `#[cfg(target_os = "macos")]`: `if support.seatbelt_available && !support.seatbelt_usable { return Err(RunError::SandboxUnavailable(format!("{}; set ASHIFT_SANDBOX=off to run engines without isolation", gaps.join("; ")))); }`; make the best-effort doctor check `required: true` in that state; document "best-effort refuses engines when the system refuses Seatbelt". This touches `runner.rs` and `doctor.rs`, so it must run in or after W1. The test below then expects `SandboxUnavailable` in best-effort.

**Regression test (option A)**, appended to `crates/ariad-host/tests/sandbox_macos.rs` (file is `#![cfg(target_os = "macos")]`; `engine`, `request`, `Workspace`, `RunError` under `debug_assertions`, `json!`, `BTreeMap`, `Command`, `CancellationToken` are already imported). The test re-runs itself in a child process so the process-wide probe sees the env var. Confirm that the probe's `sandbox-check` writes the `outside` target the same way the existing escape tests do.

```rust
#[test]
#[cfg(debug_assertions)]
fn refused_seatbelt_degrades_best_effort_and_refuses_required_mode() {
    const CHILD: &str = "ARIAD_TEST_MACOS_REFUSED_SEATBELT";
    if std::env::var_os(CHILD).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "refused_seatbelt_degrades_best_effort_and_refuses_required_mode",
            ])
            .env(CHILD, "1")
            .env("ARIAD_TEST_SANDBOX_SUPPORT", "seatbelt-refused")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    if !std::path::Path::new("/usr/bin/sandbox-exec").is_file() {
        eprintln!("skipping: /usr/bin/sandbox-exec is unavailable");
        return;
    }
    let support = sandbox::support();
    assert!(
        support.seatbelt_available && !support.seatbelt_usable,
        "the refusal is visible to doctor and the runner: {support:?}"
    );
    let workspace = Workspace::new().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("unconfined");
    let request = request(
        &workspace,
        BTreeMap::from([("outside".to_owned(), json!(target))]),
    );
    let mut command = engine(SandboxMode::BestEffort);
    command
        .env
        .push(("ARIAD_TEST_SANDBOX_SUPPORT".into(), "seatbelt-refused".into()));
    let outcome = runner::run_command(&command, &request, CancellationToken::new(), |_| {})
        .expect("best-effort runs without a Seatbelt the system refuses");
    assert!(
        matches!(outcome.sandbox, SandboxOutcome::Degraded(ref gap) if gap.contains("refused")),
        "{:?}",
        outcome.sandbox
    );
    assert!(target.exists());
    command.sandbox = Some(SandboxMode::Required);
    let error =
        runner::run_command(&command, &request, CancellationToken::new(), |_| {}).unwrap_err();
    assert!(matches!(error, RunError::SandboxUnavailable(_)), "{error:?}");
}
```

Before the fix the knob is ignored, the probe reports usable on a normal Mac and the `support` assertion fails.

**Gate (Mac):** `cargo nextest run -p ariad-host --features test-probe --test sandbox_macos`; `just test`; `just ci`; CI `verify (macos-26)`.

**Commit:** `fix(host): run best-effort engines without a Seatbelt the system refuses`

**Risks:** the parent and the launcher probe separately; if they disagree, the engine runs unconfined while the parent reports `Applied` (Linux has the same shape with seccomp). The extra probe runs only after a profile check has failed. W9 needs the final behaviour for ARCHITECTURE §11.3.

#### D26a: read-only Seatbelt grants cannot map libraries (Low)

**Current behaviour.** `macos_profile.rs:73-79` grants only `file-read*` for `SYSTEM_READ` and `policy.read_only`. Tesseract's prefix `lib`, `lib64`, `libexec` are pushed as read-only (`engines/sandbox.rs:117-126`). Landlock read access on Linux allows executable mmap, so a MacPorts or custom-prefix Tesseract cannot load its dylibs on a Mac.

**Change** (do not edit `engines/sandbox.rs`, which D5 owns in the same worker):

```diff
-        for path in SYSTEM_READ
-            .iter()
-            .map(Path::new)
-            .chain(policy.read_only.iter().map(AsRef::as_ref))
-        {
+        for path in SYSTEM_READ.iter().map(Path::new) {
             profile.grant(path, "file-read*", false)?;
         }
+        // Linux read grants allow executable mmap; engines' library directories need the same.
+        for path in &policy.read_only {
+            profile.grant(path, "file-read* file-map-executable", false)?;
+        }
```

**Test**, appended to the `mod tests` at `macos_profile.rs:121` (imports `super::*`, uses `tempfile`):

```rust
    #[test]
    fn read_only_engine_paths_can_map_libraries_but_not_execute_programs() {
        let root = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy {
            read_only: vec![root.path().to_path_buf()],
            ..SandboxPolicy::default()
        };
        let profile = Profile::new(&policy).unwrap();
        let key = format!("PATH_{}", profile.parameters.len() - 1);
        assert!(profile.source.contains(&format!(
            "(allow file-read* file-map-executable (subpath (param \"{key}\")))"
        )));
        assert!(!profile.source.contains(&format!(
            "process-exec (subpath (param \"{key}\"))"
        )));
    }
```

Fails before (the grant is `(allow file-read* (subpath …))`). Check that `parameters` and `source` are reachable from the test module.

**Gate:** `cargo nextest run -p ariad-host sandbox::macos` on a Mac; `just test`.

**Commit:** `fix(host): let Seatbelt map libraries from read-only engine paths`

**Risk:** every engine read-only path becomes mappable as executable, including `/opt` for LibreOffice until D5 narrows it. Land D5 in the same run.

---

### W3 desktop

#### D6: Finder opens show "Invalid desktop link" (Medium)

**Current behaviour.** The vendored plugin re-emits every macOS `RunEvent::Opened` URL as `deep-link://new-url` (`vendor/tauri-plugin-deep-link/src/lib.rs:698-709`). `apps/desktop/src/app.tsx:268-298` passes every URL to `commands.parse_link`, and `deep_link::parse` (`apps/desktop/src-tauri/src/deep_link.rs:25-34`) returns `Err("Invalid desktop link")` for any non-`ariadshift` scheme, shown as a notice (`app.tsx:286-287`). Files already arrive correctly through `lib.rs:58-61` and `file_open::parse_urls`.

**Do not edit `vendor/`:** `ARCHITECTURE.md:917` and `docs/desktop.md:241` record the vendored crate as 2.6.2 with exactly one change. `app.tsx:275` already skips a `null` link, so fix it in `parse`:

```diff
--- apps/desktop/src-tauri/src/deep_link.rs
 pub fn parse(value: &str) -> Result<Option<DeepLink>, String> {
     let u = url::Url::parse(value).map_err(|_| "Invalid desktop link")?;
+    // macOS delivers Finder opens to the deep-link channel as well; file_open handles them.
+    if u.scheme() == "file" {
+        return Ok(None);
+    }
     if u.scheme() != "ariadshift"
```

**Test**, in the `mod tests` of `deep_link.rs`:

```rust
    #[test]
    fn finder_file_urls_are_not_desktop_links() {
        for url in [
            "file:///Users/me/Report%20Q3.docx",
            "file://localhost/Users/me/notes.md",
        ] {
            assert!(parse(url).unwrap().is_none(), "{url}");
        }
        assert!(parse("https://open?route=md-html").is_err());
    }
```

**Gate:** `cargo nextest run -p ariad-desktop deep_link`; `just desktop-ci`.

**Commit:** `fix(desktop): ignore file URLs delivered through the deep-link channel`

**Risks:** none found; other schemes stay refused.

#### D26c: nothing runs the OS-integration drift test (Low)

**Current behaviour.** `apps/desktop/os-integration/generate.test.mjs` exists, but `test:tools` runs only `scripts/*.test.mjs` (`apps/desktop/package.json:11`); vitest includes only `test/**`. `desktop-ci` (`justfile:325-334`) and `desktop.yml:73` call `test:tools`, so neither runs it.

**Change:** `"test:tools": "node --test scripts/*.test.mjs os-integration/*.test.mjs"`. If the committed generated files are stale, regenerate with `node apps/desktop/os-integration/generate.mjs` in the same commit.

**Proof:** on a scratch tree, add an extension to `os-integration/formats.json` without regenerating. Before, `pnpm --filter @ariadshift/desktop test:tools` passes; after, it fails on "committed OS integration files match formats.json".

**Gate:** `pnpm --filter @ariadshift/desktop test:tools`; `just desktop-ci`.

**Commit:** `test(desktop): run the OS integration drift test with the desktop tool tests`

#### D26b: the per-machine MSI writes only HKCU keys (Low, OD-10)

**Current behaviour.** `os-integration.wxs:8-9`, generated by `generate.mjs:327,344`, writes HKCU only. Tauri's default WiX template is per-machine (not re-read: the bundler source is not local, and `tauri.windows.conf.json` sets no scope), so an admin-elevated install writes the verbs into the admin's hive.

**Default change (HKLM in the MSI fragment; NSIS stays per-user):** in `generate.mjs:327` and `:344` change `Root="HKCU"` to `Root="HKLM"`; generate a new component `Guid` at `:343` (the component's resources change); regenerate; in `docs/desktop.md:137` say the MSI is per machine and writes the same keys under `HKLM\Software\Classes\…` for every user. A per-user MSI instead needs a custom WiX template (larger).

**Test**, appended to `generate.test.mjs` (runnable after D26c):

```js
test("the per-machine MSI registers the verbs for every user", () => {
  const wix = files["src-tauri/windows/os-integration.wxs"];
  assert.ok(!wix.includes('Root="HKCU"'), "HKCU keys land in the installing admin's hive");
  assert.match(
    wix,
    /<RegistryValue Root="HKLM" Key="Software\\AriadShift\\OsIntegration"[^>]*KeyPath="yes"/,
  );
});
```

Check the name of the in-memory `files` map in the existing test file.

**Gate:** `pnpm --filter @ariadshift/desktop test:tools`; `just desktop-ci`; a manual MSI install from an admin account, checked from a standard account.

**Commit:** `fix(desktop): register MSI Explorer verbs machine-wide`

**Risk:** depends on OD-10.

#### D8: Windows multi-file cold start can open several windows (Medium, unmeasured)

**Current behaviour.** Explorer starts one process per file (`MultiSelectModel=Player` with `"%1"`, `os-integration.wxs:12-14`, `.nsh:14-15`). In tauri-plugin-single-instance 2.5.4 (`platform_impl/windows.rs:77-143`), a process that hits `ERROR_ALREADY_EXISTS` while `FindWindowW` returns null carries on as a full second app; the first instance creates its mutex (`:77`) before its message window (`:141`). `main.rs` has no pre-check, while `docs/desktop.md:137-138` promises one selection.

**Recommended change (std only, no new dependency).** A per-user lock file held from start-up until the single-instance plugin has set up; plugins run before the builder's `.setup`. `File::try_lock` is stable (Rust 1.89+). `tempfile` is already a normal dependency of `ariad-desktop` (`Cargo.toml:25`).

New `apps/desktop/src-tauri/src/launch_gate.rs`:

```rust
//! Serializes Windows cold starts until the first instance can take forwarded files.
//!
//! Explorer starts one process per selected file. The single-instance plugin forwards a
//! process's files only after the first instance has created its message window; a process
//! starting between the first instance's mutex and that window runs as a second app. Each
//! launch therefore holds this lock until the single-instance plugin has finished.

use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::{Duration, Instant},
};

#[cfg(windows)]
const WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(25);

/// Waits for earlier launches, then holds the lock until the returned file is dropped.
/// Returns `None`, leaving the launch unserialized, if the lock file is unusable or the wait
/// times out.
#[cfg(windows)]
pub(crate) fn acquire(identifier: &str) -> Option<File> {
    acquire_at(&std::env::temp_dir().join(format!("{identifier}.launch.lock")), WAIT)
}

fn acquire_at(path: &Path, wait: Duration) -> Option<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Some(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(POLL);
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::acquire_at;

    #[test]
    fn a_later_launch_waits_until_the_first_releases_the_gate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("launch.lock");
        let first = acquire_at(&path, Duration::ZERO).expect("first launch takes the gate");
        assert!(
            acquire_at(&path, Duration::from_millis(100)).is_none(),
            "a concurrent launch must wait"
        );
        let waiter = std::thread::spawn({
            let path = path.clone();
            move || acquire_at(&path, Duration::from_secs(5)).is_some()
        });
        std::thread::sleep(Duration::from_millis(100));
        drop(first);
        assert!(waiter.join().unwrap(), "the next launch proceeds once the first releases");
    }
}
```

```diff
--- apps/desktop/src-tauri/src/lib.rs
 pub mod file_open;
+#[cfg(any(windows, test))]
+mod launch_gate;
 pub mod types;
@@
 pub fn run() {
+    let context = tauri::generate_context!();
+    #[cfg(windows)]
+    let launch_gate = launch_gate::acquire(&context.config().identifier);
     macro_rules! register { … }
@@
-        .setup(|app| {
+        .setup(move |app| {
+            // Plugins are set up first, so the single-instance window exists: later launches
+            // can now forward their files instead of starting a second app.
+            #[cfg(windows)]
+            drop(launch_gate);
             let data = ariad_host::packs::data_dir()?;
@@
-        .build(tauri::generate_context!())
+        .build(context)
```

`__sandbox` and `__engine` return in `main.rs` before `run()`, so engine processes are never gated. On Linux and macOS, `acquire_at` is used only by the test; keep `just clippy` clean with the `cfg` shown (or `#[cfg_attr(not(windows), allow(dead_code))]`).

**Docs-only fallback if the owner declines the code:** add to `docs/desktop.md:138`: "On a cold start with many selected files, Windows may open more than one window; select the files again in the running app."

**Tests:** the unit test above (fails to compile before). The real check is manual on Windows 11 with the NSIS build: quit AriadShift; select 20 supported files; "Show more options" then "Convert with AriadShift"; expect exactly one window with all 20 files; repeat 5 times, then with the MSI build.

**Gate:** `cargo nextest run -p ariad-desktop launch_gate`; `just clippy`; `just desktop-ci`; the manual run.

**Commit:** `fix(desktop): serialize Windows cold starts until the first instance can take files`

**Risks:** launches are serialized (each forward is one `WM_COPYDATA`). A first instance stalled over 10 s before `.setup` falls back to today's behaviour. The lock file stays in `%TEMP%`. The race has never been reproduced. W9 adds a Decision Log row.

---

### W4 SDK

#### D14: `upload()` strands its pending upload on failure (Medium)

**Current behaviour.** `packages/sdk/src/client.ts:285-300` has no `try` around `transfer` or `completeUpload`, and the thrown error carries no upload id. Pending uploads count against the quota (`api/limits.rs:83`, `dto.rs:388`). The existing test `client.test.ts:183-194` asserts exactly two requests on a refused transfer and must change.

**Change:**

```ts
/** Longest wait for the best-effort cleanup of a failed upload. */
const CLEANUP_TIMEOUT_MS = 10_000;
// …
  /** Creates the upload, transfers the body and completes it. If the transfer or
   * completion fails, deletes the pending upload before rethrowing the error. */
  upload(body: UploadBody, options?: UploadOptions): Promise<Upload>;
// …
      if (!created.upload) {
        throw new Error(`upload ${created.id} has no presigned request (status ${created.status})`);
      }
-      await client.transfer(created.upload, body, uploadOptions.signal);
-      return client.completeUpload(created.id, uploadOptions.signal);
+      try {
+        await client.transfer(created.upload, body, uploadOptions.signal);
+        return await client.completeUpload(created.id, uploadOptions.signal);
+      } catch (error) {
+        // The caller never learns the id, so free the pending upload's quota slot
+        // here. Its signal may already be aborted, so cleanup gets its own.
+        await client
+          .deleteUpload(created.id, AbortSignal.timeout(CLEANUP_TIMEOUT_MS))
+          .catch(() => undefined);
+        throw error;
+      }
```

**Tests**, in `client.test.ts` inside `describe("upload")` (helpers `mockFetch`, `json`, `apiError`, `uploadBody`, `BASE_URL`, `UPLOAD_ID` come from `./mock-fetch`; `hello` and `HELLO_SHA256` are defined at `client.test.ts:14-15`). Replace the test at `:183-194` with the first one and add the other three. All four fail before (no DELETE is sent).

```ts
  it("reports a refused transfer and deletes the pending upload", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => apiError(403, "forbidden", "the URL is invalid or has expired"),
      () => new Response(null, { status: 204 }),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(
      client.upload(hello(), { filename: "report.md", sha256: HELLO_SHA256 }),
    ).rejects.toMatchObject({ status: 403, code: "forbidden" });
    expect(mock.requests).toHaveLength(3);
    expect(mock.requests[2]?.method).toBe("DELETE");
    expect(mock.requests[2]?.url).toBe(`${BASE_URL}/v1/uploads/${UPLOAD_ID}`);
  });

  it("deletes the pending upload when completion fails and keeps the original error", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => new Response(null, { status: 200 }),
      () => apiError(422, "invalid_request", "the file does not match its SHA-256"),
      () => new Response(null, { status: 204 }),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(
      client.upload(hello(), { filename: "report.md", sha256: HELLO_SHA256 }),
    ).rejects.toMatchObject({ status: 422, code: "invalid_request" });
    expect(mock.requests[3]?.method).toBe("DELETE");
  });

  it("keeps the transfer error when the cleanup also fails", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => apiError(403, "forbidden", "the URL is invalid or has expired"),
      () => apiError(500, "internal", "retry later"),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(
      client.upload(hello(), { filename: "report.md", sha256: HELLO_SHA256 }),
    ).rejects.toMatchObject({ status: 403 });
    expect(mock.requests).toHaveLength(3);
  });

  it("cleans up after an aborted transfer without reusing the aborted signal", async () => {
    const controller = new AbortController();
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => {
        controller.abort();
        throw controller.signal.reason;
      },
      () => new Response(null, { status: 204 }),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(
      client.upload(hello(), {
        filename: "report.md",
        sha256: HELLO_SHA256,
        signal: controller.signal,
      }),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(mock.requests[2]?.method).toBe("DELETE");
    expect(mock.requests[2]?.signal.aborted).toBe(false);
  });
```

**Gate:** `pnpm --filter @ariadshift/sdk test`; `pnpm --filter @ariadshift/sdk typecheck`; `just web-lint`.

**Commit:** `fix(sdk): delete the pending upload when an upload fails`

**Risks:** one extra request per failed upload, bounded at 10 s. No cleanup when no presigned request comes back (deliberate). E8 later edits `transfer()` retries in the same file (step 4). G4's web flow does not use `upload()`.

#### D15: SDK wrappers lack tests and were never run (Medium)

**Current behaviour.** `deleteUpload` and `limits` (`client.ts:302-308`) have no tests; `listJobs` lacks a 422 case and an explicit-`undefined` case (`list-jobs.test.ts`). These are coverage additions that should pass on current code; their value is running generate, check, typecheck and test for the first time on this branch.

**New file `packages/sdk/test/uploads-and-limits.test.ts`:**

```ts
import { describe, expect, it } from "vitest";
import { AriadApiError, createAriadClient } from "../src/index";
import { apiError, BASE_URL, json, mockFetch, UPLOAD_ID } from "./mock-fetch";

const noContent = () => new Response(null, { status: 204 });

const planLimits = {
  plan: "anonymous",
  authenticated: false,
  retention_s: 3600,
  requests_per_minute: 30,
  limits: { max_input_bytes: 10_485_760, max_pages: 50, timeout_s: 120, max_memory_mb: null },
  quotas: {
    uploads_per_day: { limit: 20, used: 1, remaining: 19 },
    jobs_per_day: { limit: 20, used: 0, remaining: 20 },
    input_bytes_per_day: { limit: 104_857_600, used: 5, remaining: 104_857_595 },
    pending_uploads: { limit: 1, used: 0, remaining: 1 },
    concurrent_jobs: { limit: null, used: 0, remaining: null },
  },
};

describe("deleteUpload", () => {
  it("deletes an upload and succeeds again for one already deleted", async () => {
    const mock = mockFetch(noContent, noContent);
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { apiKey: "ak_test" },
      fetch: mock.fetch,
    });

    await expect(client.deleteUpload(UPLOAD_ID)).resolves.toBeUndefined();
    await expect(client.deleteUpload(UPLOAD_ID)).resolves.toBeUndefined();

    expect(mock.requests).toHaveLength(2);
    for (const request of mock.requests) {
      expect(request.method).toBe("DELETE");
      expect(request.url).toBe(`${BASE_URL}/v1/uploads/${UPLOAD_ID}`);
      expect(request.headers.get("x-api-key")).toBe("ak_test");
    }
  });

  it("surfaces an upload that feeds a job as a 409 AriadApiError", async () => {
    const mock = mockFetch(() =>
      apiError(409, "conflict", "a job uses the upload; delete the job instead"),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const error = await client.deleteUpload(UPLOAD_ID).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(AriadApiError);
    expect(error).toMatchObject({ status: 409, code: "conflict" });
  });
});

describe("limits", () => {
  it("reads the plan limits without credentials for anonymous use", async () => {
    const mock = mockFetch(() => json(planLimits));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    expect(await client.limits()).toEqual(planLimits);
    const request = mock.requests[0];
    expect(request?.method).toBe("GET");
    expect(request?.url).toBe(`${BASE_URL}/v1/limits`);
    expect(request?.headers.get("authorization")).toBeNull();
    expect(request?.headers.get("x-api-key")).toBeNull();
  });

  it("sends the bearer token when signed in", async () => {
    const signedIn = { ...planLimits, plan: "free", authenticated: true };
    const mock = mockFetch(() => json(signedIn));
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: "jwt" },
      fetch: mock.fetch,
    });

    expect(await client.limits()).toEqual(signedIn);
    expect(mock.requests[0]?.headers.get("authorization")).toBe("Bearer jwt");
  });
});
```

Append to `list-jobs.test.ts` inside `describe("listJobs")`:

```ts
  it("surfaces an out-of-range limit as a 422 AriadApiError", async () => {
    const mock = mockFetch(() =>
      apiError(422, "invalid_request", "limit must be between 1 and 100"),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { apiKey: "ak_test" },
      fetch: mock.fetch,
    });

    const error = await client.listJobs({ limit: 0 }).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(AriadApiError);
    expect(error).toMatchObject({ status: 422, code: "invalid_request" });
    expect(new URL(mock.requests[0]?.url ?? "").searchParams.get("limit")).toBe("0");
  });

  it("leaves explicitly undefined options out of the query", async () => {
    const mock = mockFetch(() => json({ jobs: [], next_cursor: null }));
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: "token" },
      fetch: mock.fetch,
    });

    await client.listJobs({ limit: undefined, cursor: undefined, status: undefined });

    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs`);
  });
```

Add `AriadApiError` to the `list-jobs.test.ts` imports if missing. Confirm the credential option names (`apiKey`, `bearer`) against `createAriadClient`.

**Gate (after D0):** `pnpm --filter @ariadshift/sdk generate && pnpm --filter @ariadshift/sdk check && pnpm --filter @ariadshift/sdk typecheck && pnpm --filter @ariadshift/sdk test`; `just web-lint`. No `exactOptionalPropertyTypes`, so explicit `undefined` type-checks.

**Commit:** `test(sdk): cover upload deletion, plan limits and job list errors`

---

### W5 API contract

#### D16: the frozen snapshot may lag the generated document (Medium)

**Current behaviour.** `crates/ariad-server/tests/openapi_v1_snapshot.rs:38-53` fails only on breaking changes and writes the snapshot only when `ARIAD_UPDATE_OPENAPI_BASELINE` is set (`:49`), so an additive change ships unfrozen. `docs/api.md:138` says the test "fails on any breaking change".

**Change:**

```rust
 #[test]
 fn v1_stays_backward_compatible_with_the_frozen_snapshot() {
     let path = baseline_path();
     let text = fs::read_to_string(&path)
         .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
     let baseline: Value = serde_json::from_str(&text).expect("parse the snapshot");
-    let problems = breaking_changes(&baseline, &current());
+    let updating = std::env::var_os("ARIAD_UPDATE_OPENAPI_BASELINE").is_some();
+    let problems = snapshot_problems(&baseline, &current(), updating);
     assert!(
         problems.is_empty(),
-        "breaking changes to /v1 (keep the old shape, or add a new major version path):\n  {}",
+        "/v1 snapshot check failed (keep the old shape, add a new major version path, or refresh an additive change):\n  {}",
         problems.join("\n  ")
     );
-    if std::env::var_os("ARIAD_UPDATE_OPENAPI_BASELINE").is_some() {
+    if updating {
         let json = ariad_server::api::openapi_json().expect("serialize");
         fs::write(&path, json).expect("write the snapshot");
     }
 }
+
+/// Breaking changes, plus any additive change the snapshot does not contain yet,
+/// unless the snapshot is being refreshed. A field must be frozen before it ships,
+/// or a later removal cannot be detected.
+fn snapshot_problems(baseline: &Value, current: &Value, updating: bool) -> Vec<String> {
+    let mut problems = breaking_changes(baseline, current);
+    if problems.is_empty() && !updating && baseline != current {
+        problems.push(
+            "additive change not in the snapshot: refresh it with \
+             ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot"
+                .to_owned(),
+        );
+    }
+    problems
+}
```

Also update the module doc (`:12-13`) and `docs/api.md:138` ("…and on an additive change that the snapshot does not yet contain").

**Test** (same file; `current`, `breaking_changes` and `json!` exist):

```rust
#[test]
fn an_unfrozen_additive_change_fails_until_the_snapshot_is_refreshed() {
    let old = current();
    let mut new = old.clone();
    new["components"]["schemas"]["Job"]["properties"]["note"] = json!({ "type": "string" });
    assert_eq!(breaking_changes(&old, &new), Vec::<String>::new());
    let problems = snapshot_problems(&old, &new, false);
    assert!(problems.iter().any(|p| p.contains("refresh it")), "{problems:?}");
    assert_eq!(snapshot_problems(&old, &new, true), Vec::<String>::new());
    assert_eq!(snapshot_problems(&old, &old, false), Vec::<String>::new());
}
```

**Gate:** after D0, `ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot`, then without the variable `cargo nextest run -p ariad-server --test openapi_v1_snapshot --test openapi_drift`; commit the refreshed snapshot with the change.

**Commit:** `test(server): fail when the frozen OpenAPI snapshot lags the document`

**Risk:** every additive change now needs the refresh step, which `docs/api.md:158` documents.

#### D25 c: always-sent nullable fields are not `required` (Low)

**Current behaviour.** `dto.rs:345-352` `Quota.limit`/`remaining`, `:366-376` all four `ConversionLimits` fields and `:403` `PlanLimits.requests_per_minute` are `Option<u64>` without `#[schema(required = true)]`, although the server always sends them (as `null` when unset). The spec's `Quota.required` is `["used"]`. The pattern already exists at `job_list.rs:62`.

**Change:** add `#[schema(required = true)]` above each of the 7 fields (optionally also `Job.started_at`, `finished_at`, `error` at `dto.rs:242-243`). Then `just openapi`, the snapshot refresh and `pnpm --filter @ariadshift/sdk lock`.

**Test** (`openapi_v1_snapshot.rs`; `names` exists at `:388`):

```rust
#[test]
fn always_emitted_nullable_fields_are_required() {
    let doc = current();
    for (schema, field) in [
        ("Quota", "limit"),
        ("Quota", "remaining"),
        ("ConversionLimits", "max_input_bytes"),
        ("ConversionLimits", "max_pages"),
        ("ConversionLimits", "timeout_s"),
        ("ConversionLimits", "max_memory_mb"),
        ("PlanLimits", "requests_per_minute"),
    ] {
        assert!(
            names(&doc["components"]["schemas"][schema]["required"]).contains(field),
            "{schema}.{field} is always sent, so the frozen contract must require it"
        );
    }
}
```

**Commit:** `fix(server): require always-sent nullable limit fields in the OpenAPI document`

#### D25 d: snapshot checker gaps (Low)

**Current behaviour** (`openapi_v1_snapshot.rs`): composites are compared by raw equality and never traversed (`:660-667`); a response enum that disappears is not flagged (`:698-707`); `exclusiveMaximum`/`exclusiveMinimum` are missing (`:755-762`); nothing compares `default`, `pattern`, `multipleOf`, `uniqueItems` or `components.securitySchemes`.

**Change** (adapt names to the checker struct; `self.problem`, `self.schema`, `Dir`, `dir.label()` and `self.bounds` exist there):

```rust
// shape(): replace the raw composite comparison
let composites = ["oneOf", "anyOf", "allOf"];
if composites.iter().any(|key| old.get(key).is_some()) {
    for key in composites {
        match (old.get(key).and_then(Value::as_array), new.get(key).and_then(Value::as_array)) {
            (None, None) => {}
            (Some(a), Some(b)) if a.len() == b.len() => {
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    self.schema(&format!("{at}.{key}[{i}]"), a, b, dir);
                }
            }
            _ => self.problem(format!("{at}: composite schema changed ({side})")),
        }
    }
    return;
}
// shape(): enum
if let Some(old_values) = old.get("enum").and_then(Value::as_array) {
    match new.get("enum").and_then(Value::as_array) {
        Some(new_values) => {
            for value in old_values {
                if !new_values.contains(value) {
                    self.problem(format!("{at}: removed enum value {value} ({side})"));
                }
            }
        }
        None if dir == Dir::Response => self.problem(format!("{at}: enum removed ({side})")),
        None => {}
    }
} else if dir == Dir::Request && new.get("enum").is_some() { /* unchanged */ }
// shape(): after self.bounds(at, old, new, dir);
if dir == Dir::Request && old.get("default").is_some() && old.get("default") != new.get("default") {
    self.problem(format!("{at}: default changed ({side})"));
}
self.constraints(at, old, new, dir);

/// Requests may not add or change a constraint; responses may not drop or change one.
fn constraints(&mut self, at: &str, old: &Value, new: &Value, dir: Dir) {
    let side = dir.label();
    for key in ["pattern", "multipleOf", "uniqueItems"] {
        let get = |schema: &Value| schema.get(key).filter(|v| **v != json!(false)).cloned();
        let (a, b) = (get(old), get(new));
        let broken = match dir {
            Dir::Request => b.is_some() && a != b,
            Dir::Response => a.is_some() && a != b,
        };
        if broken {
            self.problem(format!("{at}: {key} changed ({side})"));
        }
    }
}
// bounds(): add ("exclusiveMaximum", true), ("exclusiveMinimum", false) to the list.
// document(): after the schema loop
let new_schemes = obj(&self.new["components"]["securitySchemes"]);
for (name, old_scheme) in obj(&self.old["components"]["securitySchemes"]) {
    match new_schemes.get(&name) {
        None => self.problem(format!("removed security scheme {name}")),
        Some(new_scheme) if without_description(new_scheme) != without_description(&old_scheme) => {
            self.problem(format!("security scheme {name} changed"));
        }
        Some(_) => {}
    }
}
fn without_description(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(map) = value.as_object_mut() {
        map.remove("description");
    }
    value
}
```

**Tests** (same file; `crud`, `object`, `assert_mentions`, `assert_compatible` exist at `:133-171`). Each fails before.

```rust
#[test]
fn composite_parts_are_compared_by_position() {
    let req = object(&[], json!({}));
    let part = object(&["id"], json!({ "id": { "type": "string" } }));
    let res = |part: Value| json!({ "allOf": [{ "$ref": "#/components/schemas/Req" }, part] });
    let old = crud(req.clone(), res(part.clone()));

    let mut wider = part.clone();
    wider["properties"]["extra"] = json!({ "type": "integer" });
    assert_compatible(&old, &crud(req.clone(), res(wider)));

    let mut narrower = part.clone();
    narrower["properties"].as_object_mut().unwrap().remove("id");
    assert_mentions(&old, &crud(req.clone(), res(narrower)), "id: removed property");

    let other = json!({ "anyOf": [{ "$ref": "#/components/schemas/Req" }, part] });
    assert_mentions(&old, &crud(req, other), "composite schema changed");
}

#[test]
fn enums_defaults_constraints_and_security_schemes_are_checked() {
    let req = object(
        &[],
        json!({
            "name": { "type": "string", "pattern": "^[a-z]+$" },
            "limit": { "type": "integer", "default": 50 }
        }),
    );
    let res = object(&["kind"], json!({ "kind": { "type": "string", "enum": ["a", "b"] } }));
    let old = crud(req.clone(), res.clone());

    let mut new = res.clone();
    new["properties"]["kind"].as_object_mut().unwrap().remove("enum");
    assert_mentions(&old, &crud(req.clone(), new), "Res.kind: enum removed");

    let mut new = req.clone();
    new["properties"]["limit"]["default"] = json!(10);
    assert_mentions(&old, &crud(new, res.clone()), "Req.limit: default changed");

    let mut new = req.clone();
    new["properties"]["name"]["pattern"] = json!("^[a-z]{1,8}$");
    assert_mentions(&old, &crud(new, res.clone()), "Req.name: pattern changed");
    let mut new = req.clone();
    new["properties"]["name"].as_object_mut().unwrap().remove("pattern");
    assert_compatible(&old, &crud(new, res.clone()));

    let mut new = req.clone();
    new["properties"]["limit"]["exclusiveMaximum"] = json!(101);
    assert_mentions(&old, &crud(new, res), "Req.limit: exclusiveMaximum tightened");

    let mut with_scheme = old.clone();
    with_scheme["components"]["securitySchemes"] =
        json!({ "api_key": { "type": "apiKey", "in": "header", "name": "x-api-key" } });
    let mut renamed = with_scheme.clone();
    renamed["components"]["securitySchemes"]["api_key"]["name"] = json!("x-key");
    assert_mentions(&with_scheme, &renamed, "security scheme api_key changed");
    let mut removed = with_scheme.clone();
    removed["components"]["securitySchemes"].as_object_mut().unwrap().remove("api_key");
    assert_mentions(&with_scheme, &removed, "removed security scheme api_key");
}
```

The exact `assert_mentions` needles depend on the at-path format the checker builds; confirm against an existing test such as `request_changes_are_checked` (`:174`).

**Gate:** `cargo nextest run -p ariad-server --test openapi_v1_snapshot --test openapi_drift`; `just openapi`; snapshot refresh; `pnpm --filter @ariadshift/sdk lock`; `just sdk-check`; `just ci`.

**Commit:** `test(server): check composite, enum, default, constraint and security scheme changes`

**Risk:** a stricter checker could flag real D0 diffs; irrelevant because the snapshot is refreshed right after D0.

---

### W6 server auth and MCP

#### D17: bare requests fill the auth-failure bucket and lock out valid credentials (Medium)

**Current behaviour.** `api/mod.rs:123-131` pre-checks `auth-failure:{client}` for any credential-bearing request before verification; `:138-145` counts every 401, including "credentials are required" (`auth/mod.rs:153-155`), which happens on `/mcp` (`mcp_auth_config` sets `anonymous = false`, `mcp/mod.rs:180`) and on REST with `ARIAD_ANONYMOUS=false`. Thirty bare probes make the next valid credential get 429, and the 31st bare probe gets 429 instead of the `WWW-Authenticate` challenge (`mcp/mod.rs:242-246`), breaking MCP discovery. `tests/auth.rs:357-358` asserts the old design (a valid key gets 429).

**Change** (in `Caller::from_request_parts`): verify first; count only presented credentials that fail.

```rust
-        let client = state.auth.client_hash(&parts.headers, peer);
-        let auth_bucket = format!("auth-failure:{client}");
-        if (parts.headers.contains_key(AUTHORIZATION) || parts.headers.contains_key(API_KEY_HEADER))
-            && let rate_limit::Decision::Limited { retry_after_s } =
-                rate_limit::check(&state.db, &auth_bucket, AUTH_FAILURES_PER_MINUTE).await?
-        { return Err(ApiError::rate_limited(retry_after_s)); }
+        // Verify first: a credential that verifies is never throttled by its
+        // address's failures, and requests without credentials never count.
+        let presented = parts.headers.contains_key(axum::http::header::AUTHORIZATION)
+            || parts.headers.contains_key(crate::auth::API_KEY_HEADER);
         let principal = match state.auth.authenticate(&state.db, &parts.headers, peer).await {
             Ok(principal) => principal,
-            Err(err) if err.status() == StatusCode::UNAUTHORIZED => {
+            Err(err) if presented && err.status() == StatusCode::UNAUTHORIZED => {
+                let client = state.auth.client_hash(&parts.headers, peer);
+                let auth_bucket = format!("auth-failure:{client}");
                 if let rate_limit::Decision::Limited { retry_after_s } =
                     rate_limit::hit(&state.db, &auth_bucket, AUTH_FAILURES_PER_MINUTE).await?
```

`rate_limit::check` becomes unused (public module, so no warning; remove optionally). `mcp/mod.rs` needs no change. `docs/server.md:200`: "After 30 rejected credentials from one client address in a minute, further rejected credentials from that address receive 429 with Retry-After until the next window. A credential that verifies is always accepted, and requests without credentials never count." `docs/server.md:113`: say one client's failures throttle everyone's rejected credentials, not everyone.

**Tests.**
- `tests/auth.rs:357-358`: change the expectation to `200` with the comment "a credential that verifies is not blocked by its address's failures".
- Add to `tests/auth.rs` (`error_code` at `:13`, `AUTH_FAILURES_PER_MINUTE` public at `api/mod.rs:55`, table `rate_limit_counters` exists; reuse the file's `Harness`, `Options`, `Auth`, `create_apikey_table`, `insert_api_key`):

```rust
#[tokio::test(flavor = "multi_thread")]
async fn requests_without_credentials_do_not_lock_out_valid_keys() {
    let Some(h) = Harness::start(
        "requests_without_credentials_do_not_lock_out_valid_keys",
        Options {
            env: vec![("ARIAD_ANONYMOUS".to_owned(), "false".to_owned())],
            ..Options::default()
        },
    )
    .await
    else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let body = json!({ "from": "md", "to": "html" });
    // Stay inside one minute window, so the count cannot reset midway.
    let into_minute = chrono::Utc::now().timestamp() % 60;
    if into_minute > 45 {
        tokio::time::sleep(std::time::Duration::from_secs((61 - into_minute) as u64)).await;
    }
    for _ in 0..=ariad_server::api::AUTH_FAILURES_PER_MINUTE {
        assert_eq!(
            error_code(h.post(&Auth::Anonymous, "/v1/plan", &body).await).await,
            (401, "unauthorized".to_owned())
        );
    }
    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rate_limit_counters WHERE bucket LIKE 'auth-failure:%'",
    )
    .fetch_one(&h.db.pool)
    .await
    .unwrap();
    assert_eq!(failures, 0);
    let valid = Auth::ApiKey(insert_api_key(&h.db.pool, "unlocked-user").await);
    assert_eq!(h.post(&valid, "/v1/plan", &body).await.status(), 200);
    h.stop().await;
}
```

- Add to `tests/mcp_remote.rs` (uses that file's `rpc` helper and `Duration` import):

```rust
#[tokio::test(flavor = "multi_thread")]
async fn bare_probes_keep_the_challenge_and_never_lock_out_credentials() {
    let Some(h) = Harness::start("mcp_bare_probes", Options::default()).await else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let key = Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await);
    let into_minute = chrono::Utc::now().timestamp() % 60;
    if into_minute > 45 {
        tokio::time::sleep(Duration::from_secs((61 - into_minute) as u64)).await;
    }
    for _ in 0..=ariad_server::api::AUTH_FAILURES_PER_MINUTE {
        let probe = rpc(&h, &Auth::Anonymous, "tools/list", json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(probe.status(), 401);
        assert!(probe.headers().contains_key("www-authenticate"));
    }
    let listed = rpc(&h, &key, "tools/list", json!({})).send().await.unwrap();
    assert_eq!(listed.status(), 200);
    h.stop().await;
}
```

Both fail before (the 31st bare request gets 429; then the key gets 429).

**Gate:** `just test-server` (Docker; the log must not say "skipping …", because Harness tests pass silently without `ARIAD_TEST_DATABASE_URL`); `just clippy`.

**Commit:** `fix(server): count only presented credentials in the auth failure throttle`

**Risk:** reverses the design asserted at `auth.rs:358`, as the master plan's fix states. A throttled address with a bad credential now pays one verification before its 429; guessing high-entropy keys stays infeasible.

#### D25 a–b: MCP inline storage (Low)

**Current behaviour.** `mcp/tools.rs:197-210` (function `:179-218`) stores inline content through `put_stream`, which goes through local blob admission (`storage.rs:486`) on every backend. `StorageError::Busy` becomes `ApiError::internal` (500). Any failure after `create_upload` (`:185`) leaves the upload `pending`, holding a quota slot until expiry. `put_stream_until` writes 256 KiB multipart parts (`storage.rs:488,514`), below the S3/R2 5 MiB minimum, and MCP inline is the only S3 caller.

**Change** in `storage.rs` (place after `delete`, about `:624`, away from the E0 hunks):

```rust
/// Stores a small in-memory object in one request on any backend. Blob admission
/// bounds slow HTTP bodies, not bytes already held in memory.
pub async fn put_bytes(&self, key: &str, bytes: bytes::Bytes) -> Result<(), StorageError> {
    self.store.put(&Path::from(key), bytes.into()).await?;
    Ok(())
}
```

Add to the `put_stream` doc: "Local blob route only; S3 needs multipart parts of at least 5 MiB." In `tools.rs`:

```rust
-        let stream = futures_util::stream::iter([Ok::<Bytes, std::io::Error>(bytes)]);
-        let stored = self.state.storage.put_stream(&key, stream, size, &sha256).await
-            .map_err(|err| ApiError::internal(&err))?;
-        if !stored { return Err(ApiError::conflict("inline content could not be stored")); }
-        let _ = api::uploads::complete_upload(State(self.state.clone()), copy_caller(caller), Path(id.to_string())).await?;
-        Ok(id)
+        let stored = async {
+            self.state.storage.put_bytes(&key, bytes).await.map_err(|err| ApiError::internal(&err))?;
+            api::uploads::complete_upload(State(self.state.clone()), copy_caller(caller), Path(id.to_string()))
+                .await
+                .map(|_| ())
+        }
+        .await;
+        if let Err(err) = stored {
+            // Free the caller's pending slot now instead of when the upload expires.
+            let _ = api::uploads::delete_upload(State(self.state.clone()), copy_caller(caller), Path(id.to_string())).await;
+            return Err(err);
+        }
+        Ok(id)
```

`delete_upload` is `pub` (`api/uploads.rs:262`) and idempotent. Check whether `size`/`sha256` become unused and drop them.

**Test 1**, appended to `tests/mcp_remote.rs` (fails before: the status stays `pending`):

```rust
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_inline_store_retires_its_upload() {
    let Some(h) = Harness::start("mcp_inline_store_failure", Options::default()).await else {
        return;
    };
    create_apikey_table(&h.db.pool).await;
    let alice = Auth::ApiKey(insert_api_key(&h.db.pool, "alice").await);
    // The first authenticated call creates the workspace.
    let engines = call(&h, &alice, "list_engines", json!({})).await;
    assert_ne!(engines["isError"], true, "{engines}");
    let workspace: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE owner_user_id = 'alice'")
            .fetch_one(&h.db.pool)
            .await
            .unwrap();
    // A file where the workspace's key prefix belongs makes every write under it fail.
    std::fs::create_dir_all(h.objects.path().join("t")).unwrap();
    std::fs::write(h.objects.path().join("t").join(workspace.to_string()), b"").unwrap();
    let failed = call(
        &h,
        &alice,
        "convert",
        json!({"inline": {"filename": "notes.md", "content": "# Stranded\n"}, "to": "html"}),
    )
    .await;
    assert_eq!(failed["isError"], true, "{failed}");
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM uploads WHERE workspace_id = $1")
            .bind(workspace)
            .fetch_all(&h.db.pool)
            .await
            .unwrap();
    assert_eq!(statuses, ["deleted"]);
    h.stop().await;
}
```

Confirm the object-key prefix (`t/<workspace>/…`) and the `h.objects` field name against `storage.rs` and the harness.

**Test 2**, new file `crates/ariad-server/tests/inline_storage.rs` (no database; does not compile before). Corrections applied versus the draft: the secret is passed as `&[u8]` (`Option<&[u8; 32]>` does not coerce), and limits are set field by field so the test compiles both before and after E0 adds fields without tripping clippy's `needless_update`.

```rust
//! In-memory writes do not wait for the local blob transfer budget.

use std::{convert::Infallible, time::Duration};

use ariad_server::{
    config::{BlobTransferLimits, StorageConfig},
    storage::{Storage, StorageError},
};
use bytes::Bytes;
use futures_util::stream;
use sha2::{Digest, Sha256};
use url::Url;

#[tokio::test]
async fn in_memory_writes_skip_the_blob_transfer_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut limits = BlobTransferLimits::default();
    limits.max_concurrent = 1;
    limits.idle_timeout = Duration::from_secs(5);
    limits.transfer_timeout = Duration::from_secs(5);
    let storage = Storage::from_config(
        &StorageConfig::Local { root: dir.path().to_owned() },
        Some(&Url::parse("http://localhost:8080").unwrap()),
        Some(b"0123456789abcdef0123456789abcdef".as_slice()),
    )
    .unwrap()
    .with_blob_transfer_limits(limits)
    .unwrap();
    let held = Bytes::from_static(b"held");
    let hash = hex::encode(Sha256::digest(&held));
    let body = || stream::iter([Ok::<_, Infallible>(held.clone())]);
    assert!(storage.put_stream("held", body(), held.len() as u64, &hash).await.unwrap());
    // One open download takes the only transfer slot.
    let (_, download) = storage.get_stream("held").await.unwrap();
    assert!(matches!(
        storage.put_stream("blocked", body(), held.len() as u64, &hash).await,
        Err(StorageError::Busy)
    ));
    storage.put_bytes("inline", Bytes::from_static(b"# Inline\n")).await.unwrap();
    assert_eq!(storage.size("inline").await.unwrap(), Some(9));
    drop(download);
}
```

Verified on the branch: `from_config(&StorageConfig, Option<&Url>, Option<&[u8]>)` (`storage.rs:177`), `with_blob_transfer_limits` (`:229`), `put_stream` returns `Result<bool, _>` (`:457`), `get_stream` (`:540`), `size` (`:378`), `BlobTransferLimits` has three public fields and `Default` (`config.rs:307-315`); `hex`, `sha2`, `bytes`, `futures-util`, `url` are dependencies and `tempfile` a dev-dependency. Confirm the `StorageConfig::Local` field name and `get_stream`'s tuple shape.

**Gate:** `cargo nextest run -p ariad-server --test inline_storage`; `just test-server`; `just clippy`.

**Commit:** `fix(server): store MCP inline content outside blob admission and retire failed uploads`

**Risk:** light conflict with E0 `90787ce`/`709f982` in `storage.rs` at step 4.

---

### W7 auth service (after C1; order D21, D23, D18, D19, D22)

Better Auth 1.7.12 is not installed on this Mac, so behaviour that depends on the library is marked "unverified (library)" and must be checked on ASUS after `just js`. Verified helpers: `services/auth/test/oauth-helpers.ts` exports `AUTH`, `RESOURCE`, `REDIRECT_URI`, `config`, `createService(database?, rateLimit = false)`, `App`, `Jar` (`store`, `header`), `signedQuery`, `pkce`, `json`, `post(app, path, body, headers)`, `token`, `authorizeUrl(clientId, extra)`, `browserFlow`; `app.test.ts` has `setup(trustedClientIpHeader?, peer, pages)` returning `{ app, seen, documents }`; `src/app.ts` exports `TOKEN_PATH` (`:32`) and `PUBLIC_OAUTH_PATHS` (`:40`); `src/pages.ts` exports `ASSETS` (`:65`).

#### D21: 2FA during OAuth and privileged registration fields are untested (unverified; critical if it fails)

**Current behaviour.** `auth.ts:151-152` lists `twoFactor` before `oauthProvider`; C1 adds only a static order test. No test covers password-only sign-in on a 2FA account during OAuth, or `skip_consent` on anonymous registration. **If either security test fails, stop and send the evidence to the Mac** instead of patching.

**Change:** tests only, in a new file so C1's edits to `oauth-security.test.ts` stay untouched.

New `services/auth/test/oauth-two-factor.test.ts`:

```ts
// Two-factor accounts and privileged client metadata during OAuth, in process
// over Better Auth's memory adapter. A password alone must never yield an
// authorization code for an account with two-factor on, and an anonymous
// registration must not be able to skip the consent screen.

import { createHmac, randomBytes } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  AUTH,
  type App,
  authorizeUrl,
  createService,
  Jar,
  json,
  pkce,
  post,
  REDIRECT_URI,
  signedQuery,
} from "./oauth-helpers.ts";

const PASSWORD = "correct horse battery staple";

function base32Decode(value: string): Buffer {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0;
  let accumulator = 0;
  const out: number[] = [];
  for (const char of value.replace(/=+$/, "").toUpperCase()) {
    const index = alphabet.indexOf(char);
    if (index < 0) throw new Error(`not base32: ${char}`);
    accumulator = (accumulator << 5) | index;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((accumulator >>> bits) & 0xff);
    }
  }
  return Buffer.from(out);
}

/** RFC 6238 TOTP: SHA-1, 6 digits, 30-second steps, as authenticator apps compute it. */
function totp(secret: string, now = Date.now()): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor(now / 30_000)));
  const mac = createHmac("sha1", base32Decode(secret)).update(counter).digest();
  const offset = (mac.at(-1) as number) & 0x0f;
  return ((mac.readUInt32BE(offset) & 0x7fffffff) % 1_000_000).toString().padStart(6, "0");
}

async function signUp(app: App) {
  const email = `2fa-${randomBytes(6).toString("hex")}@example.com`;
  const jar = new Jar();
  const res = await post(app, "/api/auth/sign-up/email", { name: "Two factor", email, password: PASSWORD });
  expect(res.status).toBe(200);
  jar.store(res);
  return { email, jar };
}

/** An account with TOTP two-factor turned on, and its TOTP secret. */
async function twoFactorAccount(app: App) {
  const { email, jar } = await signUp(app);
  const enable = await post(app, "/api/auth/two-factor/enable", { password: PASSWORD }, jar.header());
  expect(enable.status).toBe(200);
  jar.store(enable);
  const secret = new URL((await json(enable)).totpURI as string).searchParams.get("secret") ?? "";
  const verify = await post(app, "/api/auth/two-factor/verify-totp", { code: totp(secret) }, jar.header());
  expect(verify.status).toBe(200);
  return { email, secret };
}

async function registerClient(app: App, extra: Record<string, unknown> = {}) {
  return post(app, "/api/auth/oauth2/register", {
    client_name: "Two-factor test",
    redirect_uris: [REDIRECT_URI],
    token_endpoint_auth_method: "none",
    grant_types: ["authorization_code", "refresh_token"],
    response_types: ["code"],
    ...extra,
  });
}

/** The signed query of the sign-in page an unauthenticated authorization lands on. */
async function signInQuery(app: App, clientId: string) {
  const res = await app.request(
    authorizeUrl(clientId, { code_challenge: pkce().challenge, code_challenge_method: "S256" }),
  );
  expect(res.status).toBe(302);
  const location = res.headers.get("location") ?? "";
  expect(location).toContain(`${AUTH}/oauth/sign-in?`);
  return signedQuery(location);
}

describe("OAuth and two-factor accounts", () => {
  it("issues no code and keeps no session after a password-only sign-in", async () => {
    const { app } = createService();
    const { email } = await twoFactorAccount(app);
    const clientId = (await json(await registerClient(app))).client_id as string;
    const oauthQuery = await signInQuery(app, clientId);

    const browser = new Jar();
    const signIn = await post(app, "/api/auth/sign-in/email", {
      email,
      password: PASSWORD,
      oauth_query: oauthQuery,
    });
    expect(signIn.status).toBe(200);
    browser.store(signIn);
    const body = await json(signIn);
    expect(body.twoFactorRedirect).toBe(true);
    expect(body.redirect).not.toBe(true);
    expect(JSON.stringify(body)).not.toContain("code=");
    expect(signIn.headers.get("location")).toBeNull();

    const session = await app.request(`${AUTH}/api/auth/get-session`, { headers: browser.header() });
    expect(await session.json()).toBeNull();
    const again = await app.request(
      authorizeUrl(clientId, { code_challenge: pkce().challenge, code_challenge_method: "S256" }),
      { headers: browser.header() },
    );
    expect(again.status).toBe(302);
    const location = new URL(again.headers.get("location") ?? "");
    expect(`${location.origin}${location.pathname}`).toBe(`${AUTH}/oauth/sign-in`);
    expect(location.searchParams.has("code")).toBe(false);
  });

  it("continues the authorization once the second factor is verified", async () => {
    const { app } = createService();
    const { email, secret } = await twoFactorAccount(app);
    const clientId = (await json(await registerClient(app))).client_id as string;
    const oauthQuery = await signInQuery(app, clientId);
    const browser = new Jar();
    const signIn = await post(app, "/api/auth/sign-in/email", { email, password: PASSWORD, oauth_query: oauthQuery });
    browser.store(signIn);
    const second = await post(
      app,
      "/api/auth/two-factor/verify-totp",
      { code: totp(secret, Date.now() + 30_000), oauth_query: oauthQuery },
      browser.header(),
    );
    expect(second.status).toBe(200);
    expect((await json(second)).redirect).toBe(true);
  });
});

describe("OAuth registration without an account", () => {
  it("cannot register a client that skips the consent screen", async () => {
    const { app } = createService();
    const { email } = await signUp(app);
    const victim = new Jar();
    const signIn = await post(app, "/api/auth/sign-in/email", { email, password: PASSWORD });
    expect(signIn.status).toBe(200);
    victim.store(signIn);

    const register = await registerClient(app, { skip_consent: true, skipConsent: true });
    // Refusing the privileged field outright is also safe.
    if (register.status >= 400) return;
    const client = await json(register);
    expect(client.skip_consent).not.toBe(true);
    const res = await app.request(
      authorizeUrl(client.client_id as string, {
        code_challenge: pkce().challenge,
        code_challenge_method: "S256",
      }),
      { headers: victim.header() },
    );
    expect(res.status).toBe(302);
    const location = new URL(res.headers.get("location") ?? "");
    expect(`${location.origin}${location.pathname}`).toBe(`${AUTH}/oauth/consent`);
    expect(location.searchParams.has("code")).toBe(false);
  });
});
```

If the positive control (second test) fails, that is a UX bug (the page says "Signed in, but the authorization did not continue"); report it. Unverified (library): the paths `/two-factor/enable` and `/two-factor/verify-totp`, the field `totpURI`, the TOTP window. Adjust to 1.7.12 and keep the assertions.

**Gate:** `pnpm --filter @ariadshift/auth exec vitest run test/oauth-two-factor.test.ts`, then the full auth test and typecheck.

**Commit:** `test(auth): prove two-factor and consent hold during OAuth authorization`

#### D23: consent display, asset lookup, path normalisation (Low)

**Current behaviour.** `src/pages/oauth.js:197-210` shows `scope`, `resource` and `redirect_uri` from the raw query (first value), and `:228` reads `client_id` the same way, while the POST sends only the signed subset (`signedQuery`, `:14-23`). For non-HTTP redirects `:207` shows only `scheme://`. `app.ts:152` uses `ASSETS[name]` on a plain object, so `constructor`, `toString`, `__proto__` resolve to inherited members. `app.ts:92` (DPoP) and `:124` (CORS) match `c.req.path` exactly, while `oauth-lock.ts:43-47` trims trailing slashes and keeps its own path list.

**Change:**

```diff
--- services/auth/src/pages/oauth.js
+/** Parameters that must appear once; a repeat means the link was altered. */
+const SINGLE = ["response_type", "client_id", "redirect_uri", "scope", "state"];
+
+/** The signed parameters to display, or undefined when the link is not valid. */
+function signedDetails() {
+  const raw = signedQuery();
+  if (!raw) return undefined;
+  const signed = new URLSearchParams(raw);
+  return SINGLE.some((name) => signed.getAll(name).length > 1) ? undefined : signed;
+}
+
+/** An http(s) redirect's origin, or a native client's full redirect URI. */
+function redirectText(redirect) {
+  try {
+    const url = new URL(redirect);
+    return url.protocol === "http:" || url.protocol === "https:" ? url.origin : url.href;
+  } catch {
+    return redirect;
+  }
+}
 ...
 async function setUpConsent() {
+  const details = signedDetails();
+  if (!details) {
+    show("This authorization link is not valid. Start again from your application.", true);
+    for (const id of ["allow", "deny"]) document.getElementById(id).disabled = true;
+    return;
+  }
   const scopes = document.getElementById("scopes");
   addItem(scopes, "Convert documents with your account, its plan and its quotas");
-  for (const scope of (query.get("scope") ?? "").split(" ").filter(Boolean)) {
+  for (const scope of (details.get("scope") ?? "").split(" ").filter(Boolean)) {
     addItem(scopes, SCOPE_TEXT[scope] ?? scope);
   }
   document.getElementById("resources").textContent =
-    query.getAll("resource").join(", ") || "AriadShift";
+    details.getAll("resource").join(", ") || "AriadShift";
-  const redirect = query.get("redirect_uri");
-  if (redirect) { try { … url.origin : `${url.protocol}//` … } catch { … } }
+  const redirect = details.get("redirect_uri");
+  if (redirect) document.getElementById("redirect").textContent = redirectText(redirect);
 ...
-    const clientId = query.get("client_id");
+    const clientId = details.get("client_id");
--- services/auth/src/app.ts
+/** The path without trailing slashes, so `/oauth2/token/` is handled like `/oauth2/token`. */
+function routePath(c: Context): string {
+  return c.req.path.replace(/\/+$/, "") || "/";
+}
 ...
-    PUBLIC_OAUTH_PATHS.has(c.req.path) ? publicCors(c, next) : sessionCors(c, next);
+    PUBLIC_OAUTH_PATHS.has(routePath(c)) ? publicCors(c, next) : sessionCors(c, next);
 ...
-    return handler(forwarded(c.req.raw, c.req.path, address));
+    return handler(forwarded(c.req.raw, routePath(c), address));
 ...
-    const asset = ASSETS[c.req.param("name")];
+    const name = c.req.param("name");
+    // Own keys only: `constructor` or `__proto__` must not resolve to Object's members.
+    const asset = Object.hasOwn(ASSETS, name) ? ASSETS[name] : undefined;
```

Apply the same `routePath` to the DPoP check at `:92`. If C1 already added a normalising helper, reuse it instead of `routePath`. Move the serialized path list to an exported constant in `oauth-lock.ts` (also used by D18):

```ts
/** Token issuance and revocation, the requests this module serializes. */
export const SERIALIZED_OAUTH_PATHS: ReadonlySet<string> = new Set([
  "/api/auth/oauth2/token",
  "/api/auth/oauth2/revoke",
]);
```

and use it at `:44-47` as `!SERIALIZED_OAUTH_PATHS.has(path)` (keep the literal `/api/auth`; importing `BASE_PATH` from `auth.ts` would create a cycle; the drift test below guards it).

**Test 1**, in `app.test.ts` (add `TOKEN_PATH` to the `../src/app.ts` import and `import { SERIALIZED_OAUTH_PATHS } from "../src/oauth-lock.ts";`):

```ts
  it("serves only its own assets, never inherited object keys", async () => {
    const { app } = setup();
    for (const name of ["constructor", "toString", "__proto__", "hasOwnProperty"]) {
      expect((await app.request(`/oauth/assets/${name}`)).status, name).toBe(404);
    }
  });

  it("treats a trailing slash like the bare token path for CORS and DPoP", async () => {
    const { app, seen } = setup();
    const res = await app.request(`${TOKEN_PATH}/`, {
      method: "POST",
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        DPoP: "proof",
        Origin: INSPECTOR,
      },
      body: "grant_type=refresh_token",
    });
    expect(res.headers.get("access-control-allow-origin")).toBe("*");
    expect(seen[0]?.headers.get("dpop")).toBeNull();
  });

  it("serializes exactly the public token and revocation paths", () => {
    expect(SERIALIZED_OAUTH_PATHS.has(TOKEN_PATH)).toBe(true);
    for (const path of SERIALIZED_OAUTH_PATHS) expect(PUBLIC_OAUTH_PATHS.has(path), path).toBe(true);
  });
```

**Test 2**, new `services/auth/test/oauth-page.test.ts` (runs the real page script in `node:vm` against a stub DOM; no jsdom):

```ts
// The consent page script (pages/oauth.js) against a stub DOM. What it shows
// must come from the signed authorization parameters only, because those are
// the only ones the provider enforces.

import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";
import { ASSETS } from "../src/pages.ts";

interface StubElement {
  textContent: string;
  hidden: boolean;
  disabled: boolean;
  items: string[];
  classList: { toggle: () => void };
  addEventListener: () => void;
  append: (child: { textContent: string }) => void;
}

function stubElement(): StubElement {
  const element: StubElement = {
    textContent: "",
    hidden: false,
    disabled: false,
    items: [],
    classList: { toggle: () => {} },
    addEventListener: () => {},
    append: (child) => {
      element.items.push(child.textContent);
    },
  };
  return element;
}

const REDIRECT = "http://127.0.0.1:33418/callback";

/** A consent URL query: `extra` first, so a first-value lookup sees it. */
function consentSearch(signed: [string, string][], extra: [string, string][] = []): string {
  const query = new URLSearchParams();
  for (const [key, value] of [...extra, ...signed]) query.append(key, value);
  for (const key of new Set(signed.map(([key]) => key))) query.append("ba_param", key);
  query.append("sig", "signature");
  return `?${query}`;
}

function signed(redirect = REDIRECT): [string, string][] {
  return [
    ["response_type", "code"],
    ["client_id", "client-1"],
    ["redirect_uri", redirect],
    ["scope", "openid offline_access"],
    ["state", "s"],
  ];
}

async function renderConsent(search: string) {
  const elements = new Map<string, StubElement>();
  const byId = (id: string): StubElement => {
    let element = elements.get(id);
    if (!element) {
      element = stubElement();
      elements.set(id, element);
    }
    return element;
  };
  const answers: Record<string, unknown> = {
    "/api/auth/get-session": { user: { email: "victim@example.com" } },
    "/api/auth/oauth2/public-client": { client_name: "Registered client" },
  };
  runInNewContext((ASSETS["oauth.js"] as { body: string }).body, {
    window: {
      location: { search, href: `http://localhost:3001/oauth/consent${search}`, assign: () => {} },
    },
    document: {
      body: { dataset: { page: "consent" } },
      getElementById: byId,
      querySelectorAll: () => [byId("allow"), byId("deny")],
      createElement: () => ({ textContent: "" }),
    },
    fetch: async (input: string) => {
      const path = new URL(input, "http://localhost:3001").pathname;
      return { ok: true, status: 200, json: async () => answers[path] ?? {} };
    },
    URL,
    URLSearchParams,
  });
  // Let the page's fetch calls settle.
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  return byId;
}

describe("consent page", () => {
  it("shows an http redirect's origin and the signed scopes", async () => {
    const page = await renderConsent(consentSearch(signed()));
    expect(page("redirect").textContent).toBe("http://127.0.0.1:33418");
    expect(page("scopes").items).toContain("Stay connected for up to 30 days without signing in again");
    expect(page("client-name").textContent).toBe("Registered client");
  });

  it("ignores a resource the signed request does not carry", async () => {
    const page = await renderConsent(
      consentSearch(signed(), [["resource", "https://evil.example/mcp"]]),
    );
    expect(page("resources").textContent).toBe("AriadShift");
  });

  it("refuses a link that repeats a signed parameter", async () => {
    const page = await renderConsent(
      consentSearch(signed(), [["redirect_uri", "https://attacker.example/callback"]]),
    );
    expect(page("redirect").textContent).toBe("");
    expect(page("allow").disabled).toBe(true);
    expect(page("status").textContent).toMatch(/not valid/);
  });

  it("shows a native client's full redirect URI", async () => {
    const page = await renderConsent(consentSearch(signed("com.example.app:/oauth/callback")));
    expect(page("redirect").textContent).toBe("com.example.app:/oauth/callback");
  });
});
```

Align the stub element ids (`scopes`, `resources`, `redirect`, `client-name`, `status`, `allow`, `deny`) and the scope text with `pages/oauth.js` and `pages.ts` when pasting.

**Gate:** auth test, typecheck and biome (biome lints `oauth.js`).

**Commits:** `fix(auth): show only signed authorization details on the consent page`; `fix(auth): normalise OAuth paths and serve only own assets`.

**Risks:** unverified (library) which parameters the provider signs and whether it routes trailing-slash paths; C1's register-slash test settles the latter.

#### D18: the token and revoke path can be starved (Medium, OD-7)

**Current behaviour.** `oauth-lock.ts:54-56` admits at most 3 requests per pool before the single global `pg_advisory_lock` (`:92`, key `:4`); Better Auth's database-backed limiter runs inside the lock (README:68-71). `auth.ts:183-187` passes `(database) => betterAuth({ ...options, database }).handler`, which `oauth-lock.ts:123` calls on every token or revoke request, so a full Better Auth instance is built inside the global lock each time. There is no limit before admission; Better Auth's token rule is 20 per window (`test/oauth-security.test.ts:168-201`).

**Part 1 (ready): per-address pre-limit in the HTTP layer** (`app.ts`, where the address is computed; `oauth-lock.ts` cannot import `CLIENT_IP_HEADER` without a cycle). Set above Better Auth's 20 so existing behaviour is unchanged.

```diff
--- services/auth/src/app.ts
+import { SERIALIZED_OAUTH_PATHS } from "./oauth-lock.ts";
+
+/**
+ * Per-address budget for token issuance and revocation, checked before the
+ * serialized handler admits a request. One address therefore cannot keep the
+ * shared token lock busy with requests Better Auth would refuse anyway. It
+ * sits above Better Auth's own token rule, which still answers first.
+ */
+export const OAUTH_TOKEN_PRE_LIMIT = { window: 60, max: 30 } as const;
+
+/** Addresses tracked at once; the oldest windows are dropped first. */
+const MAX_TRACKED_ADDRESSES = 10_000;
+
+/** Returns the seconds to wait when `address` is over budget, else undefined. */
+function addressBudget() {
+  const windows = new Map<string, { start: number; count: number }>();
+  const windowMs = OAUTH_TOKEN_PRE_LIMIT.window * 1000;
+  return (address: string): number | undefined => {
+    const now = Date.now();
+    let entry = windows.get(address);
+    if (!entry || now - entry.start >= windowMs) {
+      windows.delete(address);
+      // Insertion order is window start order.
+      for (const key of windows.keys()) {
+        if (windows.size < MAX_TRACKED_ADDRESSES) break;
+        windows.delete(key);
+      }
+      entry = { start: now, count: 0 };
+      windows.set(address, entry);
+    }
+    entry.count += 1;
+    if (entry.count <= OAUTH_TOKEN_PRE_LIMIT.max) return undefined;
+    return Math.max(1, Math.ceil((entry.start + windowMs - now) / 1000));
+  };
+}
+
+function tooManyTokenRequests(seconds: number): Response {
+  const retry = String(seconds);
+  return Response.json(
+    { error: "temporarily_unavailable", error_description: "Too many requests. Please retry later." },
+    {
+      status: 429,
+      headers: { "Retry-After": retry, "X-Retry-After": retry, "Cache-Control": "no-store", Pragma: "no-cache" },
+    },
+  );
+}
 ...
 export function createApp(...) {
+  const overBudget = addressBudget();
 ...
   app.on(["GET", "POST"], routes, (c) => {
+    const path = routePath(c);
     const address = clientAddress(c.req.raw.headers, peerAddress(c), config.trustedClientIpHeader);
-    return handler(forwarded(c.req.raw, c.req.path, address));
+    if (c.req.method === "POST" && SERIALIZED_OAUTH_PATHS.has(path)) {
+      const wait = overBudget(address ?? "unknown");
+      if (wait !== undefined) return tooManyTokenRequests(wait);
+    }
+    return handler(forwarded(c.req.raw, path, address));
   });
```

**Part 3 (ready): build the bound provider once per handler**, routing each request's queries to its own locked connection through `AsyncLocalStorage`:

```diff
--- services/auth/src/oauth-lock.ts
+import { AsyncLocalStorage } from "node:async_hooks";
 import type pg from "pg";
+type Query = (text: string, values?: unknown[]) => Promise<unknown>;
 ...
 export function serializedOAuthHandler(pool, handler, boundHandler, limits = OAUTH_TOKEN_LIMITS): Handler {
+  // One provider for every serialized request. Each request's queries reach
+  // its own locked connection through the async context, so Better Auth is
+  // no longer rebuilt inside the global lock for every token request.
+  const current = new AsyncLocalStorage<Query>();
+  const database = {
+    connect: async () => ({
+      query: (text: string, values?: unknown[]) => {
+        const query = current.getStore();
+        return query ? query(text, values) : Promise.reject(new Error("OAuth request expired"));
+      },
+      release() {},
+    }),
+    end: async () => {},
+  } as unknown as pg.Pool;
+  let bound: Handler | undefined;
   return async (request) => {
 ...
-      const database = {
-        connect: async () => ({ query, release() {} }),
-        end: async () => {},
-      } as unknown as pg.Pool;
-      const response = await boundHandler(database)(buffered);
+      const provider = (bound ??= boundHandler(database));
+      const response = await current.run(query, () => provider(buffered));
 ...
     } catch {
+      // A provider whose first request failed mid-start may hold a rejected
+      // start-up promise; build a fresh one next time.
+      bound = undefined;
       destroy();
       return unavailable();
```

Also set `bound = undefined` in the `if (failed) return unavailable();` branch.

**Part 2 (blocked on OD-7 and a library check): key the advisory lock per `client_id`.** It needs `client_id` before admission (form body or HTTP Basic, falling back to the global key), rewrites every lock assertion in `oauth-postgres.test.ts`, and is safe only if Better Auth never writes to a refresh family when a mismatched `client_id` is presented (unverified (library)). Recommendation: accept the global lock for v1 (OD-7 option a); parts 1 and 3 remove the cheap starvation and the per-request build.

**README** (the "Better Auth 1.7.12 runs its database-backed rate limiter…" paragraph): add that a per-address pre-limit of 30 token and revoke requests per minute runs before admission, and that the bound provider is built once per process.

**Test 1**, in `app.test.ts` inside `describe("createApp")` (add `TOKEN_PATH`, `OAUTH_TOKEN_PRE_LIMIT` to the app import and `vi` to the vitest import):

```ts
  it("refuses token and revocation floods from one address before the provider", async () => {
    const { app, seen } = setup("x-real-ip");
    const send = (path: string, address: string) =>
      app.request(path, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded", "x-real-ip": address },
        body: "grant_type=refresh_token",
      });
    for (let i = 0; i < OAUTH_TOKEN_PRE_LIMIT.max; i++) {
      expect((await send(TOKEN_PATH, "198.51.100.7")).status).toBe(200);
    }
    const refused = await send("/api/auth/oauth2/revoke", "198.51.100.7");
    expect(refused.status).toBe(429);
    expect(Number(refused.headers.get("retry-after"))).toBeGreaterThan(0);
    expect(await refused.json()).toMatchObject({ error: "temporarily_unavailable" });
    expect(seen).toHaveLength(OAUTH_TOKEN_PRE_LIMIT.max);
    // Other addresses and other endpoints are unaffected.
    expect((await send(TOKEN_PATH, "198.51.100.8")).status).toBe(200);
    expect((await send("/api/auth/sign-in/email", "198.51.100.7")).status).toBe(200);
    expect(seen).toHaveLength(OAUTH_TOKEN_PRE_LIMIT.max + 2);
    // The budget comes back after the window.
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(Date.now() + OAUTH_TOKEN_PRE_LIMIT.window * 1000 + 1_000);
      expect((await send(TOKEN_PATH, "198.51.100.7")).status).toBe(200);
    } finally {
      vi.useRealTimers();
    }
  });
```

**Test 2**, new `services/auth/test/oauth-lock.test.ts` (no database; runs in the web job). Before the fix `built` is 4.

```ts
// The serialized token handler over a stub pool: no database needed.
import type pg from "pg";
import { describe, expect, it } from "vitest";
import { serializedOAuthHandler } from "../src/oauth-lock.ts";

const TOKEN_URL = "http://localhost:3001/api/auth/oauth2/token";
const MARKER = "SELECT $1::text AS marker";

/** A pool whose clients record their queries. */
function stubPool() {
  const clients: string[][] = [];
  const pool = {
    async connect() {
      const queries: string[] = [];
      clients.push(queries);
      return {
        async query(text: string, values?: unknown[]) {
          queries.push(values?.length ? `${text} ${JSON.stringify(values)}` : text);
          return { rows: [] };
        },
        on() {},
        once() {},
        removeListener() {},
        release() {},
      };
    },
  } as unknown as pg.Pool;
  return { pool, clients };
}

describe("serializedOAuthHandler", () => {
  it("builds the provider once and keeps each request on its own locked connection", async () => {
    const { pool, clients } = stubPool();
    let built = 0;
    const handler = serializedOAuthHandler(
      pool,
      async () => new Response("not serialized"),
      (database) => {
        built += 1;
        return async (request) => {
          const connection = await database.connect();
          await connection.query(MARKER, [await request.text()]);
          return new Response("ok");
        };
      },
    );
    const send = (marker: string) =>
      handler(new Request(TOKEN_URL, { method: "POST", body: marker }));
    const responses = await Promise.all([send("a"), send("b"), send("c")]);
    expect(responses.map((response) => response.status)).toEqual([200, 200, 200]);
    expect((await send("d")).status).toBe(200);
    expect(built).toBe(1);
    const markers = clients.map((queries) => queries.filter((q) => q.startsWith(MARKER)));
    expect(markers.every((found) => found.length === 1)).toBe(true);
    expect(markers.flat().sort()).toEqual(
      ["a", "b", "c", "d"].map((m) => `${MARKER} ${JSON.stringify([m])}`),
    );
  });
});
```

If the lock code issues `pg_try_advisory_lock` and expects a row like `{ locked: true }`, adjust the stub's `rows` accordingly.

**Test 3**, in the postgres `describe` of `oauth-postgres.test.ts`. First extend `createService` in `oauth-helpers.ts` with a third parameter `peer = "127.0.0.1"` passed as `peerAddress: () => peer`; import `OAUTH_TOKEN_PRE_LIMIT` from `../src/app.ts`.

```ts
  it("keeps one flooding address out of token admission", async () => {
    const legit = createService(first);
    const attacker = createService(first, false, "198.51.100.7");
    const flow = await browserFlow(legit.app);
    const issued = await flow.issue();
    const junk = { grant_type: "refresh_token", client_id: "missing", refresh_token: "missing" };
    for (let i = 0; i < OAUTH_TOKEN_PRE_LIMIT.max; i++) await token(attacker.app, junk);
    const gate = await observer.connect();
    await gate.query("SELECT pg_advisory_lock($1)", [OAUTH_TOKEN_LOCK_KEY]);
    let pending: Promise<Response> | undefined;
    try {
      // Without the pre-limit these would be admitted and wait on the lock,
      // filling the per-process capacity.
      const flood = await Promise.all(
        Array.from({ length: OAUTH_TOKEN_LIMITS.capacity }, () => token(attacker.app, junk)),
      );
      expect(flood.map((response) => response.status)).toEqual(
        Array(OAUTH_TOKEN_LIMITS.capacity).fill(429),
      );
      pending = flow.refresh(issued.refresh_token as string);
      await waiting(OAUTH_TOKEN_LOCK_KEY);
    } finally {
      await gate.query("SELECT pg_advisory_unlock($1)", [OAUTH_TOKEN_LOCK_KEY]);
      gate.release();
    }
    expect((await (pending as Promise<Response>)).status).toBe(200);
  }, 15_000);
```

`first`, `observer`, `waiting`, `OAUTH_TOKEN_LOCK_KEY`, `OAUTH_TOKEN_LIMITS` are the suite's existing names; confirm them.

**Gate:** `pnpm --filter @ariadshift/auth typecheck`; `pnpm --filter @ariadshift/auth test`; `pnpm exec biome check services/auth`; `ARIAD_TEST_DATABASE_URL=… ARIAD_TEST_REQUIRE_DATABASE=1 pnpm --filter @ariadshift/auth exec vitest run test/oauth-postgres.test.ts` (disposable PG18 with pgmq and `pg_dump` 18).

**Commits:** `fix(auth): limit token requests per address before the shared lock`; `perf(auth): build the serialized OAuth provider once per process`.

**Risks:** the pre-limit is per process and per exact address, so IPv6 /64 rotation bypasses it. Background queries Better Auth might issue after a request ends now fail (they already failed under the per-request shim).

#### D19: plain-HTTP LAN deployments no longer start (Medium, OD-9)

**Current behaviour.** `config.ts:92-98` `requireTls` throws `ConfigError` for `BETTER_AUTH_URL` (`:147-150`) or `ARIAD_PUBLIC_URL` (`:153-156`) when either is non-loopback HTTP. In compose, `db-grants` and `api` wait for a healthy `auth` (`:240-244,288-291`), and `worker-core` waits for `api` (`:395-397`), so a `BETTER_AUTH_URL=http://nas.local:3001` stack that runs on dev stays down. `test/config.test.ts:87-105` pins the throw.

**Variant (a), recommended:** turn the OAuth provider off and keep sessions, JWTs and API keys.

```diff
--- services/auth/src/config.ts
 export interface AuthServiceConfig {
 ...
   google: OAuthClient | undefined;
+  /**
+   * Why the OAuth provider for remote MCP is off, or undefined when it is on.
+   * OAuth 2.1 and MCP need TLS outside loopback, so a plain-HTTP LAN origin
+   * turns remote MCP sign-in off while accounts, sessions, JWTs and API keys
+   * keep working.
+   */
+  oauthDisabledReason: string | undefined;
 }
-function requireTls(name: string, origin: string): string {
-  const url = new URL(origin);
-  if (url.protocol !== "https:" && !isLoopbackHost(url.hostname)) {
-    throw new ConfigError(`${name} must use https unless it is a loopback host: ${origin}`);
-  }
-  return origin;
-}
+function tlsProblem(name: string, origin: string): string | undefined {
+  const url = new URL(origin);
+  return url.protocol !== "https:" && !isLoopbackHost(url.hostname)
+    ? `${name} must use https unless it is a loopback host: ${origin}`
+    : undefined;
+}
 ...
-  const baseUrl = requireTls("BETTER_AUTH_URL", parseOrigin("BETTER_AUTH_URL", required(env, "BETTER_AUTH_URL")));
+  const baseUrl = parseOrigin("BETTER_AUTH_URL", required(env, "BETTER_AUTH_URL"));
-  const apiOrigin = requireTls("ARIAD_PUBLIC_URL", parseOrigin("ARIAD_PUBLIC_URL", required(env, "ARIAD_PUBLIC_URL")));
+  const apiOrigin = parseOrigin("ARIAD_PUBLIC_URL", required(env, "ARIAD_PUBLIC_URL"));
 ...
     google: parseOAuth(env, "GOOGLE"),
+    oauthDisabledReason:
+      tlsProblem("BETTER_AUTH_URL", baseUrl) ?? tlsProblem("ARIAD_PUBLIC_URL", apiOrigin),
   };
--- services/auth/src/auth.ts (inside plugins, after C1's rateLimit line)
-      oauthProvider({ … }),
+      // Off when an origin is plain HTTP outside loopback (see parseConfig).
+      ...(config.oauthDisabledReason ? [] : [oauthProvider({ … })]),
--- services/auth/src/app.ts
-  metadata: (request: Request, document: DiscoveryDocument) => Promise<object>;
+  /** Better Auth's discovery document; absent when the OAuth provider is off. */
+  metadata?: (request: Request, document: DiscoveryDocument) => Promise<object>;
 ...
+  if (metadata) {
+    // Discovery and the OAuth pages exist only with the provider.
     app.use("/.well-known/*", publicCors);
     for (const document of DISCOVERY_DOCUMENTS) { … }
     app.get(OAUTH_SIGN_IN_PATH, …); app.get(OAUTH_CONSENT_PATH, …); app.get(`${ASSET_PATH}/:name`, …);
+  }
--- services/auth/src/main.ts
   const auth = createAuth(options);
+  if (config.oauthDisabledReason) {
+    log("info", "remote MCP OAuth is off", { reason: config.oauthDisabledReason });
+  }
 ...
-    metadata: discoveryDocuments(auth),
+    metadata: config.oauthDisabledReason ? undefined : discoveryDocuments(auth),
```

Docs: the `services/auth/README.md` configuration table, `docs/self-hosting.md` (TLS line; C3 also touches it) and the `.env.example` comment on `BETTER_AUTH_URL`: remote MCP OAuth needs https on both origins outside loopback; otherwise the service starts with OAuth off and logs why.

**Variant (b):** the same code, but the provider turns off only when `AUTH_OAUTH_PROVIDER=off`, keeping the `ConfigError` as the default (add the variable to `parseConfig`, `.env.example` and compose). **Variant (c):** an upgrade note only.

**Tests (variant a).** Replace `config.test.ts:87-105`:

```ts
  it("turns the OAuth provider off, instead of refusing to start, on plain-HTTP LAN origins", () => {
    for (const name of ["BETTER_AUTH_URL", "ARIAD_PUBLIC_URL"]) {
      for (const lan of ["http://auth.example.com", "http://192.168.1.20:3001", "http://nas.local:3001"]) {
        const config = parseConfig(env({ [name]: lan }));
        expect(config.oauthDisabledReason, `${name}=${lan}`).toMatch(/must use https/);
      }
      for (const local of [
        "http://localhost:4000",
        "http://auth.localhost",
        "http://127.0.0.2:4000",
        "http://[::1]:4000",
        "https://example.com",
      ]) {
        expect(parseConfig(env({ [name]: local })).oauthDisabledReason, `${name}=${local}`).toBeUndefined();
      }
    }
  });
```

Add to the OAuth `describe` in `auth.test.ts` (uses that file's `config(...)` helper and `pool`):

```ts
    it("leaves the OAuth provider out when an origin is plain HTTP outside loopback", () => {
      const ids = buildAuthOptions(config({ BETTER_AUTH_URL: "http://nas.local:3001" }), pool).plugins.map(
        (p) => p.id,
      );
      expect(ids).not.toContain("oauth-provider");
      expect(ids).toEqual(expect.arrayContaining(["jwt", "api-key", "passkey", "two-factor"]));
    });
```

Add to `app.test.ts`:

```ts
  it("serves no OAuth discovery or pages when the provider is off", async () => {
    const app = createApp({
      config: { trustedOrigins: [WEB], trustedClientIpHeader: undefined },
      handler: async () => Response.json({ ok: true }),
      pages: { socialProviders: [], passkeys: false },
      peerAddress: () => "203.0.113.7",
    });
    for (const path of [
      "/.well-known/oauth-authorization-server",
      "/.well-known/openid-configuration",
      "/oauth/sign-in",
      "/oauth/consent",
      "/oauth/assets/oauth.js",
    ]) {
      expect((await app.request(path)).status, path).toBe(404);
    }
    expect((await app.request("/api/auth/get-session")).status).toBe(200);
  });
```

**Gate:** auth typecheck, test, biome; then `docker compose … config` with `BETTER_AUTH_URL=http://nas.local:3001` and confirm `auth` reaches healthy.

**Commit:** `fix(auth): start without the OAuth provider on plain-HTTP LAN origins`

**Risks:** a mistyped `http://` on an HTTPS deployment silently loses remote MCP (only the log line signals it; variant (b) exists for that). A conditional plugin array may narrow Better Auth's inferred `auth.api` type (typecheck shows it). With OAuth off, `ariad-server` still advertises RFC 9728 metadata and MCP clients fail at discovery with 404; document it.

#### D22: social sign-in can link into an unverified password account (Medium, library unverified)

**Current behaviour.** `auth.ts:102-106` sets `requireEmailVerification: false`, and there is no `account`/`accountLinking` block (verified). No explicit link UI exists on hardening or #84. Whether 1.7.12 links implicitly when the provider reports the email verified is unverified (library). Exposure exists only when GitHub or Google credentials are configured.

**Step 0 (ASUS):** read the installed social-callback linking code (`handleOAuthUserInfo` in 1.x) and confirm that `account.accountLinking.enabled` exists in 1.7.12's `BetterAuthOptions`.

**Change** (after `socialProviders`, and after #84's `user` block once D0 has landed):

```diff
     socialProviders: socialProviders(config),
+    account: {
+      // Addresses are never verified here (no mail transport), so whoever
+      // registers an email first must not gain the social login of its owner.
+      // No page offers explicit linking, so linking stays off entirely.
+      accountLinking: { enabled: false },
+    },
```

**Test**, new `services/auth/test/oauth-linking.test.ts` (the first test is a control proving the stub path works):

```ts
// Social sign-in against an existing password account, with GitHub's OAuth
// endpoints stubbed. The address was never verified, so a GitHub login for
// it must not be attached to that account.

import { memoryAdapter } from "better-auth/adapters/memory";
import { getAuthTables } from "better-auth/db";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createApp } from "../src/app.ts";
import { buildAuthOptions, createAuth, discoveryDocuments } from "../src/auth.ts";
import { parseConfig } from "../src/config.ts";
import { pageOptions } from "../src/pages.ts";
import { AUTH, Jar, json, post } from "./oauth-helpers.ts";

afterEach(() => vi.restoreAllMocks());

function serviceWithGitHub() {
  const config = parseConfig({
    DATABASE_URL: "postgres://unused@127.0.0.1:1/none",
    BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
    BETTER_AUTH_URL: AUTH,
    BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
    ARIAD_PUBLIC_URL: "http://localhost:8080",
    GITHUB_CLIENT_ID: "github-client",
    GITHUB_CLIENT_SECRET: "github-secret",
  });
  const probe = buildAuthOptions(config, memoryAdapter({}));
  const db: Record<string, unknown[]> = Object.fromEntries(
    Object.values(getAuthTables(probe)).map((table) => [table.modelName, []]),
  );
  const options = buildAuthOptions(config, memoryAdapter(db));
  const auth = createAuth({ ...options, rateLimit: { ...options.rateLimit, enabled: false } });
  const app = createApp({
    config,
    handler: (request) => auth.handler(request),
    metadata: discoveryDocuments(auth),
    pages: pageOptions(config),
    peerAddress: () => "127.0.0.1",
  });
  return { app, db };
}

/** GitHub answers as if the user owns `email` and GitHub verified it. */
function stubGitHub(email: string) {
  const realFetch = globalThis.fetch;
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (url.startsWith("https://github.com/login/oauth/access_token")) {
      return Response.json({ access_token: "gho_test", token_type: "bearer", scope: "read:user,user:email" });
    }
    if (url.startsWith("https://api.github.com/user/emails")) {
      return Response.json([{ email, primary: true, verified: true, visibility: "private" }]);
    }
    if (url.startsWith("https://api.github.com/user")) {
      return Response.json({ id: 4242, login: "owner", name: "Owner", email, avatar_url: "" });
    }
    return realFetch(input, init);
  });
}

async function gitHubCallback(app: ReturnType<typeof serviceWithGitHub>["app"]) {
  const browser = new Jar();
  const start = await post(app, "/api/auth/sign-in/social", { provider: "github", callbackURL: "/" });
  expect(start.status).toBe(200);
  browser.store(start);
  const state = new URL((await json(start)).url as string).searchParams.get("state") ?? "";
  return app.request(
    `${AUTH}/api/auth/callback/github?code=test-code&state=${encodeURIComponent(state)}`,
    { headers: browser.header() },
  );
}

const gitHubAccounts = (db: Record<string, unknown[]>) =>
  (db.account ?? []).filter((row) => (row as { providerId?: string }).providerId === "github");

describe("social sign-in and existing accounts", () => {
  it("signs in a new user with GitHub", async () => {
    const { app, db } = serviceWithGitHub();
    stubGitHub("new-user@example.com");
    const callback = await gitHubCallback(app);
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).not.toContain("error=");
    expect(gitHubAccounts(db)).toHaveLength(1);
  });

  it("never attaches a GitHub login to an unverified password account with the same email", async () => {
    const { app, db } = serviceWithGitHub();
    const email = "owner@example.com";
    // Someone else registered the address first, with a password nobody verified.
    const squatter = await post(app, "/api/auth/sign-up/email", {
      name: "Squatter",
      email,
      password: "correct horse battery staple",
    });
    expect(squatter.status).toBe(200);
    stubGitHub(email);
    const callback = await gitHubCallback(app);
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).toContain("error=");
    expect(gitHubAccounts(db)).toHaveLength(0);
  });
});
```

The secret in `serviceWithGitHub` is a test constant, as in `oauth-helpers.ts`. Unverified (library): the GitHub token and user URLs and the state-cookie handling; adjust URL matching to the installed source. Check that `parseConfig` accepts this env set (compare `oauth-helpers.ts:14`).

**Gate:** auth test and typecheck.

**Commit:** `fix(auth): never link a social login into an unverified account`

**Risk:** password users cannot add a social login for the same email (a product trade-off; see §4). A finer rule (link only when the local email is verified, through `databaseHooks.account.create.before`) is equivalent on this stack because nothing verifies a local email.

#### D20 guard (W7 side)

```diff
--- services/auth/test/oauth-postgres.test.ts
-// Set ARIAD_TEST_DATABASE_URL to a disposable PostgreSQL administrator URL,
-// as in `just test-server`. This suite creates and removes its own database.
+// Set ARIAD_TEST_DATABASE_URL to a disposable PostgreSQL administrator URL,
+// as `just test-auth-db` does in CI. This suite creates and removes its own
+// database. ARIAD_TEST_REQUIRE_DATABASE=1 turns a missing URL into a failure.
 ...
 const databaseUrl = process.env.ARIAD_TEST_DATABASE_URL;
+if (process.env.ARIAD_TEST_REQUIRE_DATABASE === "1" && !databaseUrl) {
+  throw new Error("ARIAD_TEST_REQUIRE_DATABASE=1 needs ARIAD_TEST_DATABASE_URL");
+}
```

Update `services/auth/README.md:115` to name `just test-auth-db` and the CI job. Commit: `test(auth): fail the PostgreSQL suite when CI asks for a database`.

---

### W8 CI and e2e wiring (after D0; order D24, D20, D13 f–g)

#### D24: e2e credentials on argv, no volume reset, provider version outside the catalog (Low)

**Current behaviour.** `infra/test/e2e/mcp-oauth.sh` puts credentials in curl/jq argv, readable by other local users through `ps`: the bearer token (`:177`, API JWT at `:389`, `:397`), the password (`:243`, `:329`), code and verifier (`:354-357`), refresh token (`:423`). It runs `compose down --volumes` only in `cleanup` (`:91`), never before `compose up` (`:225`), so a killed run's volume keeps an old database password. Indentation is mixed (`:114-133`, `:385-418`). `services/auth/package.json:20` pins `"@better-auth/oauth-provider": "1.7.12"` while siblings use `catalog:` (`pnpm-workspace.yaml:10-11,27`). Constraint: `oauth-diagnostics.test.ts` slices the script from `diagnostic_body() {` to `# expect WANT GOT WHAT`; keep both markers.

**Change:**

```diff
--- infra/test/e2e/mcp-oauth.sh
+# private_file NAME VALUE: writes VALUE to $WORK/NAME with no trailing newline
+# and prints the path. Credentials reach curl and jq through files and the
+# environment, never argv, which other local users can read through ps.
+private_file() {
+	printf '%s' "$2" >"$WORK/$1"
+	printf '%s' "$WORK/$1"
+}
 ...mcp():
-		-H "authorization: Bearer $1" \
+		-H @"$(private_file bearer "authorization: Bearer $1")" \
 ...
 PASSWORD=$(random_hex)
+export PASSWORD
 ...sign-up:
-	--data "$(jq -nc --arg e "$EMAIL" --arg p "$PASSWORD" '{name: "E2E", email: $e, password: $p}')")
+	--data @"$(jq -nc --arg e "$EMAIL" '{name: "E2E", email: $e, password: $ENV.PASSWORD}' >"$WORK/sign-up.json"; printf '%s' "$WORK/sign-up.json")")
 ...sign-in: the same, keeping --arg q and taking the password from $ENV.PASSWORD
 ...token exchange:
-	--data-urlencode "code=$CODE" \
+	--data-urlencode "code@$(private_file code "$CODE")" \
-	--data-urlencode "code_verifier=$VERIFIER" \
+	--data-urlencode "code_verifier@$(private_file verifier "$VERIFIER")" \
 ...uploads (:389, :397):
-  -H "authorization: Bearer $API_JWT" \
+	-H @"$(private_file api-auth "authorization: Bearer $API_JWT")" \
 ...refresh:
-	--data-urlencode "refresh_token=$REFRESH_TOKEN" \
+	--data-urlencode "refresh_token@$(private_file refresh "$REFRESH_TOKEN")" \
 ...
 STACK_STARTED=1
+# A killed earlier run can leave volumes whose database password no longer
+# matches this run's environment, so start from nothing.
+compose down --volumes --remove-orphans --timeout 30 >/dev/null 2>&1 || true
 ...before "registering a public client":
+# Registration is limited per client address (20 per hour). The limiter's
+# rows live in this run's database, which starts empty, so reruns are safe.
```

Reindent `:114-133` and `:385-418` with tabs. Then:

```diff
--- pnpm-workspace.yaml (catalog, alphabetical)
   "@better-auth/api-key": 1.7.12
+  "@better-auth/oauth-provider": 1.7.12
   "@better-auth/passkey": 1.7.12
--- services/auth/package.json
-    "@better-auth/oauth-provider": "1.7.12",
+    "@better-auth/oauth-provider": "catalog:",
```

Run `pnpm install` so the importer specifier in `pnpm-lock.yaml` becomes `'catalog:'`; commit the lockfile with the manifests.

**Test**, in `services/auth/test/oauth-diagnostics.test.ts` (already imports `readFileSync`, `expect`, `it`):

```ts
it("keeps e2e credentials off the command line and starts from empty volumes", () => {
  const script = readFileSync(
    new URL("../../../infra/test/e2e/mcp-oauth.sh", import.meta.url),
    "utf8",
  );
  for (const leak of [
    /-H "authorization: Bearer \$/,
    /--arg p "\$PASSWORD"/,
    /"code_verifier=\$VERIFIER"/,
    /"refresh_token=\$REFRESH_TOKEN"/,
  ]) {
    expect(script, String(leak)).not.toMatch(leak);
  }
  const stack = script.indexOf("# --- Stack");
  const reset = script.indexOf("compose down --volumes --remove-orphans", stack);
  expect(stack).toBeGreaterThan(-1);
  expect(reset).toBeGreaterThan(stack);
  expect(reset).toBeLessThan(script.indexOf("compose up -d", stack));
});
```

Confirm the `# --- Stack` marker text in the script.

**Gate:** `pnpm --filter @ariadshift/auth exec vitest run test/oauth-diagnostics.test.ts`; `bash -n infra/test/e2e/mcp-oauth.sh`; `just mcp-oauth-e2e` (Docker, ASUS); `just js`; `just js-licenses`.

**Commits:** `test(e2e): keep MCP OAuth credentials off the command line`; `build(auth): take the OAuth provider version from the pnpm catalog`.

**Risks:** curl `-H @file` needs 7.55+, and `--data-urlencode name@file` needs no trailing newline (`printf '%s'`); `$ENV` needs jq 1.6+.

#### D20 CI side: the PostgreSQL lock suite never runs in CI, and the MCP OAuth e2e is in no workflow (Medium)

**Current behaviour.** `oauth-postgres.test.ts:19-22` is `describe.skipIf(!databaseUrl)`, and the root `vitest.config.ts` runs it in the web job (no database), so it silently skips. The CI `server` job (`ci.yml:271-362`) sets `ARIAD_TEST_DATABASE_URL` (`:297`) but runs only cargo steps (`:345-362`), with no Node or `pg_dump`. `just test-server` (`justfile:184-226`) runs only nextest. `just mcp-oauth-e2e` (`justfile:232-233`) is in no workflow. `cloud-e2e.yml` exists only on #84 and arrives through D0.

**Change:**

```diff
--- justfile (next to test-server)
+# The auth service's PostgreSQL suite (OAuth token lock); needs ARIAD_TEST_DATABASE_URL and pg_dump 18.
+test-auth-db:
+    ARIAD_TEST_REQUIRE_DATABASE=1 pnpm --filter @ariadshift/auth exec vitest run test/oauth-postgres.test.ts
--- .github/workflows/ci.yml, server job, after "Check images and run a required-mode Pandoc container job"
+      - name: Set up pnpm
+        uses: pnpm/action-setup@ea17c68df8912ef543352723c149a84f56e3d413 # v6.1.0
+        with:
+          version: 12.9.1
+          run_install: false
+
+      - name: Set up Node.js
+        uses: actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1 # v7.1.0
+        with:
+          node-version-file: .node-version
+          cache: pnpm
+          cache-dependency-path: pnpm-lock.yaml
+
+      - name: Install JavaScript dependencies
+        run: just js
+
+      # The suite takes pg_dump backups, which must match the server's major version.
+      - name: Install the PostgreSQL 18 client
+        run: |
+          sudo apt-get update
+          version="$(apt-cache policy postgresql-client-18 | awk '/Candidate:/ {print $2}')"
+          [[ -n "$version" && "$version" != '(none)' ]]
+          printf 'Selected postgresql-client-18=%s\n' "$version"
+          sudo apt-get install --no-install-recommends -y "postgresql-client-18=$version"
+          pg_dump --version | grep -q ' 18\.'
+
+      - name: Run the auth service's PostgreSQL suite
+        run: just test-auth-db
--- .github/workflows/cloud-e2e.yml (after D0; follow the concurrency layout B10 left)
+  mcp-oauth:
+    name: Remote MCP OAuth (core)
+    needs: scope
+    if: needs.scope.outputs.run == 'true'
+    runs-on: ubuntu-26.04
+    timeout-minutes: 60
+    permissions:
+      contents: read
+    steps:
+      - name: Check out source
+        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
+        with:
+          ref: ${{ github.event_name == 'schedule' && 'dev' || '' }}
+          persist-credentials: false
+      - name: Run the MCP OAuth end-to-end test
+        env:
+          E2E_WAIT_TIMEOUT: "900"
+        run: bash infra/test/e2e/mcp-oauth.sh
```

Copy the action pins and the pnpm version from the existing `web` job in `ci.yml` rather than from this sketch, and match `cloud-e2e.yml`'s actual `scope` job and output names. `just js` is `pnpm install --frozen-lockfile` (`justfile:55-56`). If Ubuntu 26.04 lacks `postgresql-client-18` (unverified), use PGDG: `/usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y`. In `docs/ci.md`, say in item 5 (`:34`) that the server job also runs `just test-auth-db`, add a row for it near `:86-87`, and mention the new Cloud E2E job in the section #84 brings.

**Proof:** the server job log shows "PostgreSQL OAuth serialization across replicas" with tests passed, not skipped; `ARIAD_TEST_DATABASE_URL= just test-auth-db` fails (needs W7's guard).

**Gate:** `just lint-workflows`; `ARIAD_TEST_DATABASE_URL= just test-auth-db` (must fail); `just test-auth-db` against the local pgmq container (must pass); `bash -n infra/test/e2e/mcp-oauth.sh`.

**Commits:** `ci: run the auth PostgreSQL suite in the server job`; `ci: run the MCP OAuth end-to-end test with the compose stack`.

**Risks:** about 2 minutes more on the server job; the e2e job builds three images (15–25 minutes).

#### D13 f–g: `just fuzz` target list and nightly cache saves (Low)

**(f)** The default list at `justfile:279` has 9 targets; CI runs 18 (`ci.yml:425`). Derive it from the crate (the existing `case` covers the only two targets with `required-features`, `fuzz/Cargo.toml:84,92`):

```diff
-        targets=("markdown_reader" "front_matter" "html_reader" "pandoc_ast_to_ir" "docling_to_ir" "ir_json" "limits_validate" "pdf_reader" "typst_writer")
+        mapfile -t targets < <(cargo fuzz list)
```

Leave the `ci.yml` list hardcoded here (D20 edits that file in the same worker; a later change may unify them).

**(g)** `fuzz-nightly.yml` uses `actions/cache` at `:58-62` (cargo-fuzz) and `:70-76` (corpus), which saves on every event, and nine legs race on one cargo-fuzz key. Split restore and save; save only on schedule:

```diff
-      - name: Cache cargo-fuzz
-        uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
+      - name: Restore cargo-fuzz
+        id: restore-cargo-fuzz
+        uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
         with:
           path: ~/.cargo/bin/cargo-fuzz
           key: cargo-fuzz-0.13.2-${{ runner.os }}
@@ (after Install cargo-fuzz)
+      - name: Save cargo-fuzz binary
+        if: github.event_name == 'schedule' && matrix.target == 'markdown_reader' && steps.restore-cargo-fuzz.outputs.cache-hit != 'true'
+        uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
+        with:
+          path: ~/.cargo/bin/cargo-fuzz
+          key: cargo-fuzz-0.13.2-${{ runner.os }}
@@
-      - name: Cache fuzz corpus
-        uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
+      - name: Restore fuzz corpus
+        uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
         with:
           path: fuzz/corpus/${{ matrix.target }}
           key: fuzz-nightly-corpus-${{ runner.os }}-${{ matrix.target }}-${{ github.run_id }}
           restore-keys: |
             fuzz-nightly-corpus-${{ runner.os }}-${{ matrix.target }}-
@@ (after Run AddressSanitizer fuzzing)
+      - name: Save fuzz corpus
+        if: github.event_name == 'schedule' && !cancelled()
+        uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
+        with:
+          path: fuzz/corpus/${{ matrix.target }}
+          key: fuzz-nightly-corpus-${{ runner.os }}-${{ matrix.target }}-${{ github.run_id }}
```

`docs/ci.md:64`: "The nightly workflow (`fuzz-nightly.yml`) restores caches on every run and saves them only from scheduled runs; one matrix leg saves the cargo-fuzz binary."

**Gate:** `just fuzz` with no argument runs all 18 targets (Linux); `just lint-workflows`; `just spell`.

**Commits:** `build(fuzz): run every fuzz target from just fuzz`; `ci(fuzz): save nightly fuzz caches only from scheduled runs`.

**Risks:** `cargo fuzz list` must run from the repo root (it finds `./fuzz`). A dispatch run no longer warms the cache.

---

### W9 architecture (last)

#### D27: ARCHITECTURE.md is stale on isolation, remote MCP, desktop plugins and OAuth (Medium, docs), plus D13 e

**Current behaviour.** Every line D27 names is still stale. Lines 1–946 are byte-identical on dev and hardening, so these line numbers survive D0 (only the Decision Log tail moves down two rows with dev's Docling rows from #51). Distribution commit `8eb1ef7` ("docs(architecture): mark the design as v1 and log v1 decisions", 2026-10-25, on `origin/feat/v1-distribution`) already holds text for about half of D27, written against the hardening code. Copying it word for word also shrinks the F0/H-8 conflict.

**Copy word for word from `git show 8eb1ef7 -- ARCHITECTURE.md`:**

| Line | Stale text | Code fact (hardening) | Action |
|---|---|---|---|
| 186 (surface table) | no remote MCP row | `crates/ariad-server/src/mcp/mod.rs:1,111` | copy the **Remote MCP** `/mcp` row |
| 223 | "with the Linux sandbox described in §11.3" | `sandbox.rs:24,430` macOS module | "with the Linux and macOS sandboxes described in §11.3" |
| after 223 | no remote MCP paragraph | `tools.rs:330,402-426`; `mod.rs:34,53-55` | copy the paragraph "`ariad-server api` also serves a remote MCP endpoint at `/mcp`: …" |
| 557 | "Operating system integration in later phases: … `IExplorerCommand` and sparse packaging…" | `9734cfc`; `os-integration.wxs:8-153`; `docs/desktop.md:119-152` | copy the "Operating system integration" bullet. OD-10 placeholder: if the MSI moves to HKLM (D26b), say so. If D8 lands as docs-only, append "On Windows, a cold start with many selected files can open more than one window." |
| 712 | "reports effective mode, Landlock ABI and seccomp availability;" | `macos.rs:14-29`; `docs/cli.md:513-518` | "…Landlock ABI and seccomp availability on Linux, and Seatbelt availability and enforcement on macOS;" |
| 714 | "**macOS / Windows:** … The Linux launcher is not used; `required` mode is unavailable. … Seatbelt remains later hardening work." | `cc4963c`; `macos.rs:61-130`; `macos_profile.rs:7-27`; `sandbox.rs:290`; `runner.rs:435-437` | replace with the two 8eb1ef7 bullets **macOS:** and **Windows:**, plus the additions below |
| Decision Log tail | no rows | as above | copy the rows **macOS engine sandbox**, **Desktop OS integration** and **Remote MCP** |

Optional (owner OK): 8eb1ef7's **`/v1` stability** and **Job listing** rows also describe hardening code. Do not copy the distribution-only parts (status line, §16, code signing, roadmap status, desktop release trust, Homebrew).

**§11.3 additions to the copied macOS bullet:**
- After "…before the engine starts;": "the launcher sets `RLIMIT_CORE=0` and the policy's resource limits through the system Bash before it execs `sandbox-exec`;" (`macos.rs:93-130`).
- D4, per OD-8. Option A (default): "When `sandbox-exec` exists but the system refuses every profile (for example inside another Seatbelt sandbox), `best-effort` runs the engine unconfined with a `sandbox_unavailable` warning and `required` refuses." Option B: "…every engine launch fails with `sandbox_unavailable` in both modes."
- LibreOffice (D2), per OD-8: undecided: "LibreOffice's Linux private `/tmp` is unavailable and Seatbelt denies its hard-coded IPC socket, so Office conversions fail on macOS unless `ASHIFT_SANDBOX=off`." IPC grant: "LibreOffice gets a private IPC directory inside its workspace, with UNIX-socket bind and connect limited to that directory." Exemption: "LibreOffice runs outside Seatbelt on macOS: Office conversions carry a `sandbox_unavailable` warning, and `required` refuses Office routes."
- Do not list grant paths (D5 narrows them; the copied text says "system libraries and fonts").

**Hardening-only edits (ready to paste):**

Line 69:
```
| Better Auth | 1.7 (1.7.12) | Runs in the separate auth service `services/auth` on Hono 4.13 (`@hono/node-server` 2.1) and `pg` 8, with the JWT, API Key (`@better-auth/api-key`), Passkey (`@better-auth/passkey`), two-factor and OAuth provider (`@better-auth/oauth-provider`) plugins; the OAuth provider makes the service the authorization server for remote MCP |
```
(`services/auth/src/auth.ts:126-152`.)

Line 92:
```
| rmcp | **3.5.1** (`default-features = false`; `ariad-cli`: `server`, `macros`, `transport-io`; `ariad-server`: `server`, `transport-streamable-http-server`) | Official MCP SDK for Rust: the stdio server in `ashift mcp` and the Streamable HTTP server at `/mcp` in `ariad-server api` |
```
(`crates/ariad-cli/Cargo.toml:20`, `crates/ariad-server/Cargo.toml:29`, root `Cargo.toml:55`.)

Line 307:
```
| `ariad-server` | `api`: Axum + OpenAPI, and the OAuth-protected remote MCP endpoint `/mcp`; `worker`: consumes jobs from pgmq and calls ariad-host | ✗ | axum, sqlx, object_store, rmcp, jsonwebtoken |
```

Line 553:
```
- Plugins used: `deep-link` (vendored, see the Decision Log), `updater`, `single-instance`, `dialog`, and `opener` with only its reveal-in-file-manager permission.
```
(`apps/desktop/src-tauri/Cargo.toml:32`, `lib.rs:37`, `capabilities/default.json:13`, `app.tsx:8`; arrived on dev in `66a483d`, 2026-10-24.)

Line 614: replace the table list with "…`twoFactor`, `rateLimit`, and the OAuth provider's `oauthClient`, `oauthAccessToken`, `oauthRefreshToken`, `oauthConsent`, `oauthResource`, `oauthClientResource`." Confirm the exact key set once from `Object.keys(plugin(...).schema)` (the test at `services/auth/test/auth.test.ts:205-220` uses `arrayContaining`).

§10, new row after "API key" (line 659); "limited per client address" assumes C1:
```
| Remote MCP | `services/auth` is the OAuth 2.1 authorization server (`@better-auth/oauth-provider`): RFC 8414 discovery, open RFC 7591 client registration limited per client address, authorization code with S256 PKCE, no client credentials grant. Access tokens are the same 15-minute EdDSA JWTs from the same issuer and JWKS, with `aud` = `<ARIAD_PUBLIC_URL origin>/mcp` (RFC 8707; the UserInfo URL may be added for `openid`). `/mcp` publishes RFC 9728 metadata at `/.well-known/oauth-protected-resource/mcp`, answers 401 with `WWW-Authenticate: Bearer resource_metadata=…`, accepts that audience or `x-api-key`, and refuses REST-audience tokens and anonymous callers. Remote MCP is disabled when `ARIAD_PUBLIC_URL` is unset |
```
(`mcp/mod.rs:37,40,155-161,176-185,256-262,424`.) If D19 variant (a) lands, add: "and when either origin is plain HTTP outside loopback".

Optional fold-in (line 660; no desktop sign-in code exists):
```
| Desktop | Planned: system browser OAuth + PKCE → `ariadshift://auth/callback`, tokens in the OS keychain. The callback route is reserved (`deep_link.rs`); the desktop app does not sign in yet |
```

New Decision Log row (do not claim path scoping; that is still open):
```
| Desktop file reveal | `tauri-plugin-opener` 2.7.2 with only the `opener:allow-reveal-item-in-dir` capability, used to show a finished output in the file manager | Spawning `open`/`explorer`/`xdg-open` from a custom command; granting the plugin's URL and file-opening permissions | One official plugin covers macOS, Windows and Linux, and the webview can only reveal a path, not open URLs or launch files |
```

Also add Decision Log rows for the choices W2 and W3 make: D4's best-effort behaviour (OD-8), D8's Windows launch gate (or the documented limit), and D26b's MSI scope (OD-10).

**D13 e (fuzz text).** Line 756 says "Seven targets" and that archive preflight and `docx_meta` are "not yet fuzzed"; hardening has 18 `[[bin]]` entries (`fuzz/Cargo.toml:34-159`). Line 766 names 6 nightly targets; `fuzz-nightly.yml:32-41` runs 9. Line 765 says "well under 10 minutes" while `ci.yml:426-427` runs 18 × 60 s with a 45-minute limit (`ci.yml:370`).

Line 750:
```
| Fuzzing | cargo-fuzz 0.13.2 | Untrusted-input parsers and trust boundaries: core readers (Markdown, front matter, HTML, PDF), the Pandoc AST and DoclingDocument mappers, IR JSON, the Typst writer, archive preflight and package metadata stamping, engine pack manifests and archives, the engine protocol, the capabilities planner, asset and URI confinement, limit validation (stable Rust with `-s none` in CI, date-pinned nightly ASan) |
```

Line 756: "…never distributed. Eighteen targets, one `[[bin]]` each in `fuzz/Cargo.toml`, cover in-process readers, mappers, limits and the host's trust-boundary parsers:". Keep the seven existing bullets; after `limits_validate` insert:
- `pdf_reader`: the base PDF text-layer reader under local and small limits; asserts typed errors, block, nesting and page limits, NFC text and provenance page statistics.
- `typst_writer`: IR JSON seeds or Markdown through the Typst markup writer and the in-memory compiler; asserts that bounded IR always compiles to a PDF.
- `archive_preflight`: EOCD and central-directory parsing, the streaming inflate check, entry-name listing, the OLE2 directory reader and `classify_file`; asserts that names passing preflight never escape an extraction root.
- `docx_meta`: DOCX/EPUB metadata stamping (the ZIP rewrite, plus the `docProps/core.xml` and OPF rewriters); asserts that a failed stamp leaves the file untouched, a successful one keeps every entry name, and no temporary file remains.
- `pack_manifest`: pack manifests and minisign signatures in install order; asserts that paths from a verified manifest stay inside the install root.
- `pack_archive`: pack component archives tried as `tar.zst`, `tar.gz`, `zip` and plain files; extracts into a temporary root and asserts that every written path and link stays inside it.
- `engine_protocol`: engine request lines and engine stdout event lines; asserts exact JSON round trips and runs the host's event validator (artifact confinement, ok/error agreement).
- `capabilities_json`: the capabilities registry and the planner over every format pair and profile; asserts that every plan is a chain of registry edges through available engines.
- `asset_resolve`: local image embedding for untrusted hrefs, with symlinks and a secret image outside the base; asserts that no href embeds the secret.
- `uri_parsers`: `file://` URIs from MCP clients, the Windows path syntax check and the link scheme filter; asserts agreement with independent oracles.
- `ir_json_versioned`: the version-checked IR JSON reader that `convert` uses; asserts byte, depth and block limits, accepts only supported versions, and agrees with the plain reader after version normalization.

Line 765: "  - PR and push CI gate (`ci.yml`): Runs on `ubuntu-26.04` with stable Rust (`-s none`) for 60 seconds per target, one target after another, on changes affecting code, with a 10s per-input timeout and a 45-minute job limit. Uses committed seeds in `fuzz/seeds/`, restores the cargo-fuzz, `fuzz/target` and corpus caches and saves them only on push, and uploads crash artifacts on failure or cancellation."

Line 766: "  - Nightly sanitizers (`fuzz-nightly.yml`): Runs daily on `ubuntu-26.04` using a date-pinned nightly toolchain (`nightly-2026-10-05`) with AddressSanitizer, one job per target for 10 minutes each with a 25s per-input timeout, on targets with unsafe-adjacent dependencies (`markdown_reader`, `html_reader`, `pandoc_ast_to_ir`, `docling_to_ir`, `ir_json`, `pdf_reader`, `archive_preflight`, `docx_meta`, `pack_archive`), testing `dev` prior to promotion."

The `pack_archive`, `engine_protocol` and `ir_json_versioned` bullets assume D11, D10 and D9 landed as specified; copy the final oracle wording from W1 and the D9 worker.

**Gate:** `just spell`; `lychee --config lychee.toml --no-progress ARIAD`… run as `lychee --config lychee.toml --no-progress ARCHITECTURE.md` (CI runs lychee only weekly or on `links.yml`/`lychee.toml` changes); then review each replaced sentence against the file:line facts above.

**Commit:** `docs(architecture): align isolation, remote MCP, desktop and fuzzing sections with the code`

**Risks:** OD-8 and OD-10 wording may need a second pass if decided after this lands; the OAuth table key list needs one runtime check. Other ARCHITECTURE editors: #84 (lines 68, 563, 657; arrives through D0, no overlap with the new §10 row), distribution (F0/H-7/H-8; copying 8eb1ef7 keeps conflicts down), J10 (line 221) and I16 (lines 344-405) after step 3b.

---

## 4. Owner decisions needed

- **OD-7** (D18 part 2): accept the global token lock for v1 (recommended; parts 1 and 3 ship now) or key the lock per client after a library check.
- **OD-8** (D4, D27 wording; D2 and D5 elsewhere): degrade in best-effort when the system refuses Seatbelt (option A, recommended, `runner.rs` untouched) or fail closed (option B, touches `runner.rs`/`doctor.rs`, run in or after W1).
- **OD-9** (D19): (a) start with OAuth off on plain-HTTP LAN origins (recommended), (b) an explicit `AUTH_OAUTH_PROVIDER=off` opt-out, or (c) an upgrade note only.
- **OD-10** (D26b, D27): MSI writes HKLM (recommended, small) or a per-user MSI through a custom WiX template.
- **New, no OD number yet:**
  - D13a: allow `minisign = "0.10.0"` as a fuzz-only, never-distributed dependency (option A), or take the seed-only option B.
  - D22: turning account linking off means a password user cannot add a social login with the same email. Accept, or choose the "link only when the local email is verified" rule (equivalent today).
  - D8: ship the Windows launch gate, or document the limit only.
  - D27: whether to also copy 8eb1ef7's `/v1` stability and Job listing Decision Log rows.

## 5. Unresolved questions

1. Better Auth 1.7.12 behaviour, to read from the installed package on ASUS: the 2FA endpoint names and after-hook order (D21); whether anonymous registration accepts `skip_consent` (D21); `account.accountLinking.enabled` and the implicit-link condition, plus the GitHub provider URLs (D22); trailing-slash routing and which parameters the provider signs (D23); whether a mismatched `client_id` writes to a refresh family (D18 part 2).
2. Does Ubuntu 26.04's archive carry `postgresql-client-18` (D20), or is PGDG needed?
3. Does Tauri 2.12's WiX template still hard-code `perMachine` (D26b)?
4. D8's race has never been reproduced on Windows. Is a Windows machine available for the manual check?
5. Should D18's pre-limit key IPv6 addresses by /64?
6. Nothing here was compiled. Assumptions to confirm on first build: `KeyPair`/`TrustedKeys` are `Sync` (D13a), `jiff::Timestamp` is inferred (D13c), the `Writer` field list and `walk` parameter order (D11), the checker's at-path format for the D25d needles, the stub DOM ids in `oauth-page.test.ts` (D23), and the object-key prefix in D25's MCP test.
7. Is there a Mac available to run W2 (Seatbelt cannot run on the ASUS box), and in what order relative to the D2/D3/D5 work in the other spec?
