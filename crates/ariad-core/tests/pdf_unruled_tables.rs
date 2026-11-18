//! Whitespace-aligned table detection on generated and real PDFs.
#![cfg(feature = "pdf")]

#[path = "support/pdf_tables.rs"]
mod pdf_tables;

use ariad_core::writer::csv::find_tables;
use ariad_core::{
    ir::Block,
    limits::Limits,
    reader::pdf::{self, ReadError, ReadOptions},
};
use pdf_tables::{DETECT, fixture, paragraph_text, read, rows, table_pages, tables};

/// The pages of `unruled-negatives.pdf`: prose and layouts that look tabular, none of which may
/// yield a table.
const NEGATIVE_PAGES: [&str; 25] = [
    "three-column prose",
    "nested lists and a definition list",
    "poetry with line numbers",
    "monospaced code",
    "table of contents with dot leaders",
    "running header and footer",
    "justified text with wide gaps",
    "grids that are too small",
    "columns that drift",
    "aligned running text",
    "a table across both columns",
    "table of contents without leaders",
    "two levels of numbered clauses",
    "verse with a line number on every line",
    "label and value form, two per line",
    "signature block",
    "letterhead footer in three columns",
    "proportional code with aligned comments",
    "bibliography with aligned years",
    "bibliography with wrapped lines",
    "two-column index",
    "numbered equations",
    "list of abbreviations with page numbers",
    "quiz options in columns",
    "transcript with time codes",
];

#[test]
fn unruled_tables_are_detected_with_and_without_horizontal_rules() {
    assert_eq!(
        tables("unruled-tables.pdf", &DETECT),
        vec![
            // Booktabs style: rules above, below and under the head, none between columns.
            rows(&[
                &["Instrument", "Range", "Error", "Unit"],
                &["Caliper", "150", "0.02", "mm"],
                &["Micrometer", "25", "0.004", "mm"],
                &["Dial gauge", "10", "0.01", "mm"],
                &["Height gauge", "300", "0.03", "mm"],
                &["Depth rod", "200", "0.05", "mm"],
            ]),
            // No rules at all, and two cells are empty.
            rows(&[
                &["Region", "Spring", "Summer", "Autumn"],
                &["North", "12", "18", ""],
                &["South", "20", "25", "14"],
                &["East", "9", "", "11"],
                &["West", "15", "22", "13"],
            ]),
            // Inside the right column of a two-column page.
            rows(&[
                &["Name", "Size", "Speed", "Score"],
                &["Alpha", "12", "340", "0.91"],
                &["Beta", "48", "210", "0.87"],
                &["Gamma", "96", "120", "0.95"],
                &["Delta", "7", "410", "0.78"],
            ]),
        ]
    );
}

#[test]
fn detected_tables_have_a_bold_head_and_sit_among_their_paragraphs() {
    let document = read("unruled-tables.pdf", &DETECT);
    assert_eq!(table_pages(&document), [1, 1, 2]);
    let found = find_tables(&document.body);
    let heads: Vec<usize> = found
        .iter()
        .map(|table| match table {
            Block::Table { head, columns, .. } => {
                assert_eq!(columns.len(), 4);
                head.len()
            }
            _ => unreachable!(),
        })
        .collect();
    // Only the tables whose first row is bold have a head.
    assert_eq!(heads, [1, 0, 1]);

    let kinds: Vec<String> = document
        .body
        .iter()
        .map(|block| match block {
            Block::Table { .. } => "table".to_owned(),
            other => paragraph_text(other).unwrap_or_else(|| "other".to_owned()),
        })
        .collect();
    let position = |needle: &str| {
        kinds
            .iter()
            .position(|kind| kind.starts_with(needle))
            .unwrap_or_else(|| panic!("{needle} missing from {kinds:?}"))
    };
    let tables: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| *kind == "table")
        .map(|(index, _)| index)
        .collect();
    assert!(position("Table 1:") == tables[0] + 1);
    assert!(position("Table 2:") == tables[1] + 1);
    assert!(position("Table 3:") == tables[2] + 1);
    // The label that spans two columns is too uncertain to join the table and stays text.
    assert!(position("Throughput") < tables[2]);
}

