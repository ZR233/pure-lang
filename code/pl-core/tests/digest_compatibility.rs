use pl_core::context::content_hash;

#[test]
fn content_hash_preserves_sha256_lowercase_hex_identity() {
    assert_eq!(
        content_hash(b"abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
}
