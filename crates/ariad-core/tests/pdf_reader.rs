#![cfg(feature = "pdf")]

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use ariad_core::{
    format::Format,
    ir::{Block, Document, Inline},
    limits::{Limits, LimitsError},
    reader::{
        ReadOutput,
        pdf::{self, PdfStats, ReadError},
    },
    warning::WarningCode,
};
use unicode_normalization::UnicodeNormalization;

fn fixture_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(fixture_path(path)).unwrap()
}

fn read(path: &str) -> ReadOutput {
    pdf::read(&fixture(path), &Limits::local()).unwrap()
}

fn source_paragraphs(name: &str) -> Vec<String> {
    let path = fixture_path("gen/src/ariad_fixture_gen/text").join(name);
    std::fs::read_to_string(path)
        .unwrap()
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(|paragraph| paragraph.nfc().collect())
        .collect()
}

fn inline_text(content: &[Inline], out: &mut String) {
    for inline in content {
        match inline {
            Inline::Text { text } | Inline::Code { text } => out.push_str(text),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => inline_text(content, out),
            Inline::SoftBreak {} | Inline::LineBreak {} => out.push(' '),
            _ => {}
        }
    }
}

/// The text of every top-level block; a list contributes one entry per item.
fn block_texts(blocks: &[Block]) -> Vec<String> {
    let mut texts = Vec::new();
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                let mut text = String::new();
                inline_text(content, &mut text);
                texts.push(text);
            }
            Block::List { items, .. } => {
                for item in items {
                    texts.extend(block_texts(&item.blocks));
                }
            }
            _ => {}
        }
    }
    texts
}

fn headings(document: &Document) -> Vec<(u8, String)> {
    document
        .body
        .iter()
        .filter_map(|block| match block {
            Block::Heading { level, content } => {
                let mut text = String::new();
                inline_text(content, &mut text);
                Some((*level, text))
            }
            _ => None,
        })
        .collect()
}

fn stats(output: &ReadOutput) -> PdfStats {
    PdfStats::from_document(&output.document).expect("PDF statistics in provenance")
}

/// Checks the invariants every successful read must satisfy.
fn assert_well_formed(output: &ReadOutput) {
    let document = &output.document;
    let stats = stats(output);
    assert_eq!(document.meta.source_format, Some(Format::Pdf));
    let layout = document.layout.as_ref().expect("layout index");
    assert_eq!(layout.pages.len(), stats.pages as usize);
    for (index, page) in layout.pages.iter().enumerate() {
        assert_eq!(page.page_no as usize, index + 1);
        assert!(page.width > 0.0 && page.height > 0.0);
    }
    for (prefix, count) in [
        ("body", document.body.len()),
        ("furniture", document.furniture.len()),
    ] {
        for index in 0..count {
            let position = layout
                .blocks
                .get(&format!("{prefix}/{index}"))
                .unwrap_or_else(|| panic!("missing position for {prefix}/{index}"));
            let page = &layout.pages[position.page_no as usize - 1];
            let bbox = position.bbox;
            assert!(
                bbox.left <= bbox.right && bbox.top <= bbox.bottom,
                "{prefix}/{index}"
            );
            assert!(
                bbox.right >= 0.0 && bbox.left <= page.width + 1.0,
                "{prefix}/{index}"
            );
            assert!(
                bbox.bottom >= 0.0 && bbox.top <= page.height + 1.0,
                "{prefix}/{index}"
            );
        }
    }
    assert_eq!(
        layout.blocks.len(),
        document.body.len() + document.furniture.len()
    );
    for text in block_texts(&document.body)
        .iter()
        .chain(&block_texts(&document.furniture))
    {
        assert_eq!(text, &text.nfc().collect::<String>(), "text must be NFC");
        assert!(!text.contains('\u{FFFD}'));
    }
    let json = serde_json::to_string(document).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, document);
}

