//! AgentSession 内固定 Plan 状态机、持久化投影与工具。

mod machine;

pub use machine::{
    AgentSessionPlanConfirmationDecision, AgentSessionPlanError, AgentSessionPlanMachine,
    AgentSessionPlanResolveCommand, AgentSessionPlanRestartCommand, AgentSessionPlanSubmitCommand,
    available_transitions, validate_plan,
};

use pl_protocol::{
    AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID, AgentSessionPlanState, MessagePresentation,
    PureError,
};

pub const MAX_PLAN_SESSION_STATE_BYTES: usize = 128 * 1024;

/// AgentSession Plan 注册选项；不包含或暴露 session-local Arc handle。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSessionPlanOptions {
    submitted_plan_presentation: MessagePresentation,
}

impl Default for AgentSessionPlanOptions {
    fn default() -> Self {
        Self {
            submitted_plan_presentation: MessagePresentation::Hidden,
        }
    }
}

impl AgentSessionPlanOptions {
    /// 预设 Plan confirmation 决议后完整 Plan 用户消息的 GUI presentation。
    pub const fn with_submitted_plan_presentation(
        mut self,
        presentation: MessagePresentation,
    ) -> Self {
        self.submitted_plan_presentation = presentation;
        self
    }

    pub const fn submitted_plan_presentation(&self) -> MessagePresentation {
        self.submitted_plan_presentation
    }
}

/// 验证 Plan 热状态的领域不变量和持久化大小边界。
pub fn validate_session_state_size(state: &AgentSessionPlanState) -> Result<(), PureError> {
    AgentSessionPlanMachine::new(state.clone())
        .map_err(|error| PureError::ConfigError(error.to_string()))?;
    let bytes = serde_json::to_vec(state)?.len();
    if bytes > MAX_PLAN_SESSION_STATE_BYTES {
        return Err(PureError::ConfigError(format!(
            "AgentSession Plan state exceeds {MAX_PLAN_SESSION_STATE_BYTES} bytes"
        )));
    }
    Ok(())
}

/// Interprets the standard plan confirmation answer.
///
/// # Errors
/// Rejects missing answers or answers with no confirmation decision.
pub fn confirmation_decision(
    resolution: &pl_protocol::UserInputResolution,
) -> Result<AgentSessionPlanConfirmationDecision, String> {
    let answer = resolution
        .answers
        .get(AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID)
        .ok_or_else(|| {
            format!(
                "Plan confirmation answer `{AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID}` is missing"
            )
        })?;
    let approved = answer.answers.iter().any(|value| value == "Approve");
    let revision = answer.answers.iter().any(|value| value == "Revise");
    if approved && revision {
        return Err("Plan confirmation cannot select both Approve and Revise".to_string());
    }
    if approved {
        return Ok(AgentSessionPlanConfirmationDecision::Approve);
    }
    let feedback = answer
        .answers
        .iter()
        .filter(|value| value.as_str() != "Revise")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if revision || !feedback.is_empty() {
        return Ok(AgentSessionPlanConfirmationDecision::RequestRevision { feedback });
    }
    Err("Plan confirmation must select Approve, Revise, or provide revision feedback".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{UserInputAnswer, UserInputResolution};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn confirmation_answers_are_typed_plan_decisions() {
        let resolution = UserInputResolution {
            answers: HashMap::from([(
                AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID.to_string(),
                UserInputAnswer {
                    answers: vec!["Revise".to_string(), "Add rollback.".to_string()],
                },
            )]),
        };
        assert_eq!(
            confirmation_decision(&resolution).unwrap(),
            AgentSessionPlanConfirmationDecision::RequestRevision {
                feedback: "Add rollback.".to_string(),
            }
        );
    }
}
