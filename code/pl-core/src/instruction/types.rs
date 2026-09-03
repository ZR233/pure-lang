use std::collections::BTreeMap;
use std::path::Path;

use pl_model::model::ModelInfo;
use pl_protocol::Message;
use serde::{Deserialize, Serialize};

use crate::config::{InstructionsConfig, SkillsConfig};
use crate::execution_environment::ExecutionEnvironment;
use crate::workspace::WorkspaceInstructions;

#[derive(Debug, Clone)]
pub struct InstructionAssemblyRequest<'a> {
    pub instructions: Option<&'a InstructionsConfig>,
    pub skills: Option<&'a SkillsConfig>,
    pub skill_catalog: Option<&'a crate::skill::SkillCatalog>,
    pub execution_profile: Option<ExecutionInstructionProfile<'a>>,
    pub model: &'a ModelInfo,
    pub workspace_root: &'a Path,
    pub current_dir: &'a Path,
    /// 已由宿主读取的项目说明文档。
    ///
    /// 远程工作区的路径只存在于远程主机，不能再次交给本地文件系统读取；
    /// 宿主若已完成读取，应通过此字段传入 canonical 文档集合。
    pub workspace_documents: Option<&'a WorkspaceInstructions>,
    pub workspace_instructions: Option<&'a str>,
    pub subagent_constraint: Option<&'a str>,
    pub skill_suggestions: Option<SkillSuggestionRequest<'a>>,
    /// Runtime facts for the command execution target. When absent, the local
    /// resolver is used; no login-shell environment variable is consulted.
    pub execution_environment: Option<&'a ExecutionEnvironment>,
}

/// Turn-local task text used to suggest Skill summaries without loading them.
#[derive(Debug, Clone, Copy)]
pub struct SkillSuggestionRequest<'a> {
    pub query: &'a str,
    pub excluded_names: &'a [String],
}

/// 产品层为一次 turn 提供的角色或模式指令。
#[derive(Debug, Clone, Copy)]
pub struct ExecutionInstructionProfile<'a> {
    pub label: &'a str,
    pub instructions: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionBundle {
    pub instructions: String,
    pub prelude_messages: Vec<Message>,
    pub prefix_section_hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstructionSnapshot {
    pub base: InstructionBlock,
    #[serde(default)]
    pub developer: Vec<InstructionBlock>,
    #[serde(default)]
    pub user: Vec<InstructionBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstructionBlock {
    pub source: InstructionSource,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstructionSource {
    pub kind: InstructionSourceKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InstructionSourceKind {
    BuiltInBase,
    ModelBase,
    ConfigBaseOverride,
    ProfileBaseOverride,
    ExecutionProfile,
    Platform,
    ConfigDeveloper,
    ProfileDeveloper,
    Skills,
    ToolGroup,
    SubagentConstraint,
    SubagentForce,
    ConfigUser,
    ProfileUser,
    ProjectDoc,
    WorkspaceFallback,
    SkillInvocation,
    SkillSuggestions,
}

impl InstructionSnapshot {
    pub(super) fn push_developer(&mut self, source: InstructionSource, content: &str) {
        push_non_empty(&mut self.developer, source, content);
    }

    pub(super) fn push_user(&mut self, source: InstructionSource, content: &str) {
        push_non_empty(&mut self.user, source, content);
    }

    /// 添加由 TurnRequest 提供的直接 Skill 调用用户指令块。
    pub(crate) fn push_skill_invocation(&mut self, content: &str) {
        self.push_user(
            InstructionSource::new(InstructionSourceKind::SkillInvocation, "skill invocation"),
            content,
        );
    }
}

impl InstructionSourceKind {
    pub(super) fn is_turn_overlay(self) -> bool {
        matches!(
            self,
            Self::ExecutionProfile
                | Self::Platform
                | Self::Skills
                | Self::ToolGroup
                | Self::SubagentConstraint
                | Self::SubagentForce
                | Self::SkillInvocation
                | Self::SkillSuggestions
        )
    }
}

impl InstructionSource {
    pub(super) fn new(kind: InstructionSourceKind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
            path: None,
        }
    }

    pub(super) fn path(kind: InstructionSourceKind, label: impl Into<String>, path: &Path) -> Self {
        Self {
            kind,
            label: label.into(),
            path: Some(path.display().to_string()),
        }
    }
}

fn push_non_empty(blocks: &mut Vec<InstructionBlock>, source: InstructionSource, content: &str) {
    let content = content.trim();
    if content.is_empty() {
        return;
    }
    blocks.push(InstructionBlock {
        source,
        content: content.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use pl_protocol::MessageRole;

    #[test]
    fn bundle_orders_fixed_layers_from_global_to_workspace() {
        let snapshot = InstructionSnapshot {
            base: InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
                content: "base".to_string(),
            },
            developer: vec![
                InstructionBlock {
                    source: InstructionSource::new(InstructionSourceKind::Platform, "platform"),
                    content: "platform".to_string(),
                },
                InstructionBlock {
                    source: InstructionSource::new(InstructionSourceKind::ExecutionProfile, "mode"),
                    content: "mode".to_string(),
                },
                InstructionBlock {
                    source: InstructionSource::new(InstructionSourceKind::Skills, "skills"),
                    content: "skills".to_string(),
                },
            ],
            user: vec![
                InstructionBlock {
                    source: InstructionSource::new(InstructionSourceKind::ConfigUser, "user"),
                    content: "global user".to_string(),
                },
                InstructionBlock {
                    source: InstructionSource::new(InstructionSourceKind::ProjectDoc, "project"),
                    content: "workspace".to_string(),
                },
            ],
        };

        let bundle = snapshot.to_bundle();

        assert_eq!(bundle.instructions, "base");
        assert_eq!(bundle.prelude_messages.len(), 5);
        assert_eq!(
            bundle
                .prelude_messages
                .iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            vec![
                MessageRole::System,
                MessageRole::User,
                MessageRole::System,
                MessageRole::System,
                MessageRole::User,
            ]
        );
        assert_eq!(
            bundle
                .prelude_messages
                .iter()
                .map(|message| message.content.text_value())
                .map(|text| text.lines().next().unwrap_or_default().to_string())
                .collect::<Vec<_>>(),
            vec![
                "# Global Developer Instructions".to_string(),
                "# Global User Context".to_string(),
                "# Mode and Role Instructions".to_string(),
                "# Skill Instructions".to_string(),
                "# Workspace Context".to_string(),
            ]
        );
    }
}