#[test]
fn vietnamese_typst_pdf_keeps_title_paragraphs_and_table_cells() {
    let output = read("pdf/vi-garden-route.pdf");
    assert_well_formed(&output);
    let document = &output.document;
    assert_eq!(
        headings(document),
        vec![(1, "Khu vườn đọc sách".to_owned())]
    );
    assert!(matches!(document.body[0], Block::Heading { .. }));

    let texts = block_texts(&document.body);
    for paragraph in source_paragraphs("vietnamese-garden.md") {
        assert_eq!(
            texts.iter().filter(|text| **text == paragraph).count(),
            1,
            "paragraph missing or split: {paragraph}"
        );
    }
    let cells = [
        "Điểm dừng",
        "Phút đi bộ",
        "Bến sông",
        "8",
        "Vườn đọc",
        "14",
        "Ghi chú",
        "Ước lượng trong ngày nắng",
    ];
    let positions: Vec<usize> = cells
        .iter()
        .map(|cell| {
            texts
                .iter()
                .position(|text| text == cell)
                .unwrap_or_else(|| panic!("cell missing: {cell}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "cells out of order"
    );

    assert_eq!(document.meta.title.as_deref(), Some("Khu vườn đọc sách"));
    assert_eq!(
        document.meta.authors,
        vec!["AriadShift contributors".to_owned()]
    );
    assert_eq!(document.meta.language.as_deref(), Some("vi"));
    let page = document.layout.as_ref().unwrap().pages[0];
    assert!((page.width - 595.276).abs() < 0.01 && (page.height - 841.89).abs() < 0.01);
    assert_eq!(
        stats(&output),
        PdfStats {
            pages: 1,
            text_pages: 1,
            image_only_pages: 0,
            encrypted: false,
            producer: None,
        }
    );
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}

#[test]
fn english_typst_pdf_keeps_paragraphs_footnote_and_table() {
    let output = read("pdf/en-map-room.pdf");
    assert_well_formed(&output);
    let document = &output.document;
    assert_eq!(
        headings(document),
        vec![(1, "Notes from the Map Room".to_owned())]
    );
    let texts = block_texts(&document.body);
    let mut expected = source_paragraphs("english-map-room.md");
    expected.push(source_paragraphs("english-repair-cafe.md").remove(0));
    for paragraph in expected {
        assert_eq!(
            texts.iter().filter(|text| **text == paragraph).count(),
            1,
            "paragraph missing or split: {paragraph}"
        );
    }
    assert!(texts.iter().any(|text| text.starts_with("Archive note")));
    assert!(texts.iter().any(|text| {
        text.ends_with("The archive keeps older map editions beside later revisions.")
    }));
    for cell in [
        "Archive item",
        "Recorded detail",
        "Street plan",
        "Survey date",
        "Overlay",
        "Transparent sheet",
        "Map weight",
        "Brass",
    ] {
        assert!(
            texts.iter().any(|text| text == cell),
            "cell missing: {cell}"
        );
    }
}

#[test]
fn vietnamese_decision_keeps_words_whole_and_moves_page_numbers_to_furniture() {
    let output = read("pdf/pd-vi-qd-1922-2026.pdf");
    assert_well_formed(&output);
    let document = &output.document;
    let texts = block_texts(&document.body);
    for phrase in ["THỦ TƯỚNG CHÍNH PHỦ", "CỘNG HÒA XÃ HỘI CHỦ NGHĨA VIỆT NAM"] {
        assert!(texts.iter().any(|text| text == phrase), "missing: {phrase}");
    }
    let words: Vec<&str> = texts
        .iter()
        .flat_map(|text| text.split_whitespace())
        .collect();
    for word in [
        "THỦ", "TƯỚNG", "CỘNG", "HÒA", "NGHĨA", "Quyết", "định", "Điều",
    ] {
        assert!(words.contains(&word), "missing word: {word}");
    }
    for fragment in ["TH", "Ủ", "ƯỚNG"] {
        assert!(
            !words.contains(&fragment),
            "split word fragment: {fragment}"
        );
    }
    assert!(
        texts.iter().any(
            |text| text.starts_with("Căn cứ Luật Tổ chức Chính phủ ngày 18 tháng 02 năm 2025;")
        )
    );
    assert!(document.body.iter().any(|block| matches!(
        block,
        Block::List { ordered: false, items, .. } if items.len() >= 5
    )));
    assert_eq!(block_texts(&document.furniture), vec!["2".to_owned()]);
    assert_eq!(
        output.document.layout.as_ref().unwrap().blocks["furniture/0"].page_no,
        2
    );
    let stats = stats(&output);
    assert_eq!((stats.pages, stats.text_pages), (2, 2));
    assert!(!stats.needs_ocr());
}

#[test]
fn image_only_decree_needs_ocr() {
    let output = read("pdf/pd-vi-decree-39-2022.pdf");
    assert_well_formed(&output);
    let stats = stats(&output);
    assert_eq!(stats.pages, 31);
    assert_eq!(stats.text_pages, 0);
    assert!(stats.image_only_pages > 0);
    assert!(stats.needs_ocr());
    assert!(output.document.body.is_empty());
    let warnings: Vec<_> = output
        .warnings
        .iter()
        .filter(|warning| warning.code == WarningCode::OcrRequired)
        .collect();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.starts_with("31 of 31 pages"));
}

#[test]
fn two_column_paper_reads_columns_in_order_and_detects_headings() {
    let output = read("pdf/arxiv-tableformer-2203-01017-v1.pdf");
    assert_well_formed(&output);
    let document = &output.document;
    let texts = block_texts(&document.body);
    let position = |needle: &str| {
        texts
            .iter()
            .position(|text| text.contains(needle))
            .unwrap_or_else(|| panic!("missing: {needle}"))
    };
    // A sentence of the abstract that wraps over several lines reads continuously.
    position(
        "This content is extremely valuable for systems such as search engines, Knowledge \
         Graph’s, etc, since they enhance their predictive capabilities.",
    );
    // The left column (abstract, introduction) comes before the right column of page 1.
    assert!(position("Tables organize valuable content") < position("1. Introduction"));
    assert!(position("1. Introduction") < position("Recently, significant progress"));
    // A paragraph broken by the page break is joined again.
    position("For all practical purposes, it can be considered as a solved problem");

    let headings = headings(document);
    assert_eq!(
        headings[0],
        (
            1,
            "TableFormer: Table Structure Understanding with Transformers.".to_owned()
        )
    );
    for heading in [
        "Abstract",
        "1. Introduction",
        "2. Previous work and State of the Art",
        "References",
    ] {
        assert!(
            headings
                .iter()
                .any(|(level, text)| text == heading && *level > 1),
            "heading missing: {heading}"
        );
    }
    assert!(
        headings
            .iter()
            .any(|(level, text)| text == "5.1. Implementation Details" && *level == 3)
    );
    let furniture = block_texts(&document.furniture);
    for page in 1..=10 {
        assert!(furniture.contains(&page.to_string()), "page number {page}");
    }
    let stats = stats(&output);
    assert_eq!((stats.pages, stats.text_pages), (10, 10));
    assert_eq!(stats.producer.as_deref(), Some("pdfTeX-1.40.21"));
}

#[test]
fn ocr_text_layer_is_read() {
    let output = read("scan/en-alice-p3.pdf");
    assert_well_formed(&output);
    let text = block_texts(&output.document.body).join("\n");
    assert!(text.contains("DOWN THE RABBIT HOLE"));
    assert!(text.contains("In another moment down went Alice after it"));
    let stats = stats(&output);
    assert_eq!((stats.pages, stats.text_pages), (1, 1));
    assert_eq!(
        output.document.meta.title.as_deref(),
        Some("Alice's adventures in Wonderland")
    );
}

#[test]
fn limits_are_enforced() {
    let decision = fixture("pdf/pd-vi-qd-1922-2026.pdf");
    let garden = fixture("pdf/vi-garden-route.pdf");
    let with = |update: fn(&mut Limits)| {
        let mut limits = Limits::local();
        update(&mut limits);
        limits
    };
    assert_eq!(
        pdf::read(&decision, &with(|limits| limits.max_pages = Some(1))).err(),
        Some(ReadError::TooManyPages {
            actual: 2,
            limit: 1
        })
    );
    assert!(pdf::read(&decision, &with(|limits| limits.max_pages = Some(2))).is_ok());
    assert_eq!(
        pdf::read(&garden, &with(|limits| limits.max_blocks = 3)).err(),
        Some(ReadError::TooManyBlocks { limit: 3 })
    );
    assert_eq!(
        pdf::read(
            &garden,
            &with(|limits| limits.max_input_bytes = Some(1_000))
        )
        .err(),
        Some(ReadError::InputTooLarge {
            actual: garden.len() as u64,
            limit: 1_000
        })
    );
    assert!(matches!(
        pdf::read(&garden, &with(|limits| limits.max_decompressed_bytes = 256)),
        Err(ReadError::DecompressedTooLarge { limit: 256 })
    ));
    assert_eq!(
        pdf::read(&decision, &with(|limits| limits.max_nesting_depth = 0)).err(),
        Some(ReadError::NestingTooDeep { limit: 0 })
    );
    assert_eq!(
        pdf::read(&garden, &with(|limits| limits.max_pages = Some(0))).err(),
        Some(ReadError::InvalidLimits(LimitsError::ZeroLimit {
            field: "max_pages"
        }))
    );
}

#[test]
fn malformed_input_is_a_typed_error() {
    for input in [
        &b""[..],
        b"not a pdf at all",
        b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF",
        &[0xFF; 4096],
    ] {
        assert_eq!(
            pdf::read(input, &Limits::local()).err(),
            Some(ReadError::Malformed)
        );
    }
    let garden = fixture("pdf/vi-garden-route.pdf");
    // A truncated file either recovers or fails cleanly; it never panics.
    for length in [16, 512, garden.len() / 2, garden.len() - 64] {
        let _ = pdf::read(&garden[..length], &Limits::local());
    }
}

fn qpdf_encrypt(input: &Path, user_password: &str, bits: &str, name: &str) -> Option<Vec<u8>> {
    let directory = std::env::temp_dir().join(format!("ariad-pdf-reader-{}", std::process::id()));
    std::fs::create_dir_all(&directory).ok()?;
    let output = directory.join(name);
    let mut command = Command::new("qpdf");
    command
        .arg("--encrypt")
        .arg(user_password)
        .arg("owner-secret")
        .arg(bits);
    if bits == "128" {
        command.arg("--use-aes=y");
    }
    let status = command.arg("--").arg(input).arg(&output).status().ok()?;
    let bytes = status
        .success()
        .then(|| std::fs::read(&output).ok())
        .flatten();
    let _ = std::fs::remove_file(&output);
    let _ = std::fs::remove_dir(&directory);
    bytes
}

#[test]
fn encrypted_pdfs_open_with_the_empty_user_password_only() {
    let input = fixture_path("pdf/vi-garden-route.pdf");
    if Command::new("qpdf").arg("--version").output().is_err() {
        eprintln!("skipping encrypted PDF test: qpdf is not installed");
        return;
    }
    let plain = read("pdf/vi-garden-route.pdf");
    for bits in ["256", "128"] {
        let encrypted = qpdf_encrypt(&input, "", bits, &format!("empty-{bits}.pdf"))
            .expect("qpdf encrypts the fixture");
        let output = pdf::read(&encrypted, &Limits::local()).unwrap();
        assert_well_formed(&output);
        assert!(stats(&output).encrypted, "{bits}-bit");
        assert_eq!(output.document.body, plain.document.body, "{bits}-bit");
        assert_eq!(output.document.meta.title, plain.document.meta.title);

        let locked = qpdf_encrypt(&input, "user-secret", bits, &format!("locked-{bits}.pdf"))
            .expect("qpdf encrypts the fixture");
        assert_eq!(
            pdf::read(&locked, &Limits::local()).err(),
            Some(ReadError::Encrypted),
            "{bits}-bit"
        );
    }
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "timing is meaningful in release builds only"
)]
fn large_fixtures_read_quickly() {
    for path in [
        "pdf/pd-vi-decree-39-2022.pdf",
        "pdf/arxiv-tableformer-2203-01017-v1.pdf",
    ] {
        let bytes = fixture(path);
        let started = Instant::now();
        pdf::read(&bytes, &Limits::local()).unwrap();
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(2), "{path} took {elapsed:?}");
    }
}

