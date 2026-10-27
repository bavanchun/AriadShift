//! Resource bounds of the PDF reader on synthetic hostile files: each read ends in a typed
//! limit error or finishes within a time bound.
#![cfg(feature = "pdf")]

use std::time::{Duration, Instant};

use ariad_core::{
    limits::Limits,
    reader::pdf::{self, ReadError},
};
use lopdf::{Dictionary, Document, Object, Stream, dictionary};

/// A one-page PDF, `height` points tall, whose resources hold font `F1` plus what `extra` adds.
fn synthetic_pdf(
    content: &[u8],
    height: i64,
    extra: impl FnOnce(&mut Document, &mut Dictionary),
) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let mut resources = dictionary! { "Font" => dictionary! { "F1" => font } };
    extra(&mut doc, &mut resources);
    let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), height.into()],
        "Resources" => resources,
        "Contents" => stream,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 },
        ),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn tall_page_of_hyphenated_lines_reads_in_bounded_time() {
    // 50,000 identical `ab-` lines at one x, size and pitch form one block. Each join used to
    // rescan, copy and lowercase the whole merged word.
    const LINES: usize = 50_000;
    let top = LINES * 6 / 5 + 10;
    let mut content = format!("BT /F1 1 Tf 1.2 TL 10 {top} Td").into_bytes();
    for _ in 0..LINES {
        content.extend_from_slice(b" (ab-) '");
    }
    content.extend_from_slice(b" ET");
    let height = i64::try_from(top + 20).unwrap();
    let bytes = synthetic_pdf(&content, height, |_, _| {});

    let started = Instant::now();
    let output = pdf::read(&bytes, &Limits::local()).expect("tall hyphenated page reads");
    let elapsed = started.elapsed();
    eprintln!("PDF bounds debug elapsed: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "hyphen joining took too long: {elapsed:?}"
    );
    assert_eq!(
        output.document.body.len(),
        1,
        "every line must stay in one paragraph, or the test no longer exercises joining"
    );
}

#[test]
fn fonts_sharing_one_large_differences_array_load_in_bounded_time() {
    // 3,000 fonts share one 400,000-element `/Differences` array. Integer entries reset the
    // code, so every font resolved every element: about 1.2e9 steps from about 1.4 MB of input.
    const FONTS: usize = 3_000;
    let mut content = b"BT".to_vec();
    for index in 0..FONTS {
        content.extend_from_slice(format!(" /G{index} 12 Tf").as_bytes());
    }
    content.extend_from_slice(b" 20 700 Td (x) Tj ET");
    let bytes = synthetic_pdf(&content, 792, |doc, resources| {
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

    let started = Instant::now();
    let result = pdf::read(&bytes, &Limits::local());
    let elapsed = started.elapsed();
    eprintln!("PDF bounds debug elapsed: {elapsed:?}");
    assert!(
        matches!(result, Ok(_) | Err(ReadError::DecompressedTooLarge { .. })),
        "{:?}",
        result.as_ref().err()
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "shared /Differences took too long: {elapsed:?}"
    );
}
