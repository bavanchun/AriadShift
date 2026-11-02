# Image extraction and process limits

Status: complete; macOS runtime verification remains for the Mac gate.

PDFium can decode inline images while opening a page. Before that call, the engine now validates inline dictionaries, checks individual and per-page pixel and decoded-byte totals with checked arithmetic, bounds scanned content, and inspects form resources with memoization and the existing traversal-work limits. It refuses missing or invalid dimensions, bit depth and color space. The form-image collection walker also runs before PDFium opens the page. The existing real bitmap checks remain in place. Pages with inline images and repeated XObject invocations or multiply reachable forms fail closed rather than undercounting them.

The merged render-budget branch supplies PDFium's existing default timeout and memory limits (600 seconds and 4096 MiB); explicit caller limits remain unchanged. On Linux the re-exec launcher sets `RLIMIT_AS` before executing PDFium, including when sandboxing is off. A failed Rust allocation reports `limit_exceeded`; the RSS watchdog still covers the process tree. On macOS the existing RSS watchdog now samples every 100 ms and maps an exceeded budget to `limit_exceeded`. macOS was not available for a native runtime check.

The generated 518-byte fixture `fixtures/pdf/inline-image-bomb.pdf` and its source and SHA-256 are recorded in `fixtures/manifest.toml`. Its test returns `limit_exceeded` in under five seconds. Under a 2 GiB `systemd-run` scope, the final measured peak RSS was 12,576 KiB. Tests also cover page and nested-form totals, repeated and inherited forms, malformed dictionaries, the Linux hard limit and allocation failure. The existing JPEG, JPX and inline image tests cover valid output.

Checks:

- `cargo fmt --all --check`: passed.
- Locked `cargo clippy --locked -p ariad-pdfium-engine -p ariad-host --features ariad-host/test-probe --all-targets -- -D warnings`: passed.
- Locked `cargo test --locked -p ariad-pdfium-engine`: passed, including 19 extraction tests; test binaries ran under `MemoryMax=2G` and `MemorySwapMax=0`.
- Locked `cargo test --locked -p ariad-host --features test-probe`: passed. An earlier run exposed two publication tests that used a PDFium-labelled shell without a launcher. Those engine-independent publication tests now use a qpdf-labelled shell, and dedicated tests exercise the PDFium launcher and its actual address-space limit.
- `inline_images_check_dimensions_before_decoding` under a 2 GiB scope: passed with 12,576 KiB peak RSS, measured by `getrusage`.
- The fixture manifest path and SHA-256 were verified against the generated file.

Compatibility: inspection fails closed for unsupported inline filters, filter chains, unresolved color-space aliases, repeated XObject invocations in pages with inline images, and multiply reachable forms. See `docs/tools.md`.
