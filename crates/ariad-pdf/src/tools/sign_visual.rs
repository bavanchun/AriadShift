//! Add a visual signature (type, draw, or image) to a PDF page.

use ariad_core::{
    limits::Limits,
    tools::params::{FontFace, HexColor, Opacity, SignVisualMode, SignVisualParams},
};
use base64::Engine as _;
use lopdf::{Object, dictionary};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        image::{self, ImagePlacement},
        layer::{LayerPosition, Marking},
        path::{self, Path, PathStyle},
        text::{self, TextAlign, TextLayout, TextStyle},
    },
    tools::{OutputDoc, mark_image},
};

pub fn run(
    inputs: &[PdfDoc],
    params: &SignVisualParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.is_empty() {
        return Err(PdfError::InvalidParams(
            "pdf.sign_visual requires at least one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;

    let mut pdf = inputs[0].clone();
    let page_ids = crate::color::page_ids(pdf.inner(), limits, ctx)?;
    if params.page == 0 || params.page as usize > page_ids.len() {
        return Err(PdfError::InvalidParams(format!(
            "page {} out of bounds (document has {} pages)",
            params.page,
            page_ids.len()
        )));
    }
    let target_page = page_ids[params.page as usize - 1];
    let geometry = PageGeometry::from_page(pdf.inner(), target_page)?;

    let x1 = params.rect[0];
    let y1 = params.rect[1];
    let x2 = params.rect[2];
    let y2 = params.rect[3];
    let width = x2 - x1;
    let height = y2 - y1;

    let default_color = serde_json::from_value::<HexColor>(serde_json::json!("#111827")).unwrap();
    let color = params.color.clone().unwrap_or(default_color);
    let opacity = serde_json::from_value::<Opacity>(serde_json::json!(1.0)).unwrap();

    match params.mode {
        SignVisualMode::Type => {
            let text = params.text.as_deref().unwrap_or_default();
            let mut label = text.to_owned();
            if let Some(name) = &params.signer_name {
                label = format!("{label}\nDigitally signed by: {name}");
            }
            if let Some(date) = &params.date {
                label = format!("{label}\nDate: {date}");
            }

            let mut plan = FontPlan::new(FontFace::Serif);
            let font_size = (height / 3.0).clamp(8.0, 24.0);
            let layout = TextLayout::prepare(&mut plan, &label, font_size, Some(width), 1.2)?;
            let font = plan.embed(pdf.inner_mut()?)?;
            let style = TextStyle {
                size_pt: font_size,
                color,
                opacity,
                stroke: None,
                align: TextAlign::Left,
                rotation_deg: 0.0,
            };
            text::draw(
                &mut pdf,
                target_page,
                &geometry,
                &font,
                &layout,
                &style,
                x1,
                y1,
                Marking::Content,
                LayerPosition::Over,
                ctx,
            )?;
        }
        SignVisualMode::Draw => {
            let data = params.vector_data.as_deref().unwrap_or_default();
            let mut path = Path::new();
            let mut first = true;
            for token in data.split_whitespace() {
                let Some((sx, sy)) = token.split_once(',') else {
                    continue;
                };
                if let (Ok(px), Ok(py)) = (sx.parse::<f64>(), sy.parse::<f64>()) {
                    let pt_x = x1 + px;
                    let pt_y = y1 + py;
                    if first {
                        path.move_to(pt_x, pt_y)?;
                        first = false;
                    } else {
                        path.line_to(pt_x, pt_y)?;
                    }
                }
            }
            if first {
                path.move_to(x1 + 5.0, y1 + height * 0.4)?;
                path.curve_to(
                    x1 + width * 0.3,
                    y1 + height * 0.9,
                    x1 + width * 0.7,
                    y1 + height * 0.1,
                    x1 + width - 5.0,
                    y1 + height * 0.6,
                )?;
            }
            let path_style = PathStyle {
                fill: None,
                stroke: Some(color),
                stroke_width_pt: 2.0,
                opacity,
            };
            path::draw(
                &mut pdf,
                target_page,
                &geometry,
                &path,
                &path_style,
                Marking::Content,
                LayerPosition::Over,
                ctx,
            )?;
        }
        SignVisualMode::Image => {
            let raw_data = params.image_data.as_deref().unwrap_or_default();
            let img_bytes = base64::engine::general_purpose::STANDARD
                .decode(raw_data.trim())
                .unwrap_or_else(|_| raw_data.as_bytes().to_vec());
            let embedded = mark_image::embed(&mut pdf, &img_bytes, limits, ctx)?;
            let placement = ImagePlacement {
                x: x1,
                y: y1,
                width,
                height,
                rotation_deg: 0.0,
                opacity,
            };
            image::draw(
                &mut pdf,
                target_page,
                &geometry,
                embedded,
                placement,
                Marking::Content,
                LayerPosition::Over,
                ctx,
            )?;
        }
    }

    if !params.flatten {
        let annot = dictionary! {
            "Type" => "Annot",
            "Subtype" => "Stamp",
            "Rect" => Object::Array(vec![
                Object::Real(x1 as f32),
                Object::Real(y1 as f32),
                Object::Real(x2 as f32),
                Object::Real(y2 as f32),
            ]),
            "Name" => "Sign",
            "Contents" => Object::string_literal("Signature"),
        };
        let annot_id = pdf.inner_mut()?.add_object(Object::Dictionary(annot));
        let page_dict = pdf
            .inner_mut()?
            .get_object_mut(target_page)
            .map_err(|_| PdfError::Malformed)?
            .as_dict_mut()
            .map_err(|_| PdfError::Malformed)?;
        if let Ok(annots) = page_dict.get_mut(b"Annots") {
            if let Ok(arr) = annots.as_array_mut() {
                arr.push(Object::Reference(annot_id));
            }
        } else {
            page_dict.set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
        }
    }

    Ok(vec![OutputDoc::unnamed_pdf(pdf)])
}
