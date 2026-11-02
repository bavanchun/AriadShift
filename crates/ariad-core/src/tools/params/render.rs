//! Parameter types for `pdf.render` and its alias `pdf.to_images`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ParamsError, check_range};
use crate::tools::PageRange;

/// Lowest accepted render resolution in dots per inch.
pub const MIN_DPI: u32 = 36;
/// Highest accepted render resolution in dots per inch.
pub const MAX_DPI: u32 = 600;
/// Default render resolution of `pdf.render`.
pub const DEFAULT_RENDER_DPI: u32 = 150;
/// Default render resolution of `pdf.to_images`, matching Stirling-PDF's "PDF to image".
pub const DEFAULT_TO_IMAGES_DPI: u32 = 300;
/// Lowest accepted lossy-encoder quality.
pub const MIN_QUALITY: u8 = 1;
/// Highest accepted lossy-encoder quality.
pub const MAX_QUALITY: u8 = 100;

/// Image encoding of a rendered page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RasterFormat {
    /// Lossless PNG.
    #[default]
    Png,
    /// Lossy JPEG; has no alpha channel.
    Jpeg,
    /// WebP.
    Webp,
    /// Uncompressed BMP.
    Bmp,
    /// TIFF.
    Tiff,
}

impl RasterFormat {
    /// Every render format in declaration order.
    pub const ALL: &'static [Self] = &[Self::Png, Self::Jpeg, Self::Webp, Self::Bmp, Self::Tiff];

    /// Format identifier, equal to the file extension used for the artifact.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
        }
    }

    /// Whether the encoder takes a `quality` setting.
    #[must_use]
    pub const fn is_lossy(self) -> bool {
        matches!(self, Self::Jpeg | Self::Webp)
    }

    /// Whether the encoding can store a transparent background.
    #[must_use]
    pub const fn supports_alpha(self) -> bool {
        !matches!(self, Self::Jpeg)
    }
}

/// Page background painted before the page content.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    /// Opaque white.
    #[default]
    White,
    /// Fully transparent; rejected for formats without alpha.
    Transparent,
}

/// Whether the rendered pages stay separate images or become one tall image.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Combine {
    /// One image per page.
    #[default]
    None,
    /// All selected pages stacked vertically into a single image.
    Vertical,
}

fn default_render_dpi() -> u32 {
    DEFAULT_RENDER_DPI
}

fn default_to_images_dpi() -> u32 {
    DEFAULT_TO_IMAGES_DPI
}

fn default_true() -> bool {
    true
}

macro_rules! render_params {
    ($(#[$meta:meta])* $name:ident, $dpi_fn:literal, $dpi_const:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            /// Pages to render (1-based selector).
            #[serde(default)]
            pub pages: PageRange,
            /// Render resolution in dots per inch.
            #[serde(default = $dpi_fn)]
            #[schemars(range(min = 36, max = 600))]
            pub dpi: u32,
            /// Output image encoding.
            #[serde(default)]
            pub format: RasterFormat,
            /// Encoder quality for `jpeg` and `webp` (1 to 100); the encoder default applies when
            /// absent. Rejected for lossless formats.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[schemars(range(min = 1, max = 100))]
            pub quality: Option<u8>,
            /// Page background; `transparent` is rejected for `jpeg`.
            #[serde(default)]
            pub background: Background,
            /// Whether annotations and form widgets are drawn.
            #[serde(default = "default_true")]
            pub annotations: bool,
            /// `vertical` stacks every selected page into one tall image.
            #[serde(default)]
            pub combine: Combine,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    pages: PageRange::all(),
                    dpi: $dpi_const,
                    format: RasterFormat::Png,
                    quality: None,
                    background: Background::White,
                    annotations: true,
                    combine: Combine::None,
                }
            }
        }

        impl $name {
            /// Checks the ranges and combinations the JSON schema alone cannot enforce.
            pub fn validate(&self) -> Result<(), ParamsError> {
                validate_render_fields(self.dpi, self.format, self.quality, self.background)
            }
        }
    };
}

render_params! {
    /// Parameters for `pdf.render`.
    RenderParams, "default_render_dpi", DEFAULT_RENDER_DPI
}

render_params! {
    /// Parameters for `pdf.to_images`, the alias of `pdf.render` with Stirling-PDF's 300 dpi
    /// default.
    ToImagesParams, "default_to_images_dpi", DEFAULT_TO_IMAGES_DPI
}

impl From<ToImagesParams> for RenderParams {
    fn from(params: ToImagesParams) -> Self {
        Self {
            pages: params.pages,
            dpi: params.dpi,
            format: params.format,
            quality: params.quality,
            background: params.background,
            annotations: params.annotations,
            combine: params.combine,
        }
    }
}

