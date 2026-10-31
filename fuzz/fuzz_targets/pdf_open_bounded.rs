//! Fuzzes bounded PDF loading (`ariad_core::reader::pdf::load_bounded`), `ariad_pdf::document::open`,
//! and `ariad_pdf::info::run` on arbitrary byte slices.
//!
//! Invariants checked on every input:
//! - Neither `load_bounded`, `document::open`, nor `info::run` panics or exceeds the configured
//!   page, object, or decompression budgets.
//! - Any document that `document::open` accepts is unencrypted (`!doc.is_encrypted()`).
//! - Any document whose `info::run` reports `encrypted == true` is rejected by `document::open`
//!   with `PdfError::Encrypted { needs_password: false }`.
//! - Every `PdfInfo` returned by `info::run` round-trips through `serde_json` and has NFC-normalized
//!   metadata fields.
#![no_main]

use ariad_core::{
    limits::Limits,
    reader::pdf::{MAX_OBJECTS, load_bounded},
};
use ariad_pdf::{PdfError, PdfInfo, document, info};
use libfuzzer_sys::fuzz_target;
use unicode_normalization::UnicodeNormalization;

const MAX_FUZZ_INPUT: usize = 256 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_INPUT {
        return;
    }

    let local_limits = Limits::local();
    let tight_limits = Limits {
        max_input_bytes: Some(64 * 1024),
        max_pages: Some(16),
        max_decompressed_bytes: 512 * 1024,
        max_blocks: 256,
        max_nesting_depth: 16,
        ..Limits::local()
    };

    for limits in [&local_limits, &tight_limits] {
        if let Ok(doc) = load_bounded(data, limits) {
            assert!(doc.objects.len() <= MAX_OBJECTS);
            assert!(doc.reference_table.entries.len() <= MAX_OBJECTS);
        }

        let open_res = document::open(data, limits);
        if let Ok(ref doc) = open_res {
            assert!(
                !doc.is_encrypted(),
                "document::open must never return an encrypted handle"
            );
        }

        if let Ok(report) = info::run(data, limits) {
            if let Some(max_pages) = limits.max_pages {
                assert!(report.page_count <= max_pages);
            }
            assert_eq!(report.pages.len(), report.page_count as usize);
            if report.encrypted {
                assert_eq!(
                    open_res.unwrap_err(),
                    PdfError::Encrypted {
                        needs_password: false
                    },
                    "owner-password-only PDF inspected by pdf.info must be rejected by document::open"
                );
            }
            for field in [
                report.metadata.title.as_deref(),
                report.metadata.author.as_deref(),
                report.metadata.creator.as_deref(),
                report.metadata.producer.as_deref(),
                report.metadata.creation_date.as_deref(),
                report.metadata.mod_date.as_deref(),
                report.metadata.language.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                assert_eq!(
                    field,
                    field.nfc().collect::<String>(),
                    "metadata strings must be NFC-normalized"
                );
            }
            let encoded = serde_json::to_vec(&report).expect("PdfInfo serializes");
            let decoded: PdfInfo = serde_json::from_slice(&encoded).expect("PdfInfo deserializes");
            assert_eq!(report, decoded);
        }
    }
});