#[test]
fn table_blocks_are_positioned_in_the_layout_index() {
    let document = read("unruled-tables.pdf", &DETECT);
    let layout = document.layout.as_ref().expect("layout index");
    let first = find_tables(&document.body)[0];
    let bbox = &layout.blocks.get(first.id()).expect("position").bbox;
    // The text of the first table runs from x 72 to about 418, and starts about 130 pt from the
    // top of the page.
    assert!((bbox.left - 72.0).abs() < 3.0, "{bbox:?}");
    assert!((bbox.right - 418.0).abs() < 8.0, "{bbox:?}");
    assert!((bbox.top - 131.0).abs() < 8.0, "{bbox:?}");
    assert!((bbox.bottom - 211.0).abs() < 8.0, "{bbox:?}");
}

#[test]
fn detection_is_off_by_default() {
    let default = pdf::read(&fixture("unruled-tables.pdf"), &Limits::local()).unwrap();
    assert!(find_tables(&default.document.body).is_empty());
}

#[test]
fn layouts_that_only_look_like_tables_stay_text() {
    let name = "unruled-negatives.pdf";
    let off = pdf::read(&fixture(name), &Limits::local()).unwrap();
    let on = pdf::read_with_options(&fixture(name), &Limits::local(), &DETECT).unwrap();
    let pages = off.document.layout.as_ref().expect("layout").pages.len();
    assert_eq!(pages, NEGATIVE_PAGES.len());
    let found = table_pages(&on.document);
    assert!(
        found.is_empty(),
        "tables found on the pages: {:?}",
        found
            .iter()
            .map(|&page| NEGATIVE_PAGES[page as usize - 1])
            .collect::<Vec<_>>()
    );
    assert_eq!(off.document, on.document);
    assert_eq!(off.warnings, on.warnings);
}

#[test]
fn a_table_beside_prose_in_a_column_does_not_pull_the_prose_in() {
    let document = read("unruled-tables.pdf", &DETECT);
    let found = find_tables(&document.body);
    for table in found {
        let Block::Table { body, head, .. } = table else {
            unreachable!()
        };
        // No cell holds a sentence of the surrounding text.
        let longest = head
            .iter()
            .chain(body)
            .flatten()
            .map(|cell| pdf_tables::cell_text(cell).chars().count())
            .max()
            .unwrap();
        assert!(longest <= 14, "{longest}");
    }
}

#[test]
fn prose_fixtures_read_the_same_with_detection_on() {
    for name in [
        "pd-en-gao-08-35-highlights.pdf",
        "pd-vi-decree-39-2022.pdf",
        "pd-vi-ip-law-155-2025.pdf",
        "pd-vi-qd-1922-2026.pdf",
    ] {
        let off = pdf::read(&fixture(name), &Limits::local()).unwrap();
        let on = pdf::read_with_options(&fixture(name), &Limits::local(), &DETECT).unwrap();
        assert!(find_tables(&on.document.body).is_empty(), "{name}");
        assert_eq!(off.document, on.document, "{name}");
        assert_eq!(off.warnings, on.warnings, "{name}");
    }
}

/// A table as `[row][cell]` text from a header and body rows.
fn grid<const N: usize>(header: [&str; N], body: &[[&str; N]]) -> Vec<Vec<String>> {
    std::iter::once(header)
        .chain(body.iter().copied())
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect()
}

