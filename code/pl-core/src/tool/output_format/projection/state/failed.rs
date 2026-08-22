#[derive(Debug, Clone, PartialEq)]
pub struct FailedToolLifecycle {
    pub(in crate::tool::output_format::projection) output: String,
    pub(in crate::tool::output_format::projection) output_preview: String,
    pub(in crate::tool::output_format::projection) output_artifacts: Vec<serde_json::Value>,
    pub(in crate::tool::output_format::projection) output_metrics:
        Option<pl_trace::TraceToolOutputMetrics>,
    pub(in crate::tool::output_format::projection) duration_ms: u64,
    pub(in crate::tool::output_format::projection) completed_at_unix: i64,
}
