//! Place a previously embedded raster image on a displayed page.

use ariad_core::tools::params::Opacity;
use lopdf::{
    Object,
    content::{Content, Operation},
    dictionary,
};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        geometry::PageGeometry,
        layer::{self, LayerPosition, Marking},
    },
    image_to_pdf::ImageXObject,
    xobject::Matrix,
};

/// The image's intrinsic dimensions after applying EXIF orientation.
#[must_use]
pub fn oriented_dimensions(image: ImageXObject) -> (u32, u32) {
    if matches!(image.orientation, 5..=8) {
        (image.height, image.width)
    } else {
        (image.width, image.height)
    }
}

fn orientation_matrix(orientation: u8) -> Result<Matrix, PdfError> {
    Ok(Matrix(match orientation {
        1 => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        2 => [-1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        3 => [-1.0, 0.0, 0.0, -1.0, 1.0, 1.0],
        4 => [1.0, 0.0, 0.0, -1.0, 0.0, 1.0],
        5 => [0.0, -1.0, -1.0, 0.0, 1.0, 1.0],
        6 => [0.0, -1.0, 1.0, 0.0, 0.0, 1.0],
        7 => [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
        8 => [0.0, 1.0, -1.0, 0.0, 1.0, 0.0],
        _ => return Err(PdfError::InvalidParams("invalid image orientation".into())),
    }))
}
fn number(value: f64) -> Result<Object, PdfError> {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err(PdfError::InvalidParams(
            "image placement out of range".into(),
        ));
    }
    Ok(Object::Real(value as f32))
}

/// Image placement in physical points in the displayed page coordinate space.
#[derive(Clone, Copy, Debug)]
pub struct ImagePlacement {
    /// Lower-left x coordinate.
    pub x: f64,
    /// Lower-left y coordinate.
    pub y: f64,
    /// Displayed width.
    pub width: f64,
    /// Displayed height.
    pub height: f64,
    /// Counterclockwise rotation about the placement point.
    pub rotation_deg: f64,
    /// Fill opacity.
    pub opacity: Opacity,
}

/// Place an image XObject previously created by `image_to_pdf::embed_image`.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    pdf: &mut PdfDoc,
    page: lopdf::ObjectId,
    geometry: &PageGeometry,
    image: ImageXObject,
    placement: ImagePlacement,
    marking: Marking,
    position: LayerPosition,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if placement.width <= 0.0
        || placement.height <= 0.0
        || !placement.rotation_deg.is_finite()
        || placement.rotation_deg.abs() > 360_000.0
    {
        return Err(PdfError::InvalidParams("invalid image placement".into()));
    }
    let mut resources = layer::page_resources(pdf.inner(), page);
    let name = layer::insert_resource(
        pdf.inner(),
        &mut resources,
        b"XObject",
        "im",
        Object::Reference(image.id),
    )?;
    let mut operations = Vec::new();
    if placement.opacity.value() < 1.0 {
        let opacity = number(placement.opacity.value())?;
        let id = pdf.inner_mut()?.add_object(
            dictionary! { "Type" => "ExtGState", "ca" => opacity.clone(), "CA" => opacity },
        );
        layer::require_transparency(pdf.inner_mut()?);
        let gs = layer::insert_resource(
            pdf.inner(),
            &mut resources,
            b"ExtGState",
            "gs",
            Object::Reference(id),
        )?;
        operations.push(Operation::new("gs", vec![Object::Name(gs)]));
    }
    let transform = orientation_matrix(image.orientation)?
        .then(Matrix::scale(placement.width, placement.height))
        .then(Matrix::rotate_deg(placement.rotation_deg))
        .then(Matrix::translate(placement.x, placement.y))
        .then(geometry.visual_to_user);
    operations.push(Operation::new(
        "cm",
        transform
            .0
            .into_iter()
            .map(number)
            .collect::<Result<_, _>>()?,
    ));
    operations.push(Operation::new("Do", vec![Object::Name(name)]));
    let body = Content { operations }
        .encode()
        .map_err(|_| PdfError::Malformed)?;
    layer::append(pdf, page, resources, &body, marking, position, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exif_matrices_map_unit_square_into_unit_square() {
        for orientation in 1..=8 {
            let matrix = orientation_matrix(orientation).unwrap();
            let mut corners = [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]
                .map(|(x, y)| matrix.transform_point(x, y));
            corners.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            assert_eq!(corners, [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]);
        }
        assert_eq!(
            orientation_matrix(6).unwrap().transform_point(0.0, 1.0),
            (1.0, 1.0)
        );
        assert_eq!(
            orientation_matrix(8).unwrap().transform_point(0.0, 1.0),
            (0.0, 0.0)
        );
    }
}
