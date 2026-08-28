mod agent;
mod inference;
mod plan;
mod text;
mod thinking;
mod tool;
mod turn;

use std::fmt;

pub use agent::*;
pub use inference::*;
use pl_protocol::{TokenUsageSnapshot, TurnState};
pub use plan::*;
use serde::{Deserialize, Serialize};
pub use text::*;
pub use thinking::*;
pub use tool::*;
pub use turn::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TracePart {
    turn_id: String,
    item_id: String,
    started_sequence: u64,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    #[serde(default, skip_serializing_if = "TracePartSource::is_model")]
    source: TracePartSource,
    state: TracePartState,
}

impl TracePart {
    pub fn new(
        turn_id: String,
        item_id: String,
        started_sequence: u64,
        timestamp: i64,
        source: TracePartSource,
        state: TracePartState,
    ) -> Self {
        Self {
            turn_id,
            item_id,
            started_sequence,
            revision: 0,
            created_at: timestamp,
            updated_at: timestamp,
            source,
            state,
        }
    }

    pub fn streaming_text(
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        sequence: u64,
        channel: TraceTextChannel,
        timestamp: i64,
    ) -> Self {
        Self::new(
            turn_id.into(),
            item_id.into(),
            sequence,
            timestamp,
            TracePartSource::Model,
            TracePartState::Text(TraceTextPart::streaming(channel, String::new())),
        )
    }

    pub fn completed_text(
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        sequence: u64,
        channel: TraceTextChannel,
        content: impl Into<String>,
        attachments: Vec<TraceAttachment>,
        timestamp: i64,
    ) -> Self {
        Self::new(
            turn_id.into(),
            item_id.into(),
            sequence,
            timestamp,
            TracePartSource::Model,
            TracePartState::Text(TraceTextPart::completed(
                channel,
                content.into(),
                attachments,
            )),
        )
    }

    pub fn runtime_commentary(
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        sequence: u64,
        content: impl Into<String>,
        timestamp: i64,
    ) -> Self {
        Self::new(
            turn_id.into(),
            item_id.into(),
            sequence,
            timestamp,
            TracePartSource::Runtime,
            TracePartState::Text(TraceTextPart::streaming(
                TraceTextChannel::Commentary,
                content.into(),
            )),
        )
    }

    pub fn streaming_thinking(
        turn_id: String,
        item_id: String,
        sequence: u64,
        timestamp: i64,
    ) -> Self {
        Self::new(
            turn_id,
            item_id,
            sequence,
            timestamp,
            TracePartSource::Model,
            TracePartState::Thinking(TraceThinkingPart::streaming()),
        )
    }

    pub fn started_plan(turn_id: String, item_id: String, sequence: u64, timestamp: i64) -> Self {
        Self::new(
            turn_id,
            item_id,
            sequence,
            timestamp,
            TracePartSource::Model,
            TracePartState::Plan(TracePlanPart::started()),
        )
    }

    pub fn started_tool(
        turn_id: String,
        item_id: String,
        sequence: u64,
        timestamp: i64,
        invocation: TraceToolInvocation,
    ) -> Self {
        Self::new(
            turn_id,
            item_id,
            sequence,
            timestamp,
            TracePartSource::Model,
            TracePartState::Tool(TraceToolPart::started(invocation)),
        )
    }

    pub fn streaming_tool(
        turn_id: String,
        item_id: String,
        sequence: u64,
        timestamp: i64,
        invocation: TraceToolInvocation,
    ) -> Self {
        Self::new(
            turn_id,
            item_id,
            sequence,
            timestamp,
            TracePartSource::Model,
            TracePartState::Tool(TraceToolPart::streaming(invocation)),
        )
    }

    pub fn running_inference(
        turn_id: String,
        item_id: String,
        sequence: u64,
        timestamp: i64,
        inference_id: String,
        model: String,
    ) -> Self {
        Self::new(
            turn_id,
            item_id,
            sequence,
            timestamp,
            TracePartSource::Model,
            TracePartState::Inference(TraceInferencePart::running(inference_id, model)),
        )
    }

    pub fn turn(
        turn_id: String,
        item_id: String,
        sequence: u64,
        timestamp: i64,
        state: TurnState,
    ) -> Self {
        Self::new(
            turn_id,
            item_id,
            sequence,
            timestamp,
            TracePartSource::Runtime,
            TracePartState::Turn(TraceTurnPart::new(state)),
        )
    }

