//! Image processing primitives for PDF optimization.

mod codec;
mod placement;

pub(crate) use codec::{decode_cmyk_jpeg, decode_rgb_jpeg, jpeg_transform};
pub mod similarity;

pub use codec::{
    ColorSpace, DecodeSession, DecodedImage, ImageError, decode, encode_flate, encode_jpeg,
    resample, to_gray,
};
pub use placement::{ImageUse, placements};
