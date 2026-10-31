//! Integration tests for `ariad_pdf::document` and `ariad_pdf::info` against the
//! `fixtures/pdf-tools/*.truth.json` corpus.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use ariad_core::{
    limits::{Limits, LimitsError},
    reader::pdf::ReadError,
};
use ariad_pdf::{
    PdfError, PdfInfo,
    document::{self, open, open_for_info},
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
        let expected_value = truth
            .get("pdf_info")
            .cloned()
            .unwrap_or_else(|| panic!("missing pdf_info in {file_name}"));
        let expected_info: PdfInfo = serde_json::from_value(expected_value.clone())
            .unwrap_or_else(|err| panic!("deserialize expected PdfInfo for {stem}: {err}"));

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
                let from_doc = info::from_doc(&doc).unwrap();
                assert_eq!(from_doc, expected_info, "from_doc mismatch on {stem}");
            }
        }
    }
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
