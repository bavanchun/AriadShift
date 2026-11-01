//! Implementation of the `pdf.divide_pages` tool.

use std::collections::HashMap;

use ariad_core::{
    reader::pdf::ReadError,
    tools::{
        page_range::DuplicatePolicy,
        params::{common::GridOrder, divide_pages::DividePagesParams, scale::ScaleFit},
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::{
        OutputDoc,
        nup::{
            append_clipped_xobject, collect_ordered_page_ids, convert_page_with_budget,
            effective_version, finalize_layout_document, insert_sheet_page,
        },
    },
    xobject::{self, Matrix, ObjectCopier, PdfRect},
};

/// Subdivides each selected page into a `cols × rows` grid of smaller pages.
pub fn run(
    inputs: &[PdfDoc],
    params: &DividePagesParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.divide_pages requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }
    if !(1..=8).contains(&params.cols) || !(1..=8).contains(&params.rows) {
        return Err(PdfError::InvalidParams(format!(
            "grid dimensions ({}x{}) out of bounds: expected cols and rows in 1..=8",
            params.cols, params.rows
        )));
    }

    let src_pdf = &inputs[0];
    let src_doc = src_pdf.inner();
    let src_pages = collect_ordered_page_ids(src_doc, ctx)?;
    let page_count = u32::try_from(src_pages.len()).unwrap_or(u32::MAX);
    let selected = params
        .pages
        .expand(page_count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;

    let cells_per_page = usize::from(params.cols) * usize::from(params.rows);
    let total_out_pages = selected.len().saturating_mul(cells_per_page);
    let cap = super::page_set::output_page_cap(ctx.max_pages);
    if (total_out_pages as u64) > cap {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: total_out_pages as u64,
            limit: u32::try_from(cap).unwrap_or(u32::MAX),
        }));
    }

    let mut target_doc = Document::with_version(effective_version(src_doc));
    let pages_root_id = target_doc.new_object_id();
    let mut copier = ObjectCopier::new();
    let mut budget = src_pdf.decode_budget();

    let mut new_page_ids = Vec::with_capacity(total_out_pages);
    let mut page_map: HashMap<ObjectId, ObjectId> = HashMap::with_capacity(selected.len());

    let mut cell_coords = Vec::with_capacity(cells_per_page);
    match params.order {
        GridOrder::Row => {
            for row in 0..params.rows {
                for col in 0..params.cols {
                    cell_coords.push((col, row));
                }
            }
        }
        GridOrder::Column => {
            for col in 0..params.cols {
                for row in 0..params.rows {
                    cell_coords.push((col, row));
                }
            }
        }
    }

    for page_num in selected {
        ctx.check()?;
        let src_page_id = src_pages[(page_num as usize) - 1];
        let form = convert_page_with_budget(
            src_doc,
            src_page_id,
            &mut target_doc,
            &mut copier,
            &mut budget,
            ctx,
        )?;

        let (visual_w, visual_h) = if matches!(form.rotation, 90 | 270) {
            (form.crop_box.height(), form.crop_box.width())
        } else {
            (form.crop_box.width(), form.crop_box.height())
        };
        let upright_rect = PdfRect::from_size(visual_w, visual_h);
        let upright_matrix = xobject::placement_matrix(
            form.crop_box,
            form.rotation,
            upright_rect,
            ScaleFit::Stretch,
        );

        let cell_w = visual_w / f64::from(params.cols);
        let cell_h = visual_h / f64::from(params.rows);
        let cell_box = PdfRect::from_size(cell_w, cell_h);

        for &(col, row) in &cell_coords {
            ctx.check()?;
            let cell_page_id = target_doc.new_object_id();
            page_map.entry(src_page_id).or_insert(cell_page_id);

            let cell_x0 = f64::from(col) * cell_w;
            let cell_y0 = visual_h - f64::from(row + 1) * cell_h;
            let cell_matrix = upright_matrix.then(Matrix::translate(-cell_x0, -cell_y0));

            let mut xobjects_dict = Dictionary::new();
            xobjects_dict.set("Fm1", Object::Reference(form.id));

            let mut content_buf = Vec::new();
            append_clipped_xobject(&mut content_buf, cell_box, "Fm1", cell_matrix);

            insert_sheet_page(
                &mut target_doc,
                cell_page_id,
                pages_root_id,
                cell_box,
                xobjects_dict,
                content_buf,
            );
            new_page_ids.push(cell_page_id);
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
