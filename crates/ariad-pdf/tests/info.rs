//! Integration tests for `ariad_pdf::document` and `ariad_pdf::info` against the
//! `fixtures/pdf-tools/*.truth.json` corpus.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ariad_core::{
    limits::{Limits, LimitsError},
    reader::pdf::{self, ReadError, load_bounded},
};
use ariad_pdf::{
    PdfError, PdfInfo, ToolCtx,
    document::{self, open, open_for_info, open_with_ctx},
    info,
};
use lopdf::{
    Document, EncryptionState, EncryptionVersion, Permissions,
    encryption::crypt_filters::{Aes256CryptFilter, CryptFilter},
};
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools")
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name)).expect("read fixture file")
}

fn read_truth(stem: &str) -> Value {
    let raw = std::fs::read_to_string(fixtures_dir().join(format!("{stem}.truth.json")))
        .expect("read truth file");
    serde_json::from_str(&raw).expect("parse truth JSON")
}

fn encrypt_aes256(
    plaintext_bytes: &[u8],
    user_password: &str,
    owner_password: &str,
    allow_print_extract: bool,
) -> Vec<u8> {
    let mut doc = Document::load_mem(plaintext_bytes).expect("load plaintext PDF");
    let permissions = if allow_print_extract {
        Permissions::PRINTABLE
            | Permissions::PRINTABLE_IN_HIGH_QUALITY
            | Permissions::COPYABLE
            | Permissions::COPYABLE_FOR_ACCESSIBILITY
            | Permissions::ANNOTABLE
            | Permissions::FILLABLE
            | Permissions::MODIFIABLE
            | Permissions::ASSEMBLABLE
    } else {
        Permissions::empty()
    };
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let crypt_filters = BTreeMap::from([(b"StdCF".to_vec(), filter)]);
    let key = [0x42_u8; 32];
    let version = EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters,
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password,
        user_password,
        permissions,
    };
    let state = EncryptionState::try_from(version).expect("build AES-256 encryption state");
    doc.encrypt(&state).expect("encrypt PDF with AES-256");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save encrypted PDF");
    out
}

fn encrypt_rc4_40(plaintext_bytes: &[u8], user_password: &str, owner_password: &str) -> Vec<u8> {
    let mut doc = Document::load_mem(plaintext_bytes).expect("load plaintext PDF");
    let version = EncryptionVersion::V1 {
        document: &doc,
        owner_password,
        user_password,
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).expect("build RC4-40 encryption state");
    doc.encrypt(&state).expect("encrypt PDF with RC4-40");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save encrypted PDF");
    out
}

