# Quality: Testing and Benchmarking

This document describes the empirical benchmark harness (`bench/`), its metrics, reference baselines, execution procedures, and how the planner consumes the resulting capabilities data.

---

## 1. Overview

AriadShift evaluates conversion engines empirically rather than relying on assumed capability claims. The benchmark harness (`ariad-bench`, managed via `uv`) measures reader edges ($X \rightarrow \text{IR}$) and writer edges ($\text{IR} \rightarrow Y$) through dedicated CLI hooks:
- `ashift __ir <INPUT> -o <OUTPUT> [--overwrite]`
- `ashift __write <INPUT> --to <FORMAT> -o <OUTPUT> [--overwrite]`

The resulting empirical quality and performance metrics are committed to `crates/ariad-core/data/capabilities.json`. The conversion planner consumes this data to select the optimal route for a given conversion request and profile.

---

## 2. Metric Definitions and Formulas

All metrics are aggregated across the fixture suite for each edge and rounded to fixed precisions.

### 2.1 Quality Metrics

#### Character Error Rate (`text_cer`)
Measures raw textual accuracy using `jiwer`:
$$\text{text\_cer} = \text{CER}(\text{canonical}(\text{ref}).\text{text}, \text{canonical}(\text{hyp}).\text{text})$$
Canonical text is NFC-normalized with sequences of contiguous whitespace collapsed into a single space.

#### Heading Tree Edit Distance (`heading_ted`)
Measures heading structural hierarchy retention using normalized Tree Edit Distance (TED) computed by `apted`:
$$\text{heading\_ted} = \frac{\text{TED}(T_{\text{ref}}, T_{\text{hyp}})}{\max(|T_{\text{ref}}|, |T_{\text{hyp}}|)}$$
Heading nodes are structured hierarchically as `(level, text)`. Rename operations between heading nodes compare text content via normalized Levenshtein distance from `rapidfuzz`.

When both reference and hypothesis documents contain no headings, structural distance is 0.0 (full credit, $1.0 - \text{heading\_ted} = 1.0$). If only one document contains headings while the other contains none, $\text{heading\_ted} = 1.0$. When both contain headings, the edit distance is normalized by the maximum count of actual heading nodes: $\max(|T_{\text{ref}}|, |T_{\text{hyp}}|)$.

#### Table Tree Edit Distance-based Similarity (`teds`)
Measures table structure and content preservation following Zhong et al. 2019 ("Image-based table recognition: data, code, and evaluation"):
$$\text{TEDS} = 1.0 - \frac{\text{TED}(T_{\text{ref}}, T_{\text{hyp}})}{\max(|T_{\text{ref}}|, |T_{\text{hyp}}|)}$$
Tables are represented as HTML-like trees: `table` $\rightarrow$ `thead`/`tbody` $\rightarrow$ `tr` $\rightarrow$ `th`/`td` (annotated with spans `colspan` and `rowspan`) $\rightarrow$ cell text. Cell content comparison uses normalized Levenshtein distance via `rapidfuzz`.

To prevent excessive latency during large test runs, TEDS is capped at tables with $\le 500$ cells; tables exceeding this limit are skipped and recorded in sample statistics. Only fixtures containing tables participate in TEDS scoring.

#### Scoring Policies and Structural Limitations
- **Furniture Exclusion Policy:** DOCX header and footer furniture elements (such as running headers, page numbers, and copyright notices) are excluded from scoring on both sides (omitted from authored truth companions and ignored in `from_ir`). Scoring focuses strictly on body content.
- **List Nesting Depth:** List structures capture ordered/unordered type and item text sequences in document order. Multi-level nested list hierarchy depth is a documented limitation in 1a and is flattened sequentially in canonical form.

#### Overall Fidelity (`fidelity`)
Combines text accuracy, heading hierarchy preservation, and table structure:
- **Documents without tables:**
  $$\text{fidelity} = \frac{(1.0 - \text{text\_cer}) + (1.0 - \text{heading\_ted})}{2.0}$$