#[test]
fn data_tables_of_a_research_paper_match_the_rendered_pages() {
    let found = tables("arxiv-tableformer-2203-01017-v1.pdf", &DETECT);
    // Reading order: the structure grid of Figure 1c, Tables 1 to 4, the two tables of Figure 5b
    // and the structure grid of Figure 6. The ruled ones are covered by the ruled-table tests.
    assert_eq!(found.len(), 8);
    // Table 1: the check marks of its Tags and Bbox columns are symbol glyphs without text.
    assert_eq!(
        found[1],
        rows(&[
            &["", "Tags", "Bbox", "Size", "Format"],
            &["PubTabNet", "", "", "509k", "PNG"],
            &["FinTabNet", "", "", "112k", "PDF"],
            &["TableBank", "", "", "145k", "JPEG"],
            &["Combined-Tabnet(*)", "", "", "400k", "PNG"],
            &["Combined(**)", "", "", "500k", "PNG"],
            &["SynthTabNet", "", "", "600k", "PNG"],
        ])
    );
    // Table 2: its head has three lines, and a label that spans columns sits over one of them.
    assert_eq!(
        found[2],
        rows(&[
            &["", "", "", "TEDS", ""],
            &["Model", "", "", "", ""],
            &["", "Dataset", "Simple", "Complex", "All"],
            &["EDD", "PTN", "91.1", "88.7", "89.9"],
            &["GTE", "PTN", "-", "-", "93.01"],
            &["TableFormer", "PTN", "98.5", "95.0", "96.75"],
            &["EDD", "FTN", "88.4", "92.08", "90.6"],
            &["GTE", "FTN", "-", "-", "87.14"],
            &["GTE (FT)", "FTN", "-", "-", "91.02"],
            &["TableFormer", "FTN", "97.5", "96.0", "96.8"],
            &["EDD", "TB", "86.0", "-", "86.0"],
            &["TableFormer", "TB", "89.6", "-", "89.6"],
            &["TableFormer", "STN", "96.9", "95.7", "96.7"],
        ])
    );
    assert_eq!(
        found[3],
        rows(&[
            &["Model", "Dataset", "mAP", "mAP (PP)"],
            &["EDD+BBox", "PubTabNet", "79.2", "82.7"],
            &["TableFormer", "PubTabNet", "82.1", "86.8"],
            &["TableFormer", "SynthTabNet", "87.7", "-"],
        ])
    );
    assert_eq!(
        found[4],
        rows(&[
            &["", "", "TEDS", ""],
            &["Model", "", "", ""],
            &["", "Simple", "Complex", "All"],
            &["Tabula", "78.0", "57.8", "67.9"],
            &["Traprange", "60.8", "49.9", "55.4"],
            &["Camelot", "80.0", "66.0", "73.0"],
            &["Acrobat Pro", "68.9", "61.8", "65.3"],
            &["EDD", "91.2", "85.4", "88.3"],
            &["TableFormer", "95.4", "90.1", "93.6"],
        ])
    );
}

#[test]
fn booktabs_tables_of_a_second_paper_match_its_text_layer() {
    let found = tables("arxiv-multilingual-open-text-2201-05609-v2.pdf", &DETECT);
    assert_eq!(found.len(), 2);
    // Characters in Wikipedia and in the corpus, page 6.
    assert_eq!(
        found[0],
        grid(
            ["Lang.", "Wikipedia", "MOT"],
            &[
                ["hau", "37,141,190", "38,341,381"],
                ["khm", "34,048,132", "93,948,921"],
                ["kin", "3,822,464", "23,881,242"],
                ["lao", "5,999,270", "61,419,714"],
                ["lin", "1,502,089", "1,744,378"],
                ["nde", "0", "31,600,251"],
                ["orm", "2,257,827", "10,469,043"],
                ["prs", "0", "67,421,867"],
                ["pus", "37,683,579", "127,570,695"],
                ["sna", "6,606,352", "29,132,817"],
                // The ISO 639-3 code of Somali is escaped: the spell checker reads it as a typo.
                ["so\u{6d}", "12,018,769", "18,244,956"],
                ["sqi", "157,576,602", "181,583,961"],
                ["tir", "152,456", "7,645,809"],
            ]
        )
    );
    // Documents, sentences and tokens per language, page 8. The page prints "70,1411" for yue.
    assert_eq!(
        found[1],
        grid(
            ["Language", "Documents", "Sentences", "Tokens"],
            &[
                ["amh", "23,457", "91,051", "1,960,739"],
                ["aze", "98,808", "644,156", "N/A"],
                ["bos", "74,923", "577,114", "N/A"],
                ["cmn", "352,893", "2,177,530", "130,214,418"],
                ["ell", "30,668", "155,284", "6,090,066"],
                ["eng", "554,119", "4,537,686", "193,129,912"],
                ["fas", "140,725", "871,887", "35,929,609"],
                ["fra", "91,348", "507,058", "19,966,932"],
                ["hat", "30,602", "100,558", "N/A"],
                ["hau", "52,644", "244,043", "N/A"],
                ["hye", "26,672", "150,086", "5,064,730"],
                ["ind", "240,399", "1,245,778", "38,383,509"],
                ["khm", "41,021", "392,724", "19,027,222"],
                ["kin", "29,793", "119,298", "N/A"],
                ["kor", "130,825", "1,516,790", "41,548,365"],
                ["lao", "36,311", "532,944", "12,058,686"],
                ["lin", "6,257", "18,757", "N/A"],
                ["mkd", "30,371", "245,127", "N/A"],
                ["mya", "81,772", "657,459", "36,006,802"],
                ["nde", "31,468", "211,156", "N/A"],
                ["orm", "10,144", "57,187", "N/A"],
                ["por", "52,514", "427,612", "13,864,438"],
                ["prs", "71,881", "461,203", "14,633,719"],
                ["pus", "141,293", "838,726", "N/A"],
                ["rus", "118,411", "1,051,201", "51,451,892"],
                ["sna", "28,051", "189,093", "N/A"],
                ["so\u{6d}", "38,376", "131,501", "N/A"],
                ["spa", "116,442", "911,685", "33,352,028"],
                ["sqi", "109,396", "793,622", "N/A"],
                ["srp", "75,618", "618,884", "26,544,508"],
                ["swh", "23,904", "63,761", "N/A"],
                ["tha", "34,073", "262,953", "9,428,506"],
                ["tir", "14,409", "76,283", "1,784,820"],
                ["tur", "121,033", "861,882", "31,419,370"],
                ["ukr", "65,924", "363,540", "17,232,122"],
                ["urd", "101,365", "986,220", "40,805,126"],
                ["uzb", "43,624", "314,141", "N/A"],
                ["vie", "182,366", "1,138,882", "59,843,930"],
                ["yue", "107,411", "70,1411", "34,730,065"],
                ["Total", "3,561,311", "25,246,273", "874,471,514"],
            ]
        )
    );
}

