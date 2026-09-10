//! Shared construction of real Studio Thread owners for host contract tests.
use pl_studio_runtime::thread_assembler::{AgentControlExposure, StudioThreadSpec};

pub fn specification(
    id: &str,
    route: pl_model::config::ResolvedModelRoute,
    root: &std::path::Path,
) -> StudioThreadSpec {
    StudioThreadSpec {
        context_preparation: None,
        agent_controls: AgentControlExposure::Disabled,
        execution: pl_core::thread::input::InputDriverOptions {
            max_model_steps: std::num::NonZeroU32::new(8).unwrap(),
        },
        id: id.into(),
        parent_id: None,
        route,
        hosted_tools: vec![],
        history: vec![],
        initial_context: vec![],
        initial_extensions: Default::default(),
        tools: vec![],
        resources: pl_core::context::ResourceAccess::new(
            pl_studio_runtime::resource_store::FileResourceStore::new(root.to_owned()),
        ),
        capacity: Default::default(),
        cold_store: None,
    }
}
