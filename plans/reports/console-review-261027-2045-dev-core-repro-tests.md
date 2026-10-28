# Regression tests for the confirmed panic, stack-overflow and super-linear findings in `ariad-core`

Date: 2026-10-27 (Asia/Saigon). Ref: `origin/dev` at `e6bd29b`, the commit both earlier reports reviewed. The working tree has no diff against `origin/dev` under `crates/ariad-core` or `fuzz`. Input: `console-review-261027-2035-verify-dev-core.md`. Read-only: nothing was built, compiled, run or fuzzed. Every test below was checked against the code by reading only, so the time bounds are estimates. Upstream behaviour (comrak 0.56.0, html5ever 0.40.1) comes from the verify report, because the cargo registry was outside this session's readable paths.

## 1. Verdict

Needs fixes. Each in-scope finding below has a test that should fail on `e6bd29b`, by a panic, a stack overflow, a wrong result or a missed time bound, and pass once the sketched fix lands.

## 2. Findings and tests

### 2.1 Scope

In scope: H1 (panic), H2 (stack overflow), and the super-linear findings H3–H8, M1, M4, M5, M8, M9 and M13. M4, M9 and M13 grow with the product of two input sizes, and one of those sizes has a cap (JSON depth 576, `MAX_ENTRIES`, DOM depth 1024), so they are bounded super-linear.

Out of scope: M2, M3, M6, M7, M11 and M12 are correctness defects. M10 is a linear 16–32× memory factor (each 2-byte `[]` becomes a 32-byte `Operand`), not super-linear.

New observation, verified by reading: the Typst half of M5 is bounded. `TableGrid::new` caps the width at `MAX_TABLE_COLUMNS = 128` (`writer/typst.rs:458-463`), so an empty IR row costs at most 128 `[]` cells. That is linear, so no test is given for it.

### 2.2 Findings table

| # | Sev | Location (origin/dev) | Defect | Failure scenario | Suggested fix | Test (target file) |
|---|---|---|---|---|---|---|
| H1 | high | `src/writer/mod.rs:103-104`, `:146-147`, `:159-160` | `strip_html_tags` slices `&str` at byte 6, byte 5 or `tag_name.len()` after only a length check. | `strip_html_tags("<p>Tiếng Việt</p>")` panics (byte 6 is inside `ế`). So does Markdown `<p>Tiếng Việt</p>` converted to Markdown, HTML or Typst/PDF, and the inline `<a b=é>` converted to HTML. | Compare byte prefixes (2.4). | `strip_html_tags_keeps_multibyte_text_next_to_tag_names`, `vietnamese_raw_html_from_markdown_reaches_every_text_writer` (`tests/writer_roundtrip.rs`); `raw_html_with_vietnamese_text_is_stripped_without_panicking` (`tests/typst_writer.rs`) |
| H2 | high | `src/reader/markdown.rs:105-125`, `:813-829` | The walk counts only block containers. The prescan counts an emphasis opener only after whitespace. | `"*a(".repeat(N) + "x" + "a*)".repeat(N)` nests `Emph` N deep and passes. Drop, `Debug`, serde and the writers then recurse N deep. | Count inline containers in the walk (2.4). | `inline_nesting_counts_toward_the_limit_when_the_prescan_misses_it`, `deep_inline_nesting_is_rejected_on_a_wasm_sized_stack` (`src/reader/markdown.rs` tests) |
| H3 | high | `src/reader/html_sink.rs:199-225`, `:129-135` | The node budget counts arena nodes, while every formatting reconstruction stores a full clone of the attribute vector. | One `<b>` with 1,000 attributes followed by `<p>t` × M keeps M × 1,000 attributes, while the arena grows by only 3 nodes per `<p>t`. | Charge `1 + attrs.len()` per element before any filtering (2.4). | `attribute_heavy_formatting_reconstruction_is_charged_to_the_node_budget` (`tests/html_reader.rs`) |
| H4 | high | `src/reader/html.rs:849-850`, `:1389-1406` | `is_paired_with_math_tex` re-walks three ancestor subtrees plus every sibling for each helper element. | 30k `<span class="katex-html">` siblings under one `<div>` cost about 5e9 node visits. | One post-order "subtree has TeX" pass, then O(1) checks (2.4). | `many_unpaired_math_helper_siblings_parse_in_linear_time` (`tests/html_reader.rs`) |
| H5 | high | `src/reader/markdown.rs:495-510` | When a label is rejected, the search restarts at `label_start` and rescans to the next `]`, normalising the whole label each time. | `\[^` × 100k followed by `x\]`: about 1e10 character operations. | Stop a label at the first `[` or `]`, cap it at 999 bytes, and track pending text apart from the search position (2.4). | `rejected_footnote_labels_are_scanned_in_linear_time` (`src/reader/markdown.rs` tests) |
| H6 | high | `src/reader/markdown.rs:608-620`, `:535`, `:780` | When `find(']')` fails, the scan advances one character and searches again. Backtick state carries across lines (and blank lines) here but resets per line in the prescan. | Line 1 is a backtick, line 2 is blank, line 3 is an indented backtick followed by `[^` × 1M: about 1e12 bytes scanned. | Skip `[^` checks once no `]` remains, and reset backtick state at blank lines (2.4). | `unterminated_footnote_candidates_are_scanned_in_linear_time` (`src/reader/markdown.rs` tests) |
| H7 | high | `src/reader/pdf/assemble.rs:593-625`, `:628-644`, `:655-666` | `line_join` scans, copies and lowercases the whole accumulated last span on every line. | 50k identical `(ab-) '` lines on one tall page: Σk ≈ 2.5e9 characters, several passes each. | Bound the left-word scan to 64 characters (2.4). | `long_runs_of_hyphenated_lines_join_in_linear_time` (`tests/pdf_reader.rs`) |
| H8 | high | `src/writer/markdown.rs:1058-1063`, `src/writer/html.rs:511-516` | Each image reference base64-encodes the whole asset again. | Output is references × 1.33 × asset size: doubling both quadruples the output. | Markdown: one reference definition per repeated asset. HTML: design open (5, question 2). | `repeated_asset_references_keep_markdown_output_linear`, `repeated_asset_references_share_one_markdown_data_uri_and_read_back`, `repeated_asset_references_keep_html_output_linear` (`tests/writer_roundtrip.rs`) |
| M1 | medium | `src/writer/markdown.rs:1161-1170` | `i += 4; continue` keeps `at_line_start` set, so the space run is counted again on each pass. | 300k leading spaces cost about 1.1e10 comparisons. | Emit the entity once, copy the rest of the run, and clear `at_line_start` (2.4). | `long_leading_space_runs_are_escaped_in_linear_time` (`tests/writer_roundtrip.rs`) |
| M4 | medium | `src/pipeline.rs:308-347` | `read_ir_json` bounds JSON depth (576) but never IR depth, so container prefixes compound across about 288 levels. | 65 nested quotes or 65 nested `Emph` pass, although every reader caps nesting at 64. | Iteratively check block-plus-inline depth after deserialising (2.4). | `rejects_ir_nested_past_the_limit_even_within_the_json_budget` (`src/pipeline.rs` tests) |
| M5 | medium | `src/writer/markdown.rs:453-507` | Every body row is padded to `col_count`. | 1,000 columns × 1,000 empty rows write 1e6 cells, and doubling both quadruples the output. | Write only the cells a row has, at least one (2.4). | `empty_rows_of_wide_tables_keep_markdown_output_linear` (`tests/writer_roundtrip.rs`) |
| M8 | medium | `src/reader/pdf/font.rs:366-378`, `:459-495`, `interpreter.rs:215` | Walking `/Differences` and `/W` is never charged. Integer entries reset the code, and rejected triplets never push. | 3,000 fonts sharing one 400k-element array: about 1.2e9 steps from about 1.4 MB of input. | Charge `array.len() × size_of::<Object>()` per walk (2.4). | `shared_differences_arrays_are_charged_for_every_font_that_walks_them`, `rejected_cid_width_triplets_are_charged_for_every_font_that_walks_them` (`src/reader/pdf/interpreter.rs` tests); fix-agnostic `fonts_sharing_one_large_differences_array_load_in_bounded_time` (`tests/pdf_reader.rs`) |
| M9 | medium | `src/reader/pdf/font.rs:91-105`, `cmap.rs:131-136`, `:145-162`, `:226-240` | `work_bytes` leaves out parsed CMaps. A list `bfrange` keeps one `Vec<u16>` (24 bytes) per code. | 16 fonts with distinct 131 KB CMaps keep about 24 MiB, but are charged about 2.4 MB. | Add the CMap heap size to `work_bytes` (2.4). | `distinct_tounicode_cmaps_charge_their_parsed_size` (`src/reader/pdf/interpreter.rs` tests) |
| M13 | medium | `src/reader/html.rs:830-833`, `:473-495`, `:1680-1691` | `code`, `kbd` and `samp` are not in `skip_children`, so every nested level runs `collect_text` over its whole subtree. | 1,000 nested `<code>` around 600k `<br>`: about 6e8 node visits. | Add them to `skip_children` (2.4). | `nested_code_elements_collect_text_once` (`tests/html_reader.rs`) |

