# Form traversal and scan coverage

## Cause and fix

Repeated `Do` operations re-expanded the same forms and grew the collected image
list exponentially. Expanded form IDs are now retained per page, repeated names
are visited once, and page and document work budgets cover operators and object
visits. Image references are also bounded. The scan detector now limits PDFium
object visits and rectangles, and its exact area sweep has page and document
work budgets with cooperative cancellation and deadline checks.

The base revision did not compile: it constructed the non-exhaustive
`ariad_pdf::ToolCtx` with a literal. The engine now uses its public constructor
and preserves the same shared cancellation flag, deadline, and page limit.

## Regression coverage

- `repeated_nested_forms_are_expanded_once` generates nested repeated forms and
  verifies that their image remains available within a small work budget.
- `repeated_operators_obey_the_traversal_budget` checks `limit_exceeded`.
- `many_overlapping_images_exceed_the_coverage_limit` generates overlapping
  page images and checks `limit_exceeded`.
- `coverage_sweep_checks_for_interruption_while_processing_overlaps` verifies
  interruption during the sweep. Existing geometry tests check ordinary overlaps.

The PDFs are generated in test code and no fixture files are committed. The
baseline compilation error required retaining the `ToolCtx` constructor repair
for the control run. With the original traversal and sweep restored, all three
integration regressions failed as expected: the nested form produced 8,191
duplicate references, and the other two inputs did not reach a limit. The image
decoding fallback is outside this task and was not changed.

## Checks

- `cargo fmt --all --check`: passed.
- `just spell`: passed.
- `cargo test --locked -p ariad-pdfium-engine --test detect_scanned --test extract_images`:
  passed, 10 tests with the assembled PDFium pack.
- `cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings`:
  passed after correcting two Clippy warnings.
- `cargo test --locked -p ariad-pdfium-engine`: passed, including integration
  and documentation tests with the assembled PDFium pack.

The full checks were run through the build lock using a `bash -c` wrapper.
The wrapper selects the shared build directory only for a direct `cargo`
command, so this inadvertently used the worktree build directory. No build
directory was cleaned or deleted.

## Assumptions

The fixed page and document budgets are defensive ceilings. Scan detection
returns the existing limit error when exact coverage is too expensive rather than
approximating it. Forms are memoized by object ID within each page as requested.
