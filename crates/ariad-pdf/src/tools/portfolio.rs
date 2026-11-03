//! Implementation of the `pdf.portfolio` tool.

use std::collections::HashSet;

use ariad_core::tools::params::portfolio::{PortfolioAction, PortfolioParams, PortfolioView};
use lopdf::{Document, Object, Stream, StringFormat, dictionary};
use serde_json::Value;

use crate::{
    document::{self, PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::{
        InputDoc, InputSource, OutputDoc,
        attachments::{
            EmbeddedFileEntry, collect_attachment_payloads_from_inputs, entries_to_summaries,
            extract_embedded_files_outputs_with_limits, read_embedded_files_with_limits,
            write_embedded_files,
        },
        metadata::catalog_object_id,
    },
    writer::{self, SaveOptions},
};

/// Creates, lists, or extracts a PDF collection portfolio (`/Collection`).
pub fn run(
    inputs: &[PdfDoc],
    params: &PortfolioParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let input_docs: Vec<InputDoc> = inputs
        .iter()
        .map(|pdf| {
            let stem = naming::doc_stem(pdf).unwrap_or_else(|| "portfolio".to_owned());
            InputDoc::from_doc(stem, pdf.clone())
        })
        .collect();
    run_inputs(&input_docs, params, ctx).map(|(_, outs)| outs)
}

/// Creates, lists, or extracts a PDF collection portfolio (`/Collection`) from raw [`InputDoc`] handles.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &PortfolioParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    ctx.check()?;
    if inputs.is_empty() {
        return Err(PdfError::InvalidParams(
            "pdf.portfolio requires at least 1 input PDF".to_owned(),
        ));
    }

    let first_stem = {
        let clean = inputs[0].clean_stem();
        if clean.is_empty() {
            "portfolio".to_owned()
        } else {
            clean
        }
    };

    match params.action {
        PortfolioAction::Create => {
            let carrier_limits = &inputs[0].limits;
            let explicit_names = unique_portfolio_filenames(inputs);
            let entries = collect_attachment_payloads_from_inputs(
                inputs,
                Some(&explicit_names),
                carrier_limits,
                ctx,
            )?;
            let mut doc = build_portfolio_document(&entries, params.view)?;
            let raw_bytes =
                writer::save_document_with_options(&mut doc, ctx, &SaveOptions::default())?;
            let pdf = document::open_with_ctx(&raw_bytes, carrier_limits, ctx)?;
            let opened_docs: Vec<PdfDoc> = inputs
                .iter()
                .filter_map(|input| match &input.source {
                    InputSource::Doc(existing) => Some((**existing).clone()),
                    InputSource::Bytes(_) => None,
                })
                .collect();
            Ok((
                opened_docs,
                vec![OutputDoc::from_pdf(
                    naming::default_pdf_output_name(&first_stem, "pdf.portfolio"),
                    pdf,
                )],
            ))
        }
        PortfolioAction::List => {
            if inputs.len() != 1 {
                return Err(PdfError::InvalidParams(format!(
                    "pdf.portfolio `list` requires exactly 1 input PDF, got {}",
                    inputs.len()
                )));
            }
            let portfolio_pdf = inputs[0].open_for_mutation(ctx)?;
            let stem = naming::doc_stem(&portfolio_pdf).unwrap_or(first_stem);
            let entries = read_embedded_files_with_limits(&portfolio_pdf, &inputs[0].limits, ctx)?;
            let summaries = entries_to_summaries(&entries);
            let out_name = naming::single_output_name(&stem, "portfolio", "json");
            let json_val: Value =
                serde_json::to_value(&summaries).map_err(|_| PdfError::Malformed)?;
            Ok((
                vec![portfolio_pdf],
                vec![OutputDoc::from_json(out_name, json_val)?],
            ))
        }
        PortfolioAction::Extract => {
            if inputs.len() != 1 {
                return Err(PdfError::InvalidParams(format!(
                    "pdf.portfolio `extract` requires exactly 1 input PDF, got {}",
                    inputs.len()
                )));
            }
            let portfolio_pdf = inputs[0].open_for_mutation(ctx)?;
            let stem = naming::doc_stem(&portfolio_pdf).unwrap_or(first_stem);
            let outputs = extract_embedded_files_outputs_with_limits(
                &portfolio_pdf,
                &stem,
                None,
                &inputs[0].limits,
                ctx,
            )?;
            Ok((vec![portfolio_pdf], outputs))
        }
    }
}

fn unique_portfolio_filenames(inputs: &[InputDoc]) -> Vec<String> {
    let mut used = HashSet::with_capacity(inputs.len());
    let mut names = Vec::with_capacity(inputs.len());

    for (idx, input) in inputs.iter().enumerate() {
        let raw_base = input
            .stem
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&input.stem)
            .trim();
        let base_stem = if raw_base.is_empty() {
            format!("document-{}", idx + 1)
        } else {
            raw_base.to_owned()
        };
        let (stem_part, ext_part) = match base_stem.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_owned(), e.to_owned()),
            _ => (base_stem, "pdf".to_owned()),
        };

        let mut candidate = format!("{stem_part}.{ext_part}");
        let mut suffix = 2u32;
        while !used.insert(candidate.clone()) {
            candidate = format!("{stem_part}-{suffix}.{ext_part}");
            suffix += 1;
        }
        names.push(candidate);
    }

    names
}

fn build_portfolio_document(
    entries: &[EmbeddedFileEntry],
    view: PortfolioView,
) -> Result<Document, PdfError> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    }));
    let resources_id = doc.add_object(Object::Dictionary(dictionary! {
        "Font" => dictionary! {
            "F1" => Object::Reference(font_id),
        },
    }));

    let mut content_ops = String::from(
        "BT\n/F1 16 Tf\n72 760 Td\n(AriadShift PDF Portfolio) Tj\n0 -28 Td\n/F1 12 Tf\n",
    );
    for (idx, entry) in entries.iter().enumerate() {
        let escaped = escape_pdf_literal(&format!("{}. {}", idx + 1, entry.name));
        content_ops.push_str(&format!("({escaped}) Tj\n0 -18 Td\n"));
    }
    content_ops.push_str("ET\n");

    let content_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        content_ops.into_bytes(),
    )));

    let page_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => Object::Reference(resources_id),
        "Contents" => Object::Reference(content_id),
    }));

    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );

    let view_name = match view {
        PortfolioView::Details => b"D".to_vec(),
        PortfolioView::Tile => b"T".to_vec(),
    };
    let mut collection_dict = dictionary! {
        "Type" => "Collection",
        "View" => Object::Name(view_name),
    };
    if let Some(first) = entries.first() {
        collection_dict.set(
            "D",
            Object::String(first.name.as_bytes().to_vec(), StringFormat::Literal),
        );
    }
    let collection_id = doc.add_object(Object::Dictionary(collection_dict));

    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
        "Collection" => Object::Reference(collection_id),
        "PageMode" => Object::Name(b"UseAttachments".to_vec()),
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));

    let mut sorted_entries = entries.to_vec();
    sorted_entries.sort_by(|a, b| a.name.cmp(&b.name));
    write_embedded_files(&mut doc, &sorted_entries)?;

    let _ = catalog_object_id(&doc)?;
    Ok(doc)
}

fn escape_pdf_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}