#[test]
fn pdf_info_matches_every_corpus_truth_file() {
    let dir = fixtures_dir();
    let mut truth_paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read fixtures/pdf-tools")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.file_name()?
                .to_str()?
                .ends_with(".truth.json")
                .then_some(path)
        })
        .collect();
    truth_paths.sort();
    assert_eq!(
        truth_paths.len(),
        22,
        "expected 22 truth files in fixtures/pdf-tools"
    );

    for truth_path in truth_paths {
        let file_name = truth_path.file_name().unwrap().to_str().unwrap();
        let stem = file_name.strip_suffix(".truth.json").unwrap();
        let truth = read_truth(stem);
        let mut expected_value = truth
            .get("pdf_info")
            .cloned()
            .unwrap_or_else(|| panic!("missing pdf_info in {file_name}"));
        let expected_info: PdfInfo = serde_json::from_value(expected_value.clone())
            .unwrap_or_else(|err| panic!("deserialize expected PdfInfo for {stem}: {err}"));
        if stem == "broken-xref" {
            assert_eq!(
                expected_value.get("repaired_xref"),
                Some(&Value::Bool(true)),
                "broken-xref.truth.json must declare repaired_xref: true"
            );
        } else if let Value::Object(ref mut map) = expected_value {
            map.entry("repaired_xref").or_insert(Value::Bool(false));
        }

        match stem {
            "encrypted-owner-only" => {
                let plaintext = read_fixture("encrypted-owner-only.plaintext.pdf");
                let encrypted_bytes =
                    encrypt_aes256(&plaintext, "", "ariad-test-owner-only", false);
                let actual = info::run(&encrypted_bytes, &Limits::local())
                    .expect("pdf.info inspects owner-password-only PDF");
                assert!(
                    actual.encrypted,
                    "encrypted-owner-only must report encrypted: true"
                );
                assert_eq!(actual, expected_info, "mismatch on {stem}");
                let actual_value = serde_json::to_value(&actual).unwrap();
                assert_eq!(actual_value, expected_value, "JSON mismatch on {stem}");

                // `document::open` must refuse to return a saveable decrypted handle.
                assert_eq!(
                    open(&encrypted_bytes, &Limits::local()).unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: false
                    },
                    "document::open must reject owner-password-only PDF without explicit unlock"
                );

                // Even the read-only handle returned by `open_for_info` must refuse mutation or saving.
                let mut ro_doc = open_for_info(&encrypted_bytes, &Limits::local())
                    .expect("open_for_info succeeds on owner-password-only PDF");
                assert!(ro_doc.is_encrypted());
                assert_eq!(
                    ro_doc.save_bytes().unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: false
                    }
                );
                assert_eq!(
                    ro_doc.inner_mut().unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: false
                    }
                );
                assert_eq!(
                    ro_doc.into_inner().unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: false
                    }
                );
            }
            "encrypted-aes256" => {
                let plaintext = read_fixture("encrypted-aes256.plaintext.pdf");
                let encrypted_bytes = encrypt_aes256(
                    &plaintext,
                    "ariad-test-user-aes256",
                    "ariad-test-owner-aes256",
                    true,
                );
                assert_eq!(
                    open(&encrypted_bytes, &Limits::local()).unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: true
                    }
                );
                assert_eq!(
                    info::run(&encrypted_bytes, &Limits::local()).unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: true
                    }
                );

                // Verify the plaintext companion matches every non-encryption field of the truth entry.
                let mut plain_info = info::run(&plaintext, &Limits::local())
                    .expect("plaintext source for encrypted-aes256 is valid");
                assert!(!plain_info.encrypted);
                plain_info.encrypted = true;
                assert_eq!(plain_info, expected_info, "mismatch on {stem} plaintext");
            }
            "encrypted-rc4-40" => {
                let plaintext = read_fixture("encrypted-rc4-40.plaintext.pdf");
                let encrypted_bytes =
                    encrypt_rc4_40(&plaintext, "ariad-user-rc4", "ariad-owner-rc4");
                assert_eq!(
                    open(&encrypted_bytes, &Limits::local()).unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: true
                    }
                );
                assert_eq!(
                    info::run(&encrypted_bytes, &Limits::local()).unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: true
                    }
                );

                let mut plain_info = info::run(&plaintext, &Limits::local())
                    .expect("plaintext source for encrypted-rc4-40 is valid");
                assert!(!plain_info.encrypted);
                plain_info.encrypted = true;
                assert_eq!(plain_info, expected_info, "mismatch on {stem} plaintext");
            }
            _ => {
                let pdf_bytes = read_fixture(&format!("{stem}.pdf"));
                let actual = info::run(&pdf_bytes, &Limits::local())
                    .unwrap_or_else(|err| panic!("info::run failed on {stem}: {err}"));
                assert_eq!(actual, expected_info, "mismatch on {stem}");
                let actual_value = serde_json::to_value(&actual).unwrap();
                assert_eq!(actual_value, expected_value, "JSON mismatch on {stem}");

                let doc = document::open(&pdf_bytes, &Limits::local())
                    .unwrap_or_else(|err| panic!("document::open failed on {stem}: {err}"));
                assert!(!doc.is_encrypted());
                assert_eq!(doc.repaired_xref(), stem == "broken-xref");
                let from_doc = info::from_doc(&doc).unwrap();
                assert_eq!(from_doc, expected_info, "from_doc mismatch on {stem}");
            }
        }
    }
}

