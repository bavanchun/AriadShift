//! Geometry for marks in the displayed page coordinate system.

use ariad_core::tools::params::Anchor;
use lopdf::{Document, Object, ObjectId};

use crate::{
    PdfError,
    xobject::{Matrix, PdfRect},
};

/// Maximum placements generated on one page.
pub const MAX_PLACEMENTS_PER_PAGE: usize = 4096;

/// Visible page dimensions in physical points and mapping back to PDF user space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
    /// Width in physical points after page rotation.
    pub width_pt: f64,
    /// Height in physical points after page rotation.
    pub height_pt: f64,
    /// Mapping from visual points to the original page user space.
    pub visual_to_user: Matrix,
}

impl PageGeometry {
    /// Build geometry from media and crop boxes, clockwise rotation and UserUnit.
    pub fn new(
        media: PdfRect,
        crop: PdfRect,
        rotation: i32,
        user_unit: f64,
    ) -> Result<Self, PdfError> {
        let coords = [
            media.left,
            media.bottom,
            media.right,
            media.top,
            crop.left,
            crop.bottom,
            crop.right,
            crop.top,
        ];
        if coords.iter().any(|v| !v.is_finite())
            || !user_unit.is_finite()
            || !(0.0..=75_000.0).contains(&user_unit)
            || user_unit == 0.0
        {
            return Err(PdfError::InvalidParams(
                "invalid page geometry or UserUnit".into(),
            ));
        }
        let left = media.left.max(crop.left);
        let bottom = media.bottom.max(crop.bottom);
        let right = media.right.min(crop.right);
        let top = media.top.min(crop.top);
        let w = right - left;
        let h = top - bottom;
        if w <= 0.0
            || h <= 0.0
            || !(w * user_unit).is_finite()
            || !(h * user_unit).is_finite()
            || w * user_unit == 0.0
            || h * user_unit == 0.0
            || !(1.0 / user_unit).is_finite()
        {
            return Err(PdfError::InvalidParams(
                "empty or invalid visible page box".into(),
            ));
        }
        let s = 1.0 / user_unit;
        let (width, height, matrix) = match rotation.rem_euclid(360) {
            0 => (w, h, Matrix([s, 0.0, 0.0, s, left, bottom])),
            90 => (h, w, Matrix([0.0, s, -s, 0.0, right, bottom])),
            180 => (w, h, Matrix([-s, 0.0, 0.0, -s, right, top])),
            270 => (h, w, Matrix([0.0, -s, s, 0.0, left, top])),
            _ => {
                return Err(PdfError::InvalidParams(
                    "page rotation must be a multiple of 90 degrees".into(),
                ));
            }
        };
        Ok(Self {
            width_pt: width * user_unit,
            height_pt: height * user_unit,
            visual_to_user: matrix,
        })
    }

    /// Resolve page boxes, rotation and UserUnit from a document.
    pub fn from_page(doc: &Document, page: ObjectId) -> Result<Self, PdfError> {
        let dict = doc.get_dictionary(page).map_err(|_| PdfError::Malformed)?;
        let unit = match dict.get(b"UserUnit") {
            Err(_) => 1.0,
            Ok(value) => {
                let mut value = value;
                for _ in 0..8 {
                    if let Object::Reference(id) = value {
                        value = doc.get_object(*id).map_err(|_| PdfError::Malformed)?;
                    } else {
                        break;
                    }
                }
                match value {
                    Object::Integer(v) => *v as f64,
                    Object::Real(v) => f64::from(*v),
                    _ => return Err(PdfError::InvalidParams("invalid UserUnit".into())),
                }
            }
        };
        let media = inherited_box(doc, page, b"MediaBox")?.ok_or(PdfError::Malformed)?;
        let crop = inherited_box(doc, page, b"CropBox")?.unwrap_or(media);
        let rotation = strict_rotation(doc, page)?;
        Self::new(media, crop, rotation, unit)
    }

