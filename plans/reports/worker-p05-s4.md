# qpdf adapter handoff

status: complete

The host serves the five qpdf tools through `ariad-engine/2`. The development
qpdf pack is used for tests only; it is not a distributable pack. The integrator
still owns tool registration and the shared documentation and lockfile.

## Pack and sandbox contract

`EngineSpec.pack_program_env` selects the adapter path. QPDF uses
`ASHIFT_QPDF_PROGRAM`; other engine specifications use `None`. Both installed
and override pack resolution go through `command_from_installed_pack` after
manifest validation and, for installed packs, signature and digest checks. The
host launches `__engine qpdf`, supplies the absolute manifest entrypoint in
`ASHIFT_QPDF_PROGRAM`, and supplies the canonical root in the host-only
`ASHIFT_ADAPTER_PACK_ROOT`. The adapter compares the canonical program with
that root and the selected manifest entrypoint; it refuses missing, relative,
outside-root and undeclared programs. Manifests cannot set either variable.
This mechanism can be reused for the Tesseract adapter. The runner grants the
pack root and, independently, the AppImage launcher root when applicable.

qpdf receives exactly one command-line argument, `@-`. Each option and path is
written as one line to its stdin; no password is on argv. The child's environment
is cleared, including PATH and loader variables. Its process inherits the
adapter sandbox. Linux uses the required sandbox in the child confinement test;
the same test is compiled for macOS Seatbelt and must run on macOS CI. A
cancelled host job kills the adapter process tree, and the adapter kills qpdf on
its own deadline. The 0600 host parameter file is removed after execution.
The adapter caps each qpdf stdout/stderr capture at 1 MiB; exceeding the cap
fails with `limit_exceeded` instead of emitting incomplete warnings or leaking a
partially captured secret. Pack metadata reads are capped at 8 MiB.

## qpdf operations

All commands below denote stdin lines following the literal process invocation
`qpdf @-`; `IN` and `OUT` are staged job paths. Secrets are placeholders.

- Encrypt preflights `--is-encrypted IN`, then passes `--encrypt
  --user-password=U --owner-password=O --bits=256|128 [--use-aes=y]
  --print=none|low|full --modify-other=y|n --extract=y|n --annotate=y|n
  --form=y|n --assemble=y|n --accessibility=y|n -- --decode-level=none
  --stream-data=preserve IN OUT`.
- Decrypt preflights `--is-encrypted IN`, then passes `--password=P --decrypt
  --deterministic-id --decode-level=none --stream-data=preserve IN OUT`.
- Permissions probes `--password=O --show-encryption IN` and
  `--requires-password IN`; it then passes `--password=O` followed by the
  encrypt options above with the replacement user password and permissions.
  Legacy RC4 is upgraded to AES-256 with an `encryption_upgraded` warning.
- Repair preflights `--is-encrypted IN`, then passes `--object-streams=preserve
  [--deterministic-id] IN OUT`. Each `WARNING:` line becomes `repaired` and
  increments `repaired_issues`.
- Linearize preflights `--is-encrypted IN`, then passes `--linearize
  [--deterministic-id] --decode-level=none --stream-data=preserve IN OUT`.

`--deterministic-id` is omitted whenever the output stays encrypted: the
provided qpdf 12.4.2 returns exit 2 and explicitly rejects that combination.
Input-requiring-password and already-encrypted failures use protocol v2
`encrypted_input`, mapped by the host to exit 12. For qpdf, `invalid_request`
maps to invalid parameters and exit 9, including CR, LF and NUL in passwords or
an omitted user password where an R5+ encrypted input needs one. Missing packs
return exit 5 with the install hint. Limits use exit 4; qpdf errors use exit 1.

`doctor` reports `qpdf 12.4.2 passes describe round trip; protocol ariad-engine/2`
when ready and an install hint when missing. `engines` reports version `12.4.2`,
`ready`, and `protocol ariad-engine/2`; a missing pack reports `not installed`
and the install hint.

Suggested `docs/tools.md` / `docs/cli.md` text: Password parameters are supplied
through the existing secret channels, not command-line parameter values. The
host transfers secrets to its engine in a private temporary parameter file, then
removes it. qpdf receives the argument list via stdin and never receives a
password in argv. Passwords containing CR, LF or NUL are rejected with exit 9.
An input requiring a password, a wrong password, or an already encrypted input
to encrypt returns exit 12. A missing qpdf pack returns exit 5 and an install
hint. Permission changes to R5+ files with a nonempty user password require that
user password. Owner-only PDFs can be decrypted without a password. Accessibility
restrictions are ignored for AES and produce `permission_ignored`.

## Shared and additional files

The integrator must add `lopdf` to the `ariad-host` lockfile dependency list;
the local shared `Cargo.lock` change is deliberately not committed. The dev-only
workspace `lopdf` dependency in `crates/ariad-host/Cargo.toml` supports required
PDF inspection tests. `runner.rs` adds the separate AppImage sandbox grant;
`tests/pack_resolve_verify.rs` updates its old direct-entrypoint assertion to
the new adapter contract; `tools/engine.rs` also maps qpdf invalid requests to
exit 9. These are the minimal additional changes authorized by the manager.

## Checks

- `cargo fmt --all --check`: passed under the build lock.
- Both requested `cargo clippy -p ariad-host --all-targets` variants, with and
  without `--features test-probe` and with `-D warnings`: passed.
- The requested integration command passed: 10 qpdf adapter tests, 5 qpdf
  secret tests, 33 pack tests and 9 pack verification tests. It used
  `ARIAD_TEST_QPDF_DIR` and `ASHIFT_PANDOC` from the brief.
- `cargo test -p ariad-host --features test-probe --lib engines::`: 43 passed
  with the supplied `ASHIFT_PANDOC`.
- `cargo test -p ariad-host --lib`: 194 passed with the supplied Pandoc.
- `cargo test -p ariad-host --features test-probe --test pdfium_pack --test
  pdfium_sandbox`: 14 passed with the supplied PDFium and Pandoc paths.
- Linux sandbox inheritance, child cancellation, parameter-file cleanup,
  raw/JSON/URL secret scrubbing, long stderr, panic suppression and timeout
  have dedicated checks. macOS Seatbelt is cfg-gated and remains a macOS CI
  verification, not a claim made from this Linux host.
- An initial test run found the absent dev-only `lopdf` dependency; after adding
  it, a permissions assertion was corrected to read lopdf's retained
  encryption state after automatic empty-password decryption. An engine-unit
  run without `ASHIFT_PANDOC` failed five Pandoc tests and passed when rerun
  with the supplied path. A format check during concurrent edits failed and
  passed on the final files. The empty/non-PDF test confirms the existing host
  rejects both, with either its input error or format error.
- Commits `6eec400` and `f606c73` both passed `committed`.
