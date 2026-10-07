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
| `1` | Failure | Conversion or operation failed, input/output I/O error, or generic non-tool check failure. |
| `2` | Usage | Invalid command-line arguments, unknown flags, unsupported profile value, or destination canonicalizing to input path. |
| `3` | Unsupported Route | Unsupported conversion route, unknown target format, unknown input format in `inspect`, or refusal to convert to the same format (e.g. `md -> md`). |
| `4` | Limit Exceeded | Document size, nesting depth, or node count exceeded configured security limits. |
| `5` | Tool Missing | Required external tool (such as Pandoc `>= 3.12, < 4`) is missing, inaccessible, or has an unsupported version. |
| `6` | Destination Exists | Destination file already exists and `--overwrite` was not specified. |
| `130` | Interrupted | Interrupted by signal (SIGINT / Ctrl-C on Unix, Ctrl-C / Ctrl-Break on Windows). Temporary workspaces and partial files are cleaned up. |

---

## Stdout and Stderr Rules

- **Stdout**:
  - In default human mode, `ashift convert` prints only the final output file path. Other commands print formatted, human-readable text and tables.
  - When `--json` is supplied, exactly one valid JSON document is printed to stdout on success. On failure, stdout is empty, error diagnostics are written to stderr, and a non-zero exit code is returned.
- **Stderr**:
  - Diagnostics and warnings are written to stderr as `warning[<code>]: <message>`. Warning messages never contain confidential document contents.
  - Conversion progress indicators (`progress: <stage>`) are printed to stderr only when stderr is connected to an interactive terminal (TTY).
- **Atomicity**:
  - Conversions write to a temporary isolated workspace first. Destinations are replaced only after a complete, successful conversion. On error or cancellation (exit 130), partial output is never left behind.

---

## Commands

### `ashift convert`

Converts a document between supported formats (`md`/`markdown`, `html`/`htm`, `docx`, and `epub`).

```bash
ashift convert [OPTIONS] --to <FORMAT> <INPUT>
```

#### Arguments and Options

- `<INPUT>`: Path to the input document file.
- `--to <FORMAT>`: Target format identifier (`md`, `markdown`, `html`, `docx`, `epub`).
- `-o, --output <OUTPUT>`: Explicit output file path. Defaults to `<input stem>.<target extension>` in the same directory as the input.
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
```

#### JSON Output Shape

When `--json` is passed, `ashift convert` emits:

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

Lists installed and known engines, versions, licenses, readiness status, and the conversion routes they serve.

```bash
ashift engines [OPTIONS]
```

#### Options

- `--json`: Output engine listing as JSON.

#### Behavior

- Reports native in-process engine `ariad-core` as `ready`.
- Probes external engine availability (`pandoc`), checking executable presence and supported version (`>= 3.12, < 4`).
- Shows `docling` roadmap status as `not installed` with note `available in v0.2`.

#### Human Output Example

```text
ENGINE       VERSION   LICENSE            STATUS         ROUTES
ariad-core   0.0.0     Apache-2.0         ready          ariad-ir+json -> html, ariad-ir+json -> markdown, html -> ariad-ir+json, markdown -> ariad-ir+json
pandoc       3.12      GPL-2.0-or-later   ready          ariad-ir+json -> docx, ariad-ir+json -> epub, docx -> ariad-ir+json, epub -> ariad-ir+json
docling      -         Apache-2.0         not installed  (none)
```

#### JSON Output Shape

```json
[
  {
    "id": "ariad-core",
    "version": "0.0.0",
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
    "version": "-",
    "license": "Apache-2.0",
    "status": "not installed",
    "note": "available in v0.2",
    "routes": []
  }
]
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
2. **Workspace**: Verifies temporary directory is writable and workspaces can be created and cleaned up.
3. **Engine Describe**: Verifies the out-of-process engine runner can exchange protocol messages with the Pandoc engine.

#### Exit Codes

- Returns exit code `0` if all required diagnostic checks pass.
- Returns exit code `5` if an external tool (Pandoc) is missing or has an unsupported version.
- Returns exit code `1` if a non-tool check fails (e.g. temporary directory unwritable).

#### Human Output Example

```text
✓ Pandoc 3.12 found in >= 3.12, < 4 at /usr/local/bin/pandoc
✓ temp directory is writable and workspace can be created and removed
✓ engine 'pandoc' passes describe round trip
```

*(On Windows (`cfg!(windows)`), `[ok]` and `[x]` are printed instead of `✓` and `✗`)*

#### JSON Output Shape

No local filesystem paths are exposed in the JSON output:

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
    }
  ]
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
- `SOURCE_DATE_EPOCH`: Unix timestamp in seconds used for deterministic, byte-reproducible ZIP archives (DOCX and EPUB).
