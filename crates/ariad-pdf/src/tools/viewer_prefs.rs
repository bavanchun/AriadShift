//! Implementation of the `pdf.viewer_prefs` tool.

use ariad_core::tools::params::viewer_prefs::{
    ViewerPageLayout, ViewerPageMode, ViewerPrefsParams,
};
use lopdf::{Dictionary, Object};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::{OutputDoc, metadata::catalog_object_id},
};

/// Sets `/PageLayout`, `/PageMode`, `/OpenAction`, and `/ViewerPreferences` on the PDF catalog.
pub fn run(
    inputs: &[PdfDoc],
    params: &ViewerPrefsParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.viewer_prefs requires exactly 1 input PDF, got {}",
            inputs.len()
        )));
    }

    if params.page_layout.is_none()
        && params.page_mode.is_none()
        && params.hide_toolbar.is_none()
        && params.fit_window.is_none()
        && params.display_doc_title.is_none()
        && params.open_page.is_none()
    {
        return Err(PdfError::InvalidParams(
            "pdf.viewer_prefs requires at least one preference field to be set".to_owned(),
        ));
    }

    let mut pdf = inputs[0].clone();
    let page_count = u32::try_from(pdf.inner().get_pages().len()).unwrap_or(0);
    let stem = naming::doc_stem(&pdf).unwrap_or_else(|| "document".to_owned());

    let open_page_id = if let Some(open_page) = params.open_page {
        if open_page == 0 || open_page > page_count {
            return Err(PdfError::InvalidParams(format!(
                "open_page ({open_page}) is out of bounds for {page_count}-page document"
            )));
        }
        let pages = pdf.inner().get_pages();
        let page_id = pages.get(&open_page).copied().ok_or_else(|| {
            PdfError::InvalidParams(format!("open_page ({open_page}) could not be resolved"))
        })?;
        Some(page_id)
    } else {
        None
    };

    let doc = pdf.inner_mut()?;
    let catalog_id = catalog_object_id(doc)?;

    let has_vp_flags = params.hide_toolbar.is_some()
        || params.fit_window.is_some()
        || params.display_doc_title.is_some();

    let vp_obj_to_set = if has_vp_flags {
        let mut vp_dict = doc
            .get_object(catalog_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|cat| cat.get(b"ViewerPreferences").ok())
            .and_then(|vp| match vp {
                Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok().cloned(),
                Object::Dictionary(d) => Some(d.clone()),
                _ => None,
            })
            .unwrap_or_else(Dictionary::new);

        if let Some(hide_toolbar) = params.hide_toolbar {
            vp_dict.set("HideToolbar", Object::Boolean(hide_toolbar));
        }
        if let Some(fit_window) = params.fit_window {
            vp_dict.set("FitWindow", Object::Boolean(fit_window));
        }
        if let Some(display_doc_title) = params.display_doc_title {
            vp_dict.set("DisplayDocTitle", Object::Boolean(display_doc_title));
        }
        Some(Object::Dictionary(vp_dict))
    } else {
        None
    };

    let catalog = doc
        .get_object_mut(catalog_id)
        .map_err(|_| PdfError::Malformed)?
        .as_dict_mut()
        .map_err(|_| PdfError::Malformed)?;

    if let Some(layout) = params.page_layout {
        catalog.set(
            "PageLayout",
            Object::Name(page_layout_name(layout).to_vec()),
        );
    }
    if let Some(mode) = params.page_mode {
        catalog.set("PageMode", Object::Name(page_mode_name(mode).to_vec()));
    }
    if let Some(vp_obj) = vp_obj_to_set {
        catalog.set("ViewerPreferences", vp_obj);
    }
    if let Some(page_id) = open_page_id {
        catalog.set(
            "OpenAction",
            Object::Array(vec![
                Object::Reference(page_id),
                Object::Name(b"Fit".to_vec()),
            ]),
        );
    }

    Ok(vec![OutputDoc::from_pdf(
        naming::default_pdf_output_name(&stem, "pdf.viewer_prefs"),
        pdf,
    )])
}

const fn page_layout_name(layout: ViewerPageLayout) -> &'static [u8] {
    match layout {
        ViewerPageLayout::SinglePage => b"SinglePage",
        ViewerPageLayout::OneColumn => b"OneColumn",
        ViewerPageLayout::TwoColumnLeft => b"TwoColumnLeft",
        ViewerPageLayout::TwoColumnRight => b"TwoColumnRight",
        ViewerPageLayout::TwoPageLeft => b"TwoPageLeft",
        ViewerPageLayout::TwoPageRight => b"TwoPageRight",
    }
}

const fn page_mode_name(mode: ViewerPageMode) -> &'static [u8] {
    match mode {
        ViewerPageMode::UseNone => b"UseNone",
        ViewerPageMode::UseOutlines => b"UseOutlines",
        ViewerPageMode::UseThumbs => b"UseThumbs",
        ViewerPageMode::FullScreen => b"FullScreen",
        ViewerPageMode::UseOc => b"UseOC",
        ViewerPageMode::UseAttachments => b"UseAttachments",
    }
}
