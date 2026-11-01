//! The `ariad-pdfium-engine` binary: serves one `ariad-engine/2` request on stdin and stdout.

#![forbid(unsafe_code)]

use std::{io, process::ExitCode};

fn main() -> ExitCode {
    ariad_engine_io::serve_v2(
        io::stdin().lock(),
        io::stdout().lock(),
        ariad_pdfium_engine::dispatch::handle,
    )
}