#[test]
fn broken_xref_is_rejected_by_converter_but_repaired_and_reported_by_pdf_tools() {
    let broken_bytes = read_fixture("broken-xref.pdf");

    // (a) Converter path (`ariad_core::reader::pdf::read`) rejects broken cross-reference tables.
    assert_eq!(
        pdf::read(&broken_bytes, &Limits::local()).unwrap_err(),
        ReadError::Malformed,
        "converter must reject broken-xref.pdf with ReadError::Malformed"
    );

    // (b) `load_bounded` and `ariad_pdf::document::open` succeed and flag `repaired_xref`.
    let bounded = load_bounded(&broken_bytes, &Limits::local())
        .expect("load_bounded succeeds on repairable broken-xref.pdf");
    assert!(
        bounded.repaired_xref,
        "load_bounded must flag repaired_xref: true"
    );

    let doc = open(&broken_bytes, &Limits::local())
        .expect("document::open succeeds on repairable broken-xref.pdf");
    assert!(
        doc.repaired_xref(),
        "PdfDoc::repaired_xref() must return true on broken-xref.pdf"
    );
    assert!(doc.provenance().repaired_xref);

    let broken_info = info::run(&broken_bytes, &Limits::local())
        .expect("pdf.info succeeds on repairable broken-xref.pdf");
    assert!(broken_info.repaired_xref);
    assert_eq!(
        broken_info.warnings,
        vec!["cross-reference table was malformed and reconstructed from object offsets"]
    );
    let broken_json = serde_json::to_value(&broken_info).unwrap();
    assert_eq!(broken_json.get("repaired_xref"), Some(&Value::Bool(true)));
    assert!(
        broken_json
            .get("warnings")
            .and_then(Value::as_array)
            .is_some_and(|w| !w.is_empty())
    );

    // (c) Well-formed fixture (`multipage-20.pdf`) reports `repaired_xref: false` and no warnings.
    let multipage_bytes = read_fixture("multipage-20.pdf");
    let clean_bounded = load_bounded(&multipage_bytes, &Limits::local())
        .expect("load_bounded succeeds on multipage-20.pdf");
    assert!(!clean_bounded.repaired_xref);

    let clean_doc = open(&multipage_bytes, &Limits::local())
        .expect("document::open succeeds on multipage-20.pdf");
    assert!(!clean_doc.repaired_xref());

    let clean_info = info::run(&multipage_bytes, &Limits::local())
        .expect("pdf.info succeeds on multipage-20.pdf");
    assert!(!clean_info.repaired_xref);
    assert!(clean_info.warnings.is_empty());
    let clean_json = serde_json::to_value(&clean_info).unwrap();
    assert_eq!(
        clean_json.get("repaired_xref"),
        Some(&Value::Bool(false)),
        "well-formed PDF must always serialize repaired_xref: false"
    );
    assert!(clean_json.get("warnings").is_none());
}

#[test]
fn limits_are_enforced_by_open_and_info() {
    let multipage = read_fixture("multipage-20.pdf");

    let mut page_limit = Limits::local();
    page_limit.max_pages = Some(5);
    let err = info::run(&multipage, &page_limit).unwrap_err();
    assert_eq!(
        err,
        PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: 20,
            limit: 5,
        })
    );
    assert_eq!(err.exit_code(), 7);
    assert_eq!(err.cli_error_code(), "limit_exceeded");

    let mut size_limit = Limits::local();
    size_limit.max_input_bytes = Some(128);
    let err = open(&multipage, &size_limit).unwrap_err();
    assert_eq!(
        err,
        PdfError::LimitExceeded(ReadError::InputTooLarge {
            actual: multipage.len() as u64,
            limit: 128,
        })
    );
    assert_eq!(err.exit_code(), 7);

    let mut invalid = Limits::local();
    invalid.max_pages = Some(0);
    let err = open(&multipage, &invalid).unwrap_err();
    assert_eq!(
        err,
        PdfError::LimitExceeded(ReadError::InvalidLimits(LimitsError::ZeroLimit {
            field: "max_pages",
        }))
    );
}

#[test]
fn pages_node_rotate_attribute_is_inherited_by_child_pages() {
    use lopdf::{Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let stream_id = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 72 720 Td (Hello) Tj ET".to_vec(),
    ));
    let page1_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.28.into(), 841.89.into()],
        "Resources" => resources_id,
        "Contents" => stream_id,
    });
    let page2_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.28.into(), 841.89.into()],
        "Resources" => resources_id,
        "Contents" => stream_id,
        "Rotate" => 180,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page1_id), Object::Reference(page2_id)],
            "Count" => 2,
            "Rotate" => 90,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();

    let report = info::run(&bytes, &Limits::local()).expect("inspect synthetic rotated PDF");
    assert_eq!(report.page_count, 2);
    assert_eq!(report.pages[0].rotation, 90);
    assert_eq!(report.pages[0].inherited_rotation, Some(true));
    assert_eq!(report.pages[1].rotation, 180);
    assert_eq!(report.pages[1].inherited_rotation, None);
}

