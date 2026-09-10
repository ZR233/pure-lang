//! Provider-executed capabilities, independent from the core tool registry.
use crate::completion::ToolSpec;
pub use pl_protocol::HostedWebSearchOptions;

/// A hosted declaration is encoded by the model adapter and never routed to a local executor.
#[derive(Debug, Clone)]
pub enum HostedTool {
    WebSearch(HostedWebSearchOptions),
    ProgrammaticToolCalling,
}

impl HostedTool {
    pub(crate) fn declaration(&self) -> ToolSpec {
        match self {
            Self::WebSearch(options) => ToolSpec::WebSearch {
                options: options.clone(),
            },
            Self::ProgrammaticToolCalling => ToolSpec::ProgrammaticToolCalling,
        }
    }
}
