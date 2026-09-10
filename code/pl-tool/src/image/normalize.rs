use std::io::Cursor;

use super::ImageOutputLimits;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, Limits};
use pl_protocol::Result;

use crate::tool_error;

pub(crate) const MAX_SOURCE_BYTES: usize = 20 * 1024 * 1024;
pub(crate) const MAX_DATA_URL_BASE64_BYTES: usize = 5 * 1024 * 1024;
pub(crate) const MAX_IMAGE_SIDE: u32 = 2000;
const MAX_DECODE_SIDE: u32 = 32_768;
const MAX_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;
const JPEG_QUALITIES: [u8; 6] = [85, 75, 65, 55, 45, 35];
const JPEG_MAX_SIDES: [u32; 6] = [2000, 1600, 1280, 1024, 768, 512];

#[derive(Debug)]
pub(crate) struct NormalizedToolImage {
    pub(crate) media_type: String,
    pub(crate) bytes: Vec<u8>,

    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn normalize_tool_image(
    tool_name: &str,
    bytes: Vec<u8>,
    declared_media_type: Option<&str>,
    capability: &ImageOutputLimits,
) -> Result<NormalizedToolImage> {
    let format = image::guess_format(&bytes)
        .map_err(|_| tool_error(tool_name, "file header is not a supported image"))?;
    let detected_media_type = media_type(format).ok_or_else(|| {
        tool_error(
            tool_name,
            "only PNG, JPEG, WebP, and GIF image data is supported",
        )
    })?;
    if let Some(declared) = declared_media_type
        && declared != detected_media_type
    {
        return Err(tool_error(
            tool_name,
            format!(
                "declared image media type {declared} does not match detected {detected_media_type}"
            ),
        ));
    }
    ensure_media_type(tool_name, capability, detected_media_type)?;

    let mut reader = ImageReader::with_format(Cursor::new(bytes.as_slice()), format);
    let mut decode_limits = Limits::default();
    decode_limits.max_image_width = Some(MAX_DECODE_SIDE);
    decode_limits.max_image_height = Some(MAX_DECODE_SIDE);
    decode_limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(decode_limits);
    let decoded = reader
        .decode()
        .map_err(|error| tool_error(tool_name, format!("failed to decode image: {error}")))?;
    let (max_width, max_height, max_bytes) = effective_limits(capability);
    let dimensions = decoded.dimensions();
    let (media_type, bytes, dimensions) =
        if image_within_limits(&bytes, dimensions, max_width, max_height, max_bytes) {
            (detected_media_type.to_string(), bytes, dimensions)
        } else {
            ensure_media_type(tool_name, capability, "image/jpeg")?;
            let (bytes, dimensions) =
                compress_image(tool_name, &decoded, max_width, max_height, max_bytes)?;
            ("image/jpeg".to_string(), bytes, dimensions)
        };

    Ok(NormalizedToolImage {
        media_type,
        bytes,

        width: dimensions.0,
        height: dimensions.1,
    })
}

fn effective_limits(capability: &ImageOutputLimits) -> (u32, u32, usize) {
    let max_width = capability
        .max_width
        .unwrap_or(MAX_IMAGE_SIDE)
        .min(MAX_IMAGE_SIDE);
    let max_height = capability
        .max_height
        .unwrap_or(MAX_IMAGE_SIDE)
        .min(MAX_IMAGE_SIDE);
    let max_bytes = capability
        .max_bytes
        .into_iter()
        .chain(capability.max_total_bytes)
        .min()
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(usize::MAX);
    (max_width, max_height, max_bytes)
}

fn image_within_limits(
    bytes: &[u8],
    dimensions: (u32, u32),
    max_width: u32,
    max_height: u32,
    max_bytes: usize,
) -> bool {
    dimensions.0 <= max_width
        && dimensions.1 <= max_height
        && bytes.len() <= max_bytes
        && base64_encoded_len(bytes.len()) <= MAX_DATA_URL_BASE64_BYTES
}

fn compress_image(
    tool_name: &str,
    decoded: &DynamicImage,
    max_width: u32,
    max_height: u32,
    max_bytes: usize,
) -> Result<(Vec<u8>, (u32, u32))> {
    for max_side in JPEG_MAX_SIDES {
        let candidate = decoded.thumbnail(max_width.min(max_side), max_height.min(max_side));
        let dimensions = candidate.dimensions();
        for quality in JPEG_QUALITIES {
            let bytes = encode_jpeg(tool_name, &candidate, quality)?;
            if image_within_limits(&bytes, dimensions, max_width, max_height, max_bytes) {
                return Ok((bytes, dimensions));
            }
        }
    }
    Err(tool_error(
        tool_name,
        "image is too large after bounded normalization",
    ))
}

fn encode_jpeg(tool_name: &str, image: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|error| tool_error(tool_name, format!("failed to normalize image: {error}")))?;
    Ok(bytes)
}

pub(crate) fn media_type(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::WebP => Some("image/webp"),
        ImageFormat::Gif => Some("image/gif"),
        _ => None,
    }
}

fn ensure_media_type(
    tool_name: &str,
    capability: &ImageOutputLimits,
    media_type: &str,
) -> Result<()> {
    if !capability.media_types.is_empty()
        && !capability
            .media_types
            .iter()
            .any(|allowed| allowed == media_type)
    {
        return Err(tool_error(
            tool_name,
            format!("the current model does not accept {media_type} images"),
        ));
    }
    Ok(())
}

pub(crate) fn base64_encoded_len(byte_len: usize) -> usize {
    byte_len.div_ceil(3) * 4
}