    /// Position the lower-left corner of a box relative to the visible page.
    pub fn place(
        &self,
        anchor: Anchor,
        box_width: f64,
        box_height: f64,
        margin: f64,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<(f64, f64), PdfError> {
        let values = [box_width, box_height, margin, offset_x, offset_y];
        if values.iter().any(|v| !v.is_finite())
            || box_width < 0.0
            || box_height < 0.0
            || margin < 0.0
        {
            return Err(PdfError::InvalidParams(
                "invalid placement dimensions".into(),
            ));
        }
        let x = match anchor {
            Anchor::TopLeft | Anchor::MiddleLeft | Anchor::BottomLeft => margin,
            Anchor::TopCenter | Anchor::Center | Anchor::BottomCenter => {
                (self.width_pt - box_width) / 2.0
            }
            Anchor::TopRight | Anchor::MiddleRight | Anchor::BottomRight => {
                self.width_pt - margin - box_width
            }
        };
        let y = match anchor {
            Anchor::BottomLeft | Anchor::BottomCenter | Anchor::BottomRight => margin,
            Anchor::MiddleLeft | Anchor::Center | Anchor::MiddleRight => {
                (self.height_pt - box_height) / 2.0
            }
            Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight => {
                self.height_pt - margin - box_height
            }
        };
        Ok((x + offset_x, y + offset_y))
    }

    /// Angle in degrees of the lower-left to upper-right page diagonal.
    #[must_use]
    pub fn diagonal_angle(self) -> f64 {
        self.height_pt.atan2(self.width_pt).to_degrees()
    }
}

fn inherited_box(doc: &Document, page: ObjectId, key: &[u8]) -> Result<Option<PdfRect>, PdfError> {
    let mut current = Some(page);
    let mut visited = std::collections::HashSet::new();
    for _ in 0..64 {
        let Some(id) = current else { return Ok(None) };
        if !visited.insert(id) {
            return Err(PdfError::Malformed);
        }
        let dict = doc.get_dictionary(id).map_err(|_| PdfError::Malformed)?;
        if let Ok(value) = dict.get(key) {
            let array = resolve(doc, value)?
                .as_array()
                .map_err(|_| PdfError::Malformed)?;
            if array.len() != 4 {
                return Err(PdfError::Malformed);
            }
            let mut values = [0.0; 4];
            for (out, value) in values.iter_mut().zip(array) {
                *out = match resolve(doc, value)? {
                    Object::Integer(v) => *v as f64,
                    Object::Real(v) => f64::from(*v),
                    _ => return Err(PdfError::Malformed),
                };
            }
            if values.iter().any(|v| !v.is_finite())
                || values[0] == values[2]
                || values[1] == values[3]
            {
                return Err(PdfError::Malformed);
            }
            return Ok(Some(PdfRect::from_array(values)));
        }
        current = match dict.get(b"Parent") {
            Ok(value) => Some(value.as_reference().map_err(|_| PdfError::Malformed)?),
            Err(_) => None,
        };
    }
    Err(PdfError::Malformed)
}

fn resolve<'a>(doc: &'a Document, mut value: &'a Object) -> Result<&'a Object, PdfError> {
    for _ in 0..8 {
        match value {
            Object::Reference(id) => {
                value = doc.get_object(*id).map_err(|_| PdfError::Malformed)?
            }
            _ => return Ok(value),
        }
    }
    Err(PdfError::Malformed)
}

fn strict_rotation(doc: &Document, page: ObjectId) -> Result<i32, PdfError> {
    let mut current = Some(page);
    let mut visited = std::collections::HashSet::new();
    for _ in 0..64 {
        let Some(id) = current else { return Ok(0) };
        if !visited.insert(id) {
            return Err(PdfError::Malformed);
        }
        let dict = doc.get_dictionary(id).map_err(|_| PdfError::Malformed)?;
        if let Ok(value) = dict.get(b"Rotate") {
            let mut value = value;
            for _ in 0..8 {
                if let Object::Reference(id) = value {
                    value = doc.get_object(*id).map_err(|_| PdfError::Malformed)?;
                } else {
                    break;
                }
            }
            let rotation = value.as_i64().map_err(|_| PdfError::Malformed)?;
            return i32::try_from(rotation.rem_euclid(360)).map_err(|_| PdfError::Malformed);
        }
        current = match dict.get(b"Parent") {
            Ok(v) => Some(v.as_reference().map_err(|_| PdfError::Malformed)?),
            Err(_) => None,
        };
    }
    Err(PdfError::Malformed)
}

/// Rotate around a point in visual space.
#[must_use]
pub fn rotation_at(x: f64, y: f64, degrees: f64) -> Matrix {
    Matrix::translate(-x, -y)
        .then(Matrix::rotate_deg(degrees))
        .then(Matrix::translate(x, y))
}

/// Generate a bounded grid of lower-left positions within the visual page.
pub fn tiled_grid(
    width: f64,
    height: f64,
    step_x: f64,
    step_y: f64,
) -> Result<Vec<(f64, f64)>, PdfError> {
    if [width, height, step_x, step_y]
        .iter()
        .any(|v| !v.is_finite())
        || width <= 0.0
        || height <= 0.0
        || step_x <= 0.0
        || step_y <= 0.0
    {
        return Err(PdfError::InvalidParams("invalid tile geometry".into()));
    }
    let columns = (width / step_x).ceil();
    let rows = (height / step_y).ceil();
    if columns * rows > MAX_PLACEMENTS_PER_PAGE as f64 {
        return Err(PdfError::InvalidParams("too many tile placements".into()));
    }
    let mut positions = Vec::new();
    for row in 0..rows as usize {
        for column in 0..columns as usize {
            positions.push((column as f64 * step_x, row as f64 * step_y));
        }
    }
    Ok(positions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    #[test]
    fn maps_rotated_cropped_pages_in_physical_points() {
        let media = PdfRect::new(0.0, 0.0, 200.0, 300.0);
        let crop = PdfRect::new(10.0, 20.0, 110.0, 220.0);
        for (rotation, size, origin, opposite) in [
            (0, (200.0, 400.0), (10.0, 20.0), (110.0, 220.0)),
            (90, (400.0, 200.0), (110.0, 20.0), (10.0, 220.0)),
            (180, (200.0, 400.0), (110.0, 220.0), (10.0, 20.0)),
            (270, (400.0, 200.0), (10.0, 220.0), (110.0, 20.0)),
        ] {
            let g = PageGeometry::new(media, crop, rotation, 2.0).unwrap();
            assert_eq!((g.width_pt, g.height_pt), size);
            assert_eq!(g.visual_to_user.transform_point(0.0, 0.0), origin);
            assert_eq!(g.visual_to_user.transform_point(size.0, size.1), opposite);
        }
        assert!(PageGeometry::new(media, crop, 45, 1.0).is_err());
        assert!(PageGeometry::new(media, crop, 0, 0.0).is_err());
        assert!(PageGeometry::new(media, crop, 0, f64::MIN_POSITIVE / 16.0).is_err());
        assert!(PageGeometry::new(media, PdfRect::new(300.0, 0.0, 400.0, 10.0), 0, 1.0).is_err());
    }
    #[test]
    fn anchors_and_grid_are_bounded() {
        let g = PageGeometry::new(
            PdfRect::from_size(200.0, 300.0),
            PdfRect::from_size(200.0, 300.0),
            0,
            1.0,
        )
        .unwrap();
        assert_eq!(
            g.place(Anchor::TopRight, 20.0, 30.0, 10.0, 2.0, -3.0)
                .unwrap(),
            (172.0, 257.0)
        );
        assert_eq!(tiled_grid(20.0, 30.0, 10.0, 10.0).unwrap().len(), 6);
        assert!(tiled_grid(10000.0, 10000.0, 1.0, 1.0).is_err());
    }

    #[test]
    fn inherited_geometry_rejects_malformed_values() {
        let mut doc = Document::with_version("1.7");
        let parent = doc.add_object(dictionary! {
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "CropBox" => vec![10.into(), 20.into(), 110.into(), 220.into()],
            "Rotate" => 90,
        });
        let page = doc.add_object(dictionary! { "Parent" => parent, "UserUnit" => 2 });
        let geometry = PageGeometry::from_page(&doc, page).unwrap();
        assert_eq!((geometry.width_pt, geometry.height_pt), (400.0, 200.0));
        assert_eq!(
            geometry.visual_to_user.transform_point(0.0, 0.0),
            (110.0, 20.0)
        );
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("CropBox", vec![0.into(), 0.into(), 0.into(), 1.into()]);
        assert!(PageGeometry::from_page(&doc, page).is_err());
        doc.get_dictionary_mut(page).unwrap().remove(b"CropBox");
        doc.get_dictionary_mut(page).unwrap().set("UserUnit", 0);
        assert!(PageGeometry::from_page(&doc, page).is_err());
        doc.get_dictionary_mut(page).unwrap().set("UserUnit", 1);
        doc.get_dictionary_mut(page).unwrap().set("Rotate", 45);
        assert!(PageGeometry::from_page(&doc, page).is_err());
    }
}
