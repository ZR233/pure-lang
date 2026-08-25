# rust-embed 8.12.0 provenance

- Source: crates.io `rust-embed` version `8.12.0`
- Crate SHA-256: `e9e7760e252aaba7b09f4be00e36476cf585bdb68a53552ac954cdf504ab4bc9`
- Upstream Git revision: `900d167d3cd2e6b897fee019a2a2ce34533edbef`
- License: MIT (`LICENSE`)

The vendored `src/lib.rs` is unmodified. This source-isolated copy prevents
Studio's required `compression` feature from being unified into
`utoipa-swagger-ui`'s registry dependency, whose generated absolute asset path
is incompatible with that feature. Update this copy and the paired
`rust-embed-impl` copy together.
