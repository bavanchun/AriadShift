# Editor round-trip preservation contract

This corpus specifies preservation for future Document mode editors. It does
not establish that Tiptap, TextKit, or `edit.export_document` is implemented.
The [IR design](../ARCHITECTURE.md#62-document-ir),
[Document mode plan](../plans/261031-1422-next-stage-pdf-suite/phase-12-document-and-page-edit-engine.md),
and [web editor plan](../plans/261031-1422-next-stage-pdf-suite/phase-16-web-editor-and-ai.md)
own the production boundaries. Page mode edits source PDF objects separately.

## Baseline and coverage

The corpus starts at `4b51bdf9b786be2d06553eee49f4d72891dbbfab`.
The frozen `/1` schema SHA-256 is
`92d3ef590f3f8dde533c938e14d5c7695b877978bc9cb246fc547aed976d96b5`.
The integrated `/2` schema is published at [schemas/ir.v2.json](../schemas/ir.v2.json), with SHA-256 `366d4141ccff677c58c6837733483b498449dc8de71e99b1df490b575ecfdd89`. Corpus tests use the real `/2` types and validate the published schema unconditionally.

The table was recorded before generation. Every row names a distinct primary
document under `fixtures/editor/`; truth and image companions do not count.

| Primary stem (extension) | Required coverage |
|---|---|
| `linked-footnote` (`ir.json`) | Linked semantic note and separate block identity |
| `reused-footnote` (`ir.json`) | Two references share one semantic note |
| `nested-footnote` (`ir.json`) | Note containing quote and nested list |
| `raw-html-block` (`ir.json`) | Exact HTML, whitespace, Unicode and CRLF |
| `raw-tex-block` (`ir.json`) | Exact TeX with comments and spacing |
| `inline-raw` (`ir.json`) | Both raw formats between supported text |
| `captioned-figure` (`ir.json`) | Original embedded PNG and caption |
| `contained-figure` (`ir.json`) | Figure inside a quote |
| `table-colspan` (`ir.json`) | Horizontal cell span |
| `table-rowspan` (`ir.json`) | Vertical cell span |
| `table-combined-spans` (`ir.json`) | Both spans and explicit row headers |
| `table-caption` (`ir.json`) | Rich caption distinct from cell text |
| `table-footnotes` (`ir.json`) | Table note linked to a semantic footnote |
| `nested-cell-blocks` (`ir.json`) | Paragraph, quote, list and nested table |
| `running-header` (`ir.json`) | Header furniture, section reference, source box |
| `running-footer` (`ir.json`) | Footer furniture, section reference, source box |
| `inline-math` (`ir.json`) | Exact inline TeX |
| `display-math` (`ir.json`) | Display formula and display flag |
| `styled-nesting` (`ir.json`) | Vietnamese, lists/quotes, named/direct cascade |
| `mixed-document` (`ir.json`) | All features, two sections, split source fragments |
| `vi-field-notes` (`md`) | Real Markdown with notes, math and nested lists |
| `vi-field-notes` (`html`) | Real HTML with caption, spans and explicit styles |
| `vi-styled-pair` (`docx`) | Authored paired text, font, size, weight and colour |
| `vi-styled-pair` (`pdf`) | Same paired text/styles and explicit page geometry |

The original Apache-2.0 generator is
[`editor_roundtrip.py`](../fixtures/gen/src/ariad_fixture_gen/editor_roundtrip.py).
[`manifest.toml`](../fixtures/manifest.toml) records each primary and companion's
SHA-256 and inherited source, licence and attribution. Each `.truth.json`
contains authored semantic expectations, style expectations, layout intent,
and preservation assertions. Truth is created from original specifications,
never by reading generated PDF/DOCX or scoring an output against itself.
Layout assertions distinguish authored intent from measured extraction boxes;
only explicitly supplied boxes are numeric reference geometry.

## Identity and reconciliation

Every body and furniture block, including nested blocks, retains its `BlockId`.
An editor stores that ID in native node attributes or a durable attributed-text
anchor. Side payloads use the same ID. Positional paths are never a recovery key.
IDs generated at initial import need not survive a fresh import of edited source;
existing valid `/2` IDs must survive edits, insertions and moves. Copying a node
creates fresh IDs for the copy while preserving the original subtree IDs.

On export the editor walks live IDs, joins side payloads, and checks for duplicates,
dangling references and ambiguous orphans. Editing adjacent text or moving a block
does not change an opaque payload. Explicit deletion removes its payload and
updates references; deleting a container removes its owned subtree. Shared note
definitions/assets remain while referenced elsewhere. An unmatched payload is
reported as an orphan; it must never be attached to the block now at its old index.
Undo restores the ID and payload together. Import/export errors must retain the
source document for recovery and identify the affected feature and block.

`Footnote.note_id` and `Inline.FootnoteRef.id` identify a semantic note.
`Footnote.id` identifies the definition block. Reused references retain one
definition; renumbering visual markers does not rename either namespace. `/1`
uses `Footnote.id` for the semantic ID and cannot store the `/2` block identity.

## Feature preservation requirements

These are required future editor mappings, not claims about installed extensions.
“Side payload” means out-of-band IR keyed by the owning `BlockId`; inline payloads
also need durable editor anchors within that owner, never an untracked text offset.

| Feature | Editor representation | Side payload | Export reconciliation | Preservation gate | Unsupported/loss notice |
|---|---|---|---|---|---|
| Footnotes | Visible note marker; editable definition where supported | Complete definition subtree, `note_id`, reference anchors | Resolve semantic refs independently of block IDs; preserve reused refs | Edit neighbour, reorder definition, retain both refs and nested content | Missing definition or unsupported note editing must be visible |
| Raw HTML / TeX blocks | Labelled atomic node, optional source editor | Exact `format` and `text` bytes | Restore by live ID; no parsing, sanitizing or whitespace normalization in stored IR | CRLF, decomposed Unicode and TeX comments survive JSON and moves | HTML rendering sanitizes unsafe content; other targets may drop raw content with notice |
| Inline raw | Anchored atomic inline | Format, exact bytes and anchor under owner ID | Keep its order relative to edited supported runs | Edit both adjacent runs without changing raw bytes | Missing inline support must show a protected placeholder |
| Figures and captions | Image node with caption editor | Figure identity, asset ref, rich caption and unsupported caption runs | Rejoin caption with its figure, including inside containers | Move container; compare PNG bytes and caption | External assets unresolved at export need a notice; captions must not become unrelated paragraphs silently |
| Assets | Preview/resource handle | Content-addressed original bytes and media type; document asset store | Preserve hash refs; prune only after explicit deletion and reference check | Figure and inline asset references resolve after reorder | Missing/corrupt asset is an error or an explicit placeholder loss |
| Table spans | Table grid with native spans where available | Original cells, rowspan, colspan and nested block IDs | Map cells by identity; reject overlapping or ambiguous merge recovery | Horizontal/vertical/combined spans survive adjacent edits | A target unable to express spans must announce flattening |
| Table captions | Attached caption, optionally editable | Full caption inline tree | Restore under table ID | Rich caption remains distinct from first row | Dropped/flattened caption needs notice |
| Table notes | Attached note area or protected marker | `footnotes` inline tree and semantic references | Restore table notes separately from document note definitions | Note refs resolve after moving table and definition | Flattening into text must be reported |
| Row headers | Header cell semantics | Explicit `header`, including false | Retain row/column header meaning through native grid mapping | Combined-span case retains header flags | Loss of accessible header semantics needs notice |
| Nested cell blocks | Paragraph/list/quote/table nodes in cell | Unsupported subtrees, their IDs and anchor order | Preserve complete subtree under owning table | Nested table and list survive edit of neighbouring cell | Plain-text cell fallback is a reported loss |
| Running header/footer | Page furniture controls or protected region | Furniture blocks, section refs, identity and geometry | Keep furniture out of body and preserve section ownership | Move body blocks without relocating furniture payload | Targets without running furniture need notice before export |
| Math | Inline/display formula node and source | Exact TeX, `display` flag and anchor | Render a preview; export original source, not preview text | Inline/display byte equality through edits | Failed conversion keeps source with `math_not_converted` or equivalent typed notice |
| Named/direct styles | Paragraph and character controls | StyleSheet, refs and unexposed direct properties | Defaults, inherited paragraph chain, named style, then direct overrides; inner spans inherit | Vietnamese styled nesting retains explicit false flags and overrides | Unsupported styles cannot silently reset to defaults |
| Sections | Page setup controls and start anchors | Page size, margins, columns, start/header/footer IDs | Validate live ordered start IDs; first section alone may omit start | Move non-start body blocks; preserve two section boundaries | Reflow/section flattening must be disclosed |
| Layout/source map | Selection/citation metadata | One layout box and all source fragments per ID | Keep original-source boxes attached to IDs; mark edited geometry stale and recompute output layout separately | Split fragments survive adjacent edit and reorder | Source geometry is not a promise of unchanged pagination |
| Provenance | Read-only document metadata | Extension-keyed original values; ID-bearing values follow owner | Preserve unknown values; append actual processing history separately | Nested extension data survives serialization | Unsupported provenance export is a metadata loss, never fabricated history |
| Font permissions | Family/fallback selector | Asset font metadata and preserved permission evidence | Embed only `allowed`; `restricted`/`unknown` require permitted fallback or typed inability | Future export gate checks no forbidden embedded bytes | Installed family availability never grants embedding permission |

Opaque preservation applies to IR storage and editor carry-through. A sanitized HTML
render, flattened Markdown, PDF drawing or DOCX conversion need not retain raw
source bytes. Export notices and fidelity distinguish target limitations from an
editor losing information before the writer runs. Explicit deletion is intentional
loss and belongs in the change list, not an invented fidelity success.

## Verification and integration gates

Run the scoped generator twice and then its read-only check:

```sh
uv run --package ariad-fixture-gen python -m ariad_fixture_gen --only editor-roundtrip
uv run --package ariad-fixture-gen python -m ariad_fixture_gen --only editor-roundtrip
uv run --package ariad-fixture-gen python -m ariad_fixture_gen --only editor-roundtrip --check
uv run --package ariad-bench pytest fixtures/gen/tests/test_editor_roundtrip.py
cargo build -p ariad-cli
cargo test -p ariad-core --test editor_roundtrip
cargo test -p ariad-cli --test fixture_manifest
uv run --directory bench pytest tests/test_editor_roundtrip_corpus.py -v
uv run --directory bench python -m ariad_bench.editor_roundtrip --check
ARIAD_EDITOR_REQUIRE_STYLES=1 cargo test -p ariad-core --test editor_roundtrip
```

During coordinated runs wrap heavy commands in the coordinator's existing build
lock and use the assigned target directory. The preparation run performs Python
and documentation work first, as directed. `--check` compares generated bytes,
declared generated paths and recorded hashes without rewriting fixtures or the
manifest; drift exits nonzero. It also works for other generator selections.

The Python tests prove primary-document count, exact case coverage, independently
authored style/layout values, real file structures, scoped determinism and negative
read-only drift detection. The Rust suite exercises real JSON read/write, adjacent
text edits, block moves, table/note preservation, HTML/Markdown losses, and shared
CLI PDF/DOCX conversions. The styled DOCX-to-PDF contract selects the Pandoc reader and shared IR/Typst writer, using the bundled fonts; competitive measurements keep the default planner route. Builds with Typst also provide those fonts to LibreOffice through its private job profile. It never starts a build from a test subprocess. Set
`ARIAD_EDITOR_TEST_ASHIFT` to an already built binary if it is outside the assigned
Cargo target; missing binaries or engines fail the integration test.

The tests deserialize complete authored `/2` documents and assert stable IDs, styles, sections, furniture, font metadata and source maps after adjacent edits and block moves. No legacy projection or optional style gate remains. The shared style cascade applies paragraph defaults, the containing table paragraph baseline, inherited/named paragraph styles and direct overrides; inner character spans retain explicit false resets.

The baseline native PDF reader reconstructs paragraphs, headings and lists, but
does not assemble table blocks. Paired tests explicitly record zero recovered
tables after a PDF read against the one authored table; DOCX import must recover
that table. This is a known structural loss, not table acceptance or an invented
runtime warning. The integrated native-PDF gate retains this explicit zero-table expectation; authored table truth remains unchanged, while DOCX-only paths require the authored table count.

The paired truth companions also include canonical benchmark data and a fully
authored `/2` `ir` reference. Original IR cases carry their authored `/2` source
alongside separate explicit expectations. Text, style and structure expectations
come from original authoring, never converter read-back.

The paired reference also carries word bounds measured from the unchanged authored
PDF with `pdftotext 26.10.0 -bbox-layout`, frozen in the generator with the source
PDF hash. Its nine body boxes cover the heading, paragraph, six cell paragraphs
and the union of cell text for the table; two more boxes locate furniture.
An independent PDFium check verifies that the boxes contain the original glyphs.
These are common target coordinates for both formats; the independently authored
DOCX has its own reflow and is not assumed to have identical pagination.
Missing output boxes contribute zero under the unchanged scorer. A zero IoU tie
passes the non-regression inequality but does not establish visual preservation.

The integrated suite validates all authored documents against the `/2` schema and exercises real PDF/DOCX exports with style read-back. The styled pair has
the same source text/styles but different containers; it supports both competitor
directions without treating either format's read-back as truth. Numeric style
gates use size tolerance 0.5 pt, exact weight and maximum RGB-channel error 2/255. The corpus font-identity assertions accept the display and space-free PostScript family spellings evidenced by the paired containers; native PDF import now restores display family names for the three bundled families, while the shared fidelity scorer retains exact family-name comparison.
Do not infer those style gates passed from a successful conversion alone.

Future gates remain explicit: native IR → Tiptap → IR and IR → TextKit → IR,
undo/delete/copy/orphan handling, forbidden-font embedding, and full Document mode
open/edit/export with fidelity. No identity mapping substitutes for those tests.
