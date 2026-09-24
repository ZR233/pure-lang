use pl_core::context::{OpaquePayload, ResourceError, ResourceReference, content_hash};

#[test]
fn content_hash_preserves_sha256_lowercase_hex_identity() {
    let digest = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(content_hash(b"abc"), digest);
    assert_eq!(OpaquePayload::text("abc").content_digest(), digest);

    let resource = ResourceReference::new("example".into(), digest.into(), 3, "text/plain".into())
        .expect("valid resource reference");
    assert_eq!(resource.verify(b"abc"), Ok(()));
    assert_eq!(resource.verify(b"abd"), Err(ResourceError::ContentMismatch));

    let truncated = ResourceReference::new("example".into(), digest.into(), 2, "text/plain".into())
        .expect("valid metadata with a shorter declared length");
    assert_eq!(
        truncated.verify(b"abc"),
        Err(ResourceError::ContentMismatch)
    );
}
