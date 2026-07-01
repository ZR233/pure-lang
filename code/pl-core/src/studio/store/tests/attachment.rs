use super::*;
use pretty_assertions::assert_eq;

#[test]
fn oversized_image_attachment_is_resized_and_compressed() {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        MAX_IMAGE_SIDE + 100,
        10,
        image::Rgba([240, 64, 32, 255]),
    ));
    let mut cursor = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();
    let bytes = cursor.into_inner();

    let normalized = normalize_image_attachment("image/png", bytes, image).unwrap();

    assert_eq!(normalized.media_type, "image/jpeg");
    assert!(normalized.dimensions.0 <= MAX_IMAGE_SIDE);
    assert!(normalized.dimensions.1 <= MAX_IMAGE_SIDE);
    assert!(base64_encoded_len(normalized.bytes.len()) <= MAX_BASE64_IMAGE_BYTES);
}
