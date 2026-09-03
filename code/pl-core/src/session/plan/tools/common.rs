use pl_protocol::{AgentSessionPlanState, MessagePresentation, PureError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{TurnWorkingSetChange, TurnWorkingSetHandle};

use super::super::{AgentSessionPlanHandle, AgentSessionPlanMachine, AgentSessionPlanOptions};

#[derive(Debug, Clone)]
pub(crate) struct AgentSessionPlanToolBinding {
    handle: AgentSessionPlanHandle,
    options: AgentSessionPlanOptions,
}

impl AgentSessionPlanToolBinding {
    pub(crate) fn new(options: AgentSessionPlanOptions) -> Self {
        Self {
            handle: AgentSessionPlanHandle::new(AgentSessionPlanState::default())
                .expect("default AgentSession Plan state is valid"),
            options,
        }
    }

    pub(crate) fn replace(&self, state: AgentSessionPlanState) -> Result<(), PureError> {
        self.handle
            .replace(state)
            .map_err(|error| PureError::ConfigError(error.to_string()))
    }
}

#[derive(Debug, Clone)]
pub(super) struct AgentSessionPlanToolRuntime {
    working_set: TurnWorkingSetHandle,
    handle: AgentSessionPlanHandle,
    options: AgentSessionPlanOptions,
}

impl AgentSessionPlanToolRuntime {
    pub(crate) fn new(
        working_set: TurnWorkingSetHandle,
        binding: AgentSessionPlanToolBinding,
    ) -> Self {
        Self {
            working_set,
            handle: binding.handle,
            options: binding.options,
        }
    }

    pub fn working_state(&self) -> Option<AgentSessionPlanState> {
        self.working_set.plan()
    }

    pub fn read_state(&self) -> AgentSessionPlanState {
        self.handle.state()
    }

    pub fn read_machine(&self) -> Result<AgentSessionPlanMachine, PureError> {
        AgentSessionPlanMachine::new(self.read_state())
            .map_err(|error| PureError::ConfigError(error.to_string()))
    }

    pub fn mutate<R>(
        &self,
        mutate: impl FnOnce(&mut AgentSessionPlanMachine) -> (R, bool),
    ) -> Result<R, PureError> {
        let previous = self.handle.state();
        let (result, state) = self
            .handle
            .mutate(mutate)
            .map_err(|error| PureError::ConfigError(error.to_string()))?;
        if let Some(state) = state {
            if let Err(error) = super::super::validate_session_state_size(&state) {
                self.handle
                    .replace(previous)
                    .map_err(|restore| PureError::ConfigError(restore.to_string()))?;
                return Err(error);
            }
            if let Err(error) = self
                .working_set
                .apply(TurnWorkingSetChange::ReplacePlan(Some(state)))
            {
                self.handle
                    .replace(previous)
                    .map_err(|restore| PureError::ConfigError(restore.to_string()))?;
                return Err(error);
            }
        }
        Ok(result)
    }

    pub fn restore(&self, state: Option<AgentSessionPlanState>) -> Result<(), PureError> {
        self.handle
            .replace(state.clone().unwrap_or_default())
            .map_err(|error| PureError::ConfigError(error.to_string()))?;
        self.working_set
            .apply(TurnWorkingSetChange::ReplacePlan(state))
    }

    pub const fn submitted_plan_presentation(&self) -> MessagePresentation {
        self.options.submitted_plan_presentation()
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

pub(super) fn operation_id(identity: &crate::ToolCallIdentity) -> String {
    let call = if identity.call_id.trim().is_empty() {
        identity.item_id.as_str()
    } else {
        identity.call_id.as_str()
    };
    format!("{}/{}/{}", identity.session_id, identity.turn_id, call)
}
