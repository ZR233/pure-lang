//! Producer-owned business output and its separately frozen model-facing projection.

use crate::context::{ContextContent, OpaquePayload};
use serde::{Deserialize, Serialize};

/// Trusted framework control, separate from arbitrary result content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolControl {
    #[default]
    Continue,
    EndTurn,
    AwaitInteraction,
}

/// Complete tool data and the exact content selected for the original model delivery.
///
/// The Thread supplies call identity and the tool-result role. Neither payload strings nor
/// content values grant authority to end a Turn, approve work, or rewrite a system instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    payload: OpaquePayload,
    context: Vec<ContextContent>,
    #[serde(default)]
    control: ToolControl,
    #[serde(default)]
    extension_mutations: Vec<crate::thread::extensions::ExtensionMutation>,
    #[serde(default)]
    revealed_tools: Vec<String>,
    #[serde(default)]
    interaction: Option<OpaquePayload>,
}

impl ToolOutput {
    /// Freezes both views together; the framework never derives one by parsing the other.
    pub fn new(payload: OpaquePayload, context: Vec<ContextContent>) -> Self {
        Self {
            payload,
            context,
            control: ToolControl::Continue,
            extension_mutations: Vec::new(),
            revealed_tools: Vec::new(),
            interaction: None,
        }
    }

    /// Requests a host interaction; core assigns its identity and validates registered authority.
    pub fn with_interaction(mut self, payload: OpaquePayload) -> Self {
        self.interaction = Some(payload);
        self.control = ToolControl::AwaitInteraction;
        self
    }

    /// Returns the opaque interaction request material, if supplied.
    pub fn interaction(&self) -> Option<&OpaquePayload> {
        self.interaction.as_ref()
    }

    /// Proposes tool discovery changes, validated against explicit registration authority.
    pub fn with_revealed_tools(mut self, ids: Vec<String>) -> Self {
        self.revealed_tools = ids;
        self
    }

    /// Stable tool identities requested for the next model step.
    pub fn revealed_tools(&self) -> &[String] {
        &self.revealed_tools
    }

    /// Proposes opaque state changes to commit atomically with this result, subject to registration authority.
    pub fn with_extension_mutations(
        mut self,
        mutations: Vec<crate::thread::extensions::ExtensionMutation>,
    ) -> Self {
        self.extension_mutations = mutations;
        self
    }

    /// Returns proposed framework mutations; payload content alone never mutates state.
    pub fn extension_mutations(&self) -> &[crate::thread::extensions::ExtensionMutation] {
        &self.extension_mutations
    }

    pub(crate) fn append_framework_context(&mut self, content: ContextContent) {
        self.context.push(content);
    }

    /// Requests Turn completion; the Thread validates the frozen registration's control permission.
    pub fn ending_turn(mut self) -> Self {
        self.control = ToolControl::EndTurn;
        self.interaction = None;
        self
    }

    /// Returns framework control without inspecting the dynamic business payload.
    pub fn control(&self) -> ToolControl {
        self.control
    }

    /// Returns complete producer-encoded data for storage or an upper-layer decoder.
    pub fn payload(&self) -> &OpaquePayload {
        &self.payload
    }

    /// Returns the original model projection, including on history replay.
    pub fn context(&self) -> &[ContextContent] {
        &self.context
    }
}
