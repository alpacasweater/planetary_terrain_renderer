//! Shared streaming helpers, so the remote and local code paths cannot silently diverge.

use std::io::Cursor;

use tiff::{
    decoder::DecodingResult,
    encoder::{TiffEncoder, colortype},
};

use crate::streaming::StreamingProviderError;

/// Converts any TIFF `read_image` result into `f32` samples, accepting every integer and float
/// sample type and rejecting only half floats.
///
/// Both the remote DEM decoder (`opentopography`) and the local cached-height decoder
/// (`scheduler`) go through this one function, so they accept exactly the same set of sample
/// types. Previously they diverged (the local decoder rejected `i16`, which real SRTM tiles
/// use), a latent bug where a tile written or fetched in one path could fail to decode in the
/// other.
pub(crate) fn decoding_result_to_f32(
    result: DecodingResult,
) -> Result<Vec<f32>, StreamingProviderError> {
    Ok(match result {
        DecodingResult::U8(values) => values.into_iter().map(f32::from).collect(),
        DecodingResult::U16(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::U32(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::U64(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::I8(values) => values.into_iter().map(f32::from).collect(),
        DecodingResult::I16(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::I32(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::I64(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::F32(values) => values,
        DecodingResult::F64(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::F16(_) => {
            return Err(StreamingProviderError::Permanent(
                "TIFF uses unsupported F16 samples".to_string(),
            ));
        }
    })
}

/// Encodes a single-channel `f32` height raster as a `Gray32Float` TIFF. Shared by every code
/// path that materializes a height tile so the on-disk encoding stays identical.
pub(crate) fn encode_height_tiff(
    width: u32,
    height: u32,
    heights: &[f32],
) -> Result<Vec<u8>, StreamingProviderError> {
    let mut cursor = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut cursor).map_err(|error| {
        StreamingProviderError::Permanent(format!("failed to create height TIFF encoder: {error}"))
    })?;
    encoder
        .write_image::<colortype::Gray32Float>(width, height, heights)
        .map_err(|error| {
            StreamingProviderError::Permanent(format!("failed to encode height TIFF: {error}"))
        })?;
    Ok(cursor.into_inner())
}
