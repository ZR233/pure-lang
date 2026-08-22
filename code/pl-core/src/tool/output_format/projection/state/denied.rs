#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeniedToolLifecycle {
    pub(in crate::tool::output_format::projection) reason: String,
    pub(in crate::tool::output_format::projection) reason_preview: String,
    pub(in crate::tool::output_format::projection) duration_ms: u64,
    pub(in crate::tool::output_format::projection) completed_at_unix: i64,
}
