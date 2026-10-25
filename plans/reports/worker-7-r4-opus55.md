# Worker report: local storage transfer limit (p7-r4)

Branch `fix/v1-security-audit--r4`, cut from `2bcb5ec`.

## Result

The accepted local-storage transfer limit is now documented for self-hosters and API clients. The API also logs one startup warning when it serves its own `file://` blob URLs. The signed-URL lifetime contract is unchanged.

## Files changed

- `crates/ariad-server/src/storage.rs`
  - `with_blob_transfer_limits` now logs one `tracing::warn!` when the storage signs local blob URLs. Only the API passes a public URL and secret, and `app::api_state` calls this method once at startup. Workers and S3 storage stay silent.
  - New `Storage::local_transfer_notice()` builds the message from `BlobTransferLimits::transfer_timeout`, `UPLOAD_URL_TTL` and `DOWNLOAD_URL_TTL`, so no number is duplicated. A small `minutes()` helper formats the durations. With the defaults the line reads: `local file storage: API-served transfers must finish within min(15 min, signed URL lifetime: 15 min upload, 5 min download); use S3 storage for large files`.
  - New unit test `local_blob_urls_announce_their_transfer_deadline`. The crate has no log-capture dependency (no `tracing-test`), so the test asserts the exact notice text that gets logged. It also checks that a worker-style local store (no signing) gives `None`, and that the formatter falls back to seconds.
- `docs/self-hosting.md`: a new **Storage** bullet under Configuration. It says compose uses the bundled SeaweedFS S3 storage and that this is the choice for large files. It gives the exact `file://` rule: the shorter of 15 minutes after admission and the signed URL's remaining lifetime, with uploads living 15 min and downloads 5 min. It works through the 2 GiB example (about 2.4 MB/s), names the `408 transfer_timeout` failure and mentions the startup warning. It links to `server.md#object-storage-api-and-worker`.
- `docs/api.md`: one sentence stating the same limit after the local-store URL paragraph, linking to the self-hosting section. The error table also gains the missing `408 transfer_timeout` row, which `api/blobs.rs` already returns.

`config.rs` and `main.rs` are unchanged. `transfer_timeout` is not read from the environment (`BlobTransferLimits::from_env` keeps the default), so the 15-minute value in the docs matches `config.rs:340`.

## Commits

- `709f982` feat(server): warn at startup about local blob transfer deadlines
- `01fc65a` docs: describe the local storage transfer deadline
- plus the commit adding this report

## Verification for the coordinator

```sh
cargo fmt -p ariad-server -- --check          # passed locally
typos docs/self-hosting.md docs/api.md crates/ariad-server/src/storage.rs   # passed locally
cargo test -p ariad-server --lib storage::tests::local_blob_urls_announce_their_transfer_deadline
cargo test -p ariad-server --test blob-transfers
cargo clippy -p ariad-server --all-targets -- -D warnings
```

Under the pause rules I did not run the build, tests or clippy.

## Open concerns

- The integration tests in `tests/blob-transfers.rs` call `with_blob_transfer_limits` on local storage, so they now emit the warning. That is harmless; it only shows up when a test subscriber is installed.
- `docs/server.md` (outside my ownership) already describes the deadline in its storage paragraph, so I left it alone. It does not mention the startup warning; one more sentence there would be optional.

Status: DONE