#[test]
fn repeated_content_stream_references_exceed_decompression_budget() {
    use lopdf::{Object, Stream, dictionary};

    let mut payload = b"BT /F1 12 Tf 72 720 Td (Hello) Tj ET\n".to_vec();
    payload.resize(4096, b' ');

    let mut stream = Stream::new(dictionary! {}, payload);
    stream.compress().expect("compress content stream");

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let stream_id = doc.add_object(stream);
    let repeated_contents: Vec<Object> = (0..32).map(|_| Object::Reference(stream_id)).collect();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => resources_id,
        "Contents" => repeated_contents,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialize synthetic PDF");

    // 32 references * 4,096 decoded bytes = 131,072 decoded bytes total.
    // A 65,536-byte decompression budget accepts a single 4,096-byte stream on load,
    // but rejects the 32 repeated references during `pdf.info`.
    let mut limits = Limits::local();
    limits.max_decompressed_bytes = 65_536;

    let err = info::run(&bytes, &limits).unwrap_err();
    assert_eq!(
        err,
        PdfError::LimitExceeded(ReadError::DecompressedTooLarge { limit: 65_536 })
    );
    assert_eq!(err.cli_error_code(), "limit_exceeded");
    assert_eq!(err.exit_code(), 7);

    // Also verify that an XMP `/Metadata` stream shares the same per-document budget.
    let mut xmp_payload =
        b"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta><rdf:RDF><rdf:Description><dc:title>Budget</dc:title>".to_vec();
    xmp_payload.resize(40_000, b' ');
    xmp_payload.extend_from_slice(b"</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>");
    let mut xmp_stream = Stream::new(
        dictionary! {
            "Type" => "Metadata",
            "Subtype" => "XML",
        },
        xmp_payload,
    );
    xmp_stream.compress().expect("compress XMP stream");

    let mut xmp_doc = Document::with_version("1.5");
    let xmp_pages_id = xmp_doc.new_object_id();
    let xmp_content_id = xmp_doc.add_object(Stream::new(dictionary! {}, vec![b' '; 32_768]));
    let xmp_page_id = xmp_doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => xmp_pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => xmp_content_id,
    });
    xmp_doc.objects.insert(
        xmp_pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(xmp_page_id)],
            "Count" => 1,
        }),
    );
    let xmp_meta_id = xmp_doc.add_object(xmp_stream);
    let xmp_catalog_id = xmp_doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => xmp_pages_id,
        "Metadata" => xmp_meta_id,
    });
    xmp_doc.trailer.set("Root", xmp_catalog_id);

    let mut xmp_bytes = Vec::new();
    xmp_doc
        .save_to(&mut xmp_bytes)
        .expect("serialize synthetic XMP PDF");

    let xmp_err = info::run(&xmp_bytes, &limits).unwrap_err();
    assert_eq!(
        xmp_err,
        PdfError::LimitExceeded(ReadError::DecompressedTooLarge { limit: 65_536 })
    );
    assert_eq!(xmp_err.cli_error_code(), "limit_exceeded");
}

#[test]
fn trailer_prescan_with_hundred_thousand_trailer_tokens_is_linear() {
    let mut bytes = Vec::with_capacity(100_000 * 8 + 64);
    bytes.extend_from_slice(b"%PDF-1.5\n");
    for _ in 0..100_000 {
        bytes.extend_from_slice(b"trailer\n");
    }
    bytes.extend_from_slice(b"startxref\n0\n%%EOF\n");

    let start = Instant::now();
    let _ = info::run(&bytes, &Limits::local());
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "100,000 trailer tokens took {elapsed:?}, expected < 1s"
    );
}

#[test]
fn cooperative_tool_ctx_cancels_and_times_out_page_loops() {
    let multipage = read_fixture("multipage-20.pdf");
    let doc = open(&multipage, &Limits::local()).expect("open multipage-20.pdf");

    let cancel = Arc::new(AtomicBool::new(true));
    let cancelled_ctx = ToolCtx::with_cancel(Arc::clone(&cancel));

    let open_err = open_with_ctx(&multipage, &Limits::local(), &cancelled_ctx).unwrap_err();
    assert_eq!(open_err, PdfError::CANCELLED);
    assert!(open_err.is_cancelled());
    assert_eq!(open_err.code(), "cancelled");

    let run_err = info::run_with_ctx(&multipage, &Limits::local(), &cancelled_ctx).unwrap_err();
    assert_eq!(run_err, PdfError::CANCELLED);
    assert!(run_err.is_cancelled());

    let from_doc_err = info::from_doc_with_ctx(&doc, &cancelled_ctx).unwrap_err();
    assert_eq!(from_doc_err, PdfError::CANCELLED);
    assert!(from_doc_err.is_cancelled());

    cancel.store(false, Ordering::Relaxed);
    let ok_info = info::from_doc_with_ctx(&doc, &cancelled_ctx).expect("uncancelled ctx succeeds");
    assert_eq!(ok_info.page_count, 20);

    let expired_ctx = ToolCtx::with_deadline(Instant::now() - Duration::from_millis(1));
    let timeout_err = info::from_doc_with_ctx(&doc, &expired_ctx).unwrap_err();
    assert_eq!(timeout_err, PdfError::TIMED_OUT);
    assert!(timeout_err.is_timed_out());
    assert_eq!(timeout_err.cli_error_code(), "limit_exceeded");
    assert_eq!(timeout_err.exit_code(), 7);
}
