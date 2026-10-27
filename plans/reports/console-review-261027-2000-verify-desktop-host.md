# Verification: hardening review of desktop, host and CLI (feat/v1-hardening)

Ref checked: `origin/feat/v1-hardening` at `dcb34bc`, merge base `68ac304`. Target report:
`plans/reports/console-review-261027-1935-hardening-desktop-host.md`. I verified each finding by
reading code on the branch, without a checkout, build or test run. I tried two runtime probes
(`sandbox-exec` with a copy of the profile, and `gh run list`), but both needed interactive
approval, so they did not run. Runtime claims below are labelled as such.

## 1. Verdict

Needs fixes. Both high findings hold. The macOS `verify` job will fail on three tests. On a Mac
with LibreOffice installed, Office conversions break under the default best-effort Seatbelt
profile. The Docling part of the second finding is speculative as written, but I found a more
concrete way it can break (N1).

## 2. Findings

### 2a. Verification of the earlier report

| Earlier finding | Verdict | Evidence on the branch |
|---|---|---|
| **High: three tests still assume macOS has no sandbox** | **CONFIRMED** | (1) `crates/ariad-cli/tests/commands_cli.rs:743` asserts `sandbox["platform"] == cfg!(target_os = "linux")`. `DoctorSandbox.support` is `#[serde(flatten)]` (`crates/ariad-host/src/commands/doctor.rs:36-37`), and `macos::probe()` sets `platform: true` (`sandbox/macos.rs:23-24`), so the JSON has `platform: true` against an expected `false`. (2) `crates/ariad-cli/tests/sandbox_cli.rs:87-92`: with `ARIAD_TEST_SANDBOX_SUPPORT=none`, the simulated support has `platform: true` (`sandbox.rs:174-179`) and `macos::gaps` returns the "Seatbelt is unavailable" gap. `EngineCommand::builtin` always sets a launcher (`runner.rs:125`), so `prepare_launch` returns `Degraded` (`runner.rs:439-461`). The launcher's simulated branch runs the engine through `/bin/bash` without `sandbox-exec` (`macos.rs:69,84-89`), the conversion succeeds, and `push_sandbox_warning` (`convert.rs:1764-1776`) adds `sandbox_unavailable`, so `assert!(codes.is_empty())` fails. (3) `crates/ariad-server/src/sandbox.rs:198-208`: the "supported" value sets only the Landlock and seccomp fields, `gaps()` on macOS looks only at the Seatbelt fields, `preflight_with_support(Required, …)` returns `Err`, and `.unwrap()` panics. `ariad-server` is a workspace member (`Cargo.toml:2`, `crates/*`) and is not gated to Linux (`crates/ariad-server/src/lib.rs`). The `verify` matrix includes `macos-26` (`ci.yml:154`), runs `just test` (`ci.yml:257`, which is `cargo nextest run --workspace`), and sets no sandbox override. The worker report flags (1) at `worker-6-t3-gpt61sol.md:40`. One correction: the human-output half of `doctor_reports_the_engine_sandbox` still passes, because the CLI rewrite keeps the prefix `engine sandbox: best-effort` (`render.rs:379-381`). The failure comes only from the `platform` assertion. |
| **High (runtime unverified): best-effort Seatbelt on macOS breaks LibreOffice and Docling** | **PARTLY** | *Correct:* the CLI and the desktop default to best-effort (`sandbox.rs:89-94`, and nothing calls `set_default_mode` in `crates/` or `apps/`). The desktop has no sandbox setting (the only `sandbox` references are `main.rs:3-4`). The profile is `deny default`, has three `mach-lookup` names and no `ipc-posix*` or `iokit` rules, and adds `(allow network*)` only when `deny_network` is false (`macos_profile.rs:7-27,83-86`). `for_engine` always sets `deny_network: true` (`sandbox.rs:290`). The macOS launcher never reads `policy.private_tmp` (no reference in `macos.rs`), even though `libreoffice_paths` sets it (`engines/sandbox.rs:78`). The branch's own docs say hard-coded Office IPC is denied (`docs/cli.md:525-527`). LibreOffice's single-instance IPC pipe is an AF_UNIX socket, created before `--convert-to` runs, with `--headless` too. That pipe needs `network-bind` and a create in `/tmp` (or a fallback directory), and the profile denies both. Converts therefore fail with a pipe/bootstrap error, or loop until the job timeout, and report a generic "LibreOffice could not convert the document." Before the branch, macOS ran engines unsandboxed (`NotApplicable`), so this is a regression. The macOS CI job installs no LibreOffice (`ci.yml:205-217` is Linux-only), so the Office leg skips (`builtin_engine_contracts.rs:100-103`). *Wrong or unproven:* (a) the Docling mechanism (shared memory, semaphores) is speculation. Docling inference does not create POSIX semaphores or shared memory on import. See N1 for a concrete mechanism. (b) "Office routes vanish if `soffice --version` fails" is unverified. Describe does run inside the profile with no writable path on macOS (the describe workspace is Linux-only, `runner.rs:301-310`), but I found no evidence that `--version` needs more than read access. (c) The cfprefsd and font-service needs are plausible, but the IPC denial is enough on its own. |
| **Medium (security): `/usr` and `/opt/homebrew` granted read and exec** | **CONFIRMED**, scope slightly wider | `SYSTEM_EXEC` at `macos_profile.rs:30-39` includes `/usr` (so also `/usr/local`, including Intel Homebrew's `var` and `etc`) and `/opt/homebrew` (`var/postgresql@*`, `var/mysql`, `etc`). These are user-owned, so an engine running as the user can read them. The network is denied, so leakage can only go into conversion output. The earlier report missed one more grant: `libreoffice_paths` always adds `read_only: /opt` (`engines/sandbox.rs:76-77`, commented "Standard Linux installs"). On macOS that applies too, so the LibreOffice engine can read all of `/opt`, including MacPorts `/opt/local/var` and vendor trees. Medium is the right severity: the threat needs an engine exploit plus the user sharing the output. |

Spot check outside the mandated scope: the medium deep-link finding holds.
`deep_link::parse` rejects any scheme other than `ariadshift` (`src-tauri/src/deep_link.rs:25-34`).
The vendored plugin re-emits every macOS `Opened` URL (`vendor/tauri-plugin-deep-link/src/lib.rs:698-709`),
and `app.tsx:286-287` shows the error as a notice.

### 2b. New issues the earlier report missed

| Severity | Location (branch) | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|
| High (unverified at runtime) | `crates/ariad-host/src/sandbox/macos_profile.rs:11-21` | The `sysctl-read` allow list omits `kern.hostname`. libSystem's `uname(3)` reads `kern.ostype`, `kern.hostname`, `kern.osrelease`, `kern.version` and `hw.machine`, and returns -1 if any read fails with an error other than ENOMEM. Apple's and Chromium's Seatbelt profiles allow `kern.hostname` for this reason. | The Docling pack (Python 3.14) imports torch for every conversion. torch's `__init__` calls `platform.system()`, which calls `os.uname()`. CPython's `platform.uname()` catches only `AttributeError`, so the OSError (EPERM) propagates and the import fails. Every Docling convert on macOS then fails under the default best-effort mode. Pandoc, Typst, PDF and the test probe never call `uname`, so `sandbox_macos.rs` cannot catch this. | Add `(sysctl-name "kern.hostname")`. The hostname is low-value data, and the network is denied. Add a `sandbox_macos` case that runs `/usr/bin/uname -a` (or Python `os.uname()`) under `SandboxPolicy::for_engine`. Run one real Docling conversion on a Mac before merge. |
| Medium | `crates/ariad-host/src/sandbox/macos.rs:69-80` with `runner.rs:439-461` | When Seatbelt is present but the probe failed, `gaps()` reports "Seatbelt is present but the system refused its enforcement probe" and best-effort returns `Degraded`, which implies the engine runs without it. The launcher, however, checks only that `sandbox-exec` exists, then runs `check_profile`, which fails again. The job therefore ends with `SandboxLaunchFailed` (`runner.rs:338-347`). Best-effort degrades only when `sandbox-exec` is missing, not when it is unusable. | `ashift convert` runs inside a parent that already applies Seatbelt, such as an agent sandbox or a CI wrapper that uses `sandbox-exec`, where nesting is refused. `doctor` says best-effort with a gap, but every engine conversion fails until the user sets `ASHIFT_SANDBOX=off`. (Runtime unverified.) | Choose one contract. Either make `prepare_launch` refuse with a clear error when `seatbelt_available && !seatbelt_usable`, or let the launcher skip Seatbelt in best-effort mode when the host-side probe already found it unusable. Then align the gap text and `docs/cli.md:511-512`. |

## 3. Contract or architecture violations

- **CONFIRMED.** `ARCHITECTURE.md:714` on the branch still says macOS does not use the launcher,
  `required` is unavailable and "Seatbelt remains later hardening work". The branch touches
  ARCHITECTURE.md (37 lines), but not this section, and adds no Decision Log row for the
  Seatbelt exec chain or for best-effort as the macOS default. AGENTS.md requires both.
- `docs/cli.md:525-527` records the LibreOffice IPC denial as intended behaviour, but it never
  says that Office conversions fail on macOS as a result. The user-facing contract is missing.

## 4. Test gaps that matter

- No macOS test runs a Python engine, LibreOffice or the desktop binary (which links AppKit and
  WebKit) as `__engine` under the profile. `sandbox_macos.rs` covers only the test probe,
  `/bin/sh` and Pandoc.
- `builtin_engine_contracts::pdf_typst_and_office_execute_through_the_sandbox_launcher` would
  expose the Office regression on any Mac with LibreOffice installed, but macOS CI has none, so
  it skips.
- No test covers the "present but unusable" Seatbelt path (N2). The simulation knob covers only
  "absent".
- `gh run list` was not approved, so I could not confirm whether `verify (macos-26)` has run on
  `dcb34bc`.

## 5. Unresolved questions

- Has anyone run a LibreOffice or Docling conversion on a Mac with this branch? A one-time
  manual run would settle the High finding and N1. LibreOffice needs a decision: exempt it with
  a reported gap, or grant a private writable IPC directory plus `network-bind` for local
  UNIX sockets under it.
- Should best-effort on macOS degrade (run without Seatbelt and warn) when Seatbelt is present
  but refused, matching Linux's best-effort Landlock, or fail closed as the launcher does now?
- Is reading all of `/opt` acceptable for the macOS LibreOffice engine, or should that grant be
  limited to Linux?