How the tests behave on origin/dev:

- **Wrong-result tests** fail fast: H1, H2 (boundary), H3, H8, M4, M5, M8 (unit) and M9.
- **Time-bound tests** run long and then fail on their bound: H4, H5, H6, H7, M1, M8 (end to end) and M13.
- **H2 stress test** aborts with a stack overflow. Under `just test` (nextest, one process per test) only that test fails. Under plain `cargo test` the abort stops the whole test binary.

Time bounds follow the repo's existing pattern (`Instant` with a generous `Duration`, run in debug by `cargo nextest run --workspace`). Each comment's operation count is an estimate. Record the real debug timings on the first green run and tighten each bound to at least 5× headroom, as the existing tests note.

### 2.3 Tests, by target file

#### `crates/ariad-core/tests/writer_roundtrip.rs`

Add `use std::time::{Duration, Instant};` to the imports. Every other name used below is already imported there.

```rust
#[test]
fn strip_html_tags_keeps_multibyte_text_next_to_tag_names() {
    // Each input puts a multibyte character across the byte index that the `script`,
    // `style` or closing-tag check used to slice at.
    let cases = [
        // `ế` covers bytes 4..7 after `<`, so the 6-byte `script` check split it.
        ("<p>Tiếng Việt</p>", "Tiếng Việt"),
        // `ả` covers bytes 3..6 after `<`, so the 5-byte `style` check split it.
        ("<p>Bảng</p>", "Bảng"),
        // `é` covers bytes 4..6 after `<`, so the `style` check split it.
        ("x <a b=é>y</a>", "x y"),
        // Inside a script body, `ế` covers bytes 4..7 after `</`.
        ("<script>x</abcdế></script>after", "after"),
        // Inside a style body, `ế` covers bytes 3..6 after `</`.
        ("<style>x</abcế></style>after", "after"),
    ];
    for (input, expected) in cases {
        assert_eq!(writer::strip_html_tags(input), expected, "input: {input:?}");
    }
}

#[test]
fn vietnamese_raw_html_from_markdown_reaches_every_text_writer() {
    let input = "<p>Tiếng Việt</p>\n\nXin chào <a b=é>bạn</a>.\n";
    let parsed = markdown::read(input, &Limits::local()).expect("read markdown");
    assert!(parsed.document.body.iter().any(|block| matches!(
        block,
        Block::Raw {
            format: RawFormat::Html,
            ..
        }
    )));

    let md_out = writer::markdown::write(&parsed.document);
    assert!(md_out.content.contains("Tiếng Việt"), "{}", md_out.content);
    assert!(md_out.content.contains("Xin chào bạn."), "{}", md_out.content);

    let html_out = writer::html::write(&parsed.document);
    assert!(
        html_out.content.contains("<p>Xin chào bạn.</p>"),
        "{}",
        html_out.content
    );
}

fn document_with_repeated_image(asset_bytes: Vec<u8>, references: usize) -> Document {
    let mut doc = Document::default();
    doc.assets.insert(
        "logo".to_owned(),
        Asset {
            media_type: "image/png".to_owned(),
            bytes: asset_bytes,
        },
    );
    for _ in 0..references {
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Asset {
                    id: "logo".to_owned(),
                },
                alt: "logo".to_owned(),
                title: None,
            }],
        });
    }
    doc
}

#[test]
fn repeated_asset_references_keep_markdown_output_linear() {
    let small = writer::markdown::write(&document_with_repeated_image(vec![0x5a; 4 * 1024], 200));
    let large = writer::markdown::write(&document_with_repeated_image(vec![0x5a; 8 * 1024], 400));
    // Doubling both the asset and the reference count doubles linear output; re-encoding the
    // asset at every reference quadruples it.
    assert!(
        large.content.len() < 3 * small.content.len(),
        "small: {}, large: {}",
        small.content.len(),
        large.content.len()
    );
}

#[test]
fn repeated_asset_references_share_one_markdown_data_uri_and_read_back() {
    let png = fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/md/assets/vietnamese-route.png"),
    )
    .expect("real PNG fixture");
    let written = writer::markdown::write(&document_with_repeated_image(png.clone(), 3));
    assert_eq!(written.content.matches("data:image/png;base64,").count(), 1);

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(read_back.document.assets.len(), 1);
    assert_eq!(read_back.document.assets.values().next().unwrap().bytes, png);
    let images = read_back
        .document
        .body
        .iter()
        .filter(|block| {
            matches!(block, Block::Paragraph { content }
                if matches!(content.as_slice(), [Inline::Image { target: AssetRef::Asset { .. }, .. }]))
        })
        .count();
    assert_eq!(images, 3);
}

#[test]
fn repeated_asset_references_keep_html_output_linear() {
    // Depends on the HTML design decision; see unresolved question 2.
    let small = writer::html::write(&document_with_repeated_image(vec![0x5a; 4 * 1024], 200));
    let large = writer::html::write(&document_with_repeated_image(vec![0x5a; 8 * 1024], 400));
    assert!(
        large.content.len() < 3 * small.content.len(),
        "small: {}, large: {}",
        small.content.len(),
        large.content.len()
    );
}

#[test]
fn long_leading_space_runs_are_escaped_in_linear_time() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Text {
            text: format!("{}y", " ".repeat(300_000)),
        }],
    });
    let start = Instant::now();
    let written = writer::markdown::write(&doc);
    let elapsed = start.elapsed();
    // Recounting the run after every 4-space step made about 1.1e10 comparisons here.
    assert!(
        elapsed < Duration::from_secs(5),
        "escaping leading spaces took too long: {elapsed:?}"
    );
    assert!(written.content.starts_with("&#32;"));
    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert!(matches!(
        read_back.document.body.as_slice(),
        [Block::Paragraph { .. }]
    ));
}

fn wide_table_with_empty_rows(columns: usize, rows: usize) -> Document {
    let mut doc = Document::default();
    doc.body.push(Block::Table {
        caption: None,
        columns: vec![
            ColumnSpec {
                align: Alignment::Default
            };
            columns
        ],
        head: vec![],
        body: vec![Vec::new(); rows],
        footnotes: vec![],
    });
    doc
}

#[test]
fn empty_rows_of_wide_tables_keep_markdown_output_linear() {
    let small = writer::markdown::write(&wide_table_with_empty_rows(500, 500))
        .content
        .len();
    let large = writer::markdown::write(&wide_table_with_empty_rows(1_000, 1_000))
        .content
        .len();
    // Padding every empty row to the column count grows with columns × rows, so doubling
    // both quadruples the output.
    assert!(large < 3 * small, "small: {small}, large: {large}");
}
```

