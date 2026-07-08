use std::collections::HashMap;
use std::path::Path;

use pl_model::ModelInfo;
use pl_protocol::{Message, MessageContent, MessageRole, Result};
use serde::{Deserialize, Serialize};

use crate::config::{InstructionsConfig, PureConfig};
use crate::turn::CompileMode;
use crate::workspace::load_workspace_instruction_documents;

const DEFAULT_BASE_INSTRUCTIONS: &str = include_str!("prompts/system.md");
const PLATFORM_COMMON_INSTRUCTIONS: &str = include_str!("prompts/platform/common.md");
#[cfg(windows)]
const PLATFORM_SPECIFIC_INSTRUCTIONS: &str = include_str!("prompts/platform/windows.md");
#[cfg(unix)]
const PLATFORM_SPECIFIC_INSTRUCTIONS: &str = include_str!("prompts/platform/unix.md");
#[cfg(not(any(windows, unix)))]
const PLATFORM_SPECIFIC_INSTRUCTIONS: &str = "";

#[derive(Debug, Clone)]
pub struct InstructionAssemblyRequest<'a> {
    pub config: Option<&'a PureConfig>,
    pub model: &'a ModelInfo,
    pub mode: CompileMode,
    pub workspace_root: &'a Path,
    pub current_dir: &'a Path,
    pub workspace_instructions: Option<&'a str>,
    pub subagent_constraint: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionBundle {
    pub instructions: String,
    pub prelude_messages: Vec<Message>,
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
    Mode,
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
    ProfileWorkspace,
}

/// 初始化阶段传入的提示词配置。
///
/// `InstructionProfile` 用于让不同宿主复用同一个 `pl-core` turn loop，
/// 同时按运行场景注入系统提示词、开发者约束和用户上下文。它不包含具体
/// 执行环境能力；工具和 workspace 行为由 core runtime profile 单独配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstructionProfile {
    base_system_prompt: Option<String>,
    developer_blocks: Vec<InstructionBlock>,
    user_context_blocks: Vec<InstructionBlock>,
    workspace_instructions: Option<String>,
}

impl InstructionProfile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.base_system_prompt = Some(prompt.into());
        self
    }

    pub fn with_developer_block(
        mut self,
        label: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.developer_blocks.push(InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::ProfileDeveloper, label),
            content: content.into(),
        });
        self
    }

    pub fn with_user_context_block(
        mut self,
        label: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.user_context_blocks.push(InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::ProfileUser, label),
            content: content.into(),
        });
        self
    }

    pub fn with_workspace_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.workspace_instructions = Some(instructions.into());
        self
    }

    pub fn workspace_instructions(&self) -> Option<&str> {
        self.workspace_instructions.as_deref()
    }
}

pub struct InstructionAssembler;

impl InstructionAssembler {
    pub fn assemble(request: InstructionAssemblyRequest<'_>) -> Result<InstructionSnapshot> {
        Self::assemble_inner(request, None)
    }

    pub fn assemble_with_profile(
        request: InstructionAssemblyRequest<'_>,
        profile: &InstructionProfile,
    ) -> Result<InstructionSnapshot> {
        Self::assemble_inner(request, Some(profile))
    }

