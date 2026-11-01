//! Implementation of the `pdf.split` tool.

use std::collections::{BTreeMap, BTreeSet};

use ariad_core::{
    reader::pdf::ReadError,
    tools::{
        page_range::DuplicatePolicy,
        params::split::{SplitMode, SplitParams},
    },
};

use crate::{
    document::{DecodeBudget, PdfDoc, ToolCtx},
    error::PdfError,
    info, naming,
    tools::{
        OutputDoc, OutputPayload,
        page_set::{
            OutlineNode, PageSetOptions, build_page_set_output, doc_page_count,
            expand_page_range_capped, output_page_cap, parse_outline_tree,
        },
    },
    writer,
};

const SPLIT_PART_OPTIONS: PageSetOptions = PageSetOptions {
    include_attachments: false,
};

/// Serialises finished parts as they are produced and charges each part's byte size against the
/// document's decode budget, so parts that each deep-copy shared fonts or images fail cleanly
/// instead of accumulating in memory.
struct PartSink {
    budget: DecodeBudget,
    parts: Vec<(Vec<u8>, Vec<String>)>,
}

impl PartSink {
    fn new(source: &PdfDoc) -> Self {
        Self {
            budget: source.decode_budget(),
            parts: Vec::new(),
        }
    }

    fn push(&mut self, part: OutputDoc, ctx: &ToolCtx) -> Result<(), PdfError> {
        let (bytes, warnings) = serialize_part_output(part, ctx)?;
        self.push_bytes_with_warnings(bytes, warnings)
    }

    fn push_bytes_with_warnings(
        &mut self,
        bytes: Vec<u8>,
        warnings: Vec<String>,
    ) -> Result<(), PdfError> {
        self.budget.charge(bytes.len())?;
        self.parts.push((bytes, warnings));
        Ok(())
    }

    fn into_outputs(self, stem: &str) -> Vec<OutputDoc> {
        let total_parts = self.parts.len();
        self.parts
            .into_iter()
            .enumerate()
            .map(|(idx, (bytes, warnings))| {
                let name = naming::part_output_name(stem, idx + 1, total_parts);
                let mut out = OutputDoc::from_bytes(name, "pdf", bytes);
                out.warnings = warnings;
                out
            })
            .collect()
    }
}

/// Fails when `total` pages across all parts exceed `min(MAX_OUTPUT_PAGES, max_pages)`.
fn ensure_total_pages_within_cap(total: u64, max_pages: Option<u32>) -> Result<(), PdfError> {
    let cap = output_page_cap(max_pages);
    if total > cap {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: total,
            limit: u32::try_from(cap).unwrap_or(u32::MAX),
        }));
    }
    Ok(())
}

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
            let mut total_pages = 0_u64;
            for range in ranges {
                ctx.check()?;
                let expanded = expand_page_range_capped(
                    range,
                    page_count,
                    DuplicatePolicy::KeepInOrder,
                    ctx.max_pages,
                )?;
                if expanded.is_empty() {
                    return Err(PdfError::InvalidParams(
                        "a split range expanded to 0 pages".to_owned(),
                    ));
                }
                total_pages = total_pages.saturating_add(expanded.len() as u64);
                ensure_total_pages_within_cap(total_pages, ctx.max_pages)?;
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
            ensure_total_pages_within_cap(u64::from(page_count), ctx.max_pages)?;
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
            compute_bookmark_groups(source, page_count, level, ctx)?
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

    ensure_total_pages_within_cap(
        groups.iter().map(|group| group.len() as u64).sum(),
        ctx.max_pages,
    )?;

    let mut sink = PartSink::new(source);
    for group in groups {
        ctx.check()?;
        let part_doc = build_page_set_output(source, &group, SPLIT_PART_OPTIONS, ctx)?;
        sink.push(part_doc, ctx)?;
    }

    Ok(sink.into_outputs(&stem))
}