fn synthetic_pdf(
    content: &[u8],
    extra: impl FnOnce(&mut lopdf::Document, &mut lopdf::Dictionary),
) -> Vec<u8> {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let mut resources = dictionary! { "Font" => dictionary! { "F1" => font } };
    extra(&mut doc, &mut resources);
    let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()], "Resources" => resources, "Contents" => stream });
    doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn object_stream_bombs_have_typed_per_stream_and_total_limits() {
    use lopdf::{Stream, dictionary};
    let make = |count| {
        // lopdf's serializer prunes old ObjStm objects, so save a same-length name first.
        let mut bytes = synthetic_pdf(b"BT /F1 12 Tf 20 700 Td (Text) Tj ET", |doc, _| {
            for _ in 0..count {
                let mut stream = Stream::new(
                    dictionary! { "Type" => "ObjStr", "N" => 0, "First" => 0 },
                    vec![b' '; 64 * 1024],
                );
                stream.compress().unwrap();
                doc.add_object(stream);
            }
        });
        for index in 0..bytes.len().saturating_sub(6) {
            if &bytes[index..index + 7] == b"/ObjStr" {
                bytes[index..index + 7].copy_from_slice(b"/ObjStm");
            }
        }
        bytes
    };
    for (count, limit) in [(1, 1024), (3, 128 * 1024)] {
        let bytes = make(count);
        let limits = Limits {
            max_decompressed_bytes: limit,
            ..Limits::local()
        };
        assert_eq!(
            pdf::read(&bytes, &limits).err(),
            Some(ReadError::DecompressedTooLarge { limit })
        );
    }
}

