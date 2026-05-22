use pl_model::TokenUsage;

/// 编译请求的执行模式。
///
/// `Plan` 只要求核心流程产出规划与解释；`Auto` 允许模型生成更主动的
/// 编译步骤，但当前版本不会执行命令、写文件或调用沙箱。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    #[default]
    Plan,
    Auto,
}

impl CompileMode {
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Plan => include_str!("../prompts/plan.md"),
            Self::Auto => include_str!("../prompts/auto.md"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Auto => "auto",
        }
    }
}

/// 单轮核心编译请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequest {
    pub prompt: String,
    pub mode: CompileMode,
    pub workspace_instructions: Option<String>,
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>, mode: CompileMode) -> Self {
        Self {
            prompt: prompt.into(),
            mode,
            workspace_instructions: None,
        }
    }

    pub fn with_workspace_instructions(mut self, instructions: String) -> Self {
        self.workspace_instructions = Some(instructions);
        self
    }
}

/// 单轮核心编译结果。
#[derive(Debug, Clone)]
pub struct TurnResult {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub model: String,
    pub usage: TokenUsage,
    pub mode: CompileMode,
    pub session_message_count: usize,
}
