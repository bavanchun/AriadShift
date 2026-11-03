//! Create linked contents pages from the document outline.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        self,
        font::FontPlan,
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        text::{self, TextAlign, TextLayout, TextStyle},
    },
    tools::{OutputDoc, furniture_common, insert_blank, outline, reorder},
    xobject::PdfRect,
};
use ariad_core::{
    limits::Limits,
    tools::params::{Opacity, TocPageParams, TocPageSize, outline::OutlineNode},
};
use lopdf::{Object, ObjectId, Stream, dictionary};

const MAX_TOC_PAGES: usize = 50;
const MAX_ENTRIES: usize = 100_000;
const MARGIN: f64 = 36.0;
struct Entry {
    title: String,
    page: u32,
    depth: u8,
}
struct Row {
    target: ObjectId,
    layout: TextLayout,
    x: f64,
    y: f64,
    width: f64,
}
struct Layout {
    title: TextLayout,
    pages: Vec<Vec<Row>>,
}
fn flatten(nodes: &[OutlineNode], max_depth: u8, ctx: &ToolCtx) -> Result<Vec<Entry>, PdfError> {
    let mut result = Vec::new();
    let mut pending: Vec<_> = nodes.iter().rev().map(|node| (node, 1u8)).collect();
    while let Some((node, depth)) = pending.pop() {
        ctx.check()?;
        if result.len() >= MAX_ENTRIES {
            return Err(PdfError::InvalidParams("too many outline entries".into()));
        }
        if node.title.chars().count() > 4096 {
            return Err(PdfError::InvalidParams("outline title is too long".into()));
        }
        result.push(Entry {
            title: node.title.clone(),
            page: node.page,
            depth,
        });
        if depth < max_depth {
            for child in node.children.iter().rev() {
                pending.push((child, depth + 1));
            }
        }
    }
    Ok(result)
}
fn row_text(
    plan: &mut FontPlan,
    title: &str,
    label: &str,
    width: f64,
    size: f64,
    dots: bool,
) -> Result<String, PdfError> {
    let title = plan.add_text(title)?;
    let label = plan.add_text(label)?;
    let label_width = plan.measure_line(&label, size)?;
    let separator = if dots { '.' } else { ' ' };
    plan.add_text(&separator.to_string())?;
    let separator_width = plan.measure_line(&separator.to_string(), size)?;
    let available = width - label_width - separator_width;
    if available <= 0.0 {
        return Err(PdfError::InvalidParams(
            "contents page is too narrow for the page label".into(),
        ));
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0.0;
    for ch in title.chars() {
        if ch == '\n' {
            lines.push((std::mem::take(&mut line), line_width));
            line_width = 0.0;
            continue;
        }
        let mut encoded = [0; 4];
        let ch_width = plan.measure_line(ch.encode_utf8(&mut encoded), size)?;
        if ch_width > available {
            return Err(PdfError::InvalidParams(
                "contents page is too narrow for an outline character".into(),
            ));
        }
        if line_width + ch_width > available && !line.is_empty() {
            lines.push((std::mem::take(&mut line), line_width));
            line_width = 0.0;
        }
        line.push(ch);
        line_width += ch_width;
    }
    let remaining = (width - line_width - label_width).max(separator_width);
    let spare =
        4096usize.saturating_sub(title.chars().count() + label.chars().count() + lines.len() + 1);
    let repeats = ((remaining / separator_width).floor() as usize)
        .clamp(1, 256)
        .min(spare.max(1));
    line.extend(std::iter::repeat_n(separator, repeats));
    line.push_str(&label);
    lines.push((line, 0.0));
    let value = lines
        .into_iter()
        .map(|(line, _)| line)
        .collect::<Vec<_>>()
        .join("\n");
    if value.chars().count() > 4096 {
        return Err(PdfError::InvalidParams(
            "contents entry exceeds the text limit".into(),
        ));
    }
    Ok(value)
}
#[allow(clippy::too_many_arguments)]
fn layout_entries(
    entries: &[Entry],
    original_pages: &[ObjectId],
    rules: &std::collections::BTreeMap<u32, crate::tools::page_set::PageLabelRule>,
    guess: usize,
    width: f64,
    height: f64,
    params: &TocPageParams,
    plan: &mut FontPlan,
    ctx: &ToolCtx,
) -> Result<Layout, PdfError> {
    let usable = width - 2.0 * MARGIN;
    if usable <= 0.0 {
        return Err(PdfError::InvalidParams(
            "contents page is too narrow".into(),
        ));
    }
    let title = TextLayout::prepare(
        plan,
        &params.title,
        params.font_size_pt * 1.3,
        Some(usable),
        1.2,
    )?;
    let top = height - MARGIN - title.height - params.font_size_pt;
    if top <= MARGIN {
        return Err(PdfError::InvalidParams("contents page is too short".into()));
    }
    let mut pages = vec![Vec::new()];
    let mut cursor = top;
    for entry in entries {
        ctx.check()?;
        let target = *original_pages
            .get((entry.page - 1) as usize)
            .ok_or_else(|| {
                PdfError::InvalidParams(
                    "outline destination does not identify an existing page".into(),
                )
            })?;
        let label = if rules.range(..entry.page).next_back().is_some() {
            furniture_common::label(rules, entry.page)?
        } else {
            entry
                .page
                .checked_add(guess as u32)
                .ok_or(PdfError::Malformed)?
                .to_string()
        };
        let indent = f64::from(entry.depth.saturating_sub(1)) * params.font_size_pt * 1.5;
        let width = usable - indent;
        if width <= 0.0 {
            return Err(PdfError::InvalidParams(
                "contents indentation exceeds page width".into(),
            ));
        }
        let text = row_text(
            plan,
            &entry.title,
            &label,
            width,
            params.font_size_pt,
            params.dot_leaders,
        )?;
        let layout = TextLayout::prepare(plan, &text, params.font_size_pt, Some(width), 1.2)?;
        if layout.height > top - MARGIN {
            return Err(PdfError::InvalidParams(
                "an outline entry does not fit on a contents page".into(),
            ));
        }
        if cursor - layout.height < MARGIN {
            if pages.len() >= MAX_TOC_PAGES {
                return Err(PdfError::InvalidParams("contents exceeds 50 pages".into()));
            }
            pages.push(Vec::new());
            cursor = top;
        }
        cursor -= layout.height;
        pages.last_mut().ok_or(PdfError::Malformed)?.push(Row {
            target,
            layout,
            x: MARGIN + indent,
            y: cursor,
            width,
        });
        cursor -= params.font_size_pt * 0.2;
    }
    Ok(Layout { title, pages })
}
/// Insert linked table-of-contents pages before the existing pages.
pub fn run(
    inputs: &[PdfDoc],
    params: &TocPageParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.toc_page requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let mut pdf = inputs[0].clone();
    let originals = crate::color::page_ids(pdf.inner(), limits, ctx)?;
    draw::check_page_count(originals.len(), limits, ctx)?;
    let exported = outline::export_outlines_strict(pdf.inner(), &originals, ctx)?;
    let entries = flatten(&exported.nodes, params.max_depth, ctx)?;
    if entries.is_empty() {
        return Err(PdfError::InvalidParams(
            "document has no outline entries".into(),
        ));
    }
    let rules = furniture_common::labels(pdf.inner(), ctx)?;
    let (width, height) = match params.page_size {
        TocPageSize::Same(_) => {
            let geometry = PageGeometry::from_page(pdf.inner(), originals[0])?;
            (geometry.width_pt, geometry.height_pt)
        }
        TocPageSize::Paper(size) => size.dimensions_pt(),
    };
    if width <= 2.0 * MARGIN
        || height <= 2.0 * MARGIN
        || width > 1_000_000.0
        || height > 1_000_000.0
    {
        return Err(PdfError::InvalidParams(
            "invalid contents page dimensions".into(),
        ));
    }
    let mut plan = FontPlan::new(params.font);
    let mut guess = 1usize;
    let mut final_layout = None;
    for _ in 0..=MAX_TOC_PAGES {
        let layout = layout_entries(
            &entries, &originals, &rules, guess, width, height, params, &mut plan, ctx,
        )?;
        let count = layout.pages.len();
        if count == guess {
            final_layout = Some(layout);
            break;
        }
        guess = count;
    }
    let layout = final_layout
        .ok_or_else(|| PdfError::InvalidParams("contents page count did not converge".into()))?;
    draw::check_page_count(originals.len() + layout.pages.len(), limits, ctx)?;
    let inner = pdf.inner_mut()?;
    let root = reorder::pages_root_id(inner)?;
    let root_dict = inner
        .get_dictionary(root)
        .map_err(|_| PdfError::Malformed)?;
    let kids = root_dict
        .get(b"Kids")
        .ok()
        .and_then(|v| crate::color::resolve(inner, v, limits.max_nesting_depth))
        .and_then(|v| v.as_array().ok())
        .cloned()
        .ok_or(PdfError::Malformed)?;
    if kids.iter().any(|item| item.as_reference().is_err()) {
        return Err(PdfError::Malformed);
    }
    let mut inserted = Vec::with_capacity(layout.pages.len());
    for _ in &layout.pages {
        ctx.check()?;
        let stream = inner.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page=inner.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),Object::Real(width as f32),Object::Real(height as f32)],"Resources"=>dictionary!{},"Contents"=>stream});
        inserted.push(page);
    }
    let mut new_kids: Vec<Object> = inserted.iter().copied().map(Object::Reference).collect();
    new_kids.extend(kids);
    let root_dict = inner
        .get_dictionary_mut(root)
        .map_err(|_| PdfError::Malformed)?;
    root_dict.set("Kids", new_kids);
    root_dict.set("Count", (originals.len() + inserted.len()) as i64);
    let mapping: Vec<usize> = (0..originals.len())
        .map(|index| index + inserted.len())
        .collect();
    insert_blank::shift_page_labels(inner, &mapping, ctx)?;
    let font = plan.embed(inner)?;
    let opacity: Opacity =
        serde_json::from_value(serde_json::json!(1.0)).map_err(|_| PdfError::Malformed)?;
    let color =
        serde_json::from_value(serde_json::json!("#000000")).map_err(|_| PdfError::Malformed)?;
    let style = TextStyle {
        size_pt: params.font_size_pt,
        color,
        opacity,
        stroke: None,
        align: TextAlign::Left,
        rotation_deg: 0.0,
    };
    for (page, rows) in inserted.into_iter().zip(layout.pages) {
        ctx.check()?;
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        let mut heading_style = style.clone();
        heading_style.size_pt = params.font_size_pt * 1.3;
        text::draw(
            &mut pdf,
            page,
            &geometry,
            &font,
            &layout.title,
            &heading_style,
            MARGIN,
            height - MARGIN - layout.title.height,
            Marking::Content,
            LayerPosition::Over,
            ctx,
        )?;
        let mut annotations = Vec::new();
        for row in rows {
            ctx.check()?;
            text::draw(
                &mut pdf,
                page,
                &geometry,
                &font,
                &row.layout,
                &style,
                row.x,
                row.y,
                Marking::Content,
                LayerPosition::Over,
                ctx,
            )?;
            if params.links {
                let rect = geometry.visual_to_user.transform_rect_bbox(PdfRect::new(
                    row.x,
                    row.y,
                    row.x + row.width,
                    row.y + row.layout.height,
                ));
                let annotation=pdf.inner_mut()?.add_object(dictionary!{"Type"=>"Annot","Subtype"=>"Link","Rect"=>rect.to_pdf_array(),"Border"=>vec![0.into(),0.into(),0.into()],"Dest"=>vec![Object::Reference(row.target),Object::Name(b"Fit".to_vec())],"P"=>page});
                annotations.push(Object::Reference(annotation));
            }
        }
        if !annotations.is_empty() {
            pdf.inner_mut()?
                .get_dictionary_mut(page)
                .map_err(|_| PdfError::Malformed)?
                .set("Annots", annotations);
        }
    }
    Ok(vec![OutputDoc::unnamed_pdf(pdf)])
}
