Implemented macOS Seatbelt engine isolation and CLI doctor reporting on `feat/v1-hardening` in `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-v1-hardening`. Implementation and tests are committed. Runtime validation remains assigned to the coordinator under the worker brief's no-build rule.

Changed files:

- `crates/ariad-host/src/sandbox/macos.rs` adds availability and enforcement probes, exact-profile validation, resource limits and the system Seatbelt launch path. Missing Seatbelt degrades best-effort mode; required mode refuses it. Profile setup failures retain the existing typed launcher error and do not retry without isolation.
- `crates/ariad-host/src/sandbox/macos_profile.rs` generates a deny-by-default profile from declared engine paths and job/session writable roots. Paths travel as separate `-D` parameters, preventing SBPL injection. System library, Homebrew and declared application-bundle runtimes are readable/executable; writable grants do not add execute permission. IP and UNIX socket access remain denied because both engine protocol modes use stdin/stdout. The profile deliberately avoids granting `/System`, which exposes the data-volume alias.
- `crates/ariad-host/src/sandbox.rs` contains macOS dispatch, support fields and platform-specific gap reporting. Linux enforcement code and dependencies are unchanged; `ariad-host` remains free of unsafe code.
- `crates/ariad-host/tests/sandbox_macos.rs` adds macOS-only coverage for denied outside writes and creation, symlink escapes, installation write denial, installation reads, self-execution, TCP/UDP/UNIX socket denial, private document reads and data-volume aliases, quoted paths, real Pandoc conversion, disabled core dumps, typed profile errors, missing-support fallback and required/off modes. A module unit test also checks that hostile path characters never enter profile source.
- `crates/ariad-cli/src/render.rs` updates only the doctor sandbox status. macOS JSON reports `seatbelt_available`, `seatbelt_usable` and `seatbelt_enforced`; off mode reports enforcement as false. Other diagnostic checks and exit semantics are preserved.
- `docs/cli.md` documents the macOS sandbox, doctor fields, fallback and temporary-directory limits.

Commits:

- `cc4963cdec21bdffcbbf1f84e774ee7e3527ca7c` — `feat(host): confine macos engines with seatbelt`
- `cee38a7263619f251767954d403945842ea46bf1` — `feat(cli): report macos seatbelt enforcement`

Validation performed: scoped `rustfmt --edition 2024 --check --config skip_children=true` and Git whitespace checks passed. Reviewed source and staged diffs against the requested behavior with the installed implementation and code-review skills. No build, test, clippy, package installation or long-running service was started. No push was performed.

Current API references checked: [Rust CommandExt::exec](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html), Context7's Rust documentation, Apple's installed `/usr/share/man/man1/sandbox-exec.1` and system SBPL profiles, and the installed Bash manual's resource-limit units. The current [OpenAI Seatbelt policy](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt_base_policy.sbpl) and [policy builder](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt.rs) corroborate scoped process operations, system executable selection and parameterized paths.

Coordinator verification commands, run sequentially in the shared build queue after the integration edits below:

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-v1-hardening
just pandoc
cargo test -p ariad-host --features test-probe --test sandbox_macos -- --nocapture
cargo test -p ariad-host --features test-probe --lib sandbox::
cargo test -p ariad-host --features test-probe --test runner --test session
cargo test -p ariad-cli --test commands_cli doctor
just ci
cargo run -p ariad-cli -- doctor
cargo run -p ariad-cli -- doctor --json
ASHIFT_SANDBOX=off cargo run -p ariad-cli -- doctor --json
ASHIFT_SANDBOX=required cargo run -p ariad-cli -- doctor --json
```

Open concerns and required coordinator integration outside this worker's file ownership:

- `crates/ariad-cli/tests/commands_cli.rs:744` still asserts that `sandbox.platform` equals `cfg!(target_os = "linux")`. Source inspection confirms this assertion must include macOS now. Update it to `cfg!(any(target_os = "linux", target_os = "macos"))` and add assertions for the macOS doctor fields. This existing test was not run or weakened by this worker.
- `crates/ariad-host/src/commands/doctor.rs::sandbox_check` still formats Linux-specific feature names and hints. The authorized CLI rendering section now shows correct Seatbelt status, and host support/gap logic is correct, but desktop callers using the host report directly still need this wording centralized in the host. The CLI rendering helper can then delegate to that shared formatting.
- `ARCHITECTURE.md` sections covering doctor and isolation still describe Seatbelt as future work, and `runner.rs` retains Linux-only wording in the unsupported-platform error. Update those owning surfaces during integration; neither file was authorized for this worker.
- Runtime compatibility has not been measured. Run the new tests on macOS and smoke-test installed LibreOffice and Docling. macOS has no equivalent here of Linux's private `/tmp` mount; hard-coded Office IPC or writes outside the job root are denied rather than broadening access. The real Pandoc conversion test requires the supported Pandoc installation and intentionally fails if it is missing.

Status: DONE_WITH_CONCERNS
