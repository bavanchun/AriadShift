# Worker Fix Report: `cli-infer-format`

## Issue #80: Infer the target format from the `-o` output extension in `ashift convert`

### Cause and Evidence
- `ConvertArgs.target_format` in `crates/ariad-cli/src/main.rs` (lines 114-116) was declared as a required `String` (`#[arg(long = "to", value_name = "FORMAT")] target_format: String`) with no fallback to the `-o` / `--output` file extension.
- In `run_convert` (`crates/ariad-cli/src/main.rs`, lines 597-600), `args.output` was used only as the destination path, never inspected via `ariad_core::format::Format::from_extension`.
- Running `ashift convert draft.md -o out.docx` without `--to` failed at Clap argument parsing with exit code `2` (`error: the following required arguments were not provided: --to <FORMAT>`), reproduced before the fix by `target_format_inferred_from_output_extension_and_overridden_by_to_flag`.

### Assumptions Recorded
- When `--to` is omitted for a single input and `-o` is provided, the extension of `-o` is resolved via `Format::from_extension` and converted to its canonical user-facing format name (`format.user_name()`).
- When `--to` is explicitly provided, it always takes precedence over the `-o` extension; a conflict or unknown extension on `-o` is not an error.
- When `--to` is omitted and either `-o` is omitted, `-o` has no extension or an unknown extension, or multiple inputs (batch mode) are given, `ashift convert` exits with code `2` and prints a usage error to `stderr` suggesting `--to <FORMAT>`.

### Fix
- `crates/ariad-cli/src/main.rs`: Changed `ConvertArgs.target_format` to `Option<String>`. In `run_convert`, resolved `(target_format, output)` across all four `(args.target_format, args.output)` combinations, inferring the format via `Format::from_extension` when `--to` is `None` and `-o` is `Some`, and returning exit code `2` with a clear usage error suggesting `--to <FORMAT>` when inference is impossible.
- `crates/ariad-cli/src/batch.rs`: Required `args.target_format` at the start of `batch::run`, exiting with code `2` and `"ashift: with several inputs, --to <FORMAT> is required"` when omitted.
- `crates/ariad-cli/tests/convert_cli.rs`: Added `target_format_inferred_from_output_extension_and_overridden_by_to_flag` covering `-o out.docx` without `--to`, `--to` overriding a conflicting extension, an unknown extension without `--to`, missing `-o` without `--to`, and batch inputs without `--to`.
- `docs/cli.md`: Documented optional `--to` inference from `-o` for single-input conversions and updated usage synopsis and examples.

### Regression Tests
- `target_format_inferred_from_output_extension_and_overridden_by_to_flag` (`crates/ariad-cli/tests/convert_cli.rs`)

### Verification Commands
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo fmt --all --check`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy -p ariad-cli --all-targets -- -D warnings`: **PASS**
- `ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc /home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-cli --test convert_cli`: **PASS** (26 passed, 0 failed)

### What Only CI or Another OS Can Verify
- Windows and macOS path handling and process-group signal tests in `convert_cli` (`#[cfg(windows)]` Ctrl-Break test and macOS Seatbelt runtime environment).

### Suggested PR Text
**Title:**
`feat(cli): infer convert target format from -o output extension`

**Body:**
```markdown
## Summary
- Make `--to <FORMAT>` optional for single-input `ashift convert` when `-o <OUTPUT>` has a known format extension (resolved via `Format::from_extension`).
- Preserve precedence of explicit `--to <FORMAT>` when both `--to` and `-o` are supplied, even if `-o` has a different or unknown extension.
- Emit a clear usage error (exit code 2) suggesting `--to <FORMAT>` when `--to` is omitted and `-o` is missing, has an unknown extension, or multiple inputs are passed.
- Document `-o` target format inference in `docs/cli.md` and add integration tests in `crates/ariad-cli/tests/convert_cli.rs`.

Closes #80
```