- **Documents with tables:**
  $$\text{fidelity} = \frac{(1.0 - \text{text\_cer}) + (1.0 - \text{heading\_ted}) + \text{teds}}{3.0}$$
Each term is clamped to $[0.0, 1.0]$. The edge score is the arithmetic mean across all scored fixtures, rounded to 3 decimal places.

#### Editability (`editability`)
Measures the proportion of structural blocks in the reference document that survive in the converted document as the same structure kind, using text-matched overlap:
- Headings are matched by exact text occurrence counts.
- Lists are matched when item content overlap is at least 50%.
- Tables are matched when cell content overlap is at least 50%.
$$\text{editability} = \frac{H_{\text{survived}} + L_{\text{survived}} + T_{\text{survived}}}{H_{\text{ref}} + L_{\text{ref}} + T_{\text{ref}}}$$
If the reference document contains no headings, lists, or tables, editability is 1.0 if the hypothesis also contains none, or 0.0 if extraneous structures were introduced.

### 2.2 Performance Metrics

#### Latency (`p50_ms`)
The median wall-clock execution time across repeated runs (default: 3 repetitions), rounded to the nearest 10 ms. For ultra-fast native commands executing in under 5 ms, the rounded metric reports `0.0` ms (indicating sub-5 ms latency, not absent data).

#### Peak Memory (`peak_mem_mb`)
The peak resident set size (RSS) across the entire child process tree (including sub-processes spawned by external engines), sampled using platform-appropriate high-water mechanisms, rounded to the nearest 1 MB. Specifically, `peak_mem_mb` reports the sum of the per-process high-water marks (`VmHWM` on Linux / `peak_wset` on Windows / RSS) across the process tree (`ashift`, an intermediate child process and the engine subprocess). That sum is an upper bound on the simultaneous tree RSS. It exceeds the single-process `wait4` `ru_maxrss` (which reports only the largest process in the tree) by the footprint of the other processes in the tree: about 15 MB for the Pandoc edges (`ashift` about 8 MB plus an intermediate child of about 6-7 MB). Measurements reproduce within a tolerance rule of $\max(2\,\text{MB}, 25\%)$.

To prevent harness memory leakage, the sampler measures only verified post-exec processes (matching target `exe()` or command line without false substring directory matching). On POSIX systems, `fork()` initially duplicates the harness memory footprint; sampling pre-exec would falsely attribute the parent harness's RSS to lightweight child commands. The harness verifies that `execve` has completed before recording samples.

Platform measurement mechanisms:
- **Linux:** Reads the kernel high-water mark (`VmHWM`) directly from `/proc/<pid>/status` with recursive child process tracking across all threads via `/proc/<pid>/task/*/children`. This captures the true peak resident memory even for fast-executing native commands (~3–8 ms) and external engines launched from worker threads.
- **Windows:** Queries process memory info using `peak_wset` (peak working set).
- **macOS / BSD:** Instantaneous RSS sampling provides a lower-bound estimate.

If a command finishes before any post-exec sample can be obtained, it is recorded as a sampling miss (`None`, never clamped to 0.0 or harness memory). If all samples on an edge are misses, the edge fails verification and its metrics are omitted (`edge["metrics"] = None`), avoiding invalid null values in `capabilities.json`. An all-miss edge causes the benchmark run to exit with a non-zero code, and `bench-check` rejects any capabilities file where an edge that was measured in the committed baseline becomes null.

#### Samples (`samples`)
The total number of fixtures evaluated for the edge. Every measured edge must have at least 2 scored fixtures.

---

## 3. Reference Standards

In accordance with Principle 5 ("Evidence over claims"), an engine is **never scored against its own reading of the same file**.

