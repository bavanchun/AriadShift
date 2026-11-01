//! Implementation of the `pdf.posterize` tool.

use std::collections::HashMap;

use ariad_core::tools::{
    page_range::DuplicatePolicy,
    params::{posterize::PosterizeParams, scale::ScaleFit},
};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    paper,
    tools::{
        OutputDoc,
        nup::{
            append_clipped_xobject, collect_ordered_page_ids, convert_page_with_budget,
            effective_version, finalize_layout_document, fmt_f64, insert_sheet_page,
        },
    },
    xobject::{self, Matrix, ObjectCopier, PdfRect},
};

/// Inset from sheet edges (in points) reserved for corner crop marks when `crop_marks == true`.
const CROP_MARK_INSET_PT: f64 = 18.0;
/// Offset gap (in points) between a trim corner and the start of a crop mark tick.
const CROP_MARK_GAP_PT: f64 = 3.0;
/// Length (in points) of each corner crop mark tick.
const CROP_MARK_LEN_PT: f64 = 9.0;

/// Enlarges selected pages across multiple tiled output sheets with overlap and crop marks.
pub fn run(
    inputs: &[PdfDoc],
    params: &PosterizeParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.posterize requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }
    if params.scale.is_some() && params.tiles.is_some() {
        return Err(PdfError::InvalidParams(
            "pdf.posterize accepts either scale or tiles, not both".to_owned(),
        ));
    }
    if let Some(scale) = params.scale
        && (!scale.is_finite() || scale <= 1.0 || scale > 64.0)
    {
        return Err(PdfError::InvalidParams(format!(
            "invalid scale {scale}: expected a finite number in (1.0, 64.0]"
        )));
    }
    if let Some(tiles) = params.tiles
        && (!(1..=32).contains(&tiles.cols) || !(1..=32).contains(&tiles.rows))
    {
        return Err(PdfError::InvalidParams(format!(
            "invalid tiles ({}x{}): expected cols and rows in 1..=32",
            tiles.cols, tiles.rows
        )));
    }
    if !params.overlap_mm.is_finite() || !(0.0..=200.0).contains(&params.overlap_mm) {
        return Err(PdfError::InvalidParams(format!(
            "invalid overlap_mm {}: expected a finite number in 0.0..=200.0",
            params.overlap_mm
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

    let sheet_dims = paper::resolve_paper_size(&params.sheet)?;
    let sheet_w = sheet_dims.width_pt;
    let sheet_h = sheet_dims.height_pt;
    let sheet_box = PdfRect::from_size(sheet_w, sheet_h);

    let overlap_pt = f64::from(params.overlap_mm) * (72.0 / 25.4);
    let trim_inset = if params.crop_marks {
        CROP_MARK_INSET_PT
    } else {
        0.0
    };
    let tile_rect = PdfRect::new(
        trim_inset,
        trim_inset,
        sheet_w - trim_inset,
        sheet_h - trim_inset,
    );
    let tile_w = tile_rect.width();
    let tile_h = tile_rect.height();

    if tile_w <= overlap_pt + 1.0 || tile_h <= overlap_pt + 1.0 {
        return Err(PdfError::InvalidParams(format!(
            "overlap_mm ({}) and crop_marks inset ({trim_inset} pt) leave insufficient tile area on {}x{} pt sheet",
            params.overlap_mm, sheet_w, sheet_h
        )));
    }

    let step_x = tile_w - overlap_pt;
    let step_y = tile_h - overlap_pt;

    let mut target_doc = Document::with_version(effective_version(src_doc));
    let pages_root_id = target_doc.new_object_id();
    let mut copier = ObjectCopier::new();
    let mut budget = src_pdf.decode_budget();

    let mut new_page_ids = Vec::new();
    let mut page_map: HashMap<ObjectId, ObjectId> = HashMap::with_capacity(selected.len());

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

        let (cols, rows, poster_w, poster_h) = if let Some(tiles) = params.tiles {
            let c = usize::from(tiles.cols);
            let r = usize::from(tiles.rows);
            let pw = (c as f64) * tile_w - ((c - 1) as f64) * overlap_pt;
            let ph = (r as f64) * tile_h - ((r - 1) as f64) * overlap_pt;
            (c, r, pw, ph)
        } else {
            let scale = f64::from(params.scale.unwrap_or(2.0));
            let c = (((visual_w * scale) / sheet_w - 1e-3).ceil().max(1.0)) as usize;
            let r = (((visual_h * scale) / sheet_h - 1e-3).ceil().max(1.0)) as usize;
            let pw = (c as f64) * tile_w - ((c - 1) as f64) * overlap_pt;
            let ph = (r as f64) * tile_h - ((r - 1) as f64) * overlap_pt;
            (c, r, pw, ph)
        };

        let poster_rect = PdfRect::from_size(poster_w, poster_h);
        let poster_matrix =
            xobject::placement_matrix(form.crop_box, form.rotation, poster_rect, ScaleFit::Contain);

        for row in 0..rows {
            for col in 0..cols {
                ctx.check()?;
                let tile_page_id = target_doc.new_object_id();
                page_map.entry(src_page_id).or_insert(tile_page_id);

                let src_x0 = (col as f64) * step_x;
                let src_y0 = poster_h - (row as f64) * step_y - tile_h;
                let tile_matrix = poster_matrix.then(Matrix::translate(
                    tile_rect.left - src_x0,
                    tile_rect.bottom - src_y0,
                ));

                let mut xobjects_dict = Dictionary::new();
                xobjects_dict.set("Fm1", Object::Reference(form.id));

                let mut content_buf = Vec::new();
                append_clipped_xobject(&mut content_buf, tile_rect, "Fm1", tile_matrix);
                if params.crop_marks {
                    append_crop_marks(&mut content_buf, tile_rect);
                }

                insert_sheet_page(
                    &mut target_doc,
                    tile_page_id,
                    pages_root_id,
                    sheet_box,
                    xobjects_dict,
                    content_buf,
                );
                new_page_ids.push(tile_page_id);
            }
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

/// Appends thin (`0.25 pt` stroke, `9 pt` length) corner crop mark ticks outside `trim_rect`.
fn append_crop_marks(buf: &mut Vec<u8>, trim_rect: PdfRect) {
    let x0 = trim_rect.left;
    let y0 = trim_rect.bottom;
    let x1 = trim_rect.right;
    let y1 = trim_rect.top;
    let g = CROP_MARK_GAP_PT;
    let l = CROP_MARK_LEN_PT;

    let lines = [
        // Bottom-left
        (x0 - g - l, y0, x0 - g, y0),
        (x0, y0 - g - l, x0, y0 - g),
        // Bottom-right
        (x1 + g, y0, x1 + g + l, y0),
        (x1, y0 - g - l, x1, y0 - g),
        // Top-left
        (x0 - g - l, y1, x0 - g, y1),
        (x0, y1 + g, x0, y1 + g + l),
        // Top-right
        (x1 + g, y1, x1 + g + l, y1),
        (x1, y1 + g, x1, y1 + g + l),
    ];

    buf.extend_from_slice(b"q 0.25 w 0 0 0 RG\n");
    for (ax, ay, bx, by) in lines {
        let seg = format!(
            "{} {} m {} {} l\n",
            fmt_f64(ax),
            fmt_f64(ay),
            fmt_f64(bx),
            fmt_f64(by),
        );
        buf.extend_from_slice(seg.as_bytes());
    }
    buf.extend_from_slice(b"S Q\n");
}
