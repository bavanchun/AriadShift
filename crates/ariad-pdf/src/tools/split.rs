//! Implementation of the `pdf.split` tool.

use std::collections::{BTreeMap, BTreeSet};

use ariad_core::tools::{
    page_range::DuplicatePolicy,
    params::split::{SplitMode, SplitParams},
};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    info, naming,
    tools::{
        OutputDoc,
        extract_pages::{OutlineNode, build_extracted_doc, doc_page_count, parse_outline_tree},
    },
    writer,
};

/// Splits a PDF document into multiple part PDFs.
pub fn run(
    inputs: &[PdfDoc],
    params: &SplitParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.split requires exactly 1 input document (got {})",
            inputs.len()
        )));
    }

    let source = &inputs[0];
    let page_count = doc_page_count(source);
    if page_count == 0 {
        return Err(PdfError::InvalidParams(
            "cannot split a 0-page PDF document".to_owned(),
        ));
    }

    let stem = naming::doc_stem(source).unwrap_or_else(|| "document".to_owned());

    if params.mode == SplitMode::Size {
        let max_bytes = params.max_bytes.ok_or_else(|| {
            PdfError::InvalidParams("size split mode requires 'max_bytes' >= 1".to_owned())
        })?;
        if max_bytes == 0 {
            return Err(PdfError::InvalidParams(
                "size split mode requires 'max_bytes' >= 1".to_owned(),
            ));
        }
        return split_by_size(source, &stem, page_count, max_bytes, ctx);
    }

    let groups: Vec<Vec<u32>> = match params.mode {
        SplitMode::Ranges => {
            let ranges = params.ranges.as_ref().ok_or_else(|| {
                PdfError::InvalidParams(
                    "ranges split mode requires a non-empty 'ranges' list".to_owned(),
                )
            })?;
            if ranges.is_empty() {
                return Err(PdfError::InvalidParams(
                    "ranges split mode requires a non-empty 'ranges' list".to_owned(),
                ));
            }
            let mut out = Vec::with_capacity(ranges.len());
            for range in ranges {
                let expanded = range
                    .expand(page_count, DuplicatePolicy::KeepInOrder)
                    .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
                if expanded.is_empty() {
                    return Err(PdfError::InvalidParams(
                        "a split range expanded to 0 pages".to_owned(),
                    ));
                }
                out.push(expanded);
            }
            out
        }
        SplitMode::Every => {
            let every = params.every.ok_or_else(|| {
                PdfError::InvalidParams("every split mode requires 'every' >= 1".to_owned())
            })?;
            if every == 0 {
                return Err(PdfError::InvalidParams(
                    "every split mode requires 'every' >= 1".to_owned(),
                ));
            }
            let all_pages: Vec<u32> = (1..=page_count).collect();
            all_pages
                .chunks(every as usize)
                .map(<[u32]>::to_vec)
                .collect()
        }
        SplitMode::Bookmarks => {
            let level = params.level.unwrap_or(1);
            if level == 0 {
                return Err(PdfError::InvalidParams(
                    "bookmarks split mode requires 'level' >= 1".to_owned(),
                ));
            }
            compute_bookmark_groups(source, page_count, level)?
        }
        SplitMode::BlankSeparator => {
            let threshold = params.blank_threshold.unwrap_or(0.995);
            if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
                return Err(PdfError::InvalidParams(format!(
                    "blank_threshold must be within 0.0..=1.0 (got {threshold})"
                )));
            }
            compute_blank_separator_groups(source, ctx)?
        }
        SplitMode::Size => unreachable!("handled above"),
    };

    let total_parts = groups.len();
    let mut outputs = Vec::with_capacity(total_parts);
    for (idx, group) in groups.into_iter().enumerate() {
        ctx.check()?;
        let part_doc = build_extracted_doc(source, &group, ctx)?;
        let part_name = naming::part_output_name(&stem, idx + 1, total_parts);
        outputs.push(OutputDoc::from_pdf(part_name, part_doc));
    }

    Ok(outputs)
}

fn compute_bookmark_groups(
    source: &PdfDoc,
    page_count: u32,
    max_level: u8,
) -> Result<Vec<Vec<u32>>, PdfError> {
    let src_doc = source.inner();
    let page_map = src_doc.get_pages();
    let mut id_to_page_num = BTreeMap::new();
    for (&num, &id) in &page_map {
        id_to_page_num.insert(id, num);
    }

    let outlines = parse_outline_tree(src_doc, &id_to_page_num);
    if outlines.is_empty() {
        return Err(PdfError::InvalidParams(
            "cannot split by bookmarks: document has no outlines".to_owned(),
        ));
    }

    let mut start_pages = BTreeSet::new();
    collect_bookmark_start_pages(&outlines, 1, max_level, page_count, &mut start_pages);

    if start_pages.is_empty() {
        return Err(PdfError::InvalidParams(format!(
            "cannot split by bookmarks: no valid page destinations up to level {max_level}"
        )));
    }

    let starts: Vec<u32> = start_pages.into_iter().collect();
    let mut groups = Vec::new();
    if starts[0] > 1 {
        groups.push((1..starts[0]).collect());
    }
    for (i, &start) in starts.iter().enumerate() {
        let end_exclusive = starts.get(i + 1).copied().unwrap_or(page_count + 1);
        if start < end_exclusive {
            groups.push((start..end_exclusive).collect());
        }
    }
    Ok(groups)
}

