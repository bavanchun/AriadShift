# Worker p6-fix: the three high findings on feat/v1-hardening

Branch `feat/v1-hardening--fix`, based on `origin/feat/v1-hardening` (`3b0ae7d`). The worker
did not build or run anything. Rust files were checked with `rustfmt --edition 2024 --check`.
Biome is not installed in this worktree, so the TypeScript changes follow the surrounding
formatting by hand.

## Commits

| Commit | Subject |
|---|---|
| `58e6046` | `fix(desktop): deliver files opened from the OS file manager` (H1) |
| `f3a5134` | `test(fuzz): accept whitespace in same-document link fragments` (H2) |
| `2b0c65a` | `ci(fuzz): run each nightly ASan target in its own job` (H3) |
| `c0b602a` | `ci(fuzz): raise the fuzz job timeout to 45 minutes` (M1) |

## H1: files opened from the OS reach the Convert selection

- `apps/desktop/src-tauri/src/command_table.rs`: new command `(opened_files, Empty, Vec<String>)`.
- `apps/desktop/src-tauri/src/backend.rs`: `Backend::opened_files` returns `file_open::take()`.
- `apps/desktop/src-tauri/src/file_open.rs`: the pending list is now a private `Inbox` type
  (`queue`, `take`) behind the same global, so tests use their own instance and do not race.
  The module docs and the `deliver` comment describe the handshake.
- `apps/desktop/src/app.tsx`: the existing listener effect subscribes to `open-files` with
  `listen` from `@tauri-apps/api/event`, then drains once with `opened_files` (in `.finally`, so
  it also drains if subscribing fails). Every later event drains again. The drained files go
  through `addPaths`, which also switches to the Convert tab. Drained files are added even after
  the effect is disposed, because draining removes them from the native list and a StrictMode
  remount keeps the component state.
- Generated outputs, edited by hand to match the generator:
  `apps/desktop/src/bindings/commands.ts` (new `opened_files` wrapper) and
  `apps/desktop/src-tauri/permissions/desktop-commands.toml` (`allow-opened-files`).
  Run `just desktop-bindings-check` to confirm that they match the generator's output.
  `core:default` in `capabilities/default.json` already allows `listen`.

How the race is closed: the native list holds every file until the window drains it. A file
queued before the window subscribes is returned by the first drain, which runs after the
subscription. A file queued after the subscription raises the event. Two drains running at the
same time return disjoint sets, and the frontend removes duplicate paths. JavaScript `listen`
receives events sent with `emit_to("main", …)` (Tauri v2: `event.listen` listens to every
target unless the options set one).

Tests:

- Rust (`file_open.rs`): `cold_launch_files_wait_until_the_window_drains_them`,
  `second_instance_files_reach_a_window_that_already_drained` (arguments resolved against the
  second process's directory), `pending_list_stops_at_the_queue_limit`, the reworked
  `pending_files_drain_once_without_duplicates` (now including reopening a file that was already
  taken), and `take_drains_the_shared_list` (the only test that touches the global).
- Frontend (`apps/desktop/test/app.test.tsx`, with a new `@tauri-apps/api/event` mock):
  "adds files the OS opened before the window subscribed, after subscribing" (cold launch, and
  it checks that `listen` runs before the first `opened_files` call), and "adds files a second
  launch forwards to the running window" (the event switches from Settings back to Convert,
  removes duplicate paths, and queues the files).

## H2: `uri_parsers` oracle

`fuzz/fuzz_targets/uri_parsers.rs`: strings that start with `#` must be allowed, whitespace
included, which matches `allowed_link`'s documented contract. Every other allowed link must be
trimmed and use `http`, `https` or `mailto`. The check stays meaningful in both directions: a
fragment that is rejected, or a non-fragment link with whitespace or another scheme that is
allowed, still crashes. New seeds: `anchor-trailing-whitespace.txt` (`#section-2 \n`, the
review's crash input) and `https-leading-space.txt` (must be rejected).

## H3: nightly ASan budget

`.github/workflows/fuzz-nightly.yml` is now a `fail-fast: false` matrix with one job per target.
All nine targets keep their 600 s runs. Each job has `timeout-minutes: 35`: 10 minutes of
fuzzing, one ASan build (the slowest is the `pdf-reader` feature build), the toolchain, and a
cargo-fuzz source install on a cache miss. The corpus cache is per target
(`path: fuzz/corpus/<target>`, key `fuzz-nightly-corpus-<os>-<target>-<run_id>`), and crash
artifacts are named `fuzz-nightly-artifacts-<target>`. The target name reaches the shell through
`env: TARGET`, not through `${{ }}` in `run`. The `docs/ci.md` line ("10 min per target") is
still accurate.

## Medium notes

- M1 (done): CI Fuzz job `timeout-minutes` 35 → 45 in `.github/workflows/ci.yml`.
- M2 (left): narrowing `SYSTEM_EXEC` in `crates/ariad-host/src/sandbox/macos_profile.rs` to
  `bin`, `lib`, `libexec`, `share`, `Cellar` and `opt` is outside this worker's files. It is
  also risky without a macOS run: Homebrew fontconfig reads `/opt/homebrew/etc/fonts`, and
  Typst, LibreOffice and Pandoc may follow paths outside those directories. Recommend that the
  Seatbelt owner make the change and run `cargo test -p ariad-host --test sandbox_macos` plus a
  PDF, Typst and Office conversion on macOS.

## Verification for the coordinator

```sh
just desktop-bindings-check          # the hand-edited bindings and ACL must match the generator
cargo nextest run -p ariad-desktop   # file_open tests
pnpm --filter @ariadshift/desktop test
just desktop-ci && just ci
just fuzz uri_parsers 120            # must run clean, including the new seeds
actionlint .github/workflows/fuzz-nightly.yml .github/workflows/ci.yml   # runs inside just ci if it is wired there
# Manual: Finder Quick Action, Explorer verb and Dolphin/Nautilus entry, each tried with the app
# closed (cold launch) and with it already open (second instance); the files appear in Convert.
# After merge: one workflow_dispatch of "Fuzz (Nightly)" to measure the per-target job time.
```

## Open concerns

- I did not build or run any of this. The hand-edited `commands.ts` and the
  `desktop-commands.toml` are the biggest risk: if `desktop-bindings-check` shows a diff, keep
  the generator's output.
- The matrix uses nine runners per nightly run instead of one, and each job builds the
  dependencies under ASan on its own. The build time goes up, but no target is cancelled.
- The corpus from the old combined cache key (`fuzz/corpus/` as one path) is not restored into
  the new per-target caches. The first matrix run starts from the seeds and rebuilds the corpus.

Status: DONE_WITH_CONCERNS