    fn assemble_inner(
        request: InstructionAssemblyRequest<'_>,
        profile: Option<&InstructionProfile>,
    ) -> Result<InstructionSnapshot> {
        let config_instructions = request.config.map(|config| &config.instructions);
        let mut snapshot = InstructionSnapshot {
            base: base_block(profile, config_instructions, request.model),
            developer: Vec::new(),
            user: Vec::new(),
        };

        let mode = request.mode.label();
        snapshot.push_developer(
            InstructionSource::new(InstructionSourceKind::Mode, format!("compile mode: {mode}")),
            request.mode.instructions(),
        );
        snapshot.push_developer(
            InstructionSource::new(InstructionSourceKind::Platform, platform_label()),
            &platform_instructions(),
        );
        if let Some(config) = config_instructions {
            snapshot.push_developer(
                InstructionSource::new(InstructionSourceKind::ConfigDeveloper, "config developer"),
                &config.developer,
            );
        }
        if let Some(profile) = profile {
            for block in &profile.developer_blocks {
                snapshot.push_developer(block.source.clone(), &block.content);
            }
        }
        if let Some(config) = request.config {
            match crate::skill::build_skills_prompt(request.workspace_root, &config.skills) {
                Ok(Some(skills)) => {
                    snapshot.push_developer(
                        InstructionSource::new(InstructionSourceKind::Skills, "skills"),
                        &skills,
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("[pl-core] failed to load skills prompt: {error}");
                }
            }
        }
        if let Some(constraint) = request.subagent_constraint {
            snapshot.push_developer(
                InstructionSource::new(
                    InstructionSourceKind::SubagentConstraint,
                    "subagent dispatch constraint",
                ),
                constraint,
            );
        }

        if let Some(config) = config_instructions {
            snapshot.push_user(
                InstructionSource::new(InstructionSourceKind::ConfigUser, "config user"),
                &config.user,
            );
            add_project_documents(
                &mut snapshot,
                request.workspace_root,
                request.current_dir,
                config,
            )?;
        } else if let Some(instructions) = request.workspace_instructions {
            snapshot.push_user(
                InstructionSource::new(InstructionSourceKind::WorkspaceFallback, "workspace"),
                instructions,
            );
        }
        if let Some(profile) = profile {
            for block in &profile.user_context_blocks {
                snapshot.push_user(block.source.clone(), &block.content);
            }
            if let Some(instructions) = profile.workspace_instructions() {
                snapshot.push_user(
                    InstructionSource::new(InstructionSourceKind::ProfileWorkspace, "workspace"),
                    instructions,
                );
            }
        }

        Ok(snapshot)
    }
}

impl InstructionSnapshot {
    /// 构造由宿主产品层提供的完整 base system prompt 快照。
    ///
    /// 这用于 mai-team 等宿主已经完成 profile 拼装的场景，避免宿主直接依赖
    /// `InstructionBlock` 和 `InstructionSource` 的内部结构。
    pub fn profile_base_override(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            base: InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::ProfileBaseOverride, label),
                content: content.into(),
            },
            developer: Vec::new(),
            user: Vec::new(),
        }
    }

    pub fn with_turn_overlay(
        &self,
        request: InstructionAssemblyRequest<'_>,
    ) -> Result<InstructionSnapshot> {
        let overlay = InstructionAssembler::assemble(request)?;
        let mut snapshot = self.without_turn_overlay();
        snapshot.developer.splice(
            0..0,
            overlay
                .developer
                .into_iter()
                .filter(|block| block.source.kind.is_turn_overlay()),
        );
        Ok(snapshot)
    }

    pub fn to_bundle(&self) -> InstructionBundle {
        let mut prelude_messages = Vec::new();
        if !self.developer.is_empty() {
            prelude_messages.push(text_message(
                MessageRole::System,
                format_blocks("# Developer Instructions", &self.developer),
            ));
        }
        if !self.user.is_empty() {
            prelude_messages.push(text_message(
                MessageRole::User,
                format_blocks("# User Context", &self.user),
            ));
        }
        InstructionBundle {
            instructions: self.base.content.clone(),
            prelude_messages,
        }
    }

    pub fn with_subagent_force(mut self, content: &str) -> Self {
        self.push_developer(
            InstructionSource::new(
                InstructionSourceKind::SubagentForce,
                "subagent force dispatch",
            ),
            content,
        );
        self
    }

    pub fn with_subagent_constraint(mut self, content: &str) -> Self {
        self.push_developer(
            InstructionSource::new(
                InstructionSourceKind::SubagentConstraint,
                "subagent dispatch constraint",
            ),
            content,
        );
        self
    }

    fn without_turn_overlay(&self) -> Self {
        Self {
            base: self.base.clone(),
            developer: self
                .developer
                .iter()
                .filter(|block| !block.source.kind.is_turn_overlay())
                .cloned()
                .collect(),
            user: self.user.clone(),
        }
    }

    fn push_developer(&mut self, source: InstructionSource, content: &str) {
        push_non_empty(&mut self.developer, source, content);
    }

    fn push_user(&mut self, source: InstructionSource, content: &str) {
        push_non_empty(&mut self.user, source, content);
    }
}

