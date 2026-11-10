//! Draw QR codes as vector paths.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        path::{self, Path, PathStyle},
    },
    tools::{OutputDoc, marks_common},
    xobject::Matrix,
};
use ariad_core::{
    limits::Limits,
    tools::params::{HexColor, Opacity, QrBackground, QrLevel, QrParams},
};
use qrcode::{
    QrCode,
    types::{Color, EcLevel},
};

/// Draw a vector QR code on every selected page.
pub fn run(
    inputs: &[PdfDoc],
    params: &QrParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.qr requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let level = match params.ec_level {
        QrLevel::L => EcLevel::L,
        QrLevel::M => EcLevel::M,
        QrLevel::Q => EcLevel::Q,
        QrLevel::H => EcLevel::H,
    };
    let code =
        QrCode::with_error_correction_level(params.content.as_bytes(), level).map_err(|_| {
            PdfError::InvalidParams(
                "QR content is too long for the selected error correction level".into(),
            )
        })?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = usize::from(params.quiet_zone_modules);
    let module = params.size_pt / ((width + 2 * quiet) as f64);
    let mut dark = Path::new();
    for row in 0..width {
        ctx.check()?;
        let mut col = 0;
        while col < width {
            if colors[row * width + col] != Color::Dark {
                col += 1;
                continue;
            }
            let start = col;
            while col < width && colors[row * width + col] == Color::Dark {
                col += 1;
            }
            dark.rect(
                (quiet + start) as f64 * module,
                params.size_pt - (quiet + row + 1) as f64 * module,
                (col - start) as f64 * module,
                module,
            )?;
        }
    }
    let opacity: Opacity =
        serde_json::from_value(serde_json::json!(1.0)).map_err(|_| PdfError::Malformed)?;
    let style = PathStyle {
        fill: Some(params.color.clone()),
        stroke: None,
        stroke_width_pt: 0.0,
        opacity,
    };
    let white: HexColor =
        serde_json::from_value(serde_json::json!("#FFFFFF")).map_err(|_| PdfError::Malformed)?;
    let mut background = Path::new();
    background.rect(0.0, 0.0, params.size_pt, params.size_pt)?;
    let bg_style = PathStyle {
        fill: Some(white),
        stroke: None,
        stroke_width_pt: 0.0,
        opacity,
    };
    let mut pdf = inputs[0].clone();
    let pages = marks_common::pages(&pdf, &params.pages, limits, ctx)?;
    for page in pages {
        ctx.check()?;
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        let (x, y) = marks_common::position(
            &geometry,
            params.anchor,
            params.size_pt,
            params.size_pt,
            params.margin_pt,
            params.offset_x_pt,
            params.offset_y_pt,
            params.x_pt,
            params.y_pt,
            0.0,
        )?;
        let transform = Matrix::translate(x, y);
        if params.background == QrBackground::White {
            path::draw_transformed(
                &mut pdf,
                page,
                &geometry,
                &background,
                &bg_style,
                transform,
                Marking::Content,
                LayerPosition::Over,
                ctx,
            )?;
        }
        path::draw_transformed(
            &mut pdf,
            page,
            &geometry,
            &dark,
            &style,
            transform,
            Marking::Content,
            LayerPosition::Over,
            ctx,
        )?;
    }
    Ok(vec![OutputDoc::unnamed_pdf(pdf)])
}
