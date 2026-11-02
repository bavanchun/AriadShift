status: incomplete

# Lossy JPEG compression

The lossy optimizer now uses pure-Rust `oxiarc-jpeg` 0.4.2 with 4:2:0 sampling, behind `codecs`. Its registry licence and that of `oxiarc-core` 0.4.2 are Apache-2.0; the encoder forbids unsafe code. `jpeg-encoder` 0.7.1 was not selected because its registry licence includes IJG, which the project licence gate does not permit. The existing JPEG path, used outside lossy optimization, remains unchanged. The encoder test inspects JPEG frame sampling and checks both paths.

The unchanged Stirling baseline and the host PDFium measurement yield 6/10 size-and-SSIM passes, below the required 7/10. All ten ebook results meet SSIM 0.98. The 50% target runs hit within 5% on 0/10, which is not a D9 failure: the ladder chooses the highest-quality evaluated result under the target or reports the smallest result when it cannot reach it. Per-file size, SSIM and target gaps are in [the measurement report](compress-measurement-p05.md). The IP-law target result has SSIM 0.7649 and emits the existing floor warning. No preset or threshold was weakened, and no ladder change was made under the approved instruction to stop at 6/10.

## Integration

The local, uncommitted integration patch adds declarations and dispatch for the existing s1/s2 code in `crates/ariad-pdf/src/lib.rs`, `crates/ariad-pdf/src/naming.rs`, `crates/ariad-pdf/src/tools/mod.rs`, and the host tool modules `mod.rs`, `publish.rs`, and `run.rs`. The existing base already has the shared warning and metric structures. All-target clippy additionally needed local declarations of `flatten` and `unlock_forms`; the integrator owns these shared declarations. `THIRD_PARTY_LICENSES` was regenerated for the two Apache-2.0 crates and is integrator-owned and uncommitted. The committed lockfile includes the pre-existing s1/s2 dependency entries required by this base alongside the two new entries.

## Checks

- Sampling regression: `cargo test -p ariad-pdf --features codecs --test images_codec lossy_jpeg_uses_420_sampling` passed.
- `cargo fmt --all --check` passed; all-target clippy for `ariad-pdf` and `ariad-host` with `codecs` and `test-probe` passed after the local declarations. Earlier clippy attempts exposed the missing `flatten` and `unlock_forms` declarations.
- Compression: 20 tests passed; grayscale: 6 passed. The PDF corpus test failed at 6/10. The host `compress_similarity` test had two other tests pass and the corpus test fail at 6/10, with no processing errors.
- `cargo check -p ariad-pdf --target wasm32-unknown-unknown` passed without `codecs`. `env UV_NO_CONFIG=1 just deny` passed. `just pandoc` passed.
- `just licenses` succeeded after installing pinned pnpm dependencies. `just licenses-check` verified the regenerated Rust and desktop notices but could not complete the web analysis: the fresh workspace lacks Next.js analysis, and a direct analysis build failed because generated WASM packages and SDK bindings are absent. The full web build was not run because its recipe creates a separate local build directory, contrary to the shared-target constraint.
- The first locked sampling test could not update the lockfile during the encoder switch; the subsequent unlocked run updated it and passed. The initial `just licenses` generated the Rust notices but failed on missing desktop dependencies, leaving its redirected desktop notice empty; that notice was restored and dependencies installed using the pinned pnpm version. The first `pnpm` attempt failed because the mise shim had no selected default.

All Cargo and just commands used the build lock. An `env` wrapper inside the lock caused the compression and grayscale test commands to use a local build directory rather than the shared one. An attempt to remove that directory after the tests was blocked by the workspace tool filter. No process from this task remains running after the checks finish.