Expected after the fix:

- **H1:** the stripped text is returned with no panic. The Markdown writer prints `Tiếng Việt` and `Xin chào bạn.`. The HTML writer keeps the sanitised block and writes `<p>Xin chào bạn.</p>`.
- **H8, Markdown:** the output grows about 2× when both sizes double. An asset referenced 2 or more times is base64-encoded once, and reading it back yields one asset and three images.
- **H8, HTML:** the same 2× growth, if the chosen design deduplicates (see 5, question 2).
- **M1:** writing finishes in milliseconds, the output starts with `&#32;`, and it reads back as one paragraph, not an indented code block.
- **M5:** the output grows about 2× when both sizes double. This holds whether the fix skips padding for missing cells or caps the columns.

#### `crates/ariad-core/tests/typst_writer.rs`

```rust
#[test]
fn raw_html_with_vietnamese_text_is_stripped_without_panicking() {
    let output = write(
        &document(vec![
            Block::Raw {
                format: RawFormat::Html,
                text: "<p>Tiếng Việt</p>".to_owned(),
            },
            para(vec![
                Inline::Raw {
                    format: RawFormat::Html,
                    text: "<a b=é>".to_owned(),
                },
                text("bạn"),
                Inline::Raw {
                    format: RawFormat::Html,
                    text: "</a>".to_owned(),
                },
            ]),
        ]),
        &TypstOptions::default(),
    );
    assert!(output.markup.contains("Tiếng Việt"), "{}", output.markup);
    assert!(output.markup.contains("bạn"), "{}", output.markup);
    assert_eq!(codes(&output), [WarningCode::RawDropped]);
}
```

