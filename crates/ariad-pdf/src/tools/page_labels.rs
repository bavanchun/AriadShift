//! Implementation of the `pdf.page_labels` tool.

use std::collections::HashSet;

use ariad_core::tools::params::page_labels::{PageLabelRange, PageLabelStyle, PageLabelsParams};
use lopdf::{Dictionary, Object, dictionary};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::{OutputDoc, metadata::catalog_object_id, outline::encode_pdf_string},
};

/// Updates the `/PageLabels` number tree on the PDF catalog.
pub fn run(
    inputs: &[PdfDoc],
    params: &PageLabelsParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.page_labels requires exactly 1 input PDF, got {}",
            inputs.len()
        )));
    }
    if params.ranges.is_empty() {
        return Err(PdfError::InvalidParams(
            "pdf.page_labels `ranges` must not be empty".to_owned(),
        ));
    }

    let mut pdf = inputs[0].clone();
    let page_count = u32::try_from(pdf.inner().get_pages().len()).unwrap_or(0);
    let stem = naming::doc_stem(&pdf).unwrap_or_else(|| "document".to_owned());

    let mut seen_from = HashSet::with_capacity(params.ranges.len());
    for range in &params.ranges {
        if range.from == 0 || range.from > page_count {
            return Err(PdfError::InvalidParams(format!(
                "page label range `from` ({}) is out of bounds for {page_count}-page document",
                range.from
            )));
        }
        if let Some(0) = range.start {
            return Err(PdfError::InvalidParams(
                "page label range `start` must be >= 1".to_owned(),
            ));
        }
        if !seen_from.insert(range.from) {
            return Err(PdfError::InvalidParams(format!(
                "duplicate page label range starting at page {}",
                range.from
            )));
        }
    }

    let mut sorted_ranges: Vec<PageLabelRange> = params.ranges.clone();
    sorted_ranges.sort_by_key(|r| r.from);

    let mut nums = Vec::with_capacity(sorted_ranges.len() * 2);
    for range in &sorted_ranges {
        let zero_based = i64::from(range.from - 1);
        let mut label_dict = Dictionary::new();
        label_dict.set("Type", Object::Name(b"PageLabel".to_vec()));
        if let Some(style_code) = style_name(range.style) {
            label_dict.set("S", Object::Name(style_code.to_vec()));
        }
        if let Some(prefix) = &range.prefix
            && !prefix.is_empty()
        {
            label_dict.set("P", encode_pdf_string(prefix));
        }
        let start = range.start.unwrap_or(1);
        label_dict.set("St", Object::Integer(i64::from(start)));

        nums.push(Object::Integer(zero_based));
        nums.push(Object::Dictionary(label_dict));
    }

    let doc = pdf.inner_mut()?;
    let page_labels_id = doc.add_object(Object::Dictionary(dictionary! {
        "Nums" => Object::Array(nums),
    }));
    let catalog_id = catalog_object_id(doc)?;
    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .as_dict_mut()
        .map_err(|_| PdfError::Malformed)?;
    catalog.set("PageLabels", Object::Reference(page_labels_id));

    Ok(vec![OutputDoc::from_pdf(
        naming::default_pdf_output_name(&stem, "pdf.page_labels"),
        pdf,
    )])
}

const fn style_name(style: PageLabelStyle) -> Option<&'static [u8]> {
    match style {
        PageLabelStyle::Decimal => Some(b"D"),
        PageLabelStyle::RomanUpper => Some(b"R"),
        PageLabelStyle::RomanLower => Some(b"r"),
        PageLabelStyle::AlphaUpper => Some(b"A"),
        PageLabelStyle::AlphaLower => Some(b"a"),
        PageLabelStyle::None => None,
    }
}
