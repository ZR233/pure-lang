use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::interaction::{
    InteractionResolution, InteractionStatus, InteractionTransitionError, UserInputResolution,
};
use crate::{UserInputAnswer, UserQuestion};

use super::{
    CancelInteraction, CancelledInteractionState, ExpireInteraction, ExpiredInteractionState,
    PendingInteractionState, ReopenRecoveredInteraction,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputInteraction {
    questions: Vec<UserQuestion>,
    state: UserInputState,
}

impl UserInputInteraction {
    pub fn new(questions: Vec<UserQuestion>, operation_id: String) -> Self {
        Self {
            questions,
            state: UserInputState::Pending(PendingInteractionState::new(operation_id)),
        }
    }
    pub fn questions(&self) -> &[UserQuestion] {
        &self.questions
    }
    pub fn state(&self) -> &UserInputState {
        &self.state
    }
    pub fn status(&self) -> InteractionStatus {
        self.state.status()
    }
    pub fn resolution(&self) -> Option<InteractionResolution> {
        match &self.state {
            UserInputState::Resolved(value) => {
                Some(InteractionResolution::UserInput(UserInputResolution {
                    answers: value.answers.clone(),
                }))
            }
            UserInputState::Pending(_)
            | UserInputState::Cancelled(_)
            | UserInputState::Expired(_) => None,
        }
    }
    pub(crate) fn decide(
        &self,
        command: UserInputCommand,
    ) -> Result<Self, InteractionTransitionError> {
        let state = self.state.decide(command)?;
        Ok(Self {
            questions: self.questions.clone(),
            state,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum UserInputState {
    Pending(PendingInteractionState),
    Resolved(ResolvedUserInputState),
    Cancelled(CancelledInteractionState),
    Expired(ExpiredInteractionState),
}

impl UserInputState {
    fn status(&self) -> InteractionStatus {
        match self {
            Self::Pending(_) => InteractionStatus::Pending,
            Self::Resolved(_) => InteractionStatus::Resolved,
            Self::Cancelled(_) => InteractionStatus::Cancelled,
            Self::Expired(_) => InteractionStatus::Expired,
        }
    }
    fn decide(&self, command: UserInputCommand) -> Result<Self, InteractionTransitionError> {
        match (self, command) {
            (Self::Pending(_), UserInputCommand::Resolve(value)) => {
                Ok(Self::Resolved(ResolvedUserInputState::new(value)))
            }
            (Self::Pending(_), UserInputCommand::Cancel(value)) => {
                Ok(Self::Cancelled(CancelledInteractionState::new(
                    value.operation_id,
                    value.cancelled_at,
                    value.reason,
                )))
            }
            (Self::Pending(_), UserInputCommand::Expire(value)) => Ok(Self::Expired(
                ExpiredInteractionState::new(value.operation_id, value.expired_at),
            )),
            (Self::Cancelled(_), UserInputCommand::ReopenRecovered(value)) => Ok(Self::Pending(
                PendingInteractionState::new(value.operation_id),
            )),
            (Self::Resolved(current), UserInputCommand::Resolve(value))
                if current.matches(&value) =>
            {
                Ok(self.clone())
            }
            (_, command) => Err(InteractionTransitionError::new(
                crate::InteractionKind::UserInput,
                self.status(),
                command.name(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedUserInputState {
    operation_id: String,
    resolved_at: i64,
    answers: HashMap<String, UserInputAnswer>,
}

impl ResolvedUserInputState {
    fn new(value: ResolveUserInput) -> Self {
        Self {
            operation_id: value.operation_id,
            resolved_at: value.resolved_at,
            answers: value.answers,
        }
    }
    fn matches(&self, value: &ResolveUserInput) -> bool {
        self.operation_id == value.operation_id
            && self.resolved_at == value.resolved_at
            && self.answers == value.answers
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn resolved_at(&self) -> i64 {
        self.resolved_at
    }
    pub fn answers(&self) -> &HashMap<String, UserInputAnswer> {
        &self.answers
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveUserInput {
    pub interaction_id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub resolved_at: i64,
    pub answers: HashMap<String, UserInputAnswer>,
}

pub(crate) enum UserInputCommand {
    Resolve(ResolveUserInput),
    Cancel(CancelInteraction),
    Expire(ExpireInteraction),
    ReopenRecovered(ReopenRecoveredInteraction),
}
impl UserInputCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Resolve(_) => "resolveUserInput",
            Self::Cancel(_) => "cancel",
            Self::Expire(_) => "expire",
            Self::ReopenRecovered(_) => "reopenRecovered",
        }
    }
}
