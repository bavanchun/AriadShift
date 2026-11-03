#![no_main]

use ariad_core::{
    limits::Limits,
    tools::params::form_model::{FieldType, FormValue},
};
use ariad_pdf::{
    ToolCtx,
    annotations::{self, AnnotationSelector},
    document, forms,
};
use libfuzzer_sys::fuzz_target;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    let limits = Limits {
        max_input_bytes: Some(64 * 1024),
        max_pages: Some(8),
        max_blocks: 256,
        max_nesting_depth: 16,
        max_decompressed_bytes: 512 * 1024,
        ..Limits::local()
    };
    let ctx = ToolCtx::with_deadline(Instant::now() + Duration::from_millis(500));
    if let Ok(pdf) = document::open_with_ctx(data, &limits, &ctx) {
        if let Ok(listing) = forms::list::list(&pdf, &[1], true, &limits, &ctx) {
            if let Some(field) = listing
                .fields
                .iter()
                .find(|field| field.field_type == Some(FieldType::Text))
            {
                let value = String::from_utf8_lossy(&data[..data.len().min(32)]).into_owned();
                let mut edited = pdf.clone();
                let _ = forms::fill::fill(
                    &mut edited,
                    &BTreeMap::from([(field.name.clone(), FormValue::Text(value))]),
                    &forms::fill::FillOptions::default(),
                    &limits,
                    &ctx,
                );
                let _ = forms::flatten::flatten(
                    &mut edited,
                    &[1],
                    Some(std::slice::from_ref(&field.name)),
                    &limits,
                    &ctx,
                );
            }
        }
        let selector = AnnotationSelector {
            pages: vec![1],
            ids: None,
            types: None,
            all: true,
        };
        let _ = annotations::read::list(&pdf, &selector, &limits, &ctx);
        let mut edited = pdf.clone();
        let _ = annotations::flatten::flatten(&mut edited, &selector, &limits, &ctx);
    }
});
