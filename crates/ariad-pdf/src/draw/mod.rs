//! Drawing primitives for adding marks to existing PDF pages.

pub mod font;
pub mod geometry;
pub mod image;
pub mod layer;
pub mod path;
pub mod text;
pub mod tokens;

/// Maximum number of pages receiving marks in one operation.
pub const MAX_DRAW_PAGES: u32 = 10_000;

/// Enforce both the drawing ceiling and the caller's page limits.
pub fn check_page_count(
    count: usize,
    limits: &ariad_core::limits::Limits,
    ctx: &crate::ToolCtx,
) -> Result<(), crate::PdfError> {
    let limit = limits
        .max_pages
        .into_iter()
        .chain(ctx.max_pages)
        .fold(MAX_DRAW_PAGES, u32::min);
    if count as u64 > u64::from(limit) {
        return Err(crate::PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::TooManyPages {
                actual: count as u64,
                limit,
            },
        ));
    }
    ctx.check()
}
