//! Image processing primitives for PDF optimization.

mod codec;
mod placement;

pub(crate) use codec::decode_with_budget;
#[cfg(feature = "codecs")]
pub(crate) use codec::{decode_cmyk_jpeg, decode_rgb_jpeg, jpeg_transform};
pub mod similarity;

pub use codec::{
    ColorSpace, DecodeSession, DecodedImage, ImageError, decode, encode_flate, encode_jpeg,
    encode_jpeg_420, resample, to_gray,
};
pub use placement::{ImageUse, placements};
