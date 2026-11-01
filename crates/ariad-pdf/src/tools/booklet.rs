//! Implementation of the `pdf.booklet` tool.

use std::collections::HashMap;

use ariad_core::{
    reader::pdf::ReadError,
    tools::params::{
        booklet::{BookletBinding, BookletParams},
        scale::ScaleFit,
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    paper,
    tools::{
        OutputDoc,
        nup::{
            MAX_OUTPUT_PAGES, append_clipped_xobject, collect_ordered_page_ids,
            convert_page_with_budget, effective_version, finalize_layout_document,
            insert_sheet_page,
        },
    },
    xobject::{self, ObjectCopier, PdfRect},
};

/// Imposes pages into saddle-stitch booklet spreads, padding with blank pages to a multiple of 4.
pub fn run(
    inputs: &[PdfDoc],
    params: &BookletParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.booklet requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }
    if !params.creep_pt.is_finite() || !(0.0..=1000.0).contains(&params.creep_pt) {
        return Err(PdfError::InvalidParams(format!(
            "invalid creep_pt {}: expected a finite number in 0.0..=1000.0",
            params.creep_pt
        )));
    }

    let src_pdf = &inputs[0];
    let src_doc = src_pdf.inner();
    let src_pages = collect_ordered_page_ids(src_doc, ctx)?;
    let sheet_dims = paper::resolve_paper_size(&params.sheet)?.landscape();
    let sheet_w = sheet_dims.width_pt;
    let sheet_h = sheet_dims.height_pt;
    let half_w = sheet_w / 2.0;

    let page_count = src_pages.len();
    let padded_total = page_count.div_ceil(4) * 4;
    let num_sheets = padded_total / 4;
    let total_out_pages = num_sheets.saturating_mul(2);
    if (total_out_pages as u64) > u64::from(MAX_OUTPUT_PAGES) {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: total_out_pages as u64,
            limit: MAX_OUTPUT_PAGES,
        }));
    }

    let mut target_doc = Document::with_version(effective_version(src_doc));
    let pages_root_id = target_doc.new_object_id();
    let mut copier = ObjectCopier::new();
    let mut budget = src_pdf.decode_budget();

    let mut new_page_ids = Vec::with_capacity(total_out_pages);
    let mut page_map: HashMap<ObjectId, ObjectId> = HashMap::with_capacity(page_count);
    let sheet_box = PdfRect::from_size(sheet_w, sheet_h);

    for s in 0..num_sheets {
        ctx.check()?;
        let shift = (s as f64) * f64::from(params.creep_pt);
        if half_w - shift <= 1.0 {
            return Err(PdfError::InvalidParams(format!(
                "creep_pt ({}) shifts inner pages by {shift} pt on sheet {}, exceeding half-sheet width ({half_w} pt)",
                params.creep_pt,
                s + 1
            )));
        }

        let left_rect = PdfRect::new(shift, 0.0, half_w, sheet_h);
        let right_rect = PdfRect::new(half_w, 0.0, sheet_w - shift, sheet_h);

        let front_lr = (padded_total - 2 * s, 2 * s + 1);
        let back_lr = (2 * s + 2, padded_total - 2 * s - 1);

        let (front_pair, back_pair) = match params.binding {
            BookletBinding::Left => (front_lr, back_lr),
            BookletBinding::Right => ((front_lr.1, front_lr.0), (back_lr.1, back_lr.0)),
        };

        for (left_p, right_p) in [front_pair, back_pair] {
            ctx.check()?;
            let spread_page_id = target_doc.new_object_id();
            let mut xobjects_dict = Dictionary::new();
            let mut content_buf = Vec::new();

            for (slot_idx, (logical_p, cell_rect)) in [(left_p, left_rect), (right_p, right_rect)]
                .into_iter()
                .enumerate()
            {
                if logical_p <= page_count {
                    let src_page_id = src_pages[logical_p - 1];
                    let form = convert_page_with_budget(
                        src_doc,
                        src_page_id,
                        &mut target_doc,
                        &mut copier,
                        &mut budget,
                        ctx,
                    )?;
                    page_map.entry(src_page_id).or_insert(spread_page_id);

                    let fm_name = format!("Fm{}", slot_idx + 1);
                    xobjects_dict.set(fm_name.as_bytes().to_vec(), Object::Reference(form.id));
                    let matrix = xobject::placement_matrix(
                        form.crop_box,
                        form.rotation,
                        cell_rect,
                        ScaleFit::Contain,
                    );
                    append_clipped_xobject(&mut content_buf, cell_rect, &fm_name, matrix);
                }
            }

            insert_sheet_page(
                &mut target_doc,
                spread_page_id,
                pages_root_id,
                sheet_box,
                xobjects_dict,
                content_buf,
            );
            new_page_ids.push(spread_page_id);
        }
    }

    finalize_layout_document(
        src_pdf,
        target_doc,
        pages_root_id,
        &new_page_ids,
        &page_map,
        &mut copier,
        false,
        ctx,
    )
}