### 3.1 Authored Truth Companions (Single-Source Generation)
For synthetic and generated fixtures (DOCX, HTML, EPUB), `fixtures/gen` generates an authored companion (`<id>.truth.json`) alongside the rendered fixture from a single unified ordered block model.
- Every fixture specification defines an ordered list of high-level blocks (`HeadingBlock`, `ParagraphBlock`, `ListBlock`, `TableBlock`, `FigureBlock`, `FootnoteBlock`, `ChapterBreakBlock`).
- Format renderers render the binary or markup document directly from this block sequence: HTML and EPUB renderers honor structural attributes including `Cell.is_header` for table header cells (`<th>` vs `<td>`) and explicit `colspan` and `rowspan` cell attributes. The DOCX renderer honors `is_header` (rendered as bold header cells) and `colspan` (rendered as horizontally merged cells), while `rowspan` is currently ignored as no synthetic DOCX fixture uses multi-row vertical spans. There are no bespoke per-fixture rendering functions or special-cased branches.
- The same block sequence directly generates the authored truth companion containing the canonical document structure and the document-ordered Ariad IR JSON representation.
- Fixture immutability is verified via SHA-256 digests recorded in `fixtures/manifest.toml`, ensuring byte-level tamper detection, while structural correctness is validated through independent format extraction and the structural drift mutation detection gate (`just bench-mutation`).

### 3.2 Markdown Source
For native Markdown fixtures, reference canonical documents are parsed using Pandoc's GFM reader (`pandoc -f gfm -t json`). This provides an independent reference standard from AriadShift's `comrak`-based native Markdown reader.

### 3.3 Unscored Fixtures
Fixtures are marked unscored in two situations:
- **Structural constructs exceeding canonical form:** Fixtures containing structures beyond canonical representation (e.g. `edge-case-merged-table.docx`, which features a nested table inside a merged cell) are marked unscored with an explicit `unscored_reason`.
- **Public documents without authored ground truth:** External public documents lacking independent authored ground truth (such as government circulars like `vn-tt-28-2012-conformity.docx`) are excluded from reader-edge fidelity scoring and recorded as unscored.

---

## 4. Normalization Rules

The canonical normalizer (`ariad_bench.canonical`) converts diverse representations (Ariad IR, Pandoc JSON AST, and truth companions) into a uniform `CanonicalDoc` structure:
- **Unicode NFC and Whitespace:** Text content is normalized to Unicode NFC. Block separators are preserved as newlines (`\n`), while runs of contiguous whitespace inside each block are collapsed into single spaces.
- **Raw HTML Stripping:** Raw inline HTML tags (such as `<mark>`, `<font>`, `<span>`, or structural wrappers) are stripped to retain pure textual meaning.
- **Footnote Ordering:** Footnote contents are extracted and appended at the end of body text in document order, ensuring inline reference markers do not disrupt sentence continuity.
- **Task Markers:** GitHub-style task list indicators (`[ ]`, `[x]`) are normalized from Ariad IR `checked: bool` properties to emit standard prefixes (`☒ ` / `☐ `), matching Pandoc GFM task markers.
- **Table Grids and Spans:** Tables are reconstructed into two-dimensional cell grids with explicit `colspan` and `rowspan` span annotations on cell nodes (rather than expanding into duplicate dummy cells), distinguishing header cells (`th`) from data cells (`td`).

---

## 5. Known Engine Biases

### Writer Read-Back Bias
Writer read-back bias applies when measuring writer edges ($\text{IR} \rightarrow Y$):
- **Pandoc writer edges:** For binary/package formats ($\text{IR} \rightarrow \text{docx}$ and $\text{IR} \rightarrow \text{epub}$), the generated output is read back into IR via Pandoc. Because Pandoc reads back the format it produced, writer edges using Pandoc may score higher on structural fidelity than if read by an independent reader.
- **Native writer edges:** For text/markup formats ($\text{IR} \rightarrow \text{html}$ and $\text{IR} \rightarrow \text{markdown}$), the generated output is read back into IR using AriadShift's native reader (`ashift __ir`), which similarly favors the native writer's structural serialization conventions.

