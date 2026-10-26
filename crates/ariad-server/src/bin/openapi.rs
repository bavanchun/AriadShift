//! Writes the `/v1` OpenAPI document to `schemas/openapi.v1.json`.
//!
//! `just openapi` runs this; the `openapi_drift` test fails while the
//! committed file differs from what the code generates.

use std::{fs, process::ExitCode};

fn main() -> ExitCode {
    let path = ariad_server::api::openapi_document_path();
    let json = match ariad_server::api::openapi_json() {
        Ok(json) => json,
        Err(err) => {
            eprintln!("openapi: cannot serialize the document: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = fs::write(&path, json) {
        eprintln!("openapi: cannot write {}: {err}", path.display());
        return ExitCode::FAILURE;
    }
    println!("wrote {}", path.display());
    ExitCode::SUCCESS
}