fn validate_render_fields(
    dpi: u32,
    format: RasterFormat,
    quality: Option<u8>,
    background: Background,
) -> Result<(), ParamsError> {
    check_range("dpi", dpi, MIN_DPI, MAX_DPI)?;
    if let Some(quality) = quality {
        check_range("quality", quality, MIN_QUALITY, MAX_QUALITY)?;
        if !format.is_lossy() {
            return Err(ParamsError::new(
                "quality",
                "quality applies only to jpeg and webp",
            ));
        }
    }
    if background == Background::Transparent && !format.supports_alpha() {
        return Err(ParamsError::new(
            "background",
            "jpeg cannot store a transparent background",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tools::params::schema_for_params;

    fn parse<P: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<P, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    fn schema_accepts<P: JsonSchema>(value: &serde_json::Value) -> bool {
        let schema = serde_json::to_value(schema_for_params::<P>()).expect("schema serializes");
        jsonschema::validator_for(&schema)
            .expect("schema compiles")
            .is_valid(value)
    }

    #[test]
    fn render_defaults_follow_the_tool_table() {
        let params: RenderParams = parse(json!({})).expect("empty object is valid");
        assert_eq!(params, RenderParams::default());
        assert_eq!(params.pages, PageRange::all());
        assert_eq!(params.dpi, 150);
        assert_eq!(params.format, RasterFormat::Png);
        assert_eq!(params.quality, None);
        assert_eq!(params.background, Background::White);
        assert!(params.annotations);
        assert_eq!(params.combine, Combine::None);
        params.validate().expect("defaults validate");
    }

    #[test]
    fn to_images_defaults_to_stirling_resolution() {
        let params: ToImagesParams = parse(json!({})).expect("empty object is valid");
        assert_eq!(params, ToImagesParams::default());
        assert_eq!(params.dpi, 300);
        assert_eq!(RenderParams::from(params).dpi, 300);
    }

    #[test]
    fn render_accepts_a_full_valid_example() {
        let value = json!({
            "pages": "1-3,last",
            "dpi": 600,
            "format": "jpeg",
            "quality": 80,
            "background": "white",
            "annotations": false,
            "combine": "vertical",
        });
        let params: RenderParams = parse(value.clone()).expect("valid example");
        params.validate().expect("valid example validates");
        assert!(schema_accepts::<RenderParams>(&value));
    }

    #[test]
    fn render_accepts_every_format() {
        for format in RasterFormat::ALL {
            let params: RenderParams = parse(json!({"format": format.as_str()})).expect("format");
            assert_eq!(params.format, *format);
        }
    }

    #[test]
    fn render_rejects_invalid_examples() {
        for value in [
            json!({"unknown": 1}),
            json!({"format": "gif"}),
            json!({"background": "red"}),
            json!({"combine": "horizontal"}),
            json!({"dpi": -1}),
            json!({"dpi": "150"}),
        ] {
            assert!(parse::<RenderParams>(value.clone()).is_err(), "{value}");
            assert!(!schema_accepts::<RenderParams>(&value), "{value}");
        }
        // The page-range grammar is enforced by deserialization, not by the JSON schema.
        assert!(parse::<RenderParams>(json!({"pages": "0"})).is_err());
    }

    #[test]
    fn render_validate_rejects_out_of_range_and_inconsistent_values() {
        for params in [
            RenderParams {
                dpi: 35,
                ..RenderParams::default()
            },
            RenderParams {
                dpi: 601,
                ..RenderParams::default()
            },
            RenderParams {
                quality: Some(0),
                format: RasterFormat::Jpeg,
                ..RenderParams::default()
            },
            RenderParams {
                quality: Some(101),
                format: RasterFormat::Webp,
                ..RenderParams::default()
            },
            RenderParams {
                quality: Some(90),
                ..RenderParams::default()
            },
            RenderParams {
                format: RasterFormat::Jpeg,
                background: Background::Transparent,
                ..RenderParams::default()
            },
        ] {
            assert!(params.validate().is_err(), "{params:?}");
        }
    }

    #[test]
    fn render_schema_declares_ranges_and_defaults() {
        let schema =
            serde_json::to_value(schema_for_params::<RenderParams>()).expect("schema serializes");
        let dpi = &schema["properties"]["dpi"];
        assert_eq!(dpi["minimum"], 36);
        assert_eq!(dpi["maximum"], 600);
        assert_eq!(dpi["default"], 150);
        assert!(!schema_accepts::<RenderParams>(&json!({"dpi": 35})));
        assert!(!schema_accepts::<RenderParams>(&json!({"dpi": 601})));
        assert!(!schema_accepts::<RenderParams>(&json!({"quality": 0})));
        assert!(!schema_accepts::<RenderParams>(&json!({"quality": 101})));
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn to_images_schema_differs_from_render_only_in_the_dpi_default() {
        let mut render =
            serde_json::to_value(schema_for_params::<RenderParams>()).expect("schema serializes");
        let mut to_images =
            serde_json::to_value(schema_for_params::<ToImagesParams>()).expect("schema serializes");
        assert_eq!(to_images["properties"]["dpi"]["default"], 300);
        for schema in [&mut render, &mut to_images] {
            schema["properties"]["dpi"]["default"] = json!(null);
            schema["title"] = json!(null);
            schema["description"] = json!(null);
        }
        assert_eq!(render, to_images);
    }
}