impl InstructionSourceKind {
    fn is_turn_overlay(self) -> bool {
        matches!(
            self,
            Self::Mode
                | Self::Platform
                | Self::Skills
                | Self::SubagentConstraint
                | Self::SubagentForce
        )
    }
}

impl InstructionSource {
    fn new(kind: InstructionSourceKind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
            path: None,
        }
    }

    fn path(kind: InstructionSourceKind, label: impl Into<String>, path: &Path) -> Self {
        Self {
            kind,
            label: label.into(),
            path: Some(path.display().to_string()),
        }
    }
}

fn base_block(
    profile: Option<&InstructionProfile>,
    config: Option<&InstructionsConfig>,
    model: &ModelInfo,
) -> InstructionBlock {
    if let Some(base_override) = profile
        .and_then(|profile| profile.base_system_prompt.as_deref())
        .map(str::trim)
        .filter(|content| !content.is_empty())
    {
        return InstructionBlock {
            source: InstructionSource::new(
                InstructionSourceKind::ProfileBaseOverride,
                "profile base override",
            ),
            content: base_override.to_string(),
        };
    }
    if let Some(base_override) = config
        .map(|config| config.base_override.trim())
        .filter(|content| !content.is_empty())
    {
        return InstructionBlock {
            source: InstructionSource::new(
                InstructionSourceKind::ConfigBaseOverride,
                "config base override",
            ),
            content: base_override.to_string(),
        };
    }
    if !model.base_instructions.trim().is_empty() {
        return InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::ModelBase, "model base"),
            content: model.base_instructions.trim().to_string(),
        };
    }
    InstructionBlock {
        source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "built-in base"),
        content: DEFAULT_BASE_INSTRUCTIONS.trim().to_string(),
    }
}

fn platform_label() -> &'static str {
    #[cfg(windows)]
    {
        "platform: windows"
    }
    #[cfg(unix)]
    {
        "platform: unix"
    }
    #[cfg(not(any(windows, unix)))]
    {
        "platform"
    }
}

fn platform_instructions() -> String {
    format!(
        "{}\n\n{}",
        PLATFORM_COMMON_INSTRUCTIONS.trim(),
        PLATFORM_SPECIFIC_INSTRUCTIONS.trim()
    )
    .trim()
    .to_string()
}

fn add_project_documents(
    snapshot: &mut InstructionSnapshot,
    workspace_root: &Path,
    current_dir: &Path,
    config: &InstructionsConfig,
) -> Result<()> {
    let documents = load_workspace_instruction_documents(
        workspace_root,
        current_dir,
        config.project_doc_max_bytes,
        &config.project_doc_fallback_filenames,
    )?;
    for document in documents.documents {
        let label = document
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "project document".to_string());
        snapshot.push_user(
            InstructionSource::path(InstructionSourceKind::ProjectDoc, label, &document.path),
            &document.content,
        );
    }
    Ok(())
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

fn format_blocks(title: &str, blocks: &[InstructionBlock]) -> String {
    let mut output = String::from(title);
    for block in blocks {
        output.push_str("\n\n## ");
        output.push_str(&block.source.label);
        if let Some(path) = &block.source.path {
            output.push_str("\nSource: ");
            output.push_str(path);
        }
        output.push_str("\n\n");
        output.push_str(&block.content);
    }
    output
}