fn collect_bookmark_start_pages(
    nodes: &[OutlineNode],
    current_level: u8,
    max_level: u8,
    page_count: u32,
    out: &mut BTreeSet<u32>,
) {
    if current_level > max_level {
        return;
    }
    for node in nodes {
        if let Some(p) = node.target_page_1_based
            && (1..=page_count).contains(&p)
        {
            out.insert(p);
        }
        if current_level < max_level && !node.children.is_empty() {
            collect_bookmark_start_pages(
                &node.children,
                current_level + 1,
                max_level,
                page_count,
                out,
            );
        }
    }
}

/// Detects blank separator pages using the deterministic in-process content-empty criterion from
/// `info::from_doc_with_ctx` (`page.blank == true`: no text operators with non-whitespace glyphs,
/// no XObject `/Do`, and no path-painting operators; pure `lopdf` has no rasterizer).
/// Splits the document into contiguous runs of non-blank pages, dropping the blank separator pages.
fn compute_blank_separator_groups(
    source: &PdfDoc,
    ctx: &ToolCtx,
) -> Result<Vec<Vec<u32>>, PdfError> {
    let report = info::from_doc_with_ctx(source, ctx)?;
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut current_run: Vec<u32> = Vec::new();

    for page in &report.pages {
        if page.blank {
            if !current_run.is_empty() {
                groups.push(std::mem::take(&mut current_run));
            }
        } else {
            current_run.push(page.page);
        }
    }
    if !current_run.is_empty() {
        groups.push(current_run);
    }

    if groups.is_empty() {
        return Err(PdfError::InvalidParams(
            "cannot split by blank separators: all pages in document are blank".to_owned(),
        ));
    }

    Ok(groups)
}

/// Greedily accumulates pages into parts whose serialized byte size (`writer::save`) is
/// at most `max_bytes`. A single page that exceeds `max_bytes` on its own is emitted as a 1-page
/// part so forward progress is always guaranteed.
fn split_by_size(
    source: &PdfDoc,
    stem: &str,
    page_count: u32,
    max_bytes: u64,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let mut part_docs: Vec<PdfDoc> = Vec::new();
    let mut start_page = 1_u32;

    while start_page <= page_count {
        ctx.check()?;
        let single_range = [start_page];
        let best_doc = build_extracted_doc(source, &single_range, ctx)?;
        let mut measure_clone = best_doc.clone();
        let single_bytes = writer::save(&mut measure_clone, ctx)?.len() as u64;

        if single_bytes >= max_bytes || start_page == page_count {
            part_docs.push(best_doc);
            start_page += 1;
            continue;
        }

        let mut accepted_doc = best_doc;
        let mut last_ok_end = start_page;
        let mut step = 1_u32;
        let mut first_over_end: Option<u32> = None;

        while last_ok_end < page_count {
            ctx.check()?;
            let candidate_end = last_ok_end.saturating_add(step).min(page_count);
            let candidate_pages: Vec<u32> = (start_page..=candidate_end).collect();
            let candidate_doc = build_extracted_doc(source, &candidate_pages, ctx)?;
            let mut candidate_clone = candidate_doc.clone();
            let candidate_size = writer::save(&mut candidate_clone, ctx)?.len() as u64;

            if candidate_size <= max_bytes {
                last_ok_end = candidate_end;
                accepted_doc = candidate_doc;
                if candidate_end == page_count {
                    break;
                }
                step = step.saturating_mul(2);
            } else {
                first_over_end = Some(candidate_end);
                break;
            }
        }

        if let Some(mut hi) = first_over_end {
            let mut lo = last_ok_end;
            while lo + 1 < hi {
                ctx.check()?;
                let mid = lo + (hi - lo) / 2;
                let candidate_pages: Vec<u32> = (start_page..=mid).collect();
                let candidate_doc = build_extracted_doc(source, &candidate_pages, ctx)?;
                let mut candidate_clone = candidate_doc.clone();
                let candidate_size = writer::save(&mut candidate_clone, ctx)?.len() as u64;
                if candidate_size <= max_bytes {
                    lo = mid;
                    accepted_doc = candidate_doc;
                } else {
                    hi = mid;
                }
            }
            last_ok_end = lo;
        }

        part_docs.push(accepted_doc);
        start_page = last_ok_end + 1;
    }

    let total_parts = part_docs.len();
    let mut outputs = Vec::with_capacity(total_parts);
    for (idx, doc) in part_docs.into_iter().enumerate() {
        let part_name = naming::part_output_name(stem, idx + 1, total_parts);
        outputs.push(OutputDoc::from_pdf(part_name, doc));
    }
    Ok(outputs)
}
