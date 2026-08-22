#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelledToolLifecycle {
    pub(in crate::tool::output_format::projection) cause: String,
    pub(in crate::tool::output_format::projection) cause_preview: String,
    pub(in crate::tool::output_format::projection) duration_ms: u64,
    pub(in crate::tool::output_format::projection) completed_at_unix: i64,
}