Expected after the fix: the block and inline raw paths (`typst.rs:804` and `:1142`) emit the stripped prose with exactly one `RawDropped` warning (`warn_raw_html`, `typst.rs:623-628`). Today the block path panics.

#### `crates/ariad-core/src/reader/markdown.rs` (`mod tests`)

Markdown reader tests live in this module on origin/dev (there is no `tests/markdown_reader.rs`). `Instant`, `Block`, `Inline`, `Limits`, `ReadError` and `read` are already imported there.

```rust
    /// Deepest chain of emphasis-like inlines in a paragraph.
    fn inline_depth(block: &Block) -> usize {
        let Block::Paragraph { content } = block else {
            return 0;
        };
        let mut deepest = 0;
        let mut stack: Vec<(&Inline, usize)> = content.iter().map(|inline| (inline, 1)).collect();
        while let Some((inline, depth)) = stack.pop() {
            if let Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content } = inline
            {
                deepest = deepest.max(depth);
                stack.extend(content.iter().map(|inline| (inline, depth + 1)));
            }
        }
        deepest
    }

    /// `depth` openers that follow `(` and closers that precede `)`. Each inner opener is only
    /// left-flanking and each closer only right-flanking, so comrak nests them all, while the
    /// prescan counts only the opener at column 0.
    fn punctuated_nesting(marker: &str, depth: usize) -> String {
        let open = format!("{marker}a(");
        let close = format!("a{marker})");
        format!("{}x{}\n", open.repeat(depth), close.repeat(depth))
    }

    #[test]
    fn inline_nesting_counts_toward_the_limit_when_the_prescan_misses_it() {
        let limit = Limits::local().max_nesting_depth;
        let depth = usize::from(limit);
        for marker in ["*", "**"] {
            let at_limit = read(&punctuated_nesting(marker, depth), &Limits::local())
                .expect("nesting at the limit is accepted");
            assert_eq!(inline_depth(&at_limit.document.body[0]), depth, "marker: {marker}");
            assert_eq!(
                read(&punctuated_nesting(marker, depth + 1), &Limits::local()),
                Err(ReadError::NestingTooDeep { limit }),
                "marker: {marker}"
            );
        }
        // If comrak also nests `~~`, it must be bounded the same way.
        match read(&punctuated_nesting("~~", depth + 1), &Limits::local()) {
            Ok(output) => assert!(inline_depth(&output.document.body[0]) <= depth),
            Err(error) => assert_eq!(error, ReadError::NestingTooDeep { limit }),
        }
    }

    #[test]
    fn deep_inline_nesting_is_rejected_on_a_wasm_sized_stack() {
        // The browser module runs with a 1 MiB stack; dropping, printing or serialising
        // 50,000 nested inlines recurses far past it.
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                let input = punctuated_nesting("*", 50_000);
                let start = Instant::now();
                let rejected = matches!(
                    read(&input, &Limits::local()),
                    Err(ReadError::NestingTooDeep { limit: 64 })
                );
                assert!(rejected, "50,000 nested inlines must be rejected");
                assert!(start.elapsed().as_secs_f64() < 10.0);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn rejected_footnote_labels_are_scanned_in_linear_time() {
        // `\[^` is an escaped bracket, so no candidate is a known reference, and comrak
        // coalesces the run into one Text node that `text_inlines` rescanned from every `[^`.
        let input = format!("{}x\\]\n", "\\[^".repeat(100_000));
        let start = Instant::now();
        assert!(read(&input, &Limits::local()).is_ok());
        assert!(start.elapsed().as_secs_f64() < 10.0);
    }

    #[test]
    fn unterminated_footnote_candidates_are_scanned_in_linear_time() {
        // Line 1 opens a code span. `scan_footnotes` carries it across the blank line, so the
        // backtick on line 3 closes it and every following `[^` rescanned the rest of the line.
        // The prescan resets per line and skips line 3 as code, and comrak reads line 3 as an
        // indented code block, so nothing else does quadratic work.
        let input = format!("`\n\n    `{}\n", "[^".repeat(1_000_000));
        let start = Instant::now();
        let result = read(&input, &Limits::local());
        assert!(
            matches!(result, Ok(_) | Err(ReadError::NestingTooDeep { .. })),
            "{:?}",
            result.as_ref().err()
        );
        assert!(start.elapsed().as_secs_f64() < 10.0);
    }
```

Expected after the fix:

- **H2 boundary:** 64 levels are accepted with an IR inline depth of exactly 64 (this also checks that the input really nests), and 65 levels fail with `NestingTooDeep { limit: 64 }`. These are the same semantics as `tests/html_reader.rs::inline_nesting_depth_65_fails`.
- **H2 stress:** rejected in the iterative walk, on a 1 MiB stack, with nothing left to drop deeply.
- **H5:** linear in input size. Optionally, lock in the M3 half of the shared fix with `assert_eq!(paragraph_text(&body[0]).chars().count(), 2 * 100_000 + 2)` (all the `[^`, plus `x]`).
- **H6:** linear in input size. The result is `Ok` if the scanners reset at blank lines, or `NestingTooDeep` if the fix lets the prescan see the brackets. Both are accepted.

#### `crates/ariad-core/tests/html_reader.rs`

```rust
#[test]
fn attribute_heavy_formatting_reconstruction_is_charged_to_the_node_budget() {
    use std::fmt::Write;
    // One `<b>` with 1,000 attributes, then 100 paragraphs. Each `<p>t` reconstructs the `<b>`
    // and clones all 1,000 attributes, while the arena grows by only three nodes.
    let mut html = String::from("<p><b");
    for i in 0..1000 {
        let _ = write!(html, " a{i}=1");
    }
    html.push_str(">x");
    for _ in 0..100 {
        html.push_str("<p>t");
    }
    let start = Instant::now();
    let result = read(html.as_bytes(), &Limits::local());
    let elapsed = start.elapsed();
    assert!(
        matches!(result, Err(ReadError::TooManyNodes { .. })),
        "expected TooManyNodes, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "attribute reconstruction took too long: {elapsed:?}"
    );

    // Ordinary attribute use stays far inside the budget.
    let mut page = String::new();
    for i in 0..1000 {
        let _ = write!(
            page,
            "<p><a href=\"https://example.test/{i}\" class=\"x\" title=\"t\">link</a> <b class=\"k\">bold</b></p>"
        );
    }
    let output = read(page.as_bytes(), &Limits::local()).expect("attribute-bearing page parses");
    assert_eq!(output.document.body.len(), 1000);
}

#[test]
fn many_unpaired_math_helper_siblings_parse_in_linear_time() {
    let mut html = String::with_capacity(30_000 * 32 + 16);
    html.push_str("<div>");
    for _ in 0..30_000 {
        html.push_str("<span class=\"katex-html\"></span>");
    }
    html.push_str("</div>");
    let start = Instant::now();
    let result = read(html.as_bytes(), &Limits::local());
    let elapsed = start.elapsed();
    assert!(result.is_ok(), "{:?}", result.as_ref().err());
    // Pairing re-walked three ancestor subtrees plus every sibling for each helper: about 5e9
    // node visits here.
    assert!(
        elapsed < Duration::from_secs(10),
        "math helper pairing took too long: {elapsed:?}"
    );
}

#[test]
fn nested_code_elements_collect_text_once() {
    // Every nested `code` level re-walked its whole subtree: 1,000 levels over 600,000 `<br>`
    // nodes is about 6e8 node visits. The DOM depth is 1,003, inside the 1,024 cap.
    let html = format!(
        "{}{}{}",
        "<code>".repeat(1000),
        "<br>".repeat(600_000),
        "</code>".repeat(1000)
    );
    let start = Instant::now();
    let result = read(html.as_bytes(), &Limits::local());
    let elapsed = start.elapsed();
    assert!(result.is_ok(), "{:?}", result.as_ref().err());
    assert!(
        elapsed < Duration::from_secs(10),
        "nested code took too long: {elapsed:?}"
    );
}
```

Expected after the fix:

- **H3, hostile input:** `TooManyNodes`. Budget arithmetic: the budget is about 8192 + 7.3k ≈ 15.5k, and each reconstruction now charges about 1,003. Today about 300 nodes are counted and the read succeeds.
- **H3, attribute-bearing page:** still parses, so the fix is not over-strict.
- **H4:** finishes in about the time html5ever takes to parse 1 MB. The existing `katex_mathjax_no_duplicate_stray_text` test still guards the paired case.
- **M13:** finishes in about the time html5ever takes to parse 600k `<br>`. The existing 400k-`<br>` test reports about 1.07 s in debug.

#### `crates/ariad-core/tests/pdf_reader.rs`

Both tests reuse the file's `synthetic_pdf` helper. The file is already gated on `feature = "pdf"`.

```rust
#[test]
fn long_runs_of_hyphenated_lines_join_in_linear_time() {
    // 50,000 identical `ab-` lines at one x, size and pitch stay in one block. Each join merged
    // the hyphen and rescanned, copied and lowercased the whole accumulated word.
    const LINES: usize = 50_000;
    let top = LINES * 6 / 5 + 10;
    let mut content = format!("BT /F1 1 Tf 1.2 TL 10 {top} Td").into_bytes();
    for _ in 0..LINES {
        content.extend_from_slice(b" (ab-) '");
    }
    content.extend_from_slice(b" ET");
    let mut doc = lopdf::Document::load_mem(&synthetic_pdf(&content, |_, _| {})).unwrap();
    let page = doc.page_iter().next().unwrap();
    let height = i64::try_from(top + 20).unwrap();
    let media_box: Vec<lopdf::Object> = vec![0.into(), 0.into(), 612.into(), height.into()];
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("MediaBox", media_box);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();

    let start = Instant::now();
    let output = pdf::read(&bytes, &Limits::local()).expect("tall hyphenated page reads");
    let elapsed = start.elapsed();
    assert_well_formed(&output);
    assert!(
        elapsed < Duration::from_secs(15),
        "hyphen joining took too long: {elapsed:?}"
    );
}

#[test]
fn fonts_sharing_one_large_differences_array_load_in_bounded_time() {
    use lopdf::{Dictionary, Object, dictionary};
    // 3,000 fonts share one 400,000-element `/Differences` array. Integer entries reset the
    // code, so every font resolved every element: about 1.2e9 steps from about 1.4 MB of input.
    const FONTS: usize = 3_000;
    let mut content = b"BT".to_vec();
    for index in 0..FONTS {
        content.extend_from_slice(format!(" /G{index} 12 Tf").as_bytes());
    }
    content.extend_from_slice(b" 20 700 Td (x) Tj ET");
    let bytes = synthetic_pdf(&content, |doc, resources| {
        let differences = (0..200_000)
            .flat_map(|_| [Object::Integer(0), Object::Name(b"a".to_vec())])
            .collect::<Vec<_>>();
        let differences = doc.add_object(Object::Array(differences));
        let mut fonts = Dictionary::new();
        for index in 0..FONTS {
            let font = doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => dictionary! { "Differences" => differences },
            });
            fonts.set(format!("G{index}"), font);
        }
        resources.set("Font", fonts);
    });

    let start = Instant::now();
    let result = pdf::read(&bytes, &Limits::local());
    let elapsed = start.elapsed();
    assert!(
        matches!(result, Ok(_) | Err(ReadError::DecompressedTooLarge { .. })),
        "{:?}",
        result.as_ref().err()
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "shared /Differences took too long: {elapsed:?}"
    );
}
```

Expected after the fix:

- **H7:** a well-formed read, fast. Once a merged word passes 64 characters the join falls back to `Hyphen`, so the text keeps hyphens from that point. The test does not assert the text.
- **M8:** fast under any of the three fixes the reports list. Charging fails with `DecompressedTooLarge` after a few dozen fonts; caching by object id or bounding the elements returns `Ok`. Budget arithmetic: the work limit is about 32 MiB + 64 × 1.4 MB ≈ 123 MB. The 3,000 fonts × about 21 KB are charged about 63 MB today, so origin/dev returns `Ok`, slowly.

#### `crates/ariad-core/src/reader/pdf/interpreter.rs` (`mod tests`)

These follow the existing `repeated_font_dictionaries_charge_retained_width_allocations` test: a `DocState` with a small work budget and direct `font_index` calls. The imports (`Dictionary`, `Document`, `Object`, `Stream`, `dictionary`, `DocState`, `Budget`) are already there.

```rust
    #[test]
    fn shared_differences_arrays_are_charged_for_every_font_that_walks_them() {
        let mut doc = Document::new();
        let differences = (0..100_000)
            .flat_map(|_| [Object::Integer(0), Object::Name(b"a".to_vec())])
            .collect::<Vec<_>>();
        let differences = doc.add_object(Object::Array(differences));
        let ids = (0..16)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                    "Encoding" => dictionary! { "Differences" => differences },
                })
            })
            .collect::<Vec<_>>();
        let limit = 1 << 20;
        let mut state = DocState::new(&doc, Budget::new(1 << 20, limit));
        let error = ids
            .iter()
            .find_map(|id| state.font_index(doc.get_object(*id).unwrap()).err());
        assert_eq!(error, Some(super::ReadError::DecompressedTooLarge { limit }));
    }

    #[test]
    fn rejected_cid_width_triplets_are_charged_for_every_font_that_walks_them() {
        let mut doc = Document::new();
        // `last < first` in every triplet, so nothing is kept and no width memory is charged.
        let widths = (0..100_000)
            .flat_map(|_| [Object::Integer(10), Object::Integer(5), Object::Integer(500)])
            .collect::<Vec<_>>();
        let widths = doc.add_object(Object::Array(widths));
        let descendant = doc.add_object(dictionary! { "Subtype" => "CIDFontType2", "W" => widths });
        let ids = (0..16)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Subtype" => "Type0",
                    "Encoding" => "Identity-H",
                    "DescendantFonts" => vec![Object::Reference(descendant)],
                })
            })
            .collect::<Vec<_>>();
        let limit = 1 << 20;
        let mut state = DocState::new(&doc, Budget::new(1 << 20, limit));
        let error = ids
            .iter()
            .find_map(|id| state.font_index(doc.get_object(*id).unwrap()).err());
        assert_eq!(error, Some(super::ReadError::DecompressedTooLarge { limit }));
    }

    #[test]
    fn distinct_tounicode_cmaps_charge_their_parsed_size() {
        let mut program = b"1 begincodespacerange <0000> <FFFF> endcodespacerange \
            1 beginbfrange <0000> <FFFF> ["
            .to_vec();
        program.extend(b"<>".repeat(65_536));
        program.extend_from_slice(b"] endbfrange");
        let mut doc = Document::new();
        let ids = (0..16)
            .map(|_| {
                let cmap = doc.add_object(Stream::new(Dictionary::new(), program.clone()));
                doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                    "ToUnicode" => cmap,
                })
            })
            .collect::<Vec<_>>();
        // Each CMap decodes about 131 KB but keeps 65,536 empty targets, about 1.5 MiB.
        let limit = 8 << 20;
        let mut state = DocState::new(&doc, Budget::new(1 << 30, limit));
        let error = ids
            .iter()
            .find_map(|id| state.font_index(doc.get_object(*id).unwrap()).err());
        assert_eq!(error, Some(super::ReadError::DecompressedTooLarge { limit }));
    }
```

Expected after the fix: `DecompressedTooLarge { limit }` from `font_index`. The arithmetic on origin/dev versus after the charging fix:

| Test | Charged today | After the fix |
|---|---|---|
| `/Differences` | about 21 KB per font, 16 × 21 KB ≈ 336 KB, under 1 MiB | 200k elements × at least 1 byte, past 1 MiB at the first font |
| `/W` | about 16 KB per font, 16 × 16 KB ≈ 256 KB, under 1 MiB | 300k elements, past 1 MiB |
| ToUnicode | about 151 KB per font, 16 × 151 KB ≈ 2.4 MB, under 8 MiB | about 1.67 MB per font, past 8 MiB by the fifth font |

The M9 test uses distinct streams, so caching by `ObjectId` alone, which does not bound that case, still fails it. The two M8 unit tests assume the charging fix; if the team picks caching or element caps instead, rely on the fix-agnostic test in `tests/pdf_reader.rs`.

#### `crates/ariad-core/src/pipeline.rs` (`mod tests`)

```rust
    #[test]
    fn rejects_ir_nested_past_the_limit_even_within_the_json_budget() {
        // Deep documents need more stack than the default test thread provides.
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let limits = Limits::local();
                let limit = usize::from(limits.max_nesting_depth);
                let quotes = |depth: usize| {
                    let mut block = Block::Paragraph { content: vec![] };
                    for _ in 0..depth {
                        block = Block::Quote {
                            blocks: vec![block],
                        };
                    }
                    serde_json::to_vec(&Document {
                        body: vec![block],
                        ..Document::default()
                    })
                    .unwrap()
                };
                assert!(read_ir_json(&quotes(limit), &limits).is_ok());
                assert_eq!(
                    read_ir_json(&quotes(limit + 1), &limits).unwrap_err(),
                    limit_exceeded("max_nesting_depth")
                );

                let mut inline = Inline::Text {
                    text: "x".to_owned(),
                };
                for _ in 0..=limit {
                    inline = Inline::Emph {
                        content: vec![inline],
                    };
                }
                let emphasis = serde_json::to_vec(&Document {
                    body: vec![Block::Paragraph {
                        content: vec![inline],
                    }],
                    ..Document::default()
                })
                .unwrap();
                assert_eq!(
                    read_ir_json(&emphasis, &limits).unwrap_err(),
                    limit_exceeded("max_nesting_depth")
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
```

Expected after the fix: 64 levels pass and 65 levels (block or inline) fail with `limit_exceeded("max_nesting_depth")`. On origin/dev, 65 quotes have a JSON depth of about 134, well inside the 576 budget, so both 65-level documents are accepted.

### 2.4 Minimal fix sketches

**H1, `writer/mod.rs`**

```rust
/// ASCII case-insensitive prefix test that never slices inside a UTF-8 character.
fn has_prefix_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    s.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}
// :103-104  after_close.len() >= tag_name.len() && after_close[..tag_name.len()].eq_ignore_ascii_case(tag_name)
//           -> has_prefix_ignore_ascii_case(after_close, tag_name)
// :146-147  -> has_prefix_ignore_ascii_case(after_open, "script")
// :159-160  -> has_prefix_ignore_ascii_case(after_open, "style")
```

**H2, `reader/markdown.rs:107-115`.** Extend `is_container` with the inline containers, then update ARCHITECTURE.md:344 to say "block plus inline", matching :346.

```rust
let is_container = matches!(
    value,
    NodeValue::BlockQuote
        | NodeValue::MultilineBlockQuote(_)
        | NodeValue::Alert(_)
        | NodeValue::List(_)
        | NodeValue::Table(_)
        | NodeValue::FootnoteDefinition(_)
        // Inline containers count too, as in the HTML reader.
        | NodeValue::Emph
        | NodeValue::Strong
        | NodeValue::Strikethrough
        | NodeValue::Link(_)
        | NodeValue::Image(_)
);
```

**H3, `reader/html_sink.rs`.** Replace the arena-length check with a running charge, and charge attributes before any filtering. Optionally drop the attributes the mapper never reads afterwards. Update the node-bound and "230 bytes of RSS per input byte" text at ARCHITECTURE.md:346.

```rust
charged: Cell<usize>, // new field, starts at 0

fn charge(&self, units: usize) {
    let total = self.charged.get().saturating_add(units);
    self.charged.set(total);
    if total > self.max_nodes && self.limit_hit.get().is_none() {
        self.limit_hit.set(Some(LimitExceeded::TooManyNodes { limit: self.max_nodes }));
    }
}
// create_element: self.charge(1 + attrs.len()); every other create_* / append of a new node: self.charge(1).
```

**H4, `reader/html.rs`.** Build one `has_tex: Vec<bool>` indexed like `converted` before the walk. Run an iterative pre-order collection, then a reverse pass: a node is true if it is a TeX `annotation` or any child is true. Then:

```rust
fn is_paired_with_math_tex(arena: &Arena, has_tex: &[bool], handle: Handle) -> bool {
    std::iter::successors(arena.nodes[handle].parent, |&parent| arena.nodes[parent].parent)
        .take(3)
        .any(|ancestor| has_tex[ancestor])
}
```

The old sibling check is subsumed: a parent's subtree flag already covers every sibling.

**H5, `reader/markdown.rs:492-529`.** Keep `pending` (start of unemitted text) separate from `search`. On rejection, advance only `search`. This also fixes M3.

```rust
const MAX_LABEL_BYTES: usize = 999;
let (mut pending, mut search) = (0, 0);
while let Some(relative) = text[search..].find("[^") {
    let start = search + relative;
    let label_start = start + 2;
    let Some(relative_end) = text[label_start..].find(|c| c == '[' || c == ']') else { break };
    let end = label_start + relative_end;
    let label = &text[label_start..end];
    let accepted = text.as_bytes()[end] == b']'
        && !label.is_empty()
        && label.len() <= MAX_LABEL_BYTES
        && {
            let normalized = normalize_footnote(label);
            state.footnote_references.contains(&normalized)
                && !state.footnote_definitions.contains(&normalized)
        };
    if !accepted {
        search = end; // the label holds no `[`, so no candidate starts inside it
        continue;
    }
    if start > pending {
        inlines.push(Inline::Text { text: nfc(&text[pending..start]) });
    }
    inlines.push(Inline::FootnoteRef { id: label.to_owned() });
    pending = end + 1;
    search = pending;
}
// then push text[pending..] as today
```

**H6, `reader/markdown.rs`.** In `scan_footnote_references`, compute `let last_close = line.rfind(']');` once. Only try a `[^` candidate when `last_close.is_some_and(|close| close > cursor + 1)`. The `find(']')` then always succeeds and jumps the cursor past it, so the scan stays linear. In `scan_footnotes`, reset `inline_code_ticks = 0` on blank lines (CommonMark ends the paragraph, and any open code span, there).

**H7, `reader/pdf/assemble.rs:593-625`**

```rust
/// Longest left-hand word `line_join` inspects; a longer run keeps its hyphen.
const MAX_JOIN_WORD: usize = 64;

let left_len = stem
    .chars()
    .rev()
    .take(MAX_JOIN_WORD + 1)
    .take_while(|ch| !ch.is_whitespace())
    .map(char::len_utf8)
    .sum::<usize>();
let left = &stem[stem.len() - left_len..];
if left.chars().count() > MAX_JOIN_WORD {
    return Join::Hyphen;
}
// the rest is unchanged and now works on at most 64 characters plus the next line's first word
```

**H8, `writer/markdown.rs`.** Count references per asset id once at writer entry. If an asset is referenced once, keep the inline data URI, so the existing `asset_backed_images_are_embedded_as_data_uris` test stays valid. If it is referenced 2 or more times, write `![alt][ariad-asset-<n>]` and append one `[ariad-asset-<n>]: data:…` definition at the end. HTML: see 5, question 2.

**M1, `writer/markdown.rs:1166-1170`**

```rust
if spaces >= 4 {
    out.push_str("&#32;");
    for _ in 1..spaces {
        out.push(' ');
    }
    i += spaces;
    at_line_start = false;
    continue;
}
```

**M4, `pipeline.rs:342`.** After deserialising, walk `body` and `furniture` iteratively with `(node, depth)` pairs. Count Quote, List, Table, Footnote and the inline containers (Emph, Strong, Strikeout, Superscript, Subscript, Link), and return `limit_exceeded("max_nesting_depth")` past `limits.max_nesting_depth`. Check `max_asset_bytes` in the same pass. See 5, question 4, on deserialising before the check.

**M5, `writer/markdown.rs:496-507`.** Write `row.len().clamp(1, col_count)` cells per body row instead of `col_count`, since GFM fills missing cells. Header and delimiter rows stay `col_count` wide, which is linear in the input.

**M8, `reader/pdf/font.rs`.** Before the `/Differences` loop, call `budget.charge_work(differences.len() * std::mem::size_of::<Object>())?`. Do the same with `array.len()` where `load_composite` calls `cid_widths`, passing `budget` through or charging at the call site.

**M9, `reader/pdf/cmap.rs` and `font.rs:91-105`.** Add `CMap::heap_bytes()`, the sum of `ranges.capacity() * size_of::<Range>()`, the `List` outer capacities × `size_of::<Vec<u16>>()`, the inner `u16` capacities × 2, and the `singles` entries. Add it for `to_unicode` and for `Codes::Encoding(cmap)` in `Font::work_bytes`. Caching by `ObjectId` behind `Rc` is an optional extra.

**M13, `reader/html.rs:477-491`.** Add `"code" | "kbd" | "samp"` to the `skip_children` list. Their conversion (`:830-833`) already collects the subtree text itself.

## 3. Contract and architecture violations

- **ARCHITECTURE.md:27, Principle 6 "Secure by default: resource limits":** broken by H3–H8, M1, M4, M5, M8, M9 and M13, as the verify report found.
- **ARCHITECTURE.md:344:** says the comrak reader "iteratively enforces … nesting", but inline nesting is not counted (H2), while :346 defines `max_nesting_depth` as block plus inline for HTML. Per AGENTS.md, the H2 fix should update :344 in the same change.
- **ARCHITECTURE.md:346:** says the node bound is the HTML reader's memory bound, and gives about 230 bytes of RSS per input byte. Both fail for attribute-heavy reconstruction (H3). The H3 fix changes what the bound counts, so that text must change in the same change.
- **The proposed tests themselves:** they add no dependency, no `unsafe` (the workspace denies `unsafe_code`), no new file and no fixture. The H8 round trip reuses the existing `fixtures/md/assets/vietnamese-route.png`.

## 4. Test gaps that matter

- **No writer fuzz target.** No fuzz target drives the Markdown or HTML writer from arbitrary IR. A `markdown_writer` / `html_writer` target fed by `pipeline::read_ir_json` would find H1-class panics, and with `-timeout` and `-rss_limit_mb` it would also find M1, M5 and H8.
- **Release-only timing tests never run in CI.** `tests/pdf_reader.rs::large_fixtures_read_quickly` is ignored in debug builds. CI runs `cargo nextest run --workspace` in debug, and the only release run (`bench.yml`) is for `ariad-cli`. That is why every time bound above is sized for debug, not marked release-only.
- **The charging tests check accounting, not memory.** H3, M8 and M9 have no peak-RSS guard. If a fix filters attributes or caches CMaps instead of charging, the deterministic test must be swapped for an RSS check (Linux `/proc/self/status` `VmHWM`, valid per test under nextest's process-per-test model).
- **Pandoc-sourced raw HTML is untested.** No test covers multibyte raw HTML arriving through Pandoc (`pandoc/to_ir.rs:152`, `:496`), which reaches the same `strip_html_tags` callers.
- **The time bounds are unmeasured.** Every bound above is an estimate from operation counts. Measure on the first green run and keep at least 5× headroom.

## 5. Unresolved questions

1. **Does comrak 0.56 itself recurse once per inline nesting level** (text-node post-processing or sourcepos fix-ups)? If it does, `deep_inline_nesting_is_rejected_on_a_wasm_sized_stack` still overflows after the walk fix, and the prescan must also catch the punctuation-flanked pattern. I could not read the registry sources in this session.
2. **H8, HTML:** which design? Plain HTML has no way to reuse one `data:` URI across several `<img>` elements that the HTML reader would still read back as images. The options are a CSS or SVG indirection (which breaks the round trip), a writer output budget (which changes `writer::html::write` to take `Limits` and return an error), or keeping duplication with a warning. `repeated_asset_references_keep_html_output_linear` only fits the first.
3. **H3, M8 and M9:** does the team accept "charge first" as the primary fix? Attribute filtering, encoding caches and CMap caches can be added on top, but each deterministic test above relies on the charge.
4. **IR JSON deserialisation stack:** `pipeline.rs::reads_deep_ir_within_the_local_budget` needs a 16 MiB thread for 60 nested quotes in debug. IR JSON up to the 576-level JSON budget (about 288 quotes) is therefore deserialised before any IR-depth check, and might overflow the 1 MiB wasm32 stack or 2 MiB tokio worker stacks in release. Unverified. If it does, M4 also needs a tighter `json_depth_budget_for_nesting`, not just a post-deserialisation walk.
5. **Comrak variant names:** the exact names for superscript and subscript in comrak 0.56 were not checked. Both extensions are off today, so the H2 sketch omits them.
6. **Slow failing tests:** should nextest get a `slow-timeout` termination for these tests? Before their fixes, H4, H5, H6, H7 and M13 can run for minutes in debug before missing their bound, inside the 30-minute CI job limit.