    pub fn agent(
        turn_id: String,
        item_id: String,
        sequence: u64,
        timestamp: i64,
        agent: TraceAgentPart,
    ) -> Self {
        Self::new(
            turn_id,
            item_id,
            sequence,
            timestamp,
            TracePartSource::Runtime,
            TracePartState::Agent(agent),
        )
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub fn started_sequence(&self) -> u64 {
        self.started_sequence
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Advances an open item to the revision reached by externally published deltas.
    ///
    /// Tool output observers publish deltas while the dispatcher retains its own item copy;
    /// the authoritative terminal snapshot must first join that delta revision before applying
    /// the terminal transition.
    pub fn synchronize_open_revision(
        &mut self,
        revision: u64,
        updated_at: i64,
    ) -> Result<(), &'static str> {
        if self.is_terminal() {
            return Err("cannot synchronize a terminal trace part");
        }
        if revision < self.revision {
            return Err("cannot move a trace part revision backwards");
        }
        self.revision = revision;
        self.updated_at = self.updated_at.max(updated_at);
        Ok(())
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }

    pub fn source(&self) -> TracePartSource {
        self.source
    }

    pub fn state(&self) -> &TracePartState {
        &self.state
    }

    pub fn kind(&self) -> TracePartKind {
        self.state.kind()
    }

    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    pub fn failure(&self) -> Option<&str> {
        self.state.failure()
    }

    pub fn tool(&self) -> Option<&TraceToolPart> {
        match &self.state {
            TracePartState::Tool(tool) => Some(tool),
            TracePartState::Text(_)
            | TracePartState::Thinking(_)
            | TracePartState::Agent(_)
            | TracePartState::Turn(_)
            | TracePartState::Inference(_)
            | TracePartState::Plan(_) => None,
        }
    }

    pub fn text(&self) -> Option<&TraceTextPart> {
        match &self.state {
            TracePartState::Text(text) => Some(text),
            TracePartState::Thinking(_)
            | TracePartState::Tool(_)
            | TracePartState::Agent(_)
            | TracePartState::Turn(_)
            | TracePartState::Inference(_)
            | TracePartState::Plan(_) => None,
        }
    }

    pub fn thinking(&self) -> Option<&TraceThinkingPart> {
        match &self.state {
            TracePartState::Thinking(thinking) => Some(thinking),
            TracePartState::Text(_)
            | TracePartState::Tool(_)
            | TracePartState::Agent(_)
            | TracePartState::Turn(_)
            | TracePartState::Inference(_)
            | TracePartState::Plan(_) => None,
        }
    }

    pub fn plan(&self) -> Option<&TracePlanPart> {
        match &self.state {
            TracePartState::Plan(plan) => Some(plan),
            TracePartState::Text(_)
            | TracePartState::Thinking(_)
            | TracePartState::Tool(_)
            | TracePartState::Agent(_)
            | TracePartState::Turn(_)
            | TracePartState::Inference(_) => None,
        }
    }

    pub fn decide(
        &self,
        command: &TracePartCommand,
    ) -> Result<TracePartTransitionDecision, TracePartTransitionError> {
        if command.item_id != self.item_id {
            return Err(self.error(command, TracePartTransitionErrorKind::WrongItem));
        }
        if command.expected_revision != self.revision {
            return Err(self.error(command, TracePartTransitionErrorKind::StaleRevision));
        }
        if self.is_terminal() {
            return Err(self.error(command, TracePartTransitionErrorKind::TerminalState));
        }
        let next_state = self
            .state
            .transition(&command.action)
            .map_err(|reason| self.error(command, TracePartTransitionErrorKind::Illegal(reason)))?;
        let changed = next_state != self.state;
        Ok(TracePartTransitionDecision {
            next_state,
            next_revision: self.revision + u64::from(changed),
            updated_at: if changed {
                command.updated_at
            } else {
                self.updated_at
            },
            delta: command.action.delta().cloned(),
            changed,
        })
    }

    pub fn apply(
        &mut self,
        command: TracePartCommand,
    ) -> Result<TracePartTransitionDecision, TracePartTransitionError> {
        let decision = self.decide(&command)?;
        self.state = decision.next_state.clone();
        self.revision = decision.next_revision;
        self.updated_at = decision.updated_at;
        Ok(decision)
    }

    pub fn command(&self, updated_at: i64, action: TracePartAction) -> TracePartCommand {
        TracePartCommand {
            item_id: self.item_id.clone(),
            expected_revision: self.revision,
            updated_at,
            action,
        }
    }

    /// Atomically applies one streaming delta and returns its canonical event.
    ///
    /// A transition that does not change the typed part produces no event, so
    /// downstream ledgers never observe a revision-less delta.
    pub fn apply_delta(
        &mut self,
        updated_at: i64,
        delta: TraceDelta,
    ) -> Result<Option<TracePartDeltaEvent>, TracePartTransitionError> {
        let decision =
            self.apply(self.command(updated_at, TracePartAction::Append(delta.clone())))?;
        if !decision.changed {
            return Ok(None);
        }
        Ok(Some(TracePartDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id: self.item_id.clone(),
            started_sequence: self.started_sequence,
            revision: self.revision,
            created_at: self.created_at,
            updated_at: self.updated_at,
            delta,
        }))
    }

