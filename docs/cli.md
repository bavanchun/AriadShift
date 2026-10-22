# CLI Reference (`ashift`)

`ashift` is the command-line interface for AriadShift, providing local document inspection, planning, conversion, engine introspection, and environment diagnostics.

All conversion and transformation logic lives in `ariad-host` and `ariad-core`; the CLI binary is a thin client that handles command parsing, terminal rendering, cancellation signals, and structured JSON output.

## Table of Contents

- [Global Options](#global-options)
- [Exit Codes](#exit-codes)
- [Stdout and Stderr Rules](#stdout-and-stderr-rules)
- [Commands](#commands)
  - [`ashift convert`](#ashift-convert)
  - [`ashift inspect`](#ashift-inspect)
  - [`ashift plan`](#ashift-plan)
  - [`ashift engines`](#ashift-engines)
  - [`ashift engines install`](#ashift-engines-install)
  - [`ashift engines remove`](#ashift-engines-remove)
  - [`ashift engines verify`](#ashift-engines-verify)
  - [`ashift doctor`](#ashift-doctor)
- [Profiles](#profiles)
- [Environment Variables](#environment-variables)

---

## Global Options

- `-h, --help`: Print help information. Available on the root command and all subcommands.
- `-V, --version`: Print version information.

---

## Exit Codes

All CLI commands adhere to a single unified exit-code table:

| Exit Code | Name | Meaning |
|---|---|---|
| `0` | Success | Command completed successfully. All required checks passed for `doctor`. |
| `1` | Failure | Conversion or operation failed, input/output I/O error, or generic non-tool check failure. Engine packs: signature, hash, size, download, unsafe-archive or I/O failure, or `engines verify` found differences. |
| `2` | Usage | Invalid command-line arguments, unknown flags, unsupported profile value, or destination canonicalizing to input path. Engine packs: malformed pack name or version, plain HTTP to a non-loopback host. |
| `3` | Unsupported Route | Unsupported conversion route, unknown target format, unknown input format in `inspect`, or refusal to convert to the same format (e.g. `md -> md`). Engine packs: unknown pack, pack or version not installed, no build for this platform. |
| `4` | Limit Exceeded | Document size, nesting depth, node count, or engine process-tree memory exceeded configured security limits. Engine packs: an archive unpacks beyond its declared size or entry count. |
| `5` | Tool Missing | Required external tool (such as Pandoc `>= 3.12, < 4`) is missing, inaccessible, has an unsupported version, or required engine sandbox isolation is unavailable. The Docling engine is not installed, or its Tesseract 5 or models are missing for the document. Engine packs: the pack requires a different `ashift` version. |
| `6` | Destination Exists | Destination file already exists and `--overwrite` was not specified. |
| `130` | Interrupted | Interrupted by signal (SIGINT / Ctrl-C on Unix, Ctrl-C / Ctrl-Break on Windows). Temporary workspaces and partial files are cleaned up. |

---

## Stdout and Stderr Rules

- **Stdout**:
  - In default human mode, `ashift convert` prints one output path per successful input. Other commands print formatted, human-readable text and tables.
  - When `--json` is supplied, exactly one valid JSON document is printed to stdout on success. Single-input failures leave stdout empty and write diagnostics to stderr. Multi-input conversion prints a `results` array even when individual inputs fail; command-level usage errors still leave stdout empty.
- **Stderr**:
  - Diagnostics and warnings are written to stderr as `warning[<code>]: <message>`. Warning messages never contain confidential document contents.
  - Conversion progress indicators (`progress: <stage>`) are printed to stderr only when stderr is connected to an interactive terminal (TTY).
- **Atomicity**:
  - Conversions write to a temporary isolated workspace first. Destinations are replaced only after a complete, successful conversion. On error or cancellation (exit 130), partial output is never left behind.

---

## Commands

### `ashift convert`

Converts a document between supported formats (`md`/`markdown`, `html`/`htm`, `docx`, and `epub`). With the [Docling engine](docling.md) (the `docling` pack, or `ASHIFT_DOCLING_ENGINE` during development) it also reads `pdf`, `pptx`, `xlsx` and the images `png`, `jpeg`, `tiff` and `webp`, with OCR for scans; DoclingDocument JSON (`*.docling.json`) converts without any engine.

```bash
ashift convert [OPTIONS] --to <FORMAT> <INPUT>...
```

#### Arguments and Options

- `<INPUT>...`: One or more input document paths, processed in order.
- `--to <FORMAT>`: Target format identifier (`md`, `markdown`, `html`, `docx`, `epub`).
- `-o, --output <OUTPUT>`: With one input, an explicit output file path. With several inputs, an existing output directory; each file keeps its stem and receives the target extension. If omitted, each output goes beside its input. Duplicate destinations are refused before conversion (exit 2).
- `--profile <PROFILE>`: Routing optimization profile: `editable` (default), `faithful`, `fast`, or `private`.
- `--overwrite`: Overwrite the destination file if it already exists.
- `--json`: Output conversion report as JSON instead of the bare output path.

#### Examples

```bash
# Convert Markdown to DOCX (default output: draft.docx)
ashift convert draft.md --to docx

# Convert DOCX to Markdown with explicit output path
ashift convert report.docx --to md -o output/report.md

# Convert with JSON output for scripts or pipelines
ashift convert draft.md --to docx --json

# Convert a batch into an existing directory
ashift convert draft.md notes.md --to docx -o output/
```

#### JSON Output Shape

For one input, `--json` preserves the existing report shape:

```json
{
  "output": "path/to/output.docx",
  "route": [
    {
      "from": "markdown",
      "to": "ariad-ir+json",
      "engine": "ariad-core"
    },
    {
      "from": "ariad-ir+json",
      "to": "docx",
      "engine": "pandoc"
    }
  ],
  "warnings": [
    {
      "code": "format_mismatch",
      "message": "The input file extension does not match the detected format."
    }
  ],
  "elapsed_ms": 42
}
```

For several inputs, `--json` emits one document with results in input order:

```json
{
  "results": [
    {"input": "draft.md", "ok": true, "output": "draft.html", "route": [], "warnings": [], "elapsed_ms": 2},
    {"input": "notes.txt", "ok": false, "error": {"code": "unsupported_route", "exit_code": 3, "message": "unsupported conversion route"}}
  ]
}
```

Successful items contain the same report fields as single-input conversion; the route above is abbreviated. A failed input does not stop later inputs. Human mode prints failures as `ashift: <input>: <error>` on stderr. The exit code is the first failed input's code, or 0 if all succeed. Cancellation stops the remaining inputs and returns 130; already completed outputs stay in place. A batch shares a session pool for engines advertising session support. Pandoc continues to use one process per job.

---

### `ashift inspect`

Inspects a document, sniffed format, structural element counts, metadata, warnings, and reachable target formats.

```bash
ashift inspect [OPTIONS] <INPUT>
```

#### Arguments and Options

- `<INPUT>`: Path to the input document file.
- `--json`: Output inspection results as JSON.

#### Behavior

- Sniffs format from prefix magic bytes, HTML tags, or ZIP central-directory inspection (for DOCX and EPUB).
- Parses metadata (title, authors, language, date) and structural counts (headings by level, paragraphs, tables, images, links, footnotes, words).
- Deep document structure analysis is capped at 32 MiB (`ANALYSIS_MAX_BYTES`). Files larger than 32 MiB skip structural counting and emit `warning[document_too_large]: document too large to analyse`.
- Note that local conversion (`ashift convert`) enforces no local file size cap (cloud plans enforce entitlements per §9.5). For compressed archives (DOCX and EPUB), the 32 MiB analysis cap applies to the compressed file size on disk rather than the uncompressed IR document size.
- Peak memory usage during AST expansion is approximately 100x the input document size (~3 GB peak RSS on a 32 MiB markdown file).
- Lists reachable target formats that the planner can convert to.
- For PDF documents, reports byte size, notes that Docling engine is required (roadmap 1b), and exits with code 0.
- If the file format cannot be recognized, exits with code 3.
- Requires regular files: FIFOs, device nodes, and directories are rejected upfront to prevent blocking or resource exhaustion.

#### Examples

```bash
# Human-readable inspection
ashift inspect report.docx

# Structured JSON inspection
ashift inspect document.md --json
```

#### JSON Output Shape

```json
{
  "format": "docx",
  "bytes": 28416,
  "meta": {
    "title": "Quarterly Report",
    "authors": ["Alice Smith"],
    "language": "en-US",
    "date": "2026-03-31",
    "subject": null,
    "keywords": [],
    "source_format": "docx"
  },
  "counts": {
    "headings": { "1": 1, "2": 3 },
    "paragraphs": 12,
    "tables": 2,
    "images": 1,
    "links": 5,
    "footnotes": 0,
    "words": 450
  },
  "warnings": [],
  "reachable": ["html", "markdown", "epub"]
}
```

For unsupported formats or unmeasured routes, `counts` or fields in `score` appear as `null`. Optional fields such as `page_count` are omitted when unavailable.

---

### `ashift plan`

Plans the conversion route from an input file to a target format without performing the conversion, reporting fidelity, editability, and estimated duration.

```bash
ashift plan [OPTIONS] --to <FORMAT> <INPUT>
```

#### Arguments and Options

- `<INPUT>`: Path to the input document file.
- `--to <FORMAT>`: Target format identifier (`md`, `docx`, `html`, `epub`).
- `--profile <PROFILE>`: Optimization profile: `editable` (default), `faithful`, `fast`, or `private`.
- `--json`: Output plan results as JSON.

#### Behavior

- Refuses same-format conversions (e.g. `md -> md`) with exit code 3.
- Refuses unreachable routes with exit code 3.
- Renders routes with engine chain arrows (`markdown ─ariad-core→ ariad-ir+json ─pandoc→ docx`).
- Marks measured score edges from benchmark capabilities or notes unmeasured edges.
- Input file summary parsing is bounded to 4 MiB to keep planning fast. Files over 4 MiB skip deep IR summary parsing; below 4 MiB, malformed input documents fail planning with the underlying IR read error.
- External engine availability is checked. If an engine required by the planned route is not installed locally (e.g. `docling`), the score line displays `engine missing (<engine>)` (e.g. `engine missing (docling)`). The command still exits with code `0` because planning analyzes route feasibility rather than executing the conversion.
- Note that `plan` validates the container and the input, not whether the engine will succeed.

#### Human Output Layout (ARCHITECTURE §6.3)

```text
input   pd-en-alice.md · markdown · 1 paragraph · 21 words
route   markdown ─ariad-core→ ariad-ir+json ─pandoc→ docx
score   fidelity 0.94 · editability 0.88 · estimated 130ms · runs locally ✓
```

#### JSON Output Shape

```json
{
  "input": "pd-en-alice.md",
  "route": [
    {
      "from": "markdown",
      "to": "ariad-ir+json",
      "engine": "ariad-core"
    },
    {
      "from": "ariad-ir+json",
      "to": "docx",
      "engine": "pandoc"
    }
  ],
  "score": {
    "fidelity": 0.94,
    "editability": 0.88,
    "estimated_duration_ms": 130.0
  },
  "measured": true,
  "alternatives": []
}
```

---

### `ashift engines`

Lists installed and known engines, versions, licenses, readiness status, and the conversion routes they serve, followed by the installed engine packs.

```bash
ashift engines [OPTIONS]
ashift engines <install|remove|verify> ...
```

#### Options

- `--json`: Output engine listing as JSON.

#### Behavior

- Reports native in-process engine `ariad-core` as `ready`.
- Probes external engine availability (`pandoc`), checking executable presence and supported version (`>= 3.12, < 4`).
- Lists `docling` as `not installed` (note `available in v0.2`) until its pack is installed. When `ASHIFT_DOCLING_ENGINE` is set, the `docling` row comes from that engine's `describe` (its Docling version, `ready` once it answers) with the note `development override`. A docling engine without Tesseract 5 is still `ready`, with a note that OCR is unavailable, because digital documents convert without it.
- For every resolved engine pack (installed, or an `ASHIFT_PACK_<NAME>` development directory) runs the engine's `describe` through the pack entrypoint and reports its status. The row carries a `pack` object; routes come from the capabilities registry, or from `describe` for engines the registry does not know. A pack that is installed but cannot be used (damaged receipt, other platform, other `ashift` range) is listed as `missing` with a note pointing at `ashift doctor`.

#### Human Output Example

```text
ENGINE       VERSION   LICENSE            STATUS                             ROUTES
ariad-core   0.2.0     Apache-2.0         ready                              ariad-ir+json -> html, ariad-ir+json -> markdown, html -> ariad-ir+json, markdown -> ariad-ir+json
pandoc       3.12      GPL-2.0-or-later   ready                              ariad-ir+json -> docx, ariad-ir+json -> epub, docx -> ariad-ir+json, epub -> ariad-ir+json
docling      0.2.0     MIT                ready                              docx -> ariad-ir+json

PACK         VERSION   TARGET                     STATE
docling      0.2.0     x86_64-unknown-linux-gnu   active
docling      0.1.0     -                          installed
```

Without packs the second section reads `no engine packs installed`.

#### JSON Output Shape

An array of engine rows. `pack` appears only on rows backed by an engine pack:

```json
[
  {
    "id": "ariad-core",
    "version": "0.2.0",
    "license": "Apache-2.0",
    "status": "ready",
    "routes": [
      "ariad-ir+json -> html",
      "ariad-ir+json -> markdown",
      "html -> ariad-ir+json",
      "markdown -> ariad-ir+json"
    ]
  },
  {
    "id": "pandoc",
    "version": "3.12",
    "license": "GPL-2.0-or-later",
    "status": "ready",
    "routes": [
      "ariad-ir+json -> docx",
      "ariad-ir+json -> epub",
      "docx -> ariad-ir+json",
      "epub -> ariad-ir+json"
    ]
  },
  {
    "id": "docling",
    "version": "0.2.0",
    "license": "MIT",
    "status": "ready",
    "routes": ["docx -> ariad-ir+json"],
    "pack": {
      "name": "docling",
      "version": "0.2.0",
      "target": "x86_64-unknown-linux-gnu",
      "source": "installed",
      "versions": ["0.2.0", "0.1.0"]
    }
  }
]
```

`status` is one of `ready`, `missing`, `wrong version`, `not installed`; `pack.source` is `installed` or `override`.

---

### `ashift engines install`

Downloads, verifies and installs a signed engine pack. Format, layout and the signing procedure: [packs.md](packs.md).

```bash
ashift engines install <NAME> [--version <VERSION>] [--manifest <URL_OR_PATH> | --from <DIR>] [--json]
```

#### Arguments and Options

- `<NAME>`: Pack name, for example `docling`.
- `--version <VERSION>`: Version to install. Defaults to the built-in index's recommended version; with `--manifest` or `--from`, the manifest's version is accepted unless this is given.
- `--manifest <URL_OR_PATH>`: Manifest to install instead of the index entry (`https://`, loopback `http://`, or a local path). The signature is read from the same location with `.minisig` appended; relative component URLs resolve beside the manifest.
- `--from <DIR>`: Install offline from a directory holding `pack.toml`, `pack.toml.minisig` and the component files.
- `--json`: Output the install report as JSON.

#### Behavior

1. Fetches the manifest and its minisign signature, verifies the signature against the keys built into `ashift`, and checks that the trusted comment names the pack and version.
2. Checks the requested name and version, `requires_ashift`, and selects the build for this platform.
3. Downloads each component (resuming from verified downloads of an earlier attempt), checking its size and SHA-256.
4. Extracts under the rules in [packs.md](packs.md#extraction-rules) into a staging directory, writes `receipt.toml`, renames the result into `<data>/packs/<name>/<version>` and makes it the active version.

Reinstalling an identical, intact version reports `already_installed` without downloading. Another `ashift` installing or removing the same pack makes the command fail with exit code 1. Progress goes to stderr when it is a terminal.

#### Human Output Example

```text
installed docling 0.2.0 (x86_64-unknown-linux-gnu) in /home/user/.local/share/ariadshift/packs/docling/0.2.0
```

#### JSON Output Shape

No local paths are included:

```json
{
  "name": "docling",
  "version": "0.2.0",
  "target": "x86_64-unknown-linux-gnu",
  "status": "installed",
  "key_id": "1A2B3C4D5E6F7A8B",
  "files": 23456,
  "bytes": 1234567890
}
```

`status` is `installed` or `already_installed`.

---

### `ashift engines remove`

Removes an installed engine pack.

```bash
ashift engines remove <NAME> [--version <VERSION>] [--json]
```

- Without `--version`, removes every installed version. Removing the active version activates the highest remaining one.
- A pack or version that is not installed is exit code 3.

```json
{ "name": "docling", "removed": ["0.2.0"], "active": "0.1.0" }
```

---

### `ashift engines verify`

Re-hashes every installed file and link of a pack against its install receipt and re-checks the manifest signature with the keys of the running `ashift`.

```bash
ashift engines verify <NAME> [--version <VERSION>] [--json]
```

- Verifies the active version unless `--version` is given.
- Exit code 0 when everything matches, 1 when a file is modified or missing or the signature no longer verifies, 3 when the pack is not installed. Files the receipt does not list (for example caches an engine wrote) are ignored.

```json
{
  "name": "docling",
  "version": "0.2.0",
  "target": "x86_64-unknown-linux-gnu",
  "ok": false,
  "key_id": "1A2B3C4D5E6F7A8B",
  "checked": 23460,
  "modified": ["runtime/lib/python3.14/os.py"],
  "missing": [],
  "problems": []
}
```

---

### `ashift doctor`

Runs environment and dependency diagnostic checks to verify the local machine is ready for conversions.

```bash
ashift doctor [OPTIONS]
```

#### Options

- `--json`: Output diagnostic checks as JSON.

#### Diagnostic Checks

1. **Pandoc**: Verifies Pandoc executable is found and satisfies `>= 3.12, < 4`.
2. **Workspace**: Verifies temporary directory is writable, workspaces can be created and cleaned up, and reports count of stale workspaces older than 24 hours without deleting them.
3. **Engine Describe**: Verifies the out-of-process engine runner can exchange protocol messages with the Pandoc engine.
4. **Engine pack directory** (`data_dir`, not required): the pack data directory exists or can be created and is writable.
5. **Pack signing keys** (`pack_keys`, not required): lists the ids of the minisign keys built into `ashift`; a build without keys cannot install packs.
6. **Installed packs** (`pack_<name>`, not required): one check per installed version (receipt present, every recorded file present with its recorded size, built for this platform and `ashift` range) and per `ASHIFT_PACK_<NAME>` override. `ashift engines verify` does the full re-hash.
7. **Sandbox**: Reports effective mode, Landlock ABI and seccomp availability. Missing isolation is an optional failed check in best-effort mode and a required failure in required mode. Required mode needs Linux.
8. **Docling engine** (`docling_engine`, not required): the engine from `ASHIFT_DOCLING_ENGINE` or the `docling` pack answers `describe`; the message lists the Docling version and the models found. Without an engine the check fails and the hint says how to install one.
9. **Tesseract** (`tesseract`, not required; only when the Docling engine answered): Tesseract 5 is on `PATH` for Vietnamese OCR. Otherwise Vietnamese OCR is unavailable and the hint gives the install command.

#### Exit Codes

- Returns exit code `0` if all required diagnostic checks pass. Engine pack checks never change the exit code.
- Returns exit code `5` if an external tool (Pandoc) is missing, has an unsupported version, or required sandbox isolation is unavailable.
- Returns exit code `1` if a non-tool check fails (e.g. temporary directory unwritable).

#### Human Output Example

```text
✓ Pandoc 3.12 found in >= 3.12, < 4 at /usr/local/bin/pandoc
✓ temp directory is writable and workspace can be created and removed
✓ engine 'pandoc' passes describe round trip
✓ engine pack directory is writable at /home/user/.local/share/ariadshift
✓ engine pack signing keys: 1A2B3C4D5E6F7A8B
✓ engine pack docling 0.2.0 (x86_64-unknown-linux-gnu) is installed and active
✓ engine sandbox: best-effort; Landlock ABI 9 (filesystem, TCP, scopes, UNIX sockets), seccomp available
```

*(On Windows (`cfg!(windows)`), `[ok]` and `[x]` are printed instead of `✓` and `✗`)*

#### JSON Output Shape

No local filesystem paths are exposed in the JSON output. Sandbox values depend on the OS and kernel; this example shows Linux with ABI 9:

```json
{
  "checks": [
    {
      "id": "pandoc_version",
      "ok": true,
      "required": true,
      "message": "Pandoc 3.12 found in >= 3.12, < 4",
      "hint": ""
    },
    {
      "id": "workspace",
      "ok": true,
      "required": true,
      "message": "temp directory is writable and workspace can be created and removed",
      "hint": ""
    },
    {
      "id": "engine_describe",
      "ok": true,
      "required": true,
      "message": "engine 'pandoc' passes describe round trip",
      "hint": ""
    },
    {
      "id": "data_dir",
      "ok": true,
      "required": false,
      "message": "engine pack directory is writable",
      "hint": ""
    },
    {
      "id": "pack_keys",
      "ok": true,
      "required": false,
      "message": "engine pack signing keys: 1A2B3C4D5E6F7A8B",
      "hint": ""
    },
    {
      "id": "sandbox",
      "ok": true,
      "required": false,
      "message": "engine sandbox: best-effort; Landlock ABI 9 (filesystem, TCP, scopes, UNIX sockets), seccomp available",
      "hint": "use a kernel with Landlock enabled (Linux 5.13 or newer with landlock in the LSM list)"
    }
  ],
  "stale_workspaces": 0,
  "sandbox": {
    "mode": "best-effort",
    "platform": true,
    "landlock_abi": 9,
    "landlock_filesystem": true,
    "landlock_network": true,
    "landlock_scopes": true,
    "landlock_unix_sockets": true,
    "seccomp": true
  }
}
```

---

## Profiles

The `--profile` flag guides route selection when multiple routes connect a format pair:

- `editable` (default): Optimizes for downstream human and AI editing, producing semantic headings, paragraphs, and tables.
- `faithful`: Optimizes for visual and layout fidelity matching the original rendering.
- `fast`: Chooses the lowest-latency route.
- `private`: Strictly excludes cloud and network execution runtimes, requiring purely local or in-process transformations.

---

## Environment Variables

- `ASHIFT_PANDOC`: Specifies the explicit path to the Pandoc executable. When unset, `ashift` searches the system `PATH`.
- `ASHIFT_DOCLING_ENGINE`: Development override for the Docling engine: a command line that starts it, for example `"$PWD/.tools/docling/python-env/bin/python" -I -m ariad_docling` (double quotes group words). It is tried before any installed pack; see [docling.md](docling.md#running-it-from-a-checkout).
- `ASHIFT_DATA_DIR`: Data directory for engine packs (packs live under `<dir>/packs`). Defaults to the platform's local data directory for `com.ariadnev.ariadshift`; see [packs.md](packs.md#layout).
- `ASHIFT_PACK_<NAME>`: Uses an unpacked development pack directory (holding an unsigned `pack.toml`) instead of the installed pack `<name>`; the name is upper-cased with `-` written as `_`.
- `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`: Proxy settings for pack downloads.
- `SSL_CERT_FILE`, `SSL_CERT_DIR`: Trusted CA certificates for pack downloads on Linux, when the system store is not in a standard location.
- `ASHIFT_SANDBOX`: `off`, `best-effort` (CLI default), or `required`. Linux engines run through Landlock and seccomp with workspace write access and declared runtime read/execute access. Missing confinement produces `warning[sandbox_unavailable]` in best-effort mode; required mode refuses it with exit 5. `off` bypasses the launcher. Unknown values fail closed. macOS/Windows retain process groups or Job Objects; required mode needs Linux.
- Linux local IPC: `required` needs Landlock filesystem rules and seccomp, while `AF_UNIX` remains allowed. Pathname UNIX socket confinement needs Landlock ABI 9; abstract sockets and signals need ABI 6. On older kernels local services may remain reachable. `doctor --json` reports `landlock_unix_sockets` and `landlock_scopes`; use ABI 9 or stronger container isolation when local IPC must be blocked.
- `SOURCE_DATE_EPOCH`: Unix timestamp in seconds used for deterministic, byte-reproducible ZIP archives (DOCX and EPUB).
