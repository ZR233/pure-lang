use super::{InstructionBlock, InstructionSource, InstructionSourceKind};

/// 初始化阶段传入的提示词配置。
///
/// `InstructionProfile` 用于让不同宿主复用同一个 `pl-core` turn loop，
/// 同时按运行场景注入系统提示词、开发者约束和用户上下文。它不包含具体
/// 执行环境能力；工具和 workspace 行为由 core runtime profile 单独配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstructionProfile {
    pub(super) base_system_prompt: Option<String>,
    pub(super) developer_blocks: Vec<InstructionBlock>,
    pub(super) user_context_blocks: Vec<InstructionBlock>,
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
}