fn compute_bookmark_groups(
    source: &PdfDoc,
    page_count: u32,
    max_level: u8,
    ctx: &ToolCtx,
) -> Result<Vec<Vec<u32>>, PdfError> {
    let src_doc = source.inner();
    let page_map = src_doc.get_pages();
    let mut id_to_page_num = BTreeMap::new();
    for (&num, &id) in &page_map {
        id_to_page_num.insert(id, num);
    }

    let outlines = parse_outline_tree(src_doc, &id_to_page_num, ctx)?;
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

fn serialize_part_output(
    part: OutputDoc,
    ctx: &ToolCtx,
) -> Result<(Vec<u8>, Vec<String>), PdfError> {
    let bytes = match part.payload {
        OutputPayload::Pdf(mut doc) => {
            writer::save_with_options(&mut doc, ctx, &part.save_options)?
        }
        OutputPayload::Bytes(raw) => raw,
    };
    Ok((bytes, part.warnings))
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
    ensure_total_pages_within_cap(u64::from(page_count), ctx.max_pages)?;
    let mut sink = PartSink::new(source);
    let mut start_page = 1_u32;

    while start_page <= page_count {
        ctx.check()?;
        let single_range = [start_page];
        let best_doc = build_page_set_output(source, &single_range, SPLIT_PART_OPTIONS, ctx)?;
        let (single_serialized, single_warnings) = serialize_part_output(best_doc, ctx)?;
        let single_bytes = single_serialized.len() as u64;

        if single_bytes >= max_bytes || start_page == page_count {
            sink.push_bytes_with_warnings(single_serialized, single_warnings)?;
            start_page += 1;
            continue;
        }

        let mut accepted_bytes = single_serialized;
        let mut accepted_warnings = single_warnings;
        let mut last_ok_end = start_page;
        let mut step = 1_u32;
        let mut first_over_end: Option<u32> = None;

        while last_ok_end < page_count {
            ctx.check()?;
            let candidate_end = last_ok_end.saturating_add(step).min(page_count);
            let candidate_pages: Vec<u32> = (start_page..=candidate_end).collect();
            let candidate_doc =
                build_page_set_output(source, &candidate_pages, SPLIT_PART_OPTIONS, ctx)?;
            let (candidate_serialized, candidate_warnings) =
                serialize_part_output(candidate_doc, ctx)?;
            let candidate_size = candidate_serialized.len() as u64;

            if candidate_size <= max_bytes {
                last_ok_end = candidate_end;
                accepted_bytes = candidate_serialized;
                accepted_warnings = candidate_warnings;
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
                let candidate_doc =
                    build_page_set_output(source, &candidate_pages, SPLIT_PART_OPTIONS, ctx)?;
                let (candidate_serialized, candidate_warnings) =
                    serialize_part_output(candidate_doc, ctx)?;
                let candidate_size = candidate_serialized.len() as u64;
                if candidate_size <= max_bytes {
                    lo = mid;
                    accepted_bytes = candidate_serialized;
                    accepted_warnings = candidate_warnings;
                } else {
                    hi = mid;
                }
            }
            last_ok_end = lo;
        }

        sink.push_bytes_with_warnings(accepted_bytes, accepted_warnings)?;
        start_page = last_ok_end + 1;
    }

    Ok(sink.into_outputs(stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_honours_smaller_max_pages_and_clamps_larger_ones() {
        assert_eq!(output_page_cap(None), 10_000);
        assert_eq!(output_page_cap(Some(5_000)), 5_000);
        assert_eq!(output_page_cap(Some(50_000)), 10_000);

        assert!(ensure_total_pages_within_cap(5_000, Some(5_000)).is_ok());
        assert_eq!(
            ensure_total_pages_within_cap(5_001, Some(5_000)).unwrap_err(),
            PdfError::LimitExceeded(ReadError::TooManyPages {
                actual: 5_001,
                limit: 5_000,
            })
        );
    }
}
