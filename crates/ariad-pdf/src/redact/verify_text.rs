//! Independent core-reader text check. Other carriers require separate checks.

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    tools::params::{
        redact::{RedactParams, RedactionTarget},
        redact_internal::RedactionCheckStatus,
    },
    warning::WarningCode,
};

use crate::{PdfError, ToolCtx};

use super::matching::{MatchBudget, Matchers};

const MAX_TEXT: usize = 16 * 1024 * 1024;

pub(super) fn core_text(
    bytes: &[u8],
    params: &RedactParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<RedactionCheckStatus, PdfError> {
    use RedactionCheckStatus::{Fail, Pass, Unavailable};
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    if !params.scope.text {
        return Ok(Unavailable);
    }
    let has_text_targets = params.targets.iter().any(|target| {
        matches!(
            target,
            RedactionTarget::Exact { .. }
                | RedactionTarget::Regex { .. }
                | RedactionTarget::Preset { .. }
        )
    });
    ctx.check()?;
    let read = ariad_core::reader::pdf::read(bytes, limits).map_err(PdfError::from)?;
    ctx.check()?;
    if read.warnings.iter().any(|warning| {
        matches!(
            warning.code,
            WarningCode::TextUnmapped | WarningCode::UnsupportedNode
        )
    }) {
        return Ok(Unavailable);
    }
    for target in &params.targets {
        match target {
            RedactionTarget::Page { page } => {
                if let Some(ref layout) = read.document.layout
                    && layout.blocks.values().any(|b| b.page_no == *page)
                {
                    return Ok(Fail);
                }
            }
            RedactionTarget::Rectangle { page, rect } => {
                if let Some(ref layout) = read.document.layout
                    && let Some(page_geom) = layout.pages.iter().find(|p| p.page_no == *page)
                {
                    let h = page_geom.height;
                    let t_left = rect.left;
                    let t_right = rect.right;
                    let t_top = (h - rect.top).max(0.0);
                    let t_bottom = (h - rect.bottom).max(0.0);
                    for block in layout.blocks.values() {
                        if block.page_no == *page {
                            let b = block.bbox;
                            if b.left < t_right
                                && b.right > t_left
                                && b.top < t_bottom
                                && b.bottom > t_top
                                && params.overlay_text.is_none()
                            {
                                return Ok(Fail);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !has_text_targets {
        return Ok(Pass);
    }
    let mut text = String::new();
    let mut todo = Vec::new();
    for block in read.document.furniture.iter().rev() {
        todo.push(Item::Block(block));
    }
    for block in read.document.body.iter().rev() {
        todo.push(Item::Block(block));
    }
    let mut work = 0usize;
    while let Some(item) = todo.pop() {
        work = work.checked_add(1).ok_or(PdfError::Malformed)?;
        if work > (limits.max_blocks as usize).saturating_mul(16) {
            return Err(PdfError::Malformed);
        }
        if work.is_multiple_of(128) {
            ctx.check()?;
        }
        match item {
            Item::Separator => append(&mut text, "\n")?,
            Item::Inline(inline) => match inline {
                Inline::Text { text: value }
                | Inline::Code { text: value }
                | Inline::Math { tex: value, .. }
                | Inline::Raw { text: value, .. } => append(&mut text, value)?,
                Inline::Styled { content, .. }
                | Inline::Emph { content }
                | Inline::Strong { content }
                | Inline::Strikeout { content }
                | Inline::Superscript { content }
                | Inline::Subscript { content }
                | Inline::Link { content, .. } => push_inlines(&mut todo, content),
                Inline::Image { alt, .. } => append(&mut text, alt)?,
                Inline::SoftBreak {} | Inline::LineBreak {} => append(&mut text, "\n")?,
                Inline::FootnoteRef { .. } => {}
            },
            Item::Block(block) => {
                todo.push(Item::Separator);
                match block {
                    Block::Heading { content, .. } | Block::Paragraph { content, .. } => {
                        push_inlines(&mut todo, content)
                    }
                    Block::Code { text: value, .. }
                    | Block::Raw { text: value, .. }
                    | Block::Math { tex: value, .. } => append(&mut text, value)?,
                    Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
                        for block in blocks.iter().rev() {
                            todo.push(Item::Block(block));
                        }
                    }
                    Block::List { items, .. } => {
                        for item in items.iter().rev() {
                            for block in item.blocks.iter().rev() {
                                todo.push(Item::Block(block));
                            }
                        }
                    }
                    Block::Table {
                        caption,
                        head,
                        body,
                        footnotes,
                        ..
                    } => {
                        push_inlines(&mut todo, footnotes);
                        for row in head.iter().chain(body).rev() {
                            for cell in row.iter().rev() {
                                for block in cell.blocks.iter().rev() {
                                    todo.push(Item::Block(block));
                                }
                            }
                        }
                        if let Some(caption) = caption {
                            push_inlines(&mut todo, caption);
                        }
                    }
                    Block::Figure { caption, .. } => push_inlines(&mut todo, caption),
                    Block::PageBreak { .. } => {}
                }
            }
        }
    }
    let matcher = Matchers::new(&params.targets).map_err(|e| PdfError::InvalidParams(e.into()))?;
    let found = matcher.find(&text, &mut MatchBudget::with_context(ctx));
    ctx.check()?;
    let found = found.map_err(|e| PdfError::InvalidParams(e.into()))?;
    Ok(if found.is_empty() { Pass } else { Fail })
}

enum Item<'a> {
    Block(&'a Block),
    Inline(&'a Inline),
    Separator,
}

fn push_inlines<'a>(todo: &mut Vec<Item<'a>>, values: &'a [Inline]) {
    for value in values.iter().rev() {
        todo.push(Item::Inline(value));
    }
}

fn append(text: &mut String, value: &str) -> Result<(), PdfError> {
    if value.len() > MAX_TEXT.saturating_sub(text.len()) {
        return Err(PdfError::InvalidParams(
            "searchable text limit exceeded".into(),
        ));
    }
    text.push_str(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::tools::params::redact::RedactionScope;

    #[test]
    fn cosmetic_colour_loss_keeps_text_verification_effective() {
        use lopdf::{Object, Stream, dictionary};
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages = pdf.new_object_id();
        let font = pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let content = pdf.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf /CS1 cs 50 0 0 scn 40 100 Td (visible marker) Tj ET".to_vec(),
        ));
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "ColorSpace" => dictionary! { "CS1" => vec![
                    Object::Name(b"Lab".to_vec()),
                    Object::Dictionary(dictionary! { "WhitePoint" => vec![0.9642.into(), 1.into(), 0.8249.into()] }),
                ] },
            },
            "Contents" => content,
        });
        pdf.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
            }),
        );
        let root = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        pdf.trailer.set("Root", root);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        let limits = Limits::local();
        let read = ariad_core::reader::pdf::read(&bytes, &limits).unwrap();
        assert!(
            read.warnings
                .iter()
                .any(|warning| warning.code == WarningCode::StylesDropped)
        );
        assert!(!read.warnings.iter().any(|warning| matches!(
            warning.code,
            WarningCode::TextUnmapped | WarningCode::UnsupportedNode
        )));
        for (target, expected) in [
            ("absent", RedactionCheckStatus::Pass),
            ("marker", RedactionCheckStatus::Fail),
        ] {
            let params = serde_json::from_value(serde_json::json!({
                "targets": [{ "kind": "exact", "text": target }],
            }))
            .unwrap();
            assert_eq!(
                core_text(&bytes, &params, &limits, &ToolCtx::new()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn geometry_only_targets_report_unavailable_when_text_scope_excluded() {
        let request = RedactParams {
            targets: vec![RedactionTarget::Page { page: 1 }],
            color: None,
            overlay_text: None,
            scope: RedactionScope {
                text: false,
                images: true,
                vectors: true,
                annotations: true,
                metadata: true,
                attachments: true,
            },
            invalidate_signatures: false,
        };
        assert_eq!(
            core_text(b"", &request, &Limits::local(), &ToolCtx::new()).unwrap(),
            RedactionCheckStatus::Unavailable,
        );
    }
}
