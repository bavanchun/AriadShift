status: incomplete

# Compression and grayscale

The slice implements compression and grayscale over the s1 primitives, including a target ladder, deterministic object-stream output, structured warnings and metrics, conservative resource pruning, Type0 font subsetting, bounded image caching, and PDFium SSIM measurement. Object-stream writing is capped at the 32-bit offset limit of the lopdf xref writer and checks cancellation while writing. The result is not ready for integration as a completed slice until the final acceptance checks below pass.

## Presets

| Levels | Profile | Maximum DPI | JPEG quality |
| --- | --- | ---: | ---: |
| 1–3 | lossless | not applied | not applied |
| 4–5 | print | 300 | 85 |
| 6–7 | ebook | 96 | 60 |
| 8 | screen | 96 | 60 |
| 9 | screen | 72 | 50 |

The ebook values were calibrated against the available corpus. In a local calibration, 96/60 produced SSIM above 0.99 for all four sampled scans; 72/50 dropped below 0.98 for two of them. The target ladder remains bounded to eight evaluations and caps each rung with any explicit DPI or quality limits.

## Evidence and remaining acceptance

The 96/60 PDF corpus run reached 6/10 by size, and the host run reached 6/10 by size and SSIM, with no execution errors; seven are required. The host run measured a 0/10 target hit rate within 5%, and recorded each closest result and its similarity. One target run fell below the SSIM floor and emitted the expected warning. The detailed table, qpdf reference sizes, and Stirling SSIM are in `plans/reports/compress-measurement-p05.md`. The host measurement predates the image-memory preflight checks and the bounded writer wrapper, which forwards the same serialization bytes. A subsequent PDF corpus run with those checks and `SOURCE_DATE_EPOCH=1700000000` processed all ten inputs with no errors and again reached 6/10; output sizes differed from the measured host run by at most a few bytes.

For D3, a local comparison on the same 1240 × 1754 RGB pixels at quality 55 produced 75,364 bytes with `image` 0.25.10 and 50,130 bytes with the system `cjpeg`. The latter is a libjpeg comparison, not a direct measurement of the `jpeg-encoder` crate. The installed image encoder source uses 1×1 sampling for all components. No encoder dependency or licence exception was added.

## Shared changes for the integrator

The following shared files are intentionally local and uncommitted: `crates/ariad-pdf/src/lib.rs`, `crates/ariad-pdf/src/tools/mod.rs`, `crates/ariad-pdf/src/naming.rs`, `crates/ariad-host/src/tools/mod.rs`, `crates/ariad-host/src/tools/run.rs`, `crates/ariad-host/src/tools/inprocess.rs`, `crates/ariad-host/src/tools/publish.rs`, and `Cargo.lock`. The generic `ToolWarning`, `coded_warnings`, and `metrics` changes and the host bridge match the pending s5 shared contract.

Add the following declarations in `ariad-pdf/src/lib.rs`:

```rust
pub mod color;
#[cfg(feature = "codecs")]
pub mod fonts;
#[cfg(feature = "codecs")]
pub mod images;
#[cfg(feature = "codecs")]
pub mod optimize;
```

Add the following declarations and dispatch arms in `ariad-pdf/src/tools/mod.rs`, alongside the s5 result-contract changes:

```rust
#[cfg(feature = "codecs")]
pub mod compress;
#[cfg(feature = "codecs")]
pub mod grayscale;

#[cfg(feature = "codecs")]
"pdf.compress" => dispatch_structural_inputs::<ariad_core::tools::params::CompressParams, _>(
    tool_id, inputs, params, ctx, compress::run_inputs,
),
#[cfg(feature = "codecs")]
"pdf.grayscale" => dispatch_structural::<ariad_core::tools::params::GrayscaleParams, _>(
    tool_id, inputs, params, ctx,
    |docs, params, ctx| grayscale::run(docs, params, &inputs[0].limits, ctx),
),
```

Add `"pdf.compress" => "compressed",` and `"pdf.grayscale" => "grayscale",` to `ariad-pdf/src/naming.rs`, and `"pdf.grayscale" => "grayscale".to_owned(),` to `ariad-host/src/tools/publish.rs`. Add `pub mod similarity;` to `ariad-host/src/tools/mod.rs`. Change `let raw_exec` to `let mut raw_exec` and add this hook after dispatch in `ariad-host/src/tools/run.rs`:

```rust
if spec.id == "pdf.compress" && !is_out_of_process {
    let result = staged_inputs_with_names.first().zip(raw_exec.artifacts.first())
        .and_then(|((input, _), (output, _))| {
            super::similarity::measure(input, output, &request.limits,
                request.engine_program.as_deref(), &request.cancel).ok()
        });
    if let Some(score) = result {
        raw_exec.metrics.insert("similarity".into(), json!(score));
        if score < super::similarity::SIMILARITY_FLOOR {
            raw_exec.warnings.push(ConvertWarning {
                code: "similarity_below_floor".into(),
                message: format!("rendered similarity {score:.4} is below 0.98"),
            });
        }
    } else {
        raw_exec.warnings.push(ConvertWarning {
            code: "similarity_not_measured".into(),
            message: "PDFium similarity measurement was unavailable".into(),
        });
    }
}
if request.cancel.is_cancelled() {
    return Err(ToolError::Interrupted);
}
```

The task also required minimal shared primitive changes in `document.rs` and `images/{codec,placement}.rs`: cached-image decoding releases temporary buffers while retaining pixel charges; the optimizer conservatively checks retained pixels and estimated intermediate allocations against the configured memory/decompression limit; indirect Form matrices are resolved; content operations use `max_blocks` while Form visits retain the 100,000 bound; and the minimum DPI across all uses prevents downsampling a shared image below a large placement. These changes need integration with s1. Commits `284bf38`, `bbc4fa74`, `6334469c`, and `666c09a5` implement the slice on top of `be7430e` and `277cdef`; all passed `committed`. No new dependency was added; the host enables the existing `ariad-pdf/codecs` feature. The memory estimate is deliberately conservative and may reject an image whose actual peak allocation would fit; it is an image-buffer bound rather than a whole-process RSS guarantee.

## Checks

- `SOURCE_DATE_EPOCH=1700000000 cargo test -p ariad-pdf --features codecs --test compress --test grayscale --test compress_corpus`: 20 compression tests passed, then the corpus test failed at 6/10. Cargo stopped before grayscale in this combined invocation.
- A separate focused run passed 6 grayscale tests; an earlier run also passed 22 image codec, 10 colour rewrite, and 4 font subset tests with `SOURCE_DATE_EPOCH=1700000000`.
- The host PDFium corpus test failed at 6/10; its 0/10 target hit rate and scores are recorded above. The later qpdf-backed host encryption test passed exit-12 checks for both tools.
- `cargo check -p ariad-pdf --target wasm32-unknown-unknown`: passed. `cargo fmt --all --check` and the requested PDF/host clippy command passed. Five writer unit tests and five metadata integration tests passed after adding the output offset bound. The host resource-limit mapping returned exit 4.
- The first encryption regression incorrectly expected the PDF-layer error to have the host exit code; the corrected test checks the typed PDF error, and the host test verifies exit 12. All Cargo commands used the shared build lock without `--locked`.

The size threshold and direct `jpeg-encoder` comparison remain unresolved. The qpdf-backed host encryption test also passed exit-12 checks; a separate host mapping check passed exit 4 for resource limits. Review was performed in the controller session without an independent reviewer. The size gap on the signed decree is not resolved by deleting its signature: the input contains an AcroForm signature and the Stirling reference does not. The slice must not be marked complete while the corpus checks fail.
