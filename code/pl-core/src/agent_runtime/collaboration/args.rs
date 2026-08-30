//! 协作工具的输入参数 DTO 与 wire 枚举。

use serde::Deserialize;
use serde_json::Value;

use super::super::AgentProgressStage;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SpawnArgs {
    pub(super) message: String,
    pub(super) profile_id: String,
    #[serde(default)]
    pub(super) fork_turns: ForkTurns,
    #[serde(default)]
    pub(super) metadata: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProgressArgs {
    pub(super) stage: ProgressStage,
    pub(super) summary: String,
    pub(super) next_step: String,
    #[serde(default)]
    pub(super) detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ProgressStage {
    Exploring,
    Implementing,
    Verifying,
    Blocked,
    ReadyForCompletion,
}

impl From<ProgressStage> for AgentProgressStage {
    fn from(value: ProgressStage) -> Self {
        match value {
            ProgressStage::Exploring => Self::Exploring,
            ProgressStage::Implementing => Self::Implementing,
            ProgressStage::Verifying => Self::Verifying,
            ProgressStage::Blocked => Self::Blocked,
            ProgressStage::ReadyForCompletion => Self::ReadyForCompletion,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SendMessageArgs {
    pub(super) target: String,
    pub(super) message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetArgs {
    pub(super) target: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WaitArgs {
    pub(super) targets: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SubmissionsArgs {
    pub(super) target: String,
    #[serde(default)]
    pub(super) offset: Option<usize>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyArgs {}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ForkTurns {
    #[default]
    None,
    All,
    Last(usize),
}
