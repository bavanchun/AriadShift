# Sign Engine Pack Development

The sign engine pack delivers digital signature creation (`pdf.sign`, `pdf.sign_token`), verification (`pdf.sign_verify`), removal (`pdf.sign_remove`), and timestamping (`pdf.timestamp`) using pyHanko and cryptographic backends.

## Architecture

- **Protocol**: `ariad-engine/2` with `op = "run_tool"` and one-shot JSON framing over standard streams.
- **Runtime**: Bundled Python 3.14 virtual environment invoked via `python -I -m ariad_sign`.
- **Brokered Network Flow**: The sign engine process runs under strict sandbox isolation with zero network access. RFC 3161 timestamping requests are emitted as protocol progress events (`stage: "tsa_request"`) and brokered by the host via `ariad-host::net::Client`.
- **Licencing**: All 31 locked packages in `uv.lock` are audited against the licence allow-list in `engines/sign/pack/licenses.py` with reviews recorded in `licence-review.toml`.

## Development Recipes

- `just sign-dev`: Syncs the sign engine virtual environment using `uv`.
- `just sign-test`: Runs ruff linting, formatting checks, and pytests for `engines/sign`.
- `just pack-sign <out>`: Builds an unsigned development sign pack.
