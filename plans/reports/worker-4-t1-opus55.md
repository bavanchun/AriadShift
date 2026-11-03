# Worker p4-t1 (Opus 5.5): OpenAPI 3.1 export and drift test

Worktree `01-AriadShift-wt-cloud-foundation`, branch `feat/cloud-foundation`. Nothing was compiled or run (per brief).

## Starting point

`ariad-server` already produced a utoipa OpenAPI 3.1 document, and `schemas/openapi.v1.json` plus `tests/openapi_drift.rs` already existed (commit ec97369; regenerated through `ARIAD_BLESS_SCHEMAS=1`). A script compared every annotation's status codes and descriptions, and every DTO's fields, with the committed JSON: they matched. Gaps: no blob routes in the document, no JWKS detail in the bearer scheme, no `Retry-After` on 429 responses, no export binary, and a drift message pointing at the env-var bless path instead of `just openapi`.

## Files changed

- `crates/ariad-server/src/api/blobs.rs`: `#[utoipa::path]` for `PUT` and `GET /v1/blobs/{token}` (tag `blobs`, no security, since the signed token is the credential; binary body; 403/404/422/500 errors; `content-length` header on GET). Module doc updated.
- `crates/ariad-server/src/api/dto.rs`: `FileBytes`, a manual `PartialSchema`/`ToSchema` that renders `{"type":"string","format":"binary"}`. utoipa 6 renders `Vec<u8>` as an integer array, so the manual type is needed. It is used inline, so it adds no component.
- `crates/ariad-server/src/api/{uploads,jobs,plan,events}.rs`: every 429 response (8) now documents the `retry-after` header (u64, sent with `rate_limited`).
- `crates/ariad-server/src/api/mod.rs`:
  - bearer scheme description: auth-service JWT, JWKS verification by `kid`, the declared asymmetric algorithm only, and required `exp`/`iss`/`aud`/non-empty `sub`. The API-key description names the auth service.
  - new `blobs` tag.
  - blob routes move into the `OpenApiRouter` (merged **after** `CompressionLayer`, still `DefaultBodyLimit::disable()`, no timeout), so runtime behaviour is unchanged and the separate axum `Router` is gone.
  - new `openapi_json()` (pretty JSON plus a trailing newline) and `openapi_document_path()`.
  - unit tests: the endpoint list includes blobs, blob operations carry no security, and the JSON ends with a newline.
- `crates/ariad-server/src/bin/openapi.rs` (new): writes `schemas/openapi.v1.json`. It is auto-discovered as bin `openapi`, so `Cargo.toml` is unchanged.
- `crates/ariad-server/tests/openapi_drift.rs`: compares the committed file with `openapi_json()` and fails with "schemas/openapi.v1.json is stale: run `just openapi` and commit schemas/openapi.v1.json". The `ARIAD_BLESS_SCHEMAS` path is removed for this file, so the bin is the only writer. A second test asserts 3.1, both security schemes (bearer JWT with a JWKS mention, `x-api-key` header), SSE `text/event-stream`, the binary blob PUT with no security, the blob GET octet-stream, and `retry-after` on 429.
- `schemas/openapi.v1.json`: hand-patched to match the expected generator output: the new path `/v1/blobs/{token}`, 429 headers, scheme descriptions, and the `blobs` tag. Field order follows utoipa 6.0.0 struct order (checked in the local registry source), and the Python/serde_json pretty round-trip was byte-identical before the edit.

## Commits

- `5547f70` feat(server): document blob transfers, retry-after and JWKS checks in OpenAPI
- `5949d76` feat(server): add an openapi export binary and point the drift test at it

## `just openapi` recipe (coordinator owns the justfile)

```just
# Regenerate the committed OpenAPI document from the server code.
openapi:
    cargo run --locked -p ariad-server --bin openapi
```

## Coordinator verification

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just openapi                      # or: cargo run --locked -p ariad-server --bin openapi
git diff --exit-code schemas/openapi.v1.json || git commit -m "chore(schemas): regenerate the /v1 OpenAPI document" schemas/openapi.v1.json
cargo test -p ariad-server --test openapi_drift
cargo test -p ariad-server --lib api::tests
cargo clippy -p ariad-server --all-targets -- -D warnings
cargo fmt --check -p ariad-server   # rustfmt --check already passes on every touched file
```

The JSON was written by reasoning from the code. If `just openapi` changes it, commit the regenerated file. The drift test is the authority.

## Open concerns / follow-ups outside my files

- `crates/ariad-server/src/main.rs` still has the hidden `ariad-server openapi` subcommand that prints the document, and `docs/server.md:14` documents it. It is now redundant with the bin. Either remove both, or leave them. Not my files.
- `docs/api.md` could mention the `blobs` tag and the `Retry-After` header. Its description of the contract ("generated from the server code and checked by a drift test") is still correct.
- `cargo build -p ariad-server` (server.Dockerfile) now also builds the small `openapi` bin. The image copies only `ariad-server`, so this is harmless.
- Not documented: the generic 408 (request timeout, empty body) and the 413 that the body-limit layer returns as plain text. Both come from tower layers, not handlers.
- For T2 (SDK): blob bodies are `type: string, format: binary`. Blob URLs are only the local-store shape; with S3 the `url` points at the object store, so the SDK should follow returned URLs rather than call `/v1/blobs` directly.

Status: DONE_WITH_CONCERNS