fn text_message(role: MessageRole, content: String) -> Message {
    Message {
        role,
        content: MessageContent::Text(content),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use pl_model::ModelInfo;
    use pretty_assertions::assert_eq;

    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pure-instruction-{name}-{stamp}"))
    }

    #[test]
    fn profile_base_override_snapshot_constructs_host_instruction_block() {
        let snapshot =
            InstructionSnapshot::profile_base_override("mai-team instructions", "host prompt");

        assert_eq!(
            snapshot,
            InstructionSnapshot {
                base: InstructionBlock {
                    source: InstructionSource {
                        kind: InstructionSourceKind::ProfileBaseOverride,
                        label: "mai-team instructions".to_string(),
                        path: None,
                    },
                    content: "host prompt".to_string(),
                },
                developer: Vec::new(),
                user: Vec::new(),
            }
        );
    }

    #[test]
    fn assembles_three_layers_in_stable_order() {
        let dir = temp_dir("order");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("AGENTS.md"), "project rules").unwrap();
        let mut config = PureConfig::default();
        config.instructions.developer = "config dev".to_string();
        config.instructions.user = "config user".to_string();

        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: Some(&config),
            model: &ModelInfo::fallback("test-model"),
            mode: CompileMode::Plan,
            workspace_root: &dir,
            current_dir: &dir,
            workspace_instructions: None,
            subagent_constraint: Some("subagent rule"),
        })
        .unwrap();

        assert_eq!(
            snapshot
                .developer
                .iter()
                .map(|block| block.source.kind)
                .collect::<Vec<_>>(),
            vec![
                InstructionSourceKind::Mode,
                InstructionSourceKind::Platform,
                InstructionSourceKind::ConfigDeveloper,
                InstructionSourceKind::Skills,
                InstructionSourceKind::SubagentConstraint
            ]
        );
        assert_eq!(
            snapshot
                .user
                .iter()
                .map(|block| block.source.kind)
                .collect::<Vec<_>>(),
            vec![
                InstructionSourceKind::ConfigUser,
                InstructionSourceKind::ProjectDoc
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn platform_block_is_after_mode_and_before_config_developer() {
        let dir = temp_dir("platform-order");
        fs::create_dir_all(&dir).unwrap();
        let mut config = PureConfig::default();
        config.instructions.developer = "config dev".to_string();

        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: Some(&config),
            model: &ModelInfo::fallback("test-model"),
            mode: CompileMode::Auto,
            workspace_root: &dir,
            current_dir: &dir,
            workspace_instructions: None,
            subagent_constraint: None,
        })
        .unwrap();

        assert_eq!(
            snapshot.developer[0].source.kind,
            InstructionSourceKind::Mode
        );
        assert_eq!(
            snapshot.developer[1].source.kind,
            InstructionSourceKind::Platform
        );
        assert_eq!(
            snapshot.developer[2].source.kind,
            InstructionSourceKind::ConfigDeveloper
        );
        assert!(snapshot.developer[1].content.contains("workspace root"));
        if cfg!(windows) {
            assert_eq!(snapshot.developer[1].source.label, "platform: windows");
            assert!(
                snapshot.developer[1]
                    .content
                    .contains("Current platform: windows.")
            );
            assert!(
                !snapshot.developer[1]
                    .content
                    .contains("Current platform: unix.")
            );
        }
        if cfg!(unix) {
            assert_eq!(snapshot.developer[1].source.label, "platform: unix");
            assert!(
                snapshot.developer[1]
                    .content
                    .contains("Current platform: unix.")
            );
            assert!(
                !snapshot.developer[1]
                    .content
                    .contains("Current platform: windows.")
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn filters_empty_blocks_and_uses_model_base() {
        let dir = temp_dir("empty");
        fs::create_dir_all(&dir).unwrap();
        let mut model = ModelInfo::fallback("test-model");
        model.base_instructions = "model base".to_string();

        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: None,
            model: &model,
            mode: CompileMode::Auto,
            workspace_root: &dir,
            current_dir: &dir,
            workspace_instructions: Some(""),
            subagent_constraint: None,
        })
        .unwrap();

        assert_eq!(snapshot.base.content, "model base");
        assert_eq!(snapshot.user, Vec::new());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn profile_can_override_base_and_add_context_blocks() {
        let dir = temp_dir("profile");
        fs::create_dir_all(&dir).unwrap();
        let profile = InstructionProfile::new()
            .with_base_system_prompt("profile base")
            .with_developer_block("runtime", "profile developer")
            .with_user_context_block("host", "profile user")
            .with_workspace_instructions("profile workspace");

        let snapshot = InstructionAssembler::assemble_with_profile(
            InstructionAssemblyRequest {
                config: None,
                model: &ModelInfo::fallback("test-model"),
                mode: CompileMode::Auto,
                workspace_root: &dir,
                current_dir: &dir,
                workspace_instructions: None,
                subagent_constraint: None,
            },
            &profile,
        )
        .unwrap();
        let bundle = snapshot.to_bundle();

        assert_eq!(snapshot.base.content, "profile base");
        assert_eq!(
            snapshot.base.source.kind,
            InstructionSourceKind::ProfileBaseOverride
        );
        assert!(bundle.prelude_messages.iter().any(|message| {
            matches!(
                &message.content,
                MessageContent::Text(text) if text.contains("profile developer")
            )
        }));
        assert!(bundle.prelude_messages.iter().any(|message| {
            matches!(
                &message.content,
                MessageContent::Text(text) if text.contains("profile user")
            )
        }));
        assert_eq!(
            snapshot
                .user
                .iter()
                .map(|block| block.source.kind)
                .collect::<Vec<_>>(),
            vec![
                InstructionSourceKind::ProfileUser,
                InstructionSourceKind::ProfileWorkspace
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bundle_maps_developer_to_system_and_user_context_to_user() {
        let snapshot = InstructionSnapshot {
            base: InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
                content: "base".to_string(),
            },
            developer: vec![InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::Mode, "mode"),
                content: "dev".to_string(),
            }],
            user: vec![InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::ConfigUser, "user"),
                content: "ctx".to_string(),
            }],
        };

        let bundle = snapshot.to_bundle();

        assert_eq!(bundle.instructions, "base");
        assert_eq!(bundle.prelude_messages.len(), 2);
        assert_eq!(bundle.prelude_messages[0].role, MessageRole::System);
        assert_eq!(bundle.prelude_messages[1].role, MessageRole::User);
    }

    #[test]
    fn config_base_override_replaces_model_base() {
        let dir = temp_dir("base-override");
        fs::create_dir_all(&dir).unwrap();
        let mut config = PureConfig::default();
        config.instructions.base_override = "config base".to_string();
        let mut model = ModelInfo::fallback("test-model");
        model.base_instructions = "model base".to_string();

        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: Some(&config),
            model: &model,
            mode: CompileMode::Auto,
            workspace_root: &dir,
            current_dir: &dir,
            workspace_instructions: None,
            subagent_constraint: None,
        })
        .unwrap();

        assert_eq!(snapshot.base.content, "config base");
        assert_eq!(
            snapshot.base.source.kind,
            InstructionSourceKind::ConfigBaseOverride
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn built_in_base_requires_doc_first_and_final_review() {
        let dir = temp_dir("built-in-base-doc-flow");
        fs::create_dir_all(&dir).unwrap();

        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: None,
            model: &ModelInfo::fallback("test-model"),
            mode: CompileMode::Auto,
            workspace_root: &dir,
            current_dir: &dir,
            workspace_instructions: None,
            subagent_constraint: None,
        })
        .unwrap();

        assert_eq!(
            snapshot.base.source.kind,
            InstructionSourceKind::BuiltInBase
        );
        assert!(snapshot.base.content.contains("再开始写代码"));
        assert!(snapshot.base.content.contains("整体回看计划"));
        assert!(snapshot.base.content.contains("首次调用工具前必须输出一句"));
        assert!(snapshot.base.content.contains("每次 commentary 只写 1 句"));
        assert!(snapshot.base.content.contains("final 只出现一次"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn force_dispatch_is_added_to_clone_only() {
        let snapshot = InstructionSnapshot {
            base: InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
                content: "base".to_string(),
            },
            developer: Vec::new(),
            user: Vec::new(),
        };

        let forced = snapshot.clone().with_subagent_force("force");

        assert_eq!(snapshot.developer.len(), 0);
        assert_eq!(forced.developer.len(), 1);
        assert_eq!(
            forced.developer[0].source.kind,
            InstructionSourceKind::SubagentForce
        );
    }

    #[test]
    fn subagent_constraint_is_added_to_clone_only() {
        let snapshot = InstructionSnapshot {
            base: InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "base"),
                content: "base".to_string(),
            },
            developer: Vec::new(),
            user: Vec::new(),
        };

        let constrained = snapshot.clone().with_subagent_constraint("constraint");

        assert_eq!(snapshot.developer.len(), 0);
        assert_eq!(constrained.developer.len(), 1);
        assert_eq!(
            constrained.developer[0].source.kind,
            InstructionSourceKind::SubagentConstraint
        );
    }
}