    fn error(
        &self,
        command: &TracePartCommand,
        kind: TracePartTransitionErrorKind,
    ) -> TracePartTransitionError {
        TracePartTransitionError {
            item_id: self.item_id.clone(),
            revision: self.revision,
            current_kind: self.kind(),
            command: command.action.name(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TracePartState {
    Text(TraceTextPart),
    Thinking(TraceThinkingPart),
    Tool(TraceToolPart),
    Agent(TraceAgentPart),
    Turn(TraceTurnPart),
    Inference(TraceInferencePart),
    Plan(TracePlanPart),
}

impl TracePartState {
    pub fn kind(&self) -> TracePartKind {
        match self {
            Self::Text(_) => TracePartKind::Text,
            Self::Thinking(_) => TracePartKind::Thinking,
            Self::Tool(_) => TracePartKind::Tool,
            Self::Agent(_) => TracePartKind::Agent,
            Self::Turn(_) => TracePartKind::Turn,
            Self::Inference(_) => TracePartKind::Inference,
            Self::Plan(_) => TracePartKind::Plan,
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Text(part) => part.state().is_terminal(),
            Self::Thinking(part) => part.state().is_terminal(),
            Self::Tool(part) => part.state().is_terminal(),
            Self::Agent(part) => part.state().is_terminal(),
            Self::Turn(part) => part.state().is_terminal(),
            Self::Inference(part) => part.state().is_terminal(),
            Self::Plan(part) => part.state().is_terminal(),
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Text(part) => part.state().failure(),
            Self::Thinking(part) => part.state().failure(),
            Self::Tool(part) => part.state().failure(),
            Self::Agent(part) => part.state().failure(),
            Self::Turn(part) => part
                .state()
                .failure()
                .map(|failure| failure.message.as_str()),
            Self::Inference(part) => part.state().failure(),
            Self::Plan(part) => part.state().failure(),
        }
    }

    fn transition(&self, action: &TracePartAction) -> Result<Self, &'static str> {
        match (self, action) {
            (Self::Text(part), TracePartAction::Append(TraceDelta::Text { delta, .. })) => {
                part.append(delta).map(Self::Text)
            }
            (
                Self::Thinking(part),
                TracePartAction::Append(TraceDelta::Thinking { chunk_index, delta }),
            ) => part.append_summary(*chunk_index, delta).map(Self::Thinking),
            (
                Self::Thinking(part),
                TracePartAction::Append(TraceDelta::ReasoningContent { chunk_index, delta }),
            ) => part.append_content(*chunk_index, delta).map(Self::Thinking),
            (Self::Tool(part), TracePartAction::Append(TraceDelta::ToolArguments { delta })) => {
                part.append_arguments(delta).map(Self::Tool)
            }
            (Self::Tool(part), TracePartAction::Append(TraceDelta::ToolResult { delta })) => {
                part.append_result(delta).map(Self::Tool)
            }
            (Self::Plan(part), TracePartAction::Append(TraceDelta::Plan { delta })) => {
                part.append(delta).map(Self::Plan)
            }
            (
                Self::Text(part),
                TracePartAction::Complete(TracePartCompletion::Text {
                    authoritative_content,
                }),
            ) => part.complete(authoritative_content.clone()).map(Self::Text),
            (
                Self::Thinking(part),
                TracePartAction::Complete(TracePartCompletion::Thinking {
                    authoritative_summary,
                }),
            ) => part
                .complete(authoritative_summary.clone())
                .map(Self::Thinking),
            (Self::Tool(part), TracePartAction::Complete(TracePartCompletion::Tool { output })) => {
                part.succeed(output.clone()).map(Self::Tool)
            }
            (
                Self::Agent(part),
                TracePartAction::Complete(TracePartCompletion::Agent { state }),
            ) => part.transition(state.clone()).map(Self::Agent),
            (Self::Turn(part), TracePartAction::Complete(TracePartCompletion::Turn { state })) => {
                part.transition(state.clone()).map(Self::Turn)
            }
            (
                Self::Inference(part),
                TracePartAction::Complete(TracePartCompletion::Inference { usage }),
            ) => part.complete(usage.clone()).map(Self::Inference),
            (
                Self::Plan(part),
                TracePartAction::Complete(TracePartCompletion::Plan { content }),
            ) => part.complete(content.clone()).map(Self::Plan),
            (Self::Text(part), TracePartAction::Fail { error, .. }) => {
                part.fail(error.clone()).map(Self::Text)
            }
            (Self::Thinking(part), TracePartAction::Fail { error, .. }) => {
                part.fail(error.clone()).map(Self::Thinking)
            }
            (Self::Tool(part), TracePartAction::Fail { error, tool_kind }) => part
                .fail(TraceToolFailure::new(*tool_kind, error.clone()), None)
                .map(Self::Tool),
            (Self::Agent(part), TracePartAction::Fail { error, .. }) => part
                .transition(TraceAgentState::Failed(FailedTraceAgent::new(
                    error.clone(),
                )))
                .map(Self::Agent),
            (Self::Inference(part), TracePartAction::Fail { error, .. }) => {
                part.fail(error.clone()).map(Self::Inference)
            }
            (Self::Plan(part), TracePartAction::Fail { error, .. }) => {
                part.fail(error.clone()).map(Self::Plan)
            }
            (Self::Tool(part), TracePartAction::FailTool { failure, output }) => {
                part.fail(failure.clone(), output.clone()).map(Self::Tool)
            }
            (Self::Tool(part), TracePartAction::DenyTool { reason }) => {
                part.deny(reason.clone()).map(Self::Tool)
            }
            (Self::Tool(part), TracePartAction::EnterToolPhase { phase }) => {
                part.enter(*phase).map(Self::Tool)
            }
            (Self::Tool(part), TracePartAction::UpdateToolInvocation { invocation }) => {
                part.update_invocation(invocation.clone()).map(Self::Tool)
            }
            (Self::Inference(part), TracePartAction::UpdateInferenceModel { model }) => {
                part.set_model(model.clone()).map(Self::Inference)
            }
            (Self::Agent(part), TracePartAction::TransitionAgent { state }) => {
                part.transition(state.clone()).map(Self::Agent)
            }
            (Self::Turn(part), TracePartAction::TransitionTurn { state }) => {
                part.transition(state.clone()).map(Self::Turn)
            }
            (Self::Text(part), TracePartAction::Cancel { reason }) => {
                part.cancel(reason.clone()).map(Self::Text)
            }
            (Self::Thinking(part), TracePartAction::Cancel { reason }) => {
                part.cancel(reason.clone()).map(Self::Thinking)
            }
            (Self::Tool(part), TracePartAction::Cancel { reason }) => part
                .cancel(TraceToolCancellationCause::TurnCancelled {
                    reason: reason.clone(),
                })
                .map(Self::Tool),
            (Self::Agent(part), TracePartAction::Cancel { reason }) => part
                .transition(TraceAgentState::Cancelled(CancelledTraceAgent::new(
                    reason.clone(),
                )))
                .map(Self::Agent),
            (Self::Inference(part), TracePartAction::Cancel { reason }) => {
                part.cancel(reason.clone()).map(Self::Inference)
            }
            (Self::Plan(part), TracePartAction::Cancel { reason }) => {
                part.cancel(reason.clone()).map(Self::Plan)
            }
            (Self::Turn(_), TracePartAction::Fail { .. } | TracePartAction::Cancel { .. }) => {
                Err("turn trace must transition with canonical TurnState")
            }
            _ => Err("command does not match trace part kind"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TracePartKind {
    Text,
    Thinking,
    Tool,
    Agent,
    Turn,
    Inference,
    Plan,
}

impl TracePartKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Thinking => "thinking",
            Self::Tool => "tool",
            Self::Agent => "agent",
            Self::Turn => "turn",
            Self::Inference => "inference",
            Self::Plan => "plan",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TracePartSource {
    #[default]
    Model,
    Runtime,
}

impl TracePartSource {
    fn is_model(&self) -> bool {
        matches!(self, Self::Model)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceAttachment {
    pub id: String,
    pub modality: TraceAttachmentModality,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    pub byte_size: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TraceAttachmentModality {
    Image,
    Video,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracePartCommand {
    pub item_id: String,
    pub expected_revision: u64,
    pub updated_at: i64,
    pub action: TracePartAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TracePartAction {
    Append(TraceDelta),
    Complete(TracePartCompletion),
    Fail {
        error: String,
        tool_kind: TraceToolFailureKind,
    },
    FailTool {
        failure: TraceToolFailure,
        output: Option<TraceToolOutput>,
    },
    Cancel {
        reason: String,
    },
    DenyTool {
        reason: String,
    },
    EnterToolPhase {
        phase: TraceToolActivePhase,
    },
    UpdateToolInvocation {
        invocation: TraceToolInvocation,
    },
    UpdateInferenceModel {
        model: String,
    },
    TransitionAgent {
        state: TraceAgentState,
    },
    TransitionTurn {
        state: TurnState,
    },
}

impl TracePartAction {
    fn name(&self) -> &'static str {
        match self {
            Self::Append(_) => "append",
            Self::Complete(_) => "complete",
            Self::Fail { .. } => "fail",
            Self::FailTool { .. } => "failTool",
            Self::Cancel { .. } => "cancel",
            Self::DenyTool { .. } => "denyTool",
            Self::EnterToolPhase { .. } => "enterToolPhase",
            Self::UpdateToolInvocation { .. } => "updateToolInvocation",
            Self::UpdateInferenceModel { .. } => "updateInferenceModel",
            Self::TransitionAgent { .. } => "transitionAgent",
            Self::TransitionTurn { .. } => "transitionTurn",
        }
    }

    fn delta(&self) -> Option<&TraceDelta> {
        match self {
            Self::Append(delta) => Some(delta),
            Self::Complete(_)
            | Self::Fail { .. }
            | Self::FailTool { .. }
            | Self::Cancel { .. }
            | Self::DenyTool { .. }
            | Self::EnterToolPhase { .. }
            | Self::UpdateToolInvocation { .. }
            | Self::UpdateInferenceModel { .. }
            | Self::TransitionAgent { .. }
            | Self::TransitionTurn { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TracePartCompletion {
    Text {
        authoritative_content: Option<String>,
    },
    Thinking {
        authoritative_summary: Option<Vec<String>>,
    },
    Tool {
        output: TraceToolOutput,
    },
    Agent {
        state: TraceAgentState,
    },
    Turn {
        state: TurnState,
    },
    Inference {
        usage: TokenUsageSnapshot,
    },
    Plan {
        content: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracePartTransitionDecision {
    pub next_state: TracePartState,
    pub next_revision: u64,
    pub updated_at: i64,
    pub delta: Option<TraceDelta>,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracePartTransitionError {
    pub item_id: String,
    pub revision: u64,
    pub current_kind: TracePartKind,
    pub command: &'static str,
    pub kind: TracePartTransitionErrorKind,
}

impl fmt::Display for TracePartTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "trace part {} at revision {} ({:?}) rejected command {}: {}",
            self.item_id, self.revision, self.current_kind, self.command, self.kind
        )
    }
}

impl std::error::Error for TracePartTransitionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TracePartTransitionErrorKind {
    WrongItem,
    StaleRevision,
    TerminalState,
    Illegal(&'static str),
}

impl fmt::Display for TracePartTransitionErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongItem => formatter.write_str("command targets a different item"),
            Self::StaleRevision => formatter.write_str("command revision is stale"),
            Self::TerminalState => formatter.write_str("terminal state is immutable"),
            Self::Illegal(reason) => formatter.write_str(reason),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TraceDelta {
    Text {
        channel: TraceTextChannel,
        delta: String,
    },
    Thinking {
        chunk_index: u32,
        delta: String,
    },
    ReasoningContent {
        chunk_index: u32,
        delta: String,
    },
    ToolArguments {
        delta: String,
    },
    ToolResult {
        delta: String,
    },
    Plan {
        delta: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TracePartDeltaEvent {
    pub turn_id: String,
    pub item_id: String,
    pub started_sequence: u64,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub delta: TraceDelta,
}

impl TracePartDeltaEvent {
    pub fn kind(&self) -> TracePartKind {
        match &self.delta {
            TraceDelta::Text { .. } => TracePartKind::Text,
            TraceDelta::Thinking { .. } | TraceDelta::ReasoningContent { .. } => {
                TracePartKind::Thinking
            }
            TraceDelta::ToolArguments { .. } | TraceDelta::ToolResult { .. } => TracePartKind::Tool,
            TraceDelta::Plan { .. } => TracePartKind::Plan,
        }
    }

    pub fn running_tool_result(
        turn_id: String,
        item_id: String,
        revision: u64,
        timestamp: i64,
        delta: String,
    ) -> Self {
        Self {
            turn_id,
            item_id,
            started_sequence: 0,
            revision,
            created_at: timestamp,
            updated_at: timestamp,
            delta: TraceDelta::ToolResult { delta },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta_requires_matching_identity_revision_and_streaming_state() {
        let mut item = TracePart::streaming_text("turn-1", "item-1", 1, TraceTextChannel::Final, 7);
        let command = item.command(
            8,
            TracePartAction::Append(TraceDelta::Text {
                channel: TraceTextChannel::Final,
                delta: "done".to_string(),
            }),
        );
        let decision = item.apply(command).expect("append text");
        assert_eq!(decision.next_revision, 1);
        assert_eq!(item.revision(), 1);

        let completion = item.command(
            9,
            TracePartAction::Complete(TracePartCompletion::Text {
                authoritative_content: None,
            }),
        );
        item.apply(completion).expect("complete text");
        assert!(item.is_terminal());
        let rejected = item
            .apply(item.command(
                10,
                TracePartAction::Append(TraceDelta::Text {
                    channel: TraceTextChannel::Final,
                    delta: " late".to_string(),
                }),
            ))
            .expect_err("terminal text rejects delta");
        assert_eq!(rejected.kind, TracePartTransitionErrorKind::TerminalState);
    }

    #[test]
    fn tool_terminal_payload_round_trips_without_parallel_optional_status_fields() {
        let mut item = TracePart::started_tool(
            "turn-1".to_string(),
            "tool-1".to_string(),
            1,
            7,
            TraceToolInvocation::new("tool-1".to_string(), "exec".to_string(), "{}".to_string()),
        );
        item.apply(item.command(
            8,
            TracePartAction::EnterToolPhase {
                phase: TraceToolActivePhase::Running,
            },
        ))
        .expect("run tool");
        item.apply(item.command(
            9,
            TracePartAction::Complete(TracePartCompletion::Tool {
                output: TraceToolOutput::new("ok".to_string()),
            }),
        ))
        .expect("complete tool");

        let json = serde_json::to_string(&item).expect("serialize");
        let restored: TracePart = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, item);
        assert!(matches!(
            restored.tool().map(TraceToolPart::state),
            Some(TraceToolState::Succeeded(_))
        ));
    }

    #[test]
    fn running_tool_result_deltas_fold_until_terminal_authoritative_replace() {
        let invocation =
            TraceToolInvocation::new("tool-1".to_string(), "exec".to_string(), "{}".to_string());
        let mut item =
            TracePart::started_tool("turn-1".to_string(), "tool-1".to_string(), 1, 7, invocation);
        item.apply(item.command(
            8,
            TracePartAction::EnterToolPhase {
                phase: TraceToolActivePhase::Running,
            },
        ))
        .expect("tool enters running phase");
        for delta in ["out", "[stderr] err"] {
            item.apply(item.command(
                9,
                TracePartAction::Append(TraceDelta::ToolResult {
                    delta: delta.to_string(),
                }),
            ))
            .expect("running tool accepts output delta");
        }
        let TraceToolState::Running(running) = item.tool().unwrap().state() else {
            panic!("tool must remain running while output streams");
        };
        assert_eq!(running.streamed_output(), "out[stderr] err");

        item.apply(item.command(
            10,
            TracePartAction::Complete(TracePartCompletion::Tool {
                output: TraceToolOutput::new("canonical result".to_string()),
            }),
        ))
        .expect("terminal output replaces the live overlay");
        assert_eq!(
            item.tool()
                .and_then(TraceToolPart::terminal_output)
                .map(TraceToolOutput::result),
            Some("canonical result")
        );
    }
}