/// Every PDF of the fixture folder: the number of tables found with detection on. A fixture that
/// is not listed must yield none, so a table appearing on prose fails the test.
#[test]
fn only_known_tables_appear_across_all_fixtures() {
    let expected: &[(&str, usize)] = &[
        // Two booktabs tables, and no other text of the paper is mistaken for one.
        ("arxiv-multilingual-open-text-2201-05609-v2.pdf", 2),
        // Four data tables without full rulings, and four ruled tables drawn inside figures.
        ("arxiv-tableformer-2203-01017-v1.pdf", 8),
        ("en-map-room.pdf", 1),
        ("ruled-table-traps.pdf", 1),
        ("ruled-tables.pdf", 2),
        ("unruled-tables.pdf", 3),
        ("vi-garden-route.pdf", 1),
    ];
    let folder = format!("{}/../../fixtures/pdf", env!("CARGO_MANIFEST_DIR"));
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".pdf"))
        .collect();
    names.sort();
    assert!(names.len() > 70, "{names:?}");
    let options = ReadOptions {
        detect_tables: true,
    };
    for name in names {
        let result = pdf::read_with_options(&fixture(&name), &Limits::local(), &options);
        if name == "repaired-xref-extraction.pdf" {
            // The reader does not repair a broken cross-reference table; every other fixture
            // must read.
            assert!(matches!(result, Err(ReadError::Malformed)), "{name}");
            continue;
        }
        let output = result.unwrap_or_else(|error| panic!("{name}: {error}"));
        let count = find_tables(&output.document.body).len();
        let want = expected
            .iter()
            .find(|(known, _)| *known == name)
            .map_or(0, |&(_, count)| count);
        assert_eq!(count, want, "{name}");
    }
}

/// Prints every detected table of the fixtures named in `PDF_FIXTURES` (comma separated), with
/// its page; run with `--ignored --nocapture`. Unreadable files are reported, not failed.
#[test]
#[ignore = "measurement dump"]
fn dump_unruled_tables() {
    let names = std::env::var("PDF_FIXTURES").unwrap_or_default();
    for name in names.split(',').filter(|name| !name.is_empty()) {
        let Ok(output) = pdf::read_with_options(&fixture(name), &Limits::local(), &DETECT) else {
            println!("== {name}: unreadable");
            continue;
        };
        let pages = table_pages(&output.document);
        println!("== {name}: {} tables", pages.len());
        for (index, table) in tables(name, &DETECT).iter().enumerate() {
            let cells: usize = table.iter().map(Vec::len).sum();
            println!(
                "-- table {index} on page {}: {} rows, {cells} cells",
                pages[index],
                table.len()
            );
            for row in table {
                println!("   {}", row.join(" | "));
            }
        }
    }
}
