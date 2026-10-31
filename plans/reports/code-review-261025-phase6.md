# Code review: phase 6 (v1 hardening)

Date: 2026-10-25. Reviewer: worker p6-review. Read-only review; nothing was built or run.
Scope: critical and high findings only, each checked against source with a concrete
scenario. Medium notes are listed where they affect a verdict or a coordinator gate.

| Diff | Revision | Verdict |
|---|---|---|
| `origin/dev...origin/feat/v1-hardening` (Seatbelt, doctor, desktop OS integration, fuzz) | `3b0ae7d` | FIX_REQUIRED |
| `origin/feat/cloud-foundation...origin/feat/v1-hardening--api` (`/v1/limits`, upload DELETE, freeze) | branch head | MERGEABLE |
| `origin/feat/cloud-foundation...origin/feat/v1-hardening--mcp` (remote MCP) | branch head | MERGEABLE |

## 1. `origin/dev...origin/feat/v1-hardening`

### H1 (high): files opened from the OS never reach the window

`file_open::deliver` queues paths and emits `open-files`, but nothing consumes them:

- `file_open::take` (`apps/desktop/src-tauri/src/file_open.rs:148`) has no caller.
- `command_table.rs:3-19` has no command that drains the list, so the webview cannot
  call it (the ACL is generated from the same table).
- No frontend code listens for `open-files` (`rg "open-files|file_open" apps/desktop`
  matches only the Rust module, `formats.json` and `generate.mjs`).

