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
  - [`ashift tools`](#ashift-tools)
  - [`ashift tool`](#ashift-tool)
  - [`ashift recipe run`](#ashift-recipe-run)
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
| `8` | Unknown Tool | Requested tool ID (`ashift tool <id>`) is not in the tool registry (`error[unknown_tool]`). |
| `9` | Invalid Params | Tool parameters failed JSON Schema validation, violated `x-ariad-secret` rules (such as an inline secret on `--param` or `--params-json`), or an imported recipe was not confirmed (`error[invalid_params]`). |
| `10` | Wrong Input | Input file count or sniffed document format does not match the tool's `ToolSpec` (`error[wrong_input]`). |
| `11` | Tier Not Allowed | Requested tool requires `Tier::Pro` or a batch/recipe run exceeds Free-plan quotas (`error[tier_not_allowed]`). |
| `12` | Encrypted Input | Input PDF is encrypted and requires a password or `pdf.decrypt` (`error[encrypted_input]`). |
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

Converts Markdown, HTML, DOCX and EPUB to/from IR JSON (`json`), reads PDF text, writes PDF through Typst, and uses system LibreOffice for legacy Office and ODF routes. With the [Docling engine](docling.md) (the `docling` pack, or `ASHIFT_DOCLING_ENGINE` during development) it also reads `pdf`, `pptx`, `xlsx` and the images `png`, `jpeg`, `tiff` and `webp`, with OCR for scans; DoclingDocument JSON (`*.docling.json`) converts without any engine.

```bash
ashift convert [OPTIONS] [--to <FORMAT>] <INPUT>...
```

#### Arguments and Options

- `<INPUT>...`: One or more input document paths, processed in order.
- `--to <FORMAT>`: Target format identifier (`md`, `markdown`, `html`, `docx`, `epub`, `pdf`, `json`, `xlsx`, `pptx`), when reachable from the input. Optional for a single input when `-o <OUTPUT>` has a known format extension; an explicit `--to` always overrides the output extension. Required when `-o` is omitted, when `-o` has an unknown extension, or when converting several inputs.
- `-o, --output <OUTPUT>`: With one input, an explicit output file path (its extension infers the target format when `--to` is omitted). With several inputs, an existing output directory; each file keeps its stem and receives the target extension. If omitted, each output goes beside its input. Duplicate destinations are refused before conversion (exit 2).
- `--engine <ENGINE>`: Select the input reader (for example, `pdf` or `docling` for PDF). Later mapping and writer steps are planned normally. A requested reader never silently falls back to a different reader.
- `--profile <PROFILE>`: Routing optimization profile: `editable` (default), `faithful`, `fast`, or `private`.
- `--overwrite`: Overwrite the destination file if it already exists.
- `--json`: Output conversion report as JSON instead of the bare output path.

#### Examples

```bash
# Convert Markdown to DOCX (default output: draft.docx)
ashift convert draft.md --to docx

# Convert Markdown to DOCX, inferring the target format from -o
ashift convert draft.md -o out.docx

# Convert DOCX to Markdown with explicit output path
ashift convert report.docx --to md -o output/report.md

# Extract digital PDF text, create PDF, or normalize a legacy document
ashift convert report.pdf --to md
ashift convert notes.md --to pdf
ashift convert legacy.doc --to md
ashift convert slides.pptx --to pdf
ashift convert notes.md --to json

# Convert with JSON output for scripts or pipelines
ashift convert draft.md --to docx --json

# Convert a batch into an existing directory
ashift convert draft.md notes.md --to docx -o output/
```

With automatic reader selection, PDF input is read once by the bounded built-in reader to check its text layer. Digital PDFs use that result directly; when `needs_ocr` is true, conversion and `plan` select Docling for OCR instead. Without the Docling engine, a scanned PDF fails with exit 5 and an installation hint. Use `--engine docling` to request layout extraction even for a digital PDF, or `--engine pdf` to keep the text-layer result and its `ocr_required` warning for pages without text. `inspect` always uses the built-in reader and never needs the Docling pack. Typst supports headings, lists/tasks, tables/spans, code, quotes, figures and footnotes, embeds Vietnamese fonts, and preserves TeX as text with `math_not_converted`.

PDF, Typst and LibreOffice advertise `supports_session: false`; batches and MCP pools run them as one-shot jobs. The shared host watchdog applies `max_memory_mb` to their process trees. On Linux, when a LibreOffice installation is found, sandboxed LibreOffice requires user and mount namespaces for a private `/tmp` because its IPC ignores `TMPDIR`. Its installation helpers remain executable through the engine's declared sandbox paths.

When LibreOffice is found, namespace setup failure is fatal in both `best-effort` and `required` modes (`error[sandbox_unavailable]` on stderr, exit 5). `doctor` reports the failed `engine_libreoffice` check with the cause, and `engines` lists the engine as `cannot start`. Conversion keeps this error when LibreOffice was the planner's first choice; it does not silently fall back to a different-fidelity route. On Ubuntu, run `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`, or install an [AppArmor profile allowing `userns` for the `ashift` binary](https://documentation.ubuntu.com/security/security-features/privilege-restriction/apparmor/#apparmor-unprivileged-user-namespace-restrictions). The sysctl changes the host-wide policy; a binary-specific profile limits that grant. Default Docker and Kubernetes container seccomp profiles also block the namespace setup. See [worker runtime requirements](server.md#worker) for cloud deployments. `ASHIFT_SANDBOX=off` explicitly bypasses this requirement by disabling engine isolation; use it only when accepting that trade-off.

LibreOffice >=24.2 is optional: DOC/RTF/ODT → DOCX, XLS/ODS → XLSX, PPT/ODP → PPTX, and all Office/ODF formats → PDF. Chained routes include DOC → DOCX → IR → MD. LibreOffice only renders the input file itself; the planner never runs it on another engine's output. Markdown, HTML, EPUB and IR JSON therefore write PDF through Typst in every profile and never need LibreOffice or its namespaces. Without LibreOffice, discovery needs no private `/tmp` or namespace setup; DOCX → PDF falls back to Pandoc and Typst, and LibreOffice-only routes report the missing optional engine. Each job uses a fresh profile with macros disabled and Writer/Calc link updates set to never.

`--to json` writes IR JSON; `--json` instead selects the conversion report on stdout.

#### Package metadata name encoding

The shared stamper rejects packages when the physical central-directory record
count differs from the ZIP index, before writing output. This also protects the
browser/WASM bridge without relying on the host's input preflight.

For v1.0, DOCX/EPUB metadata stamping retains the current ZIP name normalization:
valid UTF-8 bytes without the UTF-8 flag are written back with that flag set.
A flag-zero `c3 a9 2e 74 78 74` name therefore changes from `├⌐.txt` to `é.txt`
for a reader that initially follows CP437.
The rationale is that OOXML/EPUB names are referenced from UTF-8 XML, so the
flagged form matches the package's own references and mainstream readers already
decode these names as UTF-8. Preserving name bytes and flags exactly is a v1.x item.

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

- Sniffs format from prefix magic bytes, HTML tags, ZIP central-directory inspection (OOXML and ODF), or bounded OLE2 stream names (DOC/XLS/PPT). JPEG, WebP, TIFF and RTF use magic bytes.
- Parses metadata (title, authors, language, date) and structural counts (headings by level, paragraphs, tables, images, links, footnotes, words).
- Deep document structure analysis is capped at 32 MiB (`ANALYSIS_MAX_BYTES`). Files larger than 32 MiB skip structural counting and emit `warning[document_too_large]: document too large to analyse`.
- Note that local conversion (`ashift convert`) enforces no local file size cap (cloud plans enforce entitlements per §9.5). For compressed archives (DOCX and EPUB), the 32 MiB analysis cap applies to the compressed file size on disk rather than the uncompressed IR document size.
- Peak memory usage during AST expansion is approximately 100x the input document size (~3 GB peak RSS on a 32 MiB markdown file).
- Lists reachable target formats that the planner can convert to.
- For PDF documents within the analysis cap, reports `page_count`, metadata and a `pdf` object with `pages`, `text_pages`, `image_only_pages`, `needs_ocr`, `encrypted`, and optional `producer`. A page without a text layer emits `ocr_required`; for OCR, install the Docling engine with `ashift engines install docling`. Encrypted documents open only with an empty user password; other encrypted or malformed PDFs fail with exit code 1.
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
- `--engine <ENGINE>`: Select the input reader, with the same content-aware PDF default and no reader fallback as `convert`. PDF planning reads the text layer under the conversion limits.
- `--profile <PROFILE>`: Optimization profile: `editable` (default), `faithful`, `fast`, or `private`.
- `--json`: Output plan results as JSON.

#### Behavior

- Refuses same-format conversions (e.g. `md -> md`) with exit code 3.
- Refuses unreachable routes with exit code 3.
- Renders routes with engine chain arrows (`markdown ─ariad-core→ ariad-ir+json ─pandoc→ docx`).
- Marks measured score edges from benchmark capabilities or notes unmeasured edges.
- Input file summary parsing is bounded to 4 MiB to keep planning fast. Files over 4 MiB skip deep IR summary parsing; below 4 MiB, malformed input documents fail planning with the underlying IR read error.
- External engine availability is checked. If an engine required by the planned route is not installed locally (e.g. `docling`), the score line displays `engine missing (<engine>)` (e.g. `engine missing (docling)`). If the engine was found but its launcher failed, the human score line shows `startup_failure (<engine>): <cause>` instead; the plan JSON contract is unchanged. The command still exits with code `0` because planning analyzes route feasibility rather than executing the conversion.
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
- Reports the built-in `pdf` reader and `typst` writer, probes Pandoc (`>=3.12,<4`) and optional LibreOffice (`>=24.2`), and lists their routes. A build with a disabled feature reports that engine as missing.
- Lists `docling` as `not installed` (note `available in v0.2`) until its pack is installed. When `ASHIFT_DOCLING_ENGINE` is set, the `docling` row comes from that engine's `describe` (its Docling version, `ready` once it answers) with the note `development override`. A docling engine without Tesseract 5 is still `ready`, with a note that OCR is unavailable, because digital documents convert without it.
- For every resolved engine pack (installed, or an `ASHIFT_PACK_<NAME>` development directory) runs the engine's `describe` through the pack entrypoint and reports its status. The row carries a `pack` object; routes come from the capabilities registry, or from `describe` for engines the registry does not know. A pack that is installed but cannot be used (damaged receipt, other platform, other `ashift` range) is listed as `missing` with a note pointing at `ashift doctor`.

#### Human Output Example (excerpt)

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

#### JSON Output Shape (excerpt)

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

Reinstalling an identical, intact version reports `already_installed` without downloading. Another `ashift` installing or removing the same pack makes the command fail with exit code 1. Progress goes to stderr when it is a terminal. Human-readable Docling install output lists the Tesseract prerequisite and, on Windows, the latest x64 Microsoft Visual C++ Redistributable prerequisite; see [Docling prerequisites](docling.md#what-it-needs).

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
3. **Engine Describe**: Verifies describe round trips for Pandoc, PDF, Typst and LibreOffice. LibreOffice is optional, so its absence does not fail `doctor`.
4. **Engine pack directory** (`data_dir`, not required): the pack data directory exists or can be created and is writable.
5. **Pack signing keys** (`pack_keys`, not required): lists the ids of the minisign keys built into `ashift`; a build without keys cannot install packs.
6. **Installed packs** (`pack_<name>`, not required): one check per installed version (receipt present, every recorded file present with its recorded size, built for this platform and `ashift` range) and per `ASHIFT_PACK_<NAME>` override. `ashift engines verify` does the full re-hash.
7. **Sandbox**: Reports effective mode, Landlock ABI and seccomp availability on Linux, or Seatbelt availability and enforcement on macOS. Missing isolation is an optional failed check in best-effort mode and a required failure in required mode. Required mode needs Linux isolation or usable macOS Seatbelt.
8. **Docling engine** (`docling_engine`, not required): the engine from `ASHIFT_DOCLING_ENGINE` or the `docling` pack answers `describe`; the message lists the Docling version and the models found. Without an engine the check fails and the hint says how to install one.
9. **Tesseract** (`tesseract`, not required; only when the Docling engine answered): Tesseract 5 is on `PATH` for Vietnamese OCR. Otherwise Vietnamese OCR is unavailable and the hint gives the install command.

#### macOS engine sandbox

On macOS, engine launches use the system `/usr/bin/sandbox-exec` when present.
The generated Seatbelt profile denies access by default. Engines can read their
installation and declared runtime paths, system libraries and fonts, and can
write only their job workspace (the session directory for pooled engines), plus
`/dev/null`. LibreOffice also gets a private IPC directory as described below.
Read-only engine library paths permit executable memory mappings without
permission to execute programs. The runner places engine temporary files inside
the job workspace.
Neither the user's home directory nor the host's general temporary directory is
granted. Homebrew and `/usr/local` state directories (`var`, `etc`) are not
readable through the system runtime grants. IP networking, including loopback,
and general UNIX socket access are denied; the engine protocol and sessions use
inherited stdin/stdout pipes. System identity is readable through `uname`. The
launcher uses a granted working directory and keeps Apple's fixed `/var`, `/tmp`
and `/etc` aliases scoped to the same paths. Relative engine executables and
`ASHIFT_PANDOC` / `ASHIFT_SOFFICE` overrides resolve against the caller's working
directory before the launcher changes directory.

Paths are supplied as separate `sandbox-exec -D` parameters, so quotes,
parentheses, backslashes and newlines in paths cannot inject profile rules.
The launcher validates the exact profile in a disposable child before starting
the engine, disables core dumps, and retains process-group cancellation and the
host's resource watchdog. A profile setup error or refused enforcement probe
fails the job with `sandbox_unavailable` in both `best-effort` and `required`
modes; it never silently retries outside Seatbelt.

The host's `doctor` diagnostics report both whether `sandbox-exec` exists and
whether a disposable child can apply the profile, consistently in the CLI and
desktop. A present but refused Seatbelt is a required diagnostic failure even
in `best-effort`; both enabled modes refuse before executing the engine. On macOS, `doctor --json` adds
`sandbox.seatbelt_available`, `sandbox.seatbelt_usable`, and
`sandbox.seatbelt_enforced`. The last field means that the selected mode enables
Seatbelt for engine launches and its enforcement probe succeeded; it is false
in `off` mode. Each engine's exact profile is still checked when launched.
When `sandbox-exec` is absent, `best-effort` runs with a
`sandbox_unavailable` warning, while `required` refuses to start the engine
(exit 5). `off` explicitly bypasses isolation. Apple deprecates `sandbox-exec`,
so its availability is checked rather than assumed.

LibreOffice stays inside Seatbelt on macOS. The host creates a short, private
IPC directory with owner-only (`0700`) permissions under `/private/tmp` for each
process and removes it after exit, cancellation or launch failure. Only this
directory permits UNIX socket bind,
listen and connect; sockets elsewhere, including the job workspace, remain
denied. The launcher sets LibreOffice's `OSL_SOCKET_PATH` to that directory,
keeping socket names below Darwin's length limit. The Office profile also permits
`com.apple.coreservices.launchservicesd`, needed by native spell checking during
headless conversion. The host's general `/tmp` and IP networking stay denied.
IPC creation and canonicalization failures use fixed, path-free diagnostics.
Linux continues to use its private `/tmp` mount.

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

### `ashift tools`

Lists all registered document manipulation tools from `ariad-core::tools`, optionally filtered by functional group. See [tools.md](tools.md) for the full tool contract.

```bash
ashift tools [--group <GROUP>] [--json]
```

#### Options

- `--group <GROUP>`: Filter tools by group (`organize`, `convert`, `optimize`, `security`, `overlay`, `ocr`, `annotate`, `forms`, `sign`, `redact`, `compare`, `edit`, `ai`, `automation`). Unknown group names fail with exit code `2` (`error[usage]`).
- `--json`: Emit the array of `ToolDescriptor` objects as JSON (including resolved Draft 2020-12 `params_schema`, `tier`, `runtimes`, `engine`, and `secret_params`).

---

### `ashift tool`

Executes a single registered document manipulation tool on one or more input files.

```bash
ashift tool <ID> <INPUT>... [-o <DIR_OR_FILE>] [--param <KEY=VALUE>]... [--params-json <FILE|->] [--secret <NAME=@FILE|->]... [--overwrite] [--json]
```

#### Arguments and Options

- `<ID>`: Canonical `<domain>.<action>` tool identifier (for example `pdf.info`, `pdf.merge`, `pdf.split`).
- `<INPUT>...`: One or more input document paths.
- `-o, --output <DIR_OR_FILE>`: Explicit output file or directory. When omitted, single-output and multi-output tools publish beside the first input using default naming, while `pdf.info` prints its inspection summary (or structured JSON with `--json`) without writing a file to disk.
- `--param <KEY=VALUE>`: Repeatable parameter override (`VALUE` is parsed as JSON when valid, otherwise as a string).
- `--params-json <FILE|->`: Read the tool parameter JSON object from a file or `-` (stdin). Explicit `--param` flags override keys from `--params-json`.
- `--secret <NAME=@FILE|->`: Supply a secret parameter (`x-ariad-secret: true`) from a file (`name=@/path/to/secret`) or stdin (`name=-`). Passing a secret parameter as a literal value via `--param`, `--params-json`, or `--secret name=literal` is rejected with exit code `9` (`error[invalid_params]`) and a hint to use `--secret <name>=@<file>` or `--secret <name>=-`.
- `--overwrite`: Replace existing destination file(s).
- `--json`: Emit structured JSON output to stdout (the tool's JSON payload directly when no `-o` file is written, for example `PdfInfo` for `pdf.info` or the bookmark list for `pdf.outline --param action=export`; `ToolOutcome` otherwise). Tools that produce only JSON print it to stdout without `--json` too (`pdf.info` prints a human summary instead).

#### Examples

```bash
# Inspect PDF structure, page boxes, metadata, forms, and xref repair status as JSON
ashift tool pdf.info fixtures/pdf-tools/multipage-20.pdf --json

# Write pdf.info output to a file
ashift tool pdf.info report.pdf -o report.info.json

# Merge PDFs with per-file bookmarks and continuous page numbering (-> rotated-pages.merged.pdf)
ashift tool pdf.merge fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param outline=per-file --param page_labels=renumber -o out/

# Split a PDF every 2 pages (-> rotated-pages.part-001.pdf, rotated-pages.part-002.pdf)
ashift tool pdf.split fixtures/pdf-tools/rotated-pages.pdf --param mode=every --param every=2 -o out/

# Extract pages 1-3 into a new PDF (-> multipage-20.extracted.pdf)
ashift tool pdf.extract_pages fixtures/pdf-tools/multipage-20.pdf --param pages=1-3 -o out/

# Delete pages 2 and 4 (-> rotated-pages.trimmed.pdf)
ashift tool pdf.delete_pages fixtures/pdf-tools/rotated-pages.pdf --param pages=2,4 -o out/

# Reverse page order (-> rotated-pages.reordered.pdf)
ashift tool pdf.reorder fixtures/pdf-tools/rotated-pages.pdf --param order=reverse -o out/

# Interleave two PDFs for duplex scanning, reversing the second document (-> rotated-pages.mixed.pdf)
ashift tool pdf.alternate_mix fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param reverse_second=true -o out/

# Insert 2 blank pages matching the preceding page size after page 1 (-> rotated-pages.inserted.pdf)
ashift tool pdf.insert_blank fixtures/pdf-tools/rotated-pages.pdf --param after=1-1 --param count=2 --param size=same -o out/

# Rotate page 1 by 90 degrees clockwise via /Rotate (-> rotated-pages.rotated.pdf)
ashift tool pdf.rotate fixtures/pdf-tools/rotated-pages.pdf --param pages=1-1 --param angle=90 -o out/

# Set /CropBox to 0.5-inch (36 pt) margins on a US Letter page (-> rotated-pages.cropped.pdf)
ashift tool pdf.crop fixtures/pdf-tools/rotated-pages.pdf --param pages=all --param 'box={"left":36,"bottom":36,"right":576,"top":756}' --param target=crop -o out/

# Scale pages to A4 while preserving aspect ratio (-> mixed-sizes.scaled.pdf)
ashift tool pdf.scale fixtures/pdf-tools/mixed-sizes.pdf --param pages=all --param to=A4 --param fit=contain -o out/

# Stitch all pages vertically into a single continuous page (-> rotated-pages.single-page.pdf)
ashift tool pdf.single_page fixtures/pdf-tools/rotated-pages.pdf -o out/

# Rotate page 1 content stream by 45 degrees and expand the page box (-> rotated-pages.rotated-content.pdf)
ashift tool pdf.rotate_content fixtures/pdf-tools/rotated-pages.pdf --param pages=1-1 --param angle_deg=45 --param expand=true -o out/

# Decompress content streams and expand PDF 1.5+ /ObjStm object streams (-> object-streams.decompressed.pdf)
ashift tool pdf.decompress fixtures/pdf-tools/object-streams.pdf --param object_streams=expand -o out/

# Impose 2 pages per A4 sheet in row order (-> rotated-pages.nup.pdf)
ashift tool pdf.nup fixtures/pdf-tools/rotated-pages.pdf --param per_sheet=2 --param sheet=A4 --param order=row -o out/

# Impose pages as a saddle-stitch booklet on A4 sheets (-> rotated-pages.booklet.pdf)
ashift tool pdf.booklet fixtures/pdf-tools/rotated-pages.pdf --param sheet=A4 --param binding=left --param creep_pt=0 -o out/

# Stamp the first page of an overlay PDF over all pages of a base PDF (-> rotated-pages.overlay.pdf)
ashift tool pdf.overlay fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/watermark-layer.pdf --param overlay_pages=first --param position=over --param pages=all -o out/

# Subdivide each page into a 2x2 grid of smaller pages (-> rotated-pages.divided.pdf)
ashift tool pdf.divide_pages fixtures/pdf-tools/rotated-pages.pdf --param pages=all --param cols=2 --param rows=2 --param order=row -o out/

# Tile a page across a 2x2 grid of A4 sheets with 10 mm overlap and crop marks (-> annotations.posterized.pdf)
ashift tool pdf.posterize fixtures/pdf-tools/annotations.pdf --param 'tiles={"cols":2,"rows":2}' --param sheet=A4 --param overlap_mm=10 --param crop_marks=true -o out/

# Update PDF Info dictionary and XMP metadata fields (-> rotated-pages.metadata.pdf)
ashift tool pdf.metadata fixtures/pdf-tools/rotated-pages.pdf --param 'set={"title":"Golden Title","author":"AriadShift"}' -o out/

# Clear the bookmark (/Outlines) tree (-> multipage-20.outline.pdf)
ashift tool pdf.outline fixtures/pdf-tools/multipage-20.pdf --param action=clear -o out/

# Embed an attachment into the PDF /EmbeddedFiles name tree (-> rotated-pages.attachments.pdf)
ashift tool pdf.attachments fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param action=add -o out/

# Attach any file type, such as a CSV, and keep its name and bytes (-> rotated-pages.attachments.pdf)
ashift tool pdf.attachments fixtures/pdf-tools/rotated-pages.pdf inventory.csv --param action=add -o out/

# List embedded files as JSON on stdout; no file is written (add -o list.json to save it)
ashift tool pdf.attachments out/rotated-pages.attachments.pdf --param action=list

# Extract embedded files unchanged (-> out/extracted/rotated-pages.attachments.attachment-001.inventory.csv)
ashift tool pdf.attachments out/rotated-pages.attachments.pdf --param action=extract -o out/extracted/

# Print the bookmark tree as JSON on stdout; no file is written
ashift tool pdf.outline out/rotated-pages.merged.pdf --param action=export

# Configure /PageLabels numbering ranges (-> multipage-20.labeled.pdf)
ashift tool pdf.page_labels fixtures/pdf-tools/multipage-20.pdf --param 'ranges=[{"from":1,"style":"roman-lower"},{"from":5,"style":"decimal","prefix":"p-","start":1}]' -o out/

# Set /ViewerPreferences layout and window options (-> rotated-pages.prefs.pdf)
ashift tool pdf.viewer_prefs fixtures/pdf-tools/rotated-pages.pdf --param page_layout=two-column-left --param fit_window=true -o out/

# Create a PDF portfolio (/Collection) from multiple files (-> rotated-pages.portfolio.pdf)
ashift tool pdf.portfolio fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param action=create --param view=details -o out/

# Remove a named optional-content layer (/OCProperties) and its marked content (-> watermark-layer.layers.pdf)
ashift tool pdf.layers fixtures/pdf-tools/watermark-layer.pdf --param action=remove --param 'names=["AriadShift Watermark"]' -o out/
```

---

### `ashift recipe run`

Executes a multi-step `ariad-recipe/1` JSON pipeline (`Step::Tool` and `Step::Convert`) inside an isolated workspace and atomically publishes the final step's outputs into the target directory.

```bash
ashift recipe run <RECIPE.JSON> <INPUT>... -o <DIR> [--yes] [--overwrite] [--json]
```

#### Arguments and Options

- `<RECIPE.JSON>`: Path to a recipe file conforming to `schemas/recipe.v1.json` (`"version": "ariad-recipe/1"`).
- `<INPUT>...`: Input files fed into step 1 of the recipe. On the Free plan, batch runs are capped at 25 files and 250 MB total (`exit_code: 11`, `error[tier_not_allowed]`).
- `-o, --output <DIR>`: Destination directory for published outputs.
- `--yes`: Confirm running an imported recipe without an interactive terminal prompt. Recipes loaded from `.json` files are classified as `Imported`, print their step summary to stderr, forbid `watch` and network-fetch tools (`pdf.from_url`, `web.to_pdf`), and require `--yes` in non-interactive scripts (otherwise exiting with code `9`, `error[invalid_params]`).
- `--overwrite`: Replace existing destination files in `<DIR>`.
- `--json`: Emit the `RecipeOutcome` JSON report to stdout.

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
- `ASHIFT_SOFFICE`: Selects a system LibreOffice executable. It overrides `PATH` and the standard install locations.
- `ASHIFT_DATA_DIR`: Data directory for engine packs (packs live under `<dir>/packs`). Defaults to the platform's local data directory for `com.ariadnev.ariadshift`; see [packs.md](packs.md#layout).
- `ASHIFT_PACK_<NAME>`: Uses an unpacked development pack directory (holding an unsigned `pack.toml`) instead of the installed pack `<name>`; the name is upper-cased with `-` written as `_`.
- `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`: Proxy settings for pack downloads.
- `SSL_CERT_FILE`, `SSL_CERT_DIR`: Trusted CA certificates for pack downloads on Linux, when the system store is not in a standard location.
- `ASHIFT_SANDBOX`: `off`, `best-effort` (CLI default), or `required`. Linux engines run through Landlock and seccomp with workspace write access and declared runtime read/execute access. Missing kernel confinement support produces `warning[sandbox_unavailable]` in best-effort mode; required mode refuses it with exit 5. Launcher enforcement errors, including namespace setup denial for a discovered LibreOffice installation, are fatal in every mode except `off` (`sandbox_unavailable`, exit 5). On Ubuntu, allow `userns` for `ashift` in AppArmor or use `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`; default container seccomp profiles also block setup. `ASHIFT_SANDBOX=off` explicitly trades engine isolation for running without the launcher. Unknown values fail closed. macOS uses [Seatbelt when available](#macos-engine-sandbox) and retains process groups; Windows retains Job Objects and cannot satisfy required mode.
- Linux local IPC: `required` needs Landlock filesystem rules and seccomp, while `AF_UNIX` remains allowed. Pathname UNIX socket confinement needs Landlock ABI 9; abstract sockets and signals need ABI 6. On older kernels local services may remain reachable. `doctor --json` reports `landlock_unix_sockets` and `landlock_scopes`; use ABI 9 or stronger container isolation when local IPC must be blocked.
- `SOURCE_DATE_EPOCH`: Unix timestamp in seconds used for deterministic, byte-reproducible ZIP archives (DOCX and EPUB) and for the Typst PDF date. LibreOffice outputs are structurally tested and are not byte-reproducible.