By contrast, **reader edges** ($X \rightarrow \text{IR}$) avoid self-scoring: generated fixtures compare against independent authored truth companions written by `fixtures/gen`, and Markdown fixtures compare against Pandoc's GFM AST.

---

## 6. Running the Benchmark Harness

### Prerequisites
Ensure `ashift` and `pandoc` are available:
```bash
cargo build --release -p ariad-cli
just pandoc
```

### Common Commands
- **Run complete benchmark and update capabilities:**
  ```bash
  just bench
  ```
- **Validate capabilities against schema and invariants:**
  ```bash
  just bench-check
  ```
- **Run benchmark unit tests:**
  ```bash
  just bench-test
  ```
- **Run structural mutation detection gate:**
  ```bash
  just bench-mutation
  ```
- **Compare current capabilities against git baseline:**
  ```bash
  uv run --package ariad-bench python -m ariad_bench diff
  ```
- **Run benchmark on specific edges or fixtures:**
  ```bash
  uv run --package ariad-bench python -m ariad_bench run --edges "html->ariad-ir+json" --repeat 1 --out target/scratch/cap.json
  ```

---

## 7. How the Planner Consumes Metrics

The planner in `ariad-core` loads `capabilities.json` at compile time via `embedded()` and runtime. Optimal routes are computed via Dijkstra's shortest-path algorithm over the capability graph.

### Multi-Hop Route Score Aggregation
When a planned conversion path traverses multiple capability edges $e_1, e_2, \dots, e_k$:
- **Route Fidelity:** Quality attenuates multiplicatively:
  $$\text{fidelity}_{\text{route}} = \prod_{i=1}^{k} \text{fidelity}(e_i)$$
- **Route Editability:** Structure survival compounds multiplicatively:
  $$\text{editability}_{\text{route}} = \prod_{i=1}^{k} \text{editability}(e_i)$$
- **Route Duration:** Execution latencies sum additively:
  $$\text{p50\_ms}_{\text{route}} = \sum_{i=1}^{k} \text{p50\_ms}(e_i)$$
- **Peak Memory Residency:** Overall memory is bounded by the bottleneck edge:
  $$\text{peak\_mem\_mb}_{\text{route}} = \max_{i=1}^{k} \text{peak\_mem\_mb}(e_i)$$

### Edge Cost Formula
For an edge with measured empirical metrics, its routing cost is:
$$\text{cost} = w_{\text{fidelity}} \times (1.0 - \text{fidelity}) + w_{\text{editability}} \times (1.0 - \text{editability}) + w_{\text{time}} \times \min\left(1.0, \frac{\text{p50\_ms}}{5000\,\text{ms}}\right) + \text{hop\_penalty}$$
where:
- $\text{hop\_penalty} = 0.05$ (penalizes unnecessary intermediate conversion hops).
- Execution latency is normalized against a $5{,}000\,\text{ms}$ baseline duration.
- For unmeasured edges without empirical metrics, a pessimistic default cost of $1.5 + \text{hop\_penalty}$ is assigned, ensuring measured edges strictly beat unmeasured edges.

### Profile Weights
| Profile | $w_{\text{fidelity}}$ | $w_{\text{editability}}$ | $w_{\text{time}}$ | Description |
|---|---|---|---|---|
| `editable` | 0.30 | 0.60 | 0.10 | Prioritizes semantic editability (headings, clean lists, tables) |
| `faithful` | 0.70 | 0.20 | 0.10 | Prioritizes layout and visual preservation |
| `fast` | 0.15 | 0.15 | 0.70 | Prioritizes conversion speed |
| `private` | 0.30 | 0.60 | 0.10 | Filters graph strictly to local/WASM runtimes, then applies `editable` weights |
