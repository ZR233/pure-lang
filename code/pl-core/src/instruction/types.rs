use std::collections::BTreeMap;
use std::path::Path;

use pl_model::ModelInfo;
use pl_protocol::Message;
use serde::{Deserialize, Serialize};

use crate::config::{InstructionsConfig, SkillsConfig};

#[derive(Debug, Clone)]
pub struct InstructionAssemblyRequest<'a> {
    pub instructions: Option<&'a InstructionsConfig>,
    pub skills: Option<&'a SkillsConfig>,
    pub skill_catalog: Option<&'a crate::skill::SkillCatalog>,
    pub execution_profile: Option<ExecutionInstructionProfile<'a>>,
    pub model: &'a ModelInfo,
    pub workspace_root: &'a Path,
    pub current_dir: &'a Path,
    pub workspace_instructions: Option<&'a str>,
    pub subagent_constraint: Option<&'a str>,
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
    SubagentConstraint,
    SubagentForce,
    ConfigUser,
    ProfileUser,
    ProjectDoc,
    WorkspaceFallback,
    SkillInvocation,
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
                | Self::SubagentConstraint
                | Self::SubagentForce
                | Self::SkillInvocation
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