Scenario: a user right-clicks `notes.md` in Finder, Explorer, Nautilus or Dolphin and
picks "Convert with AriadShift". The app starts or comes to the front
(`file_open.rs:160-163`, `lib.rs:28-32`), and the Convert selection stays empty. The
behaviour that `docs/desktop.md` describes ("adds the selected files to the Convert
selection") does not happen. The phase acceptance item "The macOS Quick Action opens
the app with the selection" fails on every platform.

Fix: add a command such as `(opened_files, Empty, Vec<String>)` to `command_table.rs`
backed by `file_open::take()`, and in the Convert view call it once on mount and again
on every `open-files` event, adding the paths to the selection. The frontend file is
probably outside T2's ownership, so the coordinator needs to assign it.

### H2 (high): the `uri_parsers` fuzz oracle crashes on valid input

`fuzz/fuzz_targets/uri_parsers.rs:36-41` asserts `text.trim() == text` for every link
`allowed_link` accepts. `allowed_link` returns `true` for any string that starts with
`#` before it checks whitespace (`crates/ariad-core/src/links.rs:18-23`). For the input
`#section-2 `, or `#` followed by `\n`, `allowed_link` returns `true` and the assertion
panics. libFuzzer reaches this input in seconds: the seed `fuzz/seeds/uri_parsers/anchor.txt`
is `#section-2`, and appending one byte is enough. This is a bug in the harness, not in
the parser, but it fails the CI **Fuzz** job (`.github/workflows/ci.yml:422`) and
therefore **CI passed**.

Fix: check whitespace only for non-anchor links. For example, move the `trim`
assertion into the `if !text.starts_with('#')` branch.

### H3 (high): the nightly ASan job cannot finish inside its timeout

`.github/workflows/fuzz-nightly.yml:67` now lists 9 targets at `-max_total_time=600`.
That is 90 minutes of fuzzing alone, and the job has `timeout-minutes: 90` (line 26).
The toolchain install, the cargo-fuzz install and one ASan build per target come on
top. The job is therefore cancelled on every run, during the last target
(`pack_archive`, possibly also `docx_meta`), which then gets no ASan coverage. The
workflow also runs on `pull_request` when this file changes (line 10), so the PR that
carries this branch hits the timeout too.

Fix: shorten each run (for example `-max_total_time=420` for 9 targets), raise
`timeout-minutes` to about 130, or split the targets into a matrix.

### Medium notes (not blocking on their own)

- **M1, CI fuzz time:** the CI Fuzz job now runs 18 targets × 60 s, 18 minutes of
  fuzzing, inside `timeout-minutes: 35` (`ci.yml:367`). The PDF and Typst feature
  builds and nine new binaries are built before that. On a cold `fuzz/target` cache
  this is tight. Measure it once, or raise the timeout to 45.
- **M2, Seatbelt read scope:** `SYSTEM_EXEC` grants read and exec on all of
  `/opt/homebrew` and `/usr` (which includes `/usr/local`)
  (`crates/ariad-host/src/sandbox/macos_profile.rs:30-39`). That includes
  `/opt/homebrew/var` and `/usr/local/var` (local database data, service state). The
  network is denied, so an engine can only leak this data into a conversion output.
  Consider limiting the grant to `bin`, `lib`, `libexec`, `share`, `Cellar` and `opt`.

### Checked and found sound

- **SBPL injection:** every path reaches `sandbox-exec` as a `-D PATH_n=` parameter.
  The profile source contains only generated keys and fixed operations
  (`macos_profile.rs:90-109`). The unit test and `tests/sandbox_macos.rs` use a job
  directory named `" (allow default) ; \ newline`.
- **Allow list:** the profile is `deny default`. It never grants `/System` (the comment
  about the `/System/Volumes/Data` alias is correct), allows only a few `mach-lookup`
  services (no LaunchServices, so `open` and `osascript` cannot escape), and allows
  network only when `deny_network` is false, which `for_engine` never sets. The profile
  is applied by `/usr/bin/true` before exec, and a failure becomes a typed launch error
  without a fallback to running unconfined.
- **Bash rlimit wrapper:** only fixed flags and parsed integers enter the script, the
  program and its arguments stay in `"$@"`, and `BASH_ENV`/`ENV` are removed
  (`macos.rs:236-301`).
- **Doctor:** `render.rs` changes only the presentation, and `seatbelt_enforced`
  requires a non-off mode and a usable probe.
- **Explorer verbs:** `"<exe>" "%1"` is quoted in both NSIS and WiX
  (`os-integration.nsh`, `os-integration.wxs:14`). The paths are absolute, so a file
  name that starts with `-` cannot become an option.
- **Linux and macOS entries:** Dolphin and Nautilus pass `-- %F` and `-- "$@"`. The
  main entry passes `%U`, so files arrive as `file://` URIs. The Quick Action runs
  `/usr/bin/open -b com.ariadnev.ariadshift "$@"` with absolute Finder paths, and the
  bundle id matches `tauri.conf.json`.
- **Argument parsing:** `parse_args` skips options until `--`. Other URL schemes,
  including `ariadshift://` deep links, are ignored, and macOS `Opened` events are
  filtered to the `file` scheme. A Windows `file://host/share` argument would make
  `is_file()` touch SMB. Only a local process that can already launch the app with
  arbitrary arguments can supply one, because the URL scheme handler delivers only
  `ariadshift://` URLs. Not a finding.
- **Fuzz targets:** `archive_preflight`, `docx_meta`, `pack_manifest`, `pack_archive`,
  `engine_protocol`, `capabilities_json`, `asset_resolve` and `ir_json_versioned` call
  the production parsers (`ariad_host::archive`, `package_meta`, `packs`,
  `engines::common::read_request`, `assets::resolve`, `ir_io::read_versioned`). Every
  imported symbol is public. The `docx_meta` wrapper reaches the OPF rewriter because
  `stamp_epub` finds the first `*.opf` entry. Apart from H2, I found no oracle that
  fires on valid input.

## 2. `origin/feat/cloud-foundation...origin/feat/v1-hardening--api`

No critical or high findings.

- **Upload DELETE authorization:** the row is filtered by
  `caller.principal.can_access(workspace_id)` before any effect
  (`api/uploads.rs` in `delete_upload`), and an unknown or foreign id returns 404.
  `tests/upload_deletion.rs:151` covers a foreign workspace. Anonymous uploads stay
  reachable by anyone who holds their random UUID. This is the existing
  capability-id model (`auth/mod.rs:59-64`), shared with `complete` and
  `DELETE /v1/jobs`, not a new exposure.
- **Race with job creation:** `delete_upload` takes the upload row `FOR UPDATE` and
  `create_job` re-reads it `FOR SHARE` inside its transaction (`api/jobs.rs:174-183`).
  Whichever transaction commits first wins, and the other gets 409.
- **Quota bypass:** deleting does not refund the daily allowance, because
  `upload_usage` counts uploads of every status. Only the pending slot is released,
  which is intended.
- **Information in `/v1/limits`:** the endpoint returns only the caller's own
  subject: the workspace, or the keyed client-address hash for anonymous callers.
  It does not return the subject string or an address, it sends
  `Cache-Control: private, no-store`, and it goes through the same `Caller` rate
  limit. Anonymous users behind one NAT see shared counts, which they already share
  through 429s.
- **Freeze test:** `v1_stays_backward_compatible_with_the_frozen_snapshot` fails when
  the committed baseline is missing or any breaking change appears. The negative tests
  run the checker against the real generated document (removing `Job.status`,
  `/v1/limits`, upload DELETE, a 410 response) and against synthetic request and
  response shapes. The baseline refresh refuses to write while a change is breaking.
  The test can fail, and it fails for the right reasons. Allowing new enum values in
  responses is a choice that `docs/api.md` documents.

## 3. `origin/feat/cloud-foundation...origin/feat/v1-hardening--mcp`

No critical or high findings.

- **Audience and issuer:** the MCP authenticator clones the auth config, sets
  `anonymous = false`, and replaces the JWT audience with the canonical resource URL
  (`mcp/mod.rs:56-60`). `jwt.rs:95-99` checks `iss`, `aud`, `exp` and `sub`, so a REST
  token is rejected at `/mcp` and an MCP token is rejected at `/v1`. API keys go
  through the same `api_key::verify` and failed-credential throttle. An anonymous
  result becomes a 401 challenge with `resource_metadata` (`mod.rs:195-225`).
- **Origin and DNS rebinding:** `validate_headers` wraps every MCP route. It requires
  exactly one `Host` equal to the public host and port, and at most one `Origin`,
  which must exactly match a configured CORS origin (`mod.rs:165-193`). rmcp's own
  host and origin checks are also enabled.
- **Request size:** `RequestBodyLimitLayer(1 MiB)` and rmcp's `max_request_body_bytes`
  both apply. Inline content is capped at 256 KiB of UTF-8, and inline file names must
  be plain names with an `.md` or `.html` extension (`tools.rs:285-320`).
- **Path confinement:** no tool takes a file path or URL. Inputs are upload ids or
  inline text.
- **Ownership:** `inspect` requires the caller's own workspace, `convert` and
  `job_status` reuse `create_job`, `visible_job` and `job_outputs`, and `plan` reuses
  the REST handler.
- **Quota and rate limit:** `convert` goes through `create_upload` (`admit_upload`)
  and `create_job` (`admit`), and every request goes through `Caller` (the
  per-workspace rate limit). Once the API branch is merged, `create_job`'s
  `FOR SHARE` check also covers MCP conversions.
- **Credential forwarding:** `Authorization` and `x-api-key` are stripped before the
  rmcp service runs.
- **Medium, deployment:** the exact `Host` match rejects HTTP/2 requests that carry
  only `:authority` with no `Host` header, and clients that send `host:443` explicitly.
  Behind a TLS proxy that forwards the original `Host` over HTTP/1.1 this works.
  Document the proxy requirement in `docs/server.md`, or also accept the URI authority.

## Verification the coordinator should run

```sh
# Diff 1
just ci && just desktop-ci
just fuzz uri_parsers 60          # expect a crash on '#…<whitespace>' until H2 is fixed
just fuzz archive_preflight 60 && just fuzz docx_meta 60
cargo test -p ariad-host --test sandbox_macos   # on macOS
# Manual: Finder Quick Action / Explorer verb / Dolphin entry -> files appear (H1)
# Diff 2
just test-server   # openapi_v1_snapshot, plan_limits, upload_deletion
# Diff 3
just test-server   # mcp_remote
```

## Unresolved questions

- H1: who owns the desktop frontend file that has to drain the queue? T2's ownership
  list covers only `src-tauri/` and `os-integration/`.
