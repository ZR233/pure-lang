mod agent_host;
pub mod entities;
mod ids;
mod interaction_runtime;
mod mappers;
mod paths;
mod product_event_runtime;
mod records;
mod runtime;
mod runtime_state;
mod store;
mod store_support;
mod task_coordinator;
mod task_projection;

pub use interaction_runtime::{
    InteractionEmitter, InteractionEmitterFuture, InteractionRuntime, resolution_matches_kind,
};
pub use product_event_runtime::StudioProductEventRuntime;
pub use records::{AttachmentRecord, ProjectRecord, SessionRecord, SessionVisibility};
pub use runtime::{
    StudioPlanImplementationLifecycle, StudioResolveInteractionResponse, StudioRuntime,
    StudioStopPromptResponse, StudioSubmitPromptOptions, StudioSubmitPromptRequest,
    StudioSubmitPromptResponse, StudioUserPromptPresentation,
};
pub use runtime_state::{
    StudioActiveTurn, StudioRuntimeSnapshot, StudioRuntimeState, StudioRuntimeStatus,
};
pub use store::StudioStore;

#[cfg(test)]
mod tests;