#[test]
fn inflated_page_tree_counts_do_not_overflow_iteration_hints() {
    use lopdf::{Document, Object, dictionary};
    let source = synthetic_pdf(b"BT /F1 12 Tf 20 700 Td (Text) Tj ET", |_, _| {});
    let mut doc = Document::load_mem(&source).unwrap();
    let root = doc
        .catalog()
        .unwrap()
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    let page = doc.page_iter().next().unwrap();
    let mut kids = vec![Object::Reference(page)];
    for _ in 0..3 {
        let child = doc.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page)],
            "Count" => i64::MAX,
        });
        kids.push(Object::Reference(child));
    }
    doc.get_dictionary_mut(root).unwrap().set("Kids", kids);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();

    for input in [
        bytes.as_slice(),
        include_bytes!("../../../fuzz/seeds/pdf_reader/inflated-page-counts.pdf").as_slice(),
    ] {
        let output = pdf::read(input, &Limits::local()).unwrap();
        assert_eq!(stats(&output).pages, 1);
        assert_well_formed(&output);
    }
}

#[test]
fn decomposed_tounicode_targets_are_normalized_before_layout() {
    use lopdf::{Object, Stream, dictionary};
    let bytes = synthetic_pdf(b"BT /F2 12 Tf 20 700 Td <0001> Tj ET", |doc, resources| {
        let cmap = doc.add_object(Stream::new(dictionary! {}, b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfchar <0001> <006503020301> endbfchar".to_vec()));
        let descendant = doc.add_object(dictionary! { "Subtype" => "CIDFontType2", "DW" => 500 });
        let font = doc.add_object(dictionary! { "Subtype" => "Type0", "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(descendant)], "ToUnicode" => cmap });
        resources.set("Font", dictionary! { "F2" => font });
    });
    let read = pdf::read(&bytes, &Limits::local()).unwrap();
    assert_eq!(block_texts(&read.document.body), ["ế"]);
}

#[test]
fn symbol_and_dingbats_fonts_use_their_built_in_encodings() {
    use lopdf::dictionary;
    let bytes = synthetic_pdf(
        b"BT /F2 12 Tf 20 700 Td <616267> Tj ET BT /F3 12 Tf 20 650 Td (l) Tj /F1 12 Tf ( Dingbat item) Tj ET",
        |doc, resources| {
            let plain = doc.add_object(
                dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
            );
            let symbol = doc.add_object(
                dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ABCDEF+Symbol" },
            );
            let dingbats = doc.add_object(
                dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ZapfDingbats" },
            );
            resources.set(
                "Font",
                dictionary! { "F1" => plain, "F2" => symbol, "F3" => dingbats },
            );
        },
    );
    let read = pdf::read(&bytes, &Limits::local()).unwrap();
    assert_eq!(block_texts(&read.document.body), ["αβγ", "Dingbat item"]);
    assert!(
        read.document
            .body
            .iter()
            .any(|block| matches!(block, Block::List { ordered: false, .. })),
        "the Dingbats bullet must start a list item"
    );
}

#[test]
fn unmapped_pages_emit_both_mapping_and_ocr_warnings() {
    use lopdf::{Object, dictionary};
    let bytes = synthetic_pdf(b"BT /F2 12 Tf 20 700 Td <0001> Tj ET", |doc, resources| {
        let descendant = doc.add_object(dictionary! { "Subtype" => "CIDFontType2" });
        let font = doc.add_object(dictionary! { "Subtype" => "Type0", "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(descendant)] });
        resources.set("Font", dictionary! { "F2" => font });
    });
    let read = pdf::read(&bytes, &Limits::local()).unwrap();
    for code in [WarningCode::TextUnmapped, WarningCode::OcrRequired] {
        assert!(read.warnings.iter().any(|warning| warning.code == code));
    }
}

#[test]
fn encrypted_seed_is_checked_on_every_platform() {
    let bytes = include_bytes!("../../../fuzz/seeds/pdf_reader/encrypted-empty-password.pdf");
    let output = pdf::read(bytes, &Limits::local()).unwrap();
    assert!(stats(&output).encrypted);
    assert_well_formed(&output);
}

#[test]
fn deterministic_mutations_of_generated_pdf_return_typed_results() {
    let original = fixture("pdf/vi-garden-route.pdf");
    let mut state = 0x5a17_u64;
    for _ in 0..128 {
        let mut bytes = original.clone();
        for _ in 0..3 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let index = (state as usize) % bytes.len();
            bytes[index] ^= (state >> 32) as u8;
        }
        if let Ok(output) = pdf::read(&bytes, &Limits::local()) {
            let json = serde_json::to_vec(&output.document).unwrap();
            let back: Document = serde_json::from_slice(&json).unwrap();
            assert_eq!(output.document, back);
        }
    }
}

#[test]
fn font_transform_geometry_survives_json_round_trips() {
    let bytes = include_bytes!("../../../fuzz/seeds/pdf_reader/type3-running-header.pdf");
    let output = pdf::read(bytes, &Limits::local()).unwrap();
    assert_well_formed(&output);
}

#[test]
fn broken_xref_is_rejected_by_converter_and_flagged_by_bounded_loader() {
    let broken = fixture("pdf-tools/broken-xref.pdf");
    assert_eq!(
        pdf::read(&broken, &Limits::local()).err(),
        Some(ReadError::Malformed)
    );

    let loaded = pdf::load_bounded(&broken, &Limits::local())
        .expect("load_bounded repairs repairable cross-reference table");
    assert!(loaded.repaired_xref);
    assert_eq!(loaded.document.page_iter().count(), 1);

    let well_formed = fixture("pdf-tools/multipage-20.pdf");
    let clean = pdf::load_bounded(&well_formed, &Limits::local())
        .expect("load_bounded opens well-formed PDF strictly");
    assert!(!clean.repaired_xref);
    assert_eq!(clean.document.page_iter().count(), 20);
}
