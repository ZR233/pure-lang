//! Tool lifecycle projection payloads.

mod cancelled;
mod denied;
mod failed;
mod running;
mod started;
mod succeeded;

pub use cancelled::CancelledToolLifecycle;
pub use denied::DeniedToolLifecycle;
pub use failed::FailedToolLifecycle;
pub use running::RunningToolLifecycle;
pub use started::StartedToolLifecycle;
pub use succeeded::SucceededToolLifecycle;

#[derive(Debug, Clone, PartialEq)]
pub enum ToolLifecycleState {
    Started(StartedToolLifecycle),
    Running(RunningToolLifecycle),
    Succeeded(SucceededToolLifecycle),
    Failed(FailedToolLifecycle),
    Denied(DeniedToolLifecycle),
    Cancelled(CancelledToolLifecycle),
}

impl ToolLifecycleState {
    pub fn output(&self) -> Option<&str> {
        match self {
            Self::Succeeded(state) => Some(&state.output),
            Self::Failed(state) => Some(&state.output),
            Self::Denied(state) => Some(&state.reason),
            Self::Cancelled(state) => Some(&state.cause),
            Self::Started(_) | Self::Running(_) => None,
        }
    }

    pub fn output_preview(&self) -> Option<&str> {
        match self {
            Self::Succeeded(state) => Some(&state.output_preview),
            Self::Failed(state) => Some(&state.output_preview),
            Self::Denied(state) => Some(&state.reason_preview),
            Self::Cancelled(state) => Some(&state.cause_preview),
            Self::Started(_) | Self::Running(_) => None,
        }
    }

    pub fn output_artifacts(&self) -> &[serde_json::Value] {
        match self {
            Self::Succeeded(state) => &state.output_artifacts,
            Self::Failed(state) => &state.output_artifacts,
            Self::Started(_) | Self::Running(_) | Self::Denied(_) | Self::Cancelled(_) => &[],
        }
    }

    pub fn output_metrics(&self) -> Option<&pl_trace::TraceToolOutputMetrics> {
        match self {
            Self::Succeeded(state) => state.output_metrics.as_ref(),
            Self::Failed(state) => state.output_metrics.as_ref(),
            Self::Started(_) | Self::Running(_) | Self::Denied(_) | Self::Cancelled(_) => None,
        }
    }

    pub fn duration_ms(&self) -> Option<u64> {
        match self {
            Self::Succeeded(state) => Some(state.duration_ms),
            Self::Failed(state) => Some(state.duration_ms),
            Self::Denied(state) => Some(state.duration_ms),
            Self::Cancelled(state) => Some(state.duration_ms),
            Self::Started(_) | Self::Running(_) => None,
        }
    }

    pub fn completed_at_unix(&self) -> Option<i64> {
        match self {
            Self::Succeeded(state) => Some(state.completed_at_unix),
            Self::Failed(state) => Some(state.completed_at_unix),
            Self::Denied(state) => Some(state.completed_at_unix),
            Self::Cancelled(state) => Some(state.completed_at_unix),
            Self::Started(_) | Self::Running(_) => None,
        }
    }
}
